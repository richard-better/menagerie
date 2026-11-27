import Darwin
import Foundation

public struct DeploymentStep: Sendable, Equatable {
  public var label: String
  public var command: Command
  public var administrator: Bool = false
  public var checkouts: [String] = []
  public var pushRepo: String?
  public var pushBranch: String?
}
public enum Deployment {
  public enum Action: Sendable { case sync, rollback }
  public static func isLocal(
    _ name: String, _ machine: Machine,
    hostname: String = ProcessInfo.processInfo.hostName.components(separatedBy: ".")[0]
  ) -> Bool {
    let host = machine.agent.flatMap(URL.init(string:))?.host?.lowercased()
    var address = in_addr()
    let ipv4Loopback =
      host.map {
        inet_pton(AF_INET, $0, &address) == 1 && UInt32(bigEndian: address.s_addr) >> 24 == 127
      } ?? false
    return name.lowercased() == hostname.lowercased() || host == "localhost" || host == "[::1]"
      || host == "::1" || ipv4Loopback
  }
  public static func run(
    _ steps: [DeploymentStep], cancel: Cancellation, output: @Sendable (String) -> Void
  ) throws {
    try CheckoutMutation.run(repos: steps.flatMap(\.checkouts)) {
      try runUnlocked(steps, cancel: cancel, output: output)
    }
  }
  private static func runUnlocked(
    _ steps: [DeploymentStep], cancel: Cancellation, output: @Sendable (String) -> Void
  ) throws {
    for step in steps {
      guard !cancel.isCancelled else { throw FleetError("Cancelled") }
      output("▸ " + step.label + "\n")
      if let repo = step.pushRepo, let branch = step.pushBranch {
        try Commit.pushCheckout(repo: repo, branch: branch, cancel: cancel, output: output)
        continue
      }
      let result =
        try step.administrator
        ? Administrator.run(step.command, cancel: cancel, output: output)
        : Runner.run(step.command, cancel: cancel, output: output)
      _ = try result.checked()
    }
  }
  public static func flakeURL(path: String, branch: String, attribute: String) -> String {
    var url = URLComponents()
    url.scheme = "git+file"
    url.host = ""
    url.path = URL(fileURLWithPath: expandHome(path)).standardizedFileURL.path
    url.queryItems = [URLQueryItem(name: "ref", value: "refs/heads/" + branch)]
    url.fragment = attribute
    return url.string!
  }
  static let remoteFlakeAssignment = #"""
    cd -P . || exit 1
    encoded=$(printf '%s' "$PWD" | od -An -v -tx1 | awk '{for(i=1;i<=NF;i++) if($i=="2f") printf "/"; else printf "%%%s",toupper($i)}') || exit 1
    flake="git+file://$encoded?rev=$(git rev-parse HEAD)" || exit 1
    """#
  public static func plan(_ action: Action, fleet: Fleet, name: String) throws -> [DeploymentStep] {
    guard let machine = fleet.machines[name], let sync = machine.sync else {
      throw FleetError("No sync settings for \(name)")
    }
    let local = isLocal(name, machine)
    guard local || machine.ssh != nil else { throw FleetError("\(name) has no SSH destination") }
    let checkouts = [fleet.repoPath, local ? expandHome(sync.repo ?? ".") : nil].compactMap { $0 }
    func prepared(_ steps: [DeploymentStep]) -> [DeploymentStep] {
      steps.map {
        var step = $0
        step.checkouts = checkouts
        return step
      }
    }
    var steps: [DeploymentStep] = []
    if action == .sync, let repo = fleet.repoPath {
      steps.append(
        DeploymentStep(
          label: "Push checkout commits (uncommitted changes stay behind)",
          command: Command("/usr/bin/true"), pushRepo: repo, pushBranch: fleet.branch))
    }
    let repo = sync.repo ?? "."
    let attr = sync.flake ?? (sync.preset == .homeManager ? machine.ssh ?? name : name)
    let ref = shellQuote("refs/heads/" + fleet.branch)
    let pull = """
      for marker in rebase-merge rebase-apply MERGE_HEAD CHERRY_PICK_HEAD REVERT_HEAD sequencer index.lock; do
        test ! -e "$(git rev-parse --git-path "$marker")" || { echo "Git operation in progress: $marker"; exit 1; }
      done
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
        arguments = [
          "switch", "--flake", flakeURL(path: dir, branch: fleet.branch, attribute: attr),
        ]
      } else {
        arguments = sync.preset == .nixDarwin ? ["--rollback"] : ["switch", "--rollback"]
      }
      steps.append(
        DeploymentStep(
          label: "Switch this Mac", command: Command(program, arguments, timeout: 3600),
          administrator: true))
      return prepared(steps)
    }
    var fragment = URLComponents()
    fragment.fragment = attr
    let target = "\"$flake\"" + shellQuote("#" + (fragment.percentEncodedFragment ?? ""))
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
    let flake = remoteFlakeAssignment
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
    return prepared(steps)
  }
}
