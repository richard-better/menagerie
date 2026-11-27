import Foundation
import MenagerieCore
import UserNotifications

@MainActor final class DeploymentNotifications {
  struct Completion: Equatable {
    let title: String
    let body: String
  }
  static let shared = DeploymentNotifications(
    requestAuthorization: {
      try await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])
    },
    post: { completion in
      let content = UNMutableNotificationContent()
      content.title = completion.title
      content.body = completion.body
      content.sound = .default
      try await UNUserNotificationCenter.current().add(
        UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
    })
  private let requestAuthorization: @MainActor () async throws -> Bool
  private let post: @MainActor (Completion) async throws -> Void
  private var authorization: Task<Bool, Never>?
  init(
    requestAuthorization: @escaping @MainActor () async throws -> Bool,
    post: @escaping @MainActor (Completion) async throws -> Void
  ) {
    self.requestAuthorization = requestAuthorization
    self.post = post
  }
  func completed(machine: String, action: Deployment.Action, failure: String?) async {
    if authorization == nil {
      authorization = Task { (try? await requestAuthorization()) ?? false }
    }
    guard await authorization!.value else { return }
    let verb = action == .sync ? "Sync" : "Rollback"
    try? await post(
      Completion(
        title: "\(machine): \(verb) \(failure == nil ? "succeeded" : "failed")",
        body: failure ?? "Deployment completed successfully."))
  }
}
