import Foundation
import Testing

@testable import MenagerieCore

private final class FixtureProtocol: URLProtocol, @unchecked Sendable {
  override class func canInit(with request: URLRequest) -> Bool { true }
  override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
  override func startLoading() {
    let host = request.url!.host!
    let empty: [String: Any] = [
      "value": NSNull(), "at": NSNull(), "every_s": 10, "error": NSNull(),
    ]
    var probes = Dictionary(
      uniqueKeysWithValues: [
        "load", "memory", "disk", "battery", "temperature", "revision", "system",
      ].map { ($0, empty) })
    let rev =
      host == "bad-rev.test"
      ? "--upload-pack=bad"
      : request.url!.pathComponents.dropFirst().first(where: { Revision.isValid($0) })
        ?? "abcdef1234"
    probes["revision"] = [
      "value": ["rev": rev, "dirty": false, "source": "nix-darwin"], "at": 1000, "every_s": 10,
      "error": NSNull(),
    ]
    let data = try! JSONSerialization.data(withJSONObject: [
      "version": host == "bad-version.test" ? 99 : 1, "host": "fixture",
      "agent": ["version": "1", "now": 1000], "probes": probes,
    ])
    let response = HTTPURLResponse(
      url: request.url!, statusCode: host == "down.test" ? 503 : 200, httpVersion: "HTTP/1.1",
      headerFields: ["Content-Type": "application/json"])!
    client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
    client?.urlProtocol(self, didLoad: data)
    client?.urlProtocolDidFinishLoading(self)
  }
  override func stopLoading() {}
}
private func session() -> URLSession {
  let config = URLSessionConfiguration.ephemeral
  config.protocolClasses = [FixtureProtocol.self]
  return URLSession(configuration: config)
}
@Test func agentHTTPAndServiceHealthAreValidated() async throws {
  let fleet = try Fleet.parse(
    """
    version: 1
    machines:
      good:
        agent: http://good.test/
        services:
          - {name: Up, url: 'http://good.test', health: 'http://good.test'}
          - {name: Down, url: 'http://down.test', health: 'http://down.test'}
      bad-version: {agent: 'http://bad-version.test'}
      bad-rev: {agent: 'http://bad-rev.test'}
    """)
  let http = session()
  defer { http.invalidateAndCancel() }
  let result = await Poller.poll(fleet, known: [:], session: http)
  #expect(result.observations["good"]?.report?.host == "fixture")
  #expect(result.observations["good"]?.report?.loadHistory.isEmpty == true)
  #expect(
    result.observations["bad-version"]?.report == nil
      && result.observations["bad-version"]?.error != nil)
  #expect(
    result.observations["bad-rev"]?.report == nil && result.observations["bad-rev"]?.error != nil)
  #expect(result.health["good/0"]?.hasPrefix("up ·") == true)
  #expect(result.health["good/1"]?.hasPrefix("down ·") == true)
}
@Test func pollingCountsCommitsAgainstUpstream() async throws {
  let repo = try Repository()
  defer { repo.remove() }
  let old = try Runner.git(repo.path, ["rev-parse", "HEAD"]).trimmingCharacters(
    in: .whitespacesAndNewlines)
  for message in ["second", "third"] {
    _ = try Runner.git(repo.path, ["commit", "-qm", message, "--allow-empty"])
  }
  _ = try Runner.git(repo.path, ["update-ref", "refs/remotes/origin/main", "HEAD"])
  _ = try Runner.git(repo.path, ["commit", "-qm", "local", "--allow-empty"])
  let fleet = try Fleet.parse(
    "version: 1\nrepo: '\(repo.path)'\nmachines: { old: {agent: 'http://good.test/\(old)'} }")
  let http = session()
  defer { http.invalidateAndCancel() }
  let result = await Poller.poll(fleet, known: [:], session: http)
  #expect(result.drift[old] == Counts(ahead: 0, behind: 2))
  #expect(result.checkout?.counts == Counts(ahead: 1, behind: 0))
  #expect(result.checkout?.dirty == false)
}
