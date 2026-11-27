import AppKit
import SwiftUI
import MenagerieCore

@main struct MenagerieApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
    var body: some Scene {
        Window("Menagerie", id: "fleet") {
            FleetView(model: delegate.model).task { delegate.model.start() }
        }.defaultSize(width: 820, height: 760).defaultLaunchBehavior(.suppressed)
    }
}
@MainActor final class AppDelegate: NSObject, NSApplicationDelegate {
    let model = Model()
    private var item: NSStatusItem!
    private let popover = NSPopover()
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        NSApp.appearance = NSAppearance(named: .darkAqua)
        item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item.button?.image = NSImage(contentsOfFile: Bundle.main.resourcePath!.appending("/menubar.png")) ?? NSImage(systemSymbolName: "diamond.fill", accessibilityDescription: "Menagerie")
        item.button?.image?.isTemplate = true
        item.button?.image?.size = NSSize(width: 18, height: 18)
        item.button?.target = self; item.button?.action = #selector(click)
        item.button?.sendAction(on: [.leftMouseUp, .rightMouseUp])
        popover.behavior = .transient
        popover.contentViewController = NSHostingController(rootView: PopoverRoot(model: model, dismiss: { [weak self] in self?.popover.performClose(nil) }))
        model.start()
        observeStatus()
    }
    private func observeStatus() {
        withObservationTracking {
            item.button?.title = model.attention > 0 ? " \(model.attention)" : ""
        } onChange: { [weak self] in Task { @MainActor in self?.observeStatus() } }
    }
    @objc private func click() {
        if NSApp.currentEvent?.type == .rightMouseUp {
            let menu = NSMenu()
            let quit = menu.addItem(withTitle: "Quit Menagerie", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"); quit.target = NSApp
            item.menu = menu; item.button?.performClick(nil); item.menu = nil
        } else if popover.isShown { popover.performClose(nil) }
        else if let button = item.button { popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY) }
    }
}
struct PopoverRoot: View {
    var model: Model
    var dismiss: () -> Void
    @Environment(\.openWindow) var openWindow
    var body: some View {
        FleetPopover(model: model) { dismiss(); openWindow(id: "fleet"); NSApp.activate() }
    }
}
