# Actionable Table Interaction

Topic: `table-interaction`

Status: Product interaction direction accepted on 2026-08-03. The current
shared web table implements current-row navigation and checked batch selection,
but keyboard row focus can still move without changing the active row,
Shift+Arrow range selection and Command/Control+A are absent, and existing
state names use `selected` for more than one concept. No implementation
tactical has yet applied the direction recorded here.

## Purpose And Scope

This topic owns consistent row activation, batch selection, keyboard focus,
range behavior, and master/detail semantics for actionable tables in the
shared browser and Tauri presentation.

The contract applies immediately to:

- the Transfers torrent table;
- the Workbench torrent table; and
- the Workbench Files table.

It also applies to a future table when that table exposes row actions or a
current-row detail, preview, or inspector surface. A table need not already
have a detail sub-pane to retain an active row; Files should behave consistently
now so a later file preview or inspector does not require a different
interaction model.

Read-only inspection tables such as Peers, Swarm, Trackers, Disk, and Logs do
not gain checkboxes or batch shortcuts merely for consistency. They may adopt
the active-row part of this contract when they gain a genuine row-detail use.

This topic does not decide which commands a table supports, whether a
destructive command permits multiple targets, what a file preview contains,
or whether Android Compose uses the same presentation. Those concerns remain
with their application, detail-view, and platform owners.

## Vocabulary

The interface has two user-visible row states.

### Active row

The **active row** is singular. It is the row the user is currently navigating
and the source for a table's detail, preview, or inspector surface. It receives
the strong current-row treatment. Ordinary one-row commands target it when no
batch selection is active.

For torrents, the active row owns the Workbench torrent detail. For Files, it
is retained even though no file preview exists yet.

### Batch-selected rows

The **batch selection** is a set of zero or more rows marked for multi-row
commands. Checkboxes represent this state. A count or command label must make
the batch scope visible whenever commands target it.

The active row and batch selection are deliberately different concepts. The
active row may be outside the batch set, for example while the user browses
details without discarding checked command targets. Multiple batch-selected
rows never cause detail projections to be unioned, intersected, or otherwise
combined. Detail always shows the one active row.

### Keyboard focus and range anchor

Keyboard focus is an implementation and accessibility mechanism, not a third
user-visible row selection. When focus is on a table row, it follows the
active row. Arrow navigation must not leave a focus outline on one row while a
different row retains the current highlight and detail.

Focus may temporarily move to a checkbox, header control, resize separator,
menu, or dialog without changing the active row. Returning to row navigation
restores the active row as the roving focus target.

A range anchor is also transient table mechanics. It identifies the fixed end
of Shift ranges but receives no separate persistent visual state or product
meaning.

## Core Invariants

- Exactly zero or one row is active in a table.
- Keyboard row focus and the active row do not drift independently.
- Activating a row updates its detail or preview immediately when one exists.
- Batch-selected rows are exactly the rows represented by checked selection
  controls and batch-selection accessibility state.
- Detail always represents the active row, never aggregate data synthesized
  from a batch selection.
- Row-body activation always means “make active”; entering batch selection
  does not repurpose an ordinary row tap into an unrelated operation.
- Batch membership changes through checkboxes, modifier gestures, Space,
  Shift ranges, select-all, and bounded touch/pen long press.
- Commands use one explicit scope: the active row when batch selection is not
  active, or the batch-selected set when batch selection is active.
- The visible command surface communicates batch scope before a batch command
  can run, especially when the active row is not batch-selected.
- Sorting, filtering, and virtualization never limit a range or select-all
  operation to rows currently mounted in the DOM.
- Read-only tables do not acquire batch-selection behavior without row actions.

## Interaction Contract

### Pointer and touch

| Interaction | Result |
| --- | --- |
| Click or tap a row body | Make that row active, move row focus to it, and update detail without changing batch membership. |
| Click a row checkbox | Toggle only that row's batch membership. |
| Command/Control-click a row | Toggle that row's batch membership as a desktop accelerator. |
| Shift-click a row or checkbox | Replace the prior contiguous Shift range with the inclusive anchor-to-clicked range in current sorted/filtered order; make the clicked endpoint active. |
| Click the header checkbox | Select or clear every logical row in the current filtered table. |
| Long-press with touch or pen | Enter batch selection and mark the held row, subject to the existing movement, scrolling, cancellation, and synthetic-click guards. |
| Activate empty table space | Clear batch selection first when it is active; otherwise clear the active row when the table supports an empty current state. |

Checkbox and modifier interaction must not bubble into a second row-body
activation. Row-body behavior remains stable while batch selection is active,
so a user can inspect another row without silently changing command targets.

### Keyboard

| Key | Result while row navigation owns focus |
| --- | --- |
| Arrow Up / Arrow Down | Move the active row and row focus together by one logical row, scroll it into view, and update detail. Batch membership is unchanged. |
| Home / End | Move the active row and focus to the first or last logical row and update detail. |
| Shift+Arrow | Grow or shrink an inclusive contiguous batch range from its anchor; the moving endpoint becomes active and updates detail. |
| Shift+Home / Shift+End | Extend the batch range to the first or last logical row; the endpoint becomes active. |
| Command+A on macOS / Control+A elsewhere | Batch-select all logical rows in the current filtered table while preserving the active row and its detail. |
| Space | Toggle the active row's batch membership and establish it as the next range anchor. |
| Shift+Space | Replace batch selection with the inclusive anchor-to-active-row range. |
| Enter | Activate or reveal the active row's ordinary detail behavior; never toggle batch membership. |
| Escape or Done | Exit and clear batch selection while preserving the active row and detail. |

If a table has rows but no active row when row navigation receives focus, the
focused row becomes active rather than creating a focus-only cursor. Empty
tables ignore row-selection shortcuts.

Select-all is scoped to the actionable table that owns row focus. It must not
override native text selection in an input, textarea, editable surface, dialog,
or other non-row control. Both Meta+A and Control+A may be recognized at the
web event boundary so the shared presentation follows the host platform
without user-agent inference.

## Sorting, Filtering, And Virtualization

All range and select-all operations resolve against the complete logical row
model after the current table's filter and sort, not source insertion order
and not only rendered virtual rows.

Stable row identity preserves the active row and batch membership through
sorting and live row updates when those rows remain available. If the active
row disappears from the underlying collection, it becomes empty rather than
silently moving to an unrelated row. Disappearing batch rows are pruned.

Transfers and Workbench currently share torrent batch state across
destinations. The exact policy for already batch-selected torrents hidden by a
later filter remains to be settled in the implementation tactical. That
tactical must either prune hidden targets or expose their count and scope; it
must not let an apparently local batch command silently affect undisclosed
hidden rows. Command/Control+A itself selects the complete current filtered
table, including offscreen virtual rows, and never means only the visible DOM
window.

Changing sort or filter invalidates a transient range anchor when its row is no
longer in the logical table. The next Shift operation falls back to the active
row and then to its own endpoint rather than using a stale index.

## Visual And Accessibility Contract

- The active row uses the strongest current-row treatment and remains visually
  distinguishable from batch membership.
- Batch-selected rows show checked checkboxes and a lighter selection treatment
  that can coexist with the active-row treatment.
- A keyboard focus ring appears on the active row during row navigation; it is
  not allowed to identify a different row.
- Color is not the only distinction. Checkbox state, current-row structure,
  and focus treatment remain perceivable in every theme.
- `aria-current` identifies the active row.
- `aria-selected` and checkbox state identify batch membership on actionable
  grids.
- The batch status uses explicit scope such as “3 selected for actions” rather
  than relying on the current-row highlight to imply command targets.
- Focus remains roving and virtualized: only the active or current keyboard row
  is in the row tab sequence, while scrolling can mount and focus an offscreen
  logical destination without rendering the whole collection.

The UI may retain a visible Select/Done affordance for discoverability and
touch use. It is a batch-command context, not a mode that changes the meaning
of row activation or permits focus and active state to diverge.

## State Ownership And Naming Direction

Active and batch-selection state are ephemeral presentation concerns. They are
not persisted as engine or application truth and do not cross the Rust
application boundary.

Implementation should replace ambiguous internal names where practical:

- singular values such as `selectedId` or `selectedFileId` become an
  `active...Id` concept;
- plural checked values become `batchSelected...Ids`; and
- selection-mode values identify batch-command context rather than ordinary
  row activation.

`VirtualTable` owns transient row focus, scrolling, and range-anchor mechanics.
Torrent presentation state owns the shared active torrent and torrent batch
set. The mounted Files surface owns the active file and file batch set until a
broader file-detail owner provides evidence for lifting that state.

## Current Evidence And Gaps

Tacticals [`058`](../tactical/058-contextual-table-selection.md) and
[`059`](../tactical/059-actionable-table-range-selection.md) established the
current active-versus-batch separation, visible actionable-table checkbox
column, sorted Shift-click ranges, touch long press, Space, Escape, and
virtualized bounds.

The current implementation still diverges from this accepted direction:

- Arrow, Home, and End update a table-local focus index but not the singular
  active row or its detail.
- Shift+Arrow and Shift+Home/End range selection are absent.
- Command/Control+A is absent even though the header checkbox can select all.
- Enter toggles batch membership while batch selection is active instead of
  preserving ordinary active/detail semantics.
- Row activation toggles batch membership while selection mode is active.
- State and accessibility code use overlapping `selected` terminology for the
  active row and batch set.
- Hidden batch targets across filter changes do not yet have an accepted
  prune-or-disclose policy.

These are presentation gaps, not application-view or engine limitations.

## Recommended Next Work

Open one bounded presentation tactical that:

1. updates `VirtualTable` to synchronize row focus, active state, scrolling,
   and detail activation;
2. adds Shift+Arrow, Shift+Home/End, and scoped Command/Control+A over the full
   sorted/filtered row model;
3. keeps row-body activation stable during batch selection and makes Enter
   activate rather than check;
4. adopts unambiguous active and batch-selection names in shared table,
   torrent presentation, and Files-local state;
5. settles hidden filtered batch targets without weakening Transfers/
   Workbench continuity silently; and
6. validates torrent and Files behavior through pure component, application,
   keyboard-driven browser, accessibility, and large virtual-table scenarios.

No Rust, generated contract, persistence, transport, engine, public-swarm,
Android, or physical-device work is required for that slice.
