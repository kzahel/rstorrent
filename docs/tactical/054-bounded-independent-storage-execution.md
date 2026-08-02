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
