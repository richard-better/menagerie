# Sol design review

```scores
subject: A
Typography: 7.5
Hierarchy: 8
Spacing and density: 7.5
Colour and contrast: 6
Iconography: 7.5
Consistency and polish: 7.5
Clarity: 7
case: Glance: 1
case: Control room: 1
```

A has a strong total-first hierarchy, readable service names and convincing dot-matrix detail. Secondary forecast text, chart labels and the client breakdown recede too far into the background. The large segmented strip has no visible explanation of its segments, and the quota percentages do not explicitly say used or remaining.

```scores
subject: B
Typography: 7.5
Hierarchy: 7
Spacing and density: 7
Colour and contrast: 5.5
Iconography: 7
Consistency and polish: 7
Clarity: 6.5
case: Glance: 7
case: Control room: 7
```

B makes the selected server and its Sync action easy to find. The popover identifies the pending machine within a few seconds, but understanding the other states takes interpretation. The window has the right overall contents and a clear configuration action group; health interpretation and the usable shell area lag behind its visual finish. These scores cover the shown populated states only; loading, empty and error treatment cannot be assessed from these shots.

```scores
subject: C
Typography: 8
Hierarchy: 8
Spacing and density: 8
Colour and contrast: 7.5
Iconography: 7.5
Consistency and polish: 8
Clarity: 8
case: Glance: 1
case: Control room: 1
```

C groups choices exceptionally clearly, with consistent card geometry and unmistakable selected states. Some swatches communicate their effect better than others; the glass swatch is particularly faint. The unlabeled gear/eye and ellipsis actions require discovery, but the central choice flow remains understandable.

## B fixes

- MUST: Give missing-agent and offline states explicit, legible labels; use “No agent” in the homeassistant popover row instead of the unexplained dash, and keep the offline name and last-seen text readable. Where: B-popover-dark.png, third and fourth machine rows. Why: Clarity and colour and contrast; connectivity and management capability are different facts with different next steps.
- MUST: Raise contrast for secondary operational text, especially chart times, meter percentages, battery health, cycle count and fan speed. Where: B-window-dark.png, health panel; B-popover-dark.png, offline row. Why: Colour and contrast; small useful facts should remain readable without becoming primary.
- MUST: Give Load a current numeric reading and a clear unit or scale; show the latest sample distinctly. Where: B-window-dark.png, Load heading and chart. Why: Hierarchy and clarity; historical dots alone cannot tell whether the machine is currently under pressure.
- MUST: Make the embedded terminal tall enough to show command output, with a resize affordance or expansion control. Where: B-window-dark.png, bottom terminal section. Why: Spacing and density and clarity; the shown single prompt line supports entry but leaves no useful visible output area.
- SHOULD: Add a short “4 commits behind” explanation adjacent to the git-style counts, and identify the endpoints of the hash comparison in deeper Changes details. Where: B-window-dark.png, upper-right configuration status. Why: Clarity; keep arrows while making the direction and target understandable to a new user.
- SHOULD: Use neutral health-meter colours by default and reserve amber/red for defined warning conditions; retain green for a meaningful healthy or charging state. Where: B-window-dark.png, Load, Memory, Disk and Temperature. Why: Colour and contrast; purple memory, amber disk and orange temperature currently compete without explaining whether any needs attention.
- SHOULD: Reduce the height of the machine strip and blank area above the Load chart, reallocating space to the shell. Where: B-window-dark.png, top selector and upper health panel. Why: Spacing and density; preserve the top selector while making the control room more useful at the same window size.
- SHOULD: Promote a plain-language attention summary above routine online count, such as “1 machine needs sync,” and keep the pending row as the strongest list item. Where: B-popover-dark.png, header and server row. Why: Hierarchy; fleet attention matters more than the aggregate online total.
- SHOULD: Make the footer bulk action describe its scope explicitly, such as “Sync 1 machine.” Where: B-popover-dark.png, bottom-left button. Why: Clarity; “Sync all (1)” requires interpreting what the parenthesis counts.
- SHOULD: Give the refresh age a readable meaning, such as “Updated 8s ago,” with refresh as its own understandable action. Where: both B shots, top-right refresh indicator. Why: Clarity and consistency and polish; a circular arrow plus 8s could mean polling interval, elapsed age or countdown.

## B popover

### Show

- Attention summary and online count, with pending work first in emphasis; support an immediate fleet glance.
- Machine name, connectivity and configuration state in each row; distinguish offline, synced, pending and no-agent states.
- One-click Sync on machines with pending changes; make the next action local and explicit.
- Service icon row for each machine exposing services; preserve direct access without opening the window.
- Readable last-seen time for offline machines and an explicit no-agent label; explain incomplete information.
- Open window and a clearly scoped bulk Sync action; give both common next steps.
- Legible update age and refresh action; establish freshness.

### Move

- Routine load and battery readings to the window unless abnormal; prioritize state over telemetry in the glance.
- Agent setup explanation and configuration details into the window; keep the popover compact.

### Remove

- Unexplained dash status badge; it adds interpretation without information.
- “Off” badge when an explicit offline label and last-seen time already communicate the same state; avoid duplication.

## B window

### Show

- Compact top machine selector with connectivity, configuration state and service icon rows; preserve fleet navigation.
- Selected machine identity, role and uptime; establish context.
- Current health readings plus dot-matrix history and meaningful warning states; support diagnosis.
- Git-style ahead/behind counts, Sync, Changes and Roll back; support configuration control.
- Named service links with availability and response time; enable confident one-click access.
- A usable multiline terminal with expansion or resizing; support actual command work.
- Update age and clear refresh access; establish whether observations are current.

### Move

- Commit hashes and generation metadata into Changes; they compete with the actionable configuration summary.
- Battery cycle count and fan speed into expandable health details; keep the default health view focused on condition.
- Global History into a secondary navigation or overflow surface; its prominence need not match fleet state.

### Remove

- Redundant zero ahead count in the main summary when only behind is nonzero; show both directions when they contain information.
- Fixed metric-specific accent colours without state meaning; retain the dot-matrix design with semantic colour.
