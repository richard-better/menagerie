import Foundation
import Testing

@testable import Menagerie
@testable import MenagerieCore

private actor ReviewGate {
  private var released = false
  private var continuation: CheckedContinuation<Void, Never>?
  func wait() async { if !released { await withCheckedContinuation { continuation = $0 } } }
  func release() {
    released = true
    continuation?.resume()
    continuation = nil
  }
}
private func reviewFleet(_ repo: String, branch: String = "main", command: String = "echo draft")
  throws -> Fleet
{
  try Fleet.parse(
    "version: 1\nrepo: \(repo)\nbranch: \(branch)\ncommit_message: \(command)\nmachines: {}")
}
@MainActor @Test func commitLoadsClearSelectionAndDiscardStaleResults() async throws {
  let model = Model()
  model.fleet = try reviewFleet("/old")
  model.files = [Change(path: "previous", kind: "new")]
  model.included = ["previous"]
  let gate = ReviewGate()
  let (entered, signal) = AsyncStream<Void>.makeStream()
  let oldTask = model.loadChanges(using: { _ in
    signal.yield()
    await gate.wait()
    return [Change(path: "old", kind: "new")]
  })
  let old = try #require(oldTask)
  #expect(model.files.isEmpty && model.included.isEmpty)
  for await _ in entered.prefix(1) {}
  model.fleet = try reviewFleet("/new", branch: "next", command: "echo new")
  let newTask = model.loadChanges(using: { _ in [Change(path: "new", kind: "new")] })
  let new = try #require(newTask)
  await new.value
  await gate.release()
  await old.value
  #expect(model.files.map(\.path) == ["new"] && model.included == ["new"])
  #expect(model.commitContext?.repo == "/new" && model.commitContext?.branch == "next")
}
@MainActor @Test func draftsCannotOverwriteAChangedCommitContextOrSelection() async throws {
  let model = Model()
  model.fleet = try reviewFleet("/old")
  await model.loadChanges(using: { _ in [Change(path: "file", kind: "new")] })?.value
  let gate = ReviewGate()
  let (entered, signal) = AsyncStream<Void>.makeStream()
  let draftTask = model.draft(using: { command, repo, _ in
    #expect(repo == "/old" && command == "echo draft")
    signal.yield()
    await gate.wait()
    return ("stale message", [])
  })
  let draft = try #require(draftTask)
  for await _ in entered.prefix(1) {}
  model.fleet = try reviewFleet("/new", command: "echo new")
  await model.loadChanges(using: { _ in [Change(path: "new", kind: "new")] })?.value
  let message = model.message
  await gate.release()
  await draft.value
  #expect(model.message == message && !model.drafting)
}
@MainActor @Test func popoverJobsSelectTheirMachineAndShareCheckoutExclusion() async throws {
  let directory = try modelReviewRepository()
  defer { try? FileManager.default.removeItem(at: directory) }
  let model = Model()
  model.fleet = try Fleet.parse(
    """
    version: 1
    repo: /test-checkout
    machines:
      first: {agent: 'http://localhost:9184', sync: {preset: custom, repo: /test-checkout, command: echo}}
      clicked: {agent: 'http://localhost:9184', sync: {preset: custom, repo: /test-checkout, command: echo}}
    """)
  model.selected = "first"
  let gate = ReviewGate()
  model.deploy("clicked", action: .sync, run: { _, _, _ in await gate.wait() })
  #expect(
    model.selected == "clicked" && model.page == "log" && model.jobs["clicked"]?.running == true)
  model.deploy("first", action: .sync, run: { _, _, _ in Issue.record("Concurrent checkout job") })
  #expect(model.jobs["first"] == nil)
  model.jobs["clicked"]?.cancel.cancel()
  #expect(model.jobs["clicked"]?.cancel.isCancelled == true)
  await gate.release()
}
@MainActor @Test(arguments: ["repo", "branch", "command"])
func commitRefusesChangedCheckoutSettings(setting: String) async throws {
  let model = Model()
  model.fleet = try reviewFleet("/old")
  await model.loadChanges(using: { _ in [Change(path: "selected", kind: "new")] })?.value
  model.fleet = try reviewFleet(
    setting == "repo" ? "/new" : "/old", branch: setting == "branch" ? "next" : "main",
    command: setting == "command" ? "echo next" : "echo draft")
  model.commit()
  #expect(!model.committing)
}
@MainActor @Test func draftIsDiscardedWhenSelectionChanges() async throws {
  let model = Model()
  model.fleet = try reviewFleet("/old")
  await model.loadChanges(using: { _ in [Change(path: "selected", kind: "new")] })?.value
  let before = model.message
  let gate = ReviewGate()
  let (entered, signal) = AsyncStream<Void>.makeStream()
  let pending = model.draft(using: { _, _, _ in
    signal.yield()
    await gate.wait()
    return ("stale", [])
  })
  let task = try #require(pending)
  for await _ in entered.prefix(1) {}
  model.included = []
  await gate.release()
  await task.value
  #expect(model.message == before)
}

@MainActor @Test func blockedGitDoesNotBlockSubsequentNetworkPolls() async throws {
  let model = Model()
  model.fleet = try reviewFleet("/unused")
  let gate = ReviewGate()
  let (entered, signal) = AsyncStream<Void>.makeStream()
  let pending = model.refreshComparison(using: { _, _, _ in
    signal.yield()
    await gate.wait()
    return PollResult()
  })
  let comparison = try #require(pending)
  for await _ in entered.prefix(1) {}
  let duplicate = model.refreshComparison(using: { _, _, _ in
    Issue.record("Overlapping Git comparison")
    return PollResult()
  })
  #expect(duplicate == nil)
  await model.poll(using: { _, _ in
    var result = PollResult()
    result.health["machine/0"] = "first"
    return result
  })
  await model.poll(using: { _, _ in
    var result = PollResult()
    result.health["machine/0"] = "second"
    return result
  })
  #expect(model.health["machine/0"] == "second")
  await gate.release()
  await comparison.value
}

private func modelReviewRepository() throws -> URL {
  let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
  _ = try Runner.git(directory.path, ["init", "-q", "-b", "main"])
  return directory
}
@MainActor @Test func jobsShareGitDirectoryIdentityAcrossRootAndSubdirectory() async throws {
  let directory = try modelReviewRepository()
  defer { try? FileManager.default.removeItem(at: directory) }
  let subdirectory = directory.appendingPathComponent("nested")
  try FileManager.default.createDirectory(at: subdirectory, withIntermediateDirectories: true)
  func fleet(_ path: String, name: String) throws -> Fleet {
    try Fleet.parse(
      "version: 1\nrepo: '\(path)'\nmachines: {\(name): {agent: 'http://127.0.0.1:1', sync: {preset: custom, repo: '\(path)', command: echo}}}"
    )
  }
  let model = Model()
  let gate = ReviewGate()
  model.fleet = try fleet(directory.path, name: "root")
  let (entered, signal) = AsyncStream<Void>.makeStream()
  let first = model.deploy(
    "root", action: .sync,
    run: { _, _, _ in
      signal.yield()
      await gate.wait()
    })
  for await _ in entered.prefix(1) {}
  model.fleet = try fleet(subdirectory.path, name: "nested")
  let second = model.deploy(
    "nested", action: .sync, run: { _, _, _ in Issue.record("Same Git directory ran concurrently") }
  )
  await second?.value
  #expect(model.jobs["nested"]?.result?.contains("already mutating") == true)
  await gate.release()
  await first?.value
}
