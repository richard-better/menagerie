# Menagerie

A menu bar manager for a small fleet of personal machines: pets, not cattle.

Menagerie shows each machine's health, its services, and how far its configuration is ahead of or behind your repo, and syncs it in one click. The plan is in [docs/plans/master-plan-v1.html](docs/plans/master-plan-v1.html).

**Status:** early, and in daily use on one fleet. The agent reports each machine's health and the commit its configuration was built from; the app shows that live, syncs or rolls a machine back (on this Mac behind the system's administrator prompt, or none at all if sudoers allows the exact rebuild command; elsewhere over SSH), and opens a terminal on any machine, using Ghostty's libghostty-vt and an AppKit grid.

## The fleet file

Menagerie reads `~/.config/menagerie/fleet.yaml` (or `$MENAGERIE_FLEET`). [examples/fleet.yaml](examples/fleet.yaml) shows the format; on first launch the app offers to create the file from it.

Sync deploys commits, never a working tree: each machine builds the commit its checkout is on, so uncommitted edits (often a tool rewriting its own settings) neither block a sync nor end up in one. To deploy them, click "Checkout: uncommitted changes" in the window to commit and push them. Set `commit_message` in the fleet file to a command, such as a coding agent's CLI, and it drafts the message from the diff.

## Building

The app requires macOS 27 and Xcode 27.2 (Swift 6.4). It is a Swift package in `macos/`, with native SwiftUI controls and system Liquid Glass chrome. Rust is needed only for the agent and shared crates. Zig 0.17.1 builds the terminal library; `mise install` provides the pinned version.

```sh
mise install
scripts/build-ghostty
swift test --package-path macos
swift format lint --strict --recursive macos/Sources macos/Tests macos/Package.swift
scripts/bundle
open target/Menagerie.app
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The app starts in the menu bar. Left-click its mark for the fleet popover; right-click for Quit. Launch `target/Menagerie.app/Contents/MacOS/Menagerie --show-window` to inspect the full window directly, or use `--show-popover` for the popover. Only run one copy while testing. Use `MENAGERIE_FLEET=/path/to/fleet.yaml` to exercise disposable repositories.

`scripts/build-ghostty` checks out Ghostty commit `573c4a21e02311ae87844af26b6a7a179f82858e`, the revision used by libghostty-rs `4466c0f`, and builds its C library with Zig 0.17.1 for the host architecture and macOS 27.0. SwiftPM's `CGhostty` system-library module links the static archive through a small C bridge. Sources, headers, archives, and the PTY launcher binary stay under ignored `target/` directories. The bundle script runs this prerequisite automatically and signs the launcher as well as the app. No prebuilt Ghostty output is checked in.

[macos/TESTING.md](macos/TESTING.md) describes terminal safety, the legacy-test migration, and real-app verification.

`scripts/bundle` builds `target/Menagerie.app` signed with your Developer ID, and `scripts/bundle --notarize` also notarizes it (with a notarytool keychain profile named `menagerie`). macOS offers Touch ID in the administrator prompt only to notarized apps; other builds get a password field.

Screenshots with `screencapture` need Screen Recording permission for the terminal that runs it.

## Credits

The app uses [Yams](https://github.com/jpsim/Yams) for YAML and [Ghostty](https://github.com/ghostty-org/ghostty) for terminal emulation. Its AppKit/CoreText grid supports mouse selection, copying, and visible-text accessibility.

## License

MIT
