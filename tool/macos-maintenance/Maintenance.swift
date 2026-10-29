import Cocoa
import CryptoKit
import Darwin
import Foundation
import Security

enum MacMaintenanceError: String, Error {
  case trustNotConfigured = "MACOS_UPDATE_TRUST_NOT_CONFIGURED"
  case invalidMetadata = "MACOS_UPDATE_INVALID_METADATA"
  case invalidPackage = "MACOS_UPDATE_INVALID_PACKAGE"
  case invalidSignature = "MACOS_UPDATE_INVALID_SIGNATURE"
  case invalidInstallation = "MACOS_UPDATE_INVALID_INSTALLATION"
  case applicationRunning = "MACOS_UPDATE_APPLICATION_RUNNING"
  case recoveryRequired = "MACOS_UPDATE_RECOVERY_REQUIRED"
  case systemFailure = "MACOS_UPDATE_SYSTEM_FAILURE"
}

/// This is deliberately a compile-time policy. An environment variable,
/// command-line argument or editable bundle plist cannot enable installation.
enum MacReleaseTrust {
  static let allowedTeamIDs: Set<String> = []
  static let identifier = "io.github.georgexie2333.usque"
  static func requireConfigured() throws {
    guard !allowedTeamIDs.isEmpty else { throw MacMaintenanceError.trustNotConfigured }
  }
}

struct MacReleaseVersion: Comparable {
  let parts: [UInt64]
  init(_ text: String) throws {
    let pieces = text.split(separator: ".", omittingEmptySubsequences: false)
    guard pieces.count == 3 else { throw MacMaintenanceError.invalidMetadata }
    parts = try pieces.map {
      guard !$0.isEmpty, ($0.count == 1 || !$0.hasPrefix("0")),
        $0.utf8.allSatisfy({ (48...57).contains($0) }), let part = UInt64($0)
      else { throw MacMaintenanceError.invalidMetadata }
      return part
    }
  }
  static func < (left: Self, right: Self) -> Bool { left.parts.lexicographicallyPrecedes(right.parts) }
}

/// Matches the existing Windows helper's exact-name/size/SHA-256 metadata
/// contract. Discovery metadata alone never authorizes a macOS installation.
struct MacExpectedPackage: Codable {
  let version: String
  let variant: String
  let name: String
  let size: UInt64
  let sha256: String

  func validate(installedVersion: String) throws {
    guard try MacReleaseVersion(version) > MacReleaseVersion(installedVersion),
      ["arm64", "x86_64"].contains(variant),
      name == "usque-v\(version)-macos-\(variant).dmg",
      size > 0, size <= 512 * 1024 * 1024,
      sha256.count == 64, sha256.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) })
    else { throw MacMaintenanceError.invalidMetadata }
  }
}

protocol MacBundleVerifier {
  func architecture(_ app: URL) throws -> String
  func verify(_ app: URL, version: String, architecture: String) throws
}

struct NativeMacBundleVerifier: MacBundleVerifier {
  func architecture(_ app: URL) throws -> String {
    guard let executable = Bundle(url: app)?.executableURL,
      executable.resolvingSymlinksInPath().path.hasPrefix(app.resolvingSymlinksInPath().path + "/Contents/MacOS/")
    else { throw MacMaintenanceError.invalidPackage }
    let value = try nativeOutput("/usr/bin/lipo", ["-archs", executable.path], maximum: 128).trimmingCharacters(in: .whitespacesAndNewlines)
    guard ["arm64", "x86_64"].contains(value) else { throw MacMaintenanceError.invalidPackage }
    return value
  }

  func verify(_ app: URL, version: String, architecture: String) throws {
    try MacReleaseTrust.requireConfigured()
    let info = try MacBundleIdentity.read(app)
    guard info.identifier == MacReleaseTrust.identifier, info.version == version,
      ["arm64", "x86_64"].contains(architecture)
    else { throw MacMaintenanceError.invalidPackage }
    var code: SecStaticCode?
    guard SecStaticCodeCreateWithPath(app as CFURL, SecCSFlags(), &code) == errSecSuccess, let code else {
      throw MacMaintenanceError.invalidSignature
    }
    var information: CFDictionary?
    guard SecCodeCopySigningInformation(code, SecCSFlags(rawValue: kSecCSSigningInformation), &information) == errSecSuccess,
      let fields = information as? [String: Any], let team = fields[kSecCodeInfoTeamIdentifier as String] as? String,
      MacReleaseTrust.allowedTeamIDs.contains(team)
    else { throw MacMaintenanceError.invalidSignature }
    var requirement: SecRequirement?
    let text = "anchor apple generic and identifier \"\(MacReleaseTrust.identifier)\" and certificate leaf[subject.OU] = \"\(team)\""
    guard SecRequirementCreateWithString(text as CFString, SecCSFlags(), &requirement) == errSecSuccess,
      SecStaticCodeCheckValidity(code, SecCSFlags(rawValue: kSecCSStrictValidate | kSecCSCheckNestedCode | kSecCSCheckAllArchitectures), requirement) == errSecSuccess
    else { throw MacMaintenanceError.invalidSignature }
    // Gatekeeper assessment checks notarization in addition to the signature.
    try nativeCommand("/usr/sbin/spctl", ["--assess", "--type", "execute", "--verbose=2", app.path])
    for file in try MacBundleIdentity.entries(app) where file.kind == "file" {
      let handle = try FileHandle(forReadingFrom: file.url)
      let magic = try handle.read(upToCount: 4) ?? Data()
      try handle.close()
      if [Data([0xcf, 0xfa, 0xed, 0xfe]), Data([0xfe, 0xed, 0xfa, 0xcf]), Data([0xca, 0xfe, 0xba, 0xbe]), Data([0xca, 0xfe, 0xba, 0xbf])].contains(magic) {
        let arch = try nativeOutput("/usr/bin/lipo", ["-archs", file.url.path], maximum: 128).trimmingCharacters(in: .whitespacesAndNewlines)
        guard arch == architecture else { throw MacMaintenanceError.invalidPackage }
      }
    }
  }
}

struct MacBundleIdentity {
  let identifier: String
  let version: String
  static func read(_ app: URL) throws -> Self {
    guard app.pathExtension == "app", app.resolvingSymlinksInPath() == app.standardizedFileURL else {
      throw MacMaintenanceError.invalidInstallation
    }
    let bytes = try boundedRegularFile(app.appendingPathComponent("Contents/Info.plist"), maximum: 1024 * 1024)
    guard let info = try PropertyListSerialization.propertyList(from: bytes, format: nil) as? [String: Any],
      let identifier = info["CFBundleIdentifier"] as? String,
      let version = info["CFBundleShortVersionString"] as? String
    else { throw MacMaintenanceError.invalidInstallation }
    _ = try MacReleaseVersion(version)
    return Self(identifier: identifier, version: version)
  }

  struct Entry { let url: URL; let relative: String; let kind: String }
  static func entries(_ app: URL) throws -> [Entry] {
    guard let resolved = realpath(app.path, nil) else { throw MacMaintenanceError.invalidPackage }
    defer { free(resolved) }
    // Foundation may display /private/var as /var, while its enumerator returns
    // /private/var. Use one POSIX path for both the boundary and relative names.
    let base = String(cString:resolved)
    let app = URL(fileURLWithPath:base,isDirectory:true)
    guard let iterator = FileManager.default.enumerator(at: app, includingPropertiesForKeys: [.isRegularFileKey, .isDirectoryKey, .isSymbolicLinkKey], options: []) else {
      throw MacMaintenanceError.invalidPackage
    }
    var entries: [Entry] = []
    for case let url as URL in iterator {
      let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isDirectoryKey, .isSymbolicLinkKey])
      let kind: String
      if values.isSymbolicLink == true {
        guard let target = realpath(url.path,nil) else { throw MacMaintenanceError.invalidPackage }
        defer { free(target) }
        guard String(cString:target).hasPrefix(base + "/") else { throw MacMaintenanceError.invalidPackage }
        kind = "link"
      } else if values.isRegularFile == true { kind = "file" }
      else if values.isDirectory == true { kind = "directory" }
      else { throw MacMaintenanceError.invalidPackage }
      guard url.path.hasPrefix(base + "/") else { throw MacMaintenanceError.invalidPackage }
      entries.append(Entry(url: url, relative: String(url.path.dropFirst(base.count + 1)), kind: kind))
      guard entries.count <= 100_000 else { throw MacMaintenanceError.invalidPackage }
    }
    return entries.sorted { $0.relative < $1.relative }
  }

  static func digest(_ app: URL) throws -> String {
    var hash = SHA256()
    var total: UInt64 = 0
    for entry in try entries(app) {
      var content = ""
      if entry.kind == "link" {
        content = try FileManager.default.destinationOfSymbolicLink(atPath: entry.url.path)
      } else if entry.kind == "file" {
        let file = try FileHandle(forReadingFrom: entry.url)
        defer { try? file.close() }
        var fileHash = SHA256()
        while let chunk = try file.read(upToCount: 1024 * 1024), !chunk.isEmpty {
          total += UInt64(chunk.count)
          guard total <= 4 * 1024 * 1024 * 1024 else { throw MacMaintenanceError.invalidPackage }
          fileHash.update(data: chunk)
        }
        content = fileHash.finalize().map { String(format: "%02x", $0) }.joined()
      }
      let mode = try FileManager.default.attributesOfItem(atPath: entry.url.path)[.posixPermissions] as? NSNumber
      // A structured, length-delimited record avoids ambiguity between path,
      // link and file bytes when constructing a whole-bundle identity.
      hash.update(data: try JSONSerialization.data(withJSONObject: [entry.kind, entry.relative, content, mode?.stringValue ?? ""]))
    }
    return hash.finalize().map { String(format: "%02x", $0) }.joined()
  }
}

/// A same-volume rename swap keeps the application path continuously present.
/// The prior app stays in private staging until a separate health confirmation;
/// this helper never removes either user data or system extension registrations.
final class MacAppTransaction {
  struct Journal: Codable {
    var schemaVersion = 2
    let identifier: UUID
    let installedName: String
    let previousVersion: String
    let previousArchitecture: String
    let candidateVersion: String
    let architecture: String
    let previousDigest: String
    let candidateDigest: String
    var stage: String
  }
  private let root: URL
  private let verifier: MacBundleVerifier
  private let directory: URL
  private var lock: Int32 = -1

  init(root: URL, verifier: MacBundleVerifier) throws {
    self.root = root.standardizedFileURL
    self.verifier = verifier
    guard self.root == self.root.resolvingSymlinksInPath() else { throw MacMaintenanceError.invalidInstallation }
    directory = self.root.appendingPathComponent(".usque-update", isDirectory: true)
    if !FileManager.default.fileExists(atPath: directory.path) {
      try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    }
    let attributes = try FileManager.default.attributesOfItem(atPath: directory.path)
    guard attributes[.type] as? FileAttributeType == .typeDirectory,
      (attributes[.posixPermissions] as? NSNumber)?.intValue == 0o700,
      (attributes[.ownerAccountID] as? NSNumber)?.uint32Value == geteuid()
    else { throw MacMaintenanceError.invalidInstallation }
    let descriptor = open(directory.appendingPathComponent("lock").path, O_CREAT | O_RDWR | O_NOFOLLOW | O_NONBLOCK, 0o600)
    guard descriptor >= 0 else { throw MacMaintenanceError.recoveryRequired }
    var metadata = stat()
    guard fstat(descriptor, &metadata) == 0, metadata.st_mode & S_IFMT == S_IFREG,
      metadata.st_uid == geteuid(), metadata.st_nlink == 1, metadata.st_mode & 0o777 == 0o600,
      flock(descriptor, LOCK_EX | LOCK_NB) == 0 else {
      close(descriptor)
      throw MacMaintenanceError.recoveryRequired
    }
    lock = descriptor
  }
  deinit { if lock >= 0 { _ = flock(lock, LOCK_UN); close(lock) } }

  private var journalURL: URL { directory.appendingPathComponent("transaction.json") }
  private func backup(_ journal: Journal) -> URL { directory.appendingPathComponent("\(journal.identifier.uuidString).app") }
  private func installed(_ journal: Journal) throws -> URL {
    guard journal.schemaVersion == 2, journal.installedName == "Usque.app",
      ["prepared", "awaiting_health", "confirmed", "rolled_back"].contains(journal.stage),
      ["arm64", "x86_64"].contains(journal.architecture),
      ["arm64", "x86_64"].contains(journal.previousArchitecture),
      [journal.previousDigest, journal.candidateDigest].allSatisfy({ $0.count == 64 && $0.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }) })
    else { throw MacMaintenanceError.recoveryRequired }
    return root.appendingPathComponent(journal.installedName)
  }
  private func write(_ journal: Journal) throws {
    let bytes = try JSONEncoder().encode(journal)
    guard bytes.count <= 65_536 else { throw MacMaintenanceError.recoveryRequired }
    let temporary = directory.appendingPathComponent(".transaction-\(UUID()).json")
    let file = open(temporary.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_NONBLOCK, 0o600)
    guard file >= 0 else { throw MacMaintenanceError.systemFailure }
    let output = FileHandle(fileDescriptor: file, closeOnDealloc: false)
    defer { try? output.close(); _ = unlink(temporary.path) }
    try output.write(contentsOf: bytes)
    guard fsync(file) == 0 else { throw MacMaintenanceError.systemFailure }
    guard rename(temporary.path, journalURL.path) == 0 else { throw MacMaintenanceError.systemFailure }
    let parent = open(directory.path, O_RDONLY | O_DIRECTORY)
    guard parent >= 0 else { throw MacMaintenanceError.systemFailure }
    defer { close(parent) }
    guard fsync(parent) == 0 else { throw MacMaintenanceError.systemFailure }
  }
  private func read() throws -> Journal? {
    var metadata = stat()
    if lstat(journalURL.path, &metadata) != 0 {
      if errno == ENOENT { return nil }
      throw MacMaintenanceError.recoveryRequired
    }
    guard metadata.st_mode & S_IFMT == S_IFREG, metadata.st_uid == geteuid(), metadata.st_nlink == 1,
      metadata.st_mode & 0o777 == 0o600 else { throw MacMaintenanceError.recoveryRequired }
    do {
      let bytes = try boundedRegularFile(journalURL, maximum: 65_536, expectedOwner: geteuid(), privateMode: true)
      return try JSONDecoder().decode(Journal.self, from: bytes)
    } catch { throw MacMaintenanceError.recoveryRequired }
  }
  private func swap(_ left: URL, _ right: URL) throws {
    guard renameatx_np(AT_FDCWD, left.path, AT_FDCWD, right.path, UInt32(RENAME_SWAP)) == 0 else {
      throw MacMaintenanceError.systemFailure
    }
    for path in [root, directory] {
      let fd = open(path.path, O_RDONLY | O_DIRECTORY)
      guard fd >= 0 else { throw MacMaintenanceError.systemFailure }
      defer { close(fd) }
      guard fsync(fd) == 0 else { throw MacMaintenanceError.systemFailure }
    }
  }

  func install(candidate: URL, version: String, architecture: String) throws -> Journal {
    if let old = try read(), !["confirmed", "rolled_back"].contains(old.stage) { throw MacMaintenanceError.recoveryRequired }
    let app = root.appendingPathComponent("Usque.app")
    let previous = try MacBundleIdentity.read(app)
    let previousArchitecture = try verifier.architecture(app)
    guard previous.identifier == MacReleaseTrust.identifier,
      try MacReleaseVersion(version) > MacReleaseVersion(previous.version) else { throw MacMaintenanceError.invalidMetadata }
    try verifier.verify(app, version: previous.version, architecture: previousArchitecture)
    try verifier.verify(candidate, version: version, architecture: architecture)
    var journal = Journal(identifier: UUID(), installedName: "Usque.app", previousVersion: previous.version, previousArchitecture: previousArchitecture,
      candidateVersion: version, architecture: architecture, previousDigest: try MacBundleIdentity.digest(app),
      candidateDigest: try MacBundleIdentity.digest(candidate), stage: "prepared")
    let staged = backup(journal)
    try FileManager.default.copyItem(at: candidate, to: staged)
    try verifier.verify(staged, version: version, architecture: architecture)
    guard try MacBundleIdentity.digest(staged) == journal.candidateDigest,
      try MacBundleIdentity.digest(app) == journal.previousDigest else { throw MacMaintenanceError.invalidPackage }
    try synchronizeBundle(staged)
    try write(journal)
    try swap(app, staged)
    journal.stage = "awaiting_health"
    try write(journal)
    return journal
  }

  @discardableResult
  func recover(rollback: Bool = false) throws -> Journal? {
    guard var journal = try read() else { return nil }
    let app = try installed(journal)
    let installedDigest = try MacBundleIdentity.digest(app)
    let backupDigest = try MacBundleIdentity.digest(backup(journal))
    if installedDigest == journal.previousDigest && backupDigest == journal.candidateDigest {
      journal.stage = "rolled_back"
      try write(journal)
      return journal
    }
    guard installedDigest == journal.candidateDigest, backupDigest == journal.previousDigest else {
      throw MacMaintenanceError.recoveryRequired
    }
    if rollback {
      try verifier.verify(backup(journal), version: journal.previousVersion, architecture: journal.previousArchitecture)
      try swap(app, backup(journal))
      journal.stage = "rolled_back"
    } else if journal.stage != "confirmed" { journal.stage = "awaiting_health" }
    try write(journal)
    return journal
  }

  /// The caller must have independently verified the new GUI, Engine and
  /// extension versions. Backups remain available after this acknowledgement.
  func confirm(identifier: UUID, digest: String, componentsHealthy: Bool) throws {
    guard var journal = try recover(), journal.identifier == identifier,
      journal.stage == "awaiting_health", componentsHealthy,
      journal.candidateDigest == digest else { throw MacMaintenanceError.recoveryRequired }
    journal.stage = "confirmed"
    try write(journal)
  }
}

private func boundedRegularFile(_ url: URL, maximum: Int, expectedOwner: uid_t? = nil, privateMode: Bool = false) throws -> Data {
  let descriptor = open(url.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
  guard descriptor >= 0 else { throw MacMaintenanceError.invalidPackage }
  let file = FileHandle(fileDescriptor: descriptor, closeOnDealloc: false)
  defer { try? file.close() }
  var metadata = stat()
  guard fstat(descriptor, &metadata) == 0, metadata.st_mode & S_IFMT == S_IFREG,
    metadata.st_size >= 0, metadata.st_size <= maximum,
    expectedOwner == nil || metadata.st_uid == expectedOwner,
    !privateMode || (metadata.st_mode & 0o777 == 0o600 && metadata.st_nlink == 1)
  else { throw MacMaintenanceError.invalidPackage }
  var bytes = Data()
  while let chunk = try file.read(upToCount: min(16_384, maximum + 1 - bytes.count)), !chunk.isEmpty {
    bytes.append(chunk)
    guard bytes.count <= maximum else { throw MacMaintenanceError.invalidPackage }
  }
  return bytes
}

private func synchronizeBundle(_ app: URL) throws {
  let entries = try MacBundleIdentity.entries(app)
  for entry in entries where entry.kind == "file" {
    let descriptor = open(entry.url.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
    guard descriptor >= 0 else { throw MacMaintenanceError.invalidPackage }
    defer { close(descriptor) }
    var metadata = stat()
    guard fstat(descriptor, &metadata) == 0, metadata.st_mode & S_IFMT == S_IFREG,
      fsync(descriptor) == 0 else { throw MacMaintenanceError.systemFailure }
  }
  let directories = entries.filter { $0.kind == "directory" }.map { $0.url }.reversed() + [app]
  for path in directories {
    let descriptor = open(path.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
    guard descriptor >= 0 else { throw MacMaintenanceError.invalidPackage }
    defer { close(descriptor) }
    guard fsync(descriptor) == 0 else { throw MacMaintenanceError.systemFailure }
  }
}

/// Physical hardware selection remains arm64 under Rosetta. Older Intel
/// kernels can omit hw.optional.arm64; their CPU type is a safe fallback.
func macUpdateArchitecture(read: (String) -> Int32? = { name in
  var value: Int32 = 0
  var size = MemoryLayout.size(ofValue: value)
  return sysctlbyname(name, &value, &size, nil, 0) == 0 ? value : nil
}) throws -> String {
  if read("sysctl.proc_translated") == 1 { return "arm64" }
  if let arm = read("hw.optional.arm64") {
    if arm == 1 { return "arm64" }
    if arm == 0 { return "x86_64" }
    throw MacMaintenanceError.invalidInstallation
  }
  switch read("hw.cputype") {
  case 7, 0x01000007: return "x86_64"
  case 0x0100000c: return "arm64"
  default: throw MacMaintenanceError.invalidInstallation
  }
}

func nativeOutput(_ executable: String, _ arguments: [String], maximum: Int, timeout: TimeInterval = 120) throws -> String {
  guard maximum > 0, timeout > 0, timeout <= 120 else { throw MacMaintenanceError.systemFailure }
  let process = Process()
  let output = Pipe()
  process.executableURL = URL(fileURLWithPath: executable)
  process.arguments = arguments
  process.environment = ["PATH": "/usr/bin:/bin:/usr/sbin:/sbin"]
  process.standardInput = FileHandle.nullDevice
  process.standardOutput = output
  process.standardError = FileHandle.nullDevice
  try process.run()
  defer {
    try? output.fileHandleForReading.close()
    if process.isRunning {
      process.terminate()
      let identifier = process.processIdentifier
      DispatchQueue.global().asyncAfter(deadline: .now() + 2) {
        if process.isRunning { _ = Darwin.kill(identifier, SIGKILL) }
      }
    }
  }
  let descriptor = output.fileHandleForReading.fileDescriptor
  guard fcntl(descriptor, F_SETFL, O_NONBLOCK) == 0 else { throw MacMaintenanceError.systemFailure }
  let deadline = ProcessInfo.processInfo.systemUptime + timeout
  var bytes = Data()
  var buffer = [UInt8](repeating: 0, count: 16_384)
  var ended = false
  while !ended {
    guard ProcessInfo.processInfo.systemUptime < deadline else { throw MacMaintenanceError.systemFailure }
    var event = pollfd(fd: descriptor, events: Int16(POLLIN | POLLHUP), revents: 0)
    let ready = poll(&event, 1, 100)
    if ready < 0 {
      if errno == EINTR { continue }
      throw MacMaintenanceError.systemFailure
    }
    if ready == 0 { continue }
    guard event.revents & Int16(POLLERR | POLLNVAL) == 0 else { throw MacMaintenanceError.systemFailure }
    let count = Darwin.read(descriptor, &buffer, buffer.count)
    if count < 0 {
      if errno == EINTR || errno == EAGAIN { continue }
      throw MacMaintenanceError.systemFailure
    }
    if count == 0 { ended = true }
    else {
      guard bytes.count + count <= maximum else { throw MacMaintenanceError.systemFailure }
      bytes.append(contentsOf: buffer.prefix(count))
    }
  }
  // A tool can close stdout and continue running. Bound that phase too.
  while process.isRunning {
    guard ProcessInfo.processInfo.systemUptime < deadline else { throw MacMaintenanceError.systemFailure }
    usleep(10_000)
  }
  guard process.terminationStatus == 0, let text = String(data: bytes, encoding: .utf8) else {
    throw MacMaintenanceError.invalidSignature
  }
  return text
}

func nativeCommand(_ executable: String, _ arguments: [String]) throws {
  _ = try nativeOutput(executable, arguments, maximum: 4096)
}
