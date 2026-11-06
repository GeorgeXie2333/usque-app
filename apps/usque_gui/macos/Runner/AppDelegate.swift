import Cocoa
import FlutterMacOS

@main
class AppDelegate: FlutterAppDelegate {
  static func existingInstance() -> NSRunningApplication? {
    guard let identifier = Bundle.main.bundleIdentifier else { return nil }
    let current = NSRunningApplication.current
    return NSRunningApplication.runningApplications(withBundleIdentifier: identifier).first {
      guard $0.processIdentifier != current.processIdentifier, !$0.isTerminated else { return false }
      if let earlier = $0.launchDate, let started = current.launchDate { return earlier < started }
      return $0.processIdentifier < current.processIdentifier
    }
  }

  private var desktop: DesktopIntegration? {
    (mainFlutterWindow as? MainFlutterWindow)?.desktopIntegration
  }

  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    return false
  }

  override func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
    desktop?.shouldTerminate() ?? .terminateNow
  }

  override func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
    desktop?.showWindow()
    return true
  }

  override func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
    return true
  }
}
