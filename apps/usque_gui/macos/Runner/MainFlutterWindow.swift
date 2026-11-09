import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  private(set) var desktopIntegration: DesktopIntegration?
  private var frameChannel: FlutterMethodChannel?
  private var frameObservers: [NSObjectProtocol] = []
  // Keep aligned with kWindowTitleBarHeight in the shared Flutter caption.
  private let captionHeight: CGFloat = 40

  override func awakeFromNib() {
    if let existing = AppDelegate.existingInstance() {
      orderOut(nil)
      existing.activate(options: [.activateAllWindows])
      DispatchQueue.main.async { NSApp.terminate(nil) }
      super.awakeFromNib()
      return
    }
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    styleMask.insert(.fullSizeContentView)
    titleVisibility = .hidden
    titlebarAppearsTransparent = true
    isMovableByWindowBackground = false
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    let channel = FlutterMethodChannel(
      name: "io.github.georgexie2333.usque/window_frame",
      binaryMessenger: flutterViewController.engine.binaryMessenger)
    frameChannel = channel
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self else { result(nil); return }
      if call.method == "windowFrameState" {
        result(self.captionState)
      } else {
        result(FlutterMethodNotImplemented)
      }
    }
    for name in [NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification,
      NSWindow.didResizeNotification, NSWindow.didEnterFullScreenNotification,
      NSWindow.didExitFullScreenNotification] {
      frameObservers.append(NotificationCenter.default.addObserver(
        forName: name, object: self, queue: .main
      ) { [weak self] _ in
        guard let self else { return }
        self.frameChannel?.invokeMethod("windowFrameChanged", arguments: self.captionState)
      })
    }
    desktopIntegration = DesktopIntegration(
      window: self, messenger: flutterViewController.engine.binaryMessenger)

    super.awakeFromNib()
  }

  private var captionState: [String: Any] {
    ["maximized": isZoomed || styleMask.contains(.fullScreen),
     "active": isKeyWindow, "captionHover": "none"]
  }

  override func sendEvent(_ event: NSEvent) {
    if event.type == .leftMouseDown, event.window === self, let contentView {
      let point = contentView.convert(event.locationInWindow, from: nil)
      // AppKit owns the traffic lights and their accessibility, close-to-tray,
      // minimize and full-screen behavior. Only the remaining caption drags.
      let top = contentView.isFlipped ? point.y : contentView.bounds.height - point.y
      if point.x >= 90 && top >= 0 && top < captionHeight {
        if event.clickCount == 2 {
          switch UserDefaults.standard.string(forKey: "AppleActionOnDoubleClick") {
          case "Minimize": performMiniaturize(nil)
          case "None": break
          default: performZoom(nil)
          }
        } else {
          performDrag(with: event)
        }
        return
      }
    }
    super.sendEvent(event)
  }

  deinit {
    for observer in frameObservers { NotificationCenter.default.removeObserver(observer) }
  }
}
