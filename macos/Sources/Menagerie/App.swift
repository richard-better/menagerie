import AppKit
import MenagerieCore
import SwiftUI

@main struct MenagerieApp: App {
  @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
  var body: some Scene {
    let model = delegate.model
    return WindowGroup("Menagerie", id: "fleet") {
      FleetView(model: model).task { model.start() }
    }.defaultSize(width: 820, height: 760).defaultLaunchBehavior(
      ProcessInfo.processInfo.arguments.contains("--show-window") ? .presented : .suppressed)
  }
}
@MainActor final class AppDelegate: NSObject, NSApplicationDelegate {
  let model = Model()
  private var item: NSStatusItem!
  private let popover = NSPopover()
  private var terminating = false
  func applicationDidFinishLaunching(_ notification: Notification) {
    NSApp.setActivationPolicy(.accessory)
    NSApp.appearance = NSAppearance(named: .darkAqua)
    item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    item.button?.image =
      NSImage(contentsOfFile: Bundle.main.resourcePath!.appending("/menubar.png"))
      ?? NSImage(systemSymbolName: "diamond.fill", accessibilityDescription: "Menagerie")
    item.button?.image?.isTemplate = true
    if let image = item.button?.image {
      image.size = NSSize(width: image.size.width / max(1, image.size.height) * 18, height: 18)
    }
    item.button?.setAccessibilityLabel("Menagerie status")
    item.button?.target = self
    item.button?.action = #selector(click)
    item.button?.sendAction(on: [.leftMouseUp, .rightMouseUp])
    popover.behavior = .transient
    popover.contentViewController = NSHostingController(
      rootView: PopoverRoot(
        model: model, dismiss: { [weak self] in self?.popover.performClose(nil) }))
    model.start()
    if ProcessInfo.processInfo.arguments.contains("--show-window") { NSApp.activate() }
    observeStatus()
    if ProcessInfo.processInfo.arguments.contains("--show-popover") {
      DispatchQueue.main.async { [weak self] in
        guard let self, let button = self.item.button else { return }
        self.popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
      }
    }
  }
  func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
    guard !terminating else { return .terminateLater }
    terminating = true
    for job in model.jobs.values { job.cancel.cancel() }
    for terminal in model.terminals.values { terminal.transport.stop() }
    Task {
      try? await Task.sleep(for: .milliseconds(1200))
      let deadline = Date().addingTimeInterval(3)
      while model.jobs.values.contains(where: \.running), Date() < deadline {
        try? await Task.sleep(for: .milliseconds(100))
      }
      sender.reply(toApplicationShouldTerminate: true)
    }
    return .terminateLater
  }
  func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
  private func observeStatus() {
    withObservationTracking {
      item.button?.title = model.attention > 0 ? " \(model.attention)" : ""
    } onChange: { [weak self] in
      Task { @MainActor in self?.observeStatus() }
    }
  }
  @objc private func click() {
    if NSApp.currentEvent?.type == .rightMouseUp {
      let menu = NSMenu()
      let quit = menu.addItem(
        withTitle: "Quit Menagerie", action: #selector(NSApplication.terminate(_:)),
        keyEquivalent: "q")
      quit.target = NSApp
      item.menu = menu
      item.button?.performClick(nil)
      item.menu = nil
    } else if popover.isShown {
      popover.performClose(nil)
    } else if let button = item.button {
      popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
    }
  }
}
struct PopoverRoot: View {
  var model: Model
  var dismiss: () -> Void
  @Environment(\.openWindow) var openWindow
  var body: some View {
    FleetPopover(model: model) {
      dismiss()
      if let window = NSApp.windows.first(where: { $0.title == "Menagerie" && $0.canBecomeMain }) {
        window.makeKeyAndOrderFront(nil)
      } else {
        openWindow(id: "fleet")
      }
      NSApp.activate()
    }
  }
}
