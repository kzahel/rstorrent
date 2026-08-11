# Tactical 135: Controlled TCP Storage Near-Parity

Status: **Active** on 2026-08-11. Explicit maintainer direction broadens the
unimplemented queue-watermark Tactical
[`129`](129-bounded-storage-intake-watermark.md) into a measured near-parity
campaign. Tactical `129` is incorporated here and superseded before code
changes. The first change remains its independently bounded intake watermark;
later changes require fresh causal evidence.

Topics: `performance-and-live-evidence`, `storage-throughput-architecture`,
`capability-readiness`, `oracle-driven-engine-campaign`

## Decision And Motivation

Tactical [`128`](128-controlled-tcp-performance-diagnosis.md) proved that the
application-shaped TCP path is not limited first by request depth, checkpoint
sync, activity observation, resume semantics, or too little write/hash
concurrency. On the sustained 1 GiB/16 MiB-piece plaintext row, pinned
libtorrent reached 487.9 MiB/s while the current 64 MiB RSTorrent allowance
reached 332.9 MiB/s (`0.682x`). Reducing only that coupled allowance to 8 MiB
raised RSTorrent to 394.4 MiB/s (`0.808x`) and reduced storage-job high water
from about 3,083 to 399. Forced RC4 improved in the same direction from
`0.762x` to `0.849x`.

That control changes the resident safety ceiling, intake pressure, and job
capacity together. It selects storage admission but not an 8 MiB product
memory limit. It also leaves a material gap. Maintainer direction now requires
the campaign to continue through causally selected storage-path work rather
than stop after the first 10% improvement.

For this tactical, **near-parity** means an application-shaped RSTorrent
median of at least `0.95x` the same-cohort pinned-libtorrent median on both
matched plaintext and forced-RC4 1 GiB/16 MiB-piece transfers. This is a
same-host controlled engineering threshold, not a public-swarm promise or a
portable CI speed floor.

## Scope And Stopping Condition

This tactical owns the receive-to-verified-storage path in measured stages:

1. separate a hysteretic queued/running storage-intake byte watermark from
   the larger resident-payload emergency ceiling and block-count channel cap;
2. retain diagnostics that distinguish logical blocks, physical writes,
   coalescing, queue wait, plan/dispatch/completion work, write service, hash
   input, CPU, RSS, and terminal ownership;
3. if the post-watermark result remains below the gate, use one-variable
   controls to decide whether bounded write/task amortization is material;
4. if page-cache rereads or the write-complete hash fence remain material,
   add generation-safe pending-write read-through without whole-piece
   assembly or unchecked verification; and
5. repeat the matched geometry after every retained optimization. Peer-loop,
   framing, request, or encryption work enters this tactical only if storage
   evidence has fallen below the remaining gap and a recorded profile selects
   that exact boundary.

The tactical completes only when all of the following hold:

- four or more alternating repetitions place the primary plaintext and
  forced-RC4 medians at or above `0.95x` pinned libtorrent;
- 256 KiB, 1 MiB, and 4 MiB non-regression rows remain at least `0.90x`
  libtorrent and no retained RSTorrent row regresses more than 5% from its
  applicable pre-change control;
- the same request target, connection count, TCP-only transport, MSE method,
  materialized fixture, cache convention, and completion boundary apply to
  both clients;
- every output passes independent piece and whole-file verification,
  publication, joined shutdown, and cleanup with zero uTP peers, failed bytes,
  or redundant bytes in the retained one-peer case;
- resident payload, session/root resources, write/hash/read concurrency,
  descriptors, queues, CPU, RSS, and tail service are recorded and remain
  within declared bounds;
- delayed storage, large pieces, two torrents, cancellation, failure,
  checkpoint, selection, and publication tests prove integrity and liveness;
- both Android ABIs build and complete repository validation passes; and
- the owning topics and this evidence record distinguish adopted changes,
  rejected hypotheses, remaining limitations, and exact source fingerprints.

A candidate that improves one row but misses these gates remains evidence, not
a new default. The campaign does not lower the target to match an
implementation result.

## Stable Invariants And Resource Bounds

- Accepted peer payload is charged exactly once until every consumer releases
  it. Queue, write, and optional hash references share ownership without
  duplicating the resident charge.
- The ordinary intake high watermark is independent from the larger emergency
  resident cap. High/low hysteresis bounds thrashing; overshoot is limited to
  already accepted blocks and executing operations.
- One valid piece larger than the ordinary watermark retains one bounded
  liveness exception. It cannot multiply per peer, torrent, or block.
- Backpressure pauses new payload intake while storage completions,
  discovery, selection, upload, cancellation, and shutdown owners continue.
- Coalescing crosses no physical destination, gap, route generation, piece
  generation, overlap, padding, or control fence. Every logical block retains
  one completion and exact selected/part accounting.
- A pending-write hash may consume only immutable accepted bytes from the
  matching torrent, piece, generation, and exact range. Missing ranges fall
  back to storage only after their write completion makes that read safe.
- Hash pass and write success remain independent facts. A piece becomes
  verified only after the complete generation join succeeds; a write failure
  cannot be concealed by a matching hash.
- Hashing, checkpoint durability, trusting resume, publication, selection
  changes, and coarse namespace fences preserve their existing authority.
- Session/root fairness and the aggregate desktop/Android request, payload,
  active-piece, operation, and 40-handle limits remain authoritative.
- No per-block log, unbounded timeline, benchmark-only product mode, or public
  setting is introduced.

The initial watermark sweep uses 1, 2, 4, 6, and 8 MiB high values with a
documented low-water fraction while holding the existing desktop and Android
resident caps constant. Subsequent batch, dispatch, worker, or read-through
controls change one declared variable at a time and retain the selected
watermark.

## Source-First Record

No reference source, fixture, or test is copied.

Pinned libtorrent `2.0.13.0` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` is the implementation and test
oracle:

- `include/libtorrent/settings_pack.hpp::max_queued_disk_bytes` documents
  stopping peer reads while disk work waits. `src/settings_pack.cpp` defaults
  it to 1 MiB, `aio_threads` to ten, and `hashing_threads` to one.
- `src/disk_buffer_pool.cpp::{allocate_buffer,check_buffer_level,
  set_settings}` derives a fixed-block pool, enters pressure at its hysteretic
  high point, resumes below half, registers weak per-peer observers, and
  bounds overshoot to accepted per-peer blocks.
- `src/peer_connection.cpp::{incoming_piece,on_disk,on_disk_write_complete,
  can_read}` copies an accepted block into the disk owner, stops further
  payload reads under disk pressure, accounts queued bytes, and resumes from a
  completion/observer transition without shrinking the request policy.
- `src/mmap_disk_io.cpp::{async_write,add_job,submit_jobs,thread_fun,
  do_write,do_hash}` queues one fixed block, retains it in a piece/offset store
  buffer through physical completion, and lets hashing consume pending bytes
  before falling back to storage.
- `src/mmap_storage.cpp::{initialize,write}` uses ordinary `pwrite_all` on the
  benchmark's macOS automatic-write path; the runtime binding reports the
  pinned 1 MiB/ten-thread/one-hash-thread defaults. Libtorrent is not winning
  merely by coalescing larger writes or forcing mmap writes.
- `src/torrent.cpp::verify_piece` and the piece-picker transition start hash
  once every block is writing or finished, then join hash and final write
  callbacks independently.
- `test/test_storage.cpp::{both_sides_from_store_buffer,
  first_side_from_store_buffer,second_side_from_store_buffer,
  none_from_store_buffer}` covers reads spanning pending and completed writes
  for both mmap and positional backends.
- `test/test_piece_picker.cpp::{set_piece_priority_passed_hash_check,
  set_piece_priority_passed_hash_check_unfilter,
  set_piece_priority_passed_hash_check_bulk_filter}` covers hash pass before
  final write completion and subsequent state changes.
- `test/test_fence.cpp::{empty_fence,job_fence,double_fence}` covers jobs
  before, behind, and after exceptional storage fences.
- `test/test_alert_types.cpp` distinguishes an exhausted outstanding disk
  buffer from a configured queue too large for useful cache behavior.

RSTorrent's relevant retained implementation history is Tactical
[`032`](032-bounded-coalesced-write-batches.md), Tactical
[`053`](053-immutable-positional-storage-plans.md), Tactical
[`054`](054-bounded-independent-storage-execution.md), and Tactical
[`114`](114-session-wide-concurrent-torrent-admission.md). The current path
already coalesces at most 16 logical blocks/256 KiB, uses immutable positional
plans, runs bounded independent write/hash jobs, and shares session/root
resources. This tactical does not rediscover or replace those owners.

The local JSTorrent sibling remains at commit
`9895410beeed6aff554053769bd006a3fbd373ef`; unrelated untracked documentation
prevents a clean managed status but does not overlap the inspected source.
`packages/engine/src/core/bt-engine.ts::checkBackpressure` separates peer and
verified-write pressure, `core/disk-queue.ts` owns bounded workers, and
`adapters/native/native-batching-disk-queue.ts` plus
`core/torrent-content-storage.ts::tryBatchWrite` separately bound pending and
in-flight bytes and amortize native dispatch. RSTorrent adopts the ownership
questions, not those values, FFI topology, or TypeScript architecture.

## Owner, Task, Cancellation, And Dependency Shape

| Owner | State and work | Termination |
| --- | --- | --- |
| Content supervisor | Piece generations, request eligibility, logical completions, integrity join | Continues non-payload owners under pressure and joins storage before terminal state |
| Storage coordinator | Command intake, ready queues, batch planning, active jobs, queue-byte pressure | Stops admission, releases queued ownership, and joins every running syscall/hash |
| Session resources | Aggregate payload and root/torrent-fair write/hash/read permits | Generation registration and permit drop remain the only capacity authority |
| Write job | Immutable positional spans and shared accepted payload | Returns every logical result; owns no picker, have, or checkpoint state |
| Optional pending-write index | Exact piece-generation ranges and shared payload references | Entries retire after all write/hash consumers release; cancellation drains exactly |
| Hash job | Ordered pending-buffer or positional ranges and private SHA state | Returns a typed generation result and never establishes content directly |
| Checkpoint owner | Existing dirty epochs, sync handles, and merged durable-have commits | Unchanged joined failure and shutdown behavior |

Protocol/layout state remains independent from Tokio, files, channels, and
task handles. Storage planning remains engine-internal. No platform adapter,
application service, or benchmark profile learns about internal queue
mechanics. The concrete boundary improvement starts by replacing
`resident bytes / 16 KiB == storage job capacity` with one named byte policy;
later ownership changes must earn their shape from the retained profiles.

## Measurement And Implementation Sequence

1. **Install the independent watermark.** Add pure high/low transition tests,
   integrate exact queued/running byte accounting, preserve the resident and
   channel emergency caps, and prove large-piece and multi-torrent progress.
2. **Sweep before selecting.** Run alternating 1/2/4/6/8 MiB plaintext rows,
   select the fastest stable non-regressing point, then repeat forced RC4 and
   the small-piece controls.
3. **Attribute the residual.** Record physical-operation and coalescing shape,
   plan/dispatch/completion counts, write/hash queue and service, CPU/RSS, and
   hash bytes reread. Use bounded one-variable controls; do not infer a hot
   path from cumulative time alone when operations overlap.
4. **Amortize measured dispatch work if material.** Retain positional plans,
   generation checks, logical completions, cancellation joins, session
   fairness, and count/byte caps. A persistent worker or wider batch is not
   selected without a causal control.
5. **Add pending-write hash input if material.** Start with a task-free range
   index and complete outcome table, then prove buffer/file mixed input,
   out-of-order blocks, both hash/write completion orders, failure, stale
   generation, cancellation, selective files, part slots, and padding.
6. **Remeasure to the gate.** Repeat the primary plaintext/RC4 cohorts after
   each retained change. Profile and optimize another boundary only when the
   storage projection cannot explain the remaining deficit.
7. **Close all platforms and records.** Run repository, locked interop, both
   Android ABI, resource, cleanup, and documentation gates before declaring
   near-parity.

Implementation commits remain independently reviewable: admission policy,
diagnostics/controls, each retained optimization, and final evidence do not
collapse into one commit.

## Execution Record

### Independent intake policy

The first implementation stage is complete. `DownloadResourceLimits` now
names a storage-intake high watermark separately from the buffered-payload
resident ceiling. The low point is two thirds of high. Existing product
behavior was initially unchanged for the isolating sweep. The storage command
and completion channel capacity continues to derive from the resident ceiling,
not the new ordinary pressure point.

The resumable diagnostic accepts an internal
`--storage-intake-high-watermark-bytes` control, records the resident/high/low
values, and rejects a high point below one block or above the resident cap.
The controlled TCP harness now owns distinct 1/2/4/6/8 MiB cases while keeping
its 64 MiB diagnostic resident allowance unchanged. This is not a persisted
or product-visible setting.

Deterministic validation covers independent high/low transitions, exact
desktop/Android defaults, invalid bounds, restart propagation, and existing
storage saturation/cancellation paths. Focused engine Clippy and tests pass;
one full 502-test engine run had a pre-existing bandwidth timing failure that
passed immediately in exact isolation. The locked controlled-harness and
public-comparator unit suites pass.

### Intake sweep and selection

Clean release commit `b7dadad` ran on the Apple M4 Pro/APFS host with pinned
libtorrent `2.0.13.0`, a 64 MiB diagnostic resident allowance, 4/4 storage
concurrency, one TCP peer, zero uTP peers, and rotating order. Four plaintext
1 GiB/16 MiB-piece repetitions produced:

| Intake high | Median MiB/s | Libtorrent ratio | Payload/job high water |
| --- | ---: | ---: | ---: |
| libtorrent | 494.8 | `1.000x` | n/a |
| 1 MiB | 449.3 | `0.908x` | 1 MiB / 79 |
| 2 MiB | 443.8 | `0.897x` | 2 MiB / 143 |
| 4 MiB | 415.4 | `0.840x` | 4 MiB / 271 |
| 6 MiB | 402.1 | `0.813x` | 6 MiB / 399 |
| 8 MiB | 413.3 | `0.835x` | 8 MiB / 527 |

The 1 MiB point is the fastest stable candidate. Its forced-RC4 median is
341.9 MiB/s against 375.1 MiB/s (`0.911x`). Plaintext 256 KiB, 1 MiB, and 4
MiB-piece rows reach `0.945x`, `1.001x`, and `0.933x`, so each clears the
secondary `0.90x` floor. All 56 outputs passed independent piece/whole-file
verification, exact transport/method evidence, publication, joined shutdown,
and cleanup with no failed or redundant payload.

Desktop and Android therefore adopt 1 MiB high and 699,050-byte low intake
hysteresis while retaining 32 and 16 MiB resident ceilings. One 1 MiB run
packed 65,536 logical blocks into 6,294 write jobs and reached 480.6 MiB/s;
the other runs used 10,647--10,794 jobs and were slower. This selects bounded
write-batch fill as the next causal control. It does not select more workers,
filesystem changes, or pending-write hashing yet.

### Rejected cooperative batch fill

Two bounded cooperative-fill controls tested whether more consistent job
packing caused the remaining gap. Filling only while another write was active
left the ordinary 6.0--6.1 blocks/job shape unchanged and reached `0.875x`
libtorrent. Giving every partial batch up to 16 scheduler turns increased fill
to 11.7--13.2 blocks/job and reduced write jobs to 4,954--5,618, but its
four-run plaintext median was 449.5 MiB/s against 500.9 MiB/s (`0.897x`). That
does not improve the retained 449.3 MiB/s/`0.908x` baseline and remains below
the gate. Both candidates were removed completely.

This rejects write-job count and opportunistic coalescing as the primary
residual owner. RSTorrent's retained baseline consumes about 1.21 CPU cores
against libtorrent's 1.86, while every 16 MiB piece still waits for all write
completions before a separate full-file read/hash job. The campaign therefore
advances to generation-safe pending-write hash input; more workers, wider
batches, and a persistent write dispatcher remain unselected.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | watermark hysteresis/overshoot; generation join; pending-range ownership if added |
| Storage | adjacent/gapped/cross-file/part/padding writes; mixed buffer/file hash; short/error paths |
| Runtime | delayed write/hash, queue saturation, two torrents/two roots, cancellation, failure, checkpoint, selection, publication |
| Controlled | alternating plaintext and RC4 1 GiB/16 MiB primary; 256 KiB/1 MiB/4 MiB non-regression; same pinned seed/profile |
| Resources | request and payload bytes, queued/running jobs/bytes, physical writes, batching, hash input, CPU, RSS, handles, terminal zero ownership |
| Platforms | desktop path semantics plus Android x86_64 and arm64-v8a cross-builds; no physical device unless a later platform-specific change requires one |
| Repository | formatting, warning-denying workspace Clippy, workspace tests, locked comparator/harness tests, documentation links and clean tree |

## Non-Goals And Escalation Gates

This tactical does not enable or tune uTP, change tracker/DHT discovery,
benchmark public swarms, change transfer-rate policy, expose a storage setting,
lower the total product memory ceiling, add direct I/O, memory mapping,
`io_uring`, unsafe code, or a dependency, or promise performance parity on
Android providers and arbitrary disks. It does not weaken piece verification,
checkpoint durability, trusting-resume requirements, selection fencing, or
publication semantics.

A new dependency, unsafe or platform-specific backend, persisted/product
setting, altered integrity authority, or physical/external operation remains a
human review gate. Ordinary internal refactoring, deterministic tests,
controlled loopback runs, and both Android cross-builds are authorized by this
tactical.
