import AppKit
import Foundation

@main
enum DesktopContractMain {
  static func main() throws {
    try trayCapabilities()
    try quitAcknowledgements()
    try relaunchAfterCleanup()
    try boundedConfigurationFiles()
    try restoredWindow()
    try zeroTrustCallbacks()
    try nativeLanguageResolution()
    try nativeLocalizedCopy()
    try nativeMenuCoverage()
    try trayArtwork()
    try installedServiceIdentity()
    print("11 macOS desktop contract groups passed")
  }

  private static func installedServiceIdentity() throws {
    let fm = FileManager.default
    let folder = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    let root = folder.appendingPathComponent("installed")
    let contents = folder.appendingPathComponent("Contents")
    defer { try? fm.removeItem(at: folder) }
    for directory in [root, contents.appendingPathComponent("MacOS"), contents.appendingPathComponent("Helpers")] {
      try fm.createDirectory(at: directory, withIntermediateDirectories: true)
    }
    for (relative, name) in [("MacOS/usque-engine", "usque-engine"), ("Helpers/usque-agent", "usque-agent")] {
      let file = root.appendingPathComponent(name)
      try Data(name.utf8).write(to: file)
      try fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: file.path)
      try fm.copyItem(at: file, to: contents.appendingPathComponent(relative))
    }
    try require(matchesInstalledMacosService(root: root, contents: contents), "matching installed service rejected")
    try Data("old agent".utf8).write(to: root.appendingPathComponent("usque-agent"))
    try require(!matchesInstalledMacosService(root: root, contents: contents), "same Engine masked a stale installed Agent")
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
    for id in ["ja", "ko", "es", "pt", "fr", "nl", "tr", "ru", "fa", "ar", "de", "id", "it", "pl", "th", "uk", "vi"] {
      try require(NativeLocalization(catalogID: id).catalogID == id, "supported native catalog fell back: \(id)")
    }
    try require(NativeLocalization(preferredLanguages: ["pt-BR"]).catalogID == "pt", "regional Portuguese fallback")
    for input in ["en", "invalid", String(repeating: "x", count: 129)] {
      try require(NativeLocalization(catalogID: input).catalogID == "en", "unsupported native catalog is not safely defaulted")
    }
    try require(NativeLocalization(catalogID: "in-ID").catalogID == "id", "legacy Indonesian language code")
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
    let xib = URL(fileURLWithPath: CommandLine.arguments[1])
    let xml = try XMLDocument(contentsOf: xib)
    let titles = try xml.nodes(forXPath: "//menu/@title | //menuItem/@title")
    let traditional = NativeLocalization(catalogID: "zh_TW")
    for attribute in titles {
      guard let title = attribute.stringValue, !["APP_NAME", "Main Menu"].contains(title) else { continue }
      try require(traditional.menuTitle(title) != title.replacingOccurrences(of: "APP_NAME", with: "Usque"), "native menu lacks Traditional translation: \(title)")
    }
    let info = try Data(contentsOf: xib.deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("Info.plist"))
    let plist = try PropertyListSerialization.propertyList(from: info, format: nil) as? [String: Any]
    let regions = plist?["CFBundleLocalizations"] as? [String] ?? []
    func strings(_ folder: String) throws -> [String: String] {
      let file = Bundle.main.resourceURL!.appendingPathComponent("\(folder).lproj/UsqueNative.strings")
      return try PropertyListSerialization.propertyList(from: Data(contentsOf: file), format: nil) as? [String: String] ?? [:]
    }
    let originals = try strings("en")
    try require(!originals.isEmpty && regions.contains("fa"), "missing native catalog metadata")
    for region in regions {
      let local = NativeLocalization(catalogID: region)
      let folder = ["zh-HK", "zh-TW"].contains(region) ? "zh-Hant" : region
      let translated = try strings(folder)
      try require(Set(translated.keys) == Set(originals.keys), "incomplete native strings: \(region)")
      for (key, value) in translated {
        try require(!value.isEmpty && local.text(key) == value, "wrong native lookup: \(region) / \(key)")
      }
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

  private static func trayArtwork() throws {
    let runner = URL(fileURLWithPath: CommandLine.arguments[1])
      .deletingLastPathComponent().deletingLastPathComponent()
    guard let logo = NSImage(contentsOf: runner.appendingPathComponent(
      "Assets.xcassets/AppIcon.appiconset/app_icon_64.png")) else {
      throw NSError(domain: "DesktopContract", code: 3)
    }
    let size = NSSize(width: 18, height: 18)
    for scale in [1, 2] {
      let pixels = 18 * scale
      func render(_ image: NSImage) -> Data {
        let context = CGContext(data: nil, width: pixels, height: pixels, bitsPerComponent: 8,
          bytesPerRow: pixels * 4, space: CGColorSpaceCreateDeviceRGB(),
          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.scaleBy(x: CGFloat(scale), y: CGFloat(scale))
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
        image.draw(in: NSRect(origin: .zero, size: size))
        NSGraphicsContext.restoreGraphicsState()
        return Data(bytes: context.data!, count: pixels * pixels * 4)
      }
      let original = render(logo)
      var appearances = Set<Data>()
      for badge in ["idle", "busy", "connected", "warning", "error"] {
        var state = DesktopTrayState()
        state.badge = badge
        let image = state.menuBarImage(appIcon: logo)
        let artwork = render(image)
        try require(image.size == size && !image.isTemplate, "tray lost its size or brand colors")
        for row in 0..<pixels {
          let start = row * pixels * 4
          let leftHalf = start..<(start + pixels / 2 * 4)
          try require(artwork[leftHalf] == original[leftHalf], "tray badge replaced the Usque artwork")
        }
        if badge == "idle" { try require(artwork == original, "idle tray changed the Usque icon") }
        appearances.insert(artwork)
      }
      try require(appearances.count == 5, "tray states should remain visually distinct")
    }
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

  private static func relaunchAfterCleanup() throws {
    var quit = DesktopQuitState()
    var launches = 0
    let schedule = { launches += 1 }
    let proof = ["cleanup_complete": true]
    for invalid: Any in [["cleanup_complete": false], ["cleanup_complete": 1], NSNull()] {
      try require(!quit.confirmRelaunch(after: invalid, schedule: schedule), "unconfirmed cleanup scheduled a relaunch")
    }
    let ordinaryQuit = quit.begin()!
    try require(!quit.confirmRelaunch(after: proof, schedule: schedule), "relaunch replaced a pending quit")
    _ = quit.finish(ordinaryQuit, success: false)
    do {
      _ = try quit.confirmRelaunch(after: proof) { throw CocoaError(.executableNotLoadable) }
      throw NSError(domain: "DesktopContract", code: 3)
    } catch let error as CocoaError where error.code == .executableNotLoadable {}
    try require(!quit.confirmed && launches == 0, "failed handoff allowed termination")
    try require(quit.confirmRelaunch(after: proof, schedule: schedule) && quit.confirmed, "verified relaunch rejected")
    try require(!quit.confirmRelaunch(after: proof, schedule: schedule) && launches == 1, "duplicate opener scheduled")
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
    let moved = CGRect(x: -580, y: 40, width: 640, height: 480)
    let movedScreen = DesktopWindowPlacement.targetScreen(for: moved, screens: [screen, smallScreen])!
    try require(movedScreen == smallScreen, "window move keeps the former display's minimum")
    try require(DesktopWindowPlacement.minimumContentSize(on: movedScreen, decoration: decoration) == CGSize(width: 600, height: 398), "moved window cannot shrink to its smaller display")
    let resized = DesktopWindowPlacement.restoredFrame(onSecondary, screens: [smallScreen])
    try require(smallScreen.contains(resized), "display removal leaves the running window unreachable")
    try require(DesktopWindowPlacement.targetScreen(for: resized, screens: [smallScreen]) == smallScreen, "recovered frame uses the removed display's minimum")
    try require(DesktopWindowPlacement.targetScreen(for: normal, screens: []) == nil, "missing displays produce a synthetic placement")
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
