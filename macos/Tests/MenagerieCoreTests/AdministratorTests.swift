import Darwin
import Foundation
import Testing

@testable import MenagerieCore

@Test func onlySudosEmptyPasswordFailureOffersAuthorization() {
  #expect(
    Administrator.needsAuthorization(
      .init(status: 1, output: "", errorOutput: "sudo: a password is required\n")))
  for result in [
    CommandResult(status: 2, output: "", errorOutput: "sudo: a password is required"),
    .init(status: 1, output: "rebuild started", errorOutput: "sudo: a password is required"),
    .init(status: 1, output: "", errorOutput: "rebuild failed"), .init(status: 0, output: ""),
  ] {
    #expect(!Administrator.needsAuthorization(result))
  }
}
@Test func administratorEnvironmentIsExplicit() {
  let env = Administrator.rootEnvironment
  #expect(env["USER"] == "root" && env["LOGNAME"] == "root")
  #expect(env["SUDO_UID"] == String(getuid()))
  #expect(env["PATH"]!.contains("/usr/bin:/bin:/usr/sbin:/sbin"))
  #expect(
    Set(env.keys) == [
      "PATH", "HOME", "USER", "LOGNAME", "SUDO_USER", "SUDO_UID", "SUDO_GID", "LC_ALL",
    ])
}
@Test func privilegedSupervisorRequiresExplicitStart() throws {
  let result = try Runner.run(
    Command("/usr/bin/perl", ["-e", Administrator.supervisor, "--", "/bin/echo", "must-not-run"]))
  #expect(result.status == 0 && !result.output.contains("must-not-run"))
}
@Test func privilegedSupervisorOutlivesLeaderAndKillsIgnoringDescendants() throws {
  let process = Process()
  let input = Pipe()
  let output = Pipe()
  process.executableURL = URL(fileURLWithPath: "/usr/bin/perl")
  process.arguments = [
    "-e", Administrator.supervisor, "--", "/bin/sh", "-c",
    "trap '' TERM; sleep 30 & echo child:$!; exit 0",
  ]
  process.standardInput = input
  process.standardOutput = output
  try process.run()
  try input.fileHandleForWriting.write(contentsOf: Data("start\n".utf8))
  let text = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
  process.waitUntilExit()
  let pid = try #require(
    text.components(separatedBy: .newlines).first(where: { $0.hasPrefix("child:") }).flatMap {
      Int32($0.dropFirst(6))
    })
  #expect(kill(pid, 0) == -1 && errno == ESRCH)
}
@Test func cancellationCanAbandonPipesHeldOutsideItsAuthority() async throws {
  let cancel = Cancellation()
  let buffer = OutputBuffer()
  let (ready, signal) = AsyncStream<Void>.makeStream()
  let task = Task.detached {
    try Runner.run(
      Command(
        "/usr/bin/perl",
        [
          "-MPOSIX", "-e",
          "if (fork()==0) { POSIX::setsid(); print qq(ready $$\\n); $|=1; sleep 5; exit } sleep 30",
        ]), cancel: cancel, abandonAfterCancellation: true
    ) {
      buffer.append($0)
      if $0.contains("ready") { signal.yield() }
    }
  }
  for await _ in ready.prefix(1) {}
  let start = Date()
  cancel.cancel()
  await #expect(throws: (any Error).self) { try await task.value }
  #expect(Date().timeIntervalSince(start) < 3)
}
