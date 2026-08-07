# Tactical 108: Serialized Torrent Control And Observable Checking

Status: Complete on 2026-08-07.

Topics: `application-control`, `application-view-api`, `client-persistence`,
`download-correctness`, `storage-throughput-architecture`, `web-ui-design`

## Motivation And Outcome

Tactical `063` deliberately implemented live file selection through a coarse
but safe boundary: commit selection intent, cancel and join the complete active
content generation, reopen storage, conservatively recheck, and start a
replacement generation. It recorded a per-storage fence and mutable picker as
later work if the cost became material.

The Files surface now needs a single future `Download now` action which makes a
skipped file wanted and starts its torrent when necessary. Investigation of
that apparently small action exposed that the coarse boundary is no longer
sustainable:

- selection currently determines the set of pieces included in full checking,
  so changing download policy changes the meaning of integrity evidence;
- every effective selection change tears down peer, checker, storage, and
  scheduler ownership even when only one storage route and picker policy need
  to change;
- missing or short recheck sources and on-disk mismatches enter the same
  visible `Failed` disk-piece stage used for actual operation failures;
- a pending durable verification request says only `checking`; it does not
  reveal whether work is queued, opening storage, hashing, draining, stalled,
  or committing, and the shared UI can remain at an unexplained zero; and
- conservative full checking on every admitted generation prevents a future
  fast-resume policy from being introduced as a validation choice rather than
  another lifecycle special case.

Replace that shape with one serialized per-torrent control authority, one
bounded per-torrent storage-operation fence, integrity evidence independent of
file selection, and a generation-scoped checker progress projection. Checking
remains conservative and exclusive against peer and publication activity in
this slice. Selection changes are accepted and coalesced while checking,
applied at the storage fence, and do not restart or redefine the full check.

This tactical establishes the engine/session boundary that makes a later
`Download now` command routine. It does not add that action yet.

## Stopping Condition

This slice stops only when all of the following are true:

- one serialized controller owns semantic run, selection, verification, and
  terminal-operation reconciliation for a torrent; background completions
  cannot independently drive contradictory lifecycle transitions;
- file-priority storage mutations execute through a bounded exclusive fence
  which drains admitted work, applies or rolls back exact routing changes, and
  releases later work against the resulting storage epoch;
- a full checker inventories every physically readable logical piece
  independently of current wanted/skipped selection and produces exact
  verified, absent, mismatched, and hard-error outcomes;
- ordinary file selection no longer requests a new full-verification
  generation or replaces the complete engine generation;
- priority changes during checking preserve one continuous check generation,
  coalesce rapid updates to the latest durable revision, and leave networking
  stopped until checking reaches a terminal result;
- absence and ordinary on-disk mismatch are not represented as transfer hash
  failure, peer fault, or a generic failed storage operation;
- startup and force recheck retain the conservative full-check policy, while
  the admission decision is separated cleanly enough for a later explicit
  trusting fast-resume option without changing controller, storage-fence,
  checker-progress, or UI contracts;
- the application view distinguishes queued, preparing, hashing,
  storage-reconciling, paused, finalizing, completed, cancelled, and failed
  checker behavior and exposes monotonic exact counters plus bounded liveness;
- the shared React library, transfer table, and selected-torrent summary show
  determinate checking progress when available and truthful indeterminate
  phase text otherwise, without presenting old durable have state as current;
- pause, force-recheck replay, selection thrash, cancellation, shutdown,
  removal, publication exclusion, hard I/O, crash, stale completion, path
  storage, and platform-capability cases satisfy the scenarios below; and
- deterministic engine/session/frontend tests and a controlled pinned-
  libtorrent comparison pass with recorded task, queue, memory, handle, and
  progress-delivery high-water marks.

A working `Download now` menu item alone does not satisfy this tactical.

## Stable Scenarios

| Scenario | Required outcome |
| --- | --- |
| T108-C01 selection during check | Changing one or many file priorities while a full check is hashing does not cancel, restart, or renumber that check. Later hash jobs see the fenced storage route; the final integrity bitmap is valid independently of the final selection. |
| T108-C02 promotion while check is queued | A skipped-to-wanted change is durably accepted while checking awaits admission. The queued check uses the latest storage route and still scans every readable piece. |
| T108-C03 demotion while check is active | A wanted-to-skipped change retains an existing destination under the current non-destructive policy, updates request eligibility, and does not discard already established integrity evidence. |
| T108-C04 rapid priority changes | At least 1,000 deterministic rapid updates across multiple files settle to the last durable priority vector. There is no unbounded action backlog, repeated torrent restart, or stale route publication. |
| T108-C05 all skipped and later wanted | All-skipped content remains idle while preserving running intent. A later promotion becomes runnable without a full torrent recheck caused solely by selection. |
| T108-C06 boundary materialization | Skipped-to-wanted promotion exports exact verified part-file spans, synchronizes every durability target, and commits the new route epoch before later jobs use it. Missing or uncertain spans clear only affected integrity evidence. |
| T108-C07 integrity vocabulary | A missing source is `absent`, an on-disk hash mismatch is `mismatched`, an inaccessible source is a typed storage error, and a newly received bad piece is a transfer hash failure with the existing bounded contributor treatment. The four outcomes do not share counters or peer consequences. |
| T108-C08 exclusive force recheck | Force recheck stops announcements, discovery, incoming seeding, peers, download/upload, publication, and competing storage mutations before hashing. Selection intent remains mutable through the controller and its storage fence. |
| T108-C09 pause and resume checking | Pause closes new hash admission and drains admitted jobs. The pending verification request and in-process candidate/cursor remain coherent; resume continues without duplicating admitted work. Process death may restart the conservative check from the beginning. |
| T108-C10 force-recheck replay | Replaying the same successful request or requesting force recheck while the same generation is queued/active does not create a second pass. A later distinct request after completion can create a new generation. |
| T108-C11 removal and shutdown | Removal is terminal and shutdown is bounded: controller admission closes, checker and storage jobs join, stale results are rejected, and only exact owned artifacts are eligible for deletion. |
| T108-C12 storage and publication exclusion | Priority routing cannot overlap publication, removal, or repair namespace mutation. A hard route/materialization error rolls back or retains the previous coherent route and preserves durable selection intent for diagnosis/retry. |
| T108-C13 exact progress | Once the piece total is known, `pieces_processed` is monotonic within one check generation, never exceeds total, and equals matched plus absent plus mismatched. Completion is reported only after final storage reconciliation and the generation-matched bitmap transaction. |
| T108-C14 non-hashing phases | Queued, preparing, storage-reconciling, paused, and finalizing checks are shown as distinct indeterminate phases rather than `0% checking`. |
| T108-C15 slow hash liveness | A deliberately delayed hash emits bounded heartbeat snapshots showing an active job and increasing oldest-job/elapsed age even while the percentage is unchanged. A structurally unrunnable checker cannot remain labeled active. |
| T108-C16 crash and stale completion | Death before request commit, while queued, during hashing, during a priority fence, after hashes before bitmap commit, and after commit before runtime reconciliation yields one conservative restart path. Results carrying an old verification or storage epoch cannot mutate current evidence. |
| T108-C17 conservative resume | Every admitted restart in this slice performs full selection-independent validation before peer activity. Previously verified but currently skipped readable pieces can be recovered; missing skipped data remains simply absent. |
| T108-C18 platform capability | Fixed descriptor manifests retain existing fail-closed dynamic-selection behavior, but controller events, check progress, epoch fencing, cancellation, and stale-result rejection are backing-neutral. No path-only assumption leaks into the control or integrity layer. |

## Normative And Reference Dossier

No BitTorrent specification defines a file-priority API, part-file layout,
resume trust policy, checker queue, or product progress presentation. BEP 3 at
`reference/bittorrent.org/beps/bep_0003.rst` remains authoritative for v1
piece hashes over the concatenated logical file space. BEP 47 at
`reference/bittorrent.org/beps/bep_0047.rst` remains authoritative for
synthetic padding. Selection, storage routing, validation admission, and UI
progress are client policy constrained by those logical bytes and hashes.

### Pinned libtorrent oracle

The required oracle is libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`, pinned in
`reference/pins.toml`.

- `src/torrent.cpp::{force_recheck,on_force_recheck,start_checking,
  on_piece_hashed,should_check_files,pause,do_resume}` disconnects peers and
  stops announcements for force recheck, enters the checking queue, bounds
  outstanding hashes, stops new admission when paused, lets admitted work
  complete, and resumes checking before peer activity.
- `src/session_impl.cpp::auto_manage_checking_torrents` schedules checking
  separately from ordinary active torrent management.
- `src/torrent.cpp::{on_file_priority,prioritize_files,set_file_priority,
  update_piece_priorities}` applies file priority asynchronously, defers
  updates while a prior storage operation is outstanding, coalesces the latest
  per-file values, and updates picker priorities after storage completion.
- `include/libtorrent/aux_/disk_job_fence.hpp` and
  `src/disk_job_fence.cpp` implement the per-storage exclusive fence: block
  later jobs, wait for outstanding jobs, run the fenced job, then release the
  backlog.
- `src/mmap_disk_io.cpp::async_set_file_priority` submits priority mutation as
  a fence job.
- `src/mmap_storage.cpp::set_file_priority` and
  `src/posix_storage.cpp::set_file_priority` export part-file data during
  skipped-to-wanted promotion, retain existing destinations on demotion, flush
  part metadata, and return the exact applied priority vector or an error.
- `test/test_checking.cpp::discrete_checking` changes the wanted file while
  checking and requires the check to continue uninterrupted to seeding.
- `test/test_checking.cpp::preserve_file_priorities` establishes that the full
  checker can mark priority-zero file pieces present while selection remains
  a separate picker policy.
- `test/test_priority.cpp::{file_priority_multiple_calls,
  file_priority_stress_test}` covers asynchronous settling and 1,000 rapid
  updates; `test/test_torrent.cpp::test_running_torrent` covers quick
  select/unselect changes on a running torrent.
- `src/posix_disk_io.cpp::async_check_files` and
  `src/storage_utils.cpp::verify_resume_data` separate cheap resume-data
  validation from full hashing. The metadata check primarily proves expected
  file presence/size for claimed pieces and intentionally trusts more than
  RSTorrent does in this slice.

RSTorrent adopts the serialization, storage-fence, checker/selection
independence, coalescing, and checking-observability lessons. It does not copy
libtorrent's object model, main-thread topology, disk-job classes, resume
format, numeric priorities, timestamp/size trust policy, or automatic torrent
queue policy.

### JSTorrent product history

The local JSTorrent sibling was inspected at
`9895410beeed6aff554053769bd006a3fbd373ef`.

- The legacy `archive/legacy-app/js/torrent.js::setFilePriority` and
  `piece.js::markAsIncomplete` explicitly clear boundary-piece completion and
  redownload it on unskip. That was a simplifying workaround, not behavior to
  preserve.
- Current `packages/engine/src/core/file-priority-manager.ts` keeps piece
  request classification separate from its bitfield.
- Current `packages/engine/src/core/torrent.ts::{setFilePriorityAsync,
  setFilePrioritiesAsync,withFilePriorityUpdateLock,
  syncPartsPiecesToCurrentPriorities}` serializes priority materialization but
  retains fire-and-forget synchronous variants. RSTorrent requires one
  semantic completion boundary rather than two observable strengths.
- `torrent.ts::{verifyResumeData,_doCheckPieces,recheckData}` implements a
  metadata-trusting fast path, complete checker, and network suspension.
- `torrent-queue-manager.ts` maintains a separate bounded checking queue.

JSTorrent supplies product failure history and useful cases. Its mutable
object graph, JavaScript promise lock, FFI filesystem shape, and resume trust
decisions are not an architecture template.

## Existing RSTorrent Boundary

- `SessionStore` persists run intent, sparse file selection, one payload fact,
  have evidence, and requested/completed verification generations. Tactical
  `105` correctly makes checking runtime-derived and generation-fences full
  bitmap replacement.
- `set_file_priority_indices` currently advances `verification_requested`
  when a running torrent's selection changes and no check is already pending.
  Selection and verification admission are therefore coupled in durable
  policy.
- `ApplicationService::dispatch` stops discovery/incoming activity, commits
  selection, then calls the broad pause/join path and starts a replacement
  generation.
- `run_selective_download` constructs immutable `wanted_pieces` and uses that
  list for recheck inventory. `full_recheck_managed_storage` clears its result
  bitmap and hashes only those candidates.
- Recheck absence, missing/short preparation, ordinary mismatch, and hard
  operation failures call `DownloadControl::disk_piece_failed`, which sets
  `DiskPieceStage::Failed`.
- Background checkpoint callbacks can perform generation-fenced store updates
  while application command reconciliation is elsewhere. Transaction tokens
  prevent known stale writes, but there is no single controller which orders
  command intent, storage epochs, and runtime completion.
- `TorrentView` exposes durable `verified_piece_count` and a coarse
  `ProgressAssessment`. While verification is pending the durable snapshot
  deliberately projects verified count as zero. The live adapter derives its
  only torrent progress ratio from that value, and the UI renders only
  `Checking downloaded content`.
- Disk views can expose individual hashing activity, but they are not a
  generation-level checker contract and cannot distinguish queued, preparing,
  finalizing, or a checker with no remaining progress path.

The current behavior is conservative but makes selection policy, physical
routing, integrity authority, task lifetime, and presentation mutually
dependent. This tactical changes that boundary rather than adding more
branches to it.

## Accepted Architecture

### Orthogonal facts, not a cross-product state machine

Retain durable facts which survive process death:

- run intent;
- selection plus its durable revision;
- payload ownership;
- authoritative verified-piece bitmap;
- requested/completed full-verification generation;
- archive/removal intent; and
- quarantine or bounded operation failure when needed.

Do not persist `queued`, `preparing`, `hashing`, `reconciling`, or `finalizing`
as torrent lifecycle state. They are observations of current runtime owners.
Do not add a combined enum containing every run, selection, check, storage,
publication, and error permutation.

The application-facing `TorrentState::Checking` may remain the broad derived
state while a verification generation is pending. The separate checker view
explains what, if anything, currently owns progress.

### Serialized torrent controller

Introduce one serialized control authority per torrent. This need not mean a
new task or actor per stored torrent: it may be explicit controller state
owned by the existing application service. It must nevertheless provide one
ordering boundary for:

- semantic command intent and receipt results;
- latest durable selection revision;
- verification request admission;
- active checker generation and cancellation;
- current storage epoch and priority-fence completion;
- peer runtime admission/termination;
- publication, repair, removal, and shutdown exclusion; and
- generation-stamped background terminal events.

Background hashing, storage, checkpoint, and peer tasks perform bounded work
and emit typed observations. They do not independently choose the next
lifecycle operation. Durable piece batches may retain their dedicated
throughput owner, but they can update only evidence for the admitted integrity
epoch and cannot change run, selection, payload, or operation facts.

The controller is a reconciler, not an unbounded FIFO of closures. Replaceable
intent is stored once:

```text
latest run intent
latest selection revision and bounded sparse selection
at most one pending full-verification generation
at most one active exclusive operation
terminal removal/shutdown intent
```

If selection changes while a priority fence is running, retain only the latest
newer revision and reconcile once more after the current fence terminates.
Every successful command receipt still describes durable acceptance; checker
and storage progress describe later convergence.

### Per-torrent storage-operation fence

Add a fence at the layer which owns one torrent's logical storage routes. It
must be usable by path and platform backings without moving filesystem,
provider, channel, or task types into protocol/layout state.

An exclusive storage operation:

1. closes admission of later read, write, hash, checkpoint-target, and route
   work for that torrent;
2. awaits every previously admitted operation and associated durability work;
3. computes a transition from one immutable route epoch to the next;
4. performs materialization/export and required syncs under the old/new route
   contract;
5. either publishes the complete new epoch or retains/rolls back to the prior
   coherent epoch; and
6. releases bounded queued work against the resulting epoch.

Publication, removal, repair namespace mutation, and priority routing are
exclusive operations. Ordinary piece reads/writes/hashes remain bounded shared
operations. A priority mutation may temporarily pause checker admission while
the fence drains; it does not end the check generation.

Each prepared storage job carries the route epoch it was derived from. A stale
job or result is rejected before changing integrity evidence. The fence may
reuse existing storage execution queues and permits; it must not introduce a
second unbounded job queue.

### Selection-independent integrity evidence

The durable have bitmap answers only:

> Which logical torrent pieces currently have complete, hash-verified bytes in
> the owned storage topology?

It does not answer whether a piece is wanted. File selection and piece
priority derive request eligibility and completion from that independent
bitmap.

A full check scans every logical piece with a physically readable complete
source plan, including currently skipped files, retained destinations, and
part-file slots. A missing source resolves the affected piece as absent without
attempting to create or extend it. Padding remains synthetic. Hard permission,
capability, seek, and I/O failures retain their existing typed fail-closed
behavior.

Use a check-specific result vocabulary equivalent to:

```text
verified
absent
mismatched
unreadable(error)
```

`false` in the final bitmap means no current verified evidence; it does not
need a durable per-piece failure enum. Runtime checker counters retain the
reason for the current operation. `PieceHashFailed` and peer contributor
penalties remain reserved for complete payload assembled from peer responses
which fails its expected hash. Ordinary absence/mismatch during inventory must
not increment those counters or create a retrying transfer-stage failure row.

### File-priority reconciliation

A durable selection mutation no longer advances the full-verification
generation merely because the torrent has running intent. The controller
recomputes wanted piece priority and submits a priority fence when physical
routing must change.

- Wanted-to-skipped retains an existing destination and its verified evidence.
  New writes route according to the existing lazy part-file policy.
- Skipped-to-wanted exports exact verified part spans and synchronizes the
  destination before publishing the new route epoch.
- If every required byte is already covered by current verified logical-piece
  evidence and the fence preserves those bytes exactly, the have bit remains.
- If a newly exposed retained destination or part span has uncertain
  provenance, perform a bounded targeted check of affected pieces under the
  same checker/storage primitives. A non-match clears only those pieces; it is
  not a new whole-torrent verification generation.
- If materialization fails, no partial route becomes current. Durable selection
  remains accepted, the previous coherent route remains inspectable, and the
  controller exposes a bounded storage reconciliation failure.

The request scheduler observes the new piece priorities only after any
required routing fence completes. Newly unwanted outstanding requests are
cancelled or allowed to drain according to existing exact block ownership, but
no new unwanted request is admitted.

### Checking lifecycle and exclusivity

Force recheck and conservative startup checking remain exclusive against
discovery, tracker/DHT announcement, incoming seeding, peers, upload,
download, publication, and repair. This preserves Tactical `105` containment
and deliberately exercises the new controller, storage fence, and checker
paths before a trusting resume policy can bypass them.

Checking itself is a bounded operation:

- release/reopen current owned storage handles before inventory when the
  existing recheck contract requires fresh identity observation;
- admit no more hash jobs or bytes than the current storage/hash limits;
- stop admitting hashes on pause, removal, shutdown, hard error, or a raised
  storage fence;
- let already admitted jobs terminate and account for their exact generation
  and route epoch;
- retain the in-memory candidate bitmap and cursor across ordinary in-process
  pause/resume when ownership remains valid;
- on process death, discard runtime cursor/candidate state and repeat the
  conservative generation from current owned artifacts; and
- atomically replace durable have evidence only after every hash and recovered
  materialization/sync job joins and the exact verification generation still
  matches.

Selection updates may enter their storage fence between bounded hash batches.
Because the checker is selection-independent and the fence preserves or
invalidates affected logical evidence explicitly, the generation continues.

### Conservative policy and future trusting fast resume

This tactical implements only the conservative policy:

- ordinary admitted startup performs a complete selection-independent check;
- explicit force recheck always performs a complete check;
- selection change alone performs no complete check; and
- route transitions validate only affected uncertain pieces.

Separate the pure admission decision from checker execution so a later
explicit user option can select a policy equivalent to `Trust resume metadata`
without changing lifecycle ownership. That later policy must define, test, and
communicate what it trusts for engine-private staging, part storage,
user-visible published paths, and dynamic providers. File size/timestamp or
provider-document metadata is not cryptographic integrity and must be described
as a speed/trust tradeoff rather than verified content.

The future option must default to conservative for fresh profiles unless a
later product decision changes that policy. Force recheck remains full under
either setting. This tactical adds no setting, schema field, generated setting
contract, or optimistic resume behavior.

## Checker Progress And Liveness Contract

### Runtime snapshot

Add one optional generation-scoped checker summary to the torrent application
view. Exact internal names may change, but it is semantically equivalent to:

```text
CheckingProgress {
    generation
    phase: queued | preparing | hashing | reconciling_storage |
           paused | finalizing
    pieces_total
    pieces_processed
    pieces_matched
    pieces_absent
    pieces_mismatched
    bytes_hashed
    active_hash_jobs
    queued_hash_jobs
    elapsed_millis
    last_advance_age_millis
    oldest_active_job_age_millis?
}
```

The snapshot is present while a full-verification generation is pending and
must distinguish durable pending work with no admitted checker from an active
owner. If recovery has not yet reconstructed the queued owner, progress is
`queued`/waiting, not fabricated active hashing.

Counter invariants:

- `pieces_total` is the metainfo piece count for a full check and is independent
  of selection;
- processed equals matched plus absent plus mismatched;
- every counter is monotonic within one generation and bounded by total/logical
  payload geometry;
- missing file ranges may advance absent/processed without issuing one failed
  event per piece;
- bytes hashed counts actual logical payload bytes submitted to successful
  hash computation, not missing spans or synthetic padding;
- phase may move temporarily from hashing to storage reconciliation and back,
  but counters never reset until a distinct generation;
- finalizing includes candidate reconciliation, required sync, and atomic
  completion; 100% processed is not the same as durably complete; and
- terminal completion removes the checker snapshot only after the ordinary
  torrent state and authoritative have projection are ready in the same view
  update or a recoverable reset sequence.

### Bounded delivery and structural stall diagnosis

Hash jobs may emit detailed internal activity, but the torrent summary is
coalesced to no more than one non-terminal update per second. Phase changes,
errors, cancellation, and completion may deliver immediately. While an active
hash or storage-reconciliation job makes no counter progress, one cancellable
heartbeat per active checker updates elapsed, last-advance, and oldest-active
ages at no more than one hertz. It creates no diagnostic row per heartbeat.

Do not infer corruption or failure from elapsed wall time alone. Instead:

- a slow active job remains visibly active with an increasing age;
- queued work is waiting rather than active;
- paused work is explicitly paused;
- a hashing owner with remaining pieces, zero active jobs, zero queued jobs,
  and no raised fence has no structural progress path and must reconcile in the
  same control turn or emit one bounded diagnostic and transition to a typed
  waiting/error outcome; and
- hard I/O/capability failure leaves `checking` only through the established
  repair/error derivation and never spins a heartbeat forever.

The heartbeat owner is the checker supervisor, has one cancellation token and
join path, and is absent when no checker is active.

### Shared React presentation

Download completion and checker progress remain distinct model values.
`TorrentRow.progress` continues to mean authoritative content/download
completion. Add an optional checking model derived from the checker summary.

While status is checking:

- the Transfers/Workbench `Done` cell switches its visible label and progress
  bar to `Checked N%` using processed/total, with an accessible checking label;
- Library availability copy includes the determinate percentage when hashing,
  otherwise truthful text such as `Queued for checking`, `Preparing check`,
  `Updating file selection`, `Checking paused`, or `Finalizing check`;
- the selected torrent summary exposes exact processed/total plus matched,
  absent, and mismatched counts and bounded liveness text such as the age of an
  active job or last advance;
- queued/preparing/reconciling/finalizing phases use an indeterminate treatment
  and never render a false `0%` bar as evidence of no activity;
- a stale or disconnected view retains the existing stale/offline treatment
  rather than advancing a client-side timer as if it were live; and
- completion returns to authoritative download progress without a transient
  display of the old pre-check have total.

Sorting while checking uses checker processed ratio for rows currently
checking and ordinary download progress otherwise, with a deterministic null
ordering. No React interval reconstructs checker state; the application view is
authoritative.

## Owner, Task, Cancellation, And Dependency Map

```text
semantic command / durable recovery
  -> serialized torrent controller
       -> durable fact transaction
       -> reconciliation
            -> peer runtime admission or stop
            -> checker admission / pause / cancel
            -> exclusive storage fence
            -> publication / repair / removal exclusion

checker supervisor
  -> immutable metainfo and logical layout
  -> bounded source inventory and hash jobs through storage fence
  -> generation + route-epoch stamped results
  -> candidate bitmap and O(1) progress summary
  -> controller terminal event

storage fence
  -> shared read/write/hash admission
  -> exclusive route/publication/removal job
  -> immutable next route epoch or retained prior epoch

SessionStore
  -> durable intent, ownership, verification request, authoritative bitmap
  -X-> task handles, hash queues, checker phase, heartbeat, open descriptors

ViewHub -> generated contracts -> live adapter -> shared React UI
```

The protocol/layout layer owns deterministic piece/file geometry and has no
runtime, store, UI, or filesystem dependency. The engine owns storage plans,
hashing, the fence, and checker snapshots. The session/application layer owns
durable intent, the serialized controller, reconciliation, and view mapping.
Platform adapters own capabilities/descriptors only. React owns presentation
only.

Every controller event, checker result, fence completion, and checkpoint batch
which can mutate current state carries the minimum matching verification,
selection, content, or route generation needed to reject stale work.

## Resource Bounds

- Retain existing metainfo and durable piece-count limits and one candidate
  bitmap per active checker.
- Walk unchecked pieces with bounded cursors/ranges rather than retaining one
  queued object per torrent piece. `queued_hash_jobs` is a counter over pending
  work, not authority to allocate an eager maximum-piece-count job list.
- Retain existing storage execution and hash concurrency. This tactical may
  tighten them from evidence but may not add unbounded hash or route work.
- Retain at most one latest unapplied selection revision per torrent, one
  pending verification generation, one active exclusive storage operation,
  and one bounded controller terminal-event channel. No command history queue
  is retained in memory.
- Any fence backlog consumes the existing bounded storage command capacity and
  backpressures or rejects admission at that boundary.
- Checker summary state is O(1); detailed active jobs remain bounded by hash
  concurrency. Do not retain one runtime result object per completed piece
  beyond the candidate bitmap/counters.
- Emit at most one coalesced non-terminal checker summary and one heartbeat per
  second per active checker. Terminal/phase updates are bounded by actual
  transitions.
- Retain current file-handle, Android platform acquisition, checkpoint dirty
  set, payload memory, peer, diagnostic, and view-set bounds.
- Record high-water marks for active/queued hash jobs, fence backlog,
  controller events, candidate bitmap bytes, progress updates, heartbeats,
  open handles, and cancellation/join duration.

## Implementation Stages And Gates

1. **Freeze the observed behavior.** Add deterministic regressions for
   selection while checking, missing/short unskip sources entering `Failed`,
   repeated full-generation replacement, zero/stuck checking presentation,
   and stale verification/selection completions. These tests may initially
   document the old result but must not normalize it as desired behavior.
2. **Separate integrity vocabulary and checker accounting.** Introduce pure
   check outcomes and a bounded progress reducer. Stop routing ordinary
   absence/mismatch through transfer hash-failure or generic disk-failure
   presentation. Gate on engine-only geometry, missing-range, corruption,
   cancellation, and monotonic-counter tests.
3. **Make full checking selection-independent.** Inventory all readable
   logical pieces through one checker and replace the full bitmap from those
   outcomes. Preserve full force-recheck containment. Gate on path/part/padding
   tests and the pinned `discrete_checking` equivalent.
4. **Introduce the storage fence and route epoch.** Route shared jobs and
   priority materialization through one bounded owner, prove drain/exclusive/
   rollback/stale-result behavior, and retain backing-neutral plans. Gate on
   boundary materialization, rapid updates, hard I/O, and maximum-geometry
   queue/handle evidence.
5. **Install serialized reconciliation.** Move semantic ordering and terminal
   completion decisions under the torrent controller, coalesce selection
   intent, remove selection-triggered full-verification requests, and replace
   whole-generation selection restart with picker/route reconciliation. Gate
   on pause, recheck replay, removal, shutdown, publication exclusion, and
   crash-generation tests.
6. **Expose checker progress end to end.** Add the bounded runtime summary,
   ViewHub projection/diffs, generated Rust/TypeScript/Kotlin schema updates,
   live adapter model, and shared React presentation. Gate on pure projection,
   suspended-client recovery, slow hash heartbeat, structural no-progress,
   accessibility, and deterministic fake-clock coverage.
7. **Close platform and controlled interoperability cases.** Exercise path and
   platform-capability cancellation/fail-closed behavior, compare priority
   mutation during checking with pinned libtorrent, and record structural
   resource high water. No public swarm or physical device is required unless
   implementation changes platform code beyond generated-contract carriage.
8. **Graduate documentation.** Update the owning topics, readiness matrix,
   tactical evidence, and accepted deferrals. Remove obsolete coarse-boundary
   claims without rewriting completed Tactical `063` as though it made a
   different historical decision.

Each stage must retain focused tests and a coherent production path. Do not
land two checkers, two current storage-route authorities, or both broad
generation replacement and in-place reconciliation as nondeterministic
alternatives.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure transitions | Controller intent coalescing and precedence; route-epoch admission; check outcome reduction; counter arithmetic/overflow; phase transitions; structural progress-path classification; stale token rejection. |
| Engine storage | Full all-piece inventory; missing/short/oversized/corrupt/cross-file/padding/part cases; priority fence drain and rollback; exact materialization/sync; bounded targeted affected-piece validation; delayed and cancelled jobs. |
| Session/store | Selection no longer advances full verification; command receipt/revision replay; generation-matched complete bitmap replacement; paused/running intent; one pending recheck; background event ordering; no unrelated payload mutation. |
| Scripted runtime | Selection in every check phase; 1,000 rapid changes; all-skipped promotion; pause/resume; duplicate force recheck; hard I/O; publication conflict; removal; shutdown; slow hash heartbeat; no-path checker detection. |
| Crash/restart | Death at request, queue, hash, fence, materialization sync, finalization, bitmap transaction, and runtime-reentry boundaries; conservative repeat; no stale result or partial route authority. |
| Views/contracts | Exact optional checker snapshot, bounded diffs, reconnect/reset, generated TypeScript/Kotlin/schema consistency, no durable/runtime conflation, and no per-heartbeat diagnostics. |
| Shared React | Determinate and indeterminate phases in Library, Transfers/Workbench, and selected summary; accessible labels; sorting; stale/offline behavior; terminal handoff to download progress; component and headless browser proof. |
| Controlled oracle | Pinned libtorrent changes priority during checking without interruption; rapid latest-value settling; exact final payload and selection; no public networking. |
| Platform | All-target Rust/Android generated-contract builds; dynamic descriptor mutation remains fail-closed; capability loss and stale completion are contained. Physical device work only if newly required by changed adapter behavior. |
| Repository | Formatting, clippy with warnings denied, complete Rust tests, generated-contract clean rerun, web typecheck/tests/build/CSP, `git diff --check`, and documentation/readiness updates. |

## Implementation Record

The existing `ApplicationService` is the serialized semantic controller; this
slice did not add an actor framework or a task for every stored torrent.
`DownloadControl` supplies task-free latest-value watches for selection and
checker pause intent, while the active application owner orders durable
receipts, incoming/discovery exclusion, checker admission, storage
reconciliation, peer admission, terminal task results, and removal/shutdown.

The engine now has these concrete boundaries:

- full checking walks the complete logical piece cursor independently of
  selection and reduces matched, absent, mismatched, and hard storage outcomes
  without emitting transfer hash failure for inventory results;
- one bit-packed candidate bitmap and one bounded `JoinSet` use the existing
  hash concurrency; phase transitions and at-most-one-hertz heartbeats publish
  exact generation-scoped checker progress;
- the selection watch retains only its latest durable revision. Checking
  stops admission between bounded batches, drains hashes, reconciles the route,
  and continues the same verification generation;
- the content storage pipeline itself is the fence: it stops admitting work,
  joins its bounded queue, returns the sole `SelectiveStorage` owner, and
  restarts only after an exact route transition publishes its next epoch;
- promotion synchronizes exact verified part spans. If an expected span is
  unavailable, the route reports and durably clears only the affected pieces
  before the picker admits repair. A hard transition error restores the prior
  in-memory route and bitmap;
- live picker replacement preserves peer connections and availability while
  cancelling requests which became unwanted. Selection does not advance the
  durable full-verification generation;
- Pause while checking raises a drain-and-hold request. Admitted hashes join,
  the same task retains storage, generation, candidate bitmap, and cursor, and
  Resume releases it. Shutdown/removal still use cancellation and joined
  teardown; and
- the view contract and shared React model carry checker generation, phase,
  exact counters, and liveness. Transfers, Workbench, Library, and General use
  the same authoritative determinate/indeterminate presentation and no client
  timer.

No database migration, dependency, view-contract version change, trusting
resume heuristic, or `Download now` command was introduced. Fixed descriptor
manifests retain their prior fail-closed dynamic-selection behavior. A future
trusting resume preference can choose whether to admit the full checker before
this unchanged controller/checker boundary; Force recheck remains full.

Implementation commits are `22adbf1`, `d249371`, `23e0af4`, `3b74a3d`,
`4bcc7e1`, and `e4991a2`. The accepted tactical itself was recorded by
`04bd50d`.

## Scenario And Resource Evidence

The stable scenarios are covered as follows:

| Scenarios | Evidence |
| --- | --- |
| T108-C01 through C04 | `selection_fence_and_slow_hash_heartbeat_share_one_check_generation`, `rapid_file_selection_updates_retain_only_the_latest_revision`, application partial-priority peer-generation coverage, and the pinned libtorrent checking/priority cases prove one generation and latest-value settling across 1,000 updates. |
| T108-C05 and C06 | Application all-skipped idle/restart coverage plus `live_selection_reconcile_promotes_and_demotes_without_losing_verification` and `promotion_clears_only_piece_evidence_with_a_missing_part_span` prove exact boundary export, retained demotion, epoch publication, and affected-only invalidation. |
| T108-C07 and C08 | The checker reducer, skipped-readable full check, corrupt/absent recheck cases, transfer corruption suites, and existing force-recheck containment tests keep inventory, peer failure, storage failure, and exclusivity distinct. |
| T108-C09 and C10 | `pause_and_resume_retain_the_active_checker_generation_and_cursor`, the slow checker fence test, and application/store request-replay coverage prove drain-and-hold pause plus one pending force-recheck generation. |
| T108-C11 and C12 | Existing joined shutdown, exact managed removal, publication collision, fixed-descriptor rejection, storage-generation, and route rollback suites remain green with the new fence. |
| T108-C13 through C15 | Checker reducer/view projection tests enforce exact arithmetic and stale completion; the 1.1-second delayed hash publishes live job age; component and Chrome tests cover hashing and every non-hashing presentation class. The cursor loop cannot retain a hashing state with no admitted or remaining work. |
| T108-C16 through C18 | Schema-14 crash-generation and publication suites, generation-matched view/store tests, selection-independent skipped-piece recovery, Android host tests, the 40-handle pool bound, and fixed-descriptor fail-closed coverage remain green. |

Recorded structural high-water marks and limits:

- active checker hashes: `1` in the slow deterministic scenario and at most
  the configured production hash concurrency of `4`;
- eagerly allocated checker queue: `0`; `queued_hash_jobs` is arithmetic over
  the cursor, not one object per piece;
- selection backlog: one latest watch value after 1,000 replacements;
- extra fence/controller backlog: `0`; the fence drains the existing bounded
  storage pipeline and `ApplicationService` receives terminal results directly;
- candidate bitmap: at most 2,097,152 bits, or 262,144 bytes, under the
  metainfo piece limit; recovered staging indices remain bounded by the same
  geometry;
- progress delivery: forced phase/terminal changes plus at most one ordinary
  progress or heartbeat emission per second;
- open storage handles: no new pool; path and platform work retain the shared
  40-handle application bound; and
- join behavior: two 200-millisecond cancellation hashes joined inside the
  two-second gate, while one deliberately 1.1-second admitted hash drained to
  paused inside two seconds and resumed without duplicate admission.

## Validation Evidence

Validation completed on 2026-08-07:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`;
- `cargo test --workspace`: engine `356` passed/`7` ignored, session `194`
  passed/`2` ignored, and every other workspace/unit/doc-test target passed;
- deterministic contract regeneration produced an unchanged generated patch;
- `npm test`: `231` passed/`2` skipped across `36` files;
- `npm run typecheck`;
- `npm run build`, including the CSP bundle check;
- headless Chrome passed
  `checker progress stays truthful across every shared surface`, including
  determinate and indeterminate progressbar semantics and zero serious or
  critical Axe findings;
- pinned libtorrent `2.0.13` at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d` was built out of tree and passed
  `test_checking.cpp::{discrete_checking,preserve_file_priorities}` and
  `test_priority.cpp::{file_priority_multiple_calls,
  file_priority_stress_test}`; and
- `git diff --check` passed and the pinned reference checkout remained clean.

## Non-Goals

- The visible `Download now` file action or its future atomic
  wanted-plus-running semantic command.
- A user-facing trusting fast-resume preference, metadata trust heuristic,
  timestamp/inode/provider identity policy, clean-shutdown shortcut, or
  persisted partial check frontier.
- Relaxing force recheck into concurrent peer verification or presenting
  unchecked bytes as verified.
- Dynamic Android SAF descriptor-manifest mutation, provider document
  creation policy, or new Compose UI.
- Higher/lower/deadline/streaming file priorities, piece deadlines, sequential
  mode, or storage relocation.
- Multi-torrent execution, a new global torrent scheduler, remote daemon,
  socket proxy, external service, general actor framework, or mandatory task
  per durable torrent.
- Deleting a newly skipped destination, importing arbitrary unowned content,
  adopting ambiguous files, or changing publication collision policy.
- Persisting detailed checker telemetry, per-piece failure reasons, runtime
  queues, timestamps, or task handles in SQLite.
- A new Disk tab or checker-log stream; existing detailed storage inspection
  remains separate from the summarized checker contract.
- Public-swarm benchmarking, visible manual client testing, hardware testing,
  or a performance support claim.

## Next Slice

After this tactical completes, add one bounded semantic `Download now`
operation which atomically makes the target file wanted and sets run intent to
running, then expose it for skipped files in the shared Files surface. Its
implementation should contain no checker-state switch beyond submitting
durable intent and reporting controller convergence.

A separate later tactical may add an explicit faster, more trusting resume
option using the validation-policy seam established here. It must record the
trust and mutation-detection contract for private staging, final published
paths, and platform providers; compare it with pinned libtorrent; preserve
full Force recheck; default conservatively unless product direction changes;
and label the resulting evidence honestly.

## Escalation Contract

Implementation may choose internal type/module names, controller embedding,
bounded channel capacities within existing limits, progress copy consistent
with the accepted UI language, and refactor placement without further
approval. It may fix newly exposed bugs at the same selection/checking/storage
ownership boundary and update generated contracts and owning topics.

Stop for direction if evidence requires concurrent peer activity during full
checking, changes the default conservative trust policy, introduces a durable
schema field not implied above, makes dynamic platform selection part of this
slice, adds a dependency or general actor/runtime framework, changes payload
ownership/publication policy, requires destructive profile/payload mutation,
or materially expands into multi-torrent scheduling or the `Download now`
product slice.
