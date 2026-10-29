import AuthenticationServices
import Cocoa
import FlutterMacOS
import ServiceManagement
import UniformTypeIdentifiers
import UserNotifications

/// Native shell only. Network state and actions continue through the shared
/// controller; a menu item cannot bypass an unavailable Engine capability.
final class DesktopIntegration: NSObject, NSWindowDelegate, UNUserNotificationCenterDelegate, ASWebAuthenticationPresentationContextProviding {
  private weak var window: NSWindow?
  private let channel: FlutterMethodChannel
  private let defaults: UserDefaults
  private var statusItem: NSStatusItem?
  private var tray = DesktopTrayState()
  private var localization = NativeLocalization()
  private let menuSources = NSMapTable<AnyObject, NativeMenuTitleRecord>(keyOptions: .weakMemory, valueOptions: .strongMemory)
  private var quit = DesktopQuitState()
  private var zeroTrust = DesktopZeroTrustSession()
  private var authenticationSession: ASWebAuthenticationSession?
  private let protonVerification = ProtonVerificationLauncher()
  private var dialogOpen = false
  private var lastVisibility: Bool?
  private var observers: [NSObjectProtocol] = []
  private let closePreference = "usque.closeToTray"

  init(window: NSWindow, messenger: FlutterBinaryMessenger, defaults: UserDefaults = .standard) {
    self.window = window
    self.defaults = defaults
    channel = FlutterMethodChannel(
      name: "io.github.georgexie2333.usque/engine", binaryMessenger: messenger)
    super.init()
    protonVerification.localization = localization
    tray.status = text(tray.status)
    tray.open = text(tray.open)
    tray.connect = text(tray.connect)
    tray.disconnect = text(tray.disconnect)
    tray.disconnectExit = text(tray.disconnectExit)
    tray.settings = text(tray.settings)
    tray.quitFailed = text(tray.quitFailed)
    defaults.register(defaults: [closePreference: true])
    window.isReleasedWhenClosed = false
    window.delegate = self
    _ = window.setFrameAutosaveName("UsqueMainWindow")
    _ = window.setFrameUsingName("UsqueMainWindow")
    window.setFrame(
      DesktopWindowPlacement.restoredFrame(window.frame, screens: NSScreen.screens.map { $0.visibleFrame }),
      display: false)
    channel.setMethodCallHandler { [weak self] call, result in self?.handle(call, result: result) }
    installStatusItem()
    localizeMainMenu()
    installSettingsMenu()
    UNUserNotificationCenter.current().delegate = self
    for name in [NSApplication.didHideNotification, NSApplication.didUnhideNotification] {
      observers.append(NotificationCenter.default.addObserver(forName: name, object: NSApp, queue: .main) {
        [weak self] _ in self?.publishVisibility()
      })
    }
  }

  deinit {
    for observer in observers { NotificationCenter.default.removeObserver(observer) }
    if let statusItem { NSStatusBar.system.removeStatusItem(statusItem) }
  }

  var visible: Bool { window?.isVisible == true && window?.isMiniaturized != true && !NSApp.isHidden }

  func showWindow() {
    guard let window else { return }
    NSApp.unhide(nil)
    if window.isMiniaturized { window.deminiaturize(nil) }
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    publishVisibility()
  }

  func shouldTerminate() -> NSApplication.TerminateReply {
    if quit.confirmed { return .terminateNow }
    guard let identifier = quit.begin() else { return .terminateLater }
    cancelZeroTrustLogin()
    protonVerification.cancel()
    channel.invokeMethod("trayCommand", arguments: "disconnectAndExit") { [weak self] value in
      guard let self else { return }
      let failed = value is FlutterError || (value as AnyObject?) === FlutterMethodNotImplemented
      self.finishQuit(identifier, success: !failed)
    }
    // Leave time for preference flushes plus the shared 120-second macOS Stop
    // request. A timeout cancels quitting and keeps the protection guard.
    DispatchQueue.main.asyncAfter(deadline: .now() + 140) { [weak self] in
      self?.finishQuit(identifier, success: false)
    }
    return .terminateLater
  }

  private func finishQuit(_ identifier: UUID, success: Bool) {
    guard quit.finish(identifier, success: success) else { return }
    NSApp.reply(toApplicationShouldTerminate: success)
    if !success {
      showWindow()
      let alert = NSAlert()
      alert.messageText = tray.quitFailed
      alert.addButton(withTitle: text("OK"))
      if let window { alert.beginSheetModal(for:window) }
    }
  }

  func windowShouldClose(_ sender: NSWindow) -> Bool {
    if defaults.bool(forKey: closePreference), statusItem != nil {
      sender.orderOut(nil)
      publishVisibility()
    } else {
      NSApp.terminate(nil)
    }
    return false
  }

  func windowDidMiniaturize(_ notification: Notification) { publishVisibility() }
  func windowDidDeminiaturize(_ notification: Notification) { publishVisibility() }
  func windowDidBecomeKey(_ notification: Notification) { publishVisibility() }

  private func publishVisibility() {
    let next = visible
    guard lastVisibility != next else { return }
    lastVisibility = next
    channel.invokeMethod("windowVisibilityChanged", arguments: next)
  }

  private func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    let values = call.arguments as? [String: Any]
    switch call.method {
    case "getWindowVisibility": result(visible)
    case "platformPreferences":
      result([
        "start_on_boot": SMAppService.mainApp.status == .enabled,
        "close_to_tray": defaults.bool(forKey: closePreference),
        "tray_available": statusItem != nil,
        "login_approval_required": SMAppService.mainApp.status == .requiresApproval,
      ])
    case "setCloseToTray":
      guard let enabled = values?["enabled"] as? Bool else { invalid(result); return }
      defaults.set(enabled, forKey: closePreference)
      result(nil)
    case "setStartOnBoot":
      guard let enabled = values?["enabled"] as? Bool else { invalid(result); return }
      if enabled {
        let bundle = Bundle.main.bundleURL.resolvingSymlinksInPath()
        let volume = try? bundle.resourceValues(forKeys: [.volumeIsReadOnlyKey])
        if bundle.path.hasPrefix("/Volumes/") || volume?.volumeIsReadOnly != false {
          result(FlutterError(code:"MACOS_INSTALL_REQUIRED",message:text("Drag Usque to Applications and open that copy before enabling login startup."),details:nil))
          return
        }
      }
      do {
        if enabled && SMAppService.mainApp.status != .enabled { try SMAppService.mainApp.register() }
        else if !enabled && SMAppService.mainApp.status != .notRegistered { try SMAppService.mainApp.unregister() }
        if enabled && SMAppService.mainApp.status == .requiresApproval {
          result(FlutterError(code: "MACOS_LOGIN_APPROVAL_REQUIRED", message: text("Approve Usque in Login Items, then retry."), details: nil))
        } else { result(nil) }
      } catch {
        result(FlutterError(code: "MACOS_LOGIN_ITEM_FAILED", message: text("macOS could not change the login item. Retry in System Settings."), details: nil))
      }
    case "updateTrayState":
      guard let state = DesktopTrayState(arguments: call.arguments) else { invalid(result); return }
      if let catalog = values?["catalog_id"] as? String {
        localization = NativeLocalization(catalogID: catalog)
        protonVerification.localization = localization
        localizeMainMenu()
      }
      tray = state
      updateStatusItem()
      installSettingsMenu()
      result(nil)
    case "showTrayNotification": showNotification(values, result: result)
    case "readChainConfigurations": readConfigurations(result)
    case "selectDiagnosticsDestination":
      selectDestination(name: "usque-diagnostics.zip", title: "Export diagnostics", type: .zip, result: result)
    case "selectWarpSecretDestination":
      selectDestination(name: "usque-warp-secret.json", title: "Export WARP credentials", type: .json, result: result)
    case "beginZeroTrustLogin":
      guard let team = values?["team_name"] as? String else { invalid(result); return }
      beginZeroTrustLogin(team, result: result)
    case "consumeZeroTrustCallback":
      let pending = zeroTrust.consume()
      if let code = pending.failure {
        result(FlutterError(code: code, message: text("Cloudflare login did not complete. Retry the login."), details: nil))
      } else { result(pending.callback) }
    case "cancelZeroTrustLogin": cancelZeroTrustLogin(); result(nil)
    case "completeProtonHumanVerification":
      guard let handoff = values?["handoff"] as? FlutterStandardTypedData,
        handoff.data.count <= ProtonVerificationContract.maximumHandoff else { invalid(result); return }
      protonVerification.begin(handoff.data, result: result)
    case "cancelProtonHumanVerification": protonVerification.cancel(); result(nil)
    default: result(FlutterMethodNotImplemented)
    }
  }

  private func invalid(_ result: FlutterResult) {
    result(FlutterError(code: "INVALID_ARGUMENT", message: text("The desktop shell request is malformed."), details: nil))
  }

  private func text(_ source: String) -> String { localization.text(source) }

  private func localizeMainMenu() {
    guard let main = NSApp.mainMenu else { return }
    func translatedTitle(_ object: AnyObject, current: String) -> String {
      let record = menuSources.object(forKey: object) ?? NativeMenuTitleRecord(current)
      menuSources.setObject(record, forKey: object)
      return record.title(current: current, localization: localization)
    }
    func update(_ menu: NSMenu) {
      menu.title = translatedTitle(menu, current: menu.title)
      for item in menu.items {
        item.title = translatedTitle(item, current: item.title)
        if let submenu = item.submenu { update(submenu) }
      }
    }
    update(main)
  }

  private func cancelZeroTrustLogin() {
    // Invalidate ownership before cancel(), whose completion can arrive late.
    zeroTrust.cancel()
    let old = authenticationSession
    authenticationSession = nil
    old?.cancel()
  }

  private func beginZeroTrustLogin(_ team: String, result: @escaping FlutterResult) {
    guard DesktopZeroTrustSession.normalizedTeam(team) != nil else {
      result(FlutterError(code: "ZERO_TRUST_TEAM_INVALID", message: text("Enter one Cloudflare Zero Trust team name."), details: nil)); return
    }
    cancelZeroTrustLogin()
    guard let login = zeroTrust.begin(team) else { invalid(result); return }
    showWindow()
    let completion: ASWebAuthenticationSession.CompletionHandler = { [weak self] callback, error in
      DispatchQueue.main.async {
        guard let self, self.zeroTrust.attempt == login.identifier else { return }
        self.authenticationSession = nil
        if let callback, self.zeroTrust.accept(callback.absoluteString, for: login.identifier) {
          self.channel.invokeMethod("zeroTrustCallbackArrived", arguments: nil)
        } else {
          let cancelled = (error as? ASWebAuthenticationSessionError)?.code == .canceledLogin
          _ = self.zeroTrust.fail(login.identifier, code: cancelled ? "ZERO_TRUST_LOGIN_CANCELLED" : "ZERO_TRUST_CALLBACK_INVALID")
          self.channel.invokeMethod("zeroTrustCallbackArrived", arguments: nil)
        }
      }
    }
    let session: ASWebAuthenticationSession
    if #available(macOS 14.4, *) {
      session = ASWebAuthenticationSession(url: login.url, callback: .customScheme("com.cloudflare.warp"), completionHandler: completion)
    } else {
      session = ASWebAuthenticationSession(url: login.url, callbackURLScheme: "com.cloudflare.warp", completionHandler: completion)
    }
    session.presentationContextProvider = self
    authenticationSession = session
    guard session.start() else {
      cancelZeroTrustLogin()
      result(FlutterError(code: "ZERO_TRUST_BROWSER_FAILED", message: text("macOS could not start the authentication browser. Retry the login."), details: nil)); return
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + 300) { [weak self] in
      guard let self, self.zeroTrust.attempt == login.identifier else { return }
      let active = self.authenticationSession
      self.authenticationSession = nil
      _ = self.zeroTrust.fail(login.identifier, code: "ZERO_TRUST_LOGIN_TIMEOUT")
      active?.cancel()
      self.channel.invokeMethod("zeroTrustCallbackArrived", arguments: nil)
    }
    result(login.url.absoluteString)
  }

  func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
    // The authentication session is only created while the main window exists.
    window ?? NSWindow()
  }

  private func installStatusItem() {
    statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    statusItem?.button?.setAccessibilityLabel("Usque")
    updateStatusItem()
  }

  private func updateStatusItem() {
    guard let statusItem else { return }
    let symbol: String
    switch tray.badge {
    case "connected": symbol = "checkmark.shield.fill"
    case "warning": symbol = "exclamationmark.shield.fill"
    case "error": symbol = "xmark.shield.fill"
    case "busy": symbol = "arrow.triangle.2.circlepath"
    default: symbol = "shield.lefthalf.filled"
    }
    let image = NSImage(systemSymbolName: symbol, accessibilityDescription: "Usque")
    image?.isTemplate = true
    statusItem.button?.image = image
    statusItem.button?.toolTip = "Usque — \(tray.status)"
    statusItem.button?.setAccessibilityValue(tray.status)
    let menu = NSMenu()
    menu.autoenablesItems = false
    let status = NSMenuItem(title: tray.status, action: nil, keyEquivalent: "")
    status.isEnabled = false
    menu.addItem(status)
    menu.addItem(.separator())
    menu.addItem(item(tray.open, #selector(openWindow)))
    menu.addItem(item(tray.connected ? tray.disconnect : tray.connect, #selector(toggleConnection)))
    if !tray.tunnelLabel.isEmpty || !tray.systemProxyLabel.isEmpty { menu.addItem(.separator()) }
    if !tray.tunnelLabel.isEmpty {
      let tunnel = item(tray.tunnelLabel, #selector(toggleTunnel))
      tunnel.state = tray.tunnel ? .on : .off
      tunnel.isEnabled = tray.canToggleTunnel
      menu.addItem(tunnel)
    }
    if !tray.systemProxyLabel.isEmpty {
      let proxy = item(tray.systemProxyLabel, #selector(toggleSystemProxy))
      proxy.state = tray.systemProxy ? .on : .off
      proxy.isEnabled = tray.canToggleSystemProxy
      menu.addItem(proxy)
    }
    menu.addItem(.separator())
    menu.addItem(item(tray.disconnectExit, #selector(quitApplication), key: "q"))
    statusItem.menu = menu
  }

  private func item(_ title: String, _ action: Selector, key: String = "") -> NSMenuItem {
    let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
    item.target = self
    return item
  }

  private func installSettingsMenu() {
    guard let menu = NSApp.mainMenu?.items.first?.submenu,
      let settings = menu.items.first(where: { $0.keyEquivalent == "," }) else { return }
    settings.title = tray.settings
    settings.action = #selector(openSettings)
    settings.target = self
  }

  @objc private func openWindow() { showWindow() }
  @objc private func openSettings() {
    showWindow()
    channel.invokeMethod("openSettings", arguments: nil)
  }
  @objc private func toggleConnection() { channel.invokeMethod("trayCommand", arguments: "toggle") }
  @objc private func toggleTunnel() {
    if tray.canToggleTunnel { channel.invokeMethod("trayCommand", arguments: "toggleTunnel") }
  }
  @objc private func toggleSystemProxy() {
    if tray.canToggleSystemProxy { channel.invokeMethod("trayCommand", arguments: "toggleSystemProxy") }
  }
  @objc private func quitApplication() { NSApp.terminate(nil) }

  private func showNotification(_ values: [String: Any]?, result: @escaping FlutterResult) {
    guard let title = values?["title"] as? String, !title.isEmpty, title.utf8.count <= 1024,
      let body = values?["body"] as? String, !body.isEmpty, body.utf8.count <= 4096,
      let level = values?["level"] as? String, ["info", "warning", "error"].contains(level)
    else { invalid(result); return }
    let center = UNUserNotificationCenter.current()
    center.requestAuthorization(options: [.alert, .sound]) { granted, _ in
      guard granted else { DispatchQueue.main.async { result(false) }; return }
      let content = UNMutableNotificationContent()
      content.title = title
      content.body = body
      content.threadIdentifier = "usque.connection"
      let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
      center.add(request) { error in DispatchQueue.main.async { result(error == nil) } }
    }
  }

  func userNotificationCenter(_ center: UNUserNotificationCenter,
    willPresent notification: UNNotification,
    withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
    completionHandler([.banner])
  }

  func userNotificationCenter(_ center: UNUserNotificationCenter,
    didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
    DispatchQueue.main.async { [weak self] in self?.showWindow(); completionHandler() }
  }

  private func readConfigurations(_ result: @escaping FlutterResult) {
    guard !dialogOpen, let window else {
      result(FlutterError(code: "CHAIN_FILE_BUSY", message: text("A file picker is already open."), details: nil)); return
    }
    dialogOpen = true
    showWindow()
    let panel = NSOpenPanel()
    panel.title = text("Import configurations")
    panel.message = text("Select OpenVPN or WireGuard configuration files.")
    panel.prompt = text("Import")
    panel.allowsMultipleSelection = true
    panel.canChooseDirectories = false
    panel.canChooseFiles = true
    panel.allowedContentTypes = [UTType(filenameExtension: "ovpn"), UTType(filenameExtension: "conf")].compactMap { $0 }
    panel.allowsOtherFileTypes = true
    panel.beginSheetModal(for: window) { [weak self] response in
      self?.dialogOpen = false
      guard response == .OK else { result(nil); return }
      let urls = panel.urls
      guard urls.count <= DesktopConfigurationFiles.maximumCount else {
        result(FlutterError(code: "CHAIN_FILE_COUNT_LIMIT", message: self?.text("Select at most 128 configuration files.") ?? "Select at most 128 configuration files.", details: nil)); return
      }
      DispatchQueue.global(qos: .userInitiated).async {
        let files: [[String: Any]] = urls.map { url in
          let file = DesktopConfigurationFiles.read(url)
          if let bytes = file.bytes { return ["name": file.name, "bytes": FlutterStandardTypedData(bytes: bytes)] }
          return ["name": file.name, "error": file.error ?? "CHAIN_FILE_READ_FAILED"]
        }
        DispatchQueue.main.async { result(files) }
      }
    }
  }

  private func selectDestination(name: String, title: String, type: UTType, result: @escaping FlutterResult) {
    guard !dialogOpen, let window else {
      result(FlutterError(code: "DESTINATION_BUSY", message: text("A file picker is already open."), details: nil)); return
    }
    dialogOpen = true
    showWindow()
    let panel = NSSavePanel()
    panel.title = text(title)
    panel.prompt = text("Save")
    panel.nameFieldStringValue = name
    panel.allowedContentTypes = [type]
    panel.canCreateDirectories = true
    panel.beginSheetModal(for: window) { [weak self] response in
      self?.dialogOpen = false
      result(response == .OK ? panel.url?.path : nil)
    }
  }
}
