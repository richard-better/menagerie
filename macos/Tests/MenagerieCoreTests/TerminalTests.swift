import Darwin
import Foundation
import Synchronization
import Testing

@testable import MenagerieCore

@Test func terminalKeysBecomeBytes() {
  #expect(TerminalInput.key("enter") == Data([13]))
  #expect(TerminalInput.key("up") == Data("\u{1b}[A".utf8))
  #expect(TerminalInput.key("up", applicationCursor: true) == Data("\u{1b}OA".utf8))
  #expect(TerminalInput.key("c", control: true) == Data([3]))
  #expect(TerminalInput.key("b", alt: true) == Data("\u{1b}b".utf8))
  #expect(TerminalInput.key("c", command: true) == nil)
  #expect(TerminalInput.key("2", text: "é") == Data("é".utf8))
  #expect(TerminalInput.key("backspace") == Data([127]))
  #expect(TerminalInput.key("delete") == Data("\u{1b}[3~".utf8))
}
@Test func gridFitsSpaceInsidePadding() {
  let size = TerminalInput.fit(width: 820, height: 352, cellWidth: 8)
  #expect(size.columns == 100 && size.rows == 20)
  let small = TerminalInput.fit(width: 30, height: 10, cellWidth: 8)
  #expect(small.columns == 20 && small.rows == 4)
}
@Test func bracketedPastesCannotEscapeAndRespectCap() {
  #expect(TerminalInput.paste("a\nb", bracketed: false) == Data("a\nb".utf8))
  #expect(TerminalInput.paste("a\nb", bracketed: true) == Data("\u{1b}[200~a\nb\u{1b}[201~".utf8))
  let tricky = TerminalInput.paste("\u{1b}[20\u{1b}[201~1~ls", bracketed: true)
  #expect(!tricky.dropFirst(6).dropLast(6).contains(27))
  let capped = TerminalInput.paste(String(repeating: "界", count: 100000), bracketed: true)
  #expect(capped.count <= TerminalInput.pasteLimit + 12)
  #expect(String(data: capped, encoding: .utf8) != nil)
}
@Test func ghosttyTracksModesWideGlyphsAndAlternateScreen() throws {
  let terminal = try GhosttyScreen(columns: 10, rows: 2)
  #expect(!terminal.mode(1))
  terminal.write("a界b")
  let frame = terminal.snapshot()
  #expect(frame.glyphs.prefix(3).map(\.column) == [0, 1, 3])
  #expect(frame.glyphs.prefix(3).map(\.width) == [1, 2, 1])
  #expect(frame.glyphs.prefix(3).map(\.text) == ["a", "界", "b"])
  #expect(frame.cursor?.column == 4 && frame.cursor?.row == 0)
  terminal.write("\u{1b}[?1h\u{1b}[?2004h")
  #expect(terminal.mode(1) && terminal.mode(2004))
  terminal.write("\u{1b}[?1049h\u{1b}[HALT")
  #expect(terminal.snapshot().text.hasPrefix("ALT"))
  terminal.write("\u{1b}[?1049l")
  #expect(terminal.snapshot().text.hasPrefix("a界b"))
  try terminal.resize(columns: 80, rows: 24)
  #expect(terminal.snapshot().columns == 80)
}
@Test func ghosttyRefusesGraphicsWithoutBufferingPayloads() throws {
  let terminal = try GhosttyScreen(columns: 20, rows: 2)
  terminal.write("\u{1b}_Ga=T,f=24;" + String(repeating: "A", count: 1 << 20) + "\u{1b}\\after")
  #expect(terminal.snapshot().text.trimmingCharacters(in: .whitespacesAndNewlines) == "after")
}
@Test func ghosttyKeepsColorsCursorAndReplies() throws {
  let terminal = try GhosttyScreen(columns: 20, rows: 3)
  terminal.write("ab\u{1b}[31mred\u{1b}[0m\r\nnext\u{1b}[6n")
  let frame = terminal.snapshot()
  #expect(frame.text.hasPrefix("abred\nnext"))
  let red = frame.glyphs.first { $0.text == "r" }?.foreground
  #expect(red.map { ($0 >> 16 & 255) > ($0 >> 8 & 255) } == true)
  #expect(frame.cursor?.column == 4 && frame.cursor?.row == 1)
  #expect(terminal.replies() == Data("\u{1b}[2;5R".utf8))
}
@Test func selectionUsesColumnsAndCopiesWideCharactersOnce() throws {
  let terminal = try GhosttyScreen(columns: 10, rows: 2)
  terminal.write("a界b\r\nsecond")
  let frame = terminal.snapshot()
  #expect(frame.selectedText(from: 1, through: 3) == "界b")
  #expect(frame.selectedText(from: 2, through: 2) == "界")
  #expect(frame.selectedText(from: 3, through: 11) == "b\nse")
}
@Test func writeBudgetIncludesInFlightInputAndCloses() {
  let queue = TerminalWriteQueue()
  #expect(queue.enqueue(Data(repeating: 0, count: 1 << 20)))
  #expect(!queue.enqueue(Data([1])))
  _ = queue.first()
  #expect(queue.pendingBytes == 1 << 20)
  queue.complete()
  #expect(queue.pendingBytes == 0)
  for _ in 0..<256 { #expect(queue.enqueue(Data([1]))) }
  #expect(!queue.enqueue(Data([1])))
  queue.close()
  #expect(!queue.enqueue(Data([1])))
  #expect(queue.pendingBytes == 0)
}
@Test func terminalOutputIsASingleLatestFrame() {
  let channel = TerminalOutput()
  for index in 0..<10000 {
    var frame = TerminalFrame()
    frame.text = String(index)
    channel.publish(frame)
  }
  #expect(channel.latest(after: 0)?.1.text == "9999")
  #expect(channel.latest(after: 10000) == nil)
}
@Suite(.serialized) struct TerminalLifecycleTests {
  private func wait(_ predicate: () -> Bool, timeout: TimeInterval = 5) throws {
    let deadline = Date().addingTimeInterval(timeout)
    while !predicate() && Date() < deadline { usleep(10000) }
    #expect(predicate())
  }
  @Test func shellOnPTYRoundTripsAndDescriptorsAreNonblockingCloseOnExec() throws {
    let session = PTYSession()
    session.start(Command("/bin/sh", ["-c", "printf 'name? '; read -r x; echo \"hello $x\""]))
    defer { session.stop() }
    try wait { session.processID > 0 }
    guard let fd = session.duplicateDescriptor() else {
      Issue.record("PTY unavailable")
      return
    }
    #expect(fcntl(fd, F_GETFD) & FD_CLOEXEC != 0)
    #expect(fcntl(fd, F_GETFL) & O_NONBLOCK != 0)
    close(fd)
    session.input.enqueue(Data("pty\r".utf8))
    try wait { session.finished }
    #expect(session.exitStatus == 0)
    #expect(session.output.latest(after: 0)?.1.text.contains("hello pty") == true)
  }
  @Test func closeKillsShellAndChildrenIgnoringHangupAndTerm() throws {
    let session = PTYSession()
    session.start(Command("/bin/sh", ["-c", "trap '' HUP TERM; echo ready; sleep 30 & wait"]))
    try wait { session.output.latest(after: 0)?.1.text.contains("ready") == true }
    let pid = session.processID
    let start = Date()
    session.stop()
    try wait { session.finished }
    #expect(Date().timeIntervalSince(start) >= 0.9)
    #expect(kill(pid, 0) == -1 && errno == ESRCH)
    #expect(kill(-pid, 0) == -1 && errno == ESRCH)
  }
  @Test func killAllDoesNotWaitForGracePeriod() throws {
    let sessions = (0..<2).map { _ in PTYSession() }
    for session in sessions {
      session.start(Command("/bin/sh", ["-c", "trap '' HUP TERM; echo ready; sleep 30 & wait"]))
    }
    try wait {
      sessions.allSatisfy { $0.output.latest(after: 0)?.1.text.contains("ready") == true }
    }
    let start = Date()
    PTYSession.killAll()
    try wait { sessions.allSatisfy(\.finished) }
    #expect(Date().timeIntervalSince(start) < 0.9)
  }
  @Test func closeBeforeStartNeverSpawnsAndStartIsSingleUse() throws {
    let session = PTYSession()
    session.stop()
    session.start(Command("/bin/sh", ["-c", "exit 99"]))
    session.start(Command("/bin/sh", ["-c", "exit 99"]))
    try wait { session.finished }
    #expect(session.exitStatus == nil)
    #expect(session.processID == 0)
  }
  @Test func normalShellExitReapsItsBackgroundChildren() throws {
    let session = PTYSession()
    session.start(Command("/bin/sh", ["-c", "sleep 30 & echo ready; exit 7"]))
    try wait { session.finished }
    #expect(session.exitStatus == 7)
  }
  @Test func ptyHasAControllingTerminalForJobControl() throws {
    let session = PTYSession()
    session.start(
      Command("/bin/sh", ["-c", "/bin/stty -f /dev/tty size && echo controlling-terminal-ready"]))
    let deadline = Date().addingTimeInterval(5)
    while !session.finished && Date() < deadline { usleep(10000) }
    defer { session.stop() }
    #expect(session.finished)
    #expect(session.exitStatus == 0)
    #expect(session.output.latest(after: 0)?.1.text.contains("controlling-terminal-ready") == true)
  }
}
@Test func adjacentGlyphsShareStyledRunsWithoutMergingRows() throws {
  let screen = try GhosttyScreen(columns: 20, rows: 2)
  screen.write("ab\u{1b}[1;31mred\u{1b}[0m\r\nnext")
  let frame = screen.snapshot()
  let red = frame.runs.first { $0.glyphs.map(\.text).joined() == "red" }
  #expect((red?.flags ?? 0) & 1 == 1)
  #expect(red?.foreground != nil)
  #expect(frame.runs.allSatisfy { Set($0.glyphs.map(\.row)).count == 1 })
}
