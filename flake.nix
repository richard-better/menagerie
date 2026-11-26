{
  description = "Menagerie: a menu bar manager for a small fleet of personal machines";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        menagerie-agent = pkgs.callPackage ./nix/agent.nix { };
        default = menagerie-agent;
      });

      # Runs the agent as a launchd agent on macOS and a systemd user service
      # on Linux. See nix/home-manager.nix for the options.
      homeManagerModules.agent = import ./nix/home-manager.nix self;
      homeManagerModules.default = self.homeManagerModules.agent;

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
