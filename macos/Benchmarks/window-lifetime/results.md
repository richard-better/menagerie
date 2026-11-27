# Window lifetime results, 2026-11-27

The 20 MB closed target is not met. The released Swift app measures 41 MB after a 30-second close, with verified main-window destruction. Keeping the window alive has no observed median reopen-speed benefit in these samples.

| Option | Never opened MB | Open MB | Closed 30 s MB | Cold ms median / max | Warm ms median / max |
| --- | ---: | ---: | ---: | ---: | ---: |
| Rust baseline | 24 | 75 | 34 | 35.3 / 49.5 | 31.1 / 39.6 |
| Swift, kept alive | 20 | 105 | 106 | 115.9 / 120.7 | 68.4 / 75.7 |
| Swift, released | 20 | 99 | 41 | 118.7 / 133.7 | 67.0 / 82.6 |

Five fresh processes per option, with five cold and five warm samples. Cold is first open after launch with the backend already reporting. Warm is after a separate 30-second close. Times start at the actual open-window action and end at first live display submission. They do not measure physical scanout. Physical footprint is one sample per state; differences of a few MB are not a demonstrated rendering improvement. All apps used the same frozen agent response, disposable repository, selected machine, nominal window size, and session. No compilation overlapped collection.

| CPU % mean / max | Never opened | Open | Closed 30 s |
| --- | ---: | ---: | ---: |
| Rust baseline | 0.05 / 0.5 | 4.93 / 20.1 | 0.05 / 0.5 |
| Swift, kept alive | 0.05 / 0.6 | 0.14 / 1.4 | 0.07 / 0.8 |
| Swift, released | 0.11 / 1.2 | 0.14 / 1.5 | 0.13 / 1.4 |

CPU uses eleven one-second `top` deltas after discarding its initial cumulative sample. All state medians were 0%. The Rust open-window bursts remain in the mean and max; no outliers were removed.

The kept option explicitly hides its window. The previous implementation already destroyed the window on a normal close, so it would be incorrect to label that previous normal-close behavior as retained. Every final released trace records `probe-released`, `window-released`, and `popover-released`. The closed footprint has only 304 KB Core Animation data, versus 57 MB with the window open. Its largest remaining category is Malloc Small (26 MB). This establishes destruction and graphics-resource release, not a complete attribution of every remaining allocation.

The appearance and system glass controls are unchanged. Terminal views are weakly cached; Back retains the shell. Closing ends sessions and clears inactive drafts, without changing the selected machine or cancelling deployment jobs. Popover hosts and service images are dropped when unused. Empty cleanup avoids invalidating observation.

Real-bundle check: select HP server, close through its own AX close button, reopen through its own popover action. HP remains selected, and all seven ivory service tiles render. A second disposable two-machine fleet verifies Back retains the exact shell PID, closing kills it, and reopening restores the second machine on Details. Evidence is in ignored `target/evidence/window-lifetime/` and `target/verify-{hp,shell}.trace`. No status item belonging to the owner was clicked.

Verification: 48 Swift tests passed, one administrator opt-in test skipped; 17 Rust tests passed. Swift strict format lint, Rust fmt, Clippy with warnings denied, and signed-bundle verification passed. The administrator prompt was not re-exercised for these lifetime-only changes.

[Protocol and baseline preparation](README.md), [discarded experiments](experiments.md), [raw data and metadata](results/2026-11-27/metadata.json), [summary](results/2026-11-27/summary.json).
