# Review 1 - Opus

## Notes per subject

**A** (usage dashboard): a strong reference. The total is the hero, the stacked dot bar is read in a second, and
each service section repeats one pattern: header, chart, quota bars, value line. Colour on percentages means
state (green headroom, amber 82%). Weak spots: the captions under the quota bars ("Too early in this window to
forecast") sit almost on the bar, the forecast tick on the bars is unexplained, and "Reset credits 1" in purple
adds a colour with no clear meaning.

**C** (light settings): calm, legible, numbered steps make the flow obvious, and the selected cards carry the
only accent. The toolbar icons (gear, eye) are oversized next to 17 px text, the "..." buttons only show on
selected cards, and the display picker's caret is the only hint that the strip is a picker.

**B** (the product): the bones are right. The strip reads as a selector, the selected machine's name is the
heaviest thing in the panel, Sync is the one blue control, and the popover's "↓4 Sync" row tells you what to do.
It loses points on colour discipline (every meter has its own hue, so yellow disk and red temperature read as
warnings when they are not), on consistency between the two views (status shown four different ways), and on a
Load chart that has no value and a lot of dead height.

## Scores

```scores
subject: A
Typography: 8
Hierarchy: 8
Spacing and density: 7.5
Colour and contrast: 7
Iconography: 7.5
Consistency and polish: 7.5
Clarity: 7.5
case: Glance: 1
case: Control room: 1
```

```scores
subject: B
Typography: 7
Hierarchy: 6.5
Spacing and density: 6.5
Colour and contrast: 5.5
Iconography: 6.5
Consistency and polish: 5.5
Clarity: 6.5
case: Glance: 7
case: Control room: 7
```

```scores
subject: C
Typography: 8
Hierarchy: 8
Spacing and density: 8
Colour and contrast: 8.5
Iconography: 7
Consistency and polish: 8
Clarity: 8.5
case: Glance: 1
case: Control room: 1
```

## Fixes (B)

- MUST: Make the four meters one neutral fill (light grey dots) and colour a meter only when it crosses a threshold (amber warn, red critical); keep green only for "charging". Where: B-window-dark, Memory / Disk / Battery / Temperature meters. Why: Colour and contrast - purple, yellow, green, red are decoration today, and yellow disk at 66% and red temperature at 54 °C read as alarms that are not there.
- MUST: Grey out a zero count in the ahead/behind pair: "↑0" in muted grey, "↓4" in amber. Where: B-window-dark, top right of the machine header. Why: Colour and contrast, Hierarchy - a green zero claims meaning it does not have and pulls the eye away from the 4 behind.
- MUST: Give Load a current value in monospace on the header row ("0.4", plus "4 cores" in dim text) and cut the chart height to about where the dots actually reach; move the time labels below the dots so "00:00" does not overlap the first column. Where: B-window-dark, Load section. Why: Clarity, Spacing and density - Load is the only health metric with no number, and roughly half its height is empty.
- MUST: Use one status vocabulary in both views: the same dot colours, the same words, the same right-hand badge. Today the popover's status column uses a green check circle, a grey dash circle, an "off" pill and an amber "↓4 Sync" text, while the window strip uses dots; homeassistant says "online" in the popover and "no agent" in the window. Where: B-popover-dark, right column of each row; B-window-dark, strip subtitles. Why: Consistency and polish, Clarity - a new user cannot tell what the dash means or whether "no agent" is a problem.
- MUST: Label the bare "58%" and "87%" figures (battery glyph before the number, every time) and make the strip subline and the popover subline show the same fields. Where: B-window-dark, strip under richard-m4-max ("in sync · 58%"); B-popover-dark, "load 2.1 · 58% ⚡". Why: Clarity, Consistency - "58%" alone could be CPU, disk or battery.
- MUST: Align the meters' left edge with their labels (or put the label inline on the left of the meter for all four); today the bars start about 48 px right of "Memory" and "Disk" while the sublines ("health 71%") start at the label edge. Where: B-window-dark, health grid. Why: Consistency and polish, Spacing.
- SHOULD: Move Roll back one level deeper (into the Changes sheet, or a "..." beside Sync), leaving Changes (secondary) and Sync (primary). Put the count in the primary: "Sync ↓4". Where: B-window-dark, header button row. Why: Hierarchy, Clarity - a destructive action sits at the same weight as a read-only one, and Sync does not say what it will do.
- SHOULD: Rename or explain the toolbar: "⟳ 8s" is unclear (refreshed 8 s ago? refresh every 8 s?) and "History" competes with "Changes". Use "Updated 8s ago" on hover and an icon-only refresh; rename History to what it is (e.g. "Activity"). Where: B-window-dark, top right toolbar; B-popover-dark, "⟳ 8s" top right. Why: Clarity, Iconography.
- SHOULD: Give the terminal real height (6 to 10 lines with a scrollback hint) or collapse it to a labelled bar; one prompt line inside a rounded box reads as a text field, not a shell. Where: B-window-dark, Terminal section. Why: Clarity - the "use a shell" path should look like a shell.
- SHOULD: Make the popover headline the attention count, not the online count: "1 needs sync" in amber as the hero, "3 / 4 online" as the dim secondary. Where: B-popover-dark, top left "3 / 4 online". Why: Hierarchy - the owner's glance question is "which machine needs attention", and the menu bar already says "↓4".
- SHOULD: Drop the full-row highlight on the attention row in the popover (it reads as hover/selection) and let the amber "↓4 Sync" badge carry it, or keep the highlight but make it a faint amber tint so it means state. Where: B-popover-dark, richard-hp-server row. Why: Colour and contrast, Consistency.
- SHOULD: Make the strip subline and selected-tile service icons match the strip's other tiles: homeassistant and hp-server show service icons in the strip but m4-max (no services) leaves a gap, so tile heights look uneven; reserve the icon row height on every tile. Where: B-window-dark, machine strip. Why: Consistency and polish, Spacing.
- SHOULD: Reduce the "+" add tile to a small "+" button at the strip's end; a full-size dashed tile gets the same weight as a machine. Where: B-window-dark, strip far right. Why: Hierarchy.
- SHOULD: Let service cards fill the row (two cards = two halves, or a fixed grid of three) instead of leaving an empty third. Where: B-window-dark, Services row. Why: Spacing and density.
- SHOULD: Design and show the states the shots skip: a machine with a failed sync (error with retry), a loading strip, an offline machine selected (stale data greyed with "last seen"), and a service that is down (red dot plus latency "-"). Where: both shots. Why: Clarity - every state is designed.

## What each view should show (B)

### Popover

**Show**
- Attention count as the headline ("1 needs sync") - answers the glance question first.
- One status badge per row in a single vocabulary (ok / behind / ahead / offline / error) - readable without reading.
- Inline Sync on any row that is behind or ahead, with the count - the one-click action.
- Service icons under machines that expose them - settled, and handy as quick links.
- Error states inline ("sync failed - retry") - attention includes failures, not only drift.
- Open window - the way deeper.

**Move**
- Load figures in every row ("load 2.1") - to the window; the popover is for state, not telemetry, unless load is in alarm.
- "⟳ 8s" refresh age - to a hover tooltip or the footer in dim text.

**Remove**
- "Sync all (1)" when only one machine needs it - duplicates the inline Sync; show it only when two or more need syncing.
- The green check circle on healthy rows - silence should mean OK; use a plain dot.

### Window

**Show**
- Machine name, role and uptime - identity first (already right).
- Ahead/behind with zero muted, and Sync carrying the count - the primary action.
- Changes as the secondary button - the commit list one level deeper (settled).
- Load with a current value, plus the four meters with values and neutral fills - health at a glance.
- Services with live status and latency - one-click links.
- Terminal at usable height with Pop out - the shell path.
- Last sync result and time ("Synced 3 h ago" or "Sync failed: ...") - missing today, answers "is this healthy".

**Move**
- Roll back - into the Changes sheet or a "..." menu; advanced and destructive.
- "gen 412 · 9f8e7d1 → a1b2c3d" - into Changes or a hover; useful, but not common-path.
- Battery health and cycle count, fan rpm - into a hover or detail; secondary to the live value.

**Remove**
- The per-metric rainbow of meter colours - decoration that fakes warnings.
- The full-size dashed "+" tile - replace with a small add button.
- "History" next to Changes, unless it is clearly a different thing - two names for history confuse.
