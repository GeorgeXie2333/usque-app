import Foundation
import Darwin

private struct FixtureVerifier: MacBundleVerifier {
  var rejectVersion: String?
  func architecture(_ app: URL) throws -> String {
    let fields = try PropertyListSerialization.propertyList(from: Data(contentsOf: app.appendingPathComponent("Contents/Info.plist")), format: nil) as? [String: String]
    guard let value = fields?["UsqueTestArchitecture"] else { throw MacMaintenanceError.invalidPackage }
    return value
  }
  func verify(_ app: URL, version: String, architecture: String) throws {
    let actual = try MacBundleIdentity.read(app)
    guard actual.identifier == MacReleaseTrust.identifier, actual.version == version,
      try self.architecture(app) == architecture, rejectVersion != version else { throw MacMaintenanceError.invalidSignature }
  }
}

@main
enum MaintenanceTests {
  static func main() throws {
    setbuf(stdout, nil)
    for (name, test) in [("metadata", trustAndMetadata), ("replacement", replacementAndRollback),
      ("failed verification", failedVerificationPreservesPrevious), ("tamper", tamperedRecoveryStops),
      ("confirmation", confirmationRetainsBackup), ("Rosetta migration", rosettaMigrationAndRollback),
      ("journal and lock fences", journalAndLockFences), ("bounded commands", boundedCommands),
      ("interrupted swap", interruptedSwapRecovery)] {
      do { try test(); print("Maintenance \(name): passed") }
      catch { print("Maintenance \(name): failed \(error)"); throw error }
    }
    print("9 macOS maintenance contract groups passed (temporary fixtures, injected verifier)")
  }
  private static func require(_ value: @autoclosure () -> Bool) throws {
    if !value() { throw NSError(domain: "MaintenanceTest", code: 1) }
  }
  private static func folder() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("usque-maintenance-contract-\(UUID())")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: false)
    return url
  }
  private static func app(_ url: URL, version: String, architecture: String = "arm64") throws {
    try FileManager.default.createDirectory(at: url.appendingPathComponent("Contents"), withIntermediateDirectories: true)
    let plist = try PropertyListSerialization.data(fromPropertyList: ["CFBundleIdentifier": MacReleaseTrust.identifier, "CFBundleShortVersionString": version, "UsqueTestArchitecture": architecture], format: .xml, options: 0)
    try plist.write(to: url.appendingPathComponent("Contents/Info.plist"))
    try Data(version.utf8).write(to: url.appendingPathComponent("Contents/test-only-payload"))
  }
  private static func trustAndMetadata() throws {
    do { try MacReleaseTrust.requireConfigured(); try require(false) }
    catch MacMaintenanceError.trustNotConfigured {}
    let valid = MacExpectedPackage(version: "1.2.3", variant: "arm64", name: "usque-v1.2.3-macos-arm64.dmg", size: 100, sha256: String(repeating: "a", count: 64))
    try valid.validate(installedVersion: "1.2.2")
    do { try valid.validate(installedVersion: "1.2.3"); try require(false) }
    catch MacMaintenanceError.invalidMetadata {}
    for invalid in ["01.2.3", "1.2", "1.2.3-beta.1", "-1.2.3", "1.2.3.4"] {
      do { _ = try MacReleaseVersion(invalid); try require(false) }
      catch MacMaintenanceError.invalidMetadata {}
    }
  }
  private static func replacementAndRollback() throws {
    let root = try folder()
    defer { try? FileManager.default.removeItem(at: root) }
    let current = root.appendingPathComponent("Usque.app")
    let candidate = root.appendingPathComponent("Candidate.app")
    try app(current, version: "1.0.0"); try app(candidate, version: "1.1.0")
    let transaction = try MacAppTransaction(root: root, verifier: FixtureVerifier())
    let installed = try transaction.install(candidate: candidate, version: "1.1.0", architecture: "arm64")
    try require(installed.stage == "awaiting_health")
    try require(tryValue { try MacBundleIdentity.read(current).version } == "1.1.0")
    let recovered = try transaction.recover(rollback: true)
    try require(recovered?.stage == "rolled_back")
    try require(tryValue { try MacBundleIdentity.read(current).version } == "1.0.0")
    try require(FileManager.default.fileExists(atPath: root.appendingPathComponent(".usque-update/\(installed.identifier.uuidString).app").path))
  }
  private static func failedVerificationPreservesPrevious() throws {
    let root = try folder()
    defer { try? FileManager.default.removeItem(at: root) }
    let current = root.appendingPathComponent("Usque.app")
    let candidate = root.appendingPathComponent("Candidate.app")
    try app(current, version: "1.0.0"); try app(candidate, version: "1.1.0")
    let transaction = try MacAppTransaction(root: root, verifier: FixtureVerifier(rejectVersion: "1.1.0"))
    do { _ = try transaction.install(candidate: candidate, version: "1.1.0", architecture: "arm64"); try require(false) }
    catch MacMaintenanceError.invalidSignature {}
    try require(tryValue { try MacBundleIdentity.read(current).version } == "1.0.0")
    try require(!FileManager.default.fileExists(atPath: root.appendingPathComponent(".usque-update/transaction.json").path))
  }
  private static func tamperedRecoveryStops() throws {
    let root = try folder()
    defer { try? FileManager.default.removeItem(at: root) }
    let current = root.appendingPathComponent("Usque.app")
    let candidate = root.appendingPathComponent("Candidate.app")
    try app(current, version: "1.0.0"); try app(candidate, version: "1.1.0")
    let transaction = try MacAppTransaction(root: root, verifier: FixtureVerifier())
    _ = try transaction.install(candidate: candidate, version: "1.1.0", architecture: "arm64")
    try Data("unexpected-user-change".utf8).write(to: current.appendingPathComponent("Contents/test-only-payload"))
    do { _ = try transaction.recover(rollback: true); try require(false) }
    catch MacMaintenanceError.recoveryRequired {}
    try require(tryValue { try String(contentsOf: current.appendingPathComponent("Contents/test-only-payload"), encoding: .utf8) } == "unexpected-user-change")
  }
  private static func confirmationRetainsBackup() throws {
    let root = try folder()
    defer { try? FileManager.default.removeItem(at: root) }
    let current = root.appendingPathComponent("Usque.app")
    let candidate = root.appendingPathComponent("Candidate.app")
    try app(current, version: "1.0.0"); try app(candidate, version: "1.1.0")
    let transaction = try MacAppTransaction(root: root, verifier: FixtureVerifier())
    let journal = try transaction.install(candidate: candidate, version: "1.1.0", architecture: "arm64")
    try transaction.confirm(identifier: journal.identifier, digest: journal.candidateDigest, componentsHealthy: true)
    try require(tryValue { try transaction.recover()?.stage } == "confirmed")
    try require(FileManager.default.fileExists(atPath: root.appendingPathComponent(".usque-update/\(journal.identifier.uuidString).app").path))
  }
  private static func rosettaMigrationAndRollback() throws {
    try require(tryValue { try macUpdateArchitecture(read: { ["sysctl.proc_translated": 1, "hw.optional.arm64": 0][$0] }) } == "arm64")
    try require(tryValue { try macUpdateArchitecture(read: { ["hw.cputype": 7][$0] }) } == "x86_64")
    let root = try folder()
    defer { try? FileManager.default.removeItem(at: root) }
    let current = root.appendingPathComponent("Usque.app")
    let candidate = root.appendingPathComponent("Candidate.app")
    try app(current, version: "1.0.0", architecture: "x86_64")
    try app(candidate, version: "1.1.0", architecture: "arm64")
    let transaction = try MacAppTransaction(root: root, verifier: FixtureVerifier())
    let journal = try transaction.install(candidate: candidate, version: "1.1.0", architecture: "arm64")
    try require(journal.previousArchitecture == "x86_64" && journal.architecture == "arm64")
    _ = try transaction.recover(rollback: true)
    try require(tryValue { try FixtureVerifier().architecture(current) } == "x86_64")
  }
  private static func journalAndLockFences() throws {
    let root = try folder()
    defer { try? FileManager.default.removeItem(at: root) }
    let staging = root.appendingPathComponent(".usque-update")
    try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    let outside = root.appendingPathComponent("outside.json")
    try Data("{}".utf8).write(to: outside)
    try FileManager.default.createSymbolicLink(at: staging.appendingPathComponent("transaction.json"), withDestinationURL: outside)
    do {
      let transaction = try MacAppTransaction(root: root, verifier: FixtureVerifier())
      _ = try transaction.recover()
      try require(false)
    } catch MacMaintenanceError.recoveryRequired {}
    try FileManager.default.removeItem(at: staging.appendingPathComponent("transaction.json"))
    try FileManager.default.removeItem(at: staging.appendingPathComponent("lock"))
    try FileManager.default.createSymbolicLink(at: staging.appendingPathComponent("lock"), withDestinationURL: outside)
    do { _ = try MacAppTransaction(root: root, verifier: FixtureVerifier()); try require(false) }
    catch MacMaintenanceError.recoveryRequired {}
    try FileManager.default.removeItem(at: staging.appendingPathComponent("lock"))
    try require(mkfifo(staging.appendingPathComponent("lock").path, 0o600) == 0)
    do { _ = try MacAppTransaction(root: root, verifier: FixtureVerifier()); try require(false) }
    catch MacMaintenanceError.recoveryRequired {}
    try FileManager.default.removeItem(at: staging.appendingPathComponent("lock"))
    try FileManager.default.linkItem(at: outside, to: staging.appendingPathComponent("lock"))
    do { _ = try MacAppTransaction(root: root, verifier: FixtureVerifier()); try require(false) }
    catch MacMaintenanceError.recoveryRequired {}
  }
  private static func boundedCommands() throws {
    let output = try nativeOutput("/usr/bin/printf", ["test-only-output"], maximum: 128, timeout: 2)
    try require(output == "test-only-output")
    do { _ = try nativeOutput("/usr/bin/yes", ["test-only"], maximum: 128, timeout: 2); try require(false) }
    catch MacMaintenanceError.systemFailure {}
    let started = ProcessInfo.processInfo.systemUptime
    do { _ = try nativeOutput("/bin/sleep", ["3"], maximum: 128, timeout: 0.1); try require(false) }
    catch MacMaintenanceError.systemFailure {}
    try require(ProcessInfo.processInfo.systemUptime - started < 2)
  }
  private static func interruptedSwapRecovery() throws {
    let root = try folder()
    defer { try? FileManager.default.removeItem(at: root) }
    let current = root.appendingPathComponent("Usque.app")
    let candidate = root.appendingPathComponent("Candidate.app")
    try app(current, version: "1.0.0"); try app(candidate, version: "1.1.0")
    let transaction = try MacAppTransaction(root: root, verifier: FixtureVerifier())
    var journal = try transaction.install(candidate: candidate, version: "1.1.0", architecture: "arm64")
    journal.stage = "prepared"
    let path = root.appendingPathComponent(".usque-update/transaction.json")
    try JSONEncoder().encode(journal).write(to: path)
    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: path.path)
    try require(tryValue { try transaction.recover()?.stage } == "awaiting_health")
  }
  private static func tryValue<T>(_ body: () throws -> T) -> T? { try? body() }
}
