# Tactical 110: Atomic Download Now

Status: Complete on 2026-08-07.

Topics: `application-control`, `web-ui-design`

## Motivation And Outcome

The shared Files surface can make a skipped file `Normal`, but a paused torrent
still requires a separate torrent-level Resume action. That exposes an
implementation distinction rather than the user's intent: acquire this file
and make the torrent runnable.

Add one bounded semantic `Download now` operation. It atomically makes the
target non-padding files wanted and sets the torrent's durable run intent to
running in one existing application-state transaction and one profile
revision. The serialized controller introduced by Tactical `108` then
reconciles that intent without a checker-state switch: an active selection is
updated in place, a retained paused checker resumes its existing generation,
or an idle torrent starts through the ordinary admission path.

Expose the operation for skipped targets in both existing Files action menus.
This action is not a new piece priority: `Normal` and `Skip` remain storage and
request policy, while `Download now` is the explicit wanted-plus-running user
intent.

## Stopping Condition

This slice is complete when:

- one typed application command accepts a bounded, sorted, unique nonempty
  file-index list and no path, engine priority, task, or checker phase;
- wanted selection and running intent either commit together at one profile
  revision or do not change at all;
- exact request replay returns the recorded outcome without reapplying an old
  intent, and stale expected revision or invalid state leaves both facts
  unchanged;
- controller convergence uses current durable state and the Tactical `108`
  selection/checker controls without replacing a healthy peer or checker
  generation;
- the shared Files toolbar and row-context menu offer `Download now` whenever
  at least one target is skipped, submit one command, and wait for
  authoritative views rather than optimistically changing rows;
- deterministic store, application, generated-contract, adapter, component,
  and headless browser evidence passes; and
- owning topics, readiness, and this execution record are current.

## Command And Transaction Contract

The additive internal v1 command is:

```text
download_files {
  torrent_id,
  file_indices
}
```

`file_indices` follows the existing `set_file_priority` bounds and canonical
ordering. In one SQLite transaction the store validates the torrent and every
target, computes selection exceptions, changes the targets to wanted, changes
`desired_state` to `running`, clears the ordinary recoverable torrent error,
increments the profile revision at most once, and records the normal request
receipt. No database schema or application contract version changes.

The server is idempotent even though the UI exposes the action only for a
selection containing a skipped file:

| Durable input | Durable output |
| --- | --- |
| skipped, paused | wanted, running |
| skipped, running | wanted, running |
| wanted, paused | wanted, running |
| wanted, running | unchanged successful no-op |

A completed skipped file still receives explicit running intent. Promotion
may need to materialize verified part-file spans, and `Download now` expresses
an unambiguous request to make the torrent runnable; it is not a playback-only
operation. Normal completion and seeding policy decide the resulting runtime
work.

## Ordering And Failure Policy

- Unknown torrents, missing verified metadata, out-of-range indices, padding,
  removal, quarantine/repair, publication mutation, or archived state reject
  the complete command without a revision or partial selection/run change.
- Archive remains a separate product decision; `Download now` does not restore
  an archived torrent implicitly.
- The existing single active-download admission remains in force. If another
  torrent owns that slot, the application returns `busy` before the durable
  mutation; this slice does not add a multi-torrent scheduler or queue.
- A successful durable commit is the intent commit point, not a promise that a
  peer immediately supplies bytes. Later storage, capability, or network
  failure remains visible through ordinary application state and diagnostics.
- Request replay must reconcile only current durable intent. It cannot resume
  a checker after a newer Pause or restore selection overwritten by a newer
  command.
- Selection submitted during checking uses the latest-value fence and resumes
  a retained paused checker only when current durable run intent is running.
  No code enumerates queued, preparing, hashing, reconciling, or finalizing to
  decide the operation.

## Owner And Lifecycle Flow

```text
Files action
  -> one download_files request and request ID
  -> SessionStore transaction
       -> validate every target and torrent state
       -> wanted selection + running intent
       -> one revision + receipt
  -> ApplicationService current-intent reconciliation
       -> active owner: latest selection update + release retained checker
       -> no active owner: ordinary start_if_possible
  -> existing view refresh and Files/torrent projections
```

No new task, channel, timer, storage owner, or queue is introduced.

## Product Reference

The local JSTorrent checkout records the same product expectation in
`packages/client/src/utils/watch-video.ts::prepareTorrentForVideoPlayback`:
permanently unskip the file before starting a stopped incomplete torrent.
`packages/client/test/utils/watch-video.test.ts` fixes that ordering, and
`contracts/io-daemon-conformance.json` names
`watch_video.unskip_and_start_incomplete_torrent` as shared behavior.

RSTorrent intentionally differs in two respects:

- the generic Files action is one durable application command instead of two
  presentation-owned calls; and
- it always records explicit running intent, including for currently complete
  skipped files, because promotion/materialization and later seeding are not a
  playback helper's decision.

Tactical `108` already inspected pinned libtorrent checking, asynchronous file
priority, part-file promotion, and disk-fence behavior at exact commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. This slice adds no engine or
storage transition and relies on that completed dossier and evidence rather
than repeating it.

## Validation

### Store and contract

- skipped/paused, skipped/running, wanted/paused, and wanted/running cases;
- mixed and repeated targets, sorted/unique and resource bounds;
- one revision for a two-fact change and no revision for a true no-op;
- exact replay, request conflict, stale expected revision, and reopen;
- unknown torrent, missing metadata, padding, range, removal, quarantine,
  publication, and archived rejection with no partial mutation;
- unchanged verification request/completion generations; and
- generated Rust/JSON Schema/TypeScript/validator round trip.

### Application runtime

- all-skipped idle content becomes active from one command;
- a partial live selection retains the current peer generation;
- an active check keeps its generation while applying the selection;
- a paused checker resumes the same generation and cursor;
- replay after a newer Pause does not resume old intent;
- a different active torrent returns `busy` without durable mutation; and
- hard storage convergence remains an ordinary diagnosed runtime failure
  after coherent durable intent.

### Shared UI

- toolbar and row-context menus share one action inventory;
- no target, all-wanted, mixed, skipped, pending, demo-unavailable, and stale
  row cases have exact availability and status behavior;
- one click emits one `download_files` command for sorted target indices;
- the UI performs no local selection or torrent-state mutation; and
- a component scenario proves convergence from an authoritative update, and
  headless Chrome proves the skipped action remains accessible in both menus.

Run `cargo fmt --all -- --check`, warning-denying workspace Clippy, complete
workspace tests, generated-contract clean rerun, web tests, typecheck,
production/CSP build, the focused browser case, and `git diff --check`.

## Non-Goals

- Trusting fast resume, reduced checking, or a resume-data policy.
- High, sequential, streaming, deadline, or playback piece priority.
- Multi-torrent queueing, slot preemption, or general scheduling.
- File open/playback, HTTP range serving, publication browsing, or shell
  integration.
- Automatic archive restore, repair, provider reacquisition, or retry policy.
- Android/Compose presentation parity or a stable public remote API promise.
- Changing ordinary `Normal`, `Skip`, Pause, or Resume semantics.

## Implementation Slices

1. Record this accepted tactical and current queue.
2. Add the atomic durable command, application convergence, and deterministic
   Rust evidence.
3. Regenerate adapters and add the shared Files action with deterministic and
   headless-browser evidence.
4. Run layered validation, graduate the tactical and owning topics, and commit
   the completion record.

## Completion Record

Implementation landed in four feature slices:

- `1872619` accepted this tactical and made it the bounded current queue item;
- `09713fd` added `download_files` to the transport-neutral command, generated
  contract, SQLite transaction, and serialized application reconciliation;
- `dea0a9e` added the shared Files action, live and demo adapters, component
  coverage, and deterministic/opt-in browser cases; and
- `8c8e154` directly proved that a different active torrent returns `busy`
  without changing the profile revision, skipped selection, or paused run
  intent.

The store now performs target validation, wanted-selection replacement,
running intent, recoverable-error clearing, revision allocation, and receipt
recording in one existing SQLite transaction. Exact replay returns the stored
result, but application reconciliation reloads current durable intent, so a
newer Pause cannot be undone by replaying an older `download_files` request.
The transaction leaves verification generations unchanged and does not add a
schema or contract-version migration.

The application classifies `download_files` alongside live selection and
single-slot admission commands. Same-torrent work updates the existing
selection fence and retained checker owner; idle work uses ordinary
`start_if_possible`. It does not branch on checker phase or create a new task,
queue, timer, or engine priority.

The React Files toolbar and row-context menu share one action inventory.
`Download now` is present when at least one target is skipped, including a
mixed selection, and sends one sorted `download_files` intent. Rows remain
unchanged after dispatch until the application publishes an authoritative
snapshot. `Normal` and `Skip` remain separate priority actions.

Validation completed:

- `cargo fmt --all -- --check` passed;
- `cargo clippy --workspace --all-targets -- -D warnings` passed after the
  behavior-preserving checker-test iterator cleanup in `ced811a`;
- `cargo test --workspace` passed, including 195 session tests with 2 ignored,
  356 engine tests with 7 ignored, and every other workspace target and doc
  test; the final busy-target regression also passed in isolation after it
  was added;
- `npm run generate` reproduced the checked-in contract with no diff;
- `npm test` passed 234 tests with 2 skipped, `npm run typecheck` passed, and
  `npm run build` passed the production and CSP checks;
- the focused headless-Chrome file-menu/Axe case passed, and the complete
  deterministic inspection suite passed 22/22 after `b0c9801` scoped a stale
  geometry helper to the labelled torrent-detail tablist it measures; and
- `git diff --check` passed throughout.

The opt-in controlled live browser case now uses `Download now` for the final
materialization step, but was not run because its gateway, torrent, and
storage fixture inputs were not present. Trusting fast resume, playback
priority, multi-torrent scheduling, and Android presentation remain the
explicit non-goals above.
