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
  struct CommitContext: Equatable {
    var repo: String
    var branch: String
    var draftCommand: String?
  }
  private(set) var commitContext: CommitContext?
  private var commitGeneration = 0
  private(set) var loadingChanges = false
  private var mutatingCheckouts: Set<String> = []
  private func context(for fleet: Fleet?) -> CommitContext? {
    guard let fleet, let repo = fleet.repoPath else { return nil }
    return CommitContext(repo: repo, branch: fleet.branch, draftCommand: fleet.commitMessage)
  }
  private func invalidateCommitPage() {
    commitGeneration += 1
    commitContext = nil
    loadingChanges = false
    files = []
    included = []
    message = ""
    commitNote = ""
    drafting = false
  }
  private func claim(_ paths: [String]) -> Set<String>? {
    let keys = Set(paths.map(CheckoutMutation.key))
    guard mutatingCheckouts.isDisjoint(with: keys) else {
      error = "A job is already mutating this checkout"
      return nil
    }
    mutatingCheckouts.formUnion(keys)
    return keys
  }
  func closeWindow() {
    for terminal in terminals.values { terminal.transport.stop() }
    if !terminals.isEmpty { terminals.removeAll() }
    if page != "details" { page = "details" }
    if commitContext != nil && !committing { invalidateCommitPage() }
    if !committing && !drafting {
      if !files.isEmpty { files = [] }
      if !included.isEmpty { included = [] }
      if !message.isEmpty { message = "" }
      if !commitNote.isEmpty { commitNote = "" }
    }
  }
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
      if context(for: new) != context(for: fleet) { invalidateCommitPage() }
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
      invalidateCommitPage()
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
    using fetch: (@Sendable (Fleet, [String: LiveReport]) async -> PollResult)? = nil
  ) async {
    guard !polling, let fleet else { return }
    polling = true
    defer { polling = false }
    let generation = generation
    let result: PollResult
    if let fetch {
      result = await fetch(fleet, observations)
    } else {
      result = await Poller.poll(
        fleet, known: observations,
        onHealth: { [weak self] health in
          await self?.publishHealth(health, generation: generation)
        })
    }
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
  private func publishHealth(_ result: [String: String], generation: Int) {
    guard generation == self.generation else { return }
    health = result
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
  func deploy(
    _ name: String, action: Deployment.Action,
    run: @escaping @Sendable ([DeploymentStep], Cancellation, OutputBuffer) async throws -> Void = {
      steps, cancellation, buffer in
      try await Task.detached {
        try Deployment.run(steps, cancel: cancellation, output: { buffer.append($0) })
      }.value
    }
  ) {
    guard let fleet, jobs[name]?.running != true else { return }
    let steps: [DeploymentStep]
    do { steps = try Deployment.plan(action, fleet: fleet, name: name) } catch {
      self.error = String(describing: error)
      return
    }
    guard let paths = claim(steps.flatMap(\.checkouts)) else { return }
    selected = name
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
        try await run(steps, job.cancel, job.output)
        job.result = "Succeeded"
      } catch { job.result = String(describing: error) }
      job.running = false
      mutatingCheckouts.subtract(paths)
      await poll()
    }
  }
  func loadChanges() {
    loadChanges(using: { repo in try await Task.detached { try Commit.changes(repo: repo) }.value })
  }
  @discardableResult func loadChanges(
    using load: @escaping @Sendable (String) async throws -> [Change]
  ) -> Task<Void, Never>? {
    guard let captured = context(for: fleet) else { return nil }
    invalidateCommitPage()
    commitContext = captured
    let token = commitGeneration
    page = "commit"
    commitNote = "Loading changes…"
    loadingChanges = true
    return Task {
      defer { if token == commitGeneration { loadingChanges = false } }
      do {
        let loaded = try await load(captured.repo)
        guard token == commitGeneration && captured == context(for: fleet) else { return }
        files = loaded
        included = Set(loaded.map(\.path))
        message = Commit.template(loaded)
        commitNote = ""
      } catch {
        if token == commitGeneration { commitNote = String(describing: error) }
      }
    }
  }
  func draft() {
    draft(using: { command, repo, files in
      try await Task.detached {
        let input = try Commit.draftInput(repo: repo, changes: files)
        return (try Commit.draft(command: command, repo: repo, input: input.text), input.withheld)
      }.value
    })
  }
  @discardableResult func draft(
    using run: @escaping @Sendable (String, String, [Change]) async throws -> (String, [String])
  ) -> Task<Void, Never>? {
    guard let captured = commitContext, captured == context(for: fleet),
      let command = captured.draftCommand, !drafting, !loadingChanges
    else { return nil }
    drafting = true
    let token = commitGeneration
    let selection = included
    let original = files
    let before = message
    let chosen = files.filter { selection.contains($0.path) }
    return Task {
      defer { if token == commitGeneration { drafting = false } }
      do {
        let result = try await run(command, captured.repo, chosen)
        guard token == commitGeneration, captured == context(for: fleet), selection == included,
          original == files, before == message
        else { return }
        message = result.0
        commitNote =
          result.1.isEmpty ? "" : "Withheld secret files: " + result.1.joined(separator: ", ")
      } catch { if token == commitGeneration { commitNote = String(describing: error) } }
    }
  }
  func commit() {
    guard let captured = commitContext, captured == context(for: fleet), !committing,
      !loadingChanges
    else { return }
    let repo = captured.repo
    let branch = captured.branch
    guard let checkoutPaths = claim([repo]) else { return }
    committing = true
    let paths = files.filter { included.contains($0.path) }.map(\.path)
    let message = message
    let token = commitGeneration
    Task {
      do {
        let hash = try await Task.detached {
          try Commit.commitAndPush(repo: repo, branch: branch, paths: paths, message: message)
        }.value
        if token == commitGeneration {
          commitNote = "Pushed " + hash
          files = []
          included = []
        }
      } catch { if token == commitGeneration { commitNote = String(describing: error) } }
      committing = false
      mutatingCheckouts.subtract(checkoutPaths)
      await poll()
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
