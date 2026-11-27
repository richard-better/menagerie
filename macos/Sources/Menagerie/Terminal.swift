import AppKit
import MenagerieCore
import SwiftTerm
import SwiftUI
import Synchronization

// LocalProcess synchronizes its transport internally; delegate delivery is serial.
final class TerminalTransport: LocalProcessDelegate, @unchecked Sendable {
  private struct State {
    var size = winsize(ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0)
    var filter = TerminalFilter()
    var pendingBytes = 0
    var pendingWrites = 0
    var closed = false
  }
  private let state = Mutex(State())
  private let sender: TerminalFeedSender
  private var process: LocalProcess!
  init(sender: TerminalFeedSender) {
    self.sender = sender
    process = LocalProcess(
      delegate: self, dispatchQueue: DispatchQueue(label: "me.richardb.menagerie.terminal"),
      directDelivery: true)
    process.killEscalationDelay = 1
  }
  func start(machine: Machine, name: String, directory: String?) {
    DispatchQueue.global().async { [self] in
      let process = self.process!
      guard !state.withLock({ $0.closed }) else { return }
      if Deployment.isLocal(name, machine) {
        let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
        process.startProcess(executable: shell, args: ["-l"], currentDirectory: directory)
      } else if let ssh = machine.ssh {
        process.startProcess(executable: "/usr/bin/ssh", args: ["-tt", "--", ssh])
      }
    }
  }
  func resize(cols: Int, rows: Int) {
    var size = winsize(
      ws_row: UInt16(clamping: rows), ws_col: UInt16(clamping: cols), ws_xpixel: 0, ws_ypixel: 0)
    state.withLock { $0.size = size }
    _ = process.updateWindowSize(&size)
  }
  func send(_ data: ArraySlice<UInt8>) {
    guard data.count <= 256 * 1024 + 12 else { return }
    let accepted = state.withLock { state in
      guard !state.closed, state.pendingBytes + data.count <= 1024 * 1024, state.pendingWrites < 256
      else { return false }
      state.pendingBytes += data.count
      state.pendingWrites += 1
      return true
    }
    guard accepted else { return }
    let count = data.count
    process.send(data: data) { [weak self] _ in
      self?.state.withLock {
        $0.pendingBytes -= count
        $0.pendingWrites -= 1
      }
    }
  }
  func stop() {
    let closed = state.withLock { state in
      let old = state.closed
      state.closed = true
      return old
    }
    guard !closed else { return }
    let pid = process.shellPid
    if pid > 0 {
      kill(-pid, SIGHUP)
      kill(-pid, SIGTERM)
    }
    process.terminate()
    let handle = ProcessHandle(process: process!)
    DispatchQueue.global().asyncAfter(deadline: .now() + 1) {
      if pid > 0 { kill(-pid, SIGKILL) }
      _ = handle.process.running
    }
  }
  func dataReceived(slice: ArraySlice<UInt8>) {
    let filtered = state.withLock { $0.filter.filter(slice) }
    sender.feed(byteArray: filtered[...])
  }
  func getWindowSize() -> winsize { state.withLock { $0.size } }
  func processTerminated(_ source: LocalProcess, exitCode: Int32?) {
    sender.feed(text: "\r\nSession ended (\(exitCode.map(String.init) ?? "signal"))\r\n")
  }
  deinit { stop() }
}
@MainActor final class TerminalSession: TerminalViewDelegate {
  let view: TerminalView
  let transport: TerminalTransport
  init(machine: Machine, name: String, directory: String?) {
    var options = TerminalOptions.default
    options.scrollback = 2000
    options.maximumOscBytes = 65536
    options.enableSixelReported = false
    view = TerminalView(
      frame: CGRect(x: 0, y: 0, width: 700, height: 450),
      font: NSFont.monospacedSystemFont(ofSize: 12, weight: .regular), options: options)
    view.nativeBackgroundColor = NSColor(
      srgbRed: 14 / 255, green: 19 / 255, blue: 17 / 255, alpha: 1)
    view.nativeForegroundColor = NSColor(
      srgbRed: 240 / 255, green: 238 / 255, blue: 232 / 255, alpha: 1)
    transport = TerminalTransport(sender: view.feedSender)
    view.terminalDelegate = self
    transport.start(machine: machine, name: name, directory: directory)
  }
  func send(source: TerminalView, data: ArraySlice<UInt8>) { transport.send(data) }
  func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
    transport.resize(cols: newCols, rows: newRows)
  }
  func setTerminalTitle(source: TerminalView, title: String) {}
  func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
  func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
  func scrolled(source: TerminalView, position: Double) {}
}
struct TerminalPage: NSViewRepresentable {
  let session: TerminalSession
  func makeNSView(context: Context) -> TerminalView { session.view }
  func updateNSView(_ nsView: TerminalView, context: Context) {}
}

private struct ProcessHandle: @unchecked Sendable { let process: LocalProcess }
