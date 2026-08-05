# Tactical 085: Unified Contextual Selection Actions

Status: Planned on 2026-08-05.

Topics: `web-ui-design`, `table-interaction`, `application-control`,
`client-surfaces`, `code-organization-and-refactoring`

Dependencies: completed Tacticals
[`040`](040-torrent-lifecycle-retention-actions.md),
[`058`](058-contextual-table-selection.md),
[`059`](059-actionable-table-range-selection.md),
[`063`](063-live-file-selection.md),
[`069`](069-current-within-table-selection.md),
[`071`](071-copy-magnet-link.md),
[`073`](073-unified-storage-and-complete-recheck.md), and
[`077`](077-shared-overlay-menu-system.md) establish the semantic torrent and
file commands, current-within-selection table contract, canonical magnet copy,
managed full recheck, and shared overlay mechanics used by this slice.

## Decision And Motivation

Add desktop context menus to the actionable torrent and file tables, but do
not create a second action implementation beside the existing toolbar and More
menus. First extract one selection-action definition layer whose labels,
icons, grouping, availability, target policy, invocation, pending state, and
feedback feed every presentation of the action.

For torrents, the union of selection-scoped actions shown directly in the main
toolbar and in its More menu must equal the actions in the torrent row context
menu for the same target snapshot. Direct buttons and More are placement
choices, not different capabilities. Collection actions such as Add, Add test
torrent, Columns, Settings, and Done selecting are not torrent-selection
actions and are outside this equality rule.

For files, the visible More menu and row context menu expose the same binary
Normal and Skip actions through the same definitions and command owner. The
context menu does not invent high, low, sequential, streaming, deletion, or
path actions that the engine and visible product do not support.

The context menu may be an extension point for a later action that genuinely
depends on invocation context. This initial slice deliberately adds no
context-only selection action. Any future divergence must preserve a
discoverable non-right-click path unless the action is strictly desktop
context information.

All torrent-selection actions support more than one selected torrent. This
includes canonical magnet copy, force recheck, archive and restore, and
removal. The initial implementation may dispatch one existing semantic command
per torrent; it does not need a new batch command merely to coordinate the web
presentation.

## Desired Outcome And Stopping Condition

The tactical stops when:

- right-clicking a torrent row in Transfers or Workbench opens an accessible,
  collision-aware action menu at the invocation point;
- right-clicking a file row opens the corresponding Normal/Skip menu;
- an invocation on a selected row preserves the complete selection, while an
  invocation on an unselected row first replaces selection and current with
  that row;
- keyboard context invocation operates on current and the complete checked
  selection without requiring a pointer;
- one shared action-definition and execution boundary feeds direct toolbar
  buttons, toolbar More, torrent context menus, file More, and file context
  menus as applicable;
- the union of toolbar-direct and toolbar-More torrent-selection actions is
  exactly the torrent context-menu action set for the same targets;
- logical groups and separators have one stable order and empty groups never
  produce doubled or dangling separators;
- Start, Pause, Force recheck, Archive, Restore, Copy magnet links, and Remove
  all have explicit whole-selection behavior and never choose an implicit
  current, visible, or eligible subset;
- multi-torrent removal uses one confirmation dialog, defaults to keeping
  downloaded data, applies one explicit data policy to every target, reports
  bounded progress and partial failure, and can retry only the failed targets;
- file Normal/Skip continues to issue one bounded sorted multi-index command
  and gains no second context-specific implementation;
- visible controls remain the primary touch path and touch/pen long press
  remains the existing additive-selection gesture;
- no native browser context menu is suppressed outside an actionable row whose
  RSTorrent context menu can actually open;
- component, reducer/action-policy, production-browser, accessibility, scale,
  generated-contract, and proportionate Rust command evidence pass; and
- this tactical plus the owning topics record the implemented result, exact
  evidence, and remaining follow-up work.

## Existing Boundary

The current implementation already has most underlying behavior:

- `TorrentActions` derives selected rows, owns async status, and exposes Start,
  Pause, Archive/Restore, singleton Remove, and a More menu.
- `MoreActionsMenu` exposes singleton canonical magnet copy and the unrelated
  Add test torrent collection action.
- `FileTable` owns its local current/selection set and one multi-index
  `set_file_priority` command; `FileActionsMenu` exposes Normal and Skip.
- `VirtualTable` owns logical selection, current, focus, range selection,
  select-all, virtualization, and touch/pen long press.
- the local overlay wrapper already proves React Aria
  `trigger="contextMenu"` mechanics, point anchoring, focus, collision, and
  dismissal without attaching the behavior to product rows.
- the application command already includes `force_recheck`, but the web
  product has no visible action for it.
- removal is a durable per-torrent lifecycle operation with `keep` or
  `delete_managed`; the current confirmation dialog and web owner deliberately
  accept only one torrent.

The missing boundary is action ownership. Labels, enablement, target
derivation, grouping, and callbacks currently live across toolbar buttons,
feature-local menus, and component state. Adding context menus directly there
would duplicate policy and make the visible and contextual surfaces drift.

## Selection And Context Target Contract

The continuing current-within-selection invariants remain authoritative.
Context invocation adds these exact rules:

| Invocation | Target and current result |
| --- | --- |
| Right-click a selected row | Preserve the entire checked selection, including selected torrents outside the current filter. Preserve current when it is still selected. |
| Right-click an unselected row | Replace selection with only that row and make it current before opening the menu. |
| Context Menu key or Shift+F10 on a row | Use the current checked selection. If the focused row is not selected, establish it as the singleton selection first. |
| Invoke on empty table space, header, resize control, toolbar, or read-only table | Retain the ordinary browser/platform context menu unless that control owns a separate explicit menu. |
| Touch or pen long press | Retain the existing additive selection behavior; do not open the desktop context menu. |

Preparing an unselected context row changes selection/current but does not
perform ordinary row activation, navigate to focused Workbench detail, or
open a nested status control. Context-menu events must not bubble into click,
range, checkbox, empty-space, or error-detail behavior.

The menu opens against an exact ordered target snapshot. Torrent order follows
the complete materialized application torrent order filtered by selected IDs,
not DOM mount order, current filtered order, or checkbox-toggle history. File
order follows metainfo file index. A selection or target disappearance while a
context menu is open closes it instead of retaining stale targets. Toolbar
actions continue to derive the current live selection at activation.

Every action applies to the complete target snapshot. An action may be
disabled because one target cannot accept it, with an accessible bounded
reason. It must not silently skip that target and operate on the rest. A state
race after activation is handled as a per-target command result and included
in partial-failure feedback.

## Shared Action Model And Presentation Refactor

Introduce one small typed selection-action vocabulary for each actionable row
kind. Exact module names may follow the frontend's established naming, but the
model must remain plain data and functions rather than JSX duplicated between
surfaces. A definition contains the equivalent of:

```text
SelectionAction
  id
  label and optional pending label
  icon
  logical group
  direct-toolbar or overflow placement
  destructive flag
  availability plus disabled reason for an exact target snapshot
  invocation through the owning action runner
```

The action definition does not contain React Aria objects, transport frames,
Tauri calls, application-store mutation, or engine state. Feature presentation
maps definitions into the existing local RSTorrent button/menu wrappers.

Use one application-lifetime torrent-action owner above Transfers and
Workbench so toolbar and table context menus consume the same definitions,
pending operation, feedback, and removal dialog. This owner remains mounted
when the destination changes, preventing a multi-target command sequence from
becoming a detached component task. It may use a focused React context because
the toolbar, both torrent tables, context menus, and modal now demonstrate a
real shared owner; it must not become a generic dependency-injection or command
framework.

`VirtualTable` gains a narrow optional contextual-action contract. It owns
context invocation mechanics and selection preparation because it already owns
row focus and selection geometry. It does not learn torrent/file command
policy, action labels, archive state, magnet construction, or removal rules.
Read-only tables receive no context-menu binding.

Files retain a mounted-table-local action owner because their selection is
local to one torrent and both their visible More trigger and context rows are
inside `FileTable`. Reuse the shared action vocabulary/renderers without
lifting file selection into global Zustand state merely for symmetry.

The existing overlay layer remains the sole menu mechanic. Use its context
trigger mode and action-menu primitives for pointer coordinates, keyboard
opening, portals, viewport shifting/flipping, focus movement, Escape, outside
dismissal, and menu semantics. Add no overlay, positioning, menu, state
management, or data-grid dependency.

## Action Inventory, Ordering, And Grouping

### Torrent actions

The action order is stable across toolbar placement and context menus:

| Group | Actions | Whole-selection behavior |
| --- | --- | --- |
| Transfer | Start, Pause, Force recheck | Start sends Resume to every target; Pause sends Pause to every target; Force recheck sends the semantic recheck command to every target. |
| Sharing | Copy magnet links | Copy one canonical v1 magnet for every target as newline-separated text in stable target order. |
| Organization | Archive, Restore | Archive sets every target archived; Restore sets every target unarchived. Already-matching rows are semantic no-ops. |
| Destructive | Remove | Open one confirmation flow for the exact target snapshot; no removal begins before confirmation. |

Context menus render section boundaries between these nonempty groups. The
toolbar keeps Start and Pause as direct frequent actions. Exact placement of
Force recheck, Copy, Archive, Restore, and Remove between direct buttons and
More may preserve the current responsive layout, but their union must remain
the table above. Add test torrent remains a separately divided collection
section in toolbar More and is not rendered in row context menus.

Start and Pause are user-facing names for the existing durable Resume and
Pause intent. Do not add a second Stop command or imply process termination.
Both actions accept mixed running/paused selections: applying the target intent
to an already-matching row is a no-op, and every selected ID is still sent
through the application command owner. Removal-pending rows disable lifecycle
actions for the complete target set.

Archive and Restore are both explicit actions rather than one current-row- or
first-row-derived toggle. On a mixed archived/unarchived selection both remain
available and normalize the complete set in the chosen direction. If every row
already matches an action, that action may be disabled as already satisfied,
but the other direction remains available.

Force recheck is enabled only when every target has verified managed staging
or published content and no removal is active. The backend remains authority
at dispatch. Extend the torrent application view with the smallest semantic
recheck-availability value needed to avoid guessing from presentation status;
do not expose storage paths, artifact manifests, or SQL state. Regenerate and
validate the existing Rust/TypeScript/Kotlin contract after adding it.

Copy magnet links changes Tactical `071`'s singleton presentation limit. Build
each exact `magnet:?xt=urn:btih:<info-hash>` value from its projected v1
identity and join values with `\n`, without a trailing newline. Do not add
names, trackers, peers, web seeds, submitted-source fidelity, or other hidden
fields. One clipboard write follows explicit activation. It reports the exact
count on success and reports actual clipboard failure without claiming a
partial copy.

### File actions

Files retain one group with this exact order:

1. Normal
2. Skip

Both visible More and row context menu use the same definitions and current
`set_file_priority` execution. Target indices are sorted, unique, non-padding,
and cover the complete file selection. Demo mode and unsupported storage retain
truthful disabled or command-failure feedback. This tactical adds no numeric
priority scale or per-file deletion.

## Multi-Target Command Execution

Do not add a semantic batch API in this slice. The frontend action owner may
execute one existing command at a time in stable target order. Sequential
dispatch avoids an unbounded promise/task fan-out, respects the existing
application command serialization, and gives destructive storage cleanup no
new concurrency behavior.

The runner obeys these rules:

- at most one selection action is active for the application owner;
- it snapshots target IDs and bounded display labels before the first command;
- it starts at most one application command at a time and never retains one
  promise per target;
- command failure for one target does not prevent attempting later targets;
- status reports the action, completed count, total count, and failure count;
- retained diagnostic display includes at most the first five bounded
  target/error summaries plus the remaining count;
- navigation between Transfers and Workbench does not cancel the owner;
- application unmount or close prevents starting another queued target after
  the current application call settles and observes runner termination; and
- duplicate activation is disabled while the action is pending.

Each per-torrent dispatch retains the existing fresh request-ID, durable
receipt, replay, revision, owner-join, and error semantics. The presentation
does not claim atomicity: another client may observe each successful torrent
transition separately, and a later target can fail after earlier targets
succeed.

## Multi-Torrent Removal Contract

Extend the existing removal dialog from one row to an exact nonempty target
snapshot. The singular case preserves its current wording and behavior. The
plural case shows:

- **Remove N torrents?** as the title;
- the first five bounded torrent names in target order and an **and N more**
  summary rather than an unbounded list;
- downloaded data kept by default;
- one **Also delete downloaded data** checkbox applying the same policy to
  every target; and
- the existing irreversible managed-data warning when deletion is checked.

Keep-data removal is available when every target is present and is not already
in a nonretryable pending removal stage. Delete-managed is enabled only when
every target reports managed deletion support. A mixed-capability selection
does not silently delete capable rows while retaining the rest; the checkbox
is disabled with a count and explanation. The user may still remove the whole
selection while keeping data.

Confirmation runs the ordinary per-torrent remove command sequentially. The
dialog remains modal and shows `Removing X of N…`; Escape, backdrop dismissal,
policy changes, and duplicate confirmation are disabled while a command is in
flight. Closing the application stops scheduling undispatched removals after
the current call settles; already accepted durable removal operations retain
their existing recovery behavior.

If every removal succeeds, close the dialog and report the removed count. If
some fail, successful rows are never sent again: keep the dialog open with
only failed targets as the retry set, show the total and at most five bounded
failure details, preserve the chosen policy when it is still supported, and
offer **Retry failed** plus Close. A target that disappeared because another
client removed it is classified truthfully from the command result; it is not
reported as this runner's successful deletion.

Focus restoration depends on the invoking surface. A canceled toolbar dialog
returns to its Remove trigger. A canceled context dialog returns to the
originating row when it remains mounted, otherwise current row, table grid, or
collection heading in that order. After successful removal of every target,
focus must never be sent to a disconnected row or button.

This is coordinated multi-removal, not transactional batch deletion. No
frontend rollback can restore a catalog row or managed data after an earlier
per-torrent command succeeds.

## Availability, Feedback, And Race Semantics

Action definitions derive presentation availability from the exact target
snapshot and projected semantic state, but the application command remains
authoritative. Availability is a usability hint, not authorization and not a
promise that state cannot race.

- Empty selection disables selection actions and gives a bounded reason.
- Any pending/awaiting removal disables Start, Pause, Force recheck, Archive,
  Restore, and new Remove for the whole selection.
- Failed removal remains retryable through Remove under its existing durable
  semantics.
- Start, Pause, Archive, and Restore intentionally accept mixed target states
  and normalize every target rather than requiring a uniform selection.
- Force recheck requires every target's projected capability.
- Copy magnet links requires every target to retain a valid projected v1
  identity, which is already invariant for current torrent rows.
- File Normal/Skip requires a nonempty non-padding file selection and no
  existing priority command in flight.

Toolbar and context-menu presentations consume the same availability result
and disabled reason. They must not restate status comparisons independently.
Async success and failure feed one polite status owner. Context menu closure
does not discard command progress, reopen on failure, or claim success before
the underlying operation accepts it.

## Owner, Task, And Dependency Map

```text
projected torrents + shared torrent selection
  -> application-lifetime torrent action owner
       -> pure action definitions and availability
       -> toolbar direct buttons + toolbar More
       -> Transfers/Workbench row context menu
       -> one removal modal and sequential command runner
       -> existing InspectionApplication command adapter

projected files + FileTable-local file selection
  -> FileTable-local file action owner
       -> pure file action definitions
       -> visible file More + file row context menu
       -> existing bounded set_file_priority command

VirtualTable row/focus/selection mechanics
  -> shared overlay context trigger
  -> feature action renderers
```

The application action owner owns one optional active runner, its target
snapshot, progress, bounded failure summaries, status, and removal dialog
state. It owns no engine state, durable receipt, socket, file handle, task
handle, transport queue, or background timer. Underlying calls remain owned by
the active application adapter and application service.

Pure action definitions depend on presentation model values and semantic
callbacks. `VirtualTable` depends only on generic row selection and rendered
context content. Overlay wrappers depend outward on React Aria. No command or
torrent type enters the generic overlay layer, and no React, browser event, or
menu type enters the generated application contract.

## Shape-Changing Edge Cases

- Right-clicking one member of a multi-selection that includes hidden torrents
  retains and discloses the complete action target count.
- Right-clicking an unselected row while other rows are selected replaces the
  target set before menu items compute availability.
- Right-clicking a checkbox, error status, or cell does not toggle, activate,
  range-select, or navigate after opening the row menu.
- A row virtualized or filtered away closes its open context menu and never
  receives restored focus while disconnected.
- Shift+F10 and the Context Menu key open the same menu and action set as a
  pointer invocation.
- Mixed running/paused and archived/unarchived selections expose deterministic
  normalize-all actions rather than a first-row-derived toggle.
- A selection containing one recheck-ineligible torrent disables Force recheck
  for the complete set with no eligible-subset dispatch.
- Copying several magnets writes exactly one newline-delimited clipboard value
  in stable full-selection order.
- Multi-remove with mixed managed-deletion support allows keep-data removal but
  not a split hidden policy.
- One, middle, several, or every per-target command failing produces exact
  bounded progress and retry behavior without duplicate successful dispatch.
- A target disappearing between menu open, dialog confirmation, and command
  dispatch is reported without corrupting selection or retry state.
- Changing torrent while a file context menu is open closes it; file action
  targets cannot cross into the next torrent's catalog.
- The 4,096-file fixture and a large virtualized torrent collection retain
  bounded rendered rows and one-at-a-time command execution.
- Demo, ordinary browser, Tauri, dark/light, every interface size, narrow
  viewport, zoom, and reduced-motion presentation retain usable visible action
  paths even though row context invocation is desktop-oriented.

## Non-Goals

- a semantic Rust batch-command envelope, atomic multi-torrent transaction, or
  rollback across successful commands;
- concurrent removal or lifecycle command fan-out;
- a separate Stop command, force-start mode, queue reordering, labels, root
  relocation, export, open/reveal, copy path, or Share sheet;
- higher/lower/sequential/streaming file priority or per-file deletion;
- context actions on Library media cards, Android Compose, Peers, Swarm,
  Trackers, Disk, Logs, or other read-only tables;
- converting touch/pen long press from additive selection to context-menu
  opening;
- exact submitted magnet reconstruction, tracker/display-name augmentation, or
  `.torrent` export;
- changing durable removal cleanup, storage deletion authority, Android SAF
  executor semantics, or application request receipts;
- persisting action selection, menu state, pending progress, or failure
  summaries; or
- a generic command palette, shortcut registry, action plugin API, or
  schema-rendered menu system.

## Implementation Sequence And Gates

1. **Pure action policy.** Add exact target ordering, action IDs, groups,
   availability, placement, and parity tests without changing rendered UI.
   Gate: mixed-state, hidden-selection, recheck-capability, and empty-selection
   cases produce the accepted action set and disabled reasons.
2. **Shared torrent owner and toolbar refactor.** Move current command, status,
   copy, and dialog ownership above toolbar/table siblings; render the existing
   direct and More paths from definitions with no context menu yet. Gate:
   existing behavior plus toolbar/More union tests pass, and navigation cannot
   detach an active sequential runner.
3. **Production torrent context menus.** Extend `VirtualTable` narrowly and
   bind Transfers and Workbench rows through the shared overlay layer. Gate:
   selected/unselected pointer and keyboard targeting, focus, portal,
   virtualization, dismissal, and action-set parity pass.
4. **Whole-selection torrent actions.** Activate multi-copy, multi-recheck,
   explicit Archive and Restore, normalize-all Start/Pause, and bounded
   per-target result reporting. Add the minimal projected recheck capability
   and regenerate contracts. Gate: every action targets every selected ID in
   stable order, mixed states are deterministic, and partial failure is exact.
5. **Multi-remove modal.** Generalize confirmation, deletion capability,
   progress, partial failure, retry, target disappearance, and focus fallback.
   Gate: no removal precedes confirmation, each success dispatches once, and
   failed targets alone remain retryable.
6. **File context actions.** Render visible More and row context items from one
   definition owner while preserving the existing sorted multi-index command.
   Gate: normal, range, select-all, unselected right-click, 4,096-file, demo,
   and torrent-change cases pass.
7. **Product evidence and records.** Run production browser, accessibility,
   build/CSP, generated-contract, and proportionate Rust checks; update this
   tactical and owning topics with exact results and deliberate gaps.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure action policy | Exact stable IDs/order/groups/placement; toolbar-plus-More equals context; empty, singleton, mixed, hidden, removing, retryable, recheck-capable, and file-priority availability. |
| Store/presentation state | Selected/current preparation, hidden-target preservation, disappearance repair, context close on target change, and no action-state persistence. |
| Components | Shared renderer parity, logical separators, direct/More layout, selected versus unselected context invocation, keyboard context invocation, no row-event leakage, async pending, bounded feedback, and focus return. |
| Multi-command runner | Stable full-target order, one in flight, already-satisfied no-ops, continue-after-failure, first-five error bound, unmount termination, and no duplicate activation. |
| Multi-remove modal | Singular copy preservation; plural count/preview; keep default; all-target delete capability; warning; pending progress; partial success; retry failed only; disconnected-origin focus fallback. |
| File actions | Normal/Skip action equality, exact sorted unique indices, range/select-all, demo/unsupported failure, torrent switch, and 4,096 logical files with bounded DOM. |
| Generated contract | Minimal recheck-availability value round-trips through Rust generation, JSON Schema/Ajv validation, reducers, live/demo mapping, TypeScript, and Kotlin compilation. |
| Production browser | Transfers and Workbench pointer/keyboard context menus, viewport-edge placement, virtualized row anchors, clipboard readback for multiple magnets, multi-remove confirmation, no serious/critical Axe findings, and visible touch alternatives at phone width. |
| Rust/application | Existing Pause/Resume, Archive/Restore, ForceRecheck, and Remove idempotence/error tests plus focused projection evidence; no public network or new interoperability run is required. |

Run at minimum:

```text
npm test --prefix clients/web -- --run
npm run typecheck --prefix clients/web
npm run build --prefix clients/web
npm run test:e2e --prefix clients/web -- <focused deterministic cases>
cargo fmt --all -- --check
cargo clippy -p rstorrent-session -- -D warnings
cargo test -p rstorrent-session <focused action/projection cases>
git diff --check
```

Use the production browser bundle for final interaction evidence. Public
swarm, live Internet, visible Tauri, Android emulator/device, and physical
ChromeOS runs are not required because this slice composes existing semantic
commands and presentation mechanics without changing engine, storage, or
platform execution.

## Reference And Provenance Scope

This slice changes web presentation and composes existing semantic application
commands. It adds no BitTorrent protocol, scheduling, storage, discovery, or
performance behavior, so no new normative BEP or pinned libtorrent source/test
survey is required. Tacticals `040`, `063`, and `073` retain the source dossiers
for removal, file selection, and force recheck semantics.

Tactical `077` already records the WAI-ARIA menu-button/menu patterns and the
official React Aria Menu, Popover, and context-trigger behavior adopted by the
shared overlay layer. Reuse that dependency and evidence; do not copy external
source, fixture, style, or test data.

## Escalation Contract

Implementation may proceed autonomously for the focused frontend refactor,
minimal recheck-capability projection, generated-contract updates, sequential
multi-command runner, multi-remove dialog, deterministic tests, ordinary
accessibility fixes at the same boundary, and owning documentation updates.

Stop for direction if evidence requires a new semantic batch API, changes the
meaning or durability of an existing command, permits different deletion
policies within one confirmed target set, makes removal concurrent, replaces
long press with context invocation, adds a dependency, adds a context-only
selection capability with no discoverable alternative, or materially changes
Android/platform storage behavior.

## Potential Follow-Up Work

- Add a typed semantic multi-torrent command when measured round-trip cost,
  cross-client progress, durable group receipts, cancellation, or stronger
  consistency justifies moving orchestration below presentation. Multi-remove
  still cannot promise rollback of already deleted data.
- Add an application-owned operation/progress view if very large selections
  need to survive presentation disconnects and reconnects rather than only
  Transfers/Workbench navigation.
- Consider bounded concurrency for non-destructive idempotent actions only
  after sequential latency is measured; removal remains sequential until its
  storage-owner consequences are separately designed.
- Consider context-only **Open in Workbench**, reveal/open, copy path, source
  details, or export actions only after each has a visible or keyboard-
  discoverable alternative and truthful platform capability contract.
- Extend contextual actions to Library cards if their content-versus-torrent
  identity and multi-selection behavior are accepted.
- Design Android Compose multi-selection actions as platform-appropriate
  app-bar, overflow, or bottom-sheet controls rather than copying desktop
  right-click mechanics.
- Add High/Low, sequential, streaming, or deadline file priority only with the
  corresponding engine scheduling, persistence, interoperability, and product
  semantics.
- Consider a command palette and customizable shortcuts after the shared action
  vocabulary has enough real consumers to justify a global discovery surface.
