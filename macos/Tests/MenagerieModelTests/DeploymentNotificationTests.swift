import Foundation
import MenagerieCore
import Testing

@testable import Menagerie

@MainActor @Test(arguments: [false, true])
func deploymentCompletionNotifiesForSyncAndRollback(fails: Bool) async throws {
  let repo = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  try FileManager.default.createDirectory(at: repo, withIntermediateDirectories: true)
  defer { try? FileManager.default.removeItem(at: repo) }
  _ = try Runner.git(repo.path, ["init", "-q", "-b", "main"])
  var requests = 0
  var delivered: [DeploymentNotifications.Completion] = []
  let notifications = DeploymentNotifications(
    requestAuthorization: {
      requests += 1
      return true
    }, post: { delivered.append($0) })
  let model = Model(notifications: notifications)
  model.fleet = try Fleet.parse(
    "version: 1\nmachines: {test: {agent: 'http://localhost:9184', sync: {preset: nix-darwin, repo: '\(repo.path)'}}}"
  )
  #expect(requests == 0 && delivered.isEmpty)
  for action in [Deployment.Action.sync, .rollback] {
    let pending = model.deploy(
      "test", action: action,
      run: { _, _, _ in
        if fails { throw FleetError("fixture rebuild failed") }
      })
    await pending?.value
    #expect(model.jobs["test"]?.running == false)
  }
  #expect(requests == 1)
  #expect(
    delivered.map(\.title) == [
      "test: Sync \(fails ? "failed" : "succeeded")",
      "test: Rollback \(fails ? "failed" : "succeeded")",
    ])
  #expect(
    delivered.allSatisfy {
      $0.body == (fails ? "fixture rebuild failed" : "Deployment completed successfully.")
    })
}

@MainActor @Test func notificationAuthorizationDenialIsRequestedOnlyOnce() async {
  var requests = 0
  let notifications = DeploymentNotifications(
    requestAuthorization: {
      requests += 1
      return false
    }, post: { _ in Issue.record("Denied notifications must not be posted") })
  #expect(requests == 0)
  await notifications.completed(machine: "test", action: .sync, failure: nil)
  await notifications.completed(machine: "test", action: .rollback, failure: "failed")
  #expect(requests == 1)
}
