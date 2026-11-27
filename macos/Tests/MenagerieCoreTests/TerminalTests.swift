import Darwin
import Foundation
import SwiftTerm
import Synchronization
import Testing

@testable import MenagerieCore

private final class EngineDelegate: TerminalDelegate {
  var responses: [UInt8] = []
  func send(source: Terminal, data: ArraySlice<UInt8>) { responses.append(contentsOf: data) }
}
@Test func terminalTracksWideGlyphsCursorAndAlternateScreen() {
  let delegate = EngineDelegate()
  let terminal = Terminal(delegate: delegate, options: TerminalOptions(cols: 40, rows: 10))
  terminal.feed(text: "A界B")
  #expect(terminal.getCharacter(col: 0, row: 0) == "A")
  #expect(terminal.getCharacter(col: 1, row: 0) == "界")
  #expect(terminal.getCharacter(col: 3, row: 0) == "B")
  #expect(terminal.getCursorLocation().x == 4)
  terminal.feed(text: "\u{1b}[?1049h\u{1b}[HALT")
  #expect(terminal.getCharacter(col: 0, row: 0) == "A")
  terminal.feed(text: "\u{1b}[?1049l")
  #expect(terminal.getCharacter(col: 1, row: 0) == "界")
  terminal.resize(cols: 80, rows: 24)
  #expect(terminal.cols == 80 && terminal.rows == 24)
}
@Test func terminalRefusesGraphicsBeforeTheParser() {
  let delegate = EngineDelegate()
  let terminal = Terminal(delegate: delegate)
  var filter = TerminalFilter()
  terminal.feed(byteArray: filter.filter(Array("before\u{1b}_Gpayload\u{1b}\\after".utf8)[...]))
  #expect(
    (0..<11).compactMap { terminal.getCharacter(col: $0, row: 0) }.map(String.init).joined()
      == "beforeafter")
}
private final class PTYDelegate: LocalProcessDelegate, @unchecked Sendable {
  let output = OutputBuffer()
  let finished = DispatchSemaphore(value: 0)
  let status = Mutex<Int32?>(nil)
  func dataReceived(slice: ArraySlice<UInt8>) {
    output.append(String(decoding: slice, as: UTF8.self))
  }
  func getWindowSize() -> winsize { winsize(ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0) }
  func processTerminated(_ source: LocalProcess, exitCode: Int32?) {
    status.withLock { $0 = exitCode }
    finished.signal()
  }
}
@Test func ptyRoundTripsAndDescriptorsCloseOnExec() throws {
  let delegate = PTYDelegate()
  let process = LocalProcess(
    delegate: delegate, dispatchQueue: DispatchQueue(label: "menagerie-pty-test"),
    directDelivery: true)
  process.startProcess(
    executable: "/bin/sh", args: ["-c", "read -r line; printf 'received:%s\\n' \"$line\""])
  guard case .success(let descriptor?) = process.duplicatePrimaryDescriptor() else {
    Issue.record("PTY did not start")
    return
  }
  #expect(fcntl(descriptor, F_GETFD) & FD_CLOEXEC != 0)
  close(descriptor)
  process.send(data: Array("round-trip\n".utf8)[...])
  #expect(delegate.finished.wait(timeout: .now() + 5) == .success)
  #expect(delegate.status.withLock { $0 } == 0)
  #expect(delegate.output.text.contains("received:round-trip"))
}
