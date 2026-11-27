import Darwin
import Foundation
import Testing

@testable import MenagerieCore

private final class AuthorizedTestChannel {
  let process = Process()
  let descriptor: Int32
  init(program: String = "/usr/bin/perl", arguments: [String]) throws {
    var pair: [Int32] = [-1, -1]
    guard socketpair(AF_UNIX, SOCK_STREAM, 0, &pair) == 0 else { throw FleetError("socketpair") }
    descriptor = pair[0]
    let child = FileHandle(fileDescriptor: pair[1], closeOnDealloc: true)
    process.executableURL = URL(fileURLWithPath: program)
    process.arguments = arguments
    process.standardInput = child
    process.standardOutput = child
    process.standardError = child
    try process.run()
    try child.close()
  }
  deinit {
    close(descriptor)
    if process.isRunning { process.terminate() }
    process.waitUntilExit()
  }
}
@Test(arguments: [false, true])
func administratorStopNeverAcceptsZeroExit(cancelInsteadOfTimeout: Bool) throws {
  let channel = try AuthorizedTestChannel(arguments: [
    "-T", "-e", Administrator.supervisor, "--", "/bin/sh", "-c",
    "trap 'exit 0' TERM; echo ready; while :; do sleep 1; done",
  ])
  let cancel = Cancellation()
  do {
    _ = try Administrator.runAuthorizedChannel(
      channel.descriptor, timeout: cancelInsteadOfTimeout ? 30 : 0.1, cancel: cancel
    ) { text in
      if cancelInsteadOfTimeout && text.contains("ready") { cancel.cancel() }
    }
    Issue.record("Stop request must not succeed")
  } catch {
    #expect(
      String(describing: error)
        == (cancelInsteadOfTimeout ? "Cancelled" : "Administrator command timed out"))
  }
}
@Test func administratorCompletionDoesNotWaitForEscapedOutputHolder() throws {
  let file = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: file) }
  let script = """
    use POSIX; if (fork()==0) { POSIX::setsid(); open(my $f, '>', $ARGV[0]) or die $!; print $f $$; close $f; sleep 30; exit }
    select(undef,undef,undef,0.01) until -s $ARGV[0]; exit 0;
    """
  let channel = try AuthorizedTestChannel(arguments: [
    "-T", "-e", Administrator.supervisor, "--", "/usr/bin/perl", "-e", script, file.path,
  ])
  defer {
    if let text = try? String(contentsOf: file, encoding: .utf8), let pid = Int32(text) {
      kill(pid, SIGKILL)
    }
  }
  let start = Date()
  let result = try Administrator.runAuthorizedChannel(
    channel.descriptor, timeout: 1, cancel: Cancellation(), output: { _ in })
  #expect(result.status == 0 && Date().timeIntervalSince(start) < 1)
}
@Test func administratorBoundsMissingCompletionAfterStop() throws {
  let channel = try AuthorizedTestChannel(program: "/bin/sleep", arguments: ["30"])
  let start = Date()
  do {
    _ = try Administrator.runAuthorizedChannel(
      channel.descriptor, timeout: 0.05, cancel: Cancellation(), completionGrace: 0.1,
      output: { _ in })
    Issue.record("Missing completion must fail")
  } catch { #expect(String(describing: error) == "Administrator command timed out") }
  #expect(Date().timeIntervalSince(start) < 1)
}

@Test func administratorFramesCompletionAfterUnterminatedOutput() throws {
  let channel = try AuthorizedTestChannel(arguments: [
    "-T", "-e", Administrator.supervisor, "--", "/bin/sh", "-c", "printf partial",
  ])
  let output = OutputBuffer()
  let result = try Administrator.runAuthorizedChannel(
    channel.descriptor, timeout: 2, cancel: Cancellation(), output: { output.append($0) })
  #expect(result.status == 0 && result.output == "partial")
  #expect(output.text.contains("partial\n") && !output.text.contains("menagerie-admin-"))
}
