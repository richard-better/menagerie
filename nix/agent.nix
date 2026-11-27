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
