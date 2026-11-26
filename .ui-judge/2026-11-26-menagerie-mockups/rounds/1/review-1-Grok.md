# Review 1 - Grok

Scored from the four listed shots only. Capture artifacts (placeholder numbers, letter service tiles, fake menu-bar strip, HTML-approximate chrome) are ignored.

```scores
subject: A
Typography: 7.5
Hierarchy: 8
Spacing and density: 7
Colour and contrast: 8
Iconography: 7.5
Consistency and polish: 7
Clarity: 7.5
case: Glance: 1
case: Control room: 1
```

```scores
subject: B
Typography: 7
Hierarchy: 6.5
Spacing and density: 7.5
Colour and contrast: 7.5
Iconography: 6.5
Consistency and polish: 6
Clarity: 6.5
case: Glance: 6.5
case: Control room: 7
```

```scores
subject: C
Typography: 8
Hierarchy: 7.5
Spacing and density: 7.5
Colour and contrast: 8
Iconography: 7.5
Consistency and polish: 8
Clarity: 8
case: Glance: 1
case: Control room: 1
```

## Notes

**A.** Hero spend wins, then the quota bar, then the expensive service. Charts sit under current totals, which is the right order. Type is a real scale (hero number, section names, captions) and the dotted meters carry state (green remaining, yellow near-limit, orange spend). Density is a tool density: the Session 2% and Weekly 14% still need the bar to explain them, and "Reset credits 1" arrives with no setup. One mood, one chart language.

**B.** The two views already contain the product: strip plus panel, popover as glance, service tiles, git-style ahead/behind, one Sync, Changes, terminal. Craft is close to a shipping utility. The misses are hierarchy and one language for state. In the popover the loudest type is `3 / 4 online`, not the machine that needs a sync. Trailing status is four encodings (check, minus, the word `off`, a Sync button). `load 2.1 · 58%` glues two metrics together, and 58% is unlabeled. In the window the Load block is a 24h history with no current load, so history beats latest state. The same machine disagrees across shots (`homeassistant`: `online` vs `no agent`). Device glyphs are line icons in the popover and illustrated hardware in the window. Glance is a few seconds of reading, not instant. Control room works: health, Sync, services, and a shell are all on the selected machine, with extra chrome (gen/hash, long subtitle, ↑0) keeping it off 9.

**C.** Numbered steps, one card treatment, accent only on the selected option. Type and contrast are calm and readable. The display hero is sparse next to the tight option rows, and the gear/eye control has no label, so you infer Settings vs Preview. Otherwise a clear chooser.

## Fixes (subject B)

- MUST: Make the machine that needs action the first read, heavier than the online count. Where: `shots/B-popover-dark.png`, header `3 / 4 online` vs the `richard-hp-server` row. Why: Hierarchy, Glance - latest need should beat a census.
- MUST: Use one trailing status language (mark, tint, or the Sync control), not a check, a minus, the word `off`, and a button. Where: `shots/B-popover-dark.png`, right edge of every machine row. Why: Iconography, Clarity, Glance - four encodings force reading.
- MUST: Split load and battery; label the percent. Where: `shots/B-popover-dark.png`, `richard-m4-max` and `richard-hp-server` sublines (`load 2.1 · 58%`). Why: Clarity - a middle dot makes 58% look like load.
- MUST: Put current load next to (or above) the Load chart, same pattern as Memory and Disk. Where: `shots/B-window-dark.png`, Load block. Why: Hierarchy - latest state over history.
- MUST: Give `homeassistant` the same state in both views. Where: `shots/B-popover-dark.png` (`online`) and `shots/B-window-dark.png` strip (`no agent`). Why: Consistency and polish - sample state cannot contradict.
- SHOULD: Drop `↑0`; show only the non-zero direction. Where: `shots/B-window-dark.png`, right of the machine title. Why: Hierarchy, Clarity - a zero ahead-count is noise beside `↓4`.
- SHOULD: Move `gen 412 · 9f8e7d1 → a1b2c3d` behind Changes. Where: `shots/B-window-dark.png`, under the ahead/behind counts. Why: Clarity, Simple not minimal - hashes are the commit list, not the panel.
- SHOULD: Cut the selected strip card down to identity plus one attention mark; do not repeat `↓4` and `87%` that the panel already shows. Where: `shots/B-window-dark.png`, selected `richard-hp-server` tile. Why: Hierarchy - the strip selects, the panel reports.
- SHOULD: Collapse the subtitle to one role line (or hide OS/uptime one level deeper). Where: `shots/B-window-dark.png`, under `richard-hp-server` (`Media server · exit node · Debian 13 + Home Manager · up 12d 4h`). Why: Typography, Clarity - the line is long and all one weight.
- SHOULD: Use the same device glyph treatment in popover and window (one silhouette set). Where: `shots/B-popover-dark.png` row icons vs `shots/B-window-dark.png` strip illustrations. Why: Consistency and polish, Iconography.
- SHOULD: Stop filling the attention row as if it were selected. Where: `shots/B-popover-dark.png`, `richard-hp-server` card background. Why: Glance, Familiar before novel - a fill reads as selection, not "needs you".
- SHOULD: Give Load and Temperature different meter colours. Where: `shots/B-window-dark.png`, orange Load dots and orange Temperature bar. Why: Colour and contrast - accent is meaning, two metrics should not share it.
- SHOULD: Label or drop the `8s` chip in window chrome (the popover already has freshness). Where: `shots/B-window-dark.png`, top-right `8s | History | gear`. Why: Clarity - a lone duration is a working indicator without a referent.

## What each view should show

### Popover

**Show**
- Online/offline (or last-seen) per machine, as a compact mark.
- Which machine needs you, as the first optical hit, plus one-click Sync on that row.
- Sync all when more than one row needs it, and Open window as the escape to the panel.
- Service icons under machines that expose services (settled).
- Ahead/behind as git arrows on the row that is out of date (settled).
- A short live mark (load or battery) only when it is the reason for attention.

**Move**
- Routine load and battery percents: window meters, unless they are the alert.
- Freshness `8s`: one place in chrome, not a headline competitor.
- Anything that needs a chart (load over time, memory, disk, temp): the window.

**Remove**
- The selected-row fill when the popover is a list, not a picker.
- The minus control on `homeassistant` - it names no action and no state.
- The spelled-out `off` pill if a dim glyph plus `seen 2h ago` already says it.
- `3 / 4 online` as the largest type; keep a census only if it stays quieter than attention.

### Window

**Show**
- Machine strip that selects the panel, with a single attention mark on machines that need one.
- Current health first: load, memory, disk, battery, temperature, each with a live number and a meter; history charts under the number.
- Service links with a live check (latency is enough).
- Ahead/behind arrows, one Sync, Roll back, and Changes (settled).
- Embedded terminal on this machine (settled).
- A short identity line (name + one role).

**Move**
- Commit hashes, gen count, and the commit list: behind Changes.
- OS, uptime, "exit node", and other inventory: a details disclosure.
- History (if it is an event log): its own surface, not a peer of Sync.
- Terminal Pop out: keep, it is the advanced path for the shell.

**Remove**
- `↑0` and any other zero-count badge.
- Repeated `↓4` / `87%` on the selected strip tile.
- The dashed `+` tile if adding a machine is rare; put it in the gear menu.
- Duplicate freshness (`8s`) once the popover or a single chip already covers it.
