import Foundation

public struct DeploymentStep: Sendable, Equatable {
  public var label: String
  public var command: Command
  public var administrator: Bool = false
}
public enum Deployment {
  public enum Action: Sendable { case sync, rollback }
  public static func isLocal(
    _ name: String, _ machine: Machine,
    hostname: String = ProcessInfo.processInfo.hostName.components(separatedBy: ".")[0]
  ) -> Bool {
    let host = machine.agent.flatMap(URL.init(string:))?.host?.lowercased()
    return name.lowercased() == hostname.lowercased() || host == "localhost" || host == "[::1]"
      || host == "::1" || host?.hasPrefix("127.") == true
  }
  public static func pushScript(repo: String, branch: String) -> String {
    let ref = shellQuote("refs/heads/" + branch)
    return """
      cd \(shellQuote(repo)) || exit 1
      [ "$(git symbolic-ref --quiet HEAD)" = \(ref) ] || { echo 'Checkout is not on the configured branch'; exit 1; }
      git fetch -q origin \(ref) || exit 1
      ahead=$(git rev-list --count FETCH_HEAD..HEAD) || exit 1
      [ "$ahead" -gt 0 ] || exit 0
      if ! git merge-base --is-ancestor FETCH_HEAD HEAD; then
        git rebase -q --autostash FETCH_HEAD || { git rebase --abort; exit 1; }
      fi
      git push origin \(shellQuote("HEAD:refs/heads/" + branch))
      """
  }
  public static func plan(_ action: Action, fleet: Fleet, name: String) throws -> [DeploymentStep] {
    guard let machine = fleet.machines[name], let sync = machine.sync else {
      throw FleetError("No sync settings for \(name)")
    }
    let local = isLocal(name, machine)
    guard local || machine.ssh != nil else { throw FleetError("\(name) has no SSH destination") }
    var steps: [DeploymentStep] = []
    if action == .sync, let repo = fleet.repoPath {
      steps.append(
        DeploymentStep(
          label: "Push checkout commits (uncommitted changes stay behind)",
          command: Command(
            "/bin/sh", ["-c", pushScript(repo: repo, branch: fleet.branch)], timeout: 300)))
    }
    let repo = sync.repo ?? "."
    let attr = sync.flake ?? (sync.preset == .homeManager ? machine.ssh ?? name : name)
    let ref = shellQuote("refs/heads/" + fleet.branch)
    let pull = """
      [ "$(git symbolic-ref --quiet HEAD)" = \(ref) ] || { echo 'Checkout is not on the configured branch'; exit 1; }
      git fetch origin \(ref) || exit 1
      git merge --ff-only FETCH_HEAD || exit 1
      """
    if local && (sync.preset == .nixDarwin || sync.preset == .nixos) {
      let dir = expandHome(repo)
      let program =
        "/run/current-system/sw/bin/"
        + (sync.preset == .nixDarwin ? "darwin-rebuild" : "nixos-rebuild")
      let arguments: [String]
      if action == .sync {
        steps.append(
          DeploymentStep(
            label: "Pull this Mac's checkout",
            command: Command(
              "/bin/sh", ["-c", "cd \(shellQuote(dir)) || exit 1\n" + pull], timeout: 300)))
        let escaped = dir.replacingOccurrences(of: "%", with: "%25").replacingOccurrences(
          of: " ", with: "%20")
        arguments = [
          "switch", "--flake", "git+file://\(escaped)?ref=refs/heads/\(fleet.branch)#\(attr)",
        ]
      } else {
        arguments = sync.preset == .nixDarwin ? ["--rollback"] : ["switch", "--rollback"]
      }
      steps.append(
        DeploymentStep(
          label: "Switch this Mac", command: Command(program, arguments, timeout: 3600),
          administrator: true))
      return steps
    }
    let target = "\"$flake\"" + shellQuote("#" + attr)
    let rebuild: String
    switch (action, sync.preset) {
    case (.sync, .nixDarwin): rebuild = "sudo -n darwin-rebuild switch --flake " + target
    case (.sync, .nixos): rebuild = "sudo -n nixos-rebuild switch --flake " + target
    case (.sync, .homeManager): rebuild = "home-manager switch --flake " + target
    case (.sync, .custom): rebuild = "(\n" + (sync.command ?? "") + "\n)"
    case (.rollback, .nixDarwin): rebuild = "sudo -n darwin-rebuild --rollback"
    case (.rollback, .nixos): rebuild = "sudo -n nixos-rebuild switch --rollback"
    case (.rollback, .homeManager): rebuild = "home-manager switch --rollback"
    case (.rollback, .custom): throw FleetError("Custom commands have no rollback")
    }
    let flake =
      #"flake="git+file://$(pwd -P | sed 's/%/%25/g; s/ /%20/g')?rev=$(git rev-parse HEAD)" || exit 1"#
    let script =
      "cd \(shellPath(repo)) || exit 1\n" + (action == .sync ? pull + "\n" + flake + "\n" : "")
      + rebuild
    let command =
      local
      ? Command("/bin/sh", ["-lc", script], timeout: 3600)
      : Command(
        "/usr/bin/ssh",
        [
          "-tt", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "--", machine.ssh!,
          "sh -lc " + shellQuote(script),
        ], timeout: 3600)
    steps.append(
      DeploymentStep(
        label: action == .sync ? "Pull and switch \(name)" : "Roll back \(name)", command: command))
    return steps
  }
}
