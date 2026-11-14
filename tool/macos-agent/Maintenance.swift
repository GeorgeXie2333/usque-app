import CryptoKit
import Darwin
import Foundation
import Security

enum AgentMaintenanceError: String, Error {
  case administratorRequired, invalidInstallation, recoveryIncomplete, commandFailed, busy
}
enum AgentOperation: String { case install, repair, recover, uninstall }
protocol AgentMaintenanceBackend {
  func prepare() throws
  func stopAndRecover() throws
  func activate() throws
  func rollback() throws
  func finish() throws
  func uninstall() throws
  func restart() throws
}
func maintainAgent(_ operation: AgentOperation, backend: AgentMaintenanceBackend) throws {
  switch operation {
  case .install, .repair:
    try backend.prepare()
    do {
      try backend.stopAndRecover()
      try backend.activate()
      try backend.finish()
    } catch {
      // A failed network recovery cannot authorize replacement/removal.
      try backend.rollback()
      throw error
    }
  case .recover:
    do { try backend.stopAndRecover(); try backend.restart() }
    catch { try? backend.restart(); throw error }
  case .uninstall:
    do { try backend.stopAndRecover(); try backend.uninstall() }
    catch { try? backend.restart(); throw error }
  }
}

let agentRoot = URL(fileURLWithPath: "/Library/PrivilegedHelperTools/com.usque.agent", isDirectory: true)
let agentState = URL(fileURLWithPath: "/Library/Application Support/Usque/Agent", isDirectory: true)
let daemonPlist = URL(fileURLWithPath: "/Library/LaunchDaemons/com.usque.agent.plist")
private let fm = FileManager.default
private func fail() -> AgentMaintenanceError { .invalidInstallation }

func checkRootPath(_ url: URL, directory: Bool) throws {
  var value = stat()
  guard lstat(url.path, &value) == 0, value.st_uid == 0, value.st_mode & 0o022 == 0,
    value.st_mode & S_IFMT == (directory ? S_IFDIR : S_IFREG) else { throw fail() }
  let acl = acl_get_file(url.path, ACL_TYPE_EXTENDED)
  if let acl = acl {
    defer { acl_free(UnsafeMutableRawPointer(acl)) }
    var entry: acl_entry_t?
    guard acl_get_entry(acl, Int32(ACL_FIRST_ENTRY.rawValue), &entry) != 0 else { throw fail() }
  } else if errno != ENOENT && errno != ENOTSUP { throw fail() }
}
private func ensureRootDirectory(_ url: URL, mode: mode_t) throws {
  if url.path != "/" { try ensureRootDirectory(url.deletingLastPathComponent(), mode: 0o755) }
  let created = mkdir(url.path, mode) == 0
  if !created && errno != EEXIST { throw fail() }
  try checkRootPath(url, directory: true)
  if created { try syncDirectory(url.deletingLastPathComponent()) }
}
private func syncDirectory(_ url: URL) throws {
  let fd = open(url.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
  guard fd >= 0 else { throw fail() }; defer { close(fd) }
  guard fsync(fd) == 0, fcntl(fd, F_FULLFSYNC) == 0 else { throw fail() }
}
private func moveDurably(_ source: URL, _ destination: URL) throws {
  guard rename(source.path, destination.path) == 0 else { throw fail() }
  try syncDirectory(source.deletingLastPathComponent())
  if source.deletingLastPathComponent() != destination.deletingLastPathComponent() { try syncDirectory(destination.deletingLastPathComponent()) }
}
private struct Deletion: Codable { let name: String; let device: Int32; let inode: UInt64 }
private let deletionRecord = agentState.appendingPathComponent("deletion.json")
private func deletionPath(_ name: String) throws -> URL {
  switch name {
  case "installed": return agentRoot
  case "next": return agentRoot.appendingPathExtension("next")
  case "previous": return agentRoot.appendingPathExtension("previous")
  default: throw fail()
  }
}
private func finishDeletion() throws {
  guard fm.fileExists(atPath: deletionRecord.path) else { return }
  try checkRootPath(deletionRecord, directory: false)
  let bytes = try Data(contentsOf: deletionRecord); guard bytes.count <= 4096 else { throw fail() }
  let record = try JSONDecoder().decode(Deletion.self, from: bytes)
  let path = try deletionPath(record.name)
  if fm.fileExists(atPath: path.path) {
    try checkRootPath(path, directory: true)
    var info = stat(); guard lstat(path.path, &info) == 0, info.st_dev == record.device, info.st_ino == record.inode else { throw fail() }
    // The durable directory identity permits completing a partially removed
    // root-owned tree without requiring already-deleted binaries to hash again.
    try fm.removeItem(at: path)
  }
  try syncDirectory(path.deletingLastPathComponent())
  try fm.removeItem(at: deletionRecord); try syncDirectory(agentState)
}
private func removeDurably(_ url: URL) throws {
  guard fm.fileExists(atPath: url.path) else { return }
  var info = stat(); guard lstat(url.path, &info) == 0 else { throw fail() }
  if info.st_mode & S_IFMT == S_IFDIR {
    guard !fm.fileExists(atPath: deletionRecord.path) else { throw AgentMaintenanceError.recoveryIncomplete }
    let name = url == agentRoot ? "installed" : url == agentRoot.appendingPathExtension("next") ? "next" : url == agentRoot.appendingPathExtension("previous") ? "previous" : "invalid"
    _ = try deletionPath(name)
    try checkRootPath(url, directory: true)
    try durable(JSONEncoder().encode(Deletion(name: name, device: info.st_dev, inode: info.st_ino)), at: deletionRecord)
    try finishDeletion()
  } else {
    try fm.removeItem(at: url); try syncDirectory(url.deletingLastPathComponent())
  }
}
private func durable(_ data: Data, at url: URL, mode: mode_t = 0o600) throws {
  try checkRootPath(url.deletingLastPathComponent(), directory: true)
  let temporary = url.deletingLastPathComponent().appendingPathComponent(".write-\(UUID().uuidString)")
  let fd = open(temporary.path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, mode)
  guard fd >= 0 else { throw fail() }
  let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
  defer { try? file.close(); try? fm.removeItem(at: temporary) }
  try file.write(contentsOf: data)
  guard fcntl(fd, F_FULLFSYNC) == 0, rename(temporary.path, url.path) == 0 else { throw fail() }
  try syncDirectory(url.deletingLastPathComponent())
}
private func hash(_ url: URL) throws -> String {
  let fd = open(url.path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK)
  guard fd >= 0 else { throw fail() }
  let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
  defer { try? file.close() }
  var value = stat()
  guard fstat(fd, &value) == 0, value.st_mode & S_IFMT == S_IFREG, value.st_nlink == 1,
    value.st_size > 0, value.st_size <= 512 * 1024 * 1024 else { throw fail() }
  var hash = SHA256()
  while let bytes = try file.read(upToCount: 65536), !bytes.isEmpty { hash.update(data: bytes) }
  return hash.finalize().map { String(format: "%02x", $0) }.joined()
}
private func copyPinned(_ source: URL, to destination: URL) throws {
  // Copy from a pinned source descriptor, then verify/signature-check the root
  // copy. Neither a source symlink nor a later source swap is followed.
  let fd = open(source.path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK)
  guard fd >= 0 else { throw fail() }
  let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
  defer { try? file.close() }
  var value = stat()
  guard fstat(fd, &value) == 0, value.st_mode & S_IFMT == S_IFREG, value.st_size > 0,
    value.st_size <= 512 * 1024 * 1024 else { throw fail() }
  let data = try file.readToEnd() ?? Data()
  guard data.count == value.st_size else { throw fail() }
  try durable(data, at: destination, mode: 0o755)
}
private final class CommandOutput: @unchecked Sendable {
  let lock = NSLock(); var bytes = Data(); var overflow = false
  func append(_ data: Data) { lock.lock(); defer { lock.unlock() }; if bytes.count + data.count > 65536 { overflow = true } else { bytes.append(data) } }
}
@discardableResult
func agentCommand(_ executable: String, _ arguments: [String]) throws -> String {
  let task = Process(); task.executableURL = URL(fileURLWithPath: executable); task.arguments = arguments
  task.environment = ["LC_ALL": "C", "PATH": "/usr/bin:/bin:/usr/sbin:/sbin"]
  task.standardInput = FileHandle.nullDevice
  let pipe = Pipe(); task.standardOutput = pipe; task.standardError = pipe
  let result = CommandOutput(); let group = DispatchGroup()
  try task.run()
  group.enter()
  DispatchQueue.global().async { defer { group.leave() }; while true { let data = pipe.fileHandleForReading.availableData; if data.isEmpty { break }; result.append(data) } }
  let deadline = Date().addingTimeInterval(30)
  while task.isRunning && Date() < deadline { Thread.sleep(forTimeInterval: 0.02) }
  if task.isRunning { kill(task.processIdentifier, SIGKILL) }
  task.waitUntilExit()
  guard group.wait(timeout: .now() + 2) == .success, task.terminationStatus == 0, !result.overflow,
    let text = String(data: result.bytes, encoding: .utf8) else { throw AgentMaintenanceError.commandFailed }
  return text
}
private func verifyCode(_ url: URL, hardened: Bool) throws -> String {
  var code: SecStaticCode?
  guard SecStaticCodeCreateWithPath(url as CFURL, [], &code) == errSecSuccess, let code = code,
    SecStaticCodeCheckValidity(code, SecCSFlags(rawValue: kSecCSStrictValidate), nil) == errSecSuccess else { throw fail() }
  var info: CFDictionary?
  guard SecCodeCopySigningInformation(code, SecCSFlags(rawValue: kSecCSSigningInformation), &info) == errSecSuccess,
    let values = info as? [String: Any], let digest = values[kSecCodeInfoUnique as String] as? Data else { throw fail() }
  if hardened {
    guard let flags = values[kSecCodeInfoFlags as String] as? UInt32, flags & 0x10000 != 0,
      (values[kSecCodeInfoEntitlementsDict as String] as? [String: Any] ?? [:]).isEmpty else { throw fail() }
  }
  return digest.map { String(format: "%02x", $0) }.joined()
}

private struct InstallRecord: Codable {
  let schema: Int; let version: String; let protocolVersion: Int; let architecture: String
  let engineSHA256: String; let engineCDHash: String
  enum CodingKeys: String, CodingKey { case schema, version, architecture; case protocolVersion = "protocol", engineSHA256 = "engine_sha256", engineCDHash = "engine_cdhash" }
}
enum ReplacementPhase: String, Codable { case preparing, prepared, initializing, activated, committed }
struct Replacement: Codable {
  let schema: Int; var phase: ReplacementPhase; var candidate: String?; let previous: String?
  var previousPlist: Data? = nil; var appliedPlist: Data? = nil
  func validate() throws {
    guard schema == 2, phase == .preparing || candidate != nil, (previousPlist?.count ?? 0) <= 65536, (appliedPlist?.count ?? 0) <= 65536 else { throw fail() }
    for identity in [candidate, previous].compactMap({ $0 }) {
      let parts = identity.split(separator: ":")
      guard parts.count == 3, parts.allSatisfy({ $0.count == 64 && $0.allSatisfy({ $0.isHexDigit && !$0.isUppercase }) }) else { throw fail() }
    }
  }
}

final class NativeAgentMaintenance: AgentMaintenanceBackend {
  private let contents: URL
  private let next = agentRoot.appendingPathExtension("next")
  private let previous = agentRoot.appendingPathExtension("previous")
  private let journal = agentState.appendingPathComponent("maintenance.json")
  private var lockFD: Int32 = -1
  init(contents: URL) throws {
    guard geteuid() == 0 else { throw AgentMaintenanceError.administratorRequired }
    self.contents = contents
    try ensureRootDirectory(agentRoot.deletingLastPathComponent(), mode: 0o755)
    try ensureRootDirectory(agentState, mode: 0o700)
    try ensureRootDirectory(daemonPlist.deletingLastPathComponent(), mode: 0o755)
    let lock = agentState.appendingPathComponent("maintenance.lock")
    lockFD = open(lock.path, O_RDWR | O_CREAT | O_NOFOLLOW | O_CLOEXEC, 0o600)
    guard lockFD >= 0 else { throw fail() }
    try checkRootPath(lock, directory: false)
    guard flock(lockFD, LOCK_EX | LOCK_NB) == 0 else { throw AgentMaintenanceError.busy }
    try finishDeletion()
    if fm.fileExists(atPath: journal.path) { try rollback() }
  }
  deinit { if lockFD >= 0 { close(lockFD) } }
  private func identity(_ directory: URL) throws -> String {
    try checkRootPath(directory, directory: true)
    return try ["usque-engine", "usque-agent", "install.json"].map { try hash(directory.appendingPathComponent($0)) }.joined(separator: ":")
  }
  func prepare() throws {
    guard !fm.fileExists(atPath: next.path), !fm.fileExists(atPath: previous.path) else { throw AgentMaintenanceError.recoveryIncomplete }
    var evidence = Replacement(schema: 2, phase: .preparing, candidate: nil, previous: fm.fileExists(atPath: agentRoot.path) ? try identity(agentRoot) : nil)
    if fm.fileExists(atPath: daemonPlist.path) { try checkRootPath(daemonPlist, directory: false); evidence.previousPlist = try Data(contentsOf: daemonPlist) }
    try save(evidence)
    try ensureRootDirectory(next, mode: 0o755)
    do {
      try copyPinned(contents.appendingPathComponent("MacOS/usque-engine"), to: next.appendingPathComponent("usque-engine"))
      try copyPinned(contents.appendingPathComponent("Helpers/usque-agent"), to: next.appendingPathComponent("usque-agent"))
      let engine = next.appendingPathComponent("usque-engine")
      let cdhash = try verifyCode(engine, hardened: true)
      _ = try verifyCode(next.appendingPathComponent("usque-agent"), hardened: true)
      let output = try agentCommand(engine.path, ["--macos-agent-build-info"])
      guard let data = output.data(using: .utf8), let build = try JSONSerialization.jsonObject(with: data) as? [String: Any],
        let version = build["version"] as? String, let architecture = build["architecture"] as? String,
        ["aarch64", "x86_64"].contains(architecture), build["protocol"] as? Int == 3 else { throw fail() }
      let record = InstallRecord(schema: 1, version: version, protocolVersion: 3, architecture: architecture, engineSHA256: try hash(engine), engineCDHash: cdhash)
      try durable(JSONEncoder().encode(record), at: next.appendingPathComponent("install.json"), mode: 0o644)
      evidence.candidate = try identity(next); evidence.phase = .prepared
      try save(evidence)
    } catch { try rollback(); throw error }
  }
  func stopAndRecover() throws {
    if (try? agentCommand("/bin/launchctl", ["print", "system/com.usque.agent"])) != nil {
      try agentCommand("/bin/launchctl", ["bootout", "system/com.usque.agent"])
    }
    if fm.fileExists(atPath: agentRoot.path) {
      try checkRootPath(agentRoot.appendingPathComponent("usque-agent"), directory: false)
      let deadline = Date().addingTimeInterval(15)
      while true {
        do { try agentCommand(agentRoot.appendingPathComponent("usque-agent").path, ["--recover-state"]); break }
        catch { if Date() >= deadline { throw AgentMaintenanceError.recoveryIncomplete }; Thread.sleep(forTimeInterval: 0.1) }
      }
    }
  }
  private func load() throws -> Replacement {
    try checkRootPath(journal, directory: false)
    let data = try Data(contentsOf: journal)
    guard data.count <= 192 * 1024 else { throw fail() }
    let value = try JSONDecoder().decode(Replacement.self, from: data); try value.validate(); return value
  }
  private func save(_ value: Replacement) throws { try value.validate(); let data = try JSONEncoder().encode(value); guard data.count <= 192 * 1024 else { throw fail() }; try durable(data, at: journal) }
  func activate() throws {
    var evidence = try load()
    guard evidence.phase == .prepared, try identity(next) == evidence.candidate else { throw fail() }
    if fm.fileExists(atPath: agentRoot.path) { try moveDurably(agentRoot, previous) }
    try moveDurably(next, agentRoot)
    evidence.phase = .initializing; try save(evidence)
    let agent = agentRoot.appendingPathComponent("usque-agent").path
    try agentCommand(agent, ["--initialize-state"])
    try agentCommand(agent, ["--validate-only"])
    let plist: [String: Any] = ["Label": "com.usque.agent", "ProgramArguments": [agent, "--service"], "RunAtLoad": true, "KeepAlive": true, "UserName": "root", "GroupName": "wheel", "Umask": 0o077, "ProcessType": "Interactive", "ThrottleInterval": 5]
    evidence.appliedPlist = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
    try save(evidence)
    try durable(evidence.appliedPlist!, at: daemonPlist, mode: 0o644)
    try restart()
    evidence.phase = .activated; try save(evidence)
  }
  func restart() throws {
    guard fm.fileExists(atPath: daemonPlist.path), fm.fileExists(atPath: agentRoot.path) else { return }
    try checkRootPath(daemonPlist, directory: false)
    if (try? agentCommand("/bin/launchctl", ["print", "system/com.usque.agent"])) == nil {
      try agentCommand("/bin/launchctl", ["bootstrap", "system", daemonPlist.path])
    }
    let deadline = Date().addingTimeInterval(15)
    while true {
      if (try? agentCommand(agentRoot.appendingPathComponent("usque-agent").path, ["--readiness"])) != nil { break }
      guard Date() < deadline else { throw AgentMaintenanceError.recoveryIncomplete }
      Thread.sleep(forTimeInterval: 0.1)
    }
  }
  func finish() throws {
    var evidence = try load()
    guard [.activated, .committed].contains(evidence.phase), try identity(agentRoot) == evidence.candidate else { throw fail() }
    evidence.phase = .committed; try save(evidence)
    if fm.fileExists(atPath: previous.path) { guard try identity(previous) == evidence.previous else { throw fail() }; try removeDurably(previous) }
    try removeDurably(journal)
  }
  func rollback() throws {
    guard fm.fileExists(atPath: journal.path) else { return }
    let evidence = try load()
    if evidence.phase == .committed { try finish(); return }
    if evidence.phase == .preparing {
      if let old = evidence.previous { guard try identity(agentRoot) == old else { throw fail() } }
      else { guard !fm.fileExists(atPath: agentRoot.path) else { throw fail() } }
      try removeDurably(next); try removeDurably(journal); return
    }
    if [.prepared, .initializing].contains(evidence.phase) && evidence.previous == nil && fm.fileExists(atPath: agentRoot.path) {
      guard try identity(agentRoot) == evidence.candidate else { throw fail() }
      try agentCommand(agentRoot.appendingPathComponent("usque-agent").path, ["--initialize-state"])
    }
    if fm.fileExists(atPath: previous.path) {
      guard try identity(previous) == evidence.previous else { throw fail() }
      if fm.fileExists(atPath: agentRoot.path) {
        guard try identity(agentRoot) == evidence.candidate else { throw fail() }
        try stopAndRecover()
        try removeDurably(agentRoot)
      }
      try moveDurably(previous, agentRoot)
    } else if evidence.previous == nil && fm.fileExists(atPath: agentRoot.path) {
      guard try identity(agentRoot) == evidence.candidate else { throw fail() }
      try stopAndRecover(); try removeDurably(agentRoot)
    } else if let old = evidence.previous { guard try identity(agentRoot) == old else { throw fail() } }
    if fm.fileExists(atPath: next.path) { guard try identity(next) == evidence.candidate else { throw fail() }; try removeDurably(next) }
    let actualPlist: Data?
    if fm.fileExists(atPath: daemonPlist.path) { try checkRootPath(daemonPlist, directory: false); actualPlist = try Data(contentsOf: daemonPlist) } else { actualPlist = nil }
    if actualPlist != evidence.previousPlist {
      guard actualPlist == evidence.appliedPlist else { throw AgentMaintenanceError.recoveryIncomplete }
      if let old = evidence.previousPlist { try durable(old, at: daemonPlist, mode: 0o644) }
      else { try removeDurably(daemonPlist) }
    }
    try restart()
    try removeDurably(journal)
  }
  func uninstall() throws {
    if fm.fileExists(atPath: agentRoot.path) { _ = try identity(agentRoot); try removeDurably(agentRoot) }
    if fm.fileExists(atPath: daemonPlist.path) { try checkRootPath(daemonPlist, directory: false); try removeDurably(daemonPlist) }
    // Clean recovery evidence remains root-owned. User profiles and Keychain
    // records belong to the ordinary user and are never removed here.
  }
}
