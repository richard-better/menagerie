import Foundation
import Testing

@testable import MenagerieCore

@Test(arguments: ["rebase-merge", "rebase-apply", "MERGE_HEAD", "CHERRY_PICK_HEAD", "index.lock"])
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
