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
    guard let response = response as? HTTPURLResponse, (200..<300).contains(response.statusCode)
    else { throw FleetError("HTTP request failed") }
    var data = Data()
    for try await byte in bytes {
      guard data.count < 2 * 1024 * 1024 else { throw FleetError("Agent response exceeds limit") }
      data.append(byte)
    }
    return (data, response)
  }
  public static func poll(
    _ fleet: Fleet, known: [String: LiveReport], session: URLSession = .shared
  ) async -> PollResult {
    var result = PollResult()
    await withTaskGroup(of: Response.self) { group in
      for (name, machine) in fleet.machines {
        if let agent = machine.agent {
          group.addTask {
            do {
              let (data, _) = try await get(
                agent.trimmingCharacters(in: CharacterSet(charactersIn: "/")) + "/status",
                session: session)
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
                _ = try await get(url, session: session)
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
        case .service(let key, let health): result.health[key] = health
        }
      }
    }
    if let repo = fleet.repoPath {
      let observations = result.observations
      let git = await Task.detached {
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
      result.drift = git.drift
      result.driftErrors = git.driftErrors
      result.changes = git.changes
      result.checkout = git.checkout
    }
    return result
  }
  public static func fetch(_ fleet: Fleet) async {
    guard let repo = fleet.repoPath else { return }
    await Task.detached {
      _ = try? Runner.run(
        Command(
          "/usr/bin/git",
          ["-C", repo, "fetch", "--quiet", "origin", "--end-of-options", fleet.branch], timeout: 30)
      )
    }.value
  }
}
