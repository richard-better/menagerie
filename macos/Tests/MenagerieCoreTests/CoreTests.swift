import Testing
import Foundation
@testable import MenagerieCore

@Test func fleetValidation() throws {
    let example = try String(contentsOfFile: "../examples/fleet.yaml", encoding: .utf8)
    let fleet = try Fleet.parse(example)
    #expect(fleet.order == ["richard-m4-max", "richard-hp-server", "homeassistant", "iphone-15-pro-max"])
    #expect(fleet.machines["iphone-15-pro-max"]?.device == .phone)
    #expect(throws: (any Error).self) { try Fleet.parse("version: 2") }
    #expect(throws: (any Error).self) { try Fleet.parse("version: 1\nmachines: { a: { rol: typo } }") }
    #expect(throws: (any Error).self) { try Fleet.parse("version: 1\nmachines: { a: { sync: { preset: custom } } }") }
    #expect(try Fleet.parse(#"{"version":1,"machines":{"box":{}}}"#).machines["box"]?.device == .other)
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
    let result = try Runner.run(Command("/bin/sh", ["-c", "cat"], input: Data(repeating: 65, count: 100000)))
    #expect(result.status == 0)
    #expect(result.output.count == 100000)
    #expect(throws: (any Error).self) { try Runner.run(Command("/bin/sh", ["-c", "trap '' TERM; sleep 30 & wait"], timeout: 0.1)) }
    let cancel = Cancellation()
    cancel.cancel()
    #expect(throws: (any Error).self) { try Runner.run(Command("/bin/echo", ["never"]), cancel: cancel) }
}
struct Repository {
    let path = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).path
    init() throws {
        try FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true)
        _ = try Runner.git(path, ["init", "-q", "-b", "main"])
        for (key, value) in [("commit.gpgsign", "false"), ("user.name", "Test"), ("user.email", "test@example.com")] { _ = try Runner.git(path, ["config", key, value]) }
        _ = try Runner.git(path, ["commit", "-q", "--allow-empty", "-m", "initial"])
    }
    func write(_ name: String, _ text: String) throws { try text.write(toFile: path + "/" + name, atomically: true, encoding: .utf8) }
    func remove() { try? FileManager.default.removeItem(atPath: path) }
}
@Test func secretAndSymlinkSafety() throws {
    let repo = try Repository(); defer { repo.remove() }
    try repo.write(".env", "HIDDEN")
    try repo.write("ordinary.txt", "visible")
    try FileManager.default.createSymbolicLink(atPath: repo.path + "/link", withDestinationPath: repo.path + "/.env")
    try FileManager.default.createSymbolicLink(atPath: repo.path + "/parent", withDestinationPath: repo.path)
    #expect(Commit.smallText(repo: repo.path, path: "link") == nil)
    #expect(Commit.smallText(repo: repo.path, path: "parent/.env") == nil)
    #expect(Commit.smallText(repo: repo.path, path: "../.env") == nil)
    let input = try Commit.draftInput(repo: repo.path, changes: Commit.changes(repo: repo.path))
    #expect(!input.text.contains("HIDDEN"))
    #expect(input.text.contains("visible"))
    #expect(input.withheld == [".env"])
    for name in ["id_ed25519", "SECRET.txt", "foo.pem", "credentials.json", ".env.local"] { #expect(Commit.looksSecret(name)) }
    #expect(Commit.cleanMessage("```\n\"fix: example\"\n```") == "fix: example")
}
@Test func literalPathspecCommitPreservesOtherStagedFiles() throws {
    let repo = try Repository(); defer { repo.remove() }
    let remote = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).path
    defer { try? FileManager.default.removeItem(atPath: remote) }
    _ = try Runner.run(Command("/usr/bin/git", ["init", "--bare", "-q", remote])).checked()
    _ = try Runner.git(repo.path, ["remote", "add", "origin", remote])
    _ = try Runner.git(repo.path, ["push", "-q", "origin", "main"])
    try repo.write("a[1].txt", "one")
    try repo.write("a1.txt", "two")
    _ = try Runner.git(repo.path, ["add", "a1.txt"])
    _ = try Commit.commitAndPush(repo: repo.path, branch: "main", paths: ["a[1].txt"], message: "test: literal path")
    let committed = try Runner.git(repo.path, ["show", "--format=", "--name-only", "HEAD"])
    #expect(committed.trimmingCharacters(in: .whitespacesAndNewlines) == "a[1].txt")
    #expect(try Runner.git(repo.path, ["diff", "--cached", "--name-only"]).contains("a1.txt"))
}
@Test func syncPlansBuildCommitsAndSeparateRoot() throws {
    let fleet = try Fleet.parse("""
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
