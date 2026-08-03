# Contextual Table Selection

Status: Complete.

Topics: `web-ui-design`, `application-interface-direction`

## Motivation And Outcome

Tactical `055` made torrent multi-selection available through a permanently
visible checkbox column, but it also coupled the primary row and checked set.
That makes an ordinary row tap ambiguous: the detail surface shows one torrent
while toolbar commands appear to target another set, and the store repairs an
empty primary selection back to the first row. Files has no corresponding
selection or action affordance at all.

Replace that interaction with an explicit selection mode. Ordinary row
activation continues to select one torrent and ordinary actions target that
torrent. Long press on touch, Command/Control-click on desktop, keyboard Space,
or a visible Select control enters multi-selection. Only that mode shows check
indicators and makes subsequent row activation toggle membership. Empty table
space can clear ordinary selection, and selection mode always has an explicit
exit.

Apply the same local interaction to Files and expose a More menu containing
Download and Skip choices. Those file actions are deliberately unavailable in
this slice because the application command contract cannot mutate runtime file
selection yet.

## Stable Scenarios

- With no selection mode active, clicking or tapping a torrent row makes it
  current and the existing Start, Pause, Archive/Restore, and Remove policies
  operate on that one torrent.
- Clicking empty table space clears the current torrent instead of selecting a
  fallback row.
- Torrent checkboxes and select-all are absent during ordinary navigation.
- A touch long press, Command/Control-click, keyboard Space, or Select control
  enters a visibly labeled selection mode. The initiating row is checked when
  the entry method identifies a row.
- In selection mode, row taps toggle checks without opening another torrent.
  Select-all is available, and Done or Escape exits and clears the checked set.
- The checked torrent set is shared across Transfers and Workbench while the
  mode is active; the current/detail torrent remains a separate presentation
  concept.
- Files supports the same normal selection and explicit multi-selection
  mechanics inside the currently mounted torrent detail.
- Files exposes a keyboard- and touch-operable More menu. Download and Skip are
  visible but disabled and identify that support is not yet available; no
  receipt, state mutation, or optimistic success is fabricated.

## Dependencies And References

- [`055-application-destinations.md`](055-application-destinations.md) records
  the existing shared torrent selection and command behavior superseded here.
- [`041-live-file-inspection.md`](041-live-file-inspection.md) owns the bounded
  Files projection and explicitly deferred file mutation and actions.
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md) owns touch,
  accessibility, responsive layout, and virtual-table rendering.
- [`../topics/application-interface-direction.md`](../topics/application-interface-direction.md)
  owns shared selection continuity between Transfers and Workbench.

This is presentation interaction, not a BitTorrent protocol or engine feature.
No normative protocol or pinned libtorrent source survey is required. Familiar
mobile long-press selection is an interaction reference rather than a
compatibility claim.

## Scope

- Separate the current torrent from the checked torrent command set and add an
  explicit torrent selection-mode state to the per-application Zustand store.
- Make torrent actions derive their targets unambiguously: the current torrent
  in normal mode and the checked set in selection mode.
- Extend the shared virtual table with explicit mode entry/exit, conditional
  selection geometry, long-press and modifier entry, keyboard behavior, and a
  guarded empty-space action.
- Migrate both Transfers and Workbench torrent tables to the new model.
- Add Files-local current row and checked-set state using the same virtual-table
  interaction.
- Add a Files More menu with truthful disabled Download and Skip actions.
- Update focused unit, component, browser, accessibility, and topic evidence.

## Non-Goals

- No Rust, engine, application-service, generated contract, persistence, or
  transport changes.
- No functional file wanted/skipped mutation, priority scheduling, reveal,
  playback, deletion, or command receipt.
- No new selection persistence across reloads or across different torrents'
  Files tabs.
- No right-click-only context menu, drag selection, or mobile platform shell
  integration.
- No change to multi-torrent removal or mixed Archive/Restore policy.

## Interaction Contract

The table has two modes:

1. **Navigate.** One optional current row may be highlighted. Row activation
   replaces the current row. Enter activates the focused row. Space enters
   selection mode with the focused row checked. Empty-space activation clears
   current selection.
2. **Select.** Zero or more rows may be checked. Row activation and Space
   toggle membership without changing the current/detail row. The selection
   column, select-all control, count, and Done control are visible. Escape,
   Done, or empty-space activation exits and clears the checked set.

Command/Control-click enters Select mode and checks the clicked row. While the
mode is active it toggles like an ordinary row activation. A touch or pen hold
of 500 milliseconds enters Select mode and checks the held row. Moving more
than 10 CSS pixels, pointer cancellation, scrolling, or release before the
deadline cancels the hold. The synthetic click following a successful hold is
consumed so it cannot immediately undo the check.

The visible Select button keeps the behavior discoverable and makes long press
an accelerator rather than the only mobile path. Selection checkboxes are
large enough for coarse input but do not propagate a second row activation.
Selection mode is exposed through grid and control accessibility state, and
disabled Files actions retain both native disabled semantics and a concise
visible reason.

## Ownership, Bounds, And Data Flow

- The Zustand presentation slice owns current torrent, torrent selection mode,
  and the bounded list of checked torrent IDs. Snapshot and removal reduction
  filter IDs that no longer exist but never invent a replacement current row.
- `VirtualTable` owns only transient interaction mechanics. At most one
  long-press timer and one pointer origin exist per mounted table; unmount,
  pointer cancellation, release, or successful completion clears them.
- `TorrentActions` reads the presentation mode and chooses one explicit target
  list before applying existing sequential command behavior.
- `FileTable` owns current file, mode, and checked IDs locally because they are
  presentation state for one leased detail collection. Incoming rows prune
  missing IDs.
- Sorting and virtualization remain bounded by the existing collection and
  visible-row policies. Entering selection mode changes column geometry once;
  scrolling still renders only visible rows plus overscan.
- No new background task, dependency, network request, storage write, or
  application command is introduced.

## Shape-Changing Edge Cases

- Clearing the current torrent remains cleared across unrelated view updates;
  the reducer does not fall back to the first torrent.
- If the current torrent disappears, current/detail selection becomes null. If
  checked torrents disappear, only those IDs are removed.
- Selection mode can contain zero checks while the user is deciding; actions
  are disabled until at least one target exists.
- A normal row activation after selection mode has exited targets only that row
  even if a previous checked set existed.
- A long press cancelled by movement must still allow normal scrolling and
  must not enter selection mode later.
- Empty-state panels and clicks on controls, headers, rows, menus, or scrollbars
  do not masquerade as empty-table activation.
- File rows may report wanted, skipped, or padding semantics, but the disabled
  menu never implies those facts can be changed in this slice.

## Implementation Order And Gates

1. Record this tactical and the explicit mode/state contract.
2. Add store transitions and tests that separate current from checked torrent
   selection, including clear and disappearance repair.
3. Add reusable virtual-table interactions and focused pointer, keyboard,
   modifier, empty-space, and accessibility tests.
4. Migrate torrent tables and commands, then prove ordinary one-row actions and
   explicit multi-row actions use the intended targets.
5. Add Files-local selection and the disabled More menu, with component and
   browser coverage.
6. Update owning topics, complete this evidence record, run the web validation
   baseline, and commit only files belonging to this slice.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Current clear/replace, mode entry/exit, toggle/replace/prune, current disappearance, and no fallback selection |
| Shared table | Conditional check column, Select/Done, modifier entry, long-press success/cancellation, active row toggle, Space/Enter/Escape, select-all, and guarded empty-space clear |
| Torrent components | Normal actions target current; active mode targets checks; Transfers/Workbench continuity; no normal-mode checkbox geometry |
| Files components | Current and multi-selection behavior; More opens by pointer and keyboard; Download/Skip remain visibly and semantically disabled |
| Browser/accessibility | Desktop and phone-sized interaction, keyboard path, virtual scrolling, focus/menu semantics, and no serious or critical automated findings |
| Build | TypeScript check, unit tests, production build, and relevant Playwright project(s) |

Rust workspace, protocol interoperability, public swarms, Android, and physical
device runs are not relevant because this slice cannot change those owners.

## Escalation And Stopping Condition

Ordinary React extraction, internal naming, CSS refinement, test fixture
updates, and fixes at the presentation selection boundary are authorized.
Stop for direction if implementation needs a new application command,
generated-contract change, persistent compatibility policy, a different
multi-torrent destructive-action policy, or a materially different product
interaction.

This tactical is complete when ordinary torrent actions demonstrably target a
single current row, explicit multi-selection works consistently in torrent and
Files tables without persistent checkbox clutter, the Files More menu truthfully
shows unavailable Download and Skip actions, the validation matrix passes, the
owning topics record the result, and no backend file is changed.

## Implementation Record

- The Zustand presentation state now keeps the current torrent, selection-mode
  flag, and checked torrent IDs independent. The first materialized torrent may
  still establish the initial current row, but explicit clearing and torrent
  disappearance remain null across later updates rather than falling back.
- Transfers and Workbench supply the same selection-mode owner to
  `VirtualTable`. Ordinary row activation clears stale batch state and commands
  target the current row; explicit mode preserves current/detail context while
  commands target only checked rows.
- `VirtualTable` conditionally adds its 44-pixel selection column, exposes
  Select/Done and count controls, supports Space, Enter, Escape,
  Command/Control-click, guarded empty-space activation, and a 500 ms touch/pen
  hold cancelled beyond 10 pixels or by scroll/cancellation. The full selection
  cell toggles its check without bubbling a normal row activation.
- Files owns one local current row and checked set, prunes them as its leased
  collection changes, and uses the same table mode. Its new More menu remains
  operable with pointer and keyboard while native-disabled Download and Skip
  download items state why they are unavailable.
- Selected skipped-file text uses the ordinary high-contrast foreground after
  the new row highlight exposed a narrow light-theme contrast failure.
- No Rust, generated API, engine, persistence, transport, dependency, or
  background-task file changed.

## Validation Evidence

The completed slice passed:

```text
git diff --check
cd clients/web && npm run typecheck
cd clients/web && npm test
cd clients/web && npm run build
cd clients/web && RSTORRENT_PLAYWRIGHT_BASE_URL=http://127.0.0.1:4177 npm run test:e2e
```

Vitest passed 108 tests in 21 files, with two unrelated skipped files.
Playwright passed all 15 deterministic demo tests, with the three opt-in live
tests skipped. The browser run covered desktop explicit selection, phone-sized
long press, shared destination state, Files selection and unavailable actions,
wide/compact/phone layouts, and serious/critical axe scans. The 4,096-row Files
fixture retained 665 DOM elements, 52,586,655 sampled JavaScript heap bytes,
and a 41 ms update while rendering fewer than 100 rows.

Functional file wanted/skipped mutation remains the next backend-and-UI slice;
this tactical intentionally stops at truthful disabled affordances.
