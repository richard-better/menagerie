import Foundation
import Synchronization

public enum CheckoutMutation {
  private static let active = Mutex<Set<String>>([])
  public static func key(_ repo: String) -> String {
    URL(fileURLWithPath: expandHome(repo)).standardizedFileURL.resolvingSymlinksInPath().path
  }
  public static func run<T>(repos: [String], _ body: () throws -> T) throws -> T {
    let keys = Set(repos.map(key))
    guard
      active.withLock({ paths in
        guard paths.isDisjoint(with: keys) else { return false }
        paths.formUnion(keys)
        return true
      })
    else { throw FleetError("A job is already mutating this checkout") }
    defer { active.withLock { $0.subtract(keys) } }
    for repo in keys { try assertIdle(repo) }
    return try body()
  }
  static func assertIdle(_ repo: String) throws {
    for marker in ["rebase-merge", "rebase-apply", "MERGE_HEAD", "CHERRY_PICK_HEAD", "index.lock"] {
      let path = try Runner.git(
        repo, ["rev-parse", "--path-format=absolute", "--git-path", marker]
      ).trimmingCharacters(in: .whitespacesAndNewlines)
      if FileManager.default.fileExists(atPath: path) {
        throw FleetError("Git operation in progress: \(marker)")
      }
    }
  }
  static func rebase(_ repo: String, cancel: Cancellation) throws {
    try assertIdle(repo)
    let started = Mutex(false)
    let result: CommandResult
    do {
      result = try Runner.run(
        Command(
          "/usr/bin/git", ["-C", repo, "rebase", "-q", "--autostash", "FETCH_HEAD"], timeout: 300),
        onStart: { started.withLock { $0 = true } }, cancel: cancel)
      _ = try result.checked()
    } catch {
      if started.withLock({ $0 }) { _ = try? Runner.git(repo, ["rebase", "--abort"]) }
      throw error
    }
    try assertMerged(repo, transcript: result.output + result.errorOutput)
  }
  static func assertMerged(_ repo: String, transcript: String = "") throws {
    if !(try Runner.git(repo, ["ls-files", "-u"])).isEmpty {
      let stash = (try? Runner.git(repo, ["stash", "list"])) ?? ""
      throw FleetError(
        "Commit is rebased but not pushed: autostash restore conflicted. Git kept the changes in the worktree and stash.\n\(transcript)\n\(stash)"
      )
    }
  }
}
