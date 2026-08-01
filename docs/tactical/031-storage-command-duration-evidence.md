# Tactical 031: Storage Command Duration Evidence

Status: Complete

Topics: `performance-and-live-evidence`, `download-correctness`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

Tacticals `025`, `028`, `029`, and `030` repeatedly observed the fixed 66-job
storage high-water mark. Asynchronous ownership and selective hash operation
changes preserved correctness but did not improve the controlled or public
profiles. A full queue proves bounded buffering, not that filesystem service is
the limiting owner. At milestone cancellation it may simply contain accepted
future blocks while network or request policy controls verified throughput.

Measure each storage command's queue wait and service duration, separately for
16 KiB writes and whole-piece verification. Retain cumulative time, counts,
maxima, the current operation kind and age, and exact queue/payload bounds in
the headless diagnostic snapshot and comparator timeline. Use controlled
delays to prove attribution, then use a public screen to choose the next
behavioral tactical from evidence.

## Source Dossier

Pinned libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` is the completeness oracle. No
source or fixture is copied.

- `posix_disk_io.cpp::async_write` measures the complete storage write and
  increments `num_blocks_written`, `num_write_ops`, `disk_write_time`, and
  `disk_job_time` before posting one completion.
- `posix_disk_io.cpp::async_hash` measures the complete piece hash and
  increments `num_blocks_hashed`, `disk_hash_time`, and `disk_job_time`.
- `mmap_disk_io.cpp::perform_job`, `do_write`, and `do_hash` distinguish queued,
  running, write, and hash work and publish cumulative performance counters.
- `performance_counters.hpp` defines `disk_write_time`, `disk_hash_time`,
  `disk_job_time`, `queued_disk_jobs`, `num_running_disk_jobs`, and per-kind
  job counters. These are attribution evidence, not scheduling policy.
- `test/test_session.cpp::session_stats` checks the contiguous metric index
  contract and name lookup used to publish those counters.
- `simulation/test_swarm.cpp::session_stats` obtains counters from a bounded
  `session_stats_alert`, and `test/setup_transfer.cpp::get_counters` maps the
  published names back to their metric indexes. These cases require runtime
  observability without making counter publication part of transfer control.

RSTorrent adopts the separation of operation counts, service time, and queue
state. It does not adopt libtorrent's cache, disk pool, thread count, counter
registry, or alerts architecture.

The product-history reference was JSTorrent's
`packages/engine/src/core/disk-queue.ts`,
`packages/engine/src/core/torrent-tick-loop.ts::logBackpressureStats`, and
`docs/archive/performance/download-bottleneck-analysis.md`. Its disk jobs keep
enqueue/start times and bounded pending/running snapshots, while its diagnostic
loop compares disk occupancy with payload rate and other pipeline owners. A
Pixel 7a investigation used that combination to exonerate disk despite 18
running jobs. RSTorrent therefore does not treat queue occupancy alone as a
bottleneck; the live screen must compare service duration with wall time.

## Ownership And Bounds

`ContentStoragePipeline` owns command admission, queueing, execution, and
completion, so it owns these measurements. Each queued command receives one
monotonic enqueue timestamp. The storage task records queue wait immediately
before execution, marks one active operation, measures the complete existing
write or verification future, records service time, and clears active state
before sending the completion.

`DownloadControl` stores fixed atomic counters only: per-kind started and
completed counts, cumulative queue-wait and service microseconds, maxima, and
one active kind/start timestamp. Arithmetic saturates. Snapshotting computes
current age against the download's existing monotonic epoch. No endpoint,
path, peer identity, piece index, command history, histogram, string queue,
new task, channel, lock, or unbounded sample is retained.

Durations are diagnostic observations, not verified content or application
events. They remain separate from structured logs. A command error is still a
completed measured service operation. A command canceled before execution has
queue ownership but no invented service duration. Cancellation clears active
state when the storage task returns; the snapshot never claims a detached job.

## Shape-Changing Edge Cases

- a controlled delayed write increases only write service time, reports a
  current write while active, and leaves hash service unchanged;
- a controlled delayed hash increases only hash service time, reports a
  current hash while active, and leaves write service unchanged;
- commands behind a delayed operation accrue queue wait without being counted
  as started service;
- fast commands may round to zero microseconds, but their counts remain exact;
- failed writes, hash mismatches, and typed hash I/O failures retain exact
  counts without becoming successful verification;
- saturation, integer conversion, and long-running current age saturate rather
  than wrap; and
- cancellation with queued and active work joins the owner, clears current
  operation state, releases payload accounting, and retains no history.

## Staged Implementation And Gates

1. Add fixed per-kind timing state to `DownloadControl` and extend
   `DownloadProgress` with explicit microsecond/count fields.
2. Timestamp command admission and instrument queue-to-start and start-to-end
   transitions in `ContentStoragePipeline` without changing scheduling.
3. Add deterministic/runtime tests using existing write/hash delay controls to
   prove per-kind attribution, queue wait, active age, completion, saturation,
   cancellation, and monotonic snapshots.
4. Extend the public probe and comparator schema/timeline with these owned
   RSTorrent values. Keep unavailable libtorrent fields `null` unless the
   locked binding exposes the corresponding session counters reliably.
5. Pass formatting, warning-denying workspace clippy, workspace tests,
   selective/hash/mixed controlled interop, comparator tests, and paired
   controlled publication.
6. Run three product tracker+DHT 50% screens. A complete screen is required
   only if terminal attribution remains ambiguous.

The tactical completes when queue wait and write/hash service are separately
observable with exact bounded lifecycle tests and live evidence selects one
next owner. If write service consumes material wall time, the next tactical
owns write operation shape or concurrency. If hash service does, it owns
handle reuse or bounded hash concurrency. If both are small while queues stay
full, storage is exonerated and the paired peer/request timeline chooses
request service, piece selection, or useful-peer turnover.

## Implementation And Evidence

`DownloadControl` now owns fixed saturating atomics for per-kind starts,
completions, cumulative/max queue wait, cumulative/max service duration, and
one active operation kind/start time. `ContentStoragePipeline` timestamps
admission, start, and completion around its existing execution path. The
public probe publishes these values in endpoint-free timeline samples and
terminal diagnostics; the libtorrent adapter emits explicit `null` values for
fields its locked Python binding cannot expose. No scheduling or storage
behavior changed.

Controlled delayed-write tests prove the active-write snapshot, exact two-write
and two-hash lifecycle, at least 200 ms of queued wait behind the first write,
and at least 400 ms of cumulative write service. Queued-write and active-hash
cancellation cases prove exact completion, joined ownership, and cleared active
state. Saturation, the public-probe timeline schema, and nullable libtorrent
fields also have direct tests.

The validation gate passed formatting, warning-denying workspace clippy, and
all 253 listed workspace tests: 250 passed and the three opt-in public tests
remained ignored. Selective-hash and mixed-source controlled interop each
published exact content, the nine comparator tests passed, and Android target
checks passed for both `aarch64-linux-android` and `x86_64-linux-android`.
The paired controlled publication completed exactly for both owners. RSTorrent
reported five completed writes and three completed hashes with cleared active
state; libtorrent reported the new owner-specific fields as unavailable rather
than zero.

Three product tracker+DHT Big Buck Bunny screens targeted 50% with a 300-second
limit. One reached 528 of 1,055 pieces in 82.48 seconds; two ended at 425 and
458 pieces. Cleanup and integrity were exact and all terminal active-operation
state was clear. Across the runs:

- write service consumed 87.7%, 88.2%, and 87.7% of wall time;
- hash service consumed 5.5%, 5.5%, and 5.7% of wall time;
- combined serialized service consumed 93.2%, 93.7%, and 93.3%;
- average writes took 8.5, 38.3, and 35.1 ms, while maxima reached 272.5,
  842.0, and 697.4 ms; and
- all runs reached the 66-job bound and accumulated overlapping per-command
  queue wait, with maxima from 0.95 to 8.16 seconds.

Summed queue wait is not compared to wall time because commands wait
concurrently. It proves persistent backlog. Serialized service duration is
comparable to wall time and attributes the retained bottleneck: small writes
are the dominant owner, while hashing is secondary. The stopping condition is
met. The next tactical must inspect the pinned libtorrent write execution and
tests plus JSTorrent's platform write batching, then own write operation shape
or bounded concurrency without weakening integrity, cancellation, payload, or
memory bounds.

## Non-Goals

- changing storage queue capacity, write/hash implementation, concurrency,
  caching, sync policy, request windows, piece selection, peer ranking,
  turnover, connection limits, or discovery behavior
- a general metrics registry, per-command history, histograms, UI/log panes,
  telemetry export, network API changes, or retaining local paths/endpoints
- durable single-file resume, incoming connections, seeding, BEP breadth,
  Tauri, browser, AVD, or physical-device work

## Stopping And Escalation

No human decision is currently required. Stop only for a product-visible
diagnostic contract requiring user design, persistence compatibility break,
new dependency or license posture, destructive user-data action, visible or
physical-device interaction, or evidence requiring a session-wide metrics
architecture. Counter precision limits and public variance are evidence, not
blockers.
