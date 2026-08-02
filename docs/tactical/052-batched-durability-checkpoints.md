# Tactical 052: Batched Durability Checkpoints

Status: Active on 2026-08-02.

Topics: `storage-throughput-architecture`, `download-correctness`,
`client-persistence`, `disk-and-piece-inspection`,
`performance-and-live-evidence`, `oracle-driven-engine-campaign`

## Motivation And Outcome

Tacticals `031` and `032` attribute 93--94% of retained public wall time to
one serialized write/hash service. After write coalescing reduced 5,648--5,740
logical blocks to 500--509 physical writes, write service fell to
51.4--54.9% while the operation still labeled hash service rose to
39.1--41.6%. The current verify operation includes `sync_data` on every file
touched by each verified selective piece, and its completion synchronously
calls `StoreCheckpointSink::piece_durable` on the content supervisor. That
method locks the shared `SessionStore`, rewrites the full have bitmap and
commits one `synchronous=FULL` SQLite transaction per piece.

Separate integrity from crash durability. A piece whose complete SHA-1 hash
matches and whose writes succeeded becomes verified for in-process scheduling
without waiting for payload synchronization or SQLite. One bounded, joined
checkpoint owner batches those verified-dirty pieces, synchronizes each
touched destination once per epoch, commits the batch through one have-bitmap
transaction, and then publishes durable progress. Hash, sync and database time
become distinct observable stages.

This tactical also establishes a repeatable controlled storage profile before
and after the behavior change. It is the first implementation slice of the
accepted maximum-throughput storage campaign; it does not add positional write
workers yet.

## Stable Scenarios

- Hundreds of small pieces may hash-verify while an older checkpoint epoch is
  synchronizing, without later writes or hashes waiting behind that epoch.
- A verify command clears the active hashing stage when SHA-1 finishes. Pieces
  awaiting durability appear as checkpoint-dirty, syncing or committing, not
  hashing.
- A payload sync or database failure never creates durable have state and
  fails the owned download with every task joined.
- Graceful completion, publication, pause and shutdown flush already verified
  dirty pieces before returning their owned storage or reporting a completed
  receipt.
- A process stopped after hash verification but before the epoch commit may
  redownload or conservatively recheck the piece; it cannot trust a false have
  bit.
- One epoch touching many pieces in one file calls payload sync once and
  updates the have bitmap in one SQLite transaction.
- A mixed selected/skipped epoch synchronizes every touched wanted file and
  the part payload before its database commit.
- The controlled profile is long enough to measure steady-state overlap and
  reports exact content, cleanup, queue bounds, stage service and checkpoint
  amortization.

## Normative And Reference Dossier

No reference source, fixture or resume format is copied.

- Pinned BEP 3 at `reference/bittorrent.org/beps/bep_0003.rst` makes the
  complete piece hash the integrity authority; it does not require stable
  storage or resume persistence before a client may use an in-process piece.
- Pinned libtorrent `2.0.13` at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d` separates the hash result from
  final write callbacks in `src/torrent.cpp::{verify_piece,on_piece_verified}`
  and `test/test_piece_picker.cpp::{piece_passed,
  set_piece_priority_passed_hash_check}`. A piece joins those facts rather than
  imposing a per-piece durable-storage barrier.
- `src/mmap_disk_io.cpp::{async_write,do_write,do_hash}` retains queued
  accepted blocks for hash read-through and treats disk jobs as session-owned
  asynchronous work. `test/test_storage.cpp::{
  mmap_unaligned_read_both_store_buffer,
  posix_unaligned_read_both_store_buffer}` covers queued/completed read
  consistency.
- Libtorrent exposes resume state asynchronously through
  `torrent_handle::save_resume_data` and `save_resume_data_alert`; its caller
  owns persistence. `test/test_resume.cpp::resume_data_have_pieces` covers
  have-state snapshot content. RSTorrent retains its typed SQLite authority
  rather than adopting libtorrent's resume BLOB.
- `src/part_file.cpp::{write,flush_metadata}` protects mapping allocation but
  performs positional payload I/O separately and flushes dirty mapping state
  as a distinct operation. RSTorrent defers changing its part-file mapping
  format and current per-slot metadata sync to Tactical `053`.
- JSTorrent at `9895410beeed6aff554053769bd006a3fbd373ef` supplies product
  history through `packages/engine/src/core/disk-queue.ts` and the native
  batching queues: work and bytes are bounded separately, completions remain
  owned, and batching amortizes platform calls only under backlog.

## Accepted Owner, Task, And Data Flow

```text
content storage task
  write -> hash -> hash/write join -> checkpoint intent
                                      |
                                      v
joined checkpoint task       fixed target-handle registry
  bounded time/bytes/pieces -> sync unique destinations
                            -> one batched checkpoint callback
                                      |
                                      v
StoreCheckpointSink -> SessionStore::record_pieces -> one view batch
```

`run_content_storage_task` remains the only mutable `ContentStorage` owner and
continues to own cursor-based writes and hashes. It no longer calls
`SelectiveStorage::sync_piece`. At pipeline creation, resumable selective
storage duplicates one stable sync-only handle for each wanted file and the
part file. The cloned handles have no cursor operations and are moved into one
checkpoint task.

After a successful hash, the storage completion identifies the piece, length
and touched durability-target IDs. The supervisor reserves bounded dirty-byte
and item capacity, queues the intent, marks the piece verified in `SwarmState`
and continues ordinary intake. Waiting for capacity is allowed only when the
declared dirty checkpoint bound is genuinely full.

The checkpoint task owns batching, target de-duplication, payload sync,
database callback, metrics, failure and termination. It uses bounded blocking
work for filesystem sync and the synchronous SQLite callback. The application
service remains the SQLite owner; `DownloadCheckpointSink::pieces_durable`
receives a nonempty de-duplicated batch and `StoreCheckpointSink` commits it
with `SessionStore::record_pieces` before publishing one coherent view change.

Closing the checkpoint sender forces the current and queued dirty state. The
storage pipeline does not return its `ContentStorage` until both storage and
checkpoint tasks join. Publication therefore begins only after every verified
piece from that run is durably checkpointed. Cancellation stops new storage
admission but still flushes already hash-verified intents; unchecked or
unwritten work remains absent from the checkpoint.

Dependency direction remains inward: deterministic target selection and batch
state contain no Tokio, file, SQLite, view or application types. The engine
defines the checkpoint-sink contract and owns filesystem durability; the
session crate implements the concrete database and view commit.

## Initial Bounds

- checkpoint maximum age: 2 seconds from the oldest dirty piece;
- checkpoint maximum dirty payload: 64 MiB, with one individually valid
  larger piece admitted alone for liveness;
- checkpoint maximum pieces per epoch: 256;
- pending checkpoint channel: 256 intents;
- sync concurrency: at most four unique destinations per epoch;
- one stable extra sync-only handle per wanted file plus the part file, bounded
  by validated metainfo or the descriptor manifest;
- one SQLite writer and one transaction/revision per nonempty epoch;
- no new payload copy, piece-sized allocation, command history or unbounded
  metric collection.

Dirty bytes remain charged until the corresponding database callback succeeds.
The sender blocks only at the byte/item bounds. Exact constants may tighten
within 1--5 seconds and 16--64 MiB if deterministic or controlled evidence
finds a correctness, memory or latency problem; expanding them requires
recorded evidence.

## Integrity And Crash Invariants

- `hash_verified` requires a matching trusted hash and all generation writes
  successful; neither sync nor SQLite establishes content integrity.
- A database have bit is committed only after every captured destination sync
  succeeds.
- Pieces written or synchronized after an epoch snapshot may be physically
  ahead of metadata and remain safe false negatives.
- A checkpoint callback is all-or-nothing for its batch. An invalid index,
  SQLite error, view error, sync error or task panic fails the owner and does
  not report a partial durable completion.
- Existing restart rehashes every claimed piece before presenting it as
  verified.
- Part-slot metadata already synchronized by the current format remains
  conservative; batching or changing that metadata is explicitly deferred.
- Hash failure never enters the checkpoint queue. A later write failure cannot
  occur in this tactical because hashing still starts only after every write
  completion for that piece.

## Observability Contract

Extend the existing fixed `DownloadControl`, `DownloadProgress`,
`DiskRuntimeSnapshot`, application Disk view and generated client contracts.
At minimum expose:

- checkpoint-dirty piece count and bytes plus oldest age;
- checkpoint batches, pieces and unique sync operations;
- sync and database callback service duration separately;
- one active checkpoint stage; and
- piece stages for checkpoint dirty, syncing and committing.

The existing `storage_hash_*` fields stop timing before checkpoint work.
Counters remain saturating and history-free. One batch view update represents
all pieces committed at one SQLite revision; diagnostics do not become state.

## Staged Implementation And Gates

1. Add `SessionStore::record_pieces` and a batched `ViewHub` durable-piece
   transition. Prove one decode/encode, revision and transaction for many
   pieces, duplicate handling, rollback and bounds.
2. Add pure checkpoint batch selection and target de-duplication with exact
   time, byte, piece and large-piece behavior.
3. Add sync-only storage-handle registration and the joined checkpoint task.
   Remove per-piece `sync_piece` from verification and move the batch callback
   off the supervisor.
4. Split Disk stages and fixed metrics; update generated schemas, TypeScript,
   fixtures and frontend stage presentation without changing layout policy.
5. Add deterministic delayed-sync and delayed-checkpoint controls. Prove writes
   and hashes advance during each delay, true bound backpressure, exact task
   joins, forced final flush and typed failure propagation.
6. Add subprocess crash fixtures at pre-sync, post-sync/pre-commit and
   post-commit boundaries. Restart must recheck committed bits and safely miss
   uncommitted bits.
7. Add a configurable controlled storage-throughput profile with a quick smoke
   and a steady-state size calibrated to roughly 30--60 seconds under a hard
   4 GiB payload and temporary-disk cap. Retain the 32 MiB historical profile.
8. Pass formatting, warning-denying clippy, workspace tests, selective and
   mixed-source interop, paired controlled publication, generated-contract
   checks, and both Android Rust target checks.
9. Run the steady controlled cohort. Run the headless product Big Buck Bunny
   comparator only after controlled attribution is causal. Public speed remains
   a distribution, not the tactical's correctness oracle.

## Pre-Change Controlled Baseline

The first evidence checkpoint retained two complementary loopback profiles on
the same development machine and pre-change engine binary:

```bash
uv run --project tests/interop --locked \
  python tests/interop/selective_hash_profile.py --profile quick --runs 1
uv run --project tests/interop --locked \
  python tests/interop/selective_hash_profile.py --profile steady --runs 3 \
  --binary target/debug/rstorrent-download-piece
uv run --project tests/interop --locked \
  python tests/interop/session_checkpoint_profile.py --runs 3 \
  --binary /tmp/rstorrent-pre052/target/debug/rstorrent-session
```

The historical 32 MiB quick profile remained exact at 2.005 seconds. The new
128 MiB engine-only steady profile contains 512 256 KiB pieces and 8,192
blocks across three unaligned wanted files. It completed at 36.564--38.896
seconds with a 37.594-second median. Every run reached the 16-block/256 KiB
write-batch caps, performed 542--546 physical writes, spent
31.108--34.088 seconds in serialized write service, matched all three file
hashes and cleaned up. This profile does not instantiate SQLite; it is the
stable baseline for later positional and concurrent execution slices.

The new application-service profile downloads a separate deterministic
128 MiB/512-piece multi-file torrent through the loopback seed, path-backed
session service, `synchronous=FULL` SQLite store and ordinary publication.
The retained pre-change executable was built from exact commit `e618d2b` and
fingerprinted as SHA-256 `323722b2e925ffc9e7844a624af5d8f1fe2601dda59d61983a8c264b97bb28c6`.
Three runs completed at 50.019--50.301 seconds with a 50.085-second median.
The metadata checkpoint was observed before any verified piece on every run;
the final SQLite torrent revision was exactly 514 revisions later: one for
each of 512 per-piece checkpoints and two final storage/state transitions.
Every run retained exact raw info, full have geometry, the same payload SHA-1,
verified publication and exact owner/artifact cleanup. An earlier
12.707--13.370-second observation was rejected after the new executable
fingerprint identified a stale July 31 binary; it is not baseline evidence.

Larger 256 MiB and 768 MiB engine-only calibration attempts were stopped after
crossing the declared steady window well before completion. Their owned
processes and temporary roots cleaned exactly. The retained 128 MiB profile is
large enough to sustain backlog without making a timeout the expected result.

## Implementation Checkpoint 1: Batched Application Commit

`DownloadCheckpointSink` now exposes `pieces_durable`; its one-piece default
keeps the engine behavior unchanged until the joined checkpoint task lands.
`StoreCheckpointSink` rejects an empty batch, sorts and de-duplicates indices,
converts every view index before committing, calls
`SessionStore::record_pieces`, publishes one coherent Piece/Files transition
at that revision and emits one bounded diagnostic containing only the batch
count.

`record_pieces` decodes have state once, applies every index, encodes once and
advances one transaction/revision. A deterministic three-piece test proves
that `[2, 0, 2]` commits pieces zero and two at one revision, an empty batch is
rejected, and `[1, 3]` rolls back piece one and the global revision when the
later index is invalid. The existing `record_piece` delegates to the batch
operation. A view test proves `[3, 1, 1]` produces one patch with two exact
ranges and one aggregate verified count.

Validation at this checkpoint:

```text
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test -p rstorrent-session --lib
```

All 78 session tests pass. The next internal gate is runtime-independent
checkpoint epoch selection and target de-duplication; payload sync and engine
task ownership are still unchanged.

## Implementation Checkpoint 2: Joined Runtime Owner

The engine now separates successful SHA-1 verification from durability. Pure
checkpoint state selects epochs at the exact two-second, 64 MiB and 256-piece
bounds, admits one oversized piece alone, rejects duplicate pieces and
de-duplicates stable wanted-file/part-file target IDs. Six deterministic tests
cover those transitions.

Resumable selective storage registers one sync-only handle for every wanted
file and the part file. A joined checkpoint task holds both item and dirty-byte
permits through target synchronization and `pieces_durable`, synchronizes no
more than four unique targets concurrently, validates all handles before
launching any blocking work, and drains every launched job after a failure.
The storage owner no longer calls per-piece `sync_piece`; the supervisor queues
the verified intent, and final publication waits for both the storage and
checkpoint owners to join.

The matched post-change application profile used executable SHA-256
`7d80f5267382993143615dff333a1a4954d6553ac11944e44ff3703f7e1e9b59`.
Three exact runs completed at 44.580--45.282 seconds with a 45.221-second
median, 9.7% below the corrected 50.085-second pre-change median. Each used
only 16--18 post-metadata revisions rather than 514, a 28.6--32.1x reduction,
while exact payload, raw info, have geometry, publication and cleanup held.

The forced-death resume fixture is now 32 MiB with 128 256 KiB pieces so it
cannot complete before the batched owner's age boundary. One retained run
killed the process after 112 durable pieces, corrupted one claimed piece, then
rechecked 111 and downloaded exactly 4,456,448 missing/corrupt bytes before
exact completion and cleanup. The old eight-piece fixture was correctly
rejected because it could complete before the first epoch, making it a test of
per-piece timing rather than crash recovery.

The next internal gate is fixed checkpoint observability followed by
deterministic sync/database delay and failure control. Positional writes and
hash concurrency remain unchanged.

## Implementation Checkpoint 3: Truthful Fixed Observability

`DiskPieceStage::Hashing` now ends at the actual SHA-1 result. Resumable pieces
then move through `CheckpointDirty`, `CheckpointSyncing` and
`CheckpointCommitting`; a successful batch removes the active rows, while a
failed sync or callback marks its rows and the pipeline error state before the
owner returns the typed failure. Non-resumable pieces still leave the active
set immediately after a matching hash.

`DownloadProgress`, `DiskRuntimeSnapshot`, the application Disk projection,
generated Rust/JSON/TypeScript contracts and the web inspection model expose
one fixed checkpoint stage; current dirty pieces/bytes and oldest age; piece
and byte high-water marks; started/completed batches; completed pieces and
unique sync operations; separate cumulative/maximum sync and database service
time; and current active-stage age. The existing Disk panel adds the
checkpoint backlog and service rows without adding operation history or a
block-level table. The selected Pieces map treats the three durability states
as stored content while the detailed Disk rows retain their exact stage.

One deterministic engine transition test covers dirty, sync, commit and
terminal counters. The session Disk projection test covers typed aggregation,
active age and terminal clearing. Warning-denying workspace clippy, 163
non-live engine tests, 78 session tests, 95 web tests, strict TypeScript and a
fresh 128 MiB application profile pass; that profile retained exact payload
and 18 post-metadata revisions. Its single 50.204-second latency sample is a
correctness smoke, not a replacement performance cohort.

The next internal gate is deterministic delayed sync/callback execution,
capacity backpressure, failure propagation and forced final flush.

## Implementation Checkpoint 4: Adversarial Delay, Failure, And Crash

Bounded test controls now delay payload sync and the checkpoint callback
independently. One task-level test holds an exact 64 MiB epoch through 350 ms
of each delay, completes a real staging write and SHA-1 read while sync remains
active, and proves the next dirty piece blocks only while the byte semaphore is
truly full. Capacity reopens after the callback, closing the sender forces a
second partial epoch, the sink observes exact batches `[7]` then `[8]`, every
task joins, and dirty gauges return to zero. Separate injected sync and sink
failures reach the supervisor failure channel, preserve zero completed batches
for the failed epoch, mark its rows and global stage as error, and join with
typed checkpoint failures.

The session diagnostic accepts bounded hidden sync/commit delays and an
explicit checkpoint-stage trace only for controlled evidence. The new 64 MiB,
256-piece subprocess matrix kills that owned child at three exact markers and
restarts the same profile:

- pre-sync crashed at revision 3 with zero durable pieces, then uploaded all
  67,108,864 bytes on restart;
- post-sync/pre-commit also crashed at revision 3 with zero durable pieces and
  uploaded all 67,108,864 bytes; and
- post-commit crashed at revision 4 with five durable pieces, retained those
  exact claims after recheck, and uploaded 65,798,144 bytes: precisely the
  remaining 251 pieces.

All three restarts produced SHA-1
`645e90d7a71313eb68b0c2c3de0dd165bdcd893c`, reached complete durable have
state, joined, and removed their profile, payload, seed and process artifacts.
The first calibration also demonstrated that while a small age-triggered epoch
was held in sync, later hashes advanced until the declared global 64 MiB/256
piece dirty bound—not an unbounded queue—became full.

The next internal gate is the full workspace, generated-contract, controlled
interop and Android target matrix, followed by the retained steady cohort and
optional headless public comparator.

## Stopping Condition

The tactical completes when hash verification, payload sync and SQLite commit
are separate owned stages; ordinary writes/hashes demonstrably advance during
checkpoint delay; one bounded epoch amortizes sync and database work across
many pieces; restart and failure cases remain conservative; all owners join;
and the controlled profile records an honest before/after result.

No speed claim is required to retain the correctness-preserving state split.
Claim a checkpoint-path improvement only if the steady controlled cohort shows
materially lower hash-stage service and supervisor stall without a throughput,
memory, integrity or tail-latency regression. The next boundary is immutable
positional storage plans and part-file metadata ownership in Tactical `053`.

## Non-Goals

- concurrent writes or hashes, pending-write hash reads, a session-wide disk
  pool, multi-torrent fairness or adaptive worker counts;
- changing part-file format or removing its current per-slot metadata sync;
- fast resume that trusts un-rehashed bytes;
- direct I/O, memory mapping, `io_uring`, unsafe code or a new dependency;
- peer, picker, request-window, discovery, protocol or public UI layout policy;
  or
- physical-device interaction without the repository's separate explicit
  authorization process.

## Escalation Contract

No routine implementation input is required. Internal refactoring, generated
contract updates, deterministic failure controls, bounded temporary fixtures,
headless public cohorts and reasonable commits are authorized. Stop only for a
new dependency or compatibility break, materially different crash semantics,
destructive user-data action, visible/physical-device interaction, or evidence
that requires abandoning the accepted storage-throughput architecture.
