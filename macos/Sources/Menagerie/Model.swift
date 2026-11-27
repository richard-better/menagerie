import AppKit
import Observation
import MenagerieCore

@MainActor @Observable final class Model {
    var fleet: Fleet?
    var error: String?
    var selected = ""
    var observations: [String: LiveReport] = [:]
    var health: [String: String] = [:]
    var drift: [String: Counts] = [:]
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
    private var generation = 0
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
    func start() {
        guard !started else { return }; started = true
        Task {
            await reload()
            Task {
                while !Task.isCancelled {
                    if let fleet { await Poller.fetch(fleet); await poll() }
                    try? await Task.sleep(for: .seconds(300))
                }
            }
            while !Task.isCancelled { await poll(); try? await Task.sleep(for: .seconds(10)); await reload() }
        }
    }
    func reload() async {
        let result = await Task.detached { () -> Result<Fleet, any Error> in
            Result { try Fleet.parse(String(contentsOfFile: Fleet.path, encoding: .utf8)) }
        }.value
        switch result {
        case .success(let new):
            error = nil
            if new.machines != fleet?.machines || new.repo != fleet?.repo || new.branch != fleet?.branch {
                generation += 1; observations = [:]; health = [:]; drift = [:]; checkout = nil; changes = [:]
            }
            fleet = new
            if new.machines[selected] == nil { selected = new.order.first ?? "" }
        case .failure(let failure): error = String(describing: failure)
        }
    }
    func poll() async {
        guard !polling, let fleet else { return }; polling = true
        defer { polling = false }
        let generation = generation
        let result = await Poller.poll(fleet, known: observations)
        guard generation == self.generation else { return }
        for (name, var observation) in result.observations {
            if observation.report == nil {
                observation.report = observations[name]?.report
                observation.received = observations[name]?.received
            }
            observations[name] = observation
        }
        health = result.health; drift = result.drift; changes = result.changes; checkout = result.checkout; updated = Date()
    }
    func editFleet() {
        let url = URL(fileURLWithPath: Fleet.path)
        do {
            if !FileManager.default.fileExists(atPath: url.path) {
                try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
                try "version: 1\nmachines: {}\n".write(to: url, atomically: true, encoding: .utf8)
            }
            NSWorkspace.shared.open(url)
        } catch { self.error = String(describing: error) }
    }
    func deploy(_ name: String, action: Deployment.Action) {
        guard let fleet, jobs[name]?.running != true else { return }
        let job = Job(); jobs[name] = job; page = "log"
        Task {
            do {
                let steps = try Deployment.plan(action, fleet: fleet, name: name)
                for step in steps {
                    job.append("▸ " + step.label + "\n")
                    var command = step.command
                    if step.administrator {
                        command = Command("/usr/bin/sudo", ["-n", command.program] + command.arguments, timeout: command.timeout)
                    }
                    let commandToRun = command
                    let cancellation = job.cancel
                    let result = try await Task.detached {
                        try Runner.run(commandToRun, cancel: cancellation) { output in Task { @MainActor in job.append(output) } }
                    }.value
                    _ = try result.checked()
                }
                job.result = "Succeeded"
            } catch { job.result = String(describing: error) }
            job.running = false
            await poll()
        }
    }
    func loadChanges() {
        guard let repo = fleet?.repoPath else { return }
        page = "commit"; commitNote = "Loading changes…"
        Task {
            do {
                files = try await Task.detached { try Commit.changes(repo: repo) }.value
                included = Set(files.map(\.path)); message = Commit.template(files); commitNote = ""
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
                message = result.0; commitNote = result.1.isEmpty ? "" : "Withheld secret files: " + result.1.joined(separator: ", ")
            } catch { commitNote = String(describing: error) }
        }
    }
    func commit() {
        guard let repo = fleet?.repoPath, let branch = fleet?.branch, !committing else { return }
        committing = true
        let paths = files.filter { included.contains($0.path) }.map(\.path), message = message
        Task {
            defer { committing = false }
            do {
                let hash = try await Task.detached { try Commit.commitAndPush(repo: repo, branch: branch, paths: paths, message: message) }.value
                commitNote = "Pushed " + hash; files = []; included = []; await poll()
            } catch { commitNote = String(describing: error) }
        }
    }
}
@MainActor @Observable final class Job {
    var text = ""
    var running = true
    var result: String?
    let cancel = Cancellation()
    func append(_ output: String) {
        text += output
        if text.utf8.count > 256 * 1024 { text = String(text.suffix(128 * 1024)) }
    }
}
