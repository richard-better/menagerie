import AppKit
import MenagerieCore
import SwiftUI

@main struct MenagerieApp: App {
  @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
  var body: some Scene {
    let model = delegate.model
    return WindowGroup("Menagerie", id: "fleet") {
      FleetView(model: model).task { model.start() }
        .onDisappear {
          model.closeWindow()
          ServiceIcon.releaseImages()
        }
        .background(
          WindowLifetime(
            live: model.observations[model.selected]?.report != nil,
            sequence: WindowTiming.shared.sequence))
    }.defaultSize(width: 820, height: 760).defaultLaunchBehavior(
      ProcessInfo.processInfo.arguments.contains("--show-window") ? .presented : .suppressed)
  }
}
@MainActor final class AppDelegate: NSObject, NSApplicationDelegate, NSPopoverDelegate {
  let model = Model()
  private var item: NSStatusItem!
  private let popover = NSPopover()
  private var terminating = false
  private var benchmarkSource: DispatchSourceSignal?
  private var benchmarkAnchor: NSWindow?
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
    popover.delegate = self
    model.start()
    if ProcessInfo.processInfo.arguments.contains("--show-window") { NSApp.activate() }
    observeStatus()
    if ProcessInfo.processInfo.environment["MENAGERIE_WINDOW_TRACE"] != nil {
      signal(SIGUSR1, SIG_IGN)
      let source = DispatchSource.makeSignalSource(signal: SIGUSR1, queue: .main)
      source.setEventHandler { [weak self] in
        MainActor.assumeIsolated {
          guard let path = ProcessInfo.processInfo.environment["MENAGERIE_WINDOW_TRACE"],
            let command = try? String(contentsOfFile: path + ".command", encoding: .utf8)
          else { return }
          if command == "open" {
            if let self {
              NSApp.activate()
              if self.benchmarkAnchor == nil {
                let anchor = NSWindow(
                  contentRect: NSRect(x: 20, y: 20, width: 1, height: 1), styleMask: .borderless,
                  backing: .buffered, defer: false)
                anchor.isOpaque = false
                anchor.backgroundColor = .clear
                anchor.isReleasedWhenClosed = false
                anchor.orderFront(nil)
                self.benchmarkAnchor = anchor
              }
              self.preparePopover()
              if let view = self.benchmarkAnchor?.contentView {
                self.popover.behavior = .applicationDefined
                self.popover.show(relativeTo: view.bounds, of: view, preferredEdge: .maxY)
                WindowTiming.shared.record("popover-shown \(self.popover.isShown)")
                precondition(self.popover.isShown, "Benchmark requires a visible popover")
              }
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
              NotificationCenter.default.post(name: .init("MenagerieBenchmarkOpen"), object: nil)
            }
          } else if command == "close" {
            if let window = NSApp.windows.first(where: {
              $0.title == "Menagerie" && $0.canBecomeMain
            }) {
              if ProcessInfo.processInfo.environment["MENAGERIE_KEEP_WINDOW"] == "1" {
                window.orderOut(nil)
                WindowTiming.shared.record("window-hidden")
              } else {
                window.performClose(nil)
              }
            }
          }
          _ = self
        }
      }
      benchmarkSource = source
      source.resume()
    }
    if ProcessInfo.processInfo.arguments.contains("--show-popover") {
      Task { @MainActor [weak self] in
        try? await Task.sleep(for: .milliseconds(500))
        guard let self, let button = self.item.button else { return }
        NSApp.activate()
        self.preparePopover()
        self.popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
      }
    }
  }
  private func preparePopover() {
    guard popover.contentViewController == nil else { return }
    let hosting = NSHostingController(
      rootView: PopoverRoot(
        model: model, dismiss: { [weak self] in self?.popover.performClose(nil) }))
    hosting.sizingOptions = .preferredContentSize
    popover.contentViewController = hosting
  }
  func popoverDidClose(_ notification: Notification) {
    popover.contentViewController = nil
    if !NSApp.windows.contains(where: { $0.title == "Menagerie" && $0.isVisible }) {
      ServiceIcon.releaseImages()
    }
    WindowTiming.shared.record("popover-released")
  }
  func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
    guard !terminating else { return .terminateLater }
    terminating = true
    for job in model.jobs.values { job.cancel.cancel() }
    PTYSession.killAll()
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
      item.button?.title =
        (ProcessInfo.processInfo.environment["MENAGERIE_TEST_TITLE"] ?? "")
        + (model.attention > 0 ? " \(model.attention)" : "")
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
      NSApp.activate()
      preparePopover()
      popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
    }
  }
}
struct PopoverRoot: View {
  var model: Model
  var dismiss: () -> Void
  @Environment(\.openWindow) var openWindow
  var body: some View {
    FleetPopover(model: model, open: openFleet)
      .onReceive(NotificationCenter.default.publisher(for: .init("MenagerieBenchmarkOpen"))) { _ in
        openFleet()
      }
  }
  private func openFleet() {
    WindowTiming.shared.begin()
    dismiss()
    if let window = NSApp.windows.first(where: { $0.title == "Menagerie" && $0.canBecomeMain }) {
      window.makeKeyAndOrderFront(nil)
      DispatchQueue.main.async {
        WindowTiming.shared.display(window, live: model.observations[model.selected]?.report != nil)
      }
    } else {
      openWindow(id: "fleet")
    }
    NSApp.activate()
  }
}
