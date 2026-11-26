# Design review

You are reviewing app screens from screenshots. Work read-only: the only file you write is the
review file named in your task.

## The screen

The product is a Mac app for one person's small fleet of long-lived machines (2 to 10: laptops, a home
server, a smart-home box, a phone). It has two views:

- **Menu bar popover**: a glance. Which machines are online, which need attention, one-click sync.
- **Full window**: a machine strip on top; selecting a machine shows its health (load, memory, disk,
  battery, temperature), its services (one-click links), how far its configuration is ahead of or behind
  the repo with Sync / Roll back / Changes, and an embedded terminal.

Judge within these settled values; never ask to undo them:

- Dark look with dot-matrix charts and meters and monospace numbers.
- The machine strip at the top of the window selects which machine the panel shows.
- Two views: menu bar popover plus full window.
- A row of service icons under any machine that exposes services, in both views.
- Ahead/behind shown git-style (up and down arrows); one Sync button handles both directions.
- The commit list lives behind a "Changes" button, not in the main panel.
- An embedded terminal is part of the window.
- The most important thing is the most obvious; emphasis comes from weight before size or colour.
- Simple, not minimal: every element earns its place; the common path shows first, advanced options one level deeper.
- Colour carries meaning: accent marks state or action, never decoration.
- Every state is designed: empty, loading, long content and error look intended.

Known capture artifacts, not product bugs: all numbers are placeholders; service icons are letter
placeholders for real app icons; the strip above the popover (Wi-Fi, a badge, a clock) stands in for the
macOS menu bar; these are HTML mockups, so native controls and fonts are approximate.

## Subjects

Score each subject on its own shots, in this order. Score only what the shots show.

- **A**: `shots/A-dashboard-dark.png`: a dark usage dashboard (spend over time, per-service sections with dot-matrix charts and quota bars).
- **B**: `shots/B-window-dark.png`: a full window with a strip of machines at the top and one machine selected; `shots/B-popover-dark.png`: a compact menu bar popover listing the same machines.
- **C**: `shots/C-settings-light.png`: a light settings window with a device picker at the top and two rows of selectable option cards.

## Rubric

Scale: 1 to 10 in half points. 5 is functional but generic, 7 a good shipping app, 9 best in class.

| Row | Question |
| --- | --- |
| Typography | Type scale, weights and line lengths; how readable long text is |
| Hierarchy | Whether the eye lands first on what matters: the latest state over history |
| Spacing and density | Rhythm and grouping; how much fits without feeling cramped |
| Colour and contrast | Calm palette, legible secondary text, accent colour only where it means something |
| Iconography | Icons, avatars and symbols: consistent, sized to their text, never decoration |
| Consistency and polish | One way to do each thing; alignment, insets and timing that match across screens |
| Clarity | Whether someone new understands what each element is and what to do next |

Owner's cases (score B only; write 1 for A and C):

- **Glance**: "From the popover alone, can you tell which machine needs attention and what to do about it? 3: no, 5: with effort, 7: yes within a few seconds, 9: instantly, without reading."
- **Control room**: "From the window alone, can you understand one machine's state and act on it (sync, open a service, use a shell) without going anywhere else? 3: no, 5: partly, 7: yes, 9: yes, and nothing on screen is superfluous."

## Scores

Write one block per subject, every row filled, exactly in this form:

```scores
subject: A
Typography: 7.5
Hierarchy: 8
Spacing and density: 7
Colour and contrast: 7.5
Iconography: 7
Consistency and polish: 7
Clarity: 8
case: Glance: 1
case: Control room: 1
```

## Fixes

Subject B is the product under review. After the score blocks, list fixes for it, each ranked MUST or
SHOULD, with the shot and the location on screen:

- MUST: <fix>. Where: <shot>, <location>. Why: <what it improves, by rubric row>.

## What each view should show

Then, for B, answer for each view separately (popover, window) in three short lists:

- **Show**: information and actions that belong in this view, including anything missing today.
- **Move**: what is shown here but belongs in the other view or one level deeper.
- **Remove**: what does not earn its place.

Keep each item to one line, and say why in a few words.
