# Agent notes

- Workspace: `crates/fleet` (fleet file model, no UI), `crates/app` (gpui app: `src/ui` views, `src/platform` macOS menu bar code, `src/bin/preview.rs`).
- The plan and its decisions: `docs/plans/master-plan-v1.html`. UI judging rounds: `.ui-judge/`.
- Check: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. The app needs Zig 0.17 on PATH to build libghostty-vt: run cargo through `mise exec --` (see `mise.toml`). The published libghostty-vt 0.2.2 needs Zig 0.15, which can't link on macOS 27, so it comes from git, pinned by rev; changing the rev means updating `outputHashes` in `nix/agent.nix`.
- To look at the UI, run `cargo run -p menagerie --bin preview -- target/preview` and read `target/preview/window.png` and `popover.png`. It calls `screencapture`, which fails inside a sandbox ("could not create image from rect"); run it outside the sandbox.
- gpui 0.2.2 has no status item API, cannot move or hide a window after opening it, and forces a Dock icon; `src/platform` works around each through AppKit. Its `Display::bounds()` drops the origin, so monitor geometry comes from `display-info`.
- Colours live in `crates/app/src/theme.rs`; colour only carries meaning (green healthy or charging, amber needs a sync).
