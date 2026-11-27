import AdminBridge
import Darwin
import Foundation

public enum Administrator {
  private static let wrapper = #"""
    exec 2>&1
    echo "menagerie-admin-pid: $$"
    set -m
    /usr/bin/perl -e '
      $ENV{PATH} = "/usr/bin:/bin:/usr/sbin:/sbin"; delete @ENV{qw(IFS CDPATH ENV BASH_ENV)};
      my @argv = map { /\A(.*)\z/s; $1 } @ARGV;
      $< = $> = 0; $( = $) = "0 0";
      exec { $argv[0] } @argv or die "$argv[0]: $!\n"' "$@" < /dev/null &
    job=$!
    set +m
    ( read -r _; kill -TERM -- -"$job"; sleep 3; kill -KILL -- -"$job" ) <&0 >/dev/null 2>&1 &
    watcher=$!
    wait "$job" 2>/dev/null
    status=$?
    kill "$watcher" 2>/dev/null
    echo "menagerie-admin-exit: $status"
    """#
  public static func run(
    _ command: Command, cancel: Cancellation, output: @Sendable (String) -> Void
  ) throws -> CommandResult {
    let check = try Runner.run(
      Command("/usr/bin/sudo", ["-n", "-l", "--", command.program] + command.arguments),
      cancel: cancel)
    if check.status == 0 {
      return try Runner.run(
        Command(
          "/usr/bin/sudo", ["-n", command.program] + command.arguments, timeout: command.timeout),
        cancel: cancel, output: output)
    }
    guard !cancel.isCancelled else { throw FleetError("Cancelled") }
    var environment = ["SUDO_UID=\(getuid())", "SUDO_GID=\(getgid())"]
    for name in ["USER", "HOME"] {
      if let value = ProcessInfo.processInfo.environment[name] {
        environment.append("\(name == "USER" ? "SUDO_USER" : name)=\(value)")
      }
    }
    let arguments =
      ["-c", wrapper, "sh", "/usr/bin/env"] + environment + [command.program] + command.arguments
    guard !arguments.contains(where: { $0.utf8.contains(0) }) else {
      throw FleetError("NUL in administrator argument")
    }
    let argv = arguments.map { strdup($0) } + [nil]
    defer { for p in argv { free(p) } }
    var channel: UnsafeMutablePointer<FILE>?
    var auth: UnsafeMutableRawPointer?
    let status = argv.withUnsafeBufferPointer {
      menagerie_admin_start(
        "Menagerie needs to switch this Mac's configuration.", $0.baseAddress!, &channel, &auth)
    }
    guard status == 0, let channel, let auth else {
      throw FleetError(status == -60006 ? "Cancelled" : "Authorization failed (\(status))")
    }
    defer {
      fclose(channel)
      menagerie_admin_free(auth)
    }
    let fd = fileno(channel)
    _ = fcntl(fd, F_SETFD, FD_CLOEXEC)
    _ = fcntl(fd, F_SETFL, O_NONBLOCK)
    var bytes = [UInt8](repeating: 0, count: 16384)
    var pending = Data()
    var tail = ""
    var exit: Int32?
    var pid: pid_t?
    let start = Date()
    while true {
      if cancel.isCancelled || Date().timeIntervalSince(start) > command.timeout {
        shutdown(fd, SHUT_WR)
      }
      var descriptor = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
      _ = poll(&descriptor, 1, 100)
      let count = read(fd, &bytes, bytes.count)
      if count == 0 { break }
      if count < 0 {
        if errno == EAGAIN || errno == EINTR { continue }
        break
      }
      pending.append(contentsOf: bytes.prefix(count))
      while let end = pending.firstIndex(of: 10) {
        let line = String(decoding: pending[..<end], as: UTF8.self)
        pending.removeSubrange(...end)
        if line.hasPrefix("menagerie-admin-pid: ") {
          pid = Int32(line.dropFirst(21))
        } else if line.hasPrefix("menagerie-admin-exit: ") {
          exit = Int32(line.dropFirst(22))
        } else {
          output(line + "\n")
          tail = String(line.suffix(4096))
        }
      }
      if pending.count > 65536 {
        output(String(decoding: pending, as: UTF8.self))
        pending.removeAll(keepingCapacity: true)
      }
    }
    if let pid {
      var status: Int32 = 0
      _ = waitpid(pid, &status, 0)
    }
    guard !cancel.isCancelled else { throw FleetError("Cancelled") }
    guard let exit else { throw FleetError("Administrator command ended without reporting status") }
    return CommandResult(status: exit, output: tail)
  }
}
