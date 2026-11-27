import AppKit
import QuartzCore
import SwiftUI

@MainActor final class WindowTiming {
  static let shared = WindowTiming()
  var sequence = 0
  private var started: CFTimeInterval?
  func begin() {
    guard ProcessInfo.processInfo.environment["MENAGERIE_WINDOW_TRACE"] != nil else { return }
    sequence += 1
    started = CACurrentMediaTime()
    record("open \(Date().timeIntervalSince1970)")
  }
  func display(_ window: NSWindow, live: Bool) {
    guard live, window.isVisible, let started else { return }
    self.started = nil
    window.contentView?.layoutSubtreeIfNeeded()
    window.displayIfNeeded()
    CATransaction.flush()
    record("display \(Date().timeIntervalSince1970)")
    record("frame", milliseconds: (CACurrentMediaTime() - started) * 1000)
  }
  func record(_ event: String, milliseconds: Double = 0) {
    guard let path = ProcessInfo.processInfo.environment["MENAGERIE_WINDOW_TRACE"] else { return }
    let line = "\(event) \(sequence) \(milliseconds)\n"
    if let file = FileHandle(forWritingAtPath: path) {
      file.seekToEndOfFile()
      file.write(Data(line.utf8))
      try? file.close()
    }
  }
}
struct WindowLifetime: NSViewRepresentable {
  var live: Bool
  var sequence: Int
  func makeNSView(context: Context) -> Probe { Probe() }
  func updateNSView(_ view: Probe, context: Context) {
    view.live = live
    view.scheduleDisplay()
  }
  final class Probe: NSView {
    var live = false
    private weak var lastWindow: NSWindow?
    override func viewDidMoveToWindow() {
      super.viewDidMoveToWindow()
      if let window { lastWindow = window }
      scheduleDisplay()
    }
    func scheduleDisplay() {
      guard ProcessInfo.processInfo.environment["MENAGERIE_WINDOW_TRACE"] != nil else { return }
      DispatchQueue.main.async { [weak self] in
        guard let self, let window = self.window else { return }
        WindowTiming.shared.display(window, live: self.live)
      }
    }
    deinit {
      MainActor.assumeIsolated {
        WindowTiming.shared.record("probe-released")
        Task { @MainActor [weak lastWindow] in
          try? await Task.sleep(for: .seconds(2))
          WindowTiming.shared.record(lastWindow == nil ? "window-released" : "window-retained")
        }
      }
    }
  }
}
