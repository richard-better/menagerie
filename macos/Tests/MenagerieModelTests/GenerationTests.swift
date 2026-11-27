import Foundation
import Testing

@testable import Menagerie
@testable import MenagerieCore

private actor Gate {
  var continuation: CheckedContinuation<Void, Never>?
  var released = false
  func suspend() async {
    if released { return }
    await withCheckedContinuation { continuation = $0 }
  }
  func release() {
    released = true
    continuation?.resume()
    continuation = nil
  }
}
@MainActor @Test func stalePollForAnOldFleetGenerationIsDropped() async throws {
  let path = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: path) }
  try "version: 1\nmachines: { test: { agent: 'http://localhost:9184' } }".write(
    to: path, atomically: true, encoding: .utf8)
  let model = Model(path: path.path)
  await model.reload()
  let generation = model.generation
  let gate = Gate()
  let (started, signal) = AsyncStream<Void>.makeStream()
  let poll = Task {
    await model.poll(using: { _, _ in
      signal.yield()
      await gate.suspend()
      var result = PollResult()
      result.observations["test"] = LiveReport(error: "old result")
      result.health["test/0"] = "old health"
      result.driftErrors["abcdef1234"] = "old comparison error"
      return result
    })
  }
  for await _ in started.prefix(1) {}
  try "version: 1\nmachines: { test: { agent: 'http://localhost:9999' } }".write(
    to: path, atomically: true, encoding: .utf8)
  await model.reload()
  #expect(model.generation == generation + 1)
  await gate.release()
  await poll.value
  #expect(model.observations.isEmpty)
  #expect(model.health.isEmpty)
  #expect(model.driftErrors.isEmpty)
  #expect(model.updated == nil)
  await model.poll(using: { _, _ in
    var result = PollResult()
    result.health["test/0"] = "new health"
    return result
  })
  #expect(model.health["test/0"] == "new health")
}

@MainActor @Test func identicalAndReorderedFleetPreservesGenerationAndReports() async throws {
  let path = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: path) }
  try "version: 1\nmachines: { a: {}, b: {} }".write(to: path, atomically: true, encoding: .utf8)
  let model = Model(path: path.path)
  await model.reload()
  let generation = model.generation
  await model.poll(using: { _, _ in
    var result = PollResult()
    result.observations["a"] = LiveReport(error: "retained")
    return result
  })
  await model.reload()
  #expect(model.generation == generation)
  #expect(model.observations["a"]?.error == "retained")
  try "version: 1\nmachines: { b: {}, a: {} }".write(to: path, atomically: true, encoding: .utf8)
  await model.reload()
  #expect(model.fleet?.order == ["b", "a"])
  #expect(model.generation == generation)
  #expect(model.observations["a"]?.error == "retained")
}
