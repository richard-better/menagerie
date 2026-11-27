import AppKit
import MenagerieCore
import Testing

@testable import Menagerie

@MainActor @Test func shellModelDoesNotRetainItsTerminalView() throws {
  let machine = try Fleet.parse("version: 1\nmachines: {test: {}}").machines["test"]!
  let session = TerminalSession(machine: machine, name: "test", directory: nil)
  weak var oldView: TerminalGridView?
  autoreleasepool {
    let view = session.view
    oldView = view
    #expect(session.view === view)
    #expect(view.transport === session.transport)
  }
  #expect(oldView == nil)
  #expect(session.view.transport === session.transport)
}

@MainActor @Test func closingWindowDropsSessionsAndDraftButPreservesSelectionAndJobs() throws {
  let model = Model()
  let fleet = try Fleet.parse("version: 1\nmachines: {first: {}, test: {}}")
  model.fleet = fleet
  let machine = fleet.machines["test"]!
  model.selected = "test"
  model.page = "terminal"
  model.message = "draft"
  model.included = ["file"]
  model.terminals["test"] = TerminalSession(machine: machine, name: "test", directory: nil)
  let job = Job()
  model.jobs["test"] = job
  model.closeWindow()
  #expect(model.selected == "test")
  #expect(model.page == "details")
  #expect(model.terminals.isEmpty)
  #expect(model.message.isEmpty && model.included.isEmpty)
  #expect(model.jobs["test"] === job && !job.cancel.isCancelled)
}
