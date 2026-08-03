# Actionable Table Range Selection

Status: Complete.

Topics: `web-ui-design`, `application-interface-direction`

## Motivation And Outcome

Tactical `058` separated current-row navigation from explicit batch selection,
but it omitted desktop Shift-range selection and hid the checkbox column until
selection mode began. The resulting mode is unambiguous, yet ordinary desktop
table behavior is incomplete and batch actions are less discoverable than
desired.

Keep the checkbox column visible for every table that supplies row actions,
including Transfers, Workbench torrents, and Files. Add Shift-click range
selection in the shared virtual table so one implementation follows the
currently sorted and filtered row order on both torrent surfaces and Files.

## Stable Scenarios

- Actionable tables always reserve the same 44-pixel checkbox column, so
  entering or leaving selection mode never changes data-column geometry.
- In normal mode, the current row remains visually current and drives ordinary
  actions, while its unchecked box truthfully indicates that no batch set is
  active.
- Clicking a row checkbox enters selection mode and checks that row without
  changing the current/detail row. Header select-all does the same for all
  currently sorted and filtered rows.
- Shift-clicking a row or row checkbox selects the inclusive range from the
  most recent selection anchor to the clicked row. If selection mode is
  inactive, it enters the mode first.
- Repeated Shift-click replaces the prior contiguous range from the same
  anchor, allowing the range to grow or shrink instead of accumulating stale
  rows.
- A normal selection toggle, Command/Control-click, Space, long press, or
  explicit Select entry establishes a new range anchor.
- Done or Escape still exits selection mode and clears checks without clearing
  the current row. Empty-space behavior and command targeting remain as
  established by Tactical `058`.

## Scope

- Separate checkbox-column presence from active batch mode in `VirtualTable`.
- Add one transient range-anchor owner per mounted table and an exact
  replace-selection callback to the generic selection contract.
- Implement row and checkbox Shift-click against the complete sorted/filtered
  row list, independent of virtualization.
- Make inactive row/header checkboxes enter selection mode in the torrent and
  Files owners.
- Update unit, application, browser, accessibility, and owning-topic evidence.

## Non-Goals

- No Rust, engine, application-service, generated contract, persistence,
  transport, or functional file-action changes.
- No drag marquee, Shift-arrow range extension, cross-filter hidden-range
  policy, or persistent selection anchor.
- No checkbox column on read-only inspection tables such as Peers, Trackers,
  Disk, or Logs.
- No change to destructive-action or mixed Archive/Restore policy.

## Interaction Contract

The current row and checked batch set remain distinct. In an actionable table,
the checkbox column is present in both Navigate and Select modes. Navigate mode
uses `aria-current` and the current-row highlight; its checkboxes and
`aria-selected` states represent the empty batch set. Checking any box or using
select-all enters Select mode. Select mode retains the count and Done control,
and checked rows become action targets.

The range anchor is a row ID owned by `VirtualTable`, not application state.
Entry and direct toggle operations replace it with their operated row. A
Shift-click resolves the anchor in `sortedRows`, falls back to the current row
and then the clicked row if necessary, slices the inclusive range, and replaces
the checked set in one owner transition. Sorting or filtering that removes the
anchor makes the clicked row the new one-row range. Leaving selection mode
clears the transient anchor.

## Ownership And Bounds

- The per-application Zustand store continues to own torrent mode and checked
  IDs; Files continues to own its equivalent state locally.
- `VirtualTable` owns one scalar anchor ID. Range resolution is one bounded
  scan/slice over the already materialized sorted row list and adds no DOM
  nodes beyond the permanently visible selection column.
- The replace callback accepts row values and lets each existing owner map to
  stable IDs atomically. No background task, timer beyond Tactical `058`'s
  long-press timer, dependency, network request, or storage write is added.

## Edge Cases

- The anchor may be above or below the clicked row; both produce display-order
  ranges.
- Range selection uses sorted/filtered order rather than source insertion order
  or only the virtualized rows currently mounted in the DOM.
- A range may include the current row without making checked membership and
  current/detail state the same concept.
- Shift-click after Done starts from the current row because the old batch
  anchor was cleared.
- Checkbox clicks and the full 44-pixel selection cell never bubble into normal
  row activation.
- Zero rows retain the fixed selection header but disable explicit Select and
  cannot create a batch target.

## Validation

- Pure component tests cover persistent checkbox geometry, inactive checkbox
  entry, forward/reverse range selection, range shrinking, sorted order,
  checkbox Shift-click, missing-anchor fallback, and mode exit.
- Application tests cover shared torrent range selection, single-row action
  targeting outside the mode, and Files range selection with disabled actions.
- Playwright covers desktop Shift-click, stable column geometry, phone
  selection, Files, bounded virtualization, and serious/critical axe scans.
- Run `git diff --check`, TypeScript checking, the web unit suite, production
  build, and deterministic Playwright suite.

## Implementation Record

- `VirtualTable` now distinguishes selection capability from active batch
  mode. Supplying the selection contract permanently reserves and renders the
  44-pixel checkbox column, while active mode alone controls checked targets,
  the selection count, and Done behavior.
- Current-row and batch semantics remain separate in both markup and styling:
  `aria-current` identifies the detail row, `aria-selected` reports checked
  membership, and `data-selected` drives the appropriate current or batch
  highlight.
- The table owns a transient stable-ID anchor and resolves Shift-click or
  Shift-Space against the complete `sortedRows` model. An atomic
  `onReplace(rows)` transition supports forward and reverse ranges, repeated
  growth or shrinkage, sorted order, and fallback when filtering removes the
  anchor.
- Normal row, checkbox, Command/Control-click, Space, long-press, and explicit
  Select paths establish an anchor. Checkbox and selection-cell events remain
  isolated from current-row activation.
- The shared torrent owner and local Files owner implement replacement and
  inactive checkbox/select-all entry. Read-only tables remain unchanged, and
  Files actions remain deliberately disabled pending backend work.
- No backend, generated contract, storage, transport, or protocol file changed.

## Evidence

- `git diff --check` passed.
- `npm run typecheck` passed in `clients/web`.
- `npm test` passed: 21 files passed, 2 skipped; 110 tests passed, 2
  skipped.
- `npm run build` passed.
- `RSTORRENT_PLAYWRIGHT_BASE_URL=http://127.0.0.1:4177 npm run test:e2e`
  passed 15 deterministic demo tests with 3 opt-in live tests skipped,
  including desktop and phone selection flows, fixed row geometry, Files
  virtualization, and serious/critical axe scans.
- The final Files scale sample retained 721 DOM elements, used 37,887,735
  bytes of JavaScript heap, and updated in 21 milliseconds.
- Manual wide Transfers, wide Files, and phone Files inspection confirmed the
  always-visible column, distinct unchecked current row, and compact layout.

## Stopping Condition

This tactical is complete when actionable tables retain visible checkbox
columns without conflating current and batch state, Shift-click range selection
works in sorted/filtered order for torrents and Files, the validation evidence
passes, owning topics record the superseding behavior, no backend file changes,
and the completed slice is committed.
