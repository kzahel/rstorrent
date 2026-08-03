# Current Within Table Selection

Status: Complete.

Topics: `table-interaction`, `web-ui-design`, `application-interface-direction`

## Motivation And Outcome

Tactical 068 separated a viewed active row from checked batch targets so a
user could inspect one torrent without discarding a batch. Trial use exposed
the unavoidable command ambiguity: one shared action bar could target the
viewed row or the retained checked rows, but no implicit rule made both safe.
An explicit action-scope switch would add another mode to an interaction that
was intended to become simpler.

Replace that independent model with one conventional selection set and one
singular current row constrained to that set. Checked rows are the selection
and therefore always own actions. The current row is the selected row that
owns keyboard focus and detail. Ordinary row activation or bare arrow
navigation collapses selection to the current row; checkbox and modifier
gestures build a multi-selection.

## Stable Scenarios

- Clicking or tapping a row body selects only that row, makes it current, and
  updates detail immediately.
- Arrow Up/Down and Home/End move current and replace selection with only the
  destination row.
- A row checkbox or Command/Control-click toggles that row without clearing
  other checked rows.
- Shift+Arrow, Shift+Home/End, and Shift-click replace selection with the
  inclusive anchor range; the moving endpoint is current and drives detail.
- Command/Control+A selects every logical filtered row while preserving the
  current row when it remains in that set.
- Actions always target every checked row. There is no batch mode and no
  independent action scope.
- Detail always represents the one current row and never unions data from a
  multi-selection.
- Escape or Done collapses a multi-selection to the current row.
- Files uses the same interaction even though it has no file preview yet.

## Scope

- Replace active-plus-batch torrent presentation state with a current torrent
  ID and selected torrent IDs that maintain `current in selected`.
- Replace the shared table's batch-mode callbacks with exact selection-change
  callbacks that carry the next selected IDs and current ID.
- Make row body and bare keyboard navigation exact singleton selection.
- Preserve checkbox, modifier, Shift range, select-all, touch long press,
  sorted order, hidden-target disclosure, and virtualization behavior.
- Make torrent and file action surfaces target the selected set directly.
- Keep current/detail behavior consistent in Workbench, Transfers, Library,
  Files, and the singular peer cursor.
- Update state, shared-table, application, and browser evidence.

## Non-Goals

- No explicit toolbar action-scope selector or retained batch while browsing
  an unrelated current row.
- No aggregate multi-torrent detail or file preview.
- No batch controls for read-only inspection tables.
- No engine, application-view contract, transport, persistence, Android, or
  physical-device change.
- No change to concurrent Speed history work or its generated contracts.

## Interaction And State Contract

For an actionable table, `selectedIds` is the complete command target set and
`currentRowId` is null or names one member of that set. Focus follows current.
The detail owner follows current. Checkboxes and `aria-selected` mirror the
selection exactly.

Exact replacement gestures discard hidden targets from an older filter:
ordinary activation supplies one ID, Shift ranges supply their logical range,
and Command/Control+A supplies the current filtered table. Individual checkbox
and modifier toggles preserve other selected IDs, including disclosed IDs
outside the current view.

When a toggle removes the current row, the table chooses another selected row
in current logical order when possible, then an existing hidden selected ID.
Removing the last selected row clears current and detail. Sorting and live
updates preserve identity. Source removal prunes selected IDs and chooses a
remaining selected current row rather than inventing an unrelated selection.

The visible multi-selection status appears when more than one row is selected
or any selected target is outside the current filtered table. Done and Escape
collapse to current. Empty-space activation clears selection and current.

## Ownership And Bounds

- The Zustand presentation slice owns torrent current and selected IDs across
  Transfers, Workbench, and Library.
- `FileTable` owns its mounted torrent's current and selected file IDs.
- `VirtualTable` owns focus, scrolling, the transient range anchor, and
  deterministic toggle fallback in sorted/filtered order.
- Selection replacement remains a linear pass over the already materialized
  logical rows and adds no DOM nodes, tasks, requests, persistence, or
  dependencies.
- Concurrent Speed work in overlapping presentation files is preserved and is
  not staged, reverted, generated, or otherwise reshaped by this tactical.

## Validation

- Store tests cover default selection, singleton replacement, exact set
  replacement, current removal fallback, source pruning, and clearing.
- Shared-table tests cover pointer singleton behavior, checkbox toggles, bare
  arrows, Shift ranges, select-all, Space, Enter, Escape/Done, sorting, nested
  controls, hidden selections, long press, and virtualized logical rows.
- Application tests cover immediate torrent detail, action targeting,
  Transfers/Workbench continuity, filters, and 4,095 Files rows.
- Playwright covers the desktop keyboard flow, touch regression, Files scale,
  current/selected accessibility semantics, and bounded DOM.
- Type checking, Vitest, production build/CSP, and `git diff --check` pass.

## Stopping Condition

This tactical is complete when actionable tables have one checked selection
set, current is constrained to that set, ordinary row navigation collapses to
one checked row, modifier/range/select-all interactions build the exact set,
details use current, actions use all checked rows, and the focused web evidence
passes without incorporating unrelated concurrent changes.

## Completion Evidence

Completed on 2026-08-03.

- The shared table now accepts one selected-ID set and one constrained current
  row. Row bodies and bare navigation replace that set with one row; checkbox,
  modifier, range, select-all, Space, and long-press paths update the same set.
- Torrent presentation state, detail consumers, command targeting, logs,
  Library, the peer cursor, and mounted Files use current/selected naming and
  no longer retain an active-versus-batch mode or command heuristic.
- `VirtualTable.test.tsx` passed 11 tests and `state.test.ts` passed 7 tests.
- Six selection-focused `App.test.tsx` scenarios passed, including singleton
  row actions, multi-row commands, cross-destination selection, keyboard
  detail, and 4,095 logical Files rows. After the concurrent Speed work landed,
  the complete web unit suite passed 132 tests with 2 expected skips.
- Four Playwright scenarios passed in Chrome: primary destinations, phone
  long-press behavior, wide keyboard interaction and accessibility, and the
  virtualized full file catalog.
- `npx tsc --noEmit`, a direct Vite production build, the CSP bundle check, and
  `git diff --check` passed. The direct build avoided regenerating concurrently
  edited API artifacts.
