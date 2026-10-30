import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  private(set) var desktopIntegration: DesktopIntegration?

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
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    desktopIntegration = DesktopIntegration(
      window: self, messenger: flutterViewController.engine.binaryMessenger)

    super.awakeFromNib()
  }
}
