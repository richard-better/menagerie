import Darwin
import Foundation
import GhosttyBridge
import Synchronization

/// A single-slot output channel. Publishing a frame replaces the prior frame.
public final class TerminalOutput: Sendable {
  private struct State {
    var frame: TerminalFrame?
    var revision: UInt64 = 0
  }
  private let state = Mutex(State())
  public init() {}
  public func publish(_ frame: TerminalFrame) {
    state.withLock {
      $0.frame = frame
      $0.revision &+= 1
    }
  }
  public func latest(after revision: UInt64) -> (UInt64, TerminalFrame)? {
    state.withLock { s in
      guard s.revision != revision, let frame = s.frame else { return nil }
      return (s.revision, frame)
    }
  }
}
public final class TerminalWriteQueue: Sendable {
  private struct State {
    var entries: [Data] = []
    var bytes = 0
    var closed = false
  }
  private let state = Mutex(State())
  public init() {}
  @discardableResult public func enqueue(_ bytes: Data) -> Bool {
    state.withLock { s in
      guard !s.closed, s.entries.count < 256, s.bytes + bytes.count <= 1 << 20 else { return false }
      s.entries.append(bytes)
      s.bytes += bytes.count
      return true
    }
  }
  // Keep the in-flight write charged to the budget until completion.
  public func first() -> Data? { state.withLock { $0.entries.first } }
  public func complete() {
    state.withLock { s in if !s.entries.isEmpty { s.bytes -= s.entries.removeFirst().count } }
  }
  public func close() {
    state.withLock {
      $0.closed = true
      $0.entries = []
      $0.bytes = 0
    }
  }
  public var pendingBytes: Int { state.withLock { $0.bytes } }
}
public final class PTYSession: Sendable {
  private struct State {
    var closed = false, force = false, finished = false, started = false
    var descriptor: Int32 = -1
    var pid: pid_t = 0
    var columns = 100, rows = 22
    var exitStatus: Int32?
  }
  private struct WeakSession: @unchecked Sendable { weak var value: PTYSession? }
  private static let sessions = Mutex<[WeakSession]>([])
  private let state = Mutex(State())
  public let output = TerminalOutput()
  public let input = TerminalWriteQueue()
  public init() {
    Self.sessions.withLock {
      $0.removeAll { $0.value == nil }
      $0.append(WeakSession(value: self))
    }
  }
  public func start(_ command: Command) {
    let start = state.withLock { s in
      guard !s.started else { return false }
      s.started = true
      return true
    }
    if start { Thread.detachNewThread { [self] in supervise(command) } }
  }
  public func stop() {
    state.withLock { $0.closed = true }
    input.close()
  }
  public static func killAll() {
    let live = sessions.withLock { $0.compactMap(\.value) }
    for session in live {
      session.state.withLock {
        $0.closed = true
        $0.force = true
      }
      session.input.close()
    }
  }
  public var finished: Bool { state.withLock { $0.finished } }
  public var processID: pid_t { state.withLock { $0.pid } }
  public var exitStatus: Int32? { state.withLock { $0.exitStatus } }
  public func resize(columns: Int, rows: Int) {
    state.withLock {
      $0.columns = max(20, min(512, columns))
      $0.rows = max(4, min(256, rows))
    }
  }
  public func duplicateDescriptor() -> Int32? {
    state.withLock { s in
      guard s.descriptor >= 0 else { return nil }
      let fd = fcntl(s.descriptor, F_DUPFD_CLOEXEC, 0)
      return fd >= 0 ? fd : nil
    }
  }
  private func supervise(_ command: Command) {
    defer {
      state.withLock { $0.finished = true }
      input.close()
    }
    guard !state.withLock({ $0.closed }) else { return }
    do {
      let screen = try GhosttyScreen()
      let bundledHost = Bundle.main.executableURL?.deletingLastPathComponent()
        .appendingPathComponent("MenageriePTYHost").path
      let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
      let host =
        bundledHost.flatMap { FileManager.default.isExecutableFile(atPath: $0) ? $0 : nil }
        ?? root.appendingPathComponent("target/ghostty/bin/MenageriePTYHost").path
      let argv = ([host, command.program] + command.arguments).map { strdup($0) } + [nil]
      var environment = ProcessInfo.processInfo.environment
      environment["TERM"] = "xterm-256color"
      environment["COLORTERM"] = "truecolor"
      let envp = environment.map { strdup("\($0.key)=\($0.value)") } + [nil]
      defer { for ptr in argv + envp { free(ptr) } }
      var pid: pid_t = 0
      var fd: Int32 = -1
      let result = argv.withUnsafeBufferPointer { args in
        envp.withUnsafeBufferPointer { env in
          if let directory = command.directory {
            return directory.withCString {
              mg_pty_spawn(
                host, args.baseAddress!, env.baseAddress!, $0, 100, 22, &pid, &fd)
            }
          }
          return mg_pty_spawn(
            host, args.baseAddress!, env.baseAddress!, nil, 100, 22, &pid, &fd)
        }
      }
      guard result == 0 else {
        throw FleetError("Cannot start terminal: \(String(cString: strerror(result)))")
      }
      state.withLock {
        $0.pid = pid
        $0.descriptor = fd
      }
      defer {
        kill(-pid, SIGKILL)
        var status: Int32 = 0
        while waitpid(pid, &status, 0) < 0 && errno == EINTR {}
        state.withLock {
          $0.descriptor = -1
          $0.pid = 0
          $0.exitStatus = (status & 0x7f) == 0 ? (status >> 8) & 0xff : 128 + (status & 0x7f)
        }
        close(fd)
      }
      var bytes = [UInt8](repeating: 0, count: 8192)
      var offset = 0
      var columns = 100
      var rows = 22
      var dirty = true
      var closing: ContinuousClock.Instant?
      var ended: ContinuousClock.Instant?
      let clock = ContinuousClock()
      var published = clock.now
      var filter = TerminalFilter()
      while true {
        let flags = state.withLock { ($0.closed, $0.force, $0.columns, $0.rows) }
        if flags.0 && closing == nil {
          closing = clock.now
          kill(-pid, SIGHUP)
        }
        if flags.1 || closing.map({ clock.now - $0 >= .seconds(1) }) == true { kill(-pid, SIGKILL) }
        let exited = mg_child_exited(pid)
        if exited && ended == nil { ended = clock.now }
        if (columns, rows) != (flags.2, flags.3) {
          columns = flags.2
          rows = flags.3
          try screen.resize(columns: columns, rows: rows)
          var size = winsize(
            ws_row: UInt16(rows), ws_col: UInt16(columns), ws_xpixel: 0, ws_ypixel: 0)
          _ = ioctl(fd, TIOCSWINSZ, &size)
          dirty = true
        }
        var descriptor = pollfd(
          fd: fd, events: Int16(POLLIN) | (input.first() == nil ? 0 : Int16(POLLOUT)), revents: 0)
        _ = poll(&descriptor, 1, 16)
        var eof = false
        // Bounded work before checking cancellation and resize again.
        for _ in 0..<32 {
          let count = read(fd, &bytes, bytes.count)
          if count > 0 {
            screen.write(Data(filter.filter(bytes.prefix(count))))
            dirty = true
          } else {
            eof = count == 0 || (count < 0 && errno == EIO)
            break
          }
        }
        let replies = screen.replies()
        if !replies.isEmpty { input.enqueue(replies) }
        if descriptor.revents & Int16(POLLOUT) != 0, let data = input.first() {
          let written = data.withUnsafeBytes {
            write(fd, $0.baseAddress!.advanced(by: offset), data.count - offset)
          }
          if written > 0 {
            offset += written
            if offset == data.count {
              input.complete()
              offset = 0
            }
          }
        }
        if dirty && clock.now - published >= .milliseconds(16) {
          output.publish(screen.snapshot())
          dirty = false
          published = clock.now
        }
        if exited && (eof || ended.map { clock.now - $0 >= .milliseconds(100) } == true) {
          // Keep the unreaped leader's PID reserved until its group is gone.
          if let closing, !flags.1 && clock.now - closing < .seconds(1) {
            usleep(10_000)
            continue
          }
          screen.write("\r\nSession ended\r\n")
          output.publish(screen.snapshot())
          break
        }
      }
    } catch {
      if let screen = try? GhosttyScreen() {
        screen.write("\r\n\(error)\r\n")
        output.publish(screen.snapshot())
      }
    }
  }
}
