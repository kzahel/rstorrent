# Tactical 070: Actionable Torrent Error Status

Status: Complete on 2026-08-04.

Topics: `web-ui-design`, `desktop-inspection-surface`, `table-interaction`

## Motivation And Outcome

The live browser investigation of a legacy resume failure exposed the durable
error in General, but Transfers and Workbench reduced the same torrent to the
word `error`. A user could not discover the reason from the table and had no
cue that General contained the explanation. Repeating Start happened to
refresh the stale row, but still did not reveal where the error detail lived.

Make a torrent status with an attached error directly explanatory and
actionable on both table surfaces. Hover exposes the complete bounded error;
the same control has an explicit accessible name; activation selects that
torrent, opens Workbench General, and moves focus to the visible error card.
Statuses without an attached error remain ordinary non-interactive text.

## Scope

- Share one error-status renderer between Transfers and Workbench.
- Expose the full existing `TorrentRow.error` through native hover help and an
  accessible control name without truncating it to the table column width.
- Add one presentation action that atomically selects the torrent, opens
  Workbench General, and records a one-shot focus target.
- Scroll and focus the General error card, then clear the one-shot target.
- Cover state, pointer/keyboard semantics, tooltip text, destination/tab
  navigation, focus, and the existing disk-error scenario.

## Non-goals

- Changing torrent, storage, migration, repair, resume, or command semantics.
- Fixing the separately observed stale `checking` view transition.
- Persisting diagnostics, command history, or presentation focus targets.
- Adding a general tooltip framework, modal error inspector, or Logs routing.
- Making statuses without a concrete error string appear actionable.

## Invariants

- The error string is already bounded and sanitized by the application API;
  presentation neither parses it nor derives torrent state from its text.
- Activating an error status establishes that torrent as the singleton current
  selection before showing detail, so action scope remains unambiguous.
- The destination, active tab, open detail, selection, and pending focus target
  change in one Zustand transaction.
- The nested status control cannot trigger row selection, range selection, or
  touch long-press handling through event bubbling.
- The one-shot focus target is ephemeral and clears after General consumes it
  or after unrelated navigation replaces it.
- Pointer hover is supplemental: keyboard and assistive-technology users can
  discover and activate the same control without hover.

## Validation

- Focused Zustand and React tests for the atomic presentation transition,
  hover text, accessible name, General navigation, and focused error card.
- Web formatting, lint, tests, and production build.
- Reload the existing loopback live browser, reproduce the retained repair
  error, inspect the status control, activate it, and verify General focus.

## Stopping Condition

This slice is complete when both torrent tables use the shared actionable
status for rows with errors, activation opens and focuses the exact General
error, ordinary statuses remain plain text, focused and web-wide gates pass,
the behavior is verified against the retained live failure, and this record
contains the exact validation evidence.

## Implementation And Evidence

`TorrentStatus` is the shared Transfers/Workbench renderer. A row with no
error retains its former plain status text. A row with an error renders one
nested button with the complete error and navigation cue in its native hover
title and accessible name. Pointer-down and click propagation stop at that
button so neither long press nor row-body selection runs as a side effect.

`openTorrentErrorDetail` performs one presentation transaction: Workbench,
singleton current selection, General, open detail, closed sidebar, and one
typed torrent-error focus target. General consumes that target after render,
scrolls its existing bounded error card into view, focuses the alert, and
clears the target. Ordinary navigation also clears stale targets.

Focused tests cover the exact Zustand transition and the permanent disk-error
scenario. The component proof checks full hover text, an accessible status
button, keyboard Enter, singleton selection, Workbench/General navigation,
focused alert content, the shared Workbench renderer, and a non-error status
remaining non-interactive.

Validation run on 2026-08-04:

```text
cd clients/web
npm run typecheck
npm test
npm run build
```

TypeScript checking passed. All 23 executed test files passed with 134 tests;
two files and two tests remained intentionally skipped. Vite produced the
production bundle and the CSP scan confirmed that both JavaScript bundles use
neither direct evaluation nor the Function constructor. The existing bundle-
size advisory remained non-fatal.

The production build was then reloaded in the existing loopback browser
against the retained
`20749a6415b552f3750e33cd2a87617b6b9f8266` failure. Repeating its known
two-Start sequence exposed the actionable status with the exact persisted
publication-name error. Both Workbench and Transfers activations selected
General, and the active DOM element was the `Storage needs attention` alert.
The torrent was returned to paused afterward; its staging artifacts were not
modified.
