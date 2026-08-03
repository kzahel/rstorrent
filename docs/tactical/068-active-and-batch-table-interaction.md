# Active And Batch Table Interaction

Status: Complete on 2026-08-03.

Topics: `table-interaction`, `web-ui-design`, `application-interface-direction`

## Motivation And Outcome

The shared actionable tables currently expose three independently moving
signals: a current/detail row, checked batch targets, and a keyboard focus
index. Arrow navigation can therefore put focus on one torrent while the
Workbench detail and strong row highlight remain on another. The same table
also lacks Shift+Arrow range selection and platform select-all even though its
pointer and checkbox equivalents exist.

Apply the accepted
[`table-interaction`](../topics/table-interaction.md) contract end to end.
Transfers, Workbench torrents, and Files gain one singular active row plus a
separate batch-selected set. Row focus follows active navigation, details
always use the active torrent, and keyboard range/select-all behavior operates
over the full sorted and filtered model rather than only virtualized DOM rows.

## Stable Scenarios

- Clicking or tapping a torrent row makes it active and updates Workbench
  detail. Arrow Up/Down, Home, and End produce the same active/detail change
  from the keyboard instead of moving a focus-only cursor.
- Clicking or tapping a Files row makes that file active. The same keyboard
  navigation updates its active treatment even though Files has no preview
  pane yet.
- Row-body activation keeps its active-row meaning while batch selection is
  present. Checkboxes, Command/Control-click, Space, Shift ranges, select-all,
  and touch/pen long press own batch membership.
- Shift+Arrow grows and shrinks one contiguous range in current sorted and
  filtered order. Its moving endpoint is active and therefore drives torrent
  detail.
- Shift+Home and Shift+End extend that range to the corresponding logical edge.
- Command+A on macOS and Control+A elsewhere select every row in the focused
  actionable table, including offscreen virtual rows, without changing its
  active row or detail.
- Enter activates the focused row and never changes batch membership.
- Escape or Done clears batch selection while preserving the active row.
- Multiple batch-selected torrents never produce unioned detail. Commands use
  the visibly disclosed batch target set while detail continues to show one
  active torrent.
- Read-only inspection tables do not gain batch shortcuts.

## Dependencies And References

- [`../topics/table-interaction.md`](../topics/table-interaction.md) owns the
  accepted vocabulary, interaction contract, accessibility semantics, and
  current gaps.
- [`058-contextual-table-selection.md`](058-contextual-table-selection.md)
  established active-versus-batch state, touch long press, guarded background
  clearing, and command targeting.
- [`059-actionable-table-range-selection.md`](059-actionable-table-range-selection.md)
  established permanent actionable-table checkbox geometry and sorted
  Shift-click ranges while explicitly deferring Shift+Arrow.
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md) owns the shared
  virtualized React surface, accessibility, and scale requirements.
- [`../topics/application-interface-direction.md`](../topics/application-interface-direction.md)
  owns Transfers/Workbench continuity and the torrent detail relationship.

This is presentation interaction. It changes no BitTorrent protocol, engine,
application-view, transport, persistence, or platform capability, so no
normative protocol or pinned libtorrent survey is required.

## Scope

- Rename ambiguous torrent and Files presentation state so singular active
  rows and plural batch-selected rows are evident at their owners.
- Extend `VirtualTable`'s generic contract with explicit active-row activation
  and batch selection semantics.
- Synchronize initial row focus, pointer activation, keyboard navigation,
  scrolling, and active/detail state.
- Add Shift+Arrow, Shift+Home/End, and scoped Command/Control+A over complete
  sorted/filtered rows.
- Keep row-body and Enter activation stable while batch selection is active.
- Preserve and disclose batch-selected torrents outside the current filter or
  destination. Exact replace-range and select-all operations replace the batch
  set with their current logical rows; individual toggles retain other IDs.
- Apply the shared behavior to Transfers, Workbench torrents, and Files.
- Update focused state, component, application, browser, accessibility, and
  virtualized-scale evidence plus owning documentation.

## Non-Goals

- No Rust, engine, generated contract, application-view, persistence,
  transport, Android, or physical-device change.
- No new file preview, torrent detail aggregation, row actions, destructive
  action policy, routing, or persisted selection state.
- No checkbox column or batch behavior for read-only Peers, Swarm, Trackers,
  Disk, Speed, DHT, or Logs surfaces.
- No marquee, drag, cross-table range, or persistent range-anchor selection.
- No change to the existing bounded long-press thresholds or virtual-table
  rendering geometry unless testing exposes a same-boundary defect.

## Interaction And Command Contract

`activeRowId` is optional and singular. `batchSelectedIds` is a bounded set of
stable IDs. A table-local range anchor and the active logical row determine
Shift ranges. DOM focus is allowed on nested controls, but row-level roving
focus always returns to the active row.

Plain row activation calls only the active-row owner. Batch membership changes
call only the batch owner. Shift range operations are the deliberate combined
path: they replace the batch range and activate the moving endpoint in one
user interaction.

Torrent commands retain the established target rule:

- outside batch-selection context, the active torrent is the only target; and
- inside batch-selection context, only batch-selected torrents are targets.

The table status reports the total batch count and, when filters hide some
targets, the number outside the current view. Individual toggles preserve those
hidden IDs to retain Transfers/Workbench continuity. Range replacement and
Command/Control+A replace the batch set with their exact current logical rows,
so those operations cannot retain undisclosed out-of-range targets.

Files uses the same mechanics with component-local presentation state. The
existing priority command targets the active file outside batch context and
the checked file set inside it.

## Ownership, Bounds, And Data Flow

- The per-application Zustand presentation slice owns the active torrent,
  torrent batch context, and batch-selected torrent IDs. Existing snapshot and
  removal repair continues to prune missing IDs without inventing a new active
  row after explicit clearing.
- `FileTable` owns the active file and batch set for its mounted torrent and
  resets them when the torrent changes.
- `VirtualTable` owns one transient range-anchor ID, row focus mechanics, and
  virtual scrolling. It receives owner callbacks for activation and exact
  batch replacement rather than owning application identity.
- Range and select-all work scan/slice the already materialized sorted rows
  once. They add no row DOM nodes, background tasks, timers, dependencies,
  requests, or persistence.
- The existing long-press timer remains the only asynchronous table
  interaction and retains its unmount, scroll, move, release, and cancellation
  cleanup paths.

## Shape-Changing Edge Cases

- A table can have no active row. First row-level focus activates the focused
  logical row rather than leaving a focus-only cursor or skipping to row two.
- An active row can be outside the batch set; active styling/detail and batch
  styling/commands remain visibly distinct.
- Sorting preserves active and batch identity while row indexes change.
- Filtering can hide batch targets. Their count is disclosed until an exact
  range/select-all replacement or explicit clearing removes them.
- If a sort/filter removes the range anchor, the next Shift operation falls
  back to the active row and then its endpoint.
- Reversing Shift direction shrinks through the anchor and grows on the other
  side without accumulating the old range.
- Select-all includes every filtered logical row even when virtualization has
  mounted fewer than 100 of thousands.
- Command/Control+A is ignored inside input, textarea, contenteditable,
  checkbox, menu, dialog, resize, and other nested controls.
- An incoming update that removes the active torrent clears detail; removal of
  batch rows prunes only those rows.
- Enter in batch context cannot accidentally add or remove a command target.

## Implementation Order And Gates

1. Record this tactical and add pure `VirtualTable` tests that express active
   navigation, range movement, select-all, and stable batch behavior.
2. Rename torrent store and Files-local presentation concepts, preserving
   existing repair and command semantics under state tests.
3. Implement the shared row activation and keyboard contract, then connect
   Transfers, Workbench, and Files.
4. Extend application and browser tests for immediate detail updates,
   cross-destination continuity, filtered hidden-target disclosure, and the
   4,096-row Files scenario.
5. Run the web validation matrix, update the tactical and owning topic with
   actual evidence, and commit only files in this slice.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure store | Active activation/clear/repair, batch enter/toggle/replace/exit, disappearance, and command scope |
| Shared table | Pointer active behavior, Arrow/Home/End, Shift ranges and shrink/reverse, platform select-all, Enter, Space, Escape, nested-control exclusion, sorting, and hidden counts |
| Torrent application | Transfers and Workbench active/detail continuity, batch command targets, filters, and no unioned detail |
| Files | Active row navigation, exact batch range/select-all, priority target scope, and reset on torrent change |
| Browser/accessibility | Desktop keyboard flow, phone pointer/touch regression, focus/current/selected semantics, empty serious/critical axe findings, and bounded virtual rows |
| Build | `git diff --check`, TypeScript checking, Vitest, production build/CSP check, and deterministic Playwright suite |

Rust workspace, interoperability, public swarm, Android, and physical-device
validation are not relevant because this slice does not change those owners.

## Escalation And Stopping Condition

Ordinary component refactoring, presentation-state naming, focused CSS,
fixture updates, accessibility corrections, and browser-test changes inside
this contract are authorized. Stop for direction if implementation requires a
new application command, generated contract, persisted compatibility policy,
different multi-torrent command policy, or a new product detail/preview
surface.

This tactical is complete when torrent and Files tables use the accepted
active-versus-batch vocabulary, row focus and active/detail navigation remain
synchronized, Shift keyboard ranges and scoped platform select-all operate
over complete logical tables, browser and scale evidence passes, owning docs
record actual evidence, and no backend file is included in the commit.

## Implementation And Evidence

The shared `VirtualTable` contract now names `activeRowId`, `onActivate`, and a
separate `batchSelection` owner. Row focus activates its row; Arrow, Home, and
End move active state and torrent detail immediately. Shift variants replace a
contiguous logical range and activate its endpoint. Meta+A and Control+A
replace the batch set with all sorted and filtered rows, including rows outside
the virtual DOM. Enter retains activation semantics, and Escape or Done clears
only batch state.

Torrent presentation and command state now distinguish `activeTorrentId` from
`batchSelectedTorrentIds`. Transfers and Workbench share both identities while
retaining independent filters. Hidden batch targets persist across those
filters and disclose their outside-view count; exact range and keyboard
select-all operations remove targets outside their new logical scope. Torrent
detail and Logs follow only the active torrent. Peer navigation uses the same
singular active vocabulary without gaining batch controls.

Files owns an `activeFileId` and separate checked set for the mounted torrent.
Arrow navigation updates the active treatment even without a preview, while
priority commands continue to target the active file outside batch context and
the checked set inside it. The 4,095 actionable non-padding rows remain
virtualized while keyboard select-all targets the complete logical catalog.

Validation on 2026-08-03:

- `npm run typecheck` passed.
- `npm test -- --reporter=dot` passed 126 tests; two opt-in interop tests were
  skipped.
- `npm run build` passed, including the production CSP scan of both JavaScript
  bundles.
- Playwright passed all 15 deterministic demo scenarios outside the unrelated
  Swarm finding described below. This includes the active/detail keyboard
  scenario, hidden filtered batch disclosure, cross-destination continuity,
  touch long press, Files keyboard range/select-all, bounded-DOM checks, and
  the associated serious/critical Axe checks.
- `git diff --check` passed.

The full deterministic Playwright invocation also exposed an unrelated
existing Axe failure in the phone Swarm summary: its horizontally scrollable
summary has no keyboard focus target. No Swarm source or style is part of this
slice. The focused table scenarios pass, and the Swarm finding remains with
that surface's accessibility owner rather than being hidden or folded into
this interaction commit.
