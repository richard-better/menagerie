import Foundation
import Testing

@testable import MenagerieCore

@Test func subdirectoryCommitAndDraftUseWorktreeRootPaths() throws {
  let repo = try Repository()
  let bare = try Repository()
  defer {
    repo.remove()
    bare.remove()
  }
  let sub = repo.path + "/sub"
  try FileManager.default.createDirectory(atPath: sub, withIntermediateDirectories: true)
  try repo.write("settings", "root base\n")
  try repo.write("sub/settings", "sub base\n")
  _ = try Runner.git(repo.path, ["add", "."])
  _ = try Runner.git(repo.path, ["commit", "-qm", "base"])
  _ = try Runner.run(
    Command("/usr/bin/git", ["clone", "--bare", "-q", repo.path, bare.path + "/origin"])
  ).checked()
  _ = try Runner.git(repo.path, ["remote", "add", "origin", bare.path + "/origin"])
  try repo.write("settings", "included root edit\n")
  try repo.write("sub/settings", "excluded sub edit\n")
  let changes = try Commit.changes(repo: sub)
  #expect(Set(changes.map(\.path)) == ["settings", "sub/settings"])
  let input = try Commit.draftInput(repo: sub, changes: changes.filter { $0.path == "settings" })
  #expect(input.text.contains("included root edit") && !input.text.contains("excluded sub edit"))
  #expect(try Commit.draft(command: "cat settings", repo: sub, input: "") == "included root edit")
  _ = try Commit.commitAndPush(repo: sub, branch: "main", paths: ["settings"], message: "root only")
  #expect(try Runner.git(repo.path, ["show", "HEAD:settings"]) == "included root edit\n")
  #expect(try Runner.git(repo.path, ["show", "HEAD:sub/settings"]) == "sub base\n")
  #expect(try Runner.git(repo.path, ["diff", "--name-only"]).contains("sub/settings"))
}
