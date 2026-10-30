import CoreGraphics
import Foundation

/// The same state and command names used by the Windows and Linux shells.
struct DesktopTrayState {
  var status = "Disconnected"
  var connected = false
  var badge = "idle"
  var open = "Open Usque"
  var connect = "Connect"
  var disconnect = "Disconnect"
  var disconnectExit = "Disconnect and Quit"
  var settings = "Settings…"
  var quitFailed = "Usque could not confirm disconnection. Retry Disconnect before quitting."
  var tunnelLabel = ""
  var systemProxyLabel = ""
  var tunnel = false
  var systemProxy = false
  var outputsEnabled = false
  var tunnelAvailable = false
  var systemProxyAvailable = false

  init() {}

  init?(arguments: Any?) {
    guard let values = arguments as? [String: Any],
      let status = values["status"] as? String, !status.isEmpty,
      status.utf8.count <= 1024,
      let connected = values["connected"] as? Bool
    else { return nil }
    self.status = status
    self.connected = connected
    func label(_ key: String, _ fallback: String = "") -> String {
      guard let value = values[key] as? String, value.utf8.count <= 1024 else {
        return fallback
      }
      return value.isEmpty ? fallback : value
    }
    badge = label("badge", "idle")
    open = label("open", open)
    connect = label("connect", connect)
    disconnect = label("disconnect", disconnect)
    disconnectExit = label("disconnect_exit", disconnectExit)
    settings = label("settings", settings)
    quitFailed = label("quit_failed", quitFailed)
    tunnelLabel = label("tunnel_label")
    systemProxyLabel = label("system_proxy_label")
    tunnel = values["tunnel"] as? Bool ?? false
    systemProxy = values["system_proxy"] as? Bool ?? false
    outputsEnabled = values["outputs_enabled"] as? Bool ?? false
    tunnelAvailable = values["tunnel_available"] as? Bool ?? false
    systemProxyAvailable = values["system_proxy_available"] as? Bool ?? false
  }

  var canToggleTunnel: Bool { outputsEnabled && (tunnel || tunnelAvailable) }
  var canToggleSystemProxy: Bool { outputsEnabled && (systemProxy || systemProxyAvailable) }
}

/// A late result from an earlier quit attempt must never finish a later one.
struct DesktopQuitState {
  private(set) var pending: UUID?
  private(set) var confirmed = false

  mutating func begin() -> UUID? {
    guard !confirmed, pending == nil else { return nil }
    let identifier = UUID()
    pending = identifier
    return identifier
  }

  mutating func finish(_ identifier: UUID, success: Bool) -> Bool {
    guard pending == identifier else { return false }
    pending = nil
    confirmed = success
    return true
  }
}

/// Process-local callback ownership, matching the Windows team's strict URI
/// contract. Authentication Services delivers the URL directly to this app;
/// no system-wide Cloudflare WARP URL association is changed.
struct DesktopZeroTrustSession {
  private(set) var attempt: UUID?
  private var team: String?
  private var callback: String?
  private var failure: String?

  static func normalizedTeam(_ value: String) -> String? {
    let team = value.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    let bytes = Array(team.utf8)
    func alphanumeric(_ value: UInt8) -> Bool {
      (48...57).contains(value) || (97...122).contains(value)
    }
    guard (1...63).contains(bytes.count), let first = bytes.first, let last = bytes.last,
      alphanumeric(first), alphanumeric(last),
      bytes.allSatisfy({ alphanumeric($0) || $0 == 45 }) else { return nil }
    return team
  }

  mutating func begin(_ value: String) -> (identifier: UUID, url: URL)? {
    guard let team = Self.normalizedTeam(value),
      let url = URL(string: "https://\(team).cloudflareaccess.com/warp") else { return nil }
    cancel()
    let identifier = UUID()
    attempt = identifier
    self.team = team
    return (identifier, url)
  }

  mutating func accept(_ value: String, for identifier: UUID) -> Bool {
    guard attempt == identifier, let team,
      Self.validCallback(value, team: team) else { return false }
    callback = value
    self.team = nil
    attempt = nil
    return true
  }

  static func validCallback(_ value: String, team: String) -> Bool {
    guard !value.isEmpty, value.utf8.count <= 64 * 1024,
      value.utf8.allSatisfy({ $0 > 0x20 && $0 != 0x7f }),
      let scheme = value.range(of: "://"),
      value[..<scheme.lowerBound].lowercased() == "com.cloudflare.warp"
    else { return false }
    let remaining = value[scheme.upperBound...]
    guard let queryStart = remaining.firstIndex(of: "?"),
      !remaining.contains("#"),
      let pathStart = remaining[..<queryStart].firstIndex(of: "/"),
      remaining[..<pathStart].lowercased() == "\(team).cloudflareaccess.com",
      remaining[pathStart..<queryStart] == "/auth" else { return false }
    let query = remaining[remaining.index(after: queryStart)...]
    return query.hasPrefix("token=") && query.count > 6 && !query.contains("&")
  }

  mutating func fail(_ identifier: UUID, code: String) -> Bool {
    guard attempt == identifier else { return false }
    attempt = nil
    team = nil
    callback = nil
    failure = code
    return true
  }

  mutating func consume() -> (callback: String?, failure: String?) {
    defer { callback = nil; failure = nil }
    return (callback, failure)
  }

  mutating func cancel() {
    attempt = nil
    team = nil
    callback = nil
    failure = nil
  }
}

enum DesktopConfigurationFiles {
  static let maximumCount = 128
  static let maximumBytes = 128 * 1024

  /// Read at most one byte beyond the limit, including files that grow while
  /// open. The selected basename is the only path information sent to Dart.
  static func read(_ url: URL) -> (name: String, bytes: Data?, error: String?) {
    let name = url.lastPathComponent
    guard url.isFileURL,
      let values = try? url.resourceValues(forKeys: [.isRegularFileKey]),
      values.isRegularFile == true,
      let file = try? FileHandle(forReadingFrom: url)
    else { return (name, nil, "CHAIN_FILE_READ_FAILED") }
    defer { try? file.close() }
    do {
      let bytes = try file.read(upToCount: maximumBytes + 1) ?? Data()
      guard bytes.count <= maximumBytes else { return (name, nil, "CHAIN_FILE_TOO_LARGE") }
      guard !bytes.isEmpty else { return (name, nil, "CHAIN_FILE_READ_FAILED") }
      return (name, bytes, nil)
    } catch {
      return (name, nil, "CHAIN_FILE_READ_FAILED")
    }
  }
}

enum DesktopWindowPlacement {
  // Match the Windows logical client area. macOS keeps its native title bar
  // outside this content size instead of drawing a second caption in Flutter.
  static let defaultContentSize = CGSize(width: 1040, height: 760)
  static let preferredMinimumContentSize = CGSize(width: 640, height: 480)

  static func defaultFrame(on screen: CGRect, decoration: CGSize) -> CGRect {
    let size = CGSize(
      width: defaultContentSize.width + decoration.width,
      height: defaultContentSize.height + decoration.height)
    let centered = CGRect(
      x: screen.midX - size.width / 2,
      y: screen.midY - size.height / 2,
      width: size.width, height: size.height)
    return restoredFrame(centered, screens: [screen])
  }

  /// A minimum larger than the visible display prevents shrinking a window
  /// back onto a small screen. Account for the native caption as well.
  static func minimumContentSize(on screen: CGRect, decoration: CGSize) -> CGSize {
    CGSize(
      width: min(preferredMinimumContentSize.width, max(1, screen.width - decoration.width)),
      height: min(preferredMinimumContentSize.height, max(1, screen.height - decoration.height)))
  }

  /// Screen arrangements can change while the app is closed. Keep the saved
  /// window reachable without changing its preferred size unnecessarily.
  static func restoredFrame(_ frame: CGRect, screens: [CGRect]) -> CGRect {
    guard let screen = screens.max(by: {
      $0.intersection(frame).area < $1.intersection(frame).area
    }) else { return frame }
    let size = CGSize(width: min(frame.width, screen.width), height: min(frame.height, screen.height))
    return CGRect(
      x: min(max(frame.minX, screen.minX), screen.maxX - size.width),
      y: min(max(frame.minY, screen.minY), screen.maxY - size.height),
      width: size.width, height: size.height)
  }
}

private extension CGRect {
  var area: CGFloat { isNull ? 0 : width * height }
}
