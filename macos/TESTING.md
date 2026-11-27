# Native app verification

Run `mise install`, `scripts/build-ghostty`, then `swift test --package-path macos`. The build script pins both Ghostty and Zig; SwiftPM links `target/ghostty/lib/libghostty-vt.a` using the `CGhostty` system-library module and `GhosttyBridge`. `scripts/bundle` builds these prerequisites and includes the separately signed `MenageriePTYHost` executable beside the app. The launcher explicitly claims the controlling PTY before `execve`, which Darwin's `posix_spawn` file actions alone do not do.

## Terminal boundaries

The worker alone owns Ghostty, PTY reads and writes, resize, process-group signals, and reaping. Reads use 8 KiB chunks, with at most 32 per turn. The output channel holds one immutable latest frame, replacing older frames. Input is limited to 256 outstanding writes and 1 MiB, including the in-flight write. Pasting caps UTF-8 at 256 KiB and removes ESC from bracketed paste payloads.

Ghostty refuses APC and unknown payload accumulation; the stream filter also refuses APC before parsing. Scrollback is bounded to 4 MiB and 2,000 lines. Each copied grapheme is bounded to 255 UTF-8 bytes; an oversized cluster displays a replacement character. Grid dimensions cap at 512 columns and 256 rows. All PTY descriptors are close-on-exec and non-blocking on the app side; child descriptors remain blocking. The unreaped session leader reserves the process-group ID until the whole group has received SIGKILL, then reaps the leader. Close allows one second after SIGHUP, then kills the group; `killAll` skips the grace period.

The AppKit grid draws styled runs with CoreText, using JetBrainsMono Nerd Font Mono, JetBrainsMono Nerd Font, Symbols Nerd Font Mono, then Menlo. Mouse selection uses terminal columns, so a wide character is copied once. The visible text and selection are exposed through accessibility.

## Legacy terminal tests

All twelve Rust terminal tests have Swift equivalents in `TerminalTests.swift`:

| Rust test | Swift coverage |
| --- | --- |
| `keys_become_terminal_bytes` | `terminalKeysBecomeBytes` |
| `the_grid_fits_the_space_inside_the_padding` | `gridFitsSpaceInsidePadding` |
| `pastes_are_bracketed_when_asked` | `bracketedPastesCannotEscapeAndRespectCap` |
| `the_screen_follows_modes` | `ghosttyTracksModesWideGlyphsAndAlternateScreen` |
| `wide_glyphs_cover_two_columns_and_the_cursor_follows_columns` | Same test, with exact columns, widths, and cursor checks |
| `a_shell_on_a_pty_round_trips` | `shellOnPTYRoundTripsAndDescriptorsAreNonblockingCloseOnExec` |
| `closing_kills_a_shell_that_ignores_hangup` | `closeKillsShellAndChildrenIgnoringHangupAndTerm` |
| `pty_descriptors_close_on_exec_and_never_block` | Round-trip test checks both descriptor flags |
| `kill_all_ends_shells_at_once` | `killAllDoesNotWaitForGracePeriod` |
| `graphics_payloads_are_refused` | `ghosttyRefusesGraphicsWithoutBufferingPayloads` feeds 1 MiB directly to Ghostty |
| `the_write_budget_holds` | `writeBudgetIncludesInFlightInputAndCloses` |
| `the_screen_keeps_text_colours_and_the_cursor` | `ghosttyKeepsColorsCursorAndReplies` |

Additional cases cover the latest-frame output slot, normal shell exit with background children, close before launch, single-use launch, `/dev/tty` job control, Unicode paste caps, selection across rows and wide glyphs, and styled-run grouping. Lifecycle tests are serialized because `killAll` intentionally closes every session in that test process.

The fleet generation race is covered in `GenerationTests.swift`: suspend a poll, change and reload the fleet, release the old result, and assert that none of its report or service-health state lands. Identical and reordered fleets retain reports and the generation. The test uses a continuation gate, not timing assumptions.

Deployment tests cover cancellation of hanging steps, SIGTERM-resistant children, descendants that close output, cancellation before spawn, stopping before subsequent steps, exact branch names including tag collisions, dirty-tree diagnostics, shell quoting, fast-forward updates with `pull.rebase`, and rebasing without merge commits. HTTP fixtures validate protocol versions, revision hashes, service health, and Git drift. Existing commit tests cover literal pathspecs, unrelated staged files, secret filtering, symlinks, drafting stdout, and line counts.

## Tests intentionally not retained

Only `platform::flips_against_the_primary_height` and `platform::hangs_centred_and_clamped` were omitted. They tested coordinate conversion and manual placement required by gpui. `NSPopover` now owns placement and clamping; retaining those helpers would test unused code. Verify the actual anchored popover in the bundle instead. The old mark-decode and pasted-one-line tests have native equivalents in `BrandTests.swift` and `CoreTests.swift`.

The old ignored Authorization Services test is retained as an opt-in test, because it displays administrator prompts. Run `MENAGERIE_TEST_ADMIN=1 swift test --package-path macos --filter administratorStreamsRealRootIdentityAndCancellation` to check real/effective root identity, streamed output, exit status, and pipe-shutdown cancellation. It runs harmless temporary shell commands, never a system rebuild.

## Real app and performance

Use a disposable `MENAGERIE_FLEET` and configuration checkout. Expand the menu bar manager's « item before testing. A zero-height status-item frame while the manager hides the icon is not a usable popover anchor. Left-click the app mark, check its popover arrow, right-click for Quit, and verify that Quit ends the test PID while leaving the owner's process intact.

For comparisons, build the Rust baseline from the pre-port commit into an isolated target directory. Use the same disposable fleet, frozen agent response, selected machine, window size, and warm-up interval for both apps. Capture `footprint -p PID` and `top` samples after launch before any window, with the details window open, and 30 seconds after closing it. Stop only the explicitly recorded test PIDs.

Meters and the load chart draw through Core Graphics in AppKit views, updating only when their inputs change. The footer records the last poll time rather than running a per-second relative timestamp. Keep the system glass controls and chrome when measuring rendering changes.
