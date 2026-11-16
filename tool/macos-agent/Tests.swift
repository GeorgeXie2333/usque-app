import Foundation
final class FakeMaintenance: AgentMaintenanceBackend {
  var events: [String] = []; var failure: String?
  func event(_ name: String) throws { events.append(name); if failure == name { throw AgentMaintenanceError.commandFailed } }
  func prepare() throws { try event("prepare") }
  func stopAndRecover() throws { try event("recover") }
  func stopAndRecoverPrepared() throws { try event("recover-prepared") }
  func activate() throws { try event("activate") }
  func rollback() throws { try event("rollback") }
  func finish() throws { try event("finish") }
  func uninstall() throws { try event("uninstall") }
  func restart() throws { try event("restart") }
}
@main enum Tests {
  static func main() throws {
    // The GUI's administrator command deliberately runs with umask 077.
    // Exercise the real installer primitives without touching system paths.
    let sandbox = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: sandbox, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: sandbox) }
    let originalMask = umask(0o077); defer { umask(originalMask) }
    let directory = sandbox.appendingPathComponent("agent.next")
    let madeDirectory = try createDirectoryIfMissing(directory, mode: 0o755)
    var failures = [String]()
    func expectMode(_ url: URL, _ expected: mode_t) throws {
      var info = stat()
      guard lstat(url.path, &info) == 0 else { throw AgentMaintenanceError.invalidInstallation }
      if info.st_mode & 0o777 != expected { failures.append(url.lastPathComponent) }
    }
    precondition(madeDirectory)
    try expectMode(directory, 0o755)
    for (name, mode): (String, mode_t) in [("usque-engine", 0o755), ("install.json", 0o644), ("journal", 0o600)] {
      let path = directory.appendingPathComponent(name)
      try writeNewFile(Data("fixture".utf8), to: path, mode: mode)
      try expectMode(path, mode)
    }
    let existing = sandbox.appendingPathComponent("private")
    _ = try createDirectoryIfMissing(existing, mode: 0o700)
    _ = try createDirectoryIfMissing(existing, mode: 0o755)
    try expectMode(existing, 0o700)
    if !failures.isEmpty {
      fputs("Incorrect install permissions after umask 077: \(failures)\n", stderr)
      exit(1)
    }
    for failure in ["prepare", "recover-prepared", "activate", "finish"] {
      let fake = FakeMaintenance(); fake.failure = failure
      do { try maintainAgent(.install, backend: fake); preconditionFailure("failure swallowed") } catch {}
      if failure == "recover-prepared" { precondition(!fake.events.contains("activate")) }
      precondition(fake.events.last == (failure == "prepare" ? "prepare" : "rollback"))
    }
    let failedCleanup = FakeMaintenance(); failedCleanup.failure = "recover"
    do { try maintainAgent(.uninstall, backend: failedCleanup); preconditionFailure("failure swallowed") } catch {}
    precondition(failedCleanup.events == ["recover", "restart"])
    let success = FakeMaintenance(); try maintainAgent(.install, backend: success)
    precondition(success.events == ["prepare", "recover-prepared", "activate", "finish"])
    let repair = FakeMaintenance(); repair.failure = "recover"
    try maintainAgent(.repair, backend: repair)
    precondition(repair.events == ["prepare", "recover-prepared", "activate", "finish"])
    let identity = Array(repeating: String(repeating: "a", count: 64), count: 3).joined(separator: ":")
    for phase in [ReplacementPhase.preparing, .prepared, .initializing, .activated, .committed] {
      try Replacement(schema: 2, phase: phase, candidate: identity, previous: identity).validate()
    }
    for invalid in [
      Replacement(schema: 1, phase: .committed, candidate: identity, previous: nil),
      Replacement(schema: 2, phase: .prepared, candidate: nil, previous: nil),
      Replacement(schema: 2, phase: .initializing, candidate: "untrusted-path", previous: nil),
      Replacement(schema: 2, phase: .activated, candidate: identity, previous: nil, previousPlist: Data(repeating: 0, count: 65537)),
    ] {
      do { try invalid.validate(); preconditionFailure("corrupt replacement evidence accepted") } catch {}
    }
    let recovery = FakeMaintenance(); try maintainAgent(.recover, backend: recovery)
    precondition(recovery.events == ["recover", "restart"])
    print("Agent maintenance ordering, rollback, recovery and journal validation tests passed")
  }
}
