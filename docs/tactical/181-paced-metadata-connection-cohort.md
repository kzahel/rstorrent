# Tactical 181: Paced Metadata Connection Cohort

Status: **Active (2026-08-27).** Explicit maintainer direction temporarily
yields Tactical `176`'s unavailable macOS-only gate to this bounded engine
repair. Source inspection is complete; implementation and validation remain.

Topics: [`peer-lifecycle`](../topics/peer-lifecycle.md),
[`performance-and-live-evidence`](../topics/performance-and-live-evidence.md),
[`oracle-driven-engine-campaign`](../topics/oracle-driven-engine-campaign.md),
and [`capability-readiness`](../topics/capability-readiness.md).

## Motivation And Desired Outcome

A source-rich magnet remained in metadata acquisition for roughly a minute
despite a tracker reporting more than 300 seeders. The inspected running
service had accumulated hundreds of peer records, but the metadata supervisor
could own only eight combined pending dials and connected metadata workers.
The eight attempts visible in the initiating sample produced no connection or
metadata payload: three failed and five were canceled after a later peer
completed the dictionary. This is direct evidence that advertised swarm size
does not compensate for a narrow, slow-to-turn-over candidate cohort.

The eight-peer value originated in early metadata work and is now materially
narrower than both the ordinary content supervisor and the pinned libtorrent
oracle. Increase the metadata supervisor's combined pending-plus-connected
working set to 30 while pacing new connection attempts to a conservative,
configurable default of ten per second. The first attempt may start
immediately; later attempts must be spaced so a scheduler delay cannot create
a catch-up burst.

At the stopping condition:

- one torrent may own at most 30 combined pending metadata dials and connected
  metadata workers;
- no more than ten new metadata connection attempts begin per second at the
  default, with approximately 100 ms between starts and no accumulated burst;
- pending dials continue to consume the existing session-wide connection
  budget and fair outbound turn rather than becoming free capacity;
- failure, rejection, timeout, policy change, or worker exit refills the
  cohort through the same pacer;
- metadata completion or cancellation still cancels and joins every losing
  dial and worker and releases every connection permit; and
- desktop and Android construction select explicit bounded values from the
  same engine contract, allowing a later Android-specific reduction without
  changing metadata ownership.

## Stable Scenario Subset

1. **MC-001, exact cohort:** 30 unresolved dials or connected metadata workers
   fill the cohort; candidate 31 remains eligible and consumes no attempt or
   connection permit until capacity is released.
2. **MC-002, paced startup:** candidate one begins immediately. With the
   default ten-per-second value, attempts two through 30 begin no closer than
   the pacer's interval; one delayed supervisor wake starts only one attempt,
   not every missed interval.
3. **MC-003, paced refill:** a failed, rejected, timed-out, or completed worker
   releases cohort capacity, but replacement begins only when both capacity
   and the next pacing instant are available.
4. **MC-004, outer admission:** an unavailable fair outbound turn or exhausted
   session peer budget starts no socket and consumes no pacing interval. A
   later admission begins normally.
5. **MC-005, completion and cancellation:** a useful peer can complete while
   up to 29 other attempts are pending or connected. Every loser reaches the
   existing terminal registry transition and the session returns to zero
   metadata tasks and permits.
6. **MC-006, platform bounds:** desktop and Android defaults are 30 combined
   peers and ten attempts per second. Zero and values above the declared hard
   bounds fail resource validation before networking.
7. **MC-007, unchanged protocol:** BEP 9 block ownership, two assignments per
   peer, one-second request ramp, metadata size limit, hash validation,
   contributor attribution, v1/v2/hybrid identity, TCP/uTP/MSE transport
   selection, and all existing connection and handshake deadlines remain
   unchanged.

## Source Dossier

Pinned libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` is the behavioral completeness
oracle. No source, fixture, or test data is copied.

- `src/torrent.cpp::want_peers` applies the torrent's normal connection limit;
  `want_peers_download` includes both `downloading` and
  `downloading_metadata`. There is no metadata-specific eight-connection cap.
- `src/torrent.cpp::do_connect_boost` may spend the default
  `torrent_connect_boost = 30` immediately after the first tracker response,
  bounded by remaining session connections. Boost attempts are deducted from
  the next regular connection quota.
- `src/session_impl.cpp::try_connect_more_peers` distributes the default
  `connection_speed = 30` attempts per tick across downloading torrents and
  stops at the default `connections_limit = 200`.
- `src/settings_pack.cpp` supplies the relevant defaults: 15-second peer
  connect timeout, ten-second handshake timeout, deprecated unlimited
  half-open limit, 200 connections, ten incoming-slack connections,
  three-second uTP connect timeout, 30-attempt boost, and 30 attempts per
  second.
- `src/peer_connection.cpp` accounts half-open and established outgoing work
  in the session connection invariant. A pending connection is not exempt
  from the global limit.
- `simulation/test_metadata_extension.cpp` exercises metadata extension
  behavior including uTP; `test/test_fast_extension.cpp` exercises metadata
  message cases; `simulation/test_fast_extensions.cpp::handshake_timeout` and
  `simulation/test_swarm.cpp` connect-timeout cases retain bounded failure
  paths.

RSTorrent's current owner is
`crates/rstorrent-engine/src/driver.rs::acquire_metadata_inner`. Its constant
`MAX_METADATA_PEERS = 8` bounds `PeerSocketSet::pending_len() + JoinSet::len()`.
`PeerSocketSet` obtains a session `PeerBudget` permit before socket work, so
RSTorrent already agrees with libtorrent that pending work counts against the
global connection limit. Ordinary content separately defaults to 30 pending
and 30 established connections. The present metadata path refills all eight
slots in one supervisor iteration and has no attempt-rate pacer.

JSTorrent's retained legacy engine uses one `maxconns` policy for torrent peer
connections and emphasizes finding useful peers, but its historical metadata
fetcher has different per-peer assembly ownership. It supplies product history
only; Tactical `019`'s torrent-owned RSTorrent metadata coordinator remains
authoritative.

## Adopted Behavior And Intentional Differences

Adopt libtorrent's useful absence of a special narrow metadata ceiling and its
30-peer startup breadth. Keep RSTorrent's explicit combined metadata cohort
instead of libtorrent's effectively unlimited per-torrent default beneath the
session cap. Count pending and established metadata work together, which is
more conservative than ordinary content's independent 30-plus-30 bounds.

Do not adopt libtorrent's immediate 30-attempt tracker boost or 30 attempts per
second. The initial RSTorrent value is ten attempts per second with no burst,
chosen to bound socket, uTP, MSE, timer, and task creation on Android. Keep the
existing fair outbound admission and configurable effective session
connection limit; the common default remains 200. This tactical does not make
metadata exempt from either owner.

## Owner, Task, Cancellation, And Dependency Map

```text
platform/application DownloadResourceLimits
        metadata cohort + dial-rate values
                     |
                     v
metadata supervisor (one per active metadata operation)
  monotonic no-burst pacer + combined cohort admission
        |                              |
        v                              v
PeerSocketSet pending tasks       joined metadata workers
  PeerBudget permit                  PeerBudget permit retained by socket
        \______________________________/
                       |
                       v
        TorrentMetadataDownload (one pure BEP 9 owner)
```

- `DownloadResourceLimits` owns explicit platform-selected metadata bounds and
  validates them before runtime construction. It remains a plain value with no
  socket, task, clock, or platform dependency.
- A small task-free pacer owns only the next admissible monotonic instant. The
  supervisor supplies time, asks whether one dial may start, and advances the
  deadline only after `PeerSocketSet` accepts that dial.
- The metadata supervisor remains the only owner of dial creation, worker
  spawn, cancellation, joins, and cohort accounting. No timer task, semaphore,
  parallel scheduler, or metadata registry is added.
- `PeerSocketSet` and `PeerBudget` retain socket/task and session connection
  ownership. The fair outbound turn remains outside the torrent-local cohort.
- `TorrentMetadataDownload` remains runtime independent and unchanged; wider
  connection admission must not multiply its one-MiB dictionary allocation or
  per-peer/per-torrent request bounds.

## Bounds And Shape-Changing Edge Cases

- The configurable cohort range is `1..=200`, matching the common session
  connection default as a hard per-torrent safety ceiling. The configurable
  rate range is `1..=30` attempts per second, never more aggressive than the
  pinned reference default.
- Desktop and Android both initially select 30 peers and ten attempts per
  second. The fields are independently platform-selectable so later measured
  Android pressure may reduce either value without a new owner or API shape.
- Pacing uses a ceiling division of one second by the configured rate. An
  attempt advances the next deadline from its actual start instant, so delayed
  wakeups never accumulate tokens or start multiple catch-up dials.
- Candidate absence, denied fair turn, peer-budget exhaustion, registry
  rejection, and synchronous socket-set rejection do not spend a pacing
  interval unless a dial task was actually accepted.
- Policy changes cancel disallowed work before refill. Candidate selection,
  failure history, dry-swarm rules, and exact connection generations remain
  unchanged.
- Thirty sockets may each own transport/handshake buffers and one connection
  permit, but they share the existing one-MiB metadata owner. No content
  payload, piece, request, storage, tracker, DHT, PEX, or retained-peer bound
  grows.
- Simultaneous completion and cancellation must not leave a dial unjoined,
  double-settle an attempt, retain a permit, or advance stale registry state.

## Implementation Sequence And Gates

1. Add the pure validated metadata limits and no-burst pacer. Prove boundary,
   exact interval, delayed-wake, and invalid-value behavior without sockets or
   Tokio tasks.
2. Replace the fixed eight-peer metadata admission with the configured
   combined cohort and advance the pacer only for an accepted dial. Use the
   existing supervisor wake path with the next pacing deadline; add no timer
   task.
3. Strengthen scripted metadata tests for 30/31 admission, useful peer among a
   saturated cohort, paced failure refill, and cancellation/join/permit
   closure. Preserve existing v1/v2/hybrid and metadata-owner cases.
4. Run focused engine tests, formatting, warning-denying workspace clippy, and
   workspace tests. Run the existing controlled loopback metadata/libtorrent
   interoperability gate if its harness is locally available.
5. Build the common engine/application path for Android arm64 and x86_64 and
   record the exact artifact/build gates. No emulator or physical-device
   interaction is authorized by this tactical.
6. Reconcile this tactical and the owning topics with actual limits, paths,
   test counts, resource high waters where exposed, and any deliberate
   deferral. Resume Tactical `176` as the sole **Now** after completion.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | limit validation; 30/31 cohort arithmetic; immediate first attempt; 100 ms default spacing; delayed wake has no burst |
| Scripted runtime | paced 30-peer startup/refill; useful completion among losers; cancellation joins every pending/worker owner; terminal permits and registry activity are zero |
| Controlled interop | existing loopback pinned-libtorrent metadata acquisition remains hash exact with bounded cleanup |
| Platform | desktop/workspace Rust gates plus Android arm64 and x86_64 native builds using the explicit Android values |
| Live public | no new public traffic is authorized; the redacted 2026-08-27 running-service observation motivates the change but is not a performance threshold |

## Non-Goals And Next-Slice Boundary

- changing the 15-second RSTorrent transport connect timeout, 60-second
  peer-wire IO/handshake deadline, or sequential Prefer-uTP-to-TCP fallback;
- copying libtorrent's three-second uTP connect timeout, ten-second handshake
  timeout, immediate boost, 30-attempt rate, or per-torrent connection model;
- changing the global connection default, incoming slack, fair active-download
  admission, established content limits, tracker/DHT/PEX discovery, ranking,
  retry/backoff, dry-swarm recovery, or duplicate-peer resolution;
- changing BEP 9 scheduling, metadata geometry, integrity, v2/hybrid hash
  exchange, MSE, content requests, storage, upload/seeding, persistence, public
  application settings, generated client contracts, or UI; and
- a public-swarm comparison, AVD, physical Android device, or macOS/iOS host
  run.

If the expanded paced cohort still waits predominantly on uTP transport
fallback, a later source-first tactical may compare a shorter uTP-only attempt
deadline against Android and WAN evidence. If connection/task pressure is
material on Android, the existing platform-selected values are the first
conservative tuning point. Neither result requires weakening session/global
connection accounting.

## Stopping And Escalation

Complete when scenarios `MC-001` through `MC-007`, the validation matrix, exact
cleanup, Android builds, tactical evidence, and living-topic reconciliation
pass. Then resume Tactical `176`'s unchanged macOS-only gate.

No human decision is required for ordinary module extraction, internal names,
stronger adversarial tests, or conservative tightening within the declared
ranges. Stop for a product-visible setting/API, new dependency, persistence or
compatibility change, destructive data action, public-network or physical-
device run, a default outside the declared bounds, or evidence requiring a
different connection owner.

## Implementation And Evidence

Pending implementation.
