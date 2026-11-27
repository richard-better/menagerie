import Foundation
import MenagerieCore
import Testing

@testable import Menagerie

@MainActor @Test func expiredReportsCannotSupplyHeaderChangesOrComparison() throws {
  let empty: [String: Any] = ["value": NSNull(), "at": NSNull(), "every_s": 10, "error": NSNull()]
  var probes = Dictionary(
    uniqueKeysWithValues: [
      "load", "memory", "disk", "battery", "temperature", "revision", "system",
    ].map { ($0, empty) })
  probes["revision"] = [
    "value": ["rev": "abcdef1234", "dirty": false, "source": "nix-darwin"], "at": 1000,
    "every_s": 10, "error": NSNull(),
  ]
  probes["system"] = [
    "value": ["os": "Darwin", "uptime_s": 3600], "at": 1000, "every_s": 10, "error": NSNull(),
  ]
  let report = try JSONDecoder().decode(
    AgentStatus.self,
    from: JSONSerialization.data(withJSONObject: [
      "version": 1, "host": "test", "agent": ["version": "1", "now": 1000], "probes": probes,
    ]))
  let model = Model()
  model.fleet = try Fleet.parse(
    "version: 1\nrepo: /unused\nmachines: {test: {agent: 'http://localhost:9184', sync: {preset: nix-darwin}}}"
  )
  let received = Date()
  model.observations["test"] = LiveReport(report: report, received: received)
  model.changes["abcdef1234"] = "missing commit"
  model.driftErrors["abcdef1234"] = "old comparison"
  #expect(model.detailsReport("test", now: received)?.probes.system.value?.uptimeS == 3600)
  #expect(model.missingChanges("test", now: received) == "missing commit")
  #expect(model.comparisonNote("test", now: received)?.contains("old comparison") == true)
  let expired = received.addingTimeInterval(35)
  #expect(model.detailsReport("test", now: expired) == nil)
  #expect(model.missingChanges("test", now: expired) == nil)
  #expect(model.comparisonNote("test", now: expired)?.contains("old comparison") == false)
}
