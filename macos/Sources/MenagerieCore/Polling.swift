import Foundation

public struct LiveReport: Sendable {
  public var report: AgentStatus?
  public var received: Date?
  public var error: String?
  public init(report: AgentStatus? = nil, received: Date? = nil, error: String? = nil) {
    self.report = report
    self.received = received
    self.error = error
  }
  public func status(machine: Machine, drift: [String: Counts], now: Date = Date()) -> MachineStatus
  {
    guard machine.agent != nil else { return .noAgent }
    guard let report, let received, now.timeIntervalSince(received) < 35 else { return .offline }
    guard report.probes.revision.isFresh(now: report.agent.now),
      let revision = report.probes.revision.value, let counts = drift[revision.rev]
    else { return .online }
    return counts.ahead == 0 && counts.behind == 0
      ? .inSync(dirty: revision.dirty)
      : .drift(ahead: counts.ahead, behind: counts.behind, dirty: revision.dirty)
  }
}
public struct Counts: Sendable, Equatable {
  public var ahead: Int
  public var behind: Int
  public static func parse(_ text: String) throws -> Counts {
    let parts = text.split(whereSeparator: \.isWhitespace)
    guard parts.count == 2, let behind = Int(parts[0]), let ahead = Int(parts[1]), behind >= 0,
      ahead >= 0
    else { throw FleetError("Unexpected git counts") }
    return Counts(ahead: ahead, behind: behind)
  }
}
public struct Checkout: Sendable {
  public var counts: Counts
  public var dirty: Bool
}
public struct PollResult: Sendable {
  public init() {}
  public var observations: [String: LiveReport] = [:]
  public var health: [String: String] = [:]
  public var drift: [String: Counts] = [:]
  public var driftErrors: [String: String] = [:]
  public var changes: [String: String] = [:]
  public var checkout: Checkout?
}
public enum Poller {
  private enum Response: Sendable {
    case agent(String, LiveReport)
    case service(String, String)
  }
  private static func get(_ url: String, session: URLSession) async throws -> (
    Data, HTTPURLResponse
  ) {
    guard let url = URL(string: url) else { throw FleetError("Invalid URL") }
    var request = URLRequest(
      url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 3)
    request.httpMethod = "GET"
    // Incremental reads bound untrusted agent responses before decoding.
    let (bytes, response) = try await session.bytes(for: request)
    defer { bytes.task.cancel() }
    guard let response = response as? HTTPURLResponse, (200..<300).contains(response.statusCode)
    else { throw FleetError("HTTP request failed") }
    var data = Data()
    for try await byte in bytes {
      guard data.count < 2 * 1024 * 1024 else { throw FleetError("Agent response exceeds limit") }
      data.append(byte)
    }
    return (data, response)
  }
  private static func agent(_ url: String, session: URLSession, deadline: Duration) async throws
    -> Data
  {
    try await withThrowingTaskGroup(of: Data.self) { group in
      group.addTask { try await get(url, session: session).0 }
      group.addTask {
        try await Task.sleep(for: deadline)
        throw FleetError("Agent request exceeded total deadline")
      }
      defer { group.cancelAll() }
      return try await group.next()!
    }
  }
  private static func health(_ url: String, session: URLSession) async throws {
    guard let url = URL(string: url) else { throw FleetError("Invalid URL") }
    let (bytes, response) = try await session.bytes(
      for: URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 3))
    defer { bytes.task.cancel() }
    guard let response = response as? HTTPURLResponse, (200..<300).contains(response.statusCode)
    else { throw FleetError("HTTP request failed") }
  }

  public static func poll(
    _ fleet: Fleet, known: [String: LiveReport], session: URLSession = .shared,
    agentDeadline: Duration = .seconds(3),
    onHealth: @Sendable ([String: String]) async -> Void = { _ in },
    comparison: @Sendable (Fleet, [String: LiveReport], [String: LiveReport]) async -> PollResult =
      compare
  ) async -> PollResult {
    var result = PollResult()
    await withTaskGroup(of: Response.self) { group in
      for (name, machine) in fleet.machines {
        if let agent = machine.agent {
          group.addTask {
            do {
              let data = try await Self.agent(
                agent.trimmingCharacters(in: CharacterSet(charactersIn: "/")) + "/status",
                session: session, deadline: agentDeadline)
              return .agent(
                name,
                LiveReport(
                  report: try JSONDecoder().decode(AgentStatus.self, from: data), received: Date()))
            } catch {
              return .agent(
                name,
                LiveReport(error: (error as? FleetError)?.description ?? error.localizedDescription)
              )
            }
          }
        }
        for (index, service) in machine.services.enumerated() {
          if let url = service.health {
            group.addTask {
              let start = Date()
              do {
                try await Self.health(url, session: session)
                return .service(
                  "\(name)/\(index)", "up · \(Int(Date().timeIntervalSince(start) * 1000)) ms")
              } catch {
                return .service("\(name)/\(index)", "down · \(error.localizedDescription)")
              }
            }
          }
        }
      }
      for await response in group {
        switch response {
        case .agent(let name, let observation): result.observations[name] = observation
        case .service(let key, let health):
          result.health[key] = health
          await onHealth(result.health)
        }
      }
    }
    let git = await comparison(fleet, result.observations, known)
    result.drift = git.drift
    result.driftErrors = git.driftErrors
    result.changes = git.changes
    result.checkout = git.checkout
    return result
  }
  public static func compare(
    _ fleet: Fleet, _ observations: [String: LiveReport], _ known: [String: LiveReport]
  ) async -> PollResult {
    if let repo = fleet.repoPath {
      return await Task.detached {
        var result = PollResult()
        let upstream = "origin/" + fleet.branch
        let revisions = Set(
          (Array(observations.values) + Array(known.values)).compactMap {
            $0.report?.probes.revision.value?.rev
          })
        for rev in revisions where Revision.isValid(rev) {
          do {
            let output = try Runner.git(
              repo,
              ["rev-list", "--left-right", "--count", "--end-of-options", upstream + "..." + rev])
            result.drift[rev] = try Counts.parse(output)
            result.changes[rev] = try? Runner.git(
              repo, ["log", "--format=%h %s", "--end-of-options", rev + ".." + upstream])
          } catch {
            result.driftErrors[rev] = String(describing: error).trimmingCharacters(
              in: .whitespacesAndNewlines)
          }
        }
        if let output = try? Runner.git(
          repo, ["rev-list", "--left-right", "--count", "--end-of-options", upstream + "...HEAD"]),
          let counts = try? Counts.parse(output),
          let dirty = try? Runner.git(repo, ["status", "--porcelain", "--untracked-files=normal"])
        {
          result.checkout = Checkout(counts: counts, dirty: !dirty.isEmpty)
        }
        return result
      }.value
    }
    return PollResult()
  }
  public static func fetch(_ fleet: Fleet) async {
    guard let repo = fleet.repoPath else { return }
    await Task.detached {
      _ = try? Runner.run(
        Command(
          "/usr/bin/git",
          [
            "-C", repo, "fetch", "--quiet", "--no-write-fetch-head", "origin", "--end-of-options",
            fleet.branch,
          ], timeout: 30)
      )
    }.value
  }
}
