# Disk And Piece Inspection

Topic: `disk-and-piece-inspection`

Status: Accepted direction; implementation is owned by Tacticals `044` and
`045`.

## Scope

This topic owns the detailed web/desktop inspection model for storage pressure
and piece progress. The two views share engine facts but answer different
questions:

- **Disk** is a session-scoped, statistics-first view of the complete
  receive-to-storage pipeline, its capacity, pressure, throughput, failures,
  and active piece-level work.
- **Pieces** is a selected-torrent, read-only visual overview of verified and
  current piece lifecycle state.

The inspection contract must describe a performant storage architecture rather
than freeze today's implementation into the UI. In particular, it must remain
valid when the current torrent-local storage owner becomes one input to a
session-wide scheduler with controlled concurrency and fairness.

## Accepted Information Hierarchy

Disk is global. It belongs with other session concerns in the right-side tab
group and remains useful with no selected torrent. Its rows may carry a torrent
identity and the client may filter them locally, but there is one session Disk
view rather than parallel `session_disk` and `torrent_disk` authorities.

Pieces is torrent-specific. It stays in the selected-torrent detail group and
is retained only while that detail is of interest. Android may continue to
present the same semantic state through its distinct native UI; Tactical `045`
changes shared contracts without requiring a web-shaped Android screen.

## Shared Vocabulary

The storage pipeline uses these nouns consistently:

1. **requested**: payload promised by outstanding peer requests;
2. **resident**: received payload bytes still owned in bounded memory;
3. **queued write**: admitted storage work not yet executing;
4. **writing**: a storage write operation currently executing;
5. **stored**: accepted content persisted to its staging destination;
6. **hashing**: a complete piece being verified from storage;
7. **verified**: content whose piece hash is accepted and durable have state is
   eligible to advance.

`requested` is not memory consumption. `received` is a cumulative counter while
`resident` is a gauge. `stored` is not `verified`. Every exposed quantity must
identify itself as a current gauge, configured limit, cumulative counter,
duration, or sampled rate.

The finest Disk table identity is one piece attempt. The UI never receives one
row per 16 KiB block and never receives payload buffers. A piece attempt may
move through receiving, queued, writing, hashing, verified, failed, or cancelled
states. Individual block ownership remains inside the engine.

The Pieces view materializes a compact piece-state array in the client from one
coherent snapshot and typed changes. It must not fetch or replace the entire
piece collection on every block or verified transition. Active piece state is
sparse and bounded; verified state is represented compactly as ranges or a
packed bitmap on the wire and as a typed array for painting.

## Ownership And Dependency Direction

```text
peer/scheduler owners       storage owner(s)       piece verifier/checkpoints
          \                       |                         /
           +---------- immutable runtime facts -----------+
                                      |
                         application retained projection
                                      |
                     leased session/torrent view contracts
                                      |
                     validated TypeScript / Zustand replica
                                      |
                    Disk DOM summary/table or Pieces canvas
```

- Scheduler, payload, storage, and verification owners remain authoritative.
- Engine snapshots are immutable observations. They do not create a second
  command path or mutable queue.
- The application projection aggregates all active torrent owners into a
  session Disk view. The current one-download application is an implementation
  limit, not part of the public vocabulary.
- Diagnostics remain ordered explanatory events. Neither view parses log text
  into state.
- The web UI does not dictate write size, batching, caching, hash buffering,
  thread count, or storage backend architecture.

RSTorrent already hashes a complete v1 piece incrementally from storage rather
than assembling one contiguous piece allocation. Pending write buffers may be
consumed directly by the hash path when available. The inspection work must
preserve that no-full-piece-copy direction and make any remaining bounded
copies measurable rather than introduce a UI-shaped piece buffer.

## Disk View Contract

The session snapshot contains:

- explicit pressure state and whether intake is currently backpressured;
- configured resident-payload and queued-storage limits;
- requested, resident, queued-write, writing, and hashing gauges;
- received, stored, verified, write-operation, hash-operation, and failure
  counters;
- queue-wait and service-duration totals/maxima;
- sampled receive/write/hash rates with an honest observation interval; and
- a bounded keyed collection of active piece attempts, at most one row per
  piece attempt.

The first UI is statistics-first. It shows pipeline occupancy and capacity,
pressure/recovery state, rates, cumulative work, queue/service latency, and a
piece-level active-work table. A short client-side history may support a compact
chart, but long-lived transfer history belongs primarily in the future Speed
view.

Slow storage is ordinary flow control, not a peer error. New payload assignment
must stop at a high watermark, already-promised bounded work may overshoot only
within its explicit request/resident limits, and intake resumes below a lower
watermark. While the storage owner is the gating resource, request deadlines
must not falsely classify a healthy peer as stalled. Cancellation, storage
failure, hash failure, and normal completion still need exact ownership and
observable terminal states.

## Pieces View Contract

The selected-torrent snapshot contains piece count, compact verified state, and
the bounded current piece attempts. Patches contain verified additions/clears
and keyed active-piece upserts/removals. A fresh snapshot replaces the replica
after cursor loss, lease expiry, torrent replacement, or browser suspension.

The web view uses one high-DPI Canvas 2D surface with no DOM node per piece and
no hit testing, selection, tooltip, or navigation contract. Resize and device
pixel ratio changes recompute geometry. Incoming changes mark the canvas dirty;
at most one `requestAnimationFrame` paint is queued. Very large torrents use
bounded deterministic bucketing rather than an unbounded canvas dimension.

The visual distinguishes at least missing, requested, received, stored,
hashing, verified, and failed/retrying state. Colors are not the sole signal:
the view includes a text legend and an accessible aggregate description. It is
an overview, not a piece-control surface.

## Bounds And Lifecycle

- Disk rows are active or bounded recent attempts only; no unbounded job or
  duration history is retained.
- Active-piece range collections are bounded by engine request and
  active-piece budgets. Range validation rejects overlap, reversal, overflow,
  and coordinates beyond the piece length.
- View-set leases, byte queues, one-unacknowledged-batch delivery, cursor reset,
  and fresh-snapshot recovery remain the application-view lifecycle.
- Switching away from Pieces releases its torrent projection and client
  replica. Disk exists only while a client expresses global Disk interest.
- A suspended browser may lose its server lease. Returning to visibility must
  recover through the existing fresh-view-set path without retaining stale
  bitmaps or appending diffs to the wrong snapshot.
- Android may explicitly ignore the new global Disk projection until a native
  presentation tactical, but generated shared contracts and builds must remain
  exhaustive and valid.

## Reference Findings

Pinned libtorrent `2.0.13` revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` separates the useful contract from
its implementation:

- `include/libtorrent/disk_interface.hpp`, `src/mmap_disk_io.cpp`, and
  `src/disk_buffer_pool.cpp` own asynchronous jobs, storage fences, thread
  pools, store buffers, and high/low watermark notification.
- `src/peer_connection.cpp` stops socket reads while the disk observer is over
  its watermark and resumes only after the low-water notification.
- `src/session_stats.cpp` exposes current queued/running read, write, and hash
  jobs, queued write bytes, blocked jobs, operation counts, latency, and cache
  behavior as session metrics.
- `examples/session_view.cpp` presents aggregate disk statistics instead of
  exposing every internal job.
- `test/test_fence.cpp`, `test/test_storage.cpp`, `test/test_read_piece.cpp`,
  and `simulation/disk_io.cpp` exercise fencing, storage behavior, piece reads,
  delayed operations, and watermark callbacks.

RSTorrent adopts explicit pressure feedback, session metrics, storage fences,
and incremental hashing as completeness oracles. It does not copy libtorrent's
mmap/cache/thread-pool architecture or metric names wholesale.

Local JSTorrent revision
`9895410beeed6aff554053769bd006a3fbd373ef` supplies product history:

- `packages/ui/src/tables/DiskTable.tsx` shows pending and running raw jobs;
- `packages/engine/src/core/disk-queue.ts` owns a per-torrent worker queue;
- `packages/engine/src/adapters/native/native-batching-disk-queue.ts` shows why
  a UI tied to that queue shape does not survive platform changes;
- `packages/ui/src/components/PieceVisualization.tsx` demonstrates RAF-driven
  piece drawing; and
- `android/app/src/main/java/com/jstorrent/app/ui/components/PieceMap.kt`
  demonstrates efficient Canvas/bitset rendering and bounded aggregation.

RSTorrent keeps the familiar visibility and visual overview but replaces the
raw job table with semantic pipeline statistics and piece-attempt rows. No
source, asset, or fixture is copied.

## Work Sequence

- [`../tactical/044-global-disk-inspection.md`](../tactical/044-global-disk-inspection.md)
  establishes the global contract, engine observations, pressure behavior,
  application projection, Disk UI, and controlled slow-storage evidence.
- [`../tactical/045-piece-map-visualization.md`](../tactical/045-piece-map-visualization.md)
  then generalizes active-piece projection and implements the selected-torrent
  Canvas overview using the same lifecycle facts.

This ordering is deliberate: storage ownership, pressure, and the shared piece
lifecycle vocabulary are proven before a visualization depends on them.

## Known Gaps After This Sequence

- a session-wide multi-torrent storage scheduler and measured fair concurrency;
- platform-specific filesystem throughput, cache, direct-I/O, and memory policy;
- durable or long-window rate history in the Speed view;
- piece priorities, piece selection commands, and interactive piece details;
- a native Android Disk screen and convergence of its existing PieceMap on the
  generated shared contract; and
- mature disk-space policy, relocation, and broad failure/recovery profiles.
