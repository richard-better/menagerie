import Darwin
import Foundation
import Synchronization

public struct Change: Sendable, Equatable, Identifiable {
  public var path: String
  public var kind: String
  public var lines: LineCounts? = nil
  public var id: String { path }
  public struct LineCounts: Sendable, Equatable {
    public var added: Int
    public var removed: Int
  }
}
public enum Commit {
  public static func oneLine(_ text: String) -> String {
    text.unicodeScalars.map { $0.properties.generalCategory == .control ? " " : String($0) }
      .joined()
  }
  public static func worktreeRoot(_ repo: String, cancel: Cancellation = Cancellation()) throws
    -> String
  {
    let output = try Runner.git(repo, ["rev-parse", "--show-toplevel"], cancel: cancel)
    return output.hasSuffix("\n") ? String(output.dropLast()) : output
  }
  public static func changes(repo: String) throws -> [Change] {
    let repo = try worktreeRoot(repo)
    let numstat = try Runner.git(repo, ["diff", "HEAD", "--numstat", "--no-renames", "-z"])
    var counts: [String: Change.LineCounts] = [:]
    for entry in numstat.split(separator: "\0") {
      let fields = entry.split(separator: "\t", maxSplits: 2, omittingEmptySubsequences: false)
      if fields.count == 3, let added = Int(fields[0]), let removed = Int(fields[1]) {
        counts[String(fields[2])] = Change.LineCounts(added: added, removed: removed)
      }
    }
    return try Runner.git(
      repo,
      ["-c", "status.renames=false", "status", "--porcelain=v1", "-z", "--untracked-files=all"]
    )
    .split(separator: "\0").compactMap { entry in
      guard entry.utf8.count > 3 else { return nil }
      let code = String(entry.prefix(2))
      let path = String(entry.dropFirst(3))
      var lines = counts[path]
      if code == "??", let body = smallText(repo: repo, path: path) {
        lines = Change.LineCounts(
          added: body.isEmpty
            ? 0
            : body.split(separator: "\n", omittingEmptySubsequences: false).count
              - (body.hasSuffix("\n") ? 1 : 0), removed: 0)
      }
      return Change(
        path: path,
        kind: code == "??" || code.contains("A")
          ? "new" : code.contains("D") ? "deleted" : code.contains("U") ? "conflicted" : "modified",
        lines: lines)
    }
  }
  public static func template(_ changes: [Change]) -> String {
    var names: [String] = []
    for change in changes {
      let name = change.path.split(separator: "/").suffix(2).joined(separator: "/")
      if !names.contains(name) { names.append(name) }
    }
    if names.isEmpty { return "chore: update configuration" }
    return "chore: update " + names.prefix(3).joined(separator: ", ")
      + (names.count > 3 ? " and \(names.count - 3) more" : "")
  }
  public static func looksSecret(_ path: String) -> Bool {
    let name = URL(fileURLWithPath: path).lastPathComponent.lowercased()
    return name.hasPrefix(".env") || name.hasPrefix("id_")
      || [".key", ".pem", ".p12", ".pfx", ".age", ".gpg", ".kdbx"].contains(where: name.hasSuffix)
      || ["secret", "token", "credential", "password", "private"].contains(where: name.contains)
  }
  /// Walk with openat and O_NOFOLLOW, including every parent directory.
  /// This also closes the lstat/read race present in ordinary file reads.
  public static func smallText(repo: String, path: String) -> String? {
    let parts = path.split(separator: "/", omittingEmptySubsequences: false)
    guard !parts.isEmpty, parts.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." }) else {
      return nil
    }
    var fd = open(repo, O_RDONLY | O_DIRECTORY | O_CLOEXEC)
    guard fd >= 0 else { return nil }
    defer { close(fd) }
    for (index, component) in parts.enumerated() {
      let next = openat(
        fd, String(component),
        O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK | (index < parts.count - 1 ? O_DIRECTORY : 0)
      )
      guard next >= 0 else { return nil }
      close(fd)
      fd = next
    }
    var info = stat()
    guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_size <= 65536 else {
      return nil
    }
    var bytes = [UInt8](repeating: 0, count: 65537)
    var count = 0
    while count < bytes.count {
      let n = bytes.withUnsafeMutableBytes {
        read(fd, $0.baseAddress!.advanced(by: count), 65537 - count)
      }
      if n == 0 { break }
      if n < 0 { return nil }
      count += n
    }
    guard count <= 65536 else { return nil }
    return String(bytes: bytes.prefix(count), encoding: .utf8)
  }
  public static func draftInput(repo: String, changes: [Change]) throws -> (
    text: String, withheld: [String]
  ) {
    let repo = try worktreeRoot(repo)
    let shared = changes.filter { !looksSecret($0.path) }
    var tracked: [String] = []
    var untracked: [String] = []
    for change in shared {
      if (try? Runner.git(repo, ["ls-files", "--error-unmatch", "--", change.path])) != nil {
        tracked.append(change.path)
      } else {
        untracked.append(change.path)
      }
    }
    var text =
      tracked.isEmpty
      ? "" : try Runner.git(repo, ["diff", "HEAD", "--no-renames", "--no-color", "--"] + tracked)
    for path in untracked {
      text += "\nNew file \(path):\n" + (smallText(repo: repo, path: path) ?? "(not shown)") + "\n"
      if text.utf8.count > 200 * 1024 { break }
    }
    if text.utf8.count > 200 * 1024 {
      text = String(decoding: text.utf8.prefix(200 * 1024), as: UTF8.self) + "\n[diff truncated]\n"
    }
    return (text, changes.filter { looksSecret($0.path) }.map(\.path))
  }
  public static func cleanMessage(_ text: String) -> String? {
    text.components(separatedBy: .newlines).map { $0.trimmingCharacters(in: .whitespaces) }
      .first { !$0.isEmpty && !$0.hasPrefix("```") }?
      .trimmingCharacters(in: CharacterSet(charactersIn: "\"'` "))
  }
  public static func draft(
    command: String, repo: String, input: String, cancel: Cancellation = Cancellation()
  ) throws -> String {
    let repo = try worktreeRoot(repo, cancel: cancel)
    let output = try Runner.run(
      Command("/bin/sh", ["-lc", command], directory: repo, input: Data(input.utf8)), cancel: cancel
    ).checked()
    guard let message = cleanMessage(output), !message.isEmpty else {
      throw FleetError("Draft command printed no message")
    }
    return message
  }
  static func uncommittedCount(repo: String, cancel: Cancellation) throws -> Int {
    let count = Mutex(0)
    let result = try Runner.run(
      Command(
        "/usr/bin/git",
        [
          "-C", repo,
          "-c", "status.renames=false", "status", "--porcelain=v1", "-z", "--untracked-files=all",
          "--ignore-submodules=none",
        ], timeout: 60),
      standardOutput: { bytes in
        let records = bytes.reduce(0) { $0 + ($1 == 0 ? 1 : 0) }
        count.withLock { $0 += records }
      }, cancel: cancel)
    _ = try result.checked()
    return count.withLock { $0 }
  }

  public static func pushCheckout(
    repo: String, branch: String, cancel: Cancellation, output: @Sendable (String) -> Void
  ) throws {
    guard
      try Runner.git(repo, ["symbolic-ref", "--quiet", "HEAD"], cancel: cancel).trimmingCharacters(
        in: .whitespacesAndNewlines) == "refs/heads/" + branch
    else { throw FleetError("Checkout is not on the configured branch") }
    let count = try uncommittedCount(repo: repo, cancel: cancel)
    if count > 0 {
      output(
        "\(count) uncommitted \(count == 1 ? "file stays" : "files stay") behind: Sync deploys commits only.\n"
      )
    }
    _ = try Runner.git(repo, ["fetch", "-q", "origin", "refs/heads/" + branch], cancel: cancel)
    let ahead = try Runner.git(repo, ["rev-list", "--count", "FETCH_HEAD..HEAD"], cancel: cancel)
      .trimmingCharacters(in: .whitespacesAndNewlines)
    if ahead == "0" {
      output("Nothing to push.\n")
      return
    }
    if (try? Runner.git(
      repo, ["merge-base", "--is-ancestor", "FETCH_HEAD", "HEAD"], cancel: cancel)) == nil
    {
      try CheckoutMutation.rebase(repo, cancel: cancel)
    }
    try CheckoutMutation.assertMerged(repo)
    _ = try Runner.git(repo, ["push", "origin", "HEAD:refs/heads/" + branch], cancel: cancel)
  }
  public static func commitAndPush(
    repo: String, branch: String, paths: [String], message: String,
    cancel: Cancellation = Cancellation()
  ) throws -> String {
    let root = try worktreeRoot(repo, cancel: cancel)
    return try CheckoutMutation.run(repos: [root], cancel: cancel) {
      try commitAndPushUnlocked(
        repo: root, branch: branch, paths: paths, message: message, cancel: cancel)
    }
  }
  private static func createSelectedCommit(
    repo: String, branch: String, paths: [String], message: String, cancel: Cancellation
  ) throws {
    func rejectDirectories() throws {
      for path in paths {
        let components = path.split(separator: "/", omittingEmptySubsequences: false)
        guard !components.isEmpty,
          components.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." })
        else { throw FleetError("Invalid selected path") }
        var info = stat()
        if lstat(repo + "/" + path, &info) == 0, info.st_mode & S_IFMT == S_IFDIR {
          throw FleetError("Selected file is now a directory: \(path). Reload changes first.")
        }
      }
    }
    try rejectDirectories()
    let temporary = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(
      at: temporary, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    defer { try? FileManager.default.removeItem(at: temporary) }
    var environment = ProcessInfo.processInfo.environment
    environment["GIT_INDEX_FILE"] = temporary.appendingPathComponent("index").path
    environment["GIT_EDITOR"] = ":"
    environment["GIT_LITERAL_PATHSPECS"] = "1"
    environment["GIT_TERMINAL_PROMPT"] = "0"
    environment["GIT_NO_LAZY_FETCH"] = "1"
    func git(_ args: [String], input: Data = Data()) throws -> String {
      var command = Command("/usr/bin/git", ["-C", repo] + args, input: input, timeout: 300)
      command.environment = environment
      return try Runner.run(command, cancel: cancel).checked()
    }
    let head = try git(["rev-parse", "HEAD"]).trimmingCharacters(in: .whitespacesAndNewlines)
    _ = try git(["read-tree", head])
    _ = try git(["add", "-A", "--"] + paths)
    let messageFile = temporary.appendingPathComponent("message")
    try Data((message + "\n").utf8).write(to: messageFile)
    _ = try git(["hook", "run", "--ignore-missing", "pre-commit"])
    _ = try git([
      "hook", "run", "--ignore-missing", "prepare-commit-msg", "--", messageFile.path, "message",
    ])
    _ = try git(["hook", "run", "--ignore-missing", "commit-msg", "--", messageFile.path])
    try rejectDirectories()
    let tree = try git(["write-tree"]).trimmingCharacters(in: .whitespacesAndNewlines)
    let changed = try git(["diff", "--name-only", "--no-renames", "-z", head, tree, "--"])
      .split(separator: "\0").map(String.init)
    guard Set(changed) == Set(paths) else {
      throw FleetError("Selected files changed while preparing the commit. Reload changes first.")
    }
    var arguments = ["commit-tree", tree, "-p", head, "-F", messageFile.path]
    if (try? git(["config", "--bool", "--get", "commit.gpgsign"]))?
      .trimmingCharacters(in: .whitespacesAndNewlines) == "true"
    {
      arguments.append("-S")
    }
    let commit = try git(arguments).trimmingCharacters(in: .whitespacesAndNewlines)
    try rejectDirectories()
    _ = try git([
      "update-ref", "-m", "commit: " + oneLine(message), "refs/heads/" + branch, commit, head,
    ])
    // Update exact index entries, preserving unrelated staged files and avoiding directory pathspecs.
    let entries = try git(["ls-tree", "-r", "-z", commit, "--"] + paths)
    var index = paths.map { "0 " + String(repeating: "0", count: head.count) + "\t" + $0 + "\0" }
      .joined()
    for entry in entries.split(separator: "\0") {
      let fields = entry.split(separator: " ", maxSplits: 2)
      guard fields.count == 3 else { continue }
      index += String(fields[0]) + " " + String(fields[2]) + "\0"
    }
    _ = try Runner.run(
      Command(
        "/usr/bin/git", ["-C", repo, "update-index", "-z", "--index-info"], input: Data(index.utf8)),
      cancel: cancel
    ).checked()
    _ = try? Runner.git(repo, ["hook", "run", "--ignore-missing", "post-commit"], cancel: cancel)
  }
  private static func commitAndPushUnlocked(
    repo: String, branch: String, paths: [String], message: String, cancel: Cancellation
  ) throws -> String {
    guard !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, !paths.isEmpty else {
      throw FleetError("Choose files and write a message first")
    }
    guard
      try Runner.git(repo, ["symbolic-ref", "--quiet", "HEAD"], cancel: cancel).trimmingCharacters(
        in: .whitespacesAndNewlines) == "refs/heads/" + branch
    else { throw FleetError("Checkout is not on \(branch)") }
    try createSelectedCommit(
      repo: repo, branch: branch, paths: paths, message: message, cancel: cancel)
    let committed = try Runner.git(repo, ["rev-parse", "--short", "HEAD"])
    do {
      _ = try Runner.git(repo, ["fetch", "-q", "origin", "refs/heads/" + branch], cancel: cancel)
      if (try? Runner.git(
        repo, ["merge-base", "--is-ancestor", "FETCH_HEAD", "HEAD"], cancel: cancel)) == nil
      {
        try CheckoutMutation.rebase(repo, cancel: cancel)
      }
      try CheckoutMutation.assertMerged(repo)
      _ = try Runner.git(
        repo, ["push", "-q", "origin", "HEAD:refs/heads/" + branch], cancel: cancel)
      return try Runner.git(repo, ["rev-parse", "--short", "HEAD"]).trimmingCharacters(
        in: .whitespacesAndNewlines)
    } catch {
      throw FleetError(
        "Committed \(committed.trimmingCharacters(in: .whitespacesAndNewlines)), but push did not finish: \(error). The commit remains in your checkout."
      )
    }
  }
}
