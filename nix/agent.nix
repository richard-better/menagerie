{ lib, rustPlatform }:

let
  fs = lib.fileset;
in
rustPlatform.buildRustPackage {
  pname = "menagerie-agent";
  version = (lib.importTOML ../crates/agent/Cargo.toml).package.version;

  # Only what cargo needs: changes to docs or the app's assets don't rebuild
  # the agent.
  src = fs.toSource {
    root = ../.;
    fileset = fs.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../examples
    ];
  };
  cargoLock = {
    lockFile = ../Cargo.lock;
    # The app's terminal uses libghostty-vt from git. The agent doesn't build
    # it, but vendoring the workspace's lockfile still needs its hash.
    outputHashes."libghostty-vt-0.2.1" = "sha256-Wl/JOS2KD/s+fiUz5W2bgXt07gLhwWO9MkPWik/9/gs=";
  };

  cargoBuildFlags = [
    "-p"
    "menagerie-agent"
  ];
  cargoTestFlags = [
    "-p"
    "menagerie-agent"
    "-p"
    "menagerie-protocol"
  ];
  # Reads the build machine's disks and sensors, which a sandbox may hide.
  checkFlags = [
    "--skip"
    "a_full_run_fills_the_always_available_probes"
  ];

  meta = {
    description = "Reports a machine's health and configuration revision over HTTP for Menagerie";
    homepage = "https://github.com/richard-better/menagerie";
    license = lib.licenses.mit;
    mainProgram = "menagerie-agent";
    platforms = lib.platforms.darwin ++ lib.platforms.linux;
  };
}
