import Foundation
final class FakeMaintenance: AgentMaintenanceBackend {
  var events: [String] = []; var failure: String?
  func event(_ name: String) throws { events.append(name); if failure == name { throw AgentMaintenanceError.commandFailed } }
  func prepare() throws { try event("prepare") }
  func stopAndRecover() throws { try event("recover") }
  func activate() throws { try event("activate") }
  func rollback() throws { try event("rollback") }
  func finish() throws { try event("finish") }
  func uninstall() throws { try event("uninstall") }
  func restart() throws { try event("restart") }
}
@main enum Tests {
  static func main() throws {
    for failure in ["prepare", "recover", "activate", "finish"] {
      let fake = FakeMaintenance(); fake.failure = failure
      do { try maintainAgent(.install, backend: fake); preconditionFailure("failure swallowed") } catch {}
      if failure == "recover" { precondition(!fake.events.contains("activate")) }
      precondition(fake.events.last == (failure == "prepare" ? "prepare" : "rollback"))
    }
    let failedCleanup = FakeMaintenance(); failedCleanup.failure = "recover"
    do { try maintainAgent(.uninstall, backend: failedCleanup); preconditionFailure("failure swallowed") } catch {}
    precondition(failedCleanup.events == ["recover", "restart"])
    let success = FakeMaintenance(); try maintainAgent(.install, backend: success)
    precondition(success.events == ["prepare", "recover", "activate", "finish"])
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
