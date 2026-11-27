import Foundation
import Testing

@testable import MenagerieCore

@Test(arguments: [
  "echo ready; sleep 30", "trap '' TERM; echo ready; sleep 30 & wait",
  "(trap '' TERM; exec >/dev/null 2>&1; sleep 30) & echo ready; wait",
])
func cancellationKillsTheWholePlanAndNeverRunsTheNextStep(script: String) async throws {
  let cancel = Cancellation()
  let output = OutputBuffer()
  let (ready, signal) = AsyncStream<Void>.makeStream()
  let task = Task.detached {
    defer { signal.finish() }
    try Deployment.run(
      [
        DeploymentStep(label: "first", command: Command("/bin/sh", ["-c", script])),
        DeploymentStep(label: "never-next-step", command: Command("/bin/echo", ["unexpected"])),
      ], cancel: cancel
    ) { text in
      output.append(text)
      if text.contains("ready") { signal.yield() }
    }
  }
  for await _ in ready.prefix(1) {}
  #expect(output.text.contains("ready"))
  let start = Date()
  cancel.cancel()
  await #expect(throws: (any Error).self) { try await task.value }
  #expect(Date().timeIntervalSince(start) < 4)
  #expect(!output.text.contains("never-next-step"))
}
@Test func failedStepStopsThePlanAndShellQuotingSurvivesQuotes() throws {
  let output = OutputBuffer()
  #expect(throws: (any Error).self) {
    try Deployment.run(
      [
        DeploymentStep(label: "first", command: Command("/bin/sh", ["-c", "echo first; exit 3"])),
        DeploymentStep(label: "never-next", command: Command("/bin/echo", ["never"])),
      ], cancel: Cancellation(), output: { output.append($0) })
  }
  #expect(output.text.contains("first") && !output.text.contains("never"))
  let value = "a b'c; $(false)"
  #expect(
    try Runner.run(Command("/bin/sh", ["-c", "printf %s " + shellQuote(value)])).checked() == value)
}
@Test func unsupportedTargetsAreRefusedAndRollbackPlansKeepPrivilegesSeparate() throws {
  let fleet = try Fleet.parse(
    """
    version: 1
    machines:
      missing: {}
      unreachable: { sync: {preset: nixos} }
      custom: { agent: 'http://localhost:9184', sync: {preset: custom, command: echo} }
      local: { agent: 'http://localhost:9184', sync: {preset: nix-darwin} }
    """)
  for name in ["missing", "unreachable"] {
    #expect(throws: (any Error).self) { try Deployment.plan(.sync, fleet: fleet, name: name) }
  }
  #expect(throws: (any Error).self) { try Deployment.plan(.rollback, fleet: fleet, name: "custom") }
  let rollback = try Deployment.plan(.rollback, fleet: fleet, name: "local")
  #expect(rollback.count == 1 && rollback[0].administrator)
  #expect(rollback[0].command.arguments == ["--rollback"])
}
@Test func checkoutDiagnosticsAndExactBranchChecks() throws {
  let repo = try Repository()
  defer { repo.remove() }
  let remote = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).path
  defer { try? FileManager.default.removeItem(atPath: remote) }
  _ = try Runner.run(Command("/usr/bin/git", ["clone", "--bare", "-q", repo.path, remote]))
    .checked()
  _ = try Runner.git(repo.path, ["remote", "add", "origin", remote])
  _ = try Runner.git(repo.path, ["config", "status.showUntrackedFiles", "no"])
  func run(branch: String = "main") throws -> String {
    try Runner.run(
      Command("/bin/sh", ["-c", Deployment.pushScript(repo: repo.path, branch: branch)])
    ).checked()
  }
  #expect(try run().contains("Nothing to push."))
  try repo.write("new", "dirty")
  #expect(try run().contains("1 uncommitted file stays behind"))
  try repo.write("other", "dirty")
  #expect(try run().contains("2 uncommitted files stay behind"))
  _ = try Runner.git(repo.path, ["tag", "main"])
  #expect(throws: (any Error).self) { try run(branch: "heads/main") }
  _ = try Runner.git(repo.path, ["switch", "-q", "-c", "experiment"])
  #expect(throws: (any Error).self) { try run() }
}
@Test(.enabled(if: ProcessInfo.processInfo.environment["MENAGERIE_TEST_ADMIN"] == "1"))
func administratorStreamsRealRootIdentityAndCancellation() throws {
  let output = OutputBuffer()
  let result = try Administrator.run(
    Command("/bin/sh", ["-c", "id -u; id -ru; echo first; sleep 0.1; echo second; exit 3"]),
    cancel: Cancellation(), output: { output.append($0) })
  #expect(result.status == 3)
  #expect(output.text.hasPrefix("0\n0\nfirst\n"))
  #expect(output.text.contains("second"))
  let cancel = Cancellation()
  let started = Date()
  let stopped = OutputBuffer()
  #expect(throws: (any Error).self) {
    try Administrator.run(
      Command("/bin/sh", ["-c", "echo started; sleep 30 & wait"]), cancel: cancel
    ) { text in
      stopped.append(text)
      if text.contains("started") { cancel.cancel() }
    }
  }
  #expect(Date().timeIntervalSince(started) < 10)
}

@Test func customRebuildNeverRunsAfterAFailedPull() throws {
  let repo = try Repository()
  defer { repo.remove() }
  let marker = repo.path + "/must-not-exist"
  let fleet = try Fleet.parse(
    """
    version: 1
    machines:
      local:
        agent: http://localhost:9184
        sync: {preset: custom, repo: '\(repo.path)', command: "touch \(shellQuote(marker))"}
    """)
  let steps = try Deployment.plan(.sync, fleet: fleet, name: "local")
  #expect(throws: (any Error).self) {
    try Deployment.run(steps, cancel: Cancellation(), output: { _ in })
  }
  #expect(!FileManager.default.fileExists(atPath: marker))
}
