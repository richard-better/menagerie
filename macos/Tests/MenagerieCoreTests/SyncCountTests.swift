import Foundation
import Testing

@testable import MenagerieCore

@Test func syncCountUsesPorcelainEntriesIncludingNewlinePaths() throws {
  let repo = try Repository()
  defer { repo.remove() }
  try repo.write("one\nname", "not scanned for line counts\n")
  try repo.write("other", "")
  #expect(try Commit.uncommittedCount(repo: repo.path, cancel: Cancellation()) == 2)
}
@Test func syncStatusCountCanBeCancelledWhileGitIsRunning() async throws {
  let repo = try Repository()
  defer { repo.remove() }
  let hook = repo.path + "/.git/hooks/blocked-monitor"
  let ready = repo.path + "/.git/ready"
  try ("#!/bin/sh\nprintf ready > " + shellQuote(ready) + "\nexec /bin/sleep 30\n").write(
    toFile: hook, atomically: true, encoding: .utf8)
  try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: hook)
  _ = try Runner.git(repo.path, ["config", "core.fsmonitor", hook])
  let cancel = Cancellation()
  defer { cancel.cancel() }
  let task = Task.detached {
    try Commit.pushCheckout(repo: repo.path, branch: "main", cancel: cancel, output: { _ in })
  }
  for _ in 0..<300 where !FileManager.default.fileExists(atPath: ready) {
    try await Task.sleep(for: .milliseconds(10))
  }
  try #require(FileManager.default.fileExists(atPath: ready))
  let start = Date()
  cancel.cancel()
  do {
    try await task.value
    Issue.record("Status count must be cancelled")
  } catch { #expect(String(describing: error) == "Cancelled") }
  #expect(Date().timeIntervalSince(start) < 3)
}
