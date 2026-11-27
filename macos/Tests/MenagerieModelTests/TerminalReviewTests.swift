import Foundation
import Testing

@testable import Menagerie
@testable import MenagerieCore

@MainActor @Test func reopeningTerminalReusesLiveShellAndReplacesFinishedShell() async throws {
  let model = Model()
  model.fleet = try Fleet.parse("version: 1\nmachines: {test: {agent: 'http://localhost:9184'}}")
  model.selected = "test"
  model.openTerminal()
  let first = try #require(model.terminals["test"])
  model.openTerminal()
  #expect(model.terminals["test"] === first)
  first.transport.stop()
  for _ in 0..<300 where !first.transport.finished { try await Task.sleep(for: .milliseconds(10)) }
  try #require(first.transport.finished)
  model.openTerminal()
  #expect(model.terminals["test"] !== first)
  model.terminals["test"]?.transport.stop()
}

@MainActor @Test func oversizedPasteSendsNothingAndShowsItsSize() {
  let view = TerminalGridView()
  let transport = PTYSession()
  view.transport = transport
  view.pasteText(String(repeating: "a", count: TerminalInput.pasteLimit + 1))
  #expect(transport.input.pendingBytes == 0)
  #expect(view.pasteNotice == "Not pasted: 257 KB is more than the 256 KB limit.")
  view.pasteText("safe")
  #expect(transport.input.first() == Data("safe".utf8) && view.pasteNotice == nil)
}
