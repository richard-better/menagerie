import Foundation
import Testing

@testable import MenagerieCore

@Test(arguments: [false, true])
func rollbackDoesNotPreflightUnusedCheckouts(remote: Bool) throws {
  let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
  defer { try? FileManager.default.removeItem(at: directory) }
  if remote {
    _ = try Runner.git(directory.path, ["init", "-q"])
    try Data("ongoing operation".utf8).write(
      to: directory.appendingPathComponent(".git/MERGE_HEAD"))
  }
  let machine =
    remote
    ? "ssh: test@remote, sync: {preset: home-manager}"
    : "agent: 'http://localhost:9184', sync: {preset: nix-darwin}"
  let fleet = try Fleet.parse(
    "version: 1\nrepo: '\(directory.path)'\nmachines: {test: {\(machine)}}")
  var steps = try Deployment.plan(.rollback, fleet: fleet, name: "test")
  #expect(steps.flatMap(\.checkouts).isEmpty)
  for index in steps.indices {
    steps[index].command = Command("/usr/bin/true")
    steps[index].administrator = false
  }
  try Deployment.run(steps, cancel: Cancellation(), output: { _ in })
}
@Test func syncPreflightTracksOnlyCheckoutsUsedByItsSteps() throws {
  let fleet = try Fleet.parse(
    "version: 1\nrepo: /controller\nmachines: {test: {agent: 'http://localhost:9184', sync: {preset: nix-darwin, repo: /target}}}"
  )
  let steps = try Deployment.plan(.sync, fleet: fleet, name: "test")
  #expect(steps.map(\.checkouts) == [["/controller"], ["/target"], ["/target"]])
}
