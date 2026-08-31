# Tactical 203: JSTorrent-Shaped Add-Time File Selection

Status: **Active as of 2026-08-31.** User direction selected the JSTorrent
add-time interaction: each ordinary file has one checkbox, checked means
**Normal**, unchecked means **Skip**, and **High** remains a post-add Files
action. This planning commit authorizes bounded end-to-end implementation but
does not authorize a release, production identity change, public swarm, or
store operation.

Topics: `application-control`, `application-view-api`, `client-persistence`,
`client-surfaces`, `web-ui-design`, `android-jstorrent-replacement`,
`capability-readiness`, `download-correctness`

Dependencies: completed live selection Tactical
[`063`](063-live-file-selection.md), completed BEP 53/add-result Tactical
[`100`](100-bep53-select-only-and-duplicate-add-feedback.md), completed
serialized-control Tactical
[`108`](108-serialized-torrent-control-and-observable-checking.md), completed
Android product Tactical
[`117`](117-jstorrent-shaped-android-product-ui.md), completed durable High
priority Tactical
[`176`](176-durable-high-file-priority.md), completed Android external-intake
Tactical
[`197`](197-android-external-torrent-intake.md), and the current schema-23
application profile.

## Product Outcome

New magnet and local `.torrent` adds can stop at one shared, durable file-
selection stage before any content piece is requested or written. The shared
React product and Android Compose present the same simple choice:

- checked file: **Normal**;
- unchecked file: **Skip**;
- **All** and **None** change every ordinary file logically;
- **Download** atomically commits the complete draft and starts selected
  content;
- when no file is checked, the primary action reads **Add** and retains an
  all-skipped, non-transferring torrent; and
- **Cancel** removes only the newly pending add and never deletes payload.

The modal or sheet shows the torrent name, one bounded paged/virtualized list
of ordinary files with path and size, the selected count and byte total, the
current download root, and the number of later pending adds. Padding files are
not user choices and never appear checked or in selected totals. There is no
High state, per-row priority menu, folder tri-state tree, or optimistic content
start in this flow.

A default-on **Show file selection when adding torrents** preference mirrors
JSTorrent. The selection surface offers **Don't show file selection again**;
RSTorrent commits that preference only together with an explicit **Download**
or **Add**, rather than immediately starting every queued item. Downloads
Settings can re-enable it. When disabled, future adds retain the current
all-files/start-immediately behavior.

## Stopping Condition

This tactical is complete only when all of the following are true:

1. A new magnet added with file selection enabled enters durable
   `awaiting_file_selection` application state, acquires verified metadata,
   and performs zero content-piece requests and zero payload writes before
   confirmation.
2. A new local `.torrent` enters the same state with its already available
   metadata and does not need a client-side metainfo parser.
3. React and Compose show checked **Normal** and unchecked **Skip** rows,
   preserve source-derived BEP 53 selection, support logical All/None across
   unmounted pages, and never expose High in the add flow.
4. One atomic application command validates and commits the complete bounded
   Normal/Skip draft, clears pending state, sets running intent, and admits the
   runtime only after the durable commit. No per-file command loop can leave a
   partially applied add.
5. Explicit cancel validates that the torrent is still a new pending add,
   runs through the ordinary joined removal owner without payload deletion,
   and cannot remove an already-present torrent.
6. Plain duplicate adds remain successful no-ops and reveal the existing
   torrent without creating a pending selection or changing its run intent.
   BEP 53 duplicate selection expansion retains Tactical `100` semantics.
7. Pending state, ordering, preference, selected intent, metadata-only
   acquisition, and exact cancel/confirm outcomes survive process/service
   restart. Multiple pending adds are presented in one application-owned FIFO
   order.
8. Concurrent React/Compose presentations race safely: the first valid
   confirm or cancel wins, later stale commands fail as typed stale-state
   outcomes, and neither client maintains a second torrent authority.
9. In-app magnet, local `.torrent`, desktop external activation, Android cold
   and warm external intake, and the ChromeOS companion all converge through
   the same application state and commands without source text, provider URI,
   or path leakage.
10. Deterministic Rust, generated-contract, React, Compose, persistence,
    runtime, and cleanup gates pass, followed by a controlled real transfer on
    the shared desktop product and installed Android evidence. Physical
    ChromeOS proves at least one magnet metadata wait, partial selection,
    process recovery, exact wanted-content completion, skipped-file absence,
    and cleanup.

Passing this tactical resolves the **Add-time file selection** disposition in
`android-jstorrent-replacement`. It does not close production migration,
signing, extension rollout, localization, or network-policy gates.

## Scope

### Shared application contract and persistence

- Add an explicit, default-false request field for entering file selection to
  both semantic magnet and raw-torrent-byte add boundaries. Existing
  automation, iOS, CLI, and integrations retain their current behavior unless
  they opt in.
- Persist one closed pending-add-selection state on the torrent row. Advance
  the next available disposable profile schema (currently schema 24) and
  preserve the established reset policy for recognized older schemas and
  external payload.
- Persist the default-on `show_file_selection` product preference beside the
  existing storage/Add preferences and project it through
  `StorageSettingsSnapshot`.
- Add one atomic confirm command carrying a compact selection base
  (`current`, `all`, or `none`) plus normalized Normal/Skip range overrides.
  The store validates metadata presence, pending ownership, indices, padding,
  range order, bounds, and the final sparse representation before committing
  selection, pending-state removal, running intent, receipt, and revision.
- Add a dedicated cancel-pending-add command. It validates pending ownership
  before entering the existing joined remove-without-payload-deletion path.
- Project pending state, stable FIFO position, ordinary-file totals, current
  selected count/bytes, and the immutable file-catalog identity needed to
  reject stale confirmation. Continue to use the existing paged Files view for
  rows.
- Regenerate TypeScript, JSON Schema/validators, Kotlin UniFFI, and Swift
  UniFFI after boundary changes. iOS compiles and exhaustively reduces the
  additive contract but gains no selection presentation in this tactical.

### Runtime behavior

- Reuse the existing metadata-only add behavior. A pending magnet may own the
  bounded tracker/DHT/peer metadata acquisition generation, but piece-content
  scheduling, storage preparation, payload descriptors, and writes remain
  ineligible.
- Metadata acceptance materializes BEP 53 `so` intent before projection. The
  initial checkboxes therefore show authoritative source selection rather than
  assuming all files.
- An item awaiting user choice after metadata arrives does not qualify as
  active download/seeding work, hold a wake lock, or retain Android background
  ownership by itself. Active metadata acquisition continues to follow the
  existing visible/background lifecycle policy.
- Confirmation starts with the committed selection already installed; it does
  not transiently admit an all-Normal generation. All-skipped confirmation
  retains running intent but remains idle under the existing selection rules.
- Restart reconstructs pending acquisition or waiting-for-user state from the
  one durable application owner. No detached selection task or client-only
  queue is introduced.

### Shared React product

- Turn the existing Add workflow into a staged modal: source/root acceptance,
  then immediate metadata wait/file selection for a new opt-in add.
- When selection is enabled, replace the separate start-content checkbox with
  explicit **Download**/**Add** confirmation. The existing root-options
  preference remains independent: hiding a usable default root choice never
  suppresses the file-selection stage.
- Reuse the Files row vocabulary and byte formatter while keeping the add
  checklist independent from Workbench's current/batch selection model.
- Page at no more than the existing 1,024-row application limit and virtualize
  mounted rows. All/None and summaries operate on the logical catalog, not the
  mounted page.
- Preserve focus containment, explicit Cancel/Download actions, source-control
  focus restoration, keyboard checkbox operation, narrow layout, and Axe
  coverage. Escape or backdrop activation must not silently start or delete a
  pending torrent.
- Apply the same workflow to browser/Tauri manual add, desktop OS activation,
  and the ChromeOS companion. Adapter-specific attachment transport remains
  outside presentation state.

### Android Compose

- Replace the product Add dialog's separate **Start downloading immediately**
  decision with the default-on selection stage. The underlying
  `start_content` test/application capability remains available outside this
  product flow.
- Present the current SAF root and repair requirement without introducing a
  second root registry or generic path access. Root loss disables confirmation
  until the existing root-repair flow succeeds.
- Use a lazy paged list with full-row toggle targets, native checkbox
  semantics, selected count/bytes, All/None, explicit Cancel, and a primary
  **Download** or **Add** action.
- Preserve the selection stage across Activity recreation, picker detours,
  service reconnect, process restart, and Compose/ChromeOS companion
  coexistence. Android Back or sheet dismissal may not silently download all;
  it must remain pending or require explicit cancel confirmation.
- Route cold/warm external magnet and `.torrent` intake through the same stage
  after the existing bounded privacy-preserving source confirmation.

## Non-Goals

- High priority, streaming urgency, Download now, or a priority dropdown in
  the add surface.
- Folder-tree tri-state selection, search, extension filters, automatic media
  selection, or filename heuristics.
- Changing piece/storage integrity, boundary-piece accounting, padding rules,
  live post-add priority semantics, or BEP 53 parsing.
- A client-side metainfo parser in TypeScript or Kotlin.
- Fetching `.torrent` URLs, changing the 64-MiB raw source bound, or retaining
  raw source bytes in presentation state.
- iOS selection UI, headless/CLI prompts, remote API policy, or a new daemon.
- Localization extraction, new download-root semantics, storage free-space
  promises, tracker mutation, VPN/proxy policy, or production migration.
- Publishing, signing, tagging, changing package identity, or altering store
  listings.

## Invariants And Resource Limits

- Checked means exactly durable Normal; unchecked means exactly durable Skip.
  High is neither inferred nor preserved as a hidden add-time state.
- No content block may be requested, accepted, written, verified, or reported
  complete while a torrent awaits file selection.
- Verified metadata and immutable file indices are the authority for every
  checkbox. Padding entries are always non-user content.
- Confirm is one durable all-or-nothing transition. A store failure, stale
  catalog, invalid range, unavailable root, or resource-limit rejection changes
  neither selection nor run intent.
- Cancel can target only an awaiting new add. A duplicate response never gives
  the caller removal authority over the existing torrent.
- Pending ordering is application-owned and stable across presentations and
  restart. At most the existing 500-torrent hard ceiling can be pending.
- File catalogs retain the existing maximum of 374,998 entries and page limit
  of 1,024 rows. No client materializes the complete catalog merely to render
  checkboxes.
- One confirmation carries at most the existing 4,096 normalized selection
  ranges/overrides, and the final persisted sparse exception set retains the
  existing 4,096-entry ceiling. The UI rejects an over-fragmented draft
  explicitly; it never truncates, batches partial commits, or pretends success.
- Draft state is bounded by compact base-plus-ranges representation, not one
  object per logical file. Mounted rows remain bounded by the client viewport
  and page.
- User-controlled names and file paths remain bounded escaped display data and
  appear only where the checklist deliberately presents them. Full magnets,
  provider URIs, raw source bytes, and storage locators remain absent from
  logs, notifications, and error detail.

## Source Study

### Normative and libtorrent oracle

The pinned BEP source is
`reference/bittorrent.org/beps/bep_0053.rst` at
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`. It defines `so` as zero-based
inclusive file-index ranges applied after magnet metadata arrives. RSTorrent
already implements and tests that protocol behavior; this tactical preserves
it as the initial checkbox state.

The pinned libtorrent revision is
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. Relevant source and tests are:

- `src/magnet_uri.cpp`, especially the select-only conversion to
  `default_dont_download` plus explicit wanted file priorities;
- `src/torrent.cpp::file_to_piece_prio`, metadata-time priority application,
  `torrent::prioritize_files`, and `torrent::set_file_priority`;
- `include/libtorrent/torrent_handle.hpp` file-priority documentation,
  including metadata availability, padding, asynchronous storage work, and
  resume persistence;
- `test/test_torrent.cpp` priority-vector and individual-priority cases;
- `test/test_resume.cpp` file-priority resume cases; and
- `simulation/test_transfer.cpp` selective transfer behavior.

The adopted completeness lessons are: selection cannot be interpreted before
the immutable file catalog exists; selected files determine required pieces;
padding is not ordinary selectable content; priorities survive restart; and
priority updates must not be reported complete before their owning transition
finishes. RSTorrent deliberately does not copy libtorrent's asynchronous
handle/storage architecture: its application transaction installs selection
before admitting the content runtime, while completed Tacticals `063`, `108`,
and `176` remain the live-change and integrity authority.

### JSTorrent product reference

The inspected local JSTorrent revision is
`25e4b701433fd815398ba89526546f5e4f072e3f`. The checkout already contains
unrelated untracked maintainer files; this tactical reads it without mutation.
Relevant paths are:

- `android/app/src/main/java/com/jstorrent/app/ui/dialogs/FileSelectionDialog.kt`:
  metadata spinner, root chooser, All/None, checkbox rows, selected count/size,
  explicit Cancel/Download, queued-count copy, and no ordinary sheet dismiss;
- `android/app/src/main/java/com/jstorrent/app/viewmodel/TorrentListViewModel.kt`:
  default pending add, FIFO ordering, checked-to-Normal/unchecked-to-Skip
  application, resume, cancel-to-remove, and preference behavior;
- `android/app/src/main/java/com/jstorrent/app/ui/screens/TorrentListScreen.kt`:
  one current pending sheet and queue wiring;
- `android/app/src/main/java/com/jstorrent/app/settings/SettingsStore.kt` and
  `ui/screens/StorageSettingsScreen.kt`: default-on show-selection preference;
- `packages/ui/src/components/FileSelectionModal.tsx`: desktop checkbox draft,
  All/None, metadata wait, summary, explicit actions, and bounded modal
  presentation;
- `packages/client/src/AppContent.tsx`: pending queue, selection confirmation,
  cancellation, and post-metadata priority application; and
- `packages/client/src/utils/add-torrent-options.ts`: preference-controlled
  entry into `awaitingFileSelection`.

RSTorrent adopts the visible checked/unchecked model, default-on preference,
metadata wait, FIFO, and explicit cancel/confirm outcomes. Intentional
differences are architectural and safety-driven: Rust owns durable pending
state and atomic commands; clients use paged views instead of complete mutable
file arrays; the existing selected root is retained; **Don't show again** does
not immediately resume every queued item; and accidental dismissal never
means Download All.

## Owner, Task, And Dependency Map

```text
source/root confirmation
  -> application add transaction (pending + paused content intent)
      -> existing supervised metadata-only runtime, if magnet
      -> durable pending projection + paged Files view
          -> React modal and/or Compose sheet (bounded local draft only)
              -> one atomic confirm OR validated cancel command
                  -> store receipt/revision first
                  -> ordinary runtime/admission or joined removal reconciliation
```

- The profile store owns pending state, preference, FIFO identity, selection,
  request receipts, and atomic commit/cancel preconditions.
- `ApplicationService` owns metadata-only runtime admission, command
  serialization, runtime reconciliation, and joined removal. It adds no new
  detached task.
- The view model/hub owns transport-neutral pending and file projections.
- React and Compose own only presentation drafts, focus, paging, and explicit
  user intent. Either can disappear without losing authoritative pending state.
- Desktop/Tauri/WebSocket, Android UniFFI, and the Android external-intake
  controller translate source and commands; none owns selection truth.
- Protocol/metainfo and storage-layout modules remain inward, deterministic,
  and unaware of UI, async runtimes, SAF, React, or Compose.

The concrete boundary improvement is one application-owned pending-add state
and one atomic selection-confirm command replacing duplicated client
orchestration and per-file mutation loops.

## Validation Plan

### Deterministic application and persistence

- fresh schema and recognized-old-schema reset with external payload sentinel;
- default preference, disable-on-confirm, re-enable, no-op, replay, malformed,
  future, busy, and injected-commit-failure cases;
- local `.torrent` and magnet pending creation, metadata arrival, restart
  before/after metadata, FIFO ordering, and 500-row resource ceiling;
- all, none, sparse, BEP 53 initial, padding, zero-length, single-file,
  multifile, maximum-index, 4,096-limit, over-fragmented, stale, duplicate,
  repair, unavailable-root, concurrent confirm/cancel, and removal races;
- exact zero content requests/writes before confirm and correct boundary-piece
  integrity after partial selection;
- atomic failure/no-partial-selection and terminal owner/task/resource zero.

### Generated boundaries and clients

- regenerate and diff TypeScript, schema validators, Kotlin, and Swift;
- exhaustive reducer/validator coverage for pending state and commands;
- React component and browser tests for magnet loading, `.torrent`, All/None,
  paging/virtualization, BEP 53, none-selected Add, cancel, duplicate, queued
  items, preference, restart, focus, keyboard, narrow layout, and Axe;
- Compose/JVM/instrumentation tests for the same semantic outcomes plus
  Activity recreation, Back/dismiss safety, SAF root loss/repair, external
  intake privacy, and companion races;
- unchanged iOS generated-boundary compile and existing add regression.

### Controlled runtime and installed evidence

- one controlled multifile `.torrent` and one magnet from incomplete state;
- select a strict subset, prove wanted bytes/hash/completion, skipped-file
  absence, no pre-confirm content traffic, restart, Force recheck, reselect
  after add through the existing Files surface, removal, and exact cleanup;
- shared React live browser/Tauri-adapter coverage without a public swarm;
- API 35 AVD installed Android campaign followed by physical ChromeOS for the
  exact interaction and lifecycle cases in the stopping condition;
- record file/page/draft high waters, process descriptors, platform handles,
  pending requests, peer/runtime owners, payload paths, and cleanup.

### Repository gates

After sourcing the configured profile:

```text
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
npm run e2e --prefix clients/web
cd clients/android && ./gradlew lintDebug testDebugUnitTest \
  assembleDebug assembleDebugAndroidTest
./clients/android/build.sh
```

Run focused connected Android tests and the controlled installed profiles on
owned targets. Do not launch visible desktop clients merely to prove the
application transition. Remove task-owned profiles, payloads, screenshots,
logs, AVDs, packages, reverses, fixture peers, and temporary source files.

## Implementation Sequence

1. Add pure pending-selection values, compact draft validation, atomic store
   transitions, schema/reset coverage, and source-derived initial selection.
2. Add application commands/reconciliation and prove zero content admission,
   restart, duplicates, races, removal, and resource bounds.
3. Add pending/view projections, regenerate every boundary, and make all
   reducers exhaustive.
4. Implement the staged React modal and settings preference with deterministic
   browser/Tauri/external-intake coverage.
5. Implement the Compose sheet and Android external/SAF/lifecycle composition.
6. Run controlled desktop, AVD, and physical ChromeOS evidence; reconcile the
   tactical and owning topics with exact results.

## Escalation Contract

Implementation may choose internal type, column, component, and test names;
refactor the existing Add presentation; add the next disposable schema; and
tighten conservative bounds within the limits above without further direction.

Stop for maintainer direction if evidence requires:

- content transfer before confirmation;
- client-side metainfo parsing or a second pending-torrent owner;
- changing checked/unchecked away from Normal/Skip or exposing High;
- deleting payload on cancel or giving a duplicate caller removal authority;
- exceeding the existing file, selection, torrent, source, or page bounds;
- changing BEP 53, live post-add selection, root, integrity, or background-
  lifecycle policy;
- adding iOS presentation, localization, search/tree heuristics, a dependency,
  or a new service/process; or
- using public swarms, production identifiers, signing keys, store state,
  publication, or unapproved physical devices.

Ordinary implementation, test, AVD, or approved ChromeOS failures remain
within this tactical and should be diagnosed rather than escalated.

## Documentation Completion

Before marking complete:

- record commits, schema, exact reference paths, commands, fixtures, browser,
  AVD/device identities, resource high waters, failures, and cleanup here;
- update `application-control`, `application-view-api`, `client-persistence`,
  `download-correctness`, `web-ui-design`, and `client-surfaces` with landed
  behavior and evidence;
- mark the Add-time file selection disposition implemented in
  `android-jstorrent-replacement`;
- update the Android Compose row and current work sets in
  `capability-readiness`; and
- leave localization, JAR-004/JAR-005/JAR-010, Tactical `199`, VPN/proxy,
  search/plugins, and release work unchanged.
