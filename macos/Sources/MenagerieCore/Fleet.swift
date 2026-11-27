import Foundation
import Yams

public struct FleetError: Error, CustomStringConvertible, Sendable {
    public let description: String
    public init(_ description: String) { self.description = description }
}

public struct Service: Decodable, Equatable, Sendable {
    public var name: String
    public var icon: String?
    public var url: String
    public var health: String?
}
public struct SyncConfiguration: Decodable, Equatable, Sendable {
    public enum Preset: String, Decodable, Sendable { case nixDarwin = "nix-darwin", nixos, homeManager = "home-manager", custom }
    public var preset: Preset
    public var repo: String?
    public var command: String?
    public var flake: String?
}
public struct Machine: Decodable, Equatable, Sendable {
    public enum Device: String, Decodable, Sendable { case laptop, desktop, server, box, phone, other }
    public var role: String?
    public var device: Device = .other
    public var agent: String?
    public var ssh: String?
    public var sync: SyncConfiguration?
    public var services: [Service] = []
    enum CodingKeys: String, CodingKey { case role, device, agent, ssh, sync, services }
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        role = try c.decodeIfPresent(String.self, forKey: .role)
        device = try c.decodeIfPresent(Device.self, forKey: .device) ?? .other
        agent = try c.decodeIfPresent(String.self, forKey: .agent)
        ssh = try c.decodeIfPresent(String.self, forKey: .ssh)
        sync = try c.decodeIfPresent(SyncConfiguration.self, forKey: .sync)
        services = try c.decodeIfPresent([Service].self, forKey: .services) ?? []
    }
}
public struct Fleet: Sendable, Equatable {
    public var repo: String?
    public var branch: String
    public var commitMessage: String?
    public var machines: [String: Machine]
    public var order: [String]
    public var repoPath: String? { repo.map(expandHome) }

    public static func parse(_ text: String) throws -> Fleet {
        guard let root = try compose(yaml: text), let mapping = root.mapping else { throw FleetError("Fleet must be a mapping") }
        try check(root, keys: ["version", "repo", "branch", "commit_message", "machines"])
        struct Document: Decodable {
            var version: Int
            var repo: String?
            var branch: String?
            var commit_message: String?
            var machines: [String: Machine]?
        }
        let doc = try YAMLDecoder().decode(Document.self, from: text)
        guard doc.version == 1 else { throw FleetError("Unsupported fleet version \(doc.version)") }
        var order: [String] = []
        if let nodes = mapping["machines"]?.mapping {
            for (key, value) in nodes {
                guard let name = key.string else { throw FleetError("Machine name must be text") }
                guard !order.contains(name) else { throw FleetError("Duplicate machine \(name)") }
                order.append(name)
                try check(value, keys: ["role", "device", "agent", "ssh", "sync", "services"])
                if let sync = value.mapping?["sync"], sync.mapping != nil {
                    try check(sync, keys: ["preset", "repo", "command", "flake"])
                }
                for service in value.mapping?["services"]?.sequence ?? [] {
                    try check(service, keys: ["name", "icon", "url", "health"])
                }
            }
        }
        for (name, machine) in doc.machines ?? [:] {
            if let sync = machine.sync {
                guard (sync.preset == .custom) == (sync.command != nil) else { throw FleetError("\(name): only custom sync requires a command") }
            }
            guard machine.services.allSatisfy({ !$0.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }) else { throw FleetError("\(name): empty service name") }
        }
        return Fleet(repo: doc.repo, branch: doc.branch ?? "main", commitMessage: doc.commit_message, machines: doc.machines ?? [:], order: order)
    }
    private static func check(_ node: Node, keys: Set<String>) throws {
        guard let mapping = node.mapping else { throw FleetError("Expected mapping") }
        var seen: Set<String> = []
        for (key, _) in mapping {
            guard let key = key.string, keys.contains(key) else { throw FleetError("Unknown field \(key)") }
            guard seen.insert(key).inserted else { throw FleetError("Duplicate field \(key)") }
        }
    }
    public static var path: String {
        let env = ProcessInfo.processInfo.environment
        if let path = env["MENAGERIE_FLEET"], !path.isEmpty { return path }
        let config = env["XDG_CONFIG_HOME"].flatMap { $0.isEmpty ? nil : $0 } ?? NSHomeDirectory() + "/.config"
        return config + "/menagerie/fleet.yaml"
    }
}
public func expandHome(_ path: String) -> String {
    if path == "~" { return NSHomeDirectory() }
    if path.hasPrefix("~/") { return NSHomeDirectory() + String(path.dropFirst()) }
    return path
}
