# Torrent Lifecycle And Retention Actions

Status: Complete.

Topics: `application-control`, `client-persistence`, `application-view-api`,
`web-ui-design`

## Motivation

The live web application can add, pause, and resume torrents but cannot archive
or remove them. Archive exists only in named demo scenarios. The semantic Rust
command contract has no removal operation, and the durable catalog has no
archive or deletion-in-progress state.

Removal is not merely a list mutation. An active engine task and every storage
owner must terminate before managed data can be deleted; SQLite and filesystem
or Android document-provider changes cannot share one transaction; deletion
may fail or be interrupted; and stale platform confirmation must not finalize
a newer torrent generation. This tactical installs that lifecycle owner before
adding the destructive UI control.

## Scope

- Add durable archive and restore commands. Archive is organizational and does
  not implicitly change running intent or delete data.
- Add one semantic removal command with explicit `keep` and `delete_managed`
  data policies. The web dialog defaults to `keep`.
- Persist a bounded removal operation with an opaque generation, stage, and
  bounded error before performing cleanup.
- Quiesce and join active torrent work before logical removal or data deletion.
- For path roots, delete only the torrent's exact hash-named output, staging,
  and part artifacts, accepting absence while preserving the storage root and
  every sibling.
- For platform-capability roots, expose a typed in-process deletion plan. The
  Android adapter resolves its trusted artifact roles beneath the persisted SAF
  tree, closes descriptors, deletes documents, and confirms the matching
  operation generation back to Rust.
- Make interrupted cleanup resumable and idempotent. A failed requested delete
  remains a visible retryable removal failure; it never claims that data was
  deleted.
- Project archive, removal state, and managed-deletion availability through
  torrent summary views and generated TypeScript/Kotlin contracts.
- Add an accessible web confirmation dialog with an unchecked "also delete
  downloaded data" checkbox, explicit irreversible warning when selected,
  focus restoration, keyboard cancellation, and pending/error feedback.
- Preserve the named demo adapter with deterministic archive and removal
  behavior so frontend states remain independently testable.

## Non-goals

- Android Compose removal controls; the provider executor and generated native
  contract are included, but presentation remains behind the web UI.
- A second pause/stop alias, queue management, force start, recheck, relocation,
  per-file deletion, labels, bulk removal, undo, trash integration, or retained
  payload adoption when a removed torrent is later re-added.
- Deleting a configured storage root, accepting client-supplied paths or URIs,
  following arbitrary filesystem links, or deleting files outside exact
  RSTorrent-managed artifacts.
- Stable public remote API compatibility or exposing platform confirmation over
  the loopback HTTP gateway.

## Vocabulary And Contract

- **Pause** remains the existing durable inactive intent and joined engine
  transition. No separate `stop` command is introduced without distinct queue
  semantics.
- **Archive** hides a torrent from ordinary library categories while retaining
  its catalog, payload, progress, and running/paused intent.
- **Remove with keep** terminates engine ownership and removes the catalog while
  leaving all payload artifacts untouched.
- **Remove with delete managed** additionally deletes the exact payload,
  staging, and part artifacts owned by this torrent.
- **Removal operation** is durable application work keyed by torrent identity
  and an opaque generation. It is not a user credential or browser-supplied
  storage plan.

The user-facing command carries only torrent identity and data policy. Platform
plans and confirmations are trusted in-process adapter operations and never
accept ambient paths, document URIs, or descriptor numbers from browser input.

## Reference Review

Pinned libtorrent `2.0.13` revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/session_handle.hpp` documents synchronous session
  detachment, asynchronous owner release, stopped tracker participation,
  `delete_files`, and the internal `delete_partfile` flag.
- `src/session_impl.cpp::remove_torrent_impl()` removes session ownership before
  aborting the torrent and delegates optional deletion through the disk owner.
- `src/torrent.cpp::delete_files()` disconnects peers, stops announcing, and
  fences asynchronous disk deletion.
- `src/storage_utils.cpp::delete_files()` treats missing files as success,
  removes known content, deletes subdirectories bottom-up, and includes the part
  file when content deletion is requested.
- `test/test_remove_torrent.cpp` covers complete, partial, mid-download, double
  removal, content deletion, part deletion, and auto-managed removal races.

RSTorrent adopts the lifecycle, fencing, idempotence, and adversarial cases but
does not expose libtorrent's part-file-only implementation flag.

Local JSTorrent `main` revision
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/engine/src/core/bt-engine.ts::removeTorrent()` immediately stops
  network work, updates queue ownership, clears persistence, destroys the
  torrent, and emits removal.
- `removeTorrentWithData()` first closes network and storage owners, then
  deletes content bottom-up and the part file while retaining bounded errors.
- `packages/client/src/AppContent.tsx` currently exposes separate Remove,
  Delete Files, and Remove All Data actions. RSTorrent deliberately uses one
  safer modal with an unchecked retention checkbox.

No source or fixture is copied.

## Durable State And Lifecycle

```text
present
  -> removal requested (durable generation + policy, desired paused)
  -> engine quiesced and joined
  -> path cleanup | awaiting platform cleanup | keep-data finalization
  -> catalog removed

cleanup error -> removal failed -> explicit retry or keep-data finalization
```

Startup resumes every nonterminal removal before restoring ordinary running
torrents. Missing artifacts are successful idempotent cleanup. A crash before
cleanup, midway through cleanup, or after SAF deletion but before confirmation
therefore converges without treating absent data as corruption.

The torrent row remains the foreign-key authority until cleanup succeeds. The
final transaction removes it and all dependent state, advances the profile
revision, and produces the ordinary keyed view removal. A stale operation
generation cannot confirm or fail another operation.

## Owner And Cancellation Map

- `SessionStore` owns archive state and the durable removal record.
- `ApplicationService` owns the transition from user intent through active-task
  cancellation/join and chooses the path or platform-capability executor.
- Path cleanup executes inside the application operation and targets only
  resolved configured-root children derived from validated torrent identity.
- The Android foreground service owns SAF provider calls. Rust owns the typed
  plan and validates confirmation generation; Kotlin owns URIs, grants,
  provider traversal, and document deletion.
- View sets observe durable stages and final row removal. They do not own or
  infer cleanup from logs.

No detached deletion task is allowed. Shutdown joins or leaves a durable stage
that startup can resume.

## Required Evidence

- Schema creation and migration preserve existing torrents with `archived =
  false` and no removal operation.
- Archive/restore is durable, idempotent, orthogonal to pause, and produces
  exact summary patches.
- Keep-data removal joins an active task, removes catalog state, preserves
  output/staging/part bytes, and replays its request receipt.
- Delete-managed removal covers absent, partial, complete, active, repeated,
  interrupted, and failed path cleanup while preserving roots and siblings.
- SAF planning covers no-metadata, staging, prepared, and published states;
  confirmation rejects wrong torrent, operation, root, and stale generation.
- Android provider tests cover present/missing artifacts, revoked permission,
  repeat deletion, failure reporting, and confirmation after recreation.
- Web reducer/component/browser tests cover archive categories, removal patches,
  unchecked default, checked warning, Escape/cancel/focus restoration, command
  failure, and responsive layouts without launching Tauri.
- Existing Rust workspace, TypeScript, gateway, desktop, and Android builds and
  tests remain green.

## Stopping Condition

The slice is complete when a live web user can archive, restore, remove while
keeping data, or remove and delete managed path data; the same durable
delete-managed operation can be executed and confirmed through Android SAF;
every owner and crash boundary above has deterministic evidence; living topics
record the accepted semantics; all proportionate gates pass; and the work is
committed with a clean tree.

## Result And Evidence

The semantic contract now contains archive, restore, and one removal command
with explicit `keep` or `delete_managed` policy. Schema version `4` persists
archive plus a bounded removal generation and stage. The application service
persists paused removal intent, joins an active torrent owner, awaits a bounded
blocking path cleanup, or publishes an in-process platform plan. Successful
finalization deletes the catalog row and produces the ordinary keyed view
removal; failed cleanup remains a visible retryable upsert.

Rust tests cover version-one and version-three migrations, archive
idempotence, request receipt replay, keep-data preservation, exact path
artifact deletion with sibling retention, active cancellation/join, missing
artifacts, cleanup failure and a new retry generation, pending-work restart,
stale platform confirmation, and platform-plan restart. Kotlin tests cover the
three trusted SAF roles, missing and repeated documents, and provider refusal.
The Android build generated both UniFFI namespaces, cross-built `x86_64` and
`arm64-v8a`, compiled the provider executor, assembled the APK, and passed JVM
tests. Existing controlled SAF grant-loss, publication, and recreation
coverage remains the provider/lifecycle foundation; no Android presentation
control was added in this tactical.

The React/CSS Modules dialog defaults to retention, warns only for permanent
managed-data deletion, traps focus, supports Escape, restores focus, retains
command failures, and allows a failed durable removal to be retried. Vitest
exercises live command mapping, reducer removal, demo behavior, dialog
defaults, focus, and error handling. Headless Chrome exercised the dialog,
axe serious/critical checks, wide/compact/phone layouts, and virtualized scale;
the opt-in controlled-live Playwright case remained skipped because no live
fixture was requested for this storage-lifecycle slice.

Validation completed with:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`;
- `cargo test --workspace` and focused 53-test session runs;
- web contract regeneration, `npm test`, `npm run typecheck`, and
  `npm run build`;
- Playwright: five passed, one opt-in live test skipped, with no serious or
  critical axe findings; and
- the full Android two-ABI build plus `testDebugUnitTest`.

### Post-completion compatibility correction

The first persistent-profile use after this tactical exposed two upgrade and
reload defects. Pre-retention request receipts could not deserialize because
their embedded torrent snapshots lacked the new `archived` and managed-data
capability fields. Those fields now use conservative receipt defaults: not
archived and managed deletion unavailable. A focused database regression
removes both fields from a real stored response and proves replay after reopen.

The React live adapter also restarted its durable request IDs at `web-1` for
every application instance. Reloading the UI could therefore reuse an old
receipt ID for a different menu or toolbar command and correctly trigger a
request conflict. Each live application now owns a random 128-bit request
namespace plus its monotonic sequence. TypeScript tests prove bounded,
sequential IDs within one instance and distinct IDs across instances.

Post-correction validation passed formatting, warning-denying workspace
Clippy, all workspace tests, regenerated web contract/schema output, the full
44-test Vitest suite with two opt-in cases skipped, TypeScript checking, and
the production web build. The first workspace run shared the machine with the
web gates and one unrelated storage-pressure timing test missed its deadline;
that case passed three isolated reruns and the complete serial workspace rerun.
