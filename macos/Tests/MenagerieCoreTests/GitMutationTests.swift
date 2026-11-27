import Foundation
import Testing

@testable import MenagerieCore

@Test(arguments: [
  "rebase-merge", "rebase-apply", "MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "sequencer",
  "index.lock",
])
func existingGitOperationsAreNeverMutatedOrAborted(marker: String) throws {
  let repo = try Repository()
  defer { repo.remove() }
  let path = repo.path + "/.git/" + marker
  try Data("existing operation".utf8).write(to: URL(fileURLWithPath: path))
  #expect(throws: (any Error).self) {
    try Commit.commitAndPush(repo: repo.path, branch: "main", paths: ["new"], message: "test")
  }
  #expect(try String(contentsOfFile: path, encoding: .utf8) == "existing operation")
}
@Test func mutationsSerializeCanonicalCheckoutPaths() throws {
  let repo = try Repository()
  defer { repo.remove() }
  _ = try CheckoutMutation.run(repos: [repo.path]) {
    #expect(throws: (any Error).self) { try CheckoutMutation.run(repos: [repo.path + "/."]) {} }
  }
  try CheckoutMutation.run(repos: [repo.path]) {}
}
@Test(arguments: [true, false])
func autostashConflictNeverPushesTheRebasedCommit(viaCommit: Bool) throws {
  let repo = try Repository()
  let other = try Repository()
  let bare = try Repository()
  defer {
    repo.remove()
    other.remove()
    bare.remove()
  }
  try repo.write("dirty", "base\n")
  _ = try Runner.git(repo.path, ["add", "."])
  _ = try Runner.git(repo.path, ["commit", "-qm", "base"])
  _ = try Runner.run(
    Command("/usr/bin/git", ["clone", "--bare", "-q", repo.path, bare.path + "/origin"])
  ).checked()
  let remote = bare.path + "/origin"
  for path in [repo.path, other.path] {
    _ = try Runner.git(path, ["remote", "add", "origin", remote])
  }
  _ = try Runner.git(other.path, ["fetch", "-q", "origin"])
  _ = try Runner.git(other.path, ["reset", "--hard", "origin/main"])
  try other.write("dirty", "upstream\n")
  _ = try Runner.git(other.path, ["commit", "-qam", "upstream"])
  _ = try Runner.git(other.path, ["push", "-q", "origin", "main"])
  let upstream = try Runner.git(other.path, ["rev-parse", "HEAD"])
  try repo.write("selected", "selected\n")
  if !viaCommit {
    _ = try Runner.git(repo.path, ["add", "selected"])
    _ = try Runner.git(repo.path, ["commit", "-qm", "selected"])
  }
  try repo.write("dirty", "unselected changes\n")
  do {
    if viaCommit {
      _ = try Commit.commitAndPush(
        repo: repo.path, branch: "main", paths: ["selected"], message: "selected")
    } else {
      try CheckoutMutation.run(repos: [repo.path]) {
        try Commit.pushCheckout(
          repo: repo.path, branch: "main", cancel: Cancellation(), output: { _ in })
      }
    }
    Issue.record("Autostash conflict must stop push")
  } catch {
    #expect(String(describing: error).contains("rebased but not pushed"))
    #expect(String(describing: error).contains("autostash"))
  }
  #expect(try Runner.git(remote, ["rev-parse", "HEAD"]) == upstream)
  #expect(!(try Runner.git(repo.path, ["ls-files", "-u"])).isEmpty)
  #expect(try Runner.git(repo.path, ["stash", "list"]).contains("autostash"))
}

@Test func interruptedMultiCommitRevertIsPreserved() throws {
  let repo = try Repository()
  defer { repo.remove() }
  func commit(_ text: String) throws -> String {
    try repo.write("file", text)
    _ = try Runner.git(repo.path, ["add", "file"])
    _ = try Runner.git(repo.path, ["commit", "-qm", text])
    return try Runner.git(repo.path, ["rev-parse", "HEAD"]).trimmingCharacters(
      in: .whitespacesAndNewlines)
  }
  _ = try commit("base")
  let first = try commit("first")
  let second = try commit("second")
  _ = try commit("conflicting edit")
  #expect(throws: (any Error).self) {
    try Runner.git(repo.path, ["revert", "--no-edit", second, first])
  }
  let before = try Runner.git(repo.path, ["ls-files", "--stage"])
  try repo.write("selected", "must not stage")
  #expect(throws: (any Error).self) {
    try Commit.commitAndPush(
      repo: repo.path, branch: "main", paths: ["selected"], message: "must not commit")
  }
  #expect(try Runner.git(repo.path, ["ls-files", "--stage"]) == before)
  #expect(FileManager.default.fileExists(atPath: repo.path + "/.git/REVERT_HEAD"))
  #expect(FileManager.default.fileExists(atPath: repo.path + "/.git/sequencer"))
}
@Test(arguments: ["REVERT_HEAD", "sequencer"])
func remotePullRefusesRevertState(marker: String) throws {
  let repo = try Repository()
  defer { repo.remove() }
  try Data().write(to: URL(fileURLWithPath: repo.path + "/.git/" + marker))
  let fleet = try Fleet.parse(
    "version: 1\nmachines: {remote: {ssh: test@remote, sync: {preset: custom, repo: '\(repo.path)', command: echo}}}"
  )
  let command = try #require(
    Deployment.plan(.sync, fleet: fleet, name: "remote").last?.command.arguments.last)
  let result = try Runner.run(Command("/bin/sh", ["-c", command]))
  #expect(result.status != 0 && result.output.contains("Git operation in progress: \(marker)"))
}
