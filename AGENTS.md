# Agent notes

- Layout: `macos/` is the macOS 27 SwiftPM app. `MenagerieCore` owns fleet parsing, polling, Git, deployment, and cancellation; `Menagerie` owns SwiftUI views, the AppKit status item, and SwiftTerm integration. The Rust workspace keeps `crates/agent`, `crates/protocol`, and `crates/fleet`.
- Check Swift with `swift test --package-path macos` and `swift format lint --strict --recursive macos/Sources macos/Tests macos/Package.swift`. Check Rust with `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.
- Build the signed app with `scripts/bundle` using Xcode 27.2 or newer. `scripts/bundle --notarize` retains the notarytool profile flow. The executable accepts `--show-window` and `--show-popover` for real-app inspection. Use `screencapture` for screenshots.
- Exercise sync and commit against temporary repositories with `MENAGERIE_FLEET` pointing to a disposable fleet, using a `custom` sync command. Keep the owner's running app and real configuration checkout untouched by deployment tests.
- `macos/Sources/Menagerie/Views.swift` holds the palette. Colour carries meaning: green healthy or charging, amber needs attention, sage the primary action. Service letter icons retain their brand colours.
- Authorization Services runs only the rebuild as root, with a pipe-watching wrapper to cancel its process group. Git runs as the user. Preserve commit-only flake URLs, literal pathspecs, symlink-safe reads, secret filtering, and bounded terminal input/output when changing these paths.
- The original design plan is `docs/plans/master-plan-v1.html`; it contains historical Rust and deployment decisions. Current source is authoritative. Historical UI judging rounds are in `.ui-judge/`.
