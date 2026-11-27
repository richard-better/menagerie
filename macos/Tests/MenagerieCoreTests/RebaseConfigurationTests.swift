import Foundation
import Testing

@testable import MenagerieCore

@Test(arguments: [false, true])
func rebaseNeverPreservesMergesDespiteUserConfiguration(viaCommit: Bool) throws {
  let repo = try Repository()
  let other = try Repository()
  let bare = try Repository()
  defer {
    repo.remove()
    other.remove()
    bare.remove()
  }
  let origin = bare.path + "/origin"
  _ = try Runner.run(Command("/usr/bin/git", ["clone", "--bare", "-q", repo.path, origin]))
    .checked()
  for directory in [repo.path, other.path] {
    _ = try Runner.git(directory, ["remote", "add", "origin", origin])
  }
  _ = try Runner.git(other.path, ["fetch", "-q", "origin"])
  _ = try Runner.git(other.path, ["reset", "--hard", "origin/main"])
  try other.write("remote", "remote")
  _ = try Runner.git(other.path, ["add", "."])
  _ = try Runner.git(other.path, ["commit", "-qm", "remote"])
  _ = try Runner.git(other.path, ["push", "-q", "origin", "main"])
  _ = try Runner.git(repo.path, ["switch", "-qc", "feature"])
  try repo.write("feature", "feature")
  _ = try Runner.git(repo.path, ["add", "."])
  _ = try Runner.git(repo.path, ["commit", "-qm", "feature"])
  _ = try Runner.git(repo.path, ["switch", "-q", "main"])
  try repo.write("local", "local")
  _ = try Runner.git(repo.path, ["add", "."])
  _ = try Runner.git(repo.path, ["commit", "-qm", "local"])
  _ = try Runner.git(repo.path, ["merge", "--no-ff", "-qm", "merge", "feature"])
  for (key, value) in [
    ("rebase.rebaseMerges", "true"), ("pull.rebase", "merges"), ("merge.ff", "false"),
  ] {
    _ = try Runner.git(repo.path, ["config", key, value])
  }
  if viaCommit {
    try repo.write("selected", "selected")
    _ = try Commit.commitAndPush(
      repo: repo.path, branch: "main", paths: ["selected"], message: "selected")
  } else {
    try CheckoutMutation.run(repos: [repo.path]) {
      try Commit.pushCheckout(
        repo: repo.path, branch: "main", cancel: Cancellation(), output: { _ in })
    }
  }
  #expect(try Runner.git(repo.path, ["rev-list", "--merges", "HEAD"]).isEmpty)
}
