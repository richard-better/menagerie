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

@Test func selectedFileCannotExpandToDirectory() throws {
  let repo = try Repository()
  defer { repo.remove() }
  try repo.write("settings", "selected")
  let paths = try Commit.changes(repo: repo.path).map(\.path)
  let before = try Runner.git(repo.path, ["rev-parse", "HEAD"])
  try FileManager.default.removeItem(atPath: repo.path + "/settings")
  try FileManager.default.createDirectory(
    atPath: repo.path + "/settings", withIntermediateDirectories: false)
  try repo.write("settings/secret.env", "excluded")
  #expect(throws: (any Error).self) {
    try Commit.commitAndPush(
      repo: repo.path, branch: "main", paths: paths, message: "selected only")
  }
  #expect(try Runner.git(repo.path, ["rev-parse", "HEAD"]) == before)
  #expect(try Runner.git(repo.path, ["diff", "--cached", "--name-only"]).isEmpty)
}

@Test func commitVerifiesHookTreeBeforeUpdatingHead() throws {
  let repo = try Repository()
  defer { repo.remove() }
  try repo.write("selected", "included")
  try repo.write("excluded", "secret")
  let hook = repo.path + "/.git/hooks/pre-commit"
  try "#!/bin/sh\ngit add -- excluded\n".write(toFile: hook, atomically: true, encoding: .utf8)
  try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: hook)
  let before = try Runner.git(repo.path, ["rev-parse", "HEAD"])
  #expect(throws: (any Error).self) {
    try Commit.commitAndPush(
      repo: repo.path, branch: "main", paths: ["selected"], message: "selected only")
  }
  #expect(try Runner.git(repo.path, ["rev-parse", "HEAD"]) == before)
  #expect(try Runner.git(repo.path, ["diff", "--cached", "--name-only"]).isEmpty)
}

@Test func selectedTreeCannotHideHookStagedSubmodule() throws {
  let repo = try Repository()
  defer { repo.remove() }
  let first = try Runner.git(repo.path, ["rev-parse", "HEAD"]).trimmingCharacters(
    in: .whitespacesAndNewlines)
  _ = try Runner.git(
    repo.path, ["update-index", "--add", "--cacheinfo", "160000," + first + ",hidden"])
  _ = try Runner.git(repo.path, ["commit", "-qm", "gitlink"])
  let before = try Runner.git(repo.path, ["rev-parse", "HEAD"])
  let next = before.trimmingCharacters(in: .whitespacesAndNewlines)
  _ = try Runner.git(repo.path, ["config", "diff.ignoreSubmodules", "all"])
  try repo.write("selected", "included")
  let hook = repo.path + "/.git/hooks/pre-commit"
  try "#!/bin/sh\ngit update-index --cacheinfo 160000,\(next),hidden\n".write(
    toFile: hook, atomically: true, encoding: .utf8)
  try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: hook)
  #expect(throws: (any Error).self) {
    try Commit.commitAndPush(
      repo: repo.path, branch: "main", paths: ["selected"], message: "selected only")
  }
  #expect(try Runner.git(repo.path, ["rev-parse", "HEAD"]) == before)
}

@Test func indexLockCreatedDuringCommitLeavesHeadAndIndexUnchanged() throws {
  let repo = try Repository()
  defer { repo.remove() }
  try repo.write("selected", "included")
  try repo.write("excluded", "staged")
  _ = try Runner.git(repo.path, ["add", "excluded"])
  let before = try Runner.git(repo.path, ["rev-parse", "HEAD"])
  let index = try Data(contentsOf: URL(fileURLWithPath: repo.path + "/.git/index"))
  let hook = repo.path + "/.git/hooks/pre-commit"
  try "#!/bin/sh\n: > \"$(git rev-parse --absolute-git-dir)/index.lock\"\n".write(
    toFile: hook, atomically: true, encoding: .utf8)
  try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: hook)
  #expect(throws: (any Error).self) {
    try Commit.commitAndPush(
      repo: repo.path, branch: "main", paths: ["selected"], message: "selected only")
  }
  #expect(try Runner.git(repo.path, ["rev-parse", "HEAD"]) == before)
  #expect(try Data(contentsOf: URL(fileURLWithPath: repo.path + "/.git/index")) == index)
}

@Test func cancellationAfterRefUpdateStillInstallsPreparedIndex() throws {
  let repo = try Repository()
  defer { repo.remove() }
  try repo.write("selected", "included")
  let before = try Runner.git(repo.path, ["rev-parse", "HEAD"])
  let marker = repo.path + "/.git/ref-advanced"
  let hook = repo.path + "/.git/hooks/reference-transaction"
  try "#!/bin/sh\nif [ \"$1\" = committed ]; then\n: > '\(marker)'\nsleep 30\nfi\n".write(
    toFile: hook, atomically: true, encoding: .utf8)
  try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: hook)
  let cancel = Cancellation()
  let worker = Thread {
    let deadline = Date().addingTimeInterval(5)
    while !FileManager.default.fileExists(atPath: marker) && Date() < deadline {
      Thread.sleep(forTimeInterval: 0.01)
    }
    cancel.cancel()
  }
  worker.start()
  do {
    _ = try Commit.commitAndPush(
      repo: repo.path, branch: "main", paths: ["selected"], message: "selected only", cancel: cancel
    )
    Issue.record("Expected cancellation after committing")
  } catch {
    #expect(String(describing: error).contains("Committed"))
  }
  #expect(try Runner.git(repo.path, ["rev-parse", "HEAD"]) != before)
  #expect(try Runner.git(repo.path, ["diff", "--cached", "--name-only"]).isEmpty)
  #expect(!FileManager.default.fileExists(atPath: repo.path + "/.git/index.lock"))
}
