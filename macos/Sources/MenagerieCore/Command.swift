import Darwin
import Foundation
import Synchronization

public struct Command: Sendable, Equatable {
  public var program: String
  public var arguments: [String]
  public var directory: String?
  public var input: Data
  public var environment: [String: String]? = nil
  public var timeout: TimeInterval
  public init(
    _ program: String, _ arguments: [String] = [], directory: String? = nil, input: Data = Data(),
    timeout: TimeInterval = 30
  ) {
    self.program = program
    self.arguments = arguments
    self.directory = directory
    self.input = input
    self.timeout = timeout
  }
}
public final class Cancellation: Sendable {
  private let state = Mutex(false)
  public init() {}
  public func cancel() { state.withLock { $0 = true } }
  public var isCancelled: Bool { state.withLock { $0 } }
}
public struct CommandResult: Sendable {
  public let status: Int32
  public let output: String
  public let errorOutput: String
  public init(status: Int32, output: String, errorOutput: String = "") {
    self.status = status
    self.output = output
    self.errorOutput = errorOutput
  }
  public func checked() throws -> String {
    guard status == 0 else {
      throw FleetError(
        !errorOutput.isEmpty ? errorOutput : output.isEmpty ? "Command failed (\(status))" : output)
    }
    return output
  }
}

public enum Runner {
  /// All blocking process and pipe work is confined to the calling worker task.
  public static func run(
    _ command: Command, onStart: @Sendable () -> Void = {}, cancel: Cancellation = Cancellation(),
    abandonAfterCancellation: Bool = false,
    output: @Sendable (String) -> Void = { _ in }
  ) throws -> CommandResult {
    guard !cancel.isCancelled else { throw FleetError("Cancelled") }
    guard
      !([command.program] + command.arguments + [command.directory ?? ""]).contains(where: {
        $0.utf8.contains(0)
      })
    else { throw FleetError("NUL in argument") }
    var err: [Int32] = [0, 0]
    guard pipe(&err) == 0 else { throw FleetError("Cannot create error pipe") }
    defer { for fd in err where fd >= 0 { close(fd) } }
    var out: [Int32] = [0, 0]
    var input: [Int32] = [0, 0]
    guard pipe(&out) == 0 else { throw FleetError("Cannot create output pipe") }
    guard pipe(&input) == 0 else {
      close(out[0])
      close(out[1])
      throw FleetError("Cannot create input pipe")
    }
    for fd in out + input + err { _ = fcntl(fd, F_SETFD, FD_CLOEXEC) }
    defer { for fd in out + input where fd >= 0 { close(fd) } }
    var actions: posix_spawn_file_actions_t?
    var attributes: posix_spawnattr_t?
    posix_spawn_file_actions_init(&actions)
    posix_spawnattr_init(&attributes)
    defer {
      posix_spawn_file_actions_destroy(&actions)
      posix_spawnattr_destroy(&attributes)
    }
    posix_spawn_file_actions_adddup2(&actions, input[0], STDIN_FILENO)
    posix_spawn_file_actions_adddup2(&actions, out[1], STDOUT_FILENO)
    posix_spawn_file_actions_adddup2(&actions, err[1], STDERR_FILENO)
    if let directory = command.directory { posix_spawn_file_actions_addchdir(&actions, directory) }
    posix_spawnattr_setflags(
      &attributes, Int16(POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_CLOEXEC_DEFAULT))
    posix_spawnattr_setpgroup(&attributes, 0)
    let argv = ([command.program] + command.arguments).map { strdup($0) } + [nil]
    var environment = command.environment ?? ProcessInfo.processInfo.environment
    if command.environment == nil {
      environment["GIT_TERMINAL_PROMPT"] = "0"
      environment["GIT_NO_LAZY_FETCH"] = "1"
      environment["GIT_LITERAL_PATHSPECS"] = "1"
    }
    let envp = environment.map { strdup("\($0.key)=\($0.value)") } + [nil]
    defer { for p in argv + envp { free(p) } }
    var pid: pid_t = 0
    let error = argv.withUnsafeBufferPointer { args in
      envp.withUnsafeBufferPointer { env in
        posix_spawnp(
          &pid, command.program, &actions, &attributes, args.baseAddress!, env.baseAddress!)
      }
    }
    guard error == 0 else {
      throw FleetError("Cannot start \(command.program): \(String(cString: strerror(error)))")
    }
    onStart()
    close(input[0])
    input[0] = -1
    close(out[1])
    out[1] = -1
    close(err[1])
    err[1] = -1
    _ = fcntl(input[1], F_SETFL, O_NONBLOCK)
    _ = fcntl(out[0], F_SETFL, O_NONBLOCK)
    _ = fcntl(err[0], F_SETFL, O_NONBLOCK)
    _ = fcntl(input[1], F_SETNOSIGPIPE, 1)
    var offset = 0
    var data = [Data(), Data()]
    var buffer = [UInt8](repeating: 0, count: 16384)
    let start = Date()
    var stopped: Date?
    var status: Int32 = 0
    var exited = false
    var eof = [false, false]
    while !exited || !eof.allSatisfy({ $0 }) {
      if stopped == nil && (cancel.isCancelled || Date().timeIntervalSince(start) > command.timeout)
      {
        stopped = Date()
        kill(-pid, SIGTERM)
      }
      if let stopped, Date().timeIntervalSince(stopped) >= 1 { kill(-pid, SIGKILL) }
      // Unprivileged sudo cancellation cannot kill root descendants; they may still be finishing.
      if let stopped, abandonAfterCancellation, Date().timeIntervalSince(stopped) >= 2 {
        if !exited { _ = waitpid(pid, &status, WNOHANG) }
        throw FleetError("Cancelled or timed out; root processes may still be finishing")
      }
      if offset == command.input.count && input[1] >= 0 {
        close(input[1])
        input[1] = -1
      }
      var fds = [
        pollfd(fd: out[0], events: Int16(POLLIN), revents: 0),
        pollfd(fd: input[1], events: Int16(POLLOUT), revents: 0),
        pollfd(fd: err[0], events: Int16(POLLIN), revents: 0),
      ]
      _ = poll(&fds, 3, 50)
      if input[1] >= 0 && fds[1].revents & Int16(POLLOUT) != 0 {
        let n = command.input.withUnsafeBytes {
          write(
            input[1], $0.baseAddress!.advanced(by: offset), min(16384, command.input.count - offset)
          )
        }
        if n > 0 {
          offset += n
        } else if errno != EAGAIN && errno != EINTR {
          close(input[1])
          input[1] = -1
        }
      }
      for index in 0..<2 where !eof[index] {
        let n = read(index == 0 ? out[0] : err[0], &buffer, buffer.count)
        if n > 0 {
          output(String(decoding: buffer.prefix(n), as: UTF8.self))
          data[index].append(contentsOf: buffer.prefix(n))
          if data[index].count > 2 * 1024 * 1024 {
            data[index].removeFirst(data[index].count - 2 * 1024 * 1024)
          }
        } else if n == 0 {
          eof[index] = true
        }
      }
      if !exited { exited = waitpid(pid, &status, WNOHANG) == pid }
      // Descendants retaining a pipe also remain subject to the deadline.
      if exited && stopped != nil { kill(-pid, SIGKILL) }
    }
    if stopped != nil {
      kill(-pid, SIGKILL)
      throw FleetError(cancel.isCancelled ? "Cancelled" : "Command timed out")
    }
    return CommandResult(
      status: (status & 0x7f) == 0 ? (status >> 8) & 0xff : 128 + (status & 0x7f),
      output: String(decoding: data[0], as: UTF8.self),
      errorOutput: String(decoding: data[1], as: UTF8.self))
  }
  public static func git(
    _ repo: String, _ arguments: [String], cancel: Cancellation = Cancellation()
  ) throws -> String {
    try run(Command("/usr/bin/git", ["-C", repo] + arguments, timeout: 60), cancel: cancel)
      .checked()
  }
}
public func shellQuote(_ text: String) -> String {
  "'" + text.replacingOccurrences(of: "'", with: "'\\''") + "'"
}
public func shellPath(_ path: String) -> String {
  path.hasPrefix("~/") ? "~/" + shellQuote(String(path.dropFirst(2))) : shellQuote(path)
}

public final class OutputBuffer: Sendable {
  private let storage = Mutex("")
  public init() {}
  public func append(_ chunk: String) {
    storage.withLock { text in
      text += chunk
      if text.utf8.count > 256 * 1024 { text = String(text.suffix(128 * 1024)) }
      let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
      if lines.count > 2000 { text = lines.suffix(2000).joined(separator: "\n") }
    }
  }
  public var text: String { storage.withLock { $0 } }
}
