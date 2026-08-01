# Tactical 032: Bounded Coalesced Write Batches

> Current storage admission derives from the received-payload byte budget rather
> than the fixed job bounds recorded here. See Tactical `039`; this document
> remains the execution record for its original commit.

Status: Complete

Topics: `performance-and-live-evidence`, `download-correctness`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `031` measured three public tracker+DHT screens and attributed
93.2--93.7% of wall time to the serialized content-storage owner. Logical
16 KiB writes alone consumed 87.7--88.2%, with 8.5--38.3 ms average and
272--842 ms maximum service. Hashing consumed only 5.5--5.7%. The queue's
66-job high water is therefore a consequence of physical write service, not
an independent reason to change peer or request policy.

Replace one-write-command-per-I/O execution with a bounded storage-owner batch.
Drain already-admitted writes without waiting, cap each batch at 16 blocks and
256 KiB, sort by piece/range, and coalesce adjacent blocks in the same piece
into one storage call. Preserve a completion for every logical block and keep
verification behind all of that piece's successful write completions. Measure
physical batches separately from logical blocks so post-change service time
remains comparable to wall time.

## Source And Test Dossier

Pinned libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` is the completeness oracle. No
source or fixture is copied.

- `src/settings_pack.cpp` defaults `aio_threads` to 10, `disk_write_mode` to
  `auto_mmap_write`, and `mmap_file_size_cutoff` to forty 16 KiB blocks.
- `src/peer_connection.cpp::incoming_piece` submits accepted blocks through
  `async_write`, accounts queued write bytes, and defers `submit_jobs` rather
  than awaiting one filesystem operation in the peer path.
- `src/mmap_disk_io.cpp::async_write` owns a copied fixed block, exposes it in
  the bounded store buffer, and queues a write job. `add_job`, `submit_jobs`,
  and `thread_fun` execute queued generic I/O through the configured pool.
- `src/mmap_disk_io.cpp::do_write` performs one storage write and retires the
  store-buffer entry only after completion. `src/mmap_storage.cpp::write`
  selects `pwrite_all` or a bounded mapped-file copy per physical range.
- `src/posix_disk_io.cpp::async_write` and
  `src/posix_storage.cpp::write` are the synchronous fallback: one accepted
  block maps across files and uses retained file ownership, rather than
  reopening it at the transfer owner.
- `test/test_storage.cpp::{both_sides_from_store_buffer,
  first_side_from_store_buffer,second_side_from_store_buffer}` proves reads
  spanning queued and completed adjacent writes return exact data.
- `test/test_piece_picker.cpp::set_piece_priority_passed_hash_check` and its
  bulk/filter variants prove that a hash result and individual write
  completions may arrive in either order but a piece is not flushed until both
  conditions hold.
- `simulation/disk_io.hpp` and `simulation/disk_io.cpp::async_write` model a
  bounded high/low-water write queue, delayed completion, disk-full recovery,
  and observer wakeup. Backpressure is based on retained blocks/bytes rather
  than an unbounded executor.

RSTorrent adopts deferred bounded physical work, independent logical
completions, retained payload accounting, and explicit batch measurements. It
does not adopt libtorrent's thread count, memory mapping, store-buffer reads,
hash-before-last-write transition, disk observer registry, session-wide disk
pool, or C++ storage architecture.

The JSTorrent product-history reference is:

- `packages/engine/src/core/disk-queue.ts`, where six workers are the default
  and pending/running jobs, bytes, draining, clearing, and bounded grabbing are
  explicit;
- `packages/engine/test/core/disk-queue.test.ts`, which covers worker limits,
  drain/resume, pending rejection, byte accounting, and bounded batch grabs;
- `packages/engine/src/adapters/native/native-async-write.ts`, which collects
  boundary/unverified writes until the end of one engine tick and dispatches
  one native batch;
- `packages/engine/src/adapters/native/native-batching-disk-queue.ts`, which
  caps a native batch at 128 writes and 4 MiB and tracks pending/in-flight
  writes and bytes; and
- `packages/engine/src/core/torrent-content-storage.ts::tryBatchWrite`, which
  combines same-file work only above a backlog threshold, caps batch and
  in-flight bytes, and resolves or rejects every logical job together.

RSTorrent adopts the platform lesson that dispatch overhead is amortized only
under backlog and that every batch needs count, byte, failure, and cancellation
bounds. It uses much smaller torrent-local limits and no FFI, HTTP, native
verified write, whole-piece buffer, or multi-worker policy.

## Owner, Task, And Cancellation Map

`ContentStoragePipeline` remains the only admission owner. Its two-command
local pending bound, 64-command channel, payload charging, and completion
channel do not grow. `run_content_storage_task` remains the only executor and
the only mutable `ContentStorage` owner.

When the first received command is a write, the storage task uses non-blocking
channel receives to collect only already-admitted writes. It stops at 16
logical blocks, 256 KiB, an empty channel, disconnect, or the first verification
command. A verification command encountered while draining is retained as one
local deferred command and executes before any later channel item. The task
does not wait to fill a batch and therefore adds no low-load latency or timer.

Batch preparation validates and measures every logical block before physical
I/O, sorts by `(piece, begin)`, rejects overlap, and joins only exact adjacent
ranges in the same piece. Cross-piece and gapped ranges remain separate
physical calls. Selective-file mapping, padding rejection, skipped-file part
ownership, and path/descriptor backing stay inside `SelectiveStorage`.

Cancellation stops admission, joins the active batch, drops retained deferred
and channel work through the existing owner, clears active diagnostics, and
releases payload/job accounting through the existing terminal cleanup. No
detached file task or per-batch child task is added.

## Invariants And Resource Bounds

- one physical batch contains at most 16 logical blocks and 256 KiB of owned
  accepted payload;
- coalescing may transiently allocate at most one additional batch-sized byte
  vector, so hidden preparation memory is bounded at 256 KiB per torrent;
- the existing 66-job and payload limits remain the admission authority;
- one logical block produces exactly one success completion, activity event,
  stored-byte increment, and request-generation transition;
- no piece is verified until every logical block reports a successful physical
  write, and no have/resume state precedes its exact hash/durability transition;
- a validation or physical write failure cannot be reported as stored content;
- sorting never crosses a verification fence, piece boundary, range gap,
  overlap, padding range, or storage owner;
- physical batch counts/service time and logical block counts are distinct,
  saturating diagnostics; and
- zero/short/overflowing ranges remain rejected by the existing protocol,
  layout, and storage bounds.

## Edge Cases And Gates

1. Pure batch preparation covers one block, out-of-order adjacent blocks,
   gaps, different pieces, exact caps, and overlap rejection.
2. Single-file storage covers out-of-order coalesced writes and exact hash and
   publication.
3. Selective storage covers a coalesced range crossing wanted files, a skipped
   boundary through the part file, padding rejection before mutation, and
   descriptor-backed Android-compatible ownership.
4. Runtime storage pressure proves fewer physical operations than logical
   blocks, exact per-block completions, 16/256 KiB batch high waters, payload
   and queue bounds, and exact publication.
5. Delayed write, write failure, queued cancellation, hash cancellation,
   endgame cancellation, fair discovery intake, and corrupt-piece recovery
   remain green with no detached jobs or false verification.
6. The headless probe publishes physical operation counts, logical block
   counts, maximum batch blocks/bytes, and serialized batch service. The
   libtorrent adapter uses explicit `null` for unavailable owner fields.
7. Formatting, warning-denying workspace clippy, workspace tests, selective
   and mixed-source controlled interop, comparator tests, paired controlled
   publication, and both Android target checks pass.
8. Re-run the 32 MiB controlled profile and three alternating public Big Buck
   Bunny 50% pairs. Run a full pair if the 50% result is functional and the
   terminal owner remains ambiguous.

## Stopping Condition

The tactical completes when the bounded batch lifecycle and all integrity,
failure, cancellation, and Android gates pass, and retained evidence selects
the next owner. Claim a write improvement only if the controlled median falls
by at least 20% from Tactical `031`'s 1.196-second instrumented median and live
physical write service falls materially without a correctness or resource
regression. Public latency remains a distribution and must also be reported if
neutral or worse.

If batching is materially effective but write service remains the dominant
wall-time owner, the next tactical may add bounded positional-write concurrency
with per-piece verification fences. If batching is neutral, inspect runtime
dispatch and filesystem traces before choosing dedicated blocking ownership,
positional I/O, or concurrency. If write service becomes small, return to the
paired peer/request timeline. Do not combine those outcomes in this slice.

## Result And Evidence

The completed implementation retains the single torrent-local storage owner
and drains only writes already admitted to its 64-command channel. It stops at
16 logical blocks, 256 KiB, an empty channel, or a verification fence. Exact
adjacent same-piece ranges share one storage call; gapped, cross-piece, and
fenced work remains separate. The supervisor now also treats the existing
66-job total as an independent intake bound so draining commands into a local
batch cannot expand retained work.

Every logical block retains its own completion, selected/part byte accounting,
payload release, activity event, and swarm transition. Physical operation
counts and service time are distinct from logical block counts. The public
probe and controlled diagnostic publish both, and the libtorrent adapter emits
explicit `null` values where its Python binding has no equivalent owner.

Pure and runtime tests cover exact count and byte caps, out-of-order adjacent
ranges, gaps, different pieces, overlap rejection, failure before valid-prefix
mutation, wanted/skipped accounting across one combined range, delayed work,
queued and hash cancellation, storage pressure, fair discovery intake,
endgame cancellation, and exact logical completion. The full gate passed
formatting, warning-denying workspace clippy, and all 258 listed workspace
tests: 255 passed and the three opt-in public tests remained ignored. Three
selective-file runs, three 32 MiB selective-hash runs, the controlled
mixed-peer scenario, paired controlled publication, and all nine comparator
tests passed. `rstorrent-android` checked for both `aarch64-linux-android` and
`x86_64-linux-android` using the installed NDK toolchain.

The final 32 MiB controlled runs completed exact publication and cleanup at
1.354, 1.143, and 1.124 seconds, for a 1.143-second median. That is 4.4% below
Tactical `031`'s 1.196-second median and does not meet the predeclared 20%
improvement threshold. Each run reduced 2,048 logical blocks to 144--154
physical operations, reached the exact 16-block/256 KiB batch limits, and
spent 0.232--0.331 seconds in physical write service. Operation shape improved
materially, but controlled wall time is neutral.

A preliminary public cohort was rejected before use because `--no-build` had
selected a probe compiled before this tactical; its one-block operation counts
made the stale binary explicit. The retained cohort used the comparator's
normal build path and ran three alternating product tracker+DHT Big Buck Bunny
50% pairs with 300-second owner bounds. Libtorrent reached 50% in 27.26,
29.07, and 29.94 seconds. RSTorrent timed out at 345, 351, and 346 of 1,055
pieces, with 86.3--87.8 MiB verified, zero piece-hash failures, and successful
artifact cleanup.

Across those RSTorrent runs, 5,648--5,740 logical blocks became 500--509
physical operations, or about 11.3 blocks per operation. Every run reached the
16-block/256 KiB batch limits and the existing 66-job bound. Write service
fell from Tactical `031`'s 87.7--88.2% of wall time to 51.4--54.9%, but hash
service rose to 39.1--41.6%; combined serialized storage service remained
93.0--94.2%. The public cohort therefore makes no latency or parity claim.

The stopping condition is met: batching is a bounded structural improvement,
integrity and lifecycle gates pass, and retained evidence selects storage
execution rather than peer/request policy. A future tactical may study bounded
positional-I/O concurrency and per-piece verification fences, but this slice
does not design or implement it. The campaign is paused for maintainer review
before any next tactical opens.

## Non-Goals

- multiple storage workers, concurrent writes, mmap, a write-back cache,
  whole-piece resident assembly, in-memory hashing, read-your-pending-write
  support, or session-wide disk scheduling
- changing the 64-command queue, two-command local admission, payload budget,
  peer windows, picker, connection limits, tracker/DHT policy, or hash policy
- part-file format changes, durable single-file resume, dynamic file priority,
  incoming service, seeding, BEP breadth, product UI, browser, Tauri, AVD, or
  physical-device work

## Escalation

The tactical is complete. The maintainer requested a clean committed tree and
a discussion before further campaign work, so no next tactical begins without
explicit authorization.
