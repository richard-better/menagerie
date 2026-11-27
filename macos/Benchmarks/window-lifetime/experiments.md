# Checks and discarded experiments

- Weak-reference probe: the previous Swift implementation already destroys the main window and its hosting view after a normal close. Keeping the terminal model's view alive was a separate ownership issue, now covered by a regression test.
- Allocator pressure relief: `malloc_zone_pressure_relief(nil, 0)` on a worker after close returned zero reclaimed bytes. It was removed rather than shipped as an ineffective cleanup action.
- Timing counter: the first probe used observation to trigger display checking. This could invalidate the scene. It was replaced with a passive counter and a display callback for existing windows. Superseded timing samples were discarded.
- Popup precondition: a source status button hidden by the menu-bar manager could fail to present a real popup. The harness now uses its own transparent one-point anchor, confirms the popup is shown, activates the app, and waits 500 ms before the open action. Benchmark-only outside-click and blur dismissal are suppressed during that short presentation.
- Overlap: an earlier pipeline overlapped a replacement. All affected measurements were discarded. The runner now acquires an exclusive lock and rejects overlapping invocations.
- Rust control: a polling foreground task could affect CPU and rendering. The final control uses a close-on-exec pipe, an async-signal-safe non-blocking byte write, and a bounded channel. Idle control does not poll the UI loop.

- Event-drain experiment: a harmless key-up event sent only to the test PID did not change its closed footprint (47 MB before and after). No event injection or allocator workaround is shipped.
- Idle cleanup: writing empty values through `@Observable` still invalidated observation. Cleanup now changes only nonempty state. A test verifies that closing an idle details scene sends no observation notification. The final released measurements use this guarded implementation; earlier released samples were replaced.

The final report uses only the explicitly identified serialized run, with five cold and five warm timing samples per option. Intermediate numbers are not evidence of a performance improvement.
