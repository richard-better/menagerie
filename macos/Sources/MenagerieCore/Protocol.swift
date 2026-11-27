import Foundation

public struct Probe<Value: Decodable & Sendable>: Decodable, Sendable {
  enum CodingKeys: String, CodingKey {
    case value, at, error
    case everyS = "every_s"
  }
  public var value: Value?
  public var at: Int64?
  public var everyS: UInt32
  public var error: String?
  public func isFresh(now: Int64) -> Bool {
    guard error == nil, let at else { return false }
    let age = now.subtractingReportingOverflow(at)
    return !age.overflow && age.partialValue <= max(Int64(everyS) * 3, 35) * 1000
  }
}
public struct Load: Decodable, Sendable {
  public var one, five, fifteen: Double
  public var cpus: UInt32
}
public struct Usage: Decodable, Sendable {
  enum CodingKeys: String, CodingKey {
    case usedBytes = "used_bytes"
    case totalBytes = "total_bytes"
  }
  public var usedBytes, totalBytes: UInt64
  public var fraction: Double {
    totalBytes == 0 ? 0 : min(1, Double(usedBytes) / Double(totalBytes))
  }
}
public struct Battery: Decodable, Sendable {
  enum CodingKeys: String, CodingKey {
    case percent, charging, cycles
    case healthPercent = "health_percent"
  }
  public var percent: Double
  public var charging: Bool
  public var healthPercent: Double?
  public var cycles: UInt32?
}
public struct Temperature: Decodable, Sendable { public var celsius: Double }
public struct Revision: Decodable, Sendable {
  public var rev: String
  public var dirty: Bool
  public var generation: UInt64?
  public var source: String
  public static func isValid(_ rev: String) -> Bool {
    (7...64).contains(rev.utf8.count)
      && rev.utf8.allSatisfy {
        (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0)
      }
  }
}
public struct SystemInfo: Decodable, Sendable {
  enum CodingKeys: String, CodingKey {
    case os
    case osVersion = "os_version"
    case uptimeS = "uptime_s"
  }
  public var os: String
  public var osVersion: String?
  public var uptimeS: UInt64
}
public struct Probes: Decodable, Sendable {
  public var load: Probe<Load>
  public var memory, disk: Probe<Usage>
  public var battery: Probe<Battery>
  public var temperature: Probe<Temperature>
  public var revision: Probe<Revision>
  public var system: Probe<SystemInfo>
}
public struct AgentStatus: Decodable, Sendable {
  public struct Agent: Decodable, Sendable {
    public var version: String
    public var now: Int64
  }
  public struct Sample: Decodable, Sendable {
    public var at: Int64
    public var value: Double
  }
  public var version: UInt32
  public var host: String
  public var agent: Agent
  public var probes: Probes
  public var loadHistory: [Sample]
  enum CodingKeys: String, CodingKey {
    case version, host, agent, probes
    case loadHistory = "load_history"
  }
  public init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    version = try c.decode(UInt32.self, forKey: .version)
    host = try c.decode(String.self, forKey: .host)
    agent = try c.decode(Agent.self, forKey: .agent)
    probes = try c.decode(Probes.self, forKey: .probes)
    loadHistory = try c.decodeIfPresent([Sample].self, forKey: .loadHistory) ?? []
    guard version == 1 else { throw FleetError("Unsupported agent protocol \(version)") }
    let loads =
      [probes.load.value?.one, probes.load.value?.five, probes.load.value?.fifteen].compactMap {
        $0
      } + loadHistory.map(\.value)
    guard loads.allSatisfy({ $0.isFinite && (0...1e9).contains($0) }),
      loadHistory.count <= 1440,
      probes.battery.value.map({ (0...100).contains($0.percent) }) ?? true,
      probes.temperature.value.map({ (-273.15...1000).contains($0.celsius) }) ?? true
    else { throw FleetError("Agent reported measurements outside their valid range") }

    if let revision = probes.revision.value, !Revision.isValid(revision.rev) {
      throw FleetError("Agent revision is not a commit hash")
    }
  }
}
public enum MachineStatus: Equatable, Sendable {
  case inSync(dirty: Bool)
  case drift(ahead: Int, behind: Int, dirty: Bool)
  case online, noAgent, offline
  public var needsSync: Bool {
    switch self {
    case .drift, .inSync(dirty: true): true
    default: false
    }
  }
  public var online: Bool {
    switch self {
    case .inSync, .drift, .online: true
    default: false
    }
  }
  public var label: String {
    switch self {
    case .inSync(let dirty): dirty ? "modified" : "in sync"
    case .drift(let a, let b, _):
      [a > 0 ? "↑\(a)" : nil, b > 0 ? "↓\(b)" : nil].compactMap { $0 }.joined(separator: " ")
    case .online: "online"
    case .noAgent: "no agent"
    case .offline: "offline"
    }
  }
}
