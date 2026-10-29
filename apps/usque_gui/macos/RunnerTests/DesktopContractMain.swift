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
    try protonFraming()
    try protonResults()
    try nativeLanguageResolution()
    try nativeLocalizedCopy()
    try nativeMenuCoverage()
    print("10 macOS desktop contract groups passed")
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
      "The verification challenge expired.", "Approve Usque in Login Items, then retry."] {
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
    let state = DesktopTrayState(arguments: [
      "status": "Connected", "connected": true, "outputs_enabled": true,
      "tunnel_available": false, "system_proxy_available": true,
    ])!
    try require(!state.canToggleTunnel, "menu bypasses unavailable VPN capability")
    try require(state.canToggleSystemProxy, "available system proxy cannot be enabled")
    var selected = state
    selected.tunnel = true
    try require(selected.canToggleTunnel, "selected unavailable output cannot be disabled")
    selected.outputsEnabled = false
    try require(!selected.canToggleTunnel && !selected.canToggleSystemProxy, "busy outputs can mutate")
  }

  private static func quitAcknowledgements() throws {
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
    let missingScreen = CGRect(x: -2000, y: 1000, width: 1800, height: 1200)
    let recovered = DesktopWindowPlacement.restoredFrame(missingScreen, screens: [screen])
    try require(screen.contains(recovered), "window remains unreachable after screen removal")
    let normal = CGRect(x: 200, y: 150, width: 900, height: 650)
    try require(DesktopWindowPlacement.restoredFrame(normal, screens: [screen]) == normal, "valid saved placement changed")
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
    try require(session.fail(third.identifier, code: "ZERO_TRUST_LOGIN_CANCELLED"), "cancellation not recorded")
    try require(session.consume().failure == "ZERO_TRUST_LOGIN_CANCELLED", "cancellation not reported")
    try require(session.begin("example-team") != nil, "cancelled login cannot retry")
  }

  private static func protonFraming() throws {
    let bytes = Data("test-only".utf8)
    let frame = try ProtonVerificationContract.frame(bytes, maximum: 128)
    let decoded = try ProtonVerificationContract.payload(frame, maximum: 128)
    try require(decoded == bytes, "Proton frame lost data")
    for bad in [Data(), Data([0, 0, 0, 0]), Data([0, 1, 0, 1]), frame.dropLast(), frame + Data([0])] {
      do {
        _ = try ProtonVerificationContract.payload(Data(bad), maximum: 128)
        try require(false, "malformed Proton frame accepted")
      } catch ProtonVerificationBridgeError.invalidFrame {}
    }
  }

  private static func protonResults() throws {
    let handoff = Data(#"{"version":2,"route_epoch":1,"expires_at":101000,"methods":["captcha"]}"#.utf8)
    let binding = try ProtonVerificationContract.Binding(handoff, now: 100000)
    func payload(_ message: String) throws -> Data {
      try JSONSerialization.data(withJSONObject: ["status": "success", "message": message])
    }
    let message = #"{"type":"HUMAN_VERIFICATION_SUCCESS","payload":{"type":"captcha","token":"test-only"}}"#
    if case .success(let result) = try ProtonVerificationContract.result(payload(message), binding: binding, now: 100001) {
      try require(result == Data(message.utf8), "Proton validated response modified")
    } else { try require(false, "Proton validated response lost") }
    for invalid in [
      #"{"type":"HUMAN_VERIFICATION_SUCCESS","payload":{"type":"email","token":"test-only"}}"#,
      #"{"type":"USQUE_VERIFICATION_NETWORK_FAILED","route_epoch":0}"#,
      #"{"type":"HUMAN_VERIFICATION_SUCCESS","payload":{"type":"captcha","token":""}}"#,
    ] {
      do {
        _ = try ProtonVerificationContract.result(payload(invalid), binding: binding, now: 100001)
        try require(false, "unbound Proton response accepted")
      } catch ProtonVerificationBridgeError.invalidResult {}
    }
    do {
      _ = try ProtonVerificationContract.result(payload(message), binding: binding, now: 101001)
      try require(false, "expired Proton response accepted")
    } catch ProtonVerificationBridgeError.invalidResult {}
  }
}
