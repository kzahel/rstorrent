# Tactical 134: Hierarchical Transfer-Rate Enforcement

Status: **Ready, authoritative Now and not implemented**. The maintainer
explicitly selected this product-policy slice on 2026-08-11. Ready Tactical
[`129`](129-bounded-storage-intake-watermark.md) remains intact and queued
after this work.

Topics: `application-control`, `application-view-api`, `client-persistence`,
`client-surfaces`, `web-ui-design`, `performance-and-live-evidence`,
`code-organization-and-refactoring`, `capability-readiness`,
`oracle-driven-engine-campaign`

Dependencies: completed Tactical
[`097`](097-live-client-settings-and-replaceable-session-generations.md)
supplies the stable `SessionNetworkRuntime` and configured/effective settings
model; completed Tactical
[`114`](114-session-wide-concurrent-torrent-admission.md) supplies shared
multi-torrent admission and the common network-admission seam; completed
Tactical [`124`](124-duplex-verified-piece-upload.md) supplies duplex upload;
and completed Tactical
[`133`](133-utp-product-default-enablement.md) makes TCP and uTP both ordinary
product transports that must receive identical policy.

## Decision And Desired Outcome

Add live, durable, hierarchical upload and download rate limits at two user
ownership levels:

- one session-wide upload limit and one session-wide download limit, presented
  as **All torrents** policy; and
- one upload limit and one download limit for each torrent.

Every established peer transfer must satisfy every applicable constraint:

```text
sum(all established peer traffic in one direction) <= session limit
sum(peer traffic for torrent T in one direction)    <= torrent T limit
```

`Unlimited` removes only that constraint. A ready torrent may borrow all idle
session capacity, while two continuously ready torrents receive fair progress
independent of peer count. A torrent limit above the session limit never
overrides the session limit.

The saved user limit and transient effective limit remain distinct. This
tactical installs no automatic network policy, but a later Android metered,
roaming, VPN, background, or power owner can clamp the effective session limit
without overwriting saved session or torrent values.

## Stopping Condition

This tactical is complete only when:

1. one explicit portable `unlimited | limited(bytes_per_second)` value is used
   for session and per-torrent upload/download policy, with finite values
   bounded to `1,024..=4,294,967,295` bytes per second;
2. schema 18 durably stores both per-torrent limits, the existing settings
   singleton stores both session limits, and exact restart, no-op, replay,
   stale-revision, malformed-value, and migration behavior passes;
3. one session owner allocates upload and download quota across fixed per-
   torrent registrations, with a direct unlimited fast path, work-conserving
   torrent-first and peer-within-torrent fairness, bounded bursts and waiters,
   live limit changes, and prompt cancellation and shutdown;
4. all established initiated and accepted peer I/O uses the same policy for
   TCP and uTP, including plaintext and TCP MSE, without making upload waits
   block reads or download waits block writes;
5. deliberate local throttling cannot trigger peer read, writer no-progress,
   request-stall, snub, inactivity, or speed-eviction decisions, and rates
   below one 16-KiB peer frame still make finite progress;
6. configured and effective session policy plus per-torrent configured policy
   cross the generated Rust/JSON Schema/TypeScript/UniFFI/Kotlin boundary and
   are operable in the shared React and Android Compose products;
7. deterministic, scripted full-duplex, multi-torrent, controlled pinned-
   libtorrent, web, and no-window Android evidence proves exact final content,
   cap tolerance, fairness, lifecycle, and terminal zero ownership;
8. both Android ABIs build and the complete repository validation baseline
   passes; and
9. this tactical, its owning topics, the readiness matrix, queue, and restart
   checkpoint record the exact implemented scope and remaining limits.

## User Contract And Units

Use a tagged semantic value rather than a magic numeric sentinel:

```text
TransferRateLimit = Unlimited | Limited { bytes_per_second: u32 }
TorrentTransferLimits = { upload, download }
```

The generated application contract carries integer bytes per second. Product
controls accept and display IEC-friendly KiB/s or MiB/s values, preserve the
last valid finite entry when toggling Unlimited, and state that the limit
applies to peer traffic. A single `set_torrent_transfer_limits` command updates
both torrent directions atomically.

Session settings preserve the existing whole-group `set_client_settings`
command and configured/effective/application-state model. Bandwidth is one
independent convergence domain: a successful durable save may complete before
runtime application, and a degraded or replayed save resubmits current intent.
No listener, DHT, peer registry, established connection, or torrent generation
is replaced merely to change a limit.

## Counted Traffic And Honest Claim

The first policy counts application-visible bytes read from or written to an
established BitTorrent peer stream:

- payload, metadata, framing, and control messages;
- initiated and accepted peers;
- TCP and uTP; and
- plaintext plus bytes after TCP MSE negotiation, at the existing physical
  peer-wire observation boundary.

Incoming handshake and routing bytes before a torrent is known, outgoing
handshake/MSE setup, DNS, trackers, DHT, port mapping, and kernel IP/TCP/UDP
headers are excluded. The UI therefore calls this a **peer transfer limit**,
not a total-device or carrier-data budget. The existing `peer_wire_received`
and `peer_wire_sent` observations are the measurement authority.

There is no implicit local-network exemption. A user-selected All torrents
limit has predictable semantics on every peer address. Explicit local-traffic
or network-profile policy is later work.

## Allocator Semantics And Bounds

Each direction has one joined session allocator and at most 1,024 torrent
registrations. At most one waiter per established connection and direction is
admitted, under the existing configured 2,000-peer maximum plus ten incoming
slack. The bounded command/waiter lane therefore holds at most 4,096 entries
per direction. No per-byte, per-frame, or per-torrent task is created.

The allocator uses monotonic time and a task-free deterministic quota core.
Finite buckets:

- start without retroactive idle credit when enabled;
- accumulate at their configured bytes-per-second rate;
- retain at most one second of credit and never more than 1 MiB;
- grant at most 16 KiB at once;
- clamp excess credit immediately when a limit decreases;
- wake queued work when a limit increases or becomes unlimited; and
- cap elapsed-time accumulation after process suspension or scheduler delay.

For any continuously observed finite interval, counted bytes must satisfy:

```text
bytes <= rate * elapsed + declared bucket capacity + one 16-KiB in-flight grant
```

The last term applies only across a concurrent live limit decrease. Quota not
used by a short/failed socket operation is returned. Dropped, cancelled, stale,
or disconnected waiters cannot retain quota or registry membership.

Scheduling is work-conserving deficit round robin across ready torrents, then
FIFO/round-robin across each torrent's ready peer operations. Equal default
weights mean peer count cannot buy a torrent more of a contended session cap.
An idle or torrent-capped member reserves nothing, so another ready torrent
may consume the remainder. Priorities and weights are not public policy in
this tactical.

The direct path checks immutable/atomic effective state and performs no
allocator command, timer, quota fragmentation, or wakeup while the session
and torrent constraints for that direction are both unlimited. A concurrent
transition to finite may allow at most the already-entered 16-KiB operation.

## Source-First Record

No reference source, fixture, or test vector is copied.

### Pinned libtorrent

Re-inspected Rasterbar libtorrent `2.0.13.0` at exact commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/settings_pack.hpp` and `src/settings_pack.cpp` define
  live session `upload_rate_limit` and `download_rate_limit`, in bytes per
  second with zero meaning unlimited;
- `include/libtorrent/torrent_handle.hpp` and
  `src/torrent.cpp::{set_limit_impl,setup_peer_class}` implement per-torrent
  upload/download channels that cannot override a lower global limit;
- `src/read_resume_data.cpp` and `src/write_resume_data.cpp` persist those
  per-torrent values;
- `src/session_impl.cpp::{get_bandwidth_manager,copy_pertinent_channels}` owns
  one upload and one download manager and gathers every limited peer/torrent
  class channel;
- `src/peer_connection.cpp::request_bandwidth` submits one connection request
  against all applicable channels;
- `src/bandwidth_limit.cpp`, `src/bandwidth_queue_entry.cpp`, and
  `src/bandwidth_manager.cpp` implement quota accrual, most-restrictive-channel
  grants, proportional request priority, low-rate partial dispatch, live
  cleanup, and a direct no-channel path;
- `test/test_bandwidth_limiter.cpp` covers equal connections, session and
  torrent caps, two torrents, live rate changes, priorities, no starvation,
  and the ten-channel ceiling; `test/test_peer_classes.cpp` covers class
  limits/filters; and `test/swarm_suite.cpp` applies upload/download caps to a
  real controlled swarm.

Libtorrent also exposes generic IP/transport peer classes, exempts local peers
from global limits by default, permits up to three seconds of accumulated
quota, estimates IP overhead against the global channel by default, and keeps
DHT upload limiting separate. RSTorrent adopts simultaneous session/torrent
constraints, live mutation, partial grants, cancellation cleanup, and the
unlimited fast path. It deliberately differs by using explicit peer-transfer
scope, no hidden LAN exemption, a smaller declared burst, and torrent-first
fairness instead of libtorrent's connection/request-weighted global sharing.

### JSTorrent product history

Re-inspected local sibling commit
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/engine/src/config/{config-schema,base-config-hub}.ts` exposes live
  global upload/download values with a separate Unlimited choice;
- `packages/engine/src/core/bandwidth-tracker.ts` owns one global token bucket
  per direction;
- `packages/engine/src/core/torrent-uploader.ts` gates upload disk reads and
  rotates bounded peer queues;
- `packages/engine/src/core/piece-requester.ts` paces requested download blocks
  and caps the pipeline while limited;
- `packages/engine/src/webseed/web-seed-manager.ts` makes download waits
  abortable; and
- `packages/engine/src/core/torrent-tick-loop.ts` suppresses speed-based peer
  drops while local download policy is the bottleneck.

RSTorrent retains the useful explicit Unlimited presentation, live updates,
bounded/abortable waits, and policy-aware stall decisions. It does not copy
JSTorrent's global-only bucket, request-time download charging, tick-loop
architecture, or absence of per-torrent composition.

## Existing RSTorrent Boundary And Required Refactor

Completed Tactical `097` names `SessionNetworkRuntime` as the stable home for
later bandwidth policy. Completed Tactical `114` requires finite session and
per-torrent channels to attach at a common network-admission boundary while
leaving the unlimited path direct and work-conserving.

The concrete code boundary is now:

- `peer_stream.rs::PeerStream` is the TCP/uTP byte-stream boundary;
- `peer_io.rs::PeerIo` owns initiated direction-neutral framing and MSE
  ciphers;
- `peer_socket.rs::run_peer_task` currently awaits a complete send inside the
  same owner that reads, so sleeping there for upload quota would block
  downloads and control traffic;
- `incoming/peer_io.rs` already owns a bounded joined writer but its 60-second
  no-progress clock currently includes every delay; and
- `torrent_peer.rs::TorrentPeerHandle` is the stable torrent lifetime shared
  by outgoing work and routed incoming registrations.

The required boundary improvement is to attach one cloned torrent bandwidth
registration to `TorrentPeerHandle`, pass it into initiated and accepted peer
I/O after handshake/routing, and make initiated I/O genuinely duplex under
independent read/write quota. Rate waits are local scheduling, not socket
inactivity, and therefore do not consume peer or writer no-progress clocks.

## Owner, Task, Cancellation, And Dependency Map

| Owner | Mutable state and work | Cancellation and termination |
| --- | --- | --- |
| Pure quota core in `rstorrent-engine` | bounded session/torrent buckets, ready rings, grants, limits, counters | no runtime types, sockets, or tasks; deterministic time input |
| Session bandwidth service | one upload and one download allocator command loop | session shutdown closes admission, cancels waiters, and joins exactly two tasks |
| `SessionNetworkRuntime` | configured/effective session limits and service lifetime | live settings convergence updates in place; application shutdown joins service |
| Torrent bandwidth registration | configured torrent limits and at most one ready request per peer/direction | torrent removal unregisters; generation close cancels all waiters and returns quota |
| Initiated peer I/O | bounded frame queue, independent read/write grants, one peer task | peer/torrent/session cancellation interrupts either wait without false timeout |
| Accepted peer I/O | bounded reader plus existing joined writer, both using the same registration | writer and reader join before attachment/budget release |
| SQLite/application command owner | durable full client group and atomic torrent pair | persistence resolves before live reconciliation; receipt replay is exact |
| React/Compose adapters | user editing and semantic command dispatch only | no timer, token, payload, socket, or inferred effective-state owner |

The engine owns transfer scheduling and hot-path byte admission. The session
crate owns durable product policy and lifetime composition. Protocol codecs,
storage, platform adapters, and presentation depend inward and do not acquire
runtime quota machinery.

## Contract, Persistence, And Product Gates

Schema 18 adds two nonnegative bounded per-torrent rate-limit columns and the
matching session setting representation. Migration maps every existing value
to Unlimited. The complete typed pair appears in `TorrentSnapshot` and the
appropriate summary/detail projections; clients never infer it from current
rates.

The web product adds:

- All torrents upload/download controls to Connection settings; and
- per-torrent upload/download controls in the selected torrent's ordinary
  detail/action surface.

The Android product adds the same semantics to its existing **Speed &
Connection Limits** settings page and torrent detail. Presentation may remain
platform-appropriate, but both directions and Unlimited must be operable. The
generated boundary, runtime validator, demo/default fixtures, reducers, and
accessibility labels remain synchronized.

Configured session limits, effective session limits, bandwidth application
state, active waiter counts, queued requested bytes, grant bytes, throttle
wait duration/high-water, cancellations, and current burst credit are bounded
structured observations. Logs remain separate and no per-grant record is
emitted.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Deterministic | typed bounds, fractional accrual, burst cap, session/torrent intersection, many-peer versus one-peer fairness, idle borrowing, dynamic changes, suspension, partial/refunded grants, cancellation, unregister, and shutdown |
| Scripted runtime | initiated/accepted TCP and uTP, plaintext/RC4 where applicable, upload-only/download-only/full-duplex, low-rate sub-frame progress, no false timeout/stall, and terminal zero queues/tasks |
| Persistence/API | schema 17-to-18, restart, ephemeral parity, atomic torrent command, replay/conflict/stale revision, configured/effective convergence, generated round trips, and hostile validation |
| Controlled interop | pinned-libtorrent source/sink roles, one and two torrents, unequal peer counts, exact final SHA-1, direction totals, cap tolerance, fairness, and cleanup |
| Web | component/unit, production build, wide/compact/phone interaction, keyboard, Unlimited toggle, validation/error recovery, live authority, and serious/critical accessibility scan |
| Android | generated Kotlin, unit/Compose behavior, both native ABIs, APK checks, and one no-window AVD limited duplex or concurrent-torrent profile |
| Repository | format, warning-denying Clippy, workspace tests, generated drift, TypeScript typecheck/tests, architecture/dependency checks, and temporary-artifact cleanup |

## Staged Execution And Commit Plan

1. Commit this source-first tactical, make it the sole authoritative **Now**,
   and leave Tactical `129` ready and queued.
2. Add the pure hierarchical allocator, joined session service, torrent
   registrations, exhaustive fake-time tests, and focused resource snapshots;
   commit.
3. Integrate quota after peer handshake/routing, refactor initiated peer I/O
   for independent read/write progress, apply it to accepted TCP/uTP I/O, and
   commit after scripted duplex/lifecycle tests pass.
4. Add schema 18, portable values, session and torrent commands/views,
   convergence, generated artifacts, and restart/hostile tests; commit.
5. Add shared React and Android Compose controls and their deterministic
   presentation evidence; commit.
6. Run controlled pinned-libtorrent and no-window AVD evidence, complete all
   repository gates, record high waters/results, reconcile topics and queue,
   and commit the completed tactical.

## Non-Goals And Deliberate Deferrals

- Ratio, share, time, idle, seed-rank, automatic stop, or archive goals.
- Durable lifetime transfer totals or reset epochs beyond existing metrics.
- Per-peer limits, generic peer classes, torrent priority weights, alternate-
  rate schedules, temporary speed modes, or scheduler presets.
- Automatic metered, roaming, VPN, interface, battery, foreground/background,
  or Android power policy. The configured/effective seam is the foundation,
  not an invented platform policy.
- A hidden LAN exemption, total-device byte budget, kernel/header accounting,
  packet capture, QoS/DSCP, congestion-control changes, or uTP MTU changes.
- Limiting tracker, DHT, DNS, mapping, web-host, application-control, storage,
  or playback traffic.
- Changing active-download admission, upload-slot/choking policy, request
  windows, storage intake, or Tactical `129`'s queued optimization.
- A new crate, daemon, proxy, IPC path, dependency, or public compatibility
  promise.

The tactical stops when the four everyday peer-transfer controls work
durably, fairly, live, and identically across ordinary first-party transports
and clients. Network automation and seeding goals start only in later bounded
tacticals.
