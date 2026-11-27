import Foundation
import Testing

@testable import MenagerieCore

@Test func fleetValidation() throws {
  let example = try String(contentsOfFile: "../examples/fleet.yaml", encoding: .utf8)
  let fleet = try Fleet.parse(example)
  #expect(
    fleet.order == ["richard-m4-max", "richard-hp-server", "homeassistant", "iphone-15-pro-max"])
  #expect(fleet.machines["iphone-15-pro-max"]?.device == .phone)
  #expect(throws: (any Error).self) { try Fleet.parse("version: 2") }
  #expect(throws: (any Error).self) {
    try Fleet.parse("version: 1\nmachines: { a: { rol: typo } }")
  }
  #expect(throws: (any Error).self) {
    try Fleet.parse("version: 1\nmachines: { a: { sync: { preset: custom } } }")
  }
  #expect(
    try Fleet.parse(#"{"version":1,"machines":{"box":{}}}"#).machines["box"]?.device == .other)
  #expect(expandHome("~user/file") == "~user/file")
}
@Test func revisionsAndCounts() throws {
  #expect(Revision.isValid("abc1234"))
  #expect(!Revision.isValid("--output=bad"))
  #expect(!Revision.isValid("ééééé"))
  #expect(try Counts.parse("4\t2\n") == Counts(ahead: 2, behind: 4))
  #expect(throws: (any Error).self) { try Counts.parse("-1 2") }
  #expect(MachineStatus.inSync(dirty: true).needsSync)
  #expect(!MachineStatus.online.needsSync)
}
@Test func boundedCommandsAndCancellation() async throws {
  let result = try Runner.run(
    Command("/bin/sh", ["-c", "cat"], input: Data(repeating: 65, count: 100000)))
  #expect(result.status == 0)
  #expect(result.output.count == 100000)
  #expect(throws: (any Error).self) {
    try Runner.run(Command("/bin/sh", ["-c", "trap '' TERM; sleep 30 & wait"], timeout: 0.1))
  }
  let cancel = Cancellation()
  cancel.cancel()
  #expect(throws: (any Error).self) {
    try Runner.run(Command("/bin/echo", ["never"]), cancel: cancel)
  }
}
struct Repository {
  let path = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).path
  init() throws {
    try FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true)
    _ = try Runner.git(path, ["init", "-q", "-b", "main"])
    for (key, value) in [
      ("commit.gpgsign", "false"), ("user.name", "Test"), ("user.email", "test@example.com"),
    ] { _ = try Runner.git(path, ["config", key, value]) }
    _ = try Runner.git(path, ["commit", "-q", "--allow-empty", "-m", "initial"])
  }
  func write(_ name: String, _ text: String) throws {
    try text.write(toFile: path + "/" + name, atomically: true, encoding: .utf8)
  }
  func remove() { try? FileManager.default.removeItem(atPath: path) }
}
@Test func secretAndSymlinkSafety() throws {
  let repo = try Repository()
  defer { repo.remove() }
  try repo.write(".env", "HIDDEN")
  try repo.write("ordinary.txt", "visible")
  try FileManager.default.createSymbolicLink(
    atPath: repo.path + "/link", withDestinationPath: repo.path + "/.env")
  try FileManager.default.createSymbolicLink(
    atPath: repo.path + "/parent", withDestinationPath: repo.path)
  #expect(Commit.smallText(repo: repo.path, path: "link") == nil)
  #expect(Commit.smallText(repo: repo.path, path: "parent/.env") == nil)
  #expect(Commit.smallText(repo: repo.path, path: "../.env") == nil)
  let input = try Commit.draftInput(repo: repo.path, changes: Commit.changes(repo: repo.path))
  #expect(!input.text.contains("HIDDEN"))
  #expect(input.text.contains("visible"))
  #expect(input.withheld == [".env"])
  for name in ["id_ed25519", "SECRET.txt", "foo.pem", "credentials.json", ".env.local"] {
    #expect(Commit.looksSecret(name))
  }
  #expect(Commit.cleanMessage("```\n\"fix: example\"\n```") == "fix: example")
}
@Test func literalPathspecCommitPreservesOtherStagedFiles() throws {
  let repo = try Repository()
  defer { repo.remove() }
  let remote = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).path
  defer { try? FileManager.default.removeItem(atPath: remote) }
  _ = try Runner.run(Command("/usr/bin/git", ["init", "--bare", "-q", remote])).checked()
  _ = try Runner.git(repo.path, ["remote", "add", "origin", remote])
  _ = try Runner.git(repo.path, ["push", "-q", "origin", "main"])
  try repo.write("a[1].txt", "one")
  try repo.write("a1.txt", "two")
  _ = try Runner.git(repo.path, ["add", "a1.txt"])
  _ = try Commit.commitAndPush(
    repo: repo.path, branch: "main", paths: ["a[1].txt"], message: "test: literal path")
  let committed = try Runner.git(repo.path, ["show", "--format=", "--name-only", "HEAD"])
  #expect(committed.trimmingCharacters(in: .whitespacesAndNewlines) == "a[1].txt")
  #expect(try Runner.git(repo.path, ["diff", "--cached", "--name-only"]).contains("a1.txt"))
}
@Test func syncPlansBuildCommitsAndSeparateRoot() throws {
  let fleet = try Fleet.parse(
    """
    version: 1
    repo: /tmp/checkout
    machines:
      local:
        agent: http://localhost:9184
        sync: { preset: nix-darwin, repo: /tmp/checkout }
      remote:
        ssh: user@host
        sync: { preset: home-manager, repo: ~/config }
    """)
  let local = try Deployment.plan(.sync, fleet: fleet, name: "local")
  #expect(local.count == 3)
  #expect(local.last?.administrator == true)
  #expect(local.last?.command.arguments.last?.contains("?ref=refs/heads/main#local") == true)
  #expect(local[1].command.arguments.last?.contains("merge --ff-only") == true)
  let remote = try Deployment.plan(.sync, fleet: fleet, name: "remote")
  #expect(remote.last?.command.program == "/usr/bin/ssh")
  #expect(remote.last?.command.arguments.last?.contains("?rev=") == true)
  #expect(!remote.last!.command.arguments.joined().contains("git pull"))
}

@Test func apcIsRefusedAcrossChunksWithoutBuffering() {
  var filter = TerminalFilter()
  #expect(String(decoding: filter.filter(Array("hello\u{1b}".utf8)[...]), as: UTF8.self) == "hello")
  #expect(filter.filter(Array("_Gpayload".utf8)[...]).isEmpty)
  #expect(filter.filter(Array(repeating: 65, count: 1_000_000)[...]).isEmpty)
  #expect(filter.filter([0x1b][...]).isEmpty)
  #expect(String(decoding: filter.filter(Array("\\world".utf8)[...]), as: UTF8.self) == "world")
}
@Test func dirtyTreeFastForwardIgnoresPullRebase() throws {
  let repo = try Repository()
  defer { repo.remove() }
  let remote = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).path
  defer { try? FileManager.default.removeItem(atPath: remote) }
  _ = try Runner.run(Command("/usr/bin/git", ["clone", "--bare", "-q", repo.path, remote]))
    .checked()
  _ = try Runner.git(repo.path, ["remote", "add", "origin", remote])
  _ = try Runner.git(repo.path, ["config", "pull.rebase", "true"])
  try repo.write("dirty.txt", "do not deploy")
  let fleet = try Fleet.parse(
    """
    version: 1
    machines:
      local:
        agent: http://localhost:9184
        sync: {preset: custom, repo: '\(repo.path)', command: 'git rev-parse HEAD'}
    """)
  let steps = try Deployment.plan(.sync, fleet: fleet, name: "local")
  #expect(try Runner.run(steps[0].command).status == 0)
  #expect(try String(contentsOfFile: repo.path + "/dirty.txt", encoding: .utf8) == "do not deploy")
}
@Test func pushRebasesWithoutMergeCommits() throws {
  let repo = try Repository()
  defer { repo.remove() }
  let origin = try Repository()
  defer { origin.remove() }
  let remote = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).path
  defer { try? FileManager.default.removeItem(atPath: remote) }
  _ = try Runner.run(Command("/usr/bin/git", ["clone", "--bare", "-q", repo.path, remote]))
    .checked()
  _ = try Runner.git(repo.path, ["remote", "add", "origin", remote])
  _ = try Runner.git(origin.path, ["remote", "add", "origin", remote])
  _ = try Runner.git(origin.path, ["fetch", "-q", "origin", "main"])
  _ = try Runner.git(origin.path, ["reset", "--hard", "FETCH_HEAD"])
  try origin.write("remote", "remote")
  _ = try Runner.git(origin.path, ["add", "."])
  _ = try Runner.git(origin.path, ["commit", "-q", "-m", "remote"])
  _ = try Runner.git(origin.path, ["push", "-q", "origin", "main"])
  try repo.write("local", "local")
  _ = try Commit.commitAndPush(repo: repo.path, branch: "main", paths: ["local"], message: "local")
  #expect(
    try Runner.git(repo.path, ["rev-list", "--count", "HEAD"]).trimmingCharacters(
      in: .whitespacesAndNewlines) == "3")
  #expect(try Runner.git(repo.path, ["rev-list", "--merges", "HEAD"]).isEmpty)
}
@Test func apcFilterPreservesUnicodeContinuationBytes() {
  var filter = TerminalFilter()
  let text = "你好 🐾 日本語"
  var output: [UInt8] = []
  for byte in text.utf8 { output += filter.filter([byte][...]) }
  #expect(String(decoding: output, as: UTF8.self) == text)
}
private func report(version: Int = 1, revision: String = "abcdef1234", probeError: String? = nil)
  throws -> AgentStatus
{
  let empty: [String: Any] = ["value": NSNull(), "at": NSNull(), "every_s": 10, "error": NSNull()]
  var probes = Dictionary(
    uniqueKeysWithValues: [
      "load", "memory", "disk", "battery", "temperature", "system", "revision",
    ].map { ($0, empty) })
  probes["revision"] = [
    "value": ["rev": revision, "dirty": false, "source": "nix-darwin"], "at": 1000, "every_s": 10,
    "error": probeError as Any? ?? NSNull(),
  ]
  let json: [String: Any] = [
    "version": version, "host": "local", "agent": ["version": "1", "now": 1000], "probes": probes,
  ]
  return try JSONDecoder().decode(
    AgentStatus.self, from: JSONSerialization.data(withJSONObject: json))
}
@Test func protocolAndStaleness() throws {
  #expect(try report().loadHistory.isEmpty)
  #expect(throws: (any Error).self) { try report(version: 99) }
  #expect(throws: (any Error).self) { try report(revision: "--upload-pack=bad") }
  let machine = try Fleet.parse(
    "version: 1\nmachines: { local: { agent: 'http://localhost:9184' } }"
  ).machines["local"]!
  let now = Date()
  let observation = LiveReport(report: try report(), received: now)
  #expect(observation.status(machine: machine, drift: [:], now: now) == .online)
  #expect(
    observation.status(
      machine: machine, drift: ["abcdef1234": Counts(ahead: 0, behind: 0)], now: now)
      == .inSync(dirty: false))
  #expect(
    observation.status(
      machine: machine, drift: ["abcdef1234": Counts(ahead: 1, behind: 2)], now: now)
      == .drift(ahead: 1, behind: 2, dirty: false))
  #expect(
    observation.status(machine: machine, drift: [:], now: now.addingTimeInterval(36)) == .offline)
  let failed = LiveReport(report: try report(probeError: "unreadable"), received: now)
  #expect(
    failed.status(machine: machine, drift: ["abcdef1234": Counts(ahead: 0, behind: 0)], now: now)
      == .online)
}
@Test func draftingUsesOnlyStandardOutput() throws {
  let repo = try Repository()
  defer { repo.remove() }
  #expect(
    try Commit.draft(
      command: "echo warning >&2; printf '\"fix: correct message\"\\nignored'", repo: repo.path,
      input: "diff") == "fix: correct message")
}
