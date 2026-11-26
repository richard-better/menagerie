# Judge context: Menagerie mockups

## Screen
Menagerie is a Mac app for one person's small fleet of long-lived machines (2 to 10: laptops, a home
server, a smart-home box, a phone). Two views:

- **Menu bar popover**: a glance. Which machines are online, which need attention, one-click sync.
- **Full window**: a machine strip on top; selecting a machine shows its health (load, memory, disk,
  battery, temperature), its services (one-click links), how far its configuration is ahead/behind the
  repo with Sync / Roll back / Changes, and an embedded terminal.

## Settled decisions (judge within these)
- Dark look with dot-matrix charts and meters and monospace numbers.
- Machine strip at the top of the window selects which machine the panel shows.
- Two views: menu bar popover plus full window.
- A row of service icons under any machine that exposes services, in both views.
- Ahead/behind shown git-style (up/down arrows); one Sync button handles both directions.
- The commit list lives behind a "Changes" button, not in the main panel.
- An embedded terminal is part of the window.

## Known capture artifacts
- All numbers are placeholders.
- Service icons are letter placeholders for real app icons.
- The strip above the popover (Wi-Fi, a diamond badge, a clock) stands in for the macOS menu bar.
- These are HTML mockups, not the native app. No light theme exists yet.

## Taste and rubric
- Taste: ui-judge default (references/taste.md).
- Rubric: ui-judge default general rows, plus two owner's cases (Glance, Control room). No veto row.

## Panel
Gemini 4 Neon (Antigravity), Grok 4.6 (Grok), Claude Opus 5.5 (Claude), GPT-6.1 Sol (Codex).
One run each this round; no baseline repeats, so no noise floor yet.
