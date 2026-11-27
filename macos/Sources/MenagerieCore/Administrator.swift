import AdminBridge
import Darwin
import Foundation

public enum Administrator {
  static let supervisor = #"""
    use POSIX qw(:sys_wait_h setpgid);
    use IO::Select;
    use Time::HiRes qw(time);
    open(STDERR, ">&STDOUT") or die "stderr: $!";
    $| = 1;
    $ENV{PATH} = "/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/bin:/bin:/usr/sbin:/sbin";
    delete @ENV{qw(IFS CDPATH ENV BASH_ENV)};
    my @argv = map { /\A(.*)\z/s; $1 } @ARGV;
    $< = $>;
    if ($> == 0) { POSIX::setgid(0) == 0 or die "setgid: $!"; }
    print "\nmenagerie-admin-pid: $$\n";
    my $start = <STDIN>;
    exit 0 unless defined($start) && $start eq "start\n";
    my $job = fork(); defined($job) or die "fork: $!";
    if ($job == 0) {
      setpgid(0, 0) == 0 or die "setpgid: $!";
      open(STDIN, "<", "/dev/null") or die "stdin: $!";
      exec { $argv[0] } @argv; die "exec: $!";
    }
    setpgid($job, $job);
    my $control = IO::Select->new(\*STDIN);
    my ($status, $stopping);
    while (1) {
      if (!defined($status) && waitpid($job, WNOHANG) == $job) { $status = $?; }
      last if defined($status) && !kill(0, -$job);
      if (!defined($stopping) && (defined($status) || $control->can_read(0.05))) {
        $stopping = time; kill 'TERM', -$job;
      }
      if (defined($stopping)) {
        kill 'KILL', -$job if time - $stopping >= 1;
        select(undef, undef, undef, 0.05);
      }
    }
    print "\nmenagerie-admin-exit: ", (($status & 127) ? 128 + ($status & 127) : $status >> 8), "\n";
    """#
  static func needsAuthorization(_ result: CommandResult) -> Bool {
    result.status == 1 && result.output.isEmpty
      && result.errorOutput.trimmingCharacters(in: .whitespacesAndNewlines)
        == "sudo: a password is required"
  }
  static var rootEnvironment: [String: String] {
    let user = getpwuid(getuid())!
    let name = String(cString: user.pointee.pw_name)
    let gid = user.pointee.pw_gid
    let home = getpwnam("root").map { String(cString: $0.pointee.pw_dir) } ?? "/var/root"
    return [
      "PATH":
        "/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/bin:/bin:/usr/sbin:/sbin",
      "HOME": home, "USER": "root", "LOGNAME": "root", "SUDO_USER": name,
      "SUDO_UID": String(getuid()), "SUDO_GID": String(gid), "LC_ALL": "C",
    ]
  }
  public static func run(
    _ command: Command, cancel: Cancellation, output: @Sendable (String) -> Void
  ) throws -> CommandResult {
    var sudo = Command(
      "/usr/bin/sudo", ["-n", "--", command.program] + command.arguments,
      timeout: command.timeout)
    sudo.environment = rootEnvironment
    let result = try Runner.run(
      sudo, cancel: cancel, abandonAfterCancellation: true, output: output)
    guard needsAuthorization(result) else { return result }
    guard !cancel.isCancelled else { throw FleetError("Cancelled") }
    let arguments =
      ["-i"] + rootEnvironment.map { "\($0.key)=\($0.value)" }
      + ["/usr/bin/perl", "-e", supervisor, "--", command.program] + command.arguments
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
    return try runAuthorizedChannel(
      fileno(channel), timeout: command.timeout, cancel: cancel, output: output)
  }
  static func runAuthorizedChannel(
    _ fd: Int32, timeout: TimeInterval, cancel: Cancellation,
    completionGrace: TimeInterval = 3, output: @Sendable (String) -> Void
  ) throws -> CommandResult {
    _ = fcntl(fd, F_SETFD, FD_CLOEXEC)
    _ = fcntl(fd, F_SETFL, O_NONBLOCK)
    _ = fcntl(fd, F_SETNOSIGPIPE, 1)
    if !cancel.isCancelled {
      let startLine = Array("start\n".utf8)
      guard startLine.withUnsafeBytes({ write(fd, $0.baseAddress!, $0.count) }) == startLine.count
      else {
        throw FleetError("Administrator control pipe closed before start")
      }
    } else {
      shutdown(fd, SHUT_WR)
    }
    var bytes = [UInt8](repeating: 0, count: 16384)
    var pending = Data()
    var tail = ""
    var exit: Int32?
    var pid: pid_t?
    let start = Date()
    var stop: (reason: String, at: Date)?
    func requestStopIfNeeded() {
      guard stop == nil else { return }
      if cancel.isCancelled {
        stop = ("Cancelled", Date())
      } else if Date().timeIntervalSince(start) >= timeout {
        stop = ("Administrator command timed out", Date())
      }
      if stop != nil { shutdown(fd, SHUT_WR) }
    }
    while exit == nil {
      requestStopIfNeeded()
      if let stop, Date().timeIntervalSince(stop.at) >= completionGrace { break }
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
          if !line.isEmpty { tail = String(line.suffix(4096)) }
        }
      }
      if pending.count > 65536 {
        output(String(decoding: pending, as: UTF8.self))
        pending.removeAll(keepingCapacity: true)
      }
    }
    if let pid {
      var status: Int32 = 0
      _ = waitpid(pid, &status, WNOHANG)
    }
    requestStopIfNeeded()
    if let stop { throw FleetError(stop.reason) }
    guard let exit else { throw FleetError("Administrator command ended without reporting status") }
    return CommandResult(status: exit, output: tail)
  }
}
