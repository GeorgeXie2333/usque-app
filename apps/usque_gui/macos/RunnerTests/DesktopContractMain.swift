import CoreGraphics
import Foundation

@main
enum DesktopContractMain {
  static func main() throws {
    try trayCapabilities()
    try quitAcknowledgements()
    try boundedConfigurationFiles()
    try restoredWindow()
    try zeroTrustCallbacks()
    try nativeLanguageResolution()
    try nativeLocalizedCopy()
    try nativeMenuCoverage()
    print("8 macOS desktop contract groups passed")
  }

  private static func require(_ condition: @autoclosure () -> Bool, _ description: String) throws {
    if !condition() { throw NSError(domain: "DesktopContract", code: 1, userInfo: [NSLocalizedDescriptionKey: description]) }
  }

  private static func nativeLanguageResolution() throws {
    for input in ["zh_HK", "zh-Hant-HK", "zh-MO", "zh-Hans-HK", "zh-Hans-MO"] {
      try require(NativeLocalization(catalogID: input).catalogID == "zh_HK", "Hong Kong/Macao native catalog mismatch")
    }
    for input in ["zh_TW", "zh-Hant", "zh-Hant-TW", "zh-Hans-TW"] {
      try require(NativeLocalization(catalogID: input).catalogID == "zh_TW", "Traditional Chinese native catalog mismatch")
    }
    try require(NativeLocalization(preferredLanguages: ["zh-Hant-HK"]).catalogID == "zh_HK", "startup does not follow Traditional system language")
    try require(NativeLocalization(catalogID: "zh_CN").catalogID == "zh_CN", "Simplified Chinese native catalog mismatch")
    // Match AppStrings._catalogIdForSystemLocale: HK/MO, TW, Hant, then CN.
    for (input, expected) in [("zh-Hans-CN", "zh_CN"), ("zh-Hant-CN", "zh_TW"),
      ("zh-Hans-SG", "zh_CN"), ("zh", "zh_CN")] {
      try require(NativeLocalization(preferredLanguages: [input]).catalogID == expected, "native startup differs from shared Flutter Chinese catalog policy")
    }
    for input in ["en", "fr", "invalid", String(repeating: "x", count: 129)] {
      try require(NativeLocalization(catalogID: input).catalogID == "en", "unsupported native catalog is not safely defaulted")
    }
  }

  private static func nativeLocalizedCopy() throws {
    let traditional = NativeLocalization(catalogID: "zh_HK")
    let taiwan = NativeLocalization(catalogID: "zh_TW")
    let simplified = NativeLocalization(catalogID: "zh_CN")
    let english = NativeLocalization(catalogID: "en")
    for source in ["Import configurations", "Export diagnostics", "Export WARP credentials", "Save",
      "A file picker is already open.", "Select at most 128 configuration files.",
      "Cloudflare login did not complete. Retry the login.", "Approve Usque in Login Items, then retry."] {
      try require(traditional.text(source) != source && taiwan.text(source) == traditional.text(source), "native Traditional copy is missing")
      try require(simplified.text(source) != source, "native Simplified copy regressed")
      try require(english.text(source) == source, "English native fallback regressed")
    }
    let source = "About APP_NAME"
    try require(traditional.menuTitle(source) == "關於 Usque", "native About menu did not translate")
    try require(english.menuTitle(source) == "About Usque", "native menu cannot restore English from original title")
    try require(traditional.text("Unknown external menu item") == "Unknown external menu item", "external menu items must remain intact")
    let record = NativeMenuTitleRecord("Enter Full Screen")
    let translated = record.title(current: "Enter Full Screen", localization: traditional)
    try require(record.title(current: translated, localization: english) == "Enter Full Screen", "native language switching lost the source title")
    try require(record.title(current: "Exit Full Screen", localization: traditional) == "離開全螢幕", "AppKit's updated command was replaced by a stale title")
  }

  private static func nativeMenuCoverage() throws {
    guard CommandLine.arguments.count == 2 else {
      throw NSError(domain: "DesktopContract", code: 2, userInfo: [NSLocalizedDescriptionKey: "pass the MainMenu.xib fixture path"])
    }
    let xml = try XMLDocument(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
    let titles = try xml.nodes(forXPath: "//menu/@title | //menuItem/@title")
    let traditional = NativeLocalization(catalogID: "zh_TW")
    for attribute in titles {
      guard let title = attribute.stringValue, !["APP_NAME", "Main Menu"].contains(title) else { continue }
      try require(traditional.menuTitle(title) != title.replacingOccurrences(of: "APP_NAME", with: "Usque"), "native menu lacks Traditional translation: \(title)")
    }
  }

  private static func trayCapabilities() throws {
    try require(DesktopTrayState(arguments: ["connected": true]) == nil, "malformed tray update accepted")
    let unavailable = DesktopTrayState(arguments: ["status": "Disconnected", "connected": false])!
    try require(!unavailable.connectionEnabled, "uninitialized connection action is enabled")
    let state = DesktopTrayState(arguments: [
      "status": "Connected", "connected": true, "outputs_enabled": true,
      "connection_enabled": true,
      "tunnel_available": false, "system_proxy_available": true,
    ])!
    try require(!state.canToggleTunnel, "menu bypasses unavailable VPN capability")
    try require(state.canToggleSystemProxy, "available system proxy cannot be enabled")
    try require(state.connectionEnabled, "available disconnect action is disabled")
    var selected = state
    selected.tunnel = true
    try require(selected.canToggleTunnel, "selected unavailable output cannot be disabled")
    selected.outputsEnabled = false
    try require(!selected.canToggleTunnel && !selected.canToggleSystemProxy, "busy outputs can mutate")
  }

  private static func quitAcknowledgements() throws {
    for value: Any? in [nil, true, NSNull(), [:], ["cleanup_complete": false],
      ["cleanup_complete": 1], ["cleanup_complete": "true"], ["phase": "disconnected"]] {
      try require(!DesktopQuitState.cleanupCompleted(value), "missing or malformed cleanup acknowledgement accepted")
    }
    try require(DesktopQuitState.cleanupCompleted(["cleanup_complete": true]), "completed cleanup acknowledgement rejected")
    var quit = DesktopQuitState()
    let first = quit.begin()!
    try require(quit.begin() == nil, "parallel quit attempt admitted")
    try require(!quit.finish(UUID(), success: true), "foreign acknowledgement accepted")
    try require(quit.finish(first, success: false) && !quit.confirmed, "timed-out quit did not cancel")
    let second = quit.begin()!
    try require(!quit.finish(first, success: true), "late acknowledgement confirms new quit")
    try require(quit.finish(second, success: true) && quit.confirmed, "matching disconnect acknowledgement rejected")
    try require(!quit.finish(second, success: false), "late timeout overwrites confirmed quit")
  }

  private static func boundedConfigurationFiles() throws {
    let folder = FileManager.default.temporaryDirectory.appendingPathComponent("usque-desktop-contract-\(UUID())")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: folder) }
    let valid = folder.appendingPathComponent("wireguard.conf")
    try Data("[Interface]\nPrivateKey=test-only".utf8).write(to: valid)
    let result = DesktopConfigurationFiles.read(valid)
    try require(result.name == "wireguard.conf" && result.bytes != nil && result.error == nil, "configuration bytes lost")
    let limit = folder.appendingPathComponent("large.ovpn")
    try Data(repeating: 1, count: DesktopConfigurationFiles.maximumBytes + 1).write(to: limit)
    try require(DesktopConfigurationFiles.read(limit).error == "CHAIN_FILE_TOO_LARGE", "oversized configuration accepted")
    try require(DesktopConfigurationFiles.read(folder).error == "CHAIN_FILE_READ_FAILED", "directory accepted as configuration")
    try Data().write(to: limit)
    try require(DesktopConfigurationFiles.read(limit).error == "CHAIN_FILE_READ_FAILED", "empty configuration accepted")
  }

  private static func restoredWindow() throws {
    let screen = CGRect(x: 100, y: 100, width: 1200, height: 800)
    let decoration = CGSize(width: 0, height: 22)
    let firstLaunch = DesktopWindowPlacement.defaultFrame(on: screen, decoration: decoration)
    try require(firstLaunch.size == CGSize(width: 1040, height: 782), "default content size differs from Windows")
    try require(firstLaunch.midX == screen.midX && firstLaunch.midY == screen.midY, "first launch is not centered")
    try require(DesktopWindowPlacement.minimumContentSize(on: screen, decoration: decoration) == CGSize(width: 640, height: 480), "desktop minimum content size missing")
    let smallScreen = CGRect(x: -600, y: 20, width: 600, height: 420)
    try require(DesktopWindowPlacement.defaultFrame(on: smallScreen, decoration: decoration) == smallScreen, "default window exceeds a small display")
    try require(DesktopWindowPlacement.minimumContentSize(on: smallScreen, decoration: decoration) == CGSize(width: 600, height: 398), "minimum window cannot fit a small display")
    let missingScreen = CGRect(x: -2000, y: 1000, width: 1800, height: 1200)
    let recovered = DesktopWindowPlacement.restoredFrame(missingScreen, screens: [screen])
    try require(screen.contains(recovered), "window remains unreachable after screen removal")
    let normal = CGRect(x: 200, y: 150, width: 900, height: 650)
    try require(DesktopWindowPlacement.restoredFrame(normal, screens: [screen]) == normal, "valid saved placement changed")
    let secondary = CGRect(x: -1500, y: 100, width: 1400, height: 900)
    let onSecondary = CGRect(x: -1300, y: 180, width: 1040, height: 782)
    try require(DesktopWindowPlacement.restoredFrame(onSecondary, screens: [screen, secondary]) == onSecondary, "valid secondary monitor placement changed")
    let xml = try XMLDocument(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
    let defaults = try xml.nodes(forXPath: "//window/rect[@key='contentRect']").first as? XMLElement
    try require(defaults?.attribute(forName: "width")?.stringValue == "1040" && defaults?.attribute(forName: "height")?.stringValue == "760", "XIB initial content size differs from the placement contract")
  }

  private static func zeroTrustCallbacks() throws {
    var session = DesktopZeroTrustSession()
    try require(session.begin("https://team.example") == nil, "URL accepted as team")
    let first = session.begin(" Example-Team ")!
    try require(first.url.absoluteString == "https://example-team.cloudflareaccess.com/warp", "team URL mismatch")
    let callback = "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=test-only"
    for invalid in [
      "com.cloudflare.warp://wrong.cloudflareaccess.com/auth?token=secret",
      "com.cloudflare.warp://example-team.cloudflareaccess.com:443/auth?token=secret",
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=secret&extra=value",
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=secret#fragment",
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=",
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?%74oken=secret",
    ] {
      try require(!session.accept(invalid, for: first.identifier), "invalid callback accepted")
    }
    session.cancel()
    try require(!session.accept(callback, for: first.identifier), "cancelled login accepts callback")
    let second = session.begin("example-team")!
    try require(!session.accept(callback, for: first.identifier), "old browser can complete new login")
    try require(session.accept(callback, for: second.identifier), "matching callback rejected")
    try require(session.consume().callback == callback && session.consume().callback == nil, "callback is not single use")
    let third = session.begin("example-team")!
    try require(!session.fail(second.identifier, code: "ZERO_TRUST_LOGIN_TIMEOUT"), "old timeout cancels a newer login")
    try require(session.fail(third.identifier, code: "ZERO_TRUST_LOGIN_CANCELLED"), "cancellation not recorded")
    try require(!session.accept(callback, for: third.identifier), "cancelled browser completion restores a callback")
    try require(session.consume().failure == "ZERO_TRUST_LOGIN_CANCELLED", "cancellation not reported")
    let retry = session.begin("example-team")!
    try require(!session.fail(third.identifier, code: "ZERO_TRUST_CALLBACK_INVALID"), "late cancelled completion poisons a retry")
    try require(session.accept(callback, for: retry.identifier), "cancelled login cannot retry")
    session.cancel()
    try require(session.consume().callback == nil, "explicit cancellation retains an accepted callback")
    let timedOut = session.begin("example-team")!
    try require(session.fail(timedOut.identifier, code: "ZERO_TRUST_LOGIN_TIMEOUT"), "timeout not recorded")
    try require(!session.accept(callback, for: timedOut.identifier), "expired browser completion is accepted")
    _ = session.begin("example-team")
    let empty = session.consume()
    try require(empty.callback == nil && empty.failure == nil, "retry inherits stale timeout or callback")
  }
}
