import Foundation

public struct Probe<Value: Decodable & Sendable>: Decodable, Sendable {
    public var value: Value?
    public var at: Int64?
    public var every_s: UInt32
    public var error: String?
    public func isFresh(now: Int64) -> Bool {
        guard error == nil, let at else { return false }
        return now - at <= Int64(max(every_s * 3, 35)) * 1000
    }
}
public struct Load: Decodable, Sendable { public var one, five, fifteen: Double; public var cpus: UInt32 }
public struct Usage: Decodable, Sendable {
    public var used_bytes, total_bytes: UInt64
    public var fraction: Double { total_bytes == 0 ? 0 : Double(used_bytes) / Double(total_bytes) }
}
public struct Battery: Decodable, Sendable { public var percent: Double; public var charging: Bool; public var health_percent: Double?; public var cycles: UInt32? }
public struct Temperature: Decodable, Sendable { public var celsius: Double }
public struct Revision: Decodable, Sendable {
    public var rev: String
    public var dirty: Bool
    public var generation: UInt64?
    public var source: String
    public static func isValid(_ rev: String) -> Bool { (7...64).contains(rev.utf8.count) && rev.utf8.allSatisfy { (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0) } }
}
public struct SystemInfo: Decodable, Sendable { public var os: String; public var os_version: String?; public var uptime_s: UInt64 }
public struct Probes: Decodable, Sendable {
    public var load: Probe<Load>
    public var memory, disk: Probe<Usage>
    public var battery: Probe<Battery>
    public var temperature: Probe<Temperature>
    public var revision: Probe<Revision>
    public var system: Probe<SystemInfo>
}
public struct AgentStatus: Decodable, Sendable {
    public struct Agent: Decodable, Sendable { public var version: String; public var now: Int64 }
    public struct Sample: Decodable, Sendable { public var at: Int64; public var value: Double }
    public var version: UInt32
    public var host: String
    public var agent: Agent
    public var probes: Probes
    public var load_history: [Sample]
    enum CodingKeys: CodingKey { case version, host, agent, probes, load_history }
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        version = try c.decode(UInt32.self, forKey: .version)
        host = try c.decode(String.self, forKey: .host)
        agent = try c.decode(Agent.self, forKey: .agent)
        probes = try c.decode(Probes.self, forKey: .probes)
        load_history = try c.decodeIfPresent([Sample].self, forKey: .load_history) ?? []
        guard version == 1 else { throw FleetError("Unsupported agent protocol \(version)") }
        if let revision = probes.revision.value, !Revision.isValid(revision.rev) { throw FleetError("Agent revision is not a commit hash") }
    }
}
public enum MachineStatus: Equatable, Sendable {
    case inSync(dirty: Bool), drift(ahead: Int, behind: Int, dirty: Bool), online, noAgent, offline
    public var needsSync: Bool {
        switch self { case .drift, .inSync(dirty: true): true; default: false }
    }
    public var online: Bool {
        switch self { case .inSync, .drift, .online: true; default: false }
    }
    public var label: String {
        switch self {
        case .inSync(let dirty): dirty ? "modified" : "in sync"
        case .drift(let a, let b, _): [a > 0 ? "↑\(a)" : nil, b > 0 ? "↓\(b)" : nil].compactMap { $0 }.joined(separator: " ")
        case .online: "online"
        case .noAgent: "no agent"
        case .offline: "offline"
        }
    }
}
