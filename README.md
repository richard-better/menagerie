# Menagerie

A menu bar manager for a small fleet of personal machines: pets, not cattle.

Menagerie shows each machine's health, its services, and how far its configuration is ahead of or behind your repo, and syncs it in one click. The plan is in [docs/plans/master-plan-v1.html](docs/plans/master-plan-v1.html).

**Status:** early, and in daily use on one fleet. The agent reports each machine's health and the commit its configuration was built from; the app shows that live, syncs or rolls a machine back (on this Mac with sudo's native password dialog, elsewhere over SSH), and opens a terminal on any machine, emulated by Ghostty's libghostty-vt.

## The fleet file

Menagerie reads `~/.config/menagerie/fleet.yaml` (or `$MENAGERIE_FLEET`). [examples/fleet.yaml](examples/fleet.yaml) shows the format; on first launch the app offers to create the file from it.

## Building

Needs Rust, macOS, and Zig 0.17 for the terminal (Ghostty's libghostty-vt is built from source): `mise install` provides it from `mise.toml`. gpui's shaders are compiled at runtime, so the Command Line Tools are enough; the full Xcode is not required.

```sh
cargo run -p menagerie          # the menu bar app; right-click ◆ to quit
cargo run -p menagerie --bin preview -- target/preview   # PNGs of both views
cargo test --workspace
```

The preview uses `screencapture`, which needs the Screen Recording permission for the terminal that runs it.

## Credits

The menu bar integration follows [hi5](https://github.com/jondot/hi5) (MIT), a gpui menu bar app.

## License

MIT
