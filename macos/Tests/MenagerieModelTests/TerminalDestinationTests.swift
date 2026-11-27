import AppKit
import MenagerieCore
import Testing

@testable import Menagerie

@MainActor @Test func localTerminalUsesSelectedCheckoutAndHomeFallback() throws {
  let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
  defer { try? FileManager.default.removeItem(at: directory) }
  let fleet = try Fleet.parse(
    """
    version: 1
    repo: /controller
    machines:
      selected: {agent: 'http://localhost:9184', sync: {preset: custom, repo: '\(directory.path)', command: echo}}
      unmanaged: {agent: 'http://localhost:9184'}
    """)
  #expect(
    TerminalSession.command(machine: fleet.machines["selected"]!, name: "selected")?.directory
      == directory.path)
  #expect(
    TerminalSession.command(machine: fleet.machines["unmanaged"]!, name: "unmanaged")?.directory
      == NSHomeDirectory())
}

@MainActor @Test(arguments: ["retarget", "remove", "invalid"])
func fleetReloadEndsSessionsWithChangedDestinations(change: String) async throws {
  let path = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: path) }
  let original = "version: 1\nmachines: {test: {ssh: 'old@invalid.invalid'}}"
  try original.write(to: path, atomically: true, encoding: .utf8)
  let model = Model(path: path.path)
  await model.reload()
  model.selected = "test"
  model.openTerminal()
  let session = try #require(model.terminals["test"])
  defer { session.transport.stop() }
  await model.reload()
  #expect(model.terminals["test"] === session)
  let replacement =
    change == "retarget"
    ? original.replacingOccurrences(of: "old@", with: "new@")
    : change == "remove" ? "version: 1\nmachines: {}" : "invalid: ["
  try replacement.write(to: path, atomically: true, encoding: .utf8)
  await model.reload()
  #expect(model.terminals.isEmpty && model.page == "details")
  #expect(!session.transport.input.enqueue(Data("must not reach old host".utf8)))
}

@MainActor @Test(arguments: [false, true])
func localTerminalFallsBackWhenCheckoutIsNotADirectory(isFile: Bool) throws {
  let path = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: path) }
  if isFile { try Data().write(to: path) }
  let fleet = try Fleet.parse(
    "version: 1\nmachines: {test: {agent: 'http://localhost:9184', sync: {preset: custom, repo: '\(path.path)', command: echo}}}"
  )
  #expect(
    TerminalSession.command(machine: fleet.machines["test"]!, name: "test")?.directory
      == NSHomeDirectory())
}

@MainActor @Test func terminalRequiresALocalOrSSHDestination() throws {
  let model = Model()
  model.fleet = try Fleet.parse(
    "version: 1\nmachines: {none: {}, remote: {ssh: 'test@invalid.invalid'}, local: {agent: 'http://localhost:9184'}}"
  )
  model.selected = "remote"
  #expect(model.canOpenTerminal)
  model.selected = "local"
  #expect(model.canOpenTerminal)
  model.selected = "none"
  #expect(!model.canOpenTerminal)
  model.openTerminal()
  #expect(model.terminals.isEmpty && model.page == "details")
}
