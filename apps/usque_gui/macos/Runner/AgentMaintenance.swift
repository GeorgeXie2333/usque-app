import Cocoa
import CryptoKit
import FlutterMacOS
import Foundation

/// Only fixed service actions cross this channel. No shell, path, PF text,
/// account credential or arbitrary launchd label is accepted from Dart.
enum AgentMaintenanceBridge {
  private static let root = "/Library/PrivilegedHelperTools/com.usque.agent"
  static func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) -> Bool {
    guard ["getMacosServiceState", "maintainMacosService"].contains(call.method) else { return false }
    if call.method == "getMacosServiceState" {
      DispatchQueue.global(qos: .utility).async {
        let helper = Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/usque-agent-maintenance")
        let installed = FileManager.default.isExecutableFile(atPath: root + "/usque-engine")
        var state: [String: Any] = ["installed": installed, "compatible": false, "canMaintain": FileManager.default.isExecutableFile(atPath: helper.path), "recoveryPending": installed, "tunAvailable": false]
        if installed, let data = try? run(root + "/usque-engine", ["--macos-agent-status"]),
          let observed = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
          state.merge(observed) { _, new in new }
        }
        let response = state
        DispatchQueue.main.async { result(response) }
      }
    } else {
      let values = call.arguments as? [String: Any]
      guard let operation = values?["operation"] as? String, ["install", "repair", "recover", "uninstall"].contains(operation), values?.count == 1 else {
        result(FlutterError(code: "MACOS_AGENT_INVALID_OPERATION", message: "Unknown service action.", details: nil)); return true
      }
      let helper = Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/usque-agent-maintenance")
      guard FileManager.default.isExecutableFile(atPath: helper.path), helper.resolvingSymlinksInPath() == helper else {
        result(FlutterError(code: "MACOS_AGENT_INSTALL_REQUIRED", message: "Reinstall the application package.", details: nil)); return true
      }
      DispatchQueue.global(qos: .userInitiated).async {
        do {
          // Capture the user-approved snapshot before displaying authorization.
          // Only root-private, fully verified copies can subsequently execute.
          let contents = Bundle.main.bundleURL.appendingPathComponent("Contents")
          let artifacts = ["Helpers/usque-agent-maintenance", "Helpers/usque-agent", "MacOS/usque-engine"]
          let snapshots = try artifacts.map { relative -> (String, String, String) in
            let source = contents.appendingPathComponent(relative)
            guard source.resolvingSymlinksInPath() == source else { throw NSError(domain: "UsqueAgent", code: 3) }
            let file = try FileHandle(forReadingFrom: source); defer { try? file.close() }
            var digest = SHA256(); var size = 0
            while let data = try file.read(upToCount: 65536), !data.isEmpty {
              size += data.count; guard size <= 512 * 1024 * 1024 else { throw NSError(domain: "UsqueAgent", code: 3) }; digest.update(data: data)
            }
            guard size > 0 else { throw NSError(domain: "UsqueAgent", code: 3) }
            return (source.path, relative, digest.finalize().map { String(format: "%02x", $0) }.joined())
          }
          var command = "set -eu; umask 077; task_stage=$(/usr/bin/mktemp -d /private/tmp/usque-maint.XXXXXXXX); trap '/bin/rm -rf -- \"$task_stage\"' EXIT; /bin/mkdir -p \"$task_stage/Contents/Helpers\" \"$task_stage/Contents/MacOS\"; "
          for (source, relative, digest) in snapshots {
            let staged = "\"$task_stage/Contents/" + relative + "\""
            command += "/bin/cp -L -- " + shellQuote(source) + " " + staged + "; /bin/chmod 700 " + staged + "; task_hash=$(/usr/bin/shasum -a 256 " + staged + "); [ \"${task_hash%% *}\" = " + shellQuote(digest) + " ]; "
          }
          command += "\"$task_stage/Contents/Helpers/usque-agent-maintenance\" " + shellQuote(operation)
          let script = "do shell script " + appleQuote(command) + " with administrator privileges"
          _ = try run("/usr/bin/osascript", ["-e", script], timeout: 180)
          DispatchQueue.main.async { result(nil) }
        } catch {
          DispatchQueue.main.async { result(FlutterError(code: "MACOS_AGENT_MAINTENANCE_FAILED", message: "Service maintenance was cancelled or recovery is incomplete. Retry the action.", details: nil)) }
        }
      }
    }
    return true
  }
  private static func shellQuote(_ value: String) -> String { "'" + value.replacingOccurrences(of: "'", with: "'\\''") + "'" }
  private static func appleQuote(_ value: String) -> String { "\"" + value.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\"" }
  private static func run(_ path: String, _ arguments: [String], timeout: TimeInterval = 20) throws -> Data {
    let task = Process(); task.executableURL = URL(fileURLWithPath: path); task.arguments = arguments
    task.environment = ["PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LC_ALL": "C"]
    task.standardInput = FileHandle.nullDevice
    let pipe = Pipe(); task.standardOutput = pipe; task.standardError = FileHandle.nullDevice
    try task.run()
    let deadline = Date().addingTimeInterval(timeout)
    while task.isRunning && Date() < deadline { Thread.sleep(forTimeInterval: 0.02) }
    if task.isRunning { task.terminate() }
    guard !task.isRunning || Date() < deadline else { throw NSError(domain: "UsqueAgent", code: 1) }
    task.waitUntilExit()
    guard task.terminationStatus == 0, let data = try pipe.fileHandleForReading.read(upToCount: 65537), data.count <= 65536 else { throw NSError(domain: "UsqueAgent", code: 2) }
    return data
  }
}
