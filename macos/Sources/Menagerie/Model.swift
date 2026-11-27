import AppKit
import MenagerieCore
import Observation

@MainActor @Observable final class Model {
  var fleet: Fleet?
  var error: String?
  var selected = ""
  var observations: [String: LiveReport] = [:]
  var health: [String: String] = [:]
  var drift: [String: Counts] = [:]
  var driftErrors: [String: String] = [:]
  var changes: [String: String] = [:]
  var checkout: Checkout?
  var updated: Date?
  var page = "details"
  var jobs: [String: Job] = [:]
  var files: [Change] = []
  var included: Set<String> = []
  var message = ""
  var commitNote = ""
  var committing = false
  var drafting = false
  var terminals: [String: TerminalSession] = [:]
  func openTerminal() {
    guard let machine = fleet?.machines[selected] else { return }
    if terminals[selected] == nil {
      terminals[selected] = TerminalSession(
        machine: machine, name: selected, directory: fleet?.repoPath)
    }
    page = "terminal"
  }
  private(set) var generation = 0
  let fleetPath: String
  init(path: String = Fleet.path) { fleetPath = path }
  private var started = false
  private var polling = false
  var attention: Int { fleet?.order.filter { status($0).needsSync }.count ?? 0 }
  var summary: String {
    let online = fleet?.order.filter { status($0).online }.count ?? 0
    return "\(attention) needs sync · \(online)/\(fleet?.order.count ?? 0) online"
  }
  func status(_ name: String) -> MachineStatus {
    guard let machine = fleet?.machines[name] else { return .noAgent }
    return (observations[name] ?? LiveReport()).status(machine: machine, drift: drift)
  }
  func comparisonNote(_ name: String) -> String? {
    guard fleet?.machines[name]?.sync != nil else { return nil }
    guard let report = observations[name]?.report,
      report.probes.revision.isFresh(now: report.agent.now),
      let revision = report.probes.revision.value
    else {
      return
        "This machine doesn't report which commit built it. Rebuild it from a configuration that sets system.configurationRevision."
    }
    guard fleet?.repoPath != nil else {
      return "Add repo: to the fleet file to compare this machine with your checkout."
    }
    return driftErrors[revision.rev].map { "Can't compare with your checkout: \($0)" }
  }
  func start() {
    guard !started else { return }
    started = true
    Task {
      await reload()
      Task {
        while !Task.isCancelled {
          if let fleet {
            await Poller.fetch(fleet)
            await poll()
          }
          try? await Task.sleep(for: .seconds(300))
        }
      }
      while !Task.isCancelled {
        await poll()
        try? await Task.sleep(for: .seconds(10))
        await reload()
      }
    }
  }
  func reload() async {
    let path = fleetPath
    let result = await Task.detached { () -> Result<Fleet, any Error> in
      Result { try Fleet.parse(String(contentsOfFile: path, encoding: .utf8)) }
    }.value
    switch result {
    case .success(let new):
      error = nil
      if new.machines != fleet?.machines || new.repo != fleet?.repo || new.branch != fleet?.branch {
        generation += 1
        observations = [:]
        health = [:]
        drift = [:]
        driftErrors = [:]
        checkout = nil
        changes = [:]
      }
      fleet = new
      if new.machines[selected] == nil { selected = new.order.first ?? "" }
    case .failure(let failure):
      if fleet != nil {
        generation += 1
        observations = [:]
        health = [:]
        drift = [:]
        driftErrors = [:]
        checkout = nil
        changes = [:]
      }
      fleet = nil
      error = String(describing: failure)
    }
  }
  func poll(
    using fetch: @Sendable (Fleet, [String: LiveReport]) async -> PollResult = {
      await Poller.poll($0, known: $1)
    }
  ) async {
    guard !polling, let fleet else { return }
    polling = true
    defer { polling = false }
    let generation = generation
    let result = await fetch(fleet, observations)
    guard generation == self.generation else { return }
    for (name, var observation) in result.observations {
      if observation.report == nil {
        observation.report = observations[name]?.report
        observation.received = observations[name]?.received
      }
      observations[name] = observation
    }
    health = result.health
    drift = result.drift
    driftErrors = result.driftErrors
    changes = result.changes
    checkout = result.checkout
    updated = Date()
  }
  func editFleet() {
    let url = URL(fileURLWithPath: fleetPath)
    do {
      if !FileManager.default.fileExists(atPath: url.path) {
        try FileManager.default.createDirectory(
          at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let example = Bundle.main.url(forResource: "fleet-example", withExtension: "yaml")
        let contents =
          try example.map { try String(contentsOf: $0, encoding: .utf8) }
          ?? "version: 1\nmachines: {}\n"
        try contents.write(to: url, atomically: true, encoding: .utf8)
      }
      NSWorkspace.shared.open(url)
    } catch { self.error = String(describing: error) }
  }
  func deploy(_ name: String, action: Deployment.Action) {
    guard let fleet, jobs[name]?.running != true else { return }
    let job = Job()
    jobs[name] = job
    page = "log"
    Task {
      let logUpdates = Task {
        while job.running {
          job.text = job.output.text
          try? await Task.sleep(for: .milliseconds(200))
        }
      }
      defer {
        logUpdates.cancel()
        job.text = job.output.text
      }
      do {
        let steps = try Deployment.plan(action, fleet: fleet, name: name)
        let cancellation = job.cancel
        let buffer = job.output
        try await Task.detached {
          try Deployment.run(steps, cancel: cancellation, output: { buffer.append($0) })
        }.value
        job.result = "Succeeded"
      } catch { job.result = String(describing: error) }
      job.running = false
      await poll()
    }
  }
  func loadChanges() {
    guard let repo = fleet?.repoPath else { return }
    page = "commit"
    commitNote = "Loading changes…"
    Task {
      do {
        files = try await Task.detached { try Commit.changes(repo: repo) }.value
        included = Set(files.map(\.path))
        message = Commit.template(files)
        commitNote = ""
      } catch { commitNote = String(describing: error) }
    }
  }
  func draft() {
    guard let repo = fleet?.repoPath, let command = fleet?.commitMessage, !drafting else { return }
    drafting = true
    let files = files.filter { included.contains($0.path) }
    Task {
      defer { drafting = false }
      do {
        let result = try await Task.detached {
          let input = try Commit.draftInput(repo: repo, changes: files)
          return (try Commit.draft(command: command, repo: repo, input: input.text), input.withheld)
        }.value
        message = result.0
        commitNote =
          result.1.isEmpty ? "" : "Withheld secret files: " + result.1.joined(separator: ", ")
      } catch { commitNote = String(describing: error) }
    }
  }
  func commit() {
    guard let repo = fleet?.repoPath, let branch = fleet?.branch, !committing else { return }
    committing = true
    let paths = files.filter { included.contains($0.path) }.map(\.path)
    let message = message
    Task {
      defer { committing = false }
      do {
        let hash = try await Task.detached {
          try Commit.commitAndPush(repo: repo, branch: branch, paths: paths, message: message)
        }.value
        commitNote = "Pushed " + hash
        files = []
        included = []
        await poll()
      } catch { commitNote = String(describing: error) }
    }
  }
}
@MainActor @Observable final class Job {
  var text = ""
  var running = true
  var result: String?
  let cancel = Cancellation()
  let output = OutputBuffer()
  func append(_ chunk: String) { output.append(chunk) }
}
