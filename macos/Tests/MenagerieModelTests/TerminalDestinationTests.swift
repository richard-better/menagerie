import AppKit
import MenagerieCore
import Testing

@testable import Menagerie

@MainActor @Test func localTerminalUsesSelectedCheckoutAndHomeFallback() throws {
  let fleet = try Fleet.parse(
    """
    version: 1
    repo: /controller
    machines:
      selected: {agent: 'http://localhost:9184', sync: {preset: custom, repo: ~/selected, command: echo}}
      unmanaged: {agent: 'http://localhost:9184'}
    """)
  #expect(
    TerminalSession.command(machine: fleet.machines["selected"]!, name: "selected")?.directory
      == NSHomeDirectory() + "/selected")
  #expect(
    TerminalSession.command(machine: fleet.machines["unmanaged"]!, name: "unmanaged")?.directory
      == NSHomeDirectory())
}
