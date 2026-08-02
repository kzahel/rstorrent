# Tactical 054: Bounded Independent Storage Execution

Status: Active on 2026-08-02.

Topics: `storage-throughput-architecture`, `download-correctness`,
`disk-and-piece-inspection`, `performance-and-live-evidence`,
`capability-readiness`, `oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `053` made every ordinary payload write and piece hash an immutable,
generation-checked positional plan, but `run_content_storage_task` still owns
one FIFO and awaits one operation. A queued verify stops write batching, a hash
prevents every unrelated write from running, and only one hash can execute.

Replace that execution barrier with one bounded coordinator that admits write
and hash jobs independently, preserves exact payload ownership, and returns
out-of-order results to an explicit piece-generation join. The initial safe
stage keeps a stronger rule than libtorrent: a piece becomes hash-eligible only
after all of its own writes completed successfully. Writes for other pieces
and hashes for completed pieces may overlap, and several jobs of each kind may
run at once.

The stopping condition is an end-to-end download whose selected concurrency
keeps eligible write/hash capacity busy, never establishes stale or partially
written content, shuts down with every blocking job joined, and improves or
explains the retained 128 MiB engine and SQLite-backed application profiles.

## Stable Scenarios

- Holding one write job does not stop another disjoint write up to the selected
  write limit, or a hash for a different write-complete piece.
- Holding one hash does not stop later disjoint writes or another eligible hash
  up to their independent limits.
- A piece hash is never dispatched while one of that generation's writes is
  queued, running, failed or awaiting logical completion.
- Write and hash completions may arrive out of submission order. Only the
  matching piece and generation can change picker, have, checkpoint,
  contributor or inspection state.
- A write or hash failure fences that generation. Re-requesting the piece uses
  a new generation after every old job has joined; a late old completion is
  rejected and cannot verify or reset the replacement.
- Coalesced physical writes retain one completion per logical block, one
  resident-payload charge per accepted buffer and the existing 16-block/
  256 KiB batch caps.
- Cancellation closes admission, releases queued ownership exactly once,
  awaits every running blocking filesystem call, and leaves no job or payload
  accounting behind.
- Publication, materialization, relocation, selection changes and deletion
  remain coarse post-join fences.

## Normative And Reference Dossier

No reference source, test, fixture or queue implementation is copied.

- BEP 3 at `reference/bittorrent.org/beps/bep_0003.rst` remains the integrity
  authority: v1 piece hashes cover concatenated torrent bytes, and content is
  not authoritative before a trusted piece hash matches. BEP 47 at
  `reference/bittorrent.org/beps/bep_0047.rst` keeps padding synthetic.
- Pinned libtorrent `2.0.13` at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d` is the primary completeness
  oracle. `include/libtorrent/settings_pack.hpp` and
  `src/settings_pack.cpp` define ten generic `aio_threads` and one additional
  `hashing_threads` thread by default; ordinary download hashes use the generic
  pool, while the hash pool is for full checking.
- `src/mmap_disk_io.cpp::{add_job,submit_jobs,queue_for_job,pool_for_job,
  thread_fun,do_hash}` supplies bounded generic/hash pools, dispatch and the
  fixed-block hash read path. `src/peer_connection.cpp::incoming_piece` and
  `src/torrent.cpp::verify_piece` begin verification when all blocks are
  writing or finished. That earlier trigger is safe there because the store
  buffer is inserted before write dispatch and `do_hash` consumes it before
  reading storage; RSTorrent intentionally defers that behavior.
- `src/disk_job_fence.cpp` and
  `test/test_fence.cpp::{empty_fence,job_fence,double_fence}` are the ordering
  oracle for joining old work, blocking affected later work and releasing it
  in sequence. This tactical retains coarse post-join control fences rather
  than importing the generic fence graph.
- Pinned rqbit at `4e5f94cbcf1d57ec500885c77cf1e24d70232d89`
  offers a second Rust comparison. `crates/librqbit/src/spawn_utils.rs` applies
  one global semaphore to blocking work; `crates/rqbit/src/main.rs` defaults it
  to eight; and `torrent_state/live/mod.rs::write_to_disk` writes, completes
  piece ownership and hashes a final piece under a per-piece lock inside that
  bounded blocking path. RSTorrent keeps distinct write/hash admission and
  does not hold swarm state across I/O.
- Local JSTorrent sibling HEAD
  `9895410beeed6aff554053769bd006a3fbd373ef` uses six workers in
  `packages/engine/src/core/disk-queue.ts`, with pending/running snapshots and
  drain/clear semantics. Its Android
  `adapters/native/native-batching-disk-queue.ts` dispatches at most 128 writes
  or 4 MiB per bridge batch and tracks pending versus in-flight bytes. Those
  values are historical comparisons, not RSTorrent defaults.

Intentional differences are a write-complete hash fence, no pending-write
read-through, no memory mapping, no direct I/O, no `io_uring`, no unsafe code,
no new dependency and no general session-wide disk pool in this slice.

## State, Owner And Dependency Shape

Runtime-independent swarm state gains a monotonically advancing nonzero piece
generation. Accepted write commands, immutable plans, job results and verify
commands carry it. Hash failure advances it only after every job for the old
generation has joined. Overflow is a typed terminal invariant failure rather
than wrapping into an identity that could match stale work.

One runtime-independent join per active piece records:

```text
PieceStorageJoin {
    piece, generation,
    writes_expected, writes_succeeded, write_failed,
    hash: not_started | running | passed | failed,
}
```

The initial write-complete fence means a runtime hash pass cannot normally
precede final write success, but the join implements and tests the complete
outcome table so a later pending-write stage cannot make channel order into an
integrity assumption.

| Generation writes | Hash outcome | Join result |
| --- | --- | --- |
| All succeeded | Passed | Establish once and enqueue checkpoint intent |
| All succeeded | Failed | Fence, attribute and advance generation |
| Any failed | Any | Never establish; join old work before reset |
| Outstanding | Passed | Retain pass; wait for final write result |
| Outstanding | Failed | Retain failure; join old work before reset |

| Owner | Mutable state | Work and termination |
| --- | --- | --- |
| Content supervisor | Swarm transitions, piece generations, joins, have and contributor effects | Consumes typed results in any order; it alone establishes or resets a generation. |
| Storage coordinator task | Storage planners, ready queues, active-job sets and admission limits | Serializes only mutable plan preparation; dispatches owned positional jobs; on close drains, and on cancel stops admission then joins running calls. |
| Write job | One or more validated coalesced plans and immutable payloads | Performs only positional writes and returns each logical member result. |
| Hash job | One immutable piece hash plan and fixed private read buffer | Performs only positional reads/SHA-1 and returns hash plus durability targets; it does not mutate selection or verified state. |
| Checkpoint owner | Dirty epochs and stable sync handles | Unchanged; begins only after the supervisor's successful join. |

Protocol/layout crates remain independent of Tokio, files and task handles.
Storage modules prepare owned engine-only jobs; `driver.rs` owns scheduling and
the runtime-independent join may live with swarm state or in a focused
task-free module if that makes the transition table clearer.

## Bounds And Admission Policy

The command channel, two-command supervisor pending queue, received-payload
budget, 16-block batch count and 256 KiB batch byte limit remain unchanged.
Queued and running jobs continue to be charged by those existing owners.

The first implementation accepts one through eight write jobs and one through
eight hash jobs. Development starts at four writes and four hashes. Controlled
cohorts sweep `1/1`, `2/2`, `4/4`, `8/4` and `8/8`; the landed internal
desktop default is the fastest non-regressing setting whose accounting,
service tails and cleanup remain stable. Android begins no higher than `4/2`
until cross-build and deterministic descriptor coverage pass. The total
concurrent fixed hash-buffer allocation is therefore at most eight times
16 KiB, while write buffers are already included in the received-payload
budget.

The sweep override is diagnostic/test-only and bounded at eight. This tactical
does not add a public product setting. Session/root fairness and device-aware
adaptive limits remain the next scheduler layer; the single active-download
session means torrent-local limits are currently the real execution boundary.

Admission is work-conserving across the two classes without priority polling:
fill each available class from its own ready queue, then await a command,
completion or cancellation. Completion delivery must not occupy an execution
permit, so a saturated completion channel cannot deadlock admission shutdown.

## Implementation And Validation Sequence

1. Add the task-free piece generation/join and exhaustive transition tests:
   success, mismatch, write failure, both completion orders, duplicates,
   stale generation, reset and overflow.
2. Split single and selective storage into serial plan preparation plus owned
   write/hash execution. Move `record_verified` and durability-target effects
   back to the coordinator/supervisor after a matching successful result.
3. Replace the FIFO executor with independently bounded ready/active sets.
   Preserve batching and one logical completion per block. Add exact active
   counts, high-water marks and oldest active ages for both classes.
4. Add paused-job runtime tests proving write/write, hash/hash and write/hash
   overlap; same-piece fencing; out-of-order completion; injected failures;
   completion-channel saturation; cancellation join; exact byte/item release;
   and enforcement at one and maximum capacity.
5. Re-run all single/selective/part-file, mixed-source, hash-failure, resume and
   crash matrices. Pass formatting, warning-denying workspace clippy/tests,
   generated-contract stability, web gates and Android Rust cross-builds.
6. Run exact 128 MiB engine cohorts for every declared sweep point, then three
   final runs at the selected value. Run the SQLite-backed application cohort
   with the selected value and retain executable fingerprints, transfer and
   service timing, active high-water, physical writes, exact hashes,
   publication, revisions and cleanup.
7. Run the controlled paired interop scenario. A public Big Buck Bunny smoke
   is authorized only after deterministic and controlled gates pass; it is
   headless, bounded by the existing campaign policy and must clean its owned
   download. Public peer supply is contextual evidence, not a pass threshold.

## Large-Geometry Baseline Checkpoint

Commits `2fa5c2c` and `516ab64` landed the task-free generation join and the
independently bounded write/hash executor at the initial `4/4` desktop limits.
Commit `dd92643` then added
`tests/interop/local_throughput_compare.py`, a controlled single-file loopback
comparator with a pinned libtorrent `2.0.13` seeder. It materializes a
deterministic non-sparse source, rotates client order, excludes fixture and
whole-file validation time from transfer time, requires exact byte counts and
full-file SHA-1, and removes each output immediately. Its retained matrix is
1 GiB and 10 GiB at 256 KiB, 1 MiB, 4 MiB and 16 MiB pieces.

This matrix is now the first performance gate for the remainder of the
tactical. Schema `2` accepts several bounded `WRITE/HASH` storage points
against one libtorrent client observation per workload, rotates their order
across repetitions, identifies each point in every raw result and emits cohort
medians plus RSTorrent/libtorrent ratios. Optional throughput and ratio floors
turn the measurement into an executable failing gate without imposing this
machine's hardware target on other environments. On this machine the retained
large-transfer acceptance command uses three runs and a 170.667 MiB/s
RSTorrent floor, equivalent to 10 GiB in 60 seconds. No optimization or
desktop-limit change graduates if any piece-size row misses that floor, exact
bytes, whole-file SHA-1, publication or cleanup.

```bash
cd tests/interop
uv run python local_throughput_compare.py \
  --sizes-mib 1024 10240 \
  --piece-sizes-kib 256 1024 4096 16384 \
  --runs 3 \
  --storage-points 4/4 \
  --minimum-rstorrent-mib-s 170.667 \
  --output /tmp/rstorrent-large-baseline.json
```

The first 1 GiB/256 KiB RSTorrent case made no useful completion progress in
more than four minutes while one core remained saturated. A three-second
process sample attributed the main-thread work to `observe_swarm` and
`SwarmState::snapshot`, which walked the complete piece/block geometry after
each peer or storage event. Replacing derived whole-geometry queries with
checked phase counts and bounded indexes, and publishing the full diagnostic
snapshot on a 100 ms maintenance cadence, reduced the 32 MiB controls from
14.5 to 74.0 MiB/s at 256 KiB pieces and from 18.7 to 219.3 MiB/s at 1 MiB
pieces.

The first 10 GiB fixture then exposed two ordinary-capability guards before it
could transfer: the generic 512 KiB bencode string limit rejected its 819,200
byte v1 `pieces` string, and the old single-file path rejected total lengths
above `u32::MAX`. Metainfo now allows the complete existing 1 MiB input budget
to hold piece hashes, deriving a still-bounded 52,428-piece ceiling, while the
single-file runtime uses its existing 64-bit layout and storage offsets. A
deterministic 10 GiB/256 KiB geometry test covers both conditions. This remains
much tighter than pinned libtorrent's 30 MiB default `max_metadata_size`; it is
the smallest bound change required by this baseline.

Two later 10 GiB samples found and removed additional geometry-dependent
supervisor work. Contributor-history pruning scanned every block after every
verified piece; an incremental per-connection unverified-block count replaced
it. Scheduling then spent 82% of sampled main-thread time scanning active
pieces that had no missing block. A generation-safe
`requestable_active_pieces` index now contains only active pieces with a
currently missing request; assignment removes exhausted pieces and every
retry transition refreshes membership. Test-only recomputation independently
checks phase counts, active attempts, per-peer request and contributor counts,
incomplete/active/requestable piece sets, and their byte totals across
endgame, hash failure, retry, completion and cancellation.

The final single-observation matrix used RSTorrent executable SHA-256
`1ac603546048301173505dc784b77a073379878bb6642c339ab240f3d95fa097`, a
64 MiB payload allowance and `4/4` storage execution. Times exclude source
construction, torrent hashing and final SHA-1 validation:

| Size | Piece | RSTorrent time | RSTorrent MiB/s | libtorrent time | libtorrent MiB/s | RST/libtorrent |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 GiB | 256 KiB | 2.135 s | 479.5 | 2.143 s | 477.9 | 100.3% |
| 1 GiB | 1 MiB | 1.604 s | 638.5 | 2.107 s | 485.9 | 131.4% |
| 1 GiB | 4 MiB | 1.680 s | 609.5 | 2.030 s | 504.5 | 120.8% |
| 1 GiB | 16 MiB | 2.992 s | 342.3 | 2.114 s | 484.4 | 70.7% |
| 10 GiB | 256 KiB | 30.042 s | 340.9 | 28.243 s | 362.6 | 94.0% |
| 10 GiB | 1 MiB | 20.893 s | 490.1 | 19.670 s | 520.6 | 94.1% |
| 10 GiB | 4 MiB | 17.151 s | 597.0 | 10.678 s | 959.0 | 62.3% |
| 10 GiB | 16 MiB | 35.451 s | 288.9 | 10.798 s | 948.4 | 30.5% |

All 16 client transfers matched their expected full-file SHA-1, reported the
exact payload byte count, had zero failed and redundant bytes, published the
complete output and cleaned it. The hardest small-piece row now finishes in
30.0 seconds versus 119.5 seconds immediately before the requestable-piece
index and more than four minutes before the first profile-guided correction.

This is a reproducible scaling screen, not a stable parity claim. It is one
observation per point with an explicitly warm, uncontrolled operating-system
page cache, and later libtorrent rows reached nearly 959 MiB/s. The 4 MiB and
especially 16 MiB RSTorrent rows retain material write-service gaps; the
16 MiB case accumulated 125.958 seconds of write service across four workers
and only one active hash, while the 256 KiB case accumulated 75.505 seconds
of write and 74.252 seconds of hash service. The next Tactical `054` gate is
therefore the declared raw-stage/concurrency sweep and repeated controlled
cohort, not a claim that the integrated pipeline is already graduated.

A first shared-fixture three-run discriminator at 10 GiB/4 MiB then compared
`4/4` and `8/4` in the same process. The `4/4` wall-time median was 23.631
seconds (433.3 MiB/s) and the `8/4` median was 16.136 seconds (634.6 MiB/s),
versus libtorrent's 9.627-second median (1,063.6 MiB/s). Individual RSTorrent
runs still ranged from 15.477 to 27.850 seconds. Median summed write service
was 76.464 seconds at `4/4` and 71.065 seconds at `8/4`; median hash service was
13.321 and 12.565 seconds respectively. This makes `8/4` a candidate for the
full matrix, not a selected default: the retained rotation exposed enough
cache/order variance that every declared point still needs the common
piece-size screen and the finalists need repeated cohorts.

## Raw Storage Ceiling Checkpoint

`rstorrent-storage-stage-profile` and
`tests/interop/storage_stage_profile.py` now retain the hardware-ceiling side
of the graduation contract. The Rust probe reuses the engine's exact
positional I/O helper, 16 KiB hash reads and SHA-1 implementation. It measures
raw writes, warm file-backed hashing, in-memory hashing and a bounded combined
pipeline that dispatches a piece hash only after all of that piece's writes
complete. The Python driver rotates declared worker points, fingerprints the
executable, emits cohort medians and requires exact operation counts, full
allocation, matching hashes, bounded ready backlog and complete cleanup.

The default raw workload deliberately permutes all 256 KiB write spans across
the 10 GiB file with a deterministic bijection. This is a harder and more
representative positional workload than contiguous file construction. The
ready channel capacity is `write + hash`; reported backlog additionally counts
at most one blocked completion per writer, so its checked high-water bound is
`2 * write + hash`. Sync runs after the transfer-like wall interval and is
reported separately. The profile labels its reads as warm OS-page-cache
observations; it does not claim an unmeasured cold-cache result.

```bash
cd tests/interop
uv run python storage_stage_profile.py \
  --size-mib 10240 \
  --piece-size-kib 4096 \
  --write-chunk-kib 256 \
  --write-order permuted \
  --storage-points 1/1 2/2 4/4 8/4 8/8 \
  --output /tmp/rstorrent-storage-stage-profile.json
```

The first bounded 10 GiB/4 MiB/256 KiB-write observation used raw-profile
executable SHA-256
`2b82632cdbb2869f6d68ed82bf0247ed59316645365e4c7b4f6e95e48af65f31`:

| Point | Raw write MiB/s | Warm file SHA-1 MiB/s | Memory SHA-1 MiB/s | Combined MiB/s |
| --- | ---: | ---: | ---: | ---: |
| `1/1` | 4,018.5 | 1,278.7 | 1,386.6 | 1,017.4 |
| `2/2` | 3,647.7 | 2,385.9 | 2,679.3 | 1,008.0 |
| `4/4` | 3,447.6 | 4,515.1 | 5,217.5 | 2,020.9 |
| `8/4` | 3,373.1 | 4,510.1 | 5,216.9 | 1,987.3 |
| `8/8` | 3,369.9 | 8,051.7 | 10,305.8 | 2,237.1 |

Every row materialized both 10 GiB files, executed 40,960 writes and 2,560
hashes per hash stage, matched every expected piece hash, respected its ready
bound and removed both files. This falsifies raw SHA-1 capacity as the current
bottleneck. The shared integrated 10 GiB/4 MiB `4/4` median of 433.3 MiB/s is
only 21.4% of the equivalent raw combined observation; even the isolated
650.2 MiB/s `8/4` row is 32.7% of its raw point. The next action is repeated
raw-finalist evidence plus an integrated process profile that explains the
large write-service inflation before selecting a desktop default.

Validation at this checkpoint passed all 173 non-live engine library tests
with three live-network tests ignored, the focused metainfo geometry tests,
warning-denying Clippy for protocol and engine, and the comparator's exact
integrity and cleanup assertions. Full workspace gates remain required before
graduation.

## Escalation And Next Boundary

Ordinary refactoring, internal naming, tests, concurrency selection within the
declared range, measurement-driven tightening, same-boundary bug fixes and
documentation/contract regeneration do not require maintainer feedback.

Stop only if evidence requires a new dependency, persistence/part-file format
change, public product setting, session-wide multi-torrent policy, destructive
data action, visible application or physical-device interaction, or a material
scope expansion beyond this coordinator and generation join.

After graduation, measure whether page-cache rereads remain material. Only
then author pending-write read-through and earlier hashing; session/root-aware
fairness follows when the session can run multiple downloads concurrently.
