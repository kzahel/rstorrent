# Tactical 129: Bounded Storage Intake Watermark

Status: **Ready, queued and not implemented** on 2026-08-10. Completed Tactical
[`128`](128-controlled-tcp-performance-diagnosis.md) selected storage intake
backpressure as the next optimization owner. The maintainer subsequently
resumed the bounded uTP campaign through Tactical `130`; this plan and its
evidence remain intact behind that explicitly selected work.

Topics: `performance-and-live-evidence`, `storage-throughput-architecture`,
`capability-readiness`, `oracle-driven-engine-campaign`

## Decision And Motivation

RSTorrent currently derives both resident-payload capacity and storage-command
backpressure from `max_buffered_payload_bytes`. On the controlled 1 GiB/16 MiB
piece row, a 64 MiB allowance admitted a 48 MiB payload high water and about
3,083 pending storage jobs. Holding everything else constant while reducing
the allowance to 8 MiB raised matched-plaintext median throughput from 332.9
to 394.4 MiB/s, cut peak RSS from 103.3 to 44.2 MiB, and improved the
libtorrent ratio from `0.682` to `0.808`. A separate 8/16/32/64 MiB sweep was
monotonic: 398.9, 376.3, 355.6, and 332.6 MiB/s.

The next slice therefore separates the storage-intake watermark from the
larger resident-payload safety ceiling. It must preserve bounded memory and
large-piece liveness while stopping ordinary peer reads before thousands of
small storage jobs amplify queue wait and filesystem service time. This is an
engine policy optimization, not a user-visible memory setting.

## Stopping Condition

This tactical is complete when:

1. storage waiting/running bytes and jobs have an explicit owner and
   hysteretic high/low watermark independent from the total resident-payload
   emergency ceiling;
2. deterministic delayed-storage tests prove prompt pressure entry, bounded
   overshoot, progress while draining, release below the low watermark,
   cancellation, hash/checkpoint completion, and one-piece liveness when a
   piece exceeds the ordinary watermark;
3. session-wide multi-torrent accounting preserves root/torrent fairness and
   the existing aggregate payload, write, hash, and file-handle ceilings;
4. an alternating controlled sweep selects a watermark rather than assuming
   8 MiB is optimal, with 1 GiB/16 MiB as the primary sustained row and 256
   KiB/1 MiB pieces as non-regression rows;
5. the selected application-shaped path improves the primary plaintext row
   by at least 10% over the current desktop policy without regressing any
   retained small-piece or two-torrent control by more than 5%;
6. matched forced-RC4 retains the same direction, exact hashes/publication and
   cleanup pass, resource high waters are recorded, both Android ABIs build,
   and repository validation passes; and
7. the remaining libtorrent-relative deficit is remeasured before any TCP
   peer-loop, framing, hashing, or storage-backend optimization is selected.

If no candidate clears those gates, retain the negative result and do not
change the default.

## Invariants And Bounds

- Accepted peer data remains charged exactly once until storage completion;
  a watermark cannot weaken the resident-payload or session-wide byte caps.
- Backpressure pauses new payload intake without losing socket bytes,
  requests, discovery events, storage completions, checkpoint failures, file
  selection, cancellation, or upload progress.
- High/low hysteresis prevents read-pause thrashing. Overshoot is bounded by
  already accepted blocks and executing writes, not by a second unbounded
  queue.
- A valid piece larger than the ordinary queue watermark retains one bounded
  liveness exception. The exception cannot multiply per block, peer, or
  torrent.
- Hashes start only after their existing generation/write fence. Durability
  checkpoints, publication, and crash semantics remain unchanged.
- Multi-torrent admission remains session/root fair. A slow root cannot hold
  every storage slot or consume the full aggregate resident allowance.
- Diagnostics use counters and sampled snapshots; no per-block log or
  unbounded timeline is introduced.

Initial measurement candidates are 1, 2, 4, 6, and 8 MiB high watermarks with
low watermarks at a documented hysteretic fraction. The existing desktop and
Android resident-payload ceilings remain unchanged during that sweep. The
chosen values are implementation constants unless evidence later justifies a
separate product policy surface.

## Source-First Record

No reference source or test is copied.

- Pinned libtorrent `2.0.13.0` at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d` documents
  `settings_pack::max_queued_disk_bytes` in
  `include/libtorrent/settings_pack.hpp`: peers stop reading when queued disk
  bytes exceed the bound and resume after the disk thread catches up.
  `src/peer_connection.cpp` applies that pressure after `async_write` and
  accounts `queued_write_bytes`. `src/settings_pack.cpp` defaults the bound to
  1 MiB; `src/session.cpp`'s high-performance preset uses 7 MiB. Those values
  are comparison points, not RSTorrent defaults.
- Libtorrent's `performance_warning` cases in `test/test_alert_types.cpp`
  distinguish an outstanding-disk-buffer limit from a queue that is too high
  for its cache. RSTorrent adopts the separate ownership signal, not its alert
  API or disk architecture.
- RSTorrent's current coupling is exact:
  `driver/storage_pipeline.rs::content_storage_job_limit` divides the complete
  resident allowance by one 16 KiB block, while
  `DownloadControl::configure_disk_runtime` sets pressure at 75% and releases
  it at 50%. Existing storage-pressure tests prove discovery and completion
  owners still advance while payload intake is paused.
- Local JSTorrent sibling commit
  `9895410beeed6aff554053769bd006a3fbd373ef` separately models peer-buffer and
  verified-write-queue pressure. `core/bt-engine.ts::checkBackpressure` uses
  independent high/low queue watermarks; `presets/native.ts` supplies its
  platform values. RSTorrent adopts only the need to separate queue pressure
  from total buffering.

## Owner And Dependency Shape

The content storage pipeline owns queued and running write bytes/jobs. The
download control owns their bounded diagnostic projection and pressure state.
The content supervisor consumes only the pressure transition when deciding
whether peer payload events remain eligible; it continues to rotate storage,
discovery, selection, shutdown, and upload owners. Session resources remain
the aggregate byte and concurrency authority. Protocol, picker, file layout,
checkpoint, and platform adapters depend inward on those facts and do not
learn about benchmark profiles.

The concrete boundary improvement is replacing the accidental
`resident bytes / block size == storage queue capacity` relationship with one
named queue policy expressed in bytes plus bounded execution overshoot.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Deterministic | high/low transitions, overshoot, large-piece liveness, queue completion ordering, cancellation, checkpoint failure, and session fairness |
| Scripted runtime | delayed path storage and Android-shaped provider delay with continued discovery/control progress and terminal zero ownership |
| Controlled interop | alternating plaintext 1 GiB/16 MiB sweep, small-piece non-regression, two-torrent control, and representative forced RC4 against pinned libtorrent |
| Resources | resident payload, queued/running bytes/jobs, request window, write/hash service and wait, RSS, CPU, publication, and cleanup |
| Repository | formatting, warning-denying clippy, workspace tests, locked interop, and Android x86_64 plus arm64-v8a cross-builds |

## Non-Goals

This tactical does not enable uTP, change tracker behavior, add a setting,
lower the total session memory safety cap, change write/hash concurrency,
introduce direct I/O or memory mapping, tune the request window, alter
checkpoint durability, profile public swarms, or promise parity with
libtorrent. The remaining roughly 15--20% sustained ceiling is a later
profile-driven slice only after this queue policy lands and is remeasured.
