# Window lifetime benchmark

These runs never discover, activate, click, or stop an existing app. The harness launches each executable itself and retains its `Popen` object. Every signal goes to that child only. An exclusive file lock rejects overlapping benchmark runs. Use a disposable fleet and repository; the agents must serve the same frozen response to every variant. The command file and trace must live on a local filesystem.

## Boundaries

`MENAGERIE_WINDOW_TRACE=/absolute/file` opts a Swift test process into diagnostics. Create the empty file before launching. Write `open` or `close` to `/absolute/file.command`, then send SIGUSR1 to that test PID. The open command activates its own app, presents its popover from a transparent one-point benchmark anchor window, waits 500 ms, and invokes the same `PopoverRoot.openFleet` action used by the Open button. Benchmark-only blur and outside-click dismissal are disabled during this short automated presentation, in both apps. It asserts that its popup is shown and does not synthesize a status-item click. The anchor avoids the menu-bar manager hiding the source view. `MENAGERIE_TEST_TITLE=BENCH` also distinguishes the test item visually.

The start is inside that action, before dismissal and window creation or activation. The end is the first display submission with a live report: the probe waits until it is attached to a visible window with live content, lays out and displays the window, then flushes the Core Animation transaction. Wall-clock timestamps identify both events; elapsed milliseconds use a monotonic clock. This measures the first display submission, not physical monitor scanout. The Rust control likewise presents its own popup and activates its app before the same 500 ms wait. The Rust patch starts inside `open_main_window` and ends in gpui's callback directly after a live frame renders. Neither boundary includes command delivery, popup presentation, backend startup, or time waiting for the first HTTP response.

The probe also logs its own destruction and checks a weak reference to the window two seconds later. `window-released` distinguishes real destruction from ordering a retained window out. The benchmark-only `MENAGERIE_KEEP_WINDOW=1` close action uses `orderOut`, preserving the real window and its view graph. Ordinary closes always use normal AppKit close behavior.

## Variants

- `rust`: isolated pre-port Rust app at `99a0aef`, with `rust-timing.patch` applied. The later `74face6` change adds assets only and does not affect this one-machine, no-services fixture.
- `retained`: the previous Swift implementation at `4f14843`, with `prior-swift-timing.patch` and the current `WindowLifetime.swift` added. Despite the historical variant name, the weak-reference trace proves this implementation already destroys its main window and hosting view.
- `kept`: the same previous Swift executable with the explicit keep-alive switch. This measures an actual retained window rather than assuming that the previous implementation retained it.
- `released`: the current Swift app with closed-window terminal state, popover hosts, and service image caches discarded.

To prepare the old source, use `git archive` into ignored directories under `target/`, not the owner's checkout. Apply the patches there with `patch -p1`. The old Rust build can reuse `GHOSTTY_SOURCE_DIR` and `CARGO_TARGET_DIR` from the isolated baseline build. The Swift archive needs the built `target/ghostty` prerequisite at its package root. Build both Swift variants with `scripts/bundle` and keep the old bundle separately as `target/Menagerie-retained.app`. Preserve all signing and deployment boundaries.

## Running

From the repository root, with the isolated executables and `target/window-bench-fleet.yaml` prepared:

```sh
python3 macos/Benchmarks/window-lifetime/run.py rust
python3 macos/Benchmarks/window-lifetime/run.py retained
python3 macos/Benchmarks/window-lifetime/run.py kept
python3 macos/Benchmarks/window-lifetime/run.py released
```

`--executable`, `--fleet`, and `--output` override paths. Each variant takes five fresh processes. Cold means first window open after launch, with the backend already reporting. Warm means reopening after a separate 30-second close. It does not mean a cold OS file cache or reboot. The first process also captures physical footprint with `footprint -p` and eleven one-second `top` CPU deltas in all three states, using a 30-second settling interval. A separate close is used for warm timing so CPU sampling does not extend its wait. Run sequentially on the same machine; do not compile while collecting timings.

Summarize and validate all three requested options with:

```sh
python3 macos/Benchmarks/window-lifetime/summarize.py target/window-results
```

Raw traces and measurements go into `target/window-results/` (or `--output`); archived results and analysis accompany this document. The fixture's content hash identifies the frozen response without checking personal agent telemetry into the repository.
