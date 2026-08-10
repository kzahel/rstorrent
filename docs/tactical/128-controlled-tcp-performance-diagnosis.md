# Tactical 128: Controlled TCP Performance Diagnosis

Status: **Completed** on 2026-08-10. Human review explicitly paused further
uTP work and selected a bounded synthetic comparison to identify when and why
RSTorrent is slower than libtorrent. The completed evidence selects Tactical
[`129`](129-bounded-storage-intake-watermark.md); no uTP or public-swarm work
ran.

Topics: `performance-and-live-evidence`, `storage-throughput-architecture`,
`peer-lifecycle`, `capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tactical
[`054`](054-bounded-independent-storage-execution.md) supplies the deterministic
single-file loopback comparator and storage telemetry. Completed Tactical
[`111`](111-mse-peer-stream-encryption.md) supplies matched ordinary-plaintext
and forced-RC4 profiles. Completed Tactical
[`122`](122-paired-public-download-performance-cohorts.md) supplies the
resumable-path milestone, process-resource, integrity, and cleanup schema.

## Decision And Motivation

Public swarms answered useful interoperability questions but mixed discovery,
peer supply, transport, and payload execution. They are a poor instrument for
deciding which engine owner to optimize. This tactical returns to one pinned
libtorrent loopback seed and exact deterministic fixtures, keeps uTP disabled,
and compares the same payload through:

1. RSTorrent's focused direct-metainfo download path;
2. RSTorrent's resumable application-shaped download path; and
3. pinned libtorrent `2.0.13.0` as the leecher.

The work first maps the regimes in which a current release build is behind.
It then varies one causal boundary at a time and selects the smallest owner-
level follow-up justified by repeated evidence. A historical difference is a
hypothesis, not the result: Tactical `054` ranged from parity or better on
several 1 GiB geometries to 69.8% of libtorrent on 10 GiB/16 MiB, while
Tactical `122` measured the resumable 1 GiB/1 MiB control behind the focused
path's earlier result. Both must be reproduced on the current revision before
they guide a change.

## Stopping Condition

The tactical is complete when all of the following hold:

1. One retained harness runs the three owners against byte-identical
   metainfo, payload, seed, TCP-only transport, peer count, encryption policy,
   output filesystem, cache policy, and order rotation.
2. A bounded screen classifies current payload-size, piece-size, and execution-
   path regimes as materially behind, near parity, or ahead. At least three
   alternating repetitions confirm every row used for the final decision.
3. The selected slower row has enough request-window, useful/redundant bytes,
   piece progress, write/hash service and occupancy, process CPU/RSS, and
   phase timing evidence to distinguish network/request supply, swarm
   maintenance, storage/hash service, resumable bookkeeping, and observation
   overhead.
4. Exact piece hashes, whole-output bytes, expected peer-wire method,
   publication, child-process join, and artifact cleanup pass for every
   retained result.
5. Evidence names one bounded next feature/optimization tactical, or records
   that no current difference is repeatable enough to justify one. This
   tactical does not fold that follow-up implementation into its measurement
   result.
6. Focused tests, repository validation, required Android cross-builds for any
   common Rust change, and the owning documentation are reconciled and
   committed.

## Stable Experimental Contract

The primary fixture is a deterministic materialized v1 torrent served by one
pinned libtorrent loopback seed. Discovery, DHT, LSD, trackers, web seeds,
incoming connections, rate limits, and uTP are disabled. The seed and leecher
allow one payload peer. Requests remain BEP 3's 16 KiB blocks. Ordinary
plaintext is the first diagnostic profile; forced RC4 is applied only to a
representative final row after the plain-path owner is known, because Tactical
`111` already found RSTorrent's relative RC4 retention no worse than the
oracle's.

The same fixture is reused within a row. Each output directory starts empty,
is independently piece-verified after publication, and is removed before the
next owner runs. The harness balances which owner runs first. It records warm,
uncontrolled operating-system cache state explicitly and never treats the
result as a disk-device ceiling or CI pass threshold.

The default staged matrix is intentionally short:

1. **Geometry screen:** one 1 GiB single-file pass at 256 KiB, 1 MiB, 4 MiB,
   and 16 MiB pieces through the focused RSTorrent and libtorrent paths.
2. **Path discriminator:** the same 1 GiB/1 MiB single-file fixture through
   focused RSTorrent, resumable RSTorrent, and libtorrent. If the resumable
   delta reproduces, selectively disable or count checkpoint, peer-observation,
   activity-snapshot, and publication work rather than changing their policy.
3. **Scaling discriminator:** repeat the most informative geometry at the
   smallest payload sizes that separate fixed startup cost, per-piece cost,
   and per-byte cost. A 10 GiB case is permitted only if 1 GiB evidence cannot
   distinguish the candidate owners.
4. **Finalist cohort:** at least three order-balanced repetitions of the
   slowest reproducible row and its causal control. Add one single-versus-
   multi-file or plain-versus-RC4 pair only when earlier evidence points at
   that boundary.

Each case has a 45-second owner deadline and a five-second forced-cleanup
allowance. The measured experiment budget after fixture construction is 30
minutes for the complete tactical; a timed-out row is evidence and is not
retried with a longer public-style deadline. At most one 2 GiB materialized
source and three output roots exist concurrently. Captured process output is
bounded to 200 lines per stream and retained reports contain no payload bytes,
peer IDs, or endpoint addresses.

## Measurements And Causal Tests

Every owner reports process-start, peer-ready or first-payload, last-payload,
verification, and publication timing when that milestone exists. The common
result includes wall time, process-tree CPU seconds/core equivalents, peak
RSS, payload bytes, failed/redundant bytes, completed pieces, and the verified
wire method.

RSTorrent additionally records:

- requested bytes, target/outstanding request high-water marks, useful payload
  rate, live peer count, and active-piece high water;
- storage write/hash operation counts, queue and active high waters, summed
  service time, oldest queued/active age, and resident-payload high water;
- bounded scheduler/maintenance work counts and longest observed service gap;
  and
- resumable checkpoint count/service time, activity/peer observation count,
  diagnostic snapshot count, and publication time.

The diagnosis uses ratios rather than raw counters alone:

- low payload supply with idle storage implicates request/window or peer-loop
  service;
- saturated storage with wall time tracking write/hash service implicates the
  storage boundary;
- rising work per piece or block with idle resources implicates swarm or
  observation scans;
- a direct/resumable delta on identical network/storage geometry implicates
  resumable bookkeeping, observation, or publication and must be separated by
  counters or one bounded on/off diagnostic control; and
- a delta confined to large payloads, large pieces, multi-file layout, or RC4
  selects that exact scaling boundary rather than a general throughput claim.

Instrumentation is diagnostic and bounded. Runtime-independent counters remain
with the state owner they describe; the harness samples snapshots and owns
only aggregation. Timing must not add per-block logging or an unbounded event
stream. A control run must show that enabled measurement changes throughput by
no more than 5%, or the instrument is sampled less often before its evidence
is used.

## Source-First Record

No reference source, test, fixture, or benchmark implementation is copied.

- BEP 3 at `reference/bittorrent.org/beps/bep_0003.rst` remains the normative
  wire and integrity authority: requests are 16 KiB blocks, several requests
  should be pipelined for TCP performance, and v1 piece hashes cover the exact
  concatenated torrent byte stream.
- Pinned libtorrent `2.0.13.0` at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d` is the primary oracle.
  `src/peer_connection.cpp::update_desired_queue_size` derives a bounded
  request target from rate, queue time, and block size, including slow start;
  `src/request_blocks.cpp::request_a_block` fills that target and can prefer
  contiguous whole-piece regions. `test/test_piece_picker.cpp` cases
  `pick_whole_pieces`, `prefer_contiguous_no_duplicates`,
  `prefer_contiguous_suggested`, `prefer_cnotiguous_blocks`, and
  `prefer_aligned_whole_pieces` exercise the relevant picker shapes.
- `src/mmap_disk_io.cpp::{async_write,async_hash,do_write,do_hash}` separates
  write and hash job execution. The `mmap_disk_io`, `posix_disk_io`,
  `mmap_unaligned_read_both_store_buffer`, and
  `posix_unaligned_read_both_store_buffer` cases in
  `test/test_storage.cpp` cover both storage backends and pending-store-buffer
  visibility. The `apply_pack` and `clear_single_int` cases in
  `test/test_settings_pack.cpp` cover the request-queue setting and its default
  of 500.
- `tools/run_benchmark.py` is a scale and resource-observation comparison: it
  uses a many-file, many-peer synthetic torrent and samples counters. This
  tactical intentionally keeps one peer and varies one engine boundary at a
  time instead of adopting that topology.
- Local JSTorrent sibling HEAD
  `9895410beeed6aff554053769bd006a3fbd373ef` provides product-history context.
  `packages/engine/integration/python/benchmark_tick.py` measures download
  throughput together with 100 ms scheduler-tick average and tail latency at
  several payload and peer counts. RSTorrent adopts the need to bound and
  observe maintenance latency, not JSTorrent's timer-loop architecture.

Intentional differences from libtorrent remain independent first-party Rust
state and scheduling, no memory mapping, no direct I/O, no `io_uring`, no new
dependency, and no attempt to reproduce its picker or disk architecture.

## Owner, Task, Cancellation, And Dependency Shape

| Owner | State and evidence | Termination |
| --- | --- | --- |
| Python experiment owner | Fixture identity, case order, child processes, deadlines, hashes, reports, cleanup | Stops admission on failure or interrupt, terminates exact children, verifies removal, and emits only complete cases. |
| Pinned libtorrent seed | One torrent, one TCP listener, one payload peer | Removed and session-aborted after each case; the harness joins the worker. |
| Focused RSTorrent download | Existing `DownloadConfig`, one manual peer, common swarm/storage path | Existing cancellation joins peer, storage, and publication owners. |
| Resumable RSTorrent download | Existing `ResumableMagnetDownloadConfig`, checkpoint, peer observation, and activity snapshot owners | Existing control cancellation and torrent-peer handle drain before process exit. |
| Libtorrent leecher | One torrent and matched TCP/encryption policy | Removed and session-aborted before its worker exits. |

Protocol and swarm transitions remain independent of Python, process
telemetry, filesystems, Tokio task handles, and libtorrent. Diagnostic
snapshots flow outward from the engine; the experiment harness never becomes
an engine policy owner.

## Shape-Changing Failure Cases

- A fixture, peer count, transport, encryption method, block geometry, cache
  policy, or output validation differs between owners. Reject the row rather
  than compute a ratio.
- A child times out, crashes, leaves a process, fails a piece/hash check, or
  leaves output. Preserve bounded diagnostics, clean exact owned paths, and
  classify it separately from throughput.
- One-run ordering or cache state changes the apparent winner. Rotate order
  and require the finalist median; do not optimize a single observation.
- Added instrumentation shifts its control by more than 5%. Reduce sampling
  or use existing counters; do not use the perturbed result causally.
- Several owners remain plausible after the bounded matrix. Select the next
  smallest discriminating experiment rather than changing two hot-path owners.
- The evidence points to a product policy, persistent schema, new dependency,
  public protocol claim, or uTP. Stop at the tactical boundary and request a
  separate decision.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Harness | argument/bound tests, timeout and malformed-output tests, order balance, exact fixture/method/hash checks, child and artifact cleanup |
| Deterministic engine | focused counter/state tests for any added bounded diagnostic fields; no policy transition depends on diagnostics |
| Controlled interoperability | current geometry screen, exact three-owner path discriminator, and three-run finalist/control cohort against the pinned oracle |
| Measurement overhead | otherwise-identical diagnostic on/off control within 5% or a documented lower sampling rate |
| Repository | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and locked interop tests proportional to changed harnesses |
| Platforms | Android x86_64 and arm64-v8a cross-builds if common engine Rust changes; no emulator or physical-device run for diagnostic-only work |

## Completed Evidence

### Retained harness and geometry screen

Commits `332f987`, `82436d6`, `edb50bc`, `1a48335`, and `4024ef6` add the
release-only `tests/interop/controlled_tcp_diagnosis.py` harness, exact
piece/whole-file verification, process sampling, phase/checkpoint telemetry,
and narrowly gated causal controls. Each case used one materialized 1 GiB v1
file, one pinned libtorrent TCP seed, one plaintext or forced-RC4 payload
peer, 16 KiB requests, rotating owner order, warm uncontrolled APFS cache,
and complete output removal. Every retained case reported one TCP peer, zero
uTP peers, zero failed/redundant bytes, exact piece and whole-file hashes,
joined termination, and cleanup.

The three-run matched-plaintext geometry screen reproduced a sustained gap:

| Piece size | Focused MiB/s | Focused/libtorrent | Resumable MiB/s | Resumable/libtorrent | Libtorrent MiB/s |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 256 KiB | 402.0 | 0.839 | 435.0 | 0.908 | 479.1 |
| 1 MiB | 446.0 | 0.910 | 424.8 | 0.867 | 490.2 |
| 4 MiB | 431.2 | 0.848 | 388.6 | 0.764 | 508.5 |
| 16 MiB | 414.1 | 0.838 | 330.8 | 0.670 | 494.0 |

Both RSTorrent paths filled the same 500-request/8,192,000-byte request
window. The focused deficit stayed roughly 9--16%; the additional
application-shaped deficit grew with piece size. A 16 MiB-piece focused
storage-concurrency sweep measured `1/1` at 241.2 MiB/s, `2/2` at 403.7,
`4/4` at 390.7, `8/4` at 395.7, and `8/8` at 379.8 against libtorrent at
469.3. Too little concurrency is harmful, but increasing the existing
bounded concurrency is not the missing optimization.

### Scaling and rejected causes

A three-run 16 MiB-piece scaling cohort found the gap only after sustained
work dominates startup. At 64 MiB, focused/resumable/libtorrent medians were
202.8/205.7/77.6 MiB/s. At 256 MiB they were 305.8/333.8/168.1. At 1 GiB they
were 423.1/333.3/484.7, or `0.873` and `0.688` of libtorrent. The small
libtorrent rows include its fixed process/session completion cost and are not
claimed as a byte-path ceiling.

Alternating same-owner controls rejected three tempting explanations:

- bypassing physical checkpoint sync measured `0.988x` enabled throughput
  over six repetitions, despite removing all 16 sync operations;
- summary activity observation measured `0.974x` detailed per-block milestone
  observation, within the 5% instrumentation bound and in the wrong direction;
  and
- through the same probe and peer evidence, the resumable path measured
  338.1 MiB/s versus 306.5 for the nonresumable path (`1.103x`). Resume and
  checkpoint semantics therefore do not explain the general application-
  shaped gap.

The resumable phase trace showed that payload intake and storage overlap:
last storage completed only about 20--33 ms after the last payload, and final
piece verification followed roughly 0.2 seconds later. The slow 64 MiB-
allowance runs instead accumulated about 48 MiB of resident payload and 3,083
storage jobs. Write/hash queue and summed service grew with that backlog.

### Causal backlog control and final comparison

Six alternating 1 GiB/16 MiB-piece runs changed only the resumable payload
allowance from 64 to 8 MiB. Median throughput rose from 336.8 to 407.1 MiB/s
(`1.209x`), median peak RSS fell from 108,355,584 to 46,358,528 bytes, payload
high water fell from 50,331,648 to 6,291,456 bytes, and storage-job high water
fell from 3,083 to 399. Median write queue wait fell from about 121 million to
16 million summed microseconds; write service fell from about 7.6 to 5.3
million, while hash service fell from about 10.1 to 8.0 million. Median CPU
core-equivalents remained effectively unchanged near 1.18.

The four-run 8/16/32/64 MiB sweep was monotonic:

| Allowance | Payload high water | Storage jobs high water | Median MiB/s | Median peak RSS |
| ---: | ---: | ---: | ---: | ---: |
| 8 MiB | 6 MiB | 399 | 398.9 | 48,291,840 B |
| 16 MiB | 12 MiB | 782 | 376.3 | 58,679,296 B |
| 32 MiB | 24 MiB | 1,550 | 355.6 | 79,781,888 B |
| 64 MiB | 48 MiB | 3,083 | 332.6 | 112,689,152 B |

This control changes the emergency resident ceiling and its 75% intake
watermark together, so it proves queue pressure is causal but does not prove
that the total memory ceiling should be lowered. Tactical `129` will separate
those policies and sweep the queue watermark with resident safety limits held
constant.

The final four-run plaintext cohort measured libtorrent at 487.9 MiB/s,
RSTorrent/64 MiB at 332.9 (`0.682x`), and RSTorrent/8 MiB at 394.4 (`0.808x`).
The final three-run forced-RC4 cohort measured 371.3, 283.0 (`0.762x`), and
315.4 MiB/s (`0.849x`) respectively. Backlog control therefore removes most
of the application-shaped penalty across both matched wire methods but does
not erase the remaining sustained TCP ceiling.

Raw JSON reports and payloads were summarized here and removed. These values
are same-host controlled evidence, not CI thresholds or public-swarm claims.

Final repository validation passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`;
- `cargo test --workspace`;
- the eight controlled-diagnosis Python contract tests and 23 retained
  public-comparator contract tests under the locked interop environment; and
- `cargo ndk -t x86_64 -t arm64-v8a -P 28 check -p rstorrent-android --lib`.

## Decision

The first optimization owner is storage intake admission, specifically the
accidental coupling between total resident-payload capacity and a storage job
queue that can grow to thousands of 16 KiB items. Tactical `129` will add an
independent byte watermark with hysteresis, preserve the larger safety and
large-piece liveness limits, validate session fairness, and remeasure before
any remaining peer-loop/CPU work. Checkpoint policy, observation, RC4, storage
worker count, uTP, and UDP trackers are not selected by this evidence.

## Non-Goals And Next Boundary

This tactical does not enable or benchmark uTP, revisit UDP trackers, use a
public swarm, create a product benchmark surface, add a user-visible setting,
change default connection/storage policy, add a dependency, establish a CI
performance threshold, or implement the selected optimization. It also does
not claim that loopback throughput predicts every WAN swarm.

Ordinary harness refactoring, bounded diagnostic counters, exact cleanup fixes,
and same-owner defects exposed by the tests are authorized. A materially
different protocol, persistence, product, dependency, or platform contract
requires human direction. The next tactical begins only after the completed
evidence record names its causal owner and falsifiable target.
