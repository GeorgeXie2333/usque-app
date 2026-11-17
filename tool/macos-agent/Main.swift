import Foundation
import Darwin
@main enum AgentMaintenanceMain {
  static func main() {
    do {
      guard CommandLine.arguments.count == 2, let operation = AgentOperation(rawValue: CommandLine.arguments[1]) else { throw AgentMaintenanceError.invalidInstallation }
      let executable = URL(fileURLWithPath: CommandLine.arguments[0]).standardizedFileURL
      guard executable.resolvingSymlinksInPath() == executable, executable.lastPathComponent == "usque-agent-maintenance" else { throw AgentMaintenanceError.invalidInstallation }
      let contents = executable.deletingLastPathComponent().deletingLastPathComponent()
      guard contents.lastPathComponent == "Contents" else { throw AgentMaintenanceError.invalidInstallation }
      try maintainAgent(operation, backend: NativeAgentMaintenance(contents: contents))
      print("{\"success\":true,\"restart_engine\":true}")
    } catch {
      fputs("macos_agent_maintenance_failed: \((error as? AgentMaintenanceError)?.rawValue ?? "recoveryIncomplete")\n", stderr)
      exit(1)
    }
  }
}
