# Design review - Neon

## Scores

```scores
subject: A
Typography: 8.5
Hierarchy: 8.5
Spacing and density: 8
Colour and contrast: 8
Iconography: 8.5
Consistency and polish: 8.5
Clarity: 7.5
case: Glance: 1
case: Control room: 1
```

```scores
subject: B
Typography: 6.5
Hierarchy: 6
Spacing and density: 6
Colour and contrast: 5.5
Iconography: 6
Consistency and polish: 5.5
Clarity: 6.5
case: Glance: 7
case: Control room: 6
```

```scores
subject: C
Typography: 8.5
Hierarchy: 9
Spacing and density: 9
Colour and contrast: 9
Iconography: 8.5
Consistency and polish: 9
Clarity: 8.5
case: Glance: 1
case: Control room: 1
```

## Fixes

- MUST: Align the Load dot-matrix chart and all four health meter bars flush with their left labels and right values instead of indenting them inward. Where: `B-window-dark.png`, Load chart and Memory / Disk / Battery / Temperature meters. Why: Spacing and density, Consistency and polish.
- MUST: Show the current load value (e.g. `0.4`) and a y-axis scale or peak reference in the Load section so the latest state is readable before history. Where: `B-window-dark.png`, Load header and chart area. Why: Hierarchy, Clarity, Control room.
- MUST: Increase the embedded terminal height from 1 line to at least 6-10 lines (or make it resizable) so command output is usable without popping out. Where: `B-window-dark.png`, Terminal section at bottom. Why: Spacing and density, Control room.
- MUST: Replace decorative per-metric colours (purple Memory, yellow Disk, red-orange Temperature at 54°C, orange Load) with a calm neutral fill for healthy ranges and reserve amber/red strictly for warning or critical thresholds. Where: `B-window-dark.png`, Load chart and four health meters. Why: Colour and contrast, Clarity.
- MUST: Remove status-accent colouring from healthy or zero values - render `↑0` in muted grey instead of bright green, and render `⚡87%` in neutral text instead of amber in the machine strip. Where: `B-window-dark.png`, `richard-hp-server` card in top machine strip and detail header top-right. Why: Colour and contrast, Hierarchy.
- MUST: Make machine icons, online status dots, and device states consistent between views (match `homeassistant` icon orientation, `richard-hp-server` screen glyph, `richard-m4-max` charging state, and include status dots in the popover). Where: `B-window-dark.png` top machine strip and `B-popover-dark.png` machine rows. Why: Iconography, Consistency and polish, Glance.
- SHOULD: Increase contrast on tertiary metadata (`41%`, `66%`, `health 71%`, `412 cycles`, `fan 2100 rpm`, chart timestamps, `seen 2h ago`, `off`). Where: `B-window-dark.png` health meters and `B-popover-dark.png` offline row. Why: Typography, Colour and contrast.
- SHOULD: Clarify the popover's right-hand status/action column - style `↓4 Sync` clearly as an interactive button, replace the cryptic `-` badge on `homeassistant` with `no agent`, and hide or demote `Sync all (1)` when only one machine needs sync. Where: `B-popover-dark.png`, machine rows and footer. Why: Clarity, Glance, Consistency and polish.
- SHOULD: Rebalance the detail header so `Changes` / `Roll back` / `Sync` do not leave an empty void under the machine subtitle, and give the selected machine card in the top strip clearer surface/border separation. Where: `B-window-dark.png`, top machine strip and selected machine header. Why: Spacing and density, Hierarchy.
- SHOULD: Separate the refresh status (`↻ 8s`) from the `History` and Settings toolbar pill, and clarify whether `8s` means age ("8s ago") or refresh interval. Where: `B-window-dark.png` top-right toolbar and `B-popover-dark.png` top-right header. Why: Clarity, Consistency and polish.
- SHOULD: Use consistent storage units across Memory and Disk (both `GB` or both `GiB`) and place percentage readouts consistently across all four meters. Where: `B-window-dark.png`, Memory, Disk, Battery, Temperature meters. Why: Consistency and polish.

## What each view should show

### Popover

- **Show**:
  - Fleet online count (`3 / 4 online`) and clear last-updated age - instant fleet health check.
  - Per-machine online status dot, consistent hardware icon, name, and service icon row - fast scanning and one-click service access.
  - Explicit sync/attention control (`↓4 Sync` button, `✓` synced, `no agent`, `off` + last seen) - immediate next action.
  - `Sync all (N)` when 2 or more machines need sync, alongside `Open window` - fast bulk action and drill-down.
- **Move**:
  - Raw load figures (`load 2.1`, `load 0.4`) to the full window - unhelpful without scale; surface in the popover only if abnormal.
  - Healthy battery percentages (`58%`, `87%`) to the full window - show in the popover only when low.
- **Remove**:
  - Cryptic `-` badge on `homeassistant` - unexplained symbol; state `no agent` directly.
  - Redundant `Sync all (1)` primary button when only a single machine is behind - competes with the row's own Sync button.

### Window

- **Show**:
  - Top machine strip with status dot, consistent hardware icon, name, concise state (`in sync`, `↓4`, `no agent`, `seen 2h ago`), service icon row, and `+` button - effortless fleet switching.
  - Detail header with online status dot, role/OS/uptime, git-style ahead/behind (with zero muted), generation/commit range, and `Changes` / `Roll back` / `Sync` - full config control.
  - Current load number and y-axis scale alongside the `1h / 24h / 7d` dot-matrix chart - latest state before history.
  - Flush-aligned health meters (Memory, Disk, Battery, Temperature) with neutral healthy fills and threshold-driven warning colours - instant anomaly detection.
  - Services cards with one-click links and latency, plus a usable 6-10+ line embedded terminal with `Pop out` - complete control room in one window.
- **Move**:
  - Refresh age (`↻ 8s`) out of the `History` / Settings segmented pill into a separate status indicator - separates status from navigation.
  - Healthy battery wear details (`health 71%`, `412 cycles`) into a secondary popover or tooltip - keeps the health grid scannable.
- **Remove**:
  - Decorative per-metric bar colours (purple Memory, yellow Disk, red-orange Temperature at 54°C) - colour should communicate thresholds, not categories.
  - Amber highlight on healthy battery (`⚡87%`) in the strip and bright green on `↑0` - false emphasis that distracts from `↓4`.
