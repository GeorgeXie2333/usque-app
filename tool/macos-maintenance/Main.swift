import Cocoa
import CryptoKit
import Darwin
import Foundation

private struct Request: Decodable {
  let operation: String
  let root: String
  let package: String?
  let expected: MacExpectedPackage?
}

@main
enum MacMaintenanceMain {
  static func main() {
    do {
      if CommandLine.arguments == [CommandLine.arguments[0], "--status"] {
        print("{\"schema_version\":1,\"production_capable\":\(!MacReleaseTrust.allowedTeamIDs.isEmpty),\"trust_configured\":\(!MacReleaseTrust.allowedTeamIDs.isEmpty),\"extension_upgrade\":\"separate_approval_required\"}")
        return
      }
      // In particular, do not create staging or mount a disk image until the
      // binary itself contains a configured release signing authority.
      try MacReleaseTrust.requireConfigured()
      let bytes = try FileHandle.standardInput.read(upToCount: 65_537) ?? Data()
      guard bytes.count <= 65_536 else { throw MacMaintenanceError.invalidMetadata }
      let request = try JSONDecoder().decode(Request.self, from: bytes)
      let root = URL(fileURLWithPath: request.root, isDirectory: true).standardizedFileURL
      let userApplications = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Applications", isDirectory: true)
      guard [URL(fileURLWithPath: "/Applications", isDirectory: true), userApplications].contains(root),
        root.resolvingSymlinksInPath() == root else { throw MacMaintenanceError.invalidInstallation }
      let installed = root.appendingPathComponent("Usque.app")
      if NSRunningApplication.runningApplications(withBundleIdentifier: MacReleaseTrust.identifier).contains(where: {
        $0.bundleURL?.standardizedFileURL == installed && !$0.isTerminated
      }) { throw MacMaintenanceError.applicationRunning }
      let architecture = try macUpdateArchitecture()
      let verifier = NativeMacBundleVerifier()
      let transaction = try MacAppTransaction(root: root, verifier: verifier)
      if request.operation == "rollback" {
        // Recovery compares both recorded bundle hashes before trusting and
        // verifying the previous version. Unknown changes are preserved.
        _ = try transaction.recover(rollback: true)
      } else {
        let current = try MacBundleIdentity.read(installed)
        let previousArchitecture = try verifier.architecture(installed)
        try verifier.verify(installed, version: current.version, architecture: previousArchitecture)
        guard ["verify", "install"].contains(request.operation), let package = request.package,
          let expected = request.expected, expected.variant == architecture else { throw MacMaintenanceError.invalidMetadata }
        try expected.validate(installedVersion: current.version)
        let image = URL(fileURLWithPath: package).standardizedFileURL
        try verifyPackage(image, expected: expected)
        let mount = FileManager.default.temporaryDirectory.appendingPathComponent("usque-update-\(UUID())", isDirectory: true)
        try FileManager.default.createDirectory(at: mount, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: mount) }
        _ = try nativeOutput("/usr/bin/hdiutil", ["attach", "-readonly", "-nobrowse", "-noautoopen", "-mountpoint", mount.path, "-plist", image.path], maximum: 1024 * 1024)
        defer { try? nativeCommand("/usr/bin/hdiutil", ["detach", mount.path]) }
        let candidate = mount.appendingPathComponent("Usque.app")
        try verifier.verify(candidate, version: expected.version, architecture: expected.variant)
        if request.operation == "install" {
          _ = try transaction.install(candidate: candidate, version: expected.version, architecture: expected.variant)
        }
      }
      print("{\"schema_version\":1,\"success\":true,\"backup_retained\":true,\"extension_upgrade\":\"separate_approval_required\"}")
    } catch let error as MacMaintenanceError {
      fputs(error.rawValue + "\n", stderr)
      exit(1)
    } catch {
      fputs(MacMaintenanceError.systemFailure.rawValue + "\n", stderr)
      exit(1)
    }
  }

  private static func verifyPackage(_ image: URL, expected: MacExpectedPackage) throws {
    guard image.path.hasPrefix("/"), image.lastPathComponent == expected.name,
      image.resolvingSymlinksInPath() == image else { throw MacMaintenanceError.invalidPackage }
    let fd = open(image.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
    guard fd >= 0 else { throw MacMaintenanceError.invalidPackage }
    let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
    defer { try? file.close() }
    var attributes = stat()
    guard fstat(fd, &attributes) == 0, attributes.st_mode & S_IFMT == S_IFREG,
      attributes.st_size == expected.size else { throw MacMaintenanceError.invalidPackage }
    var hash = SHA256()
    var size: UInt64 = 0
    while let bytes = try file.read(upToCount: 1024 * 1024), !bytes.isEmpty {
      size += UInt64(bytes.count)
      guard size <= expected.size else { throw MacMaintenanceError.invalidPackage }
      hash.update(data: bytes)
    }
    guard size == expected.size,
      hash.finalize().map({ String(format: "%02x", $0) }).joined() == expected.sha256 else { throw MacMaintenanceError.invalidPackage }
    try nativeCommand("/usr/sbin/spctl", ["--assess", "--type", "open", "--context", "context:primary-signature", image.path])
  }
}
