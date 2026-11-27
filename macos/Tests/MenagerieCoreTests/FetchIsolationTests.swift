import Foundation
import Testing

@testable import MenagerieCore

@Test func backgroundFetchPreservesDeploymentFetchHead() async throws {
  let repo = try Repository()
  let remote = try Repository()
  let other = try Repository()
  defer {
    repo.remove()
    remote.remove()
    other.remove()
  }
  let origin = remote.path + "/origin"
  _ = try Runner.run(Command("/usr/bin/git", ["clone", "--bare", "-q", repo.path, origin]))
    .checked()
  for path in [repo.path, other.path] {
    _ = try Runner.git(path, ["remote", "add", "origin", origin])
  }
  _ = try Runner.git(repo.path, ["fetch", "-q", "origin", "refs/heads/main"])
  let pinned = try Data(contentsOf: URL(fileURLWithPath: repo.path + "/.git/FETCH_HEAD"))
  _ = try Runner.git(other.path, ["fetch", "-q", "origin"])
  _ = try Runner.git(other.path, ["reset", "--hard", "origin/main"])
  _ = try Runner.git(other.path, ["commit", "-q", "--allow-empty", "-m", "next"])
  _ = try Runner.git(other.path, ["push", "-q", "origin", "main"])
  let head = try Runner.git(other.path, ["rev-parse", "HEAD"])
  let fleet = try Fleet.parse("version: 1\nrepo: '\(repo.path)'\nmachines: {}")
  await Poller.fetch(fleet)
  #expect(try Runner.git(repo.path, ["rev-parse", "origin/main"]) == head)
  #expect(try Data(contentsOf: URL(fileURLWithPath: repo.path + "/.git/FETCH_HEAD")) == pinned)
}
