# Tactical 137: Product uTP Path-MTU Discovery

Status: **Active on 2026-08-11.** Maintainer direction reactivated this
decision-complete tactical after verified HTTP file-serving Tactical `138`
completed and authorized end-to-end implementation with logical commits.
Stage 2's shared-egress and safe platform-option boundary is complete.
Maintainer review approved target-specific `dontfrag 1.0.1` for macOS;
existing `rustix` remains the Linux/Android adapter, both Android native ABIs
cross-build, and actual macOS set/get/restore passes. Stage 3 deterministic
revalidation and downward recovery are complete; product packetization remains
fixed at 548 while Stage 4 integrates protected sends. No unsafe project code,
public-network activity, or physical-device work is authorized.

Topics: `utp-transport-campaign`, `capability-readiness`,
`oracle-driven-engine-campaign`, `protocol-support`,
`performance-and-live-evidence`

Dependencies: completed Tactical
[`121`](121-deterministic-utp-loss-congestion-and-mtu.md) supplies the pure
path-MTU state and packetization contract; completed Tactical
[`125`](125-shared-udp-utp-runtime-and-loopback-interop.md) supplies the shared
DHT/uTP socket and supervised runtime; closed Tactical
[`130`](130-utp-transport-solidification.md) supplies diagnostic MTU feedback
and a controlled black-hole fixture; completed Tactical
[`133`](133-utp-product-default-enablement.md) supplies the product-default
fixed-548 IPv4/plaintext path; and completed Tactical
[`134`](134-hierarchical-transfer-rate-enforcement.md) supplies the common
TCP/uTP stream-byte rate owner that this slice must preserve.

## Decision And Desired Outcome

Replace the ordinary product's fixed 548-byte IPv4 uTP datagrams with bounded
packetization-layer path-MTU discovery on platforms that can prove a
fragmentation-protected probe. Each connection starts from the existing
548-byte UDP-payload floor, sends ordinary data only at a confirmed size, and
may search toward the 1,472-byte IPv4 Ethernet ceiling. Acknowledgement raises
the confirmed floor; an isolated probe loss or local message-too-large result
lowers the search ceiling without falsely reducing the congestion window.

This is not a switch from 548 directly to 1,472. The existing deterministic
search becomes an ordinary runtime capability only after the shared socket can
apply and restore the operating-system fragmentation policy around exactly one
probe without affecting DHT, another uTP connection, or a replacement socket
generation. A platform without that verified capability remains fixed at 548
and reports the fallback honestly.

The product policy remains `PreferUtp`; there is no new setting, schema,
generated application contract, or UI. The BEP 29 claim remains **Partial**.

## Scope And Stopping Condition

This tactical owns:

1. a safe platform capability for fragmentation-protected IPv4 UDP probe
   sends, including exact get/set/restore behavior and message-too-large
   classification;
2. one per-family shared-UDP egress exclusion boundary used by DHT and uTP so
   socket-wide probe policy cannot bleed into another datagram;
3. runtime activation of the existing 548--1,472-byte uTP search only when the
   platform capability is positively verified;
4. deterministic revalidation and bounded failure behavior after search
   completion or a later path change;
5. exact preservation of uTP sequence/payload identity for the one
   fragmentable compatibility retry that follows a failed probe;
6. structured MTU, send-policy, failure, queue, and terminal ownership facts;
7. controlled pinned-libtorrent, fixed-black-hole, shared-DHT, rate-limit,
   performance, desktop, and Android evidence; and
8. reconciliation of the uTP, readiness, oracle, protocol, reference, and
   performance topics without promoting the protocol claim.

The tactical completes only when:

- a protected probe cannot interleave with DHT or another uTP send, and every
  success, error, cancellation, panic boundary, and generation replacement
  restores the exact prior socket policy before another sender proceeds;
- failure to restore the policy terminally fences and replaces that socket
  generation rather than returning a potentially contaminated socket to DHT
  or uTP;
- verified platforms construct ordinary product uTP with a 548-byte floor and
  1,472-byte ceiling, while unsupported or failed capability checks construct
  the existing fixed `548..=548` profile;
- pure state and runtime tests cover ACK, three-later-ACK loss, sole-packet
  timeout, ordinary congestion loss, local message-too-large, fragmentable
  same-sequence retry, minimum-floor failure, search completion, later path
  reduction, hard fragment black holes, cancellation, and retry exhaustion;
- a clean 1,500-byte controlled path confirms at least 1,456 UDP-payload bytes
  and a 1,280-byte controlled path confirms between 1,264 and 1,280 bytes,
  both within the existing 16-byte convergence interval and with exact content;
- the existing 548-byte baseline and an unsupported-capability case send no
  probe, exceed no 548-byte datagram, and preserve exact transfer and cleanup;
- the clean 1,500-byte product fixture reduces RSTorrent uTP DATA datagram count
  by at least 50% relative to an alternating fixed-548 control, without a
  material median transfer regression, integrity failure, rate-cap bypass, or
  increased unbounded state;
- pinned libtorrent transfers the exact fixture in both RSTorrent roles through
  the ordinary application policy, with selected MTU, packet counts, probe
  outcomes, congestion response, resource high waters, and terminal zero
  ownership recorded;
- macOS behavior is proved on the development host, both Android native ABIs
  build, and a no-window API 34 AVD proves actual option, send, replacement,
  application transfer, and cleanup semantics; and
- formatting, workspace Clippy with warnings denied, complete workspace tests,
  and applicable web contract checks pass.

No public swarm, WAN peer, `pimom`, visible product client, or physical device
is required or authorized by this tactical.

## Non-Goals

- IPv6 uTP, IPv6 PMTU discovery, Teredo, SOCKS/proxy overhead, interface MTU
  enumeration, multi-interface MTU caching, or route-change subscriptions.
- UDP UPnP mapping, incoming-uTP tracker/DHT advertisement, NAT-PMP, PCP,
  hole punching, or a public incoming-reachability claim.
- Persisted or shared destination-MTU cache, durable endpoint capability,
  user-selected packet size, automatic network policy, or presentation.
- Parsing ICMP Packet Too Big messages or treating unauthenticated ICMP as the
  sole MTU authority. This slice uses local send errors and validated,
  connection-scoped uTP acknowledgement/loss evidence.
- MSE-over-uTP, IPv6 transport selection, TCP/uTP racing, proxy support, or a
  different sequential fallback policy.
- Replacing RFC 6817 congestion control, changing ordinary peer-stream rate
  accounting, or counting IP/UDP/uTP overhead against user payload caps.
- Full RFC 8899 conformance or a **Supported** BEP 29 claim. uTP's per-packet
  sequence identity requires one deployed compatibility behavior that RFC
  8899 does not define: retrying a failed application-data probe with the same
  sequence and payload after removing fragmentation protection.

## Accepted Protocol And Runtime Contract

### MTU state and packet identity

- Values are complete IPv4 UDP payload sizes, including uTP header and
  extensions but excluding IPv4 and UDP headers. The base is 548 bytes and the
  ordinary ceiling is 1,472 bytes.
- One connection owns one runtime-independent state machine with explicit
  `Base`, `Search`, and `SearchComplete` behavior. It stores one confirmed
  floor, one ceiling, one candidate, at most one active probe, one optional
  fragmentable retry, and bounded counters/deadlines.
- Ordinary DATA uses only the confirmed floor. Search sends at most one probe
  at a time, only after at least three floor-sized ordinary packets and when
  the congestion window exceeds three floors. At least one smoothed RTT
  separates probe outcomes and the next probe.
- Search completes when ceiling minus floor is at most 16 bytes. Completion is
  not permanent: a bounded confirmation/revalidation deadline reopens search
  so a long-lived connection can detect changed path behavior. The initial
  implementation chooses and records a conservative interval from RFC 8899
  and pinned libtorrent behavior, within 10--30 minutes; this bounded choice
  does not require human review.
- Only an ACK for the exact probe sequence raises the floor. Three later ACKs,
  a sole-packet timeout, or local message-too-large may classify the exact
  active probe as failed. Ambiguous or multi-packet loss follows ordinary RFC
  6817 congestion behavior and cannot silently lower a confirmed MTU.
- An isolated probe failure does not reduce the congestion window or advance
  congestion timeout backoff. The same already-sequenced packet is retried
  once without fragmentation protection because BEP 29 sequence numbers name
  packets, not byte offsets. It is never split, merged, or assigned a new
  sequence.
- If the fragmentable retry also cannot complete, or an ordinary packet at the
  current floor reaches bounded retry exhaustion after a path reduction, the
  connection closes with an exact MTU/path failure. The application may
  recover missing content through its ordinary new-connection and sequential
  transport policy; the runtime must not corrupt, silently skip, or infinitely
  retain the sequenced payload.
- Every new uTP connection starts from the base rather than inheriting an
  unverified destination cache. A shared conservative cache like libtorrent's
  `restrict_mtu` is a possible later optimization, not this tactical.

### Shared-socket egress isolation

The session currently shares one `Arc<tokio::net::UdpSocket>` per address
family between DHT and all uTP workers. Fragmentation policy is socket-wide on
the target platforms, so changing it from a worker without a common send
boundary would race with unrelated traffic.

The accepted invariant is one per-family egress exclusion owner through which
all DHT and uTP datagrams pass:

```text
DHT owner --------------------\
                               > per-family egress exclusion -> UDP socket
uTP workers -> send intent ---/          |
                                          + set protected policy
                                          + synchronous probe send attempt
                                          + restore exact prior policy
```

- A normal send may use the existing asynchronous readiness behavior, but it
  participates in the same exclusion boundary.
- A protected send holds the boundary, reads the current policy, applies the
  protected policy, performs one synchronous nonblocking send attempt, and
  restores the exact prior policy before releasing the boundary. It performs
  no `.await` while the socket policy differs from its prior value.
- `WouldBlock` publishes no successful send and lets the existing worker retry
  after readiness. `MessageTooLarge` is typed MTU feedback. Other I/O errors
  retain the existing generation-fenced failure path.
- Generation is checked while the egress boundary owns the selected socket.
  Replacement cannot publish or reuse the old socket until any protected
  window has restored or the old generation has been terminally fenced.
- No detached send actor or unbounded command queue is required. If evidence
  makes a dedicated actor materially simpler, it must retain the admission
  bounds below and be recorded before implementation proceeds.

### Platform capability

The engine and platform crates retain `#![forbid(unsafe_code)]`. The current
dependency graph does not expose one portable safe API for every required
desktop and Android target:

- existing `rustix 1.1.4` can expose safe Linux/Android
  `IP_MTU_DISCOVER` get/set operations by enabling its `net` feature and can
  preserve the exact prior enum value;
- the active macOS SDK exposes `IP_DONTFRAG`, but current `rustix` and
  `socket2` APIs do not expose a safe macOS setter/getter; and
- unsupported platforms can retain fixed 548 without weakening behavior.

The first implementation stage must prove the smallest safe cross-platform
adapter. Adding an external crate, accepting its license/distribution posture,
creating an unsafe isolation crate, or relaxing `forbid(unsafe_code)` is a
mandatory human review before that change. A candidate wrapper is not accepted
merely because its source is locally cached or another dependency uses it.

Capability is positive only after set/get/restore and an oversized protected
loopback send behave as expected. Compile-time platform detection alone is not
enough. Failure is closed and observable: dynamic MTU stays disabled for that
socket generation.

## Owner, Task, Cancellation, And Dependency Map

| Owner | Mutable state | Work and termination |
| --- | --- | --- |
| `utp::mtu` | pure phase, floor/ceiling/candidate, one probe/retry, explicit deadlines and counters | No socket, runtime, task, channel, or platform type; caller supplies time, send results, ACK/loss facts. |
| `utp::transport` | unsent bytes, sent ledger, packetization, retransmission and congestion composition | Emits one typed datagram intent per poll and consumes one typed send result; preserves sequence/payload identity. |
| Per-family session UDP egress owner | current socket generation, exclusion boundary, verified fragmentation capability and prior policy during one protected attempt | Starts no detached work; all DHT/uTP senders participate; replacement or shutdown waits for restoration or fences the generation. |
| uTP connection worker | one connection state, clock/deadlines, stream events, and send feedback | Existing supervised task only; cancellation closes the stream and joins the worker with no active probe or retained payload. |
| DHT actor | existing bounded operation state and datagram sends | Uses the same egress exclusion but gains no MTU state and observes no probe policy. |
| Application peer owner | existing `PreferUtp`, endpoint capability, permit, and sequential TCP fallback | One logical dial still owns at most one live transport subattempt; failed uTP joins before TCP. |
| Transfer-rate allocator | session/torrent stream-byte tokens | Remains above TCP/uTP packetization; larger datagrams cannot mint or bypass payload allowance. |

Dependency direction remains inward: runtime/platform code maps OS results to
protocol enums; pure MTU and transport state never depend on Tokio, raw
descriptors, OS constants, application state, or rate-limit owners.

## Stable Resource, Security, And Observability Bounds

- Existing uTP limits remain 64 connections, 16 incoming half-opens, 64 queued
  datagrams per connection, a 256-datagram shared uTP route, 1 MiB receive
  credit, 1 MiB unsent bytes, 1,024 sent packets, and a 1 MiB sent ledger.
- Shared UDP already accepts an IPv4 uTP datagram up to the 1,472-byte ceiling
  plus one malformed sentinel byte. Dynamic outbound MTU does not enlarge the
  hostile receive allocation or queue entry count.
- The egress boundary admits no independent queue. At most the existing 64
  uTP workers and one DHT owner can wait through their already-owned tasks;
  each uTP worker has at most one outstanding send attempt.
- Each connection has at most one protected probe and one fragmentable retry.
  Probes are separated by at least one RTT and search-complete revalidation is
  no more frequent than once per ten minutes.
- Socket policy is never peer-controlled directly. Hostile packets can affect
  ACK/loss state only after connection-ID, endpoint, sequence, and generation
  validation already owned by the uTP transport.
- No unauthenticated ICMP input, route MTU, public endpoint, peer ID, or packet
  payload is persisted or exposed through the application contract.
- Structured snapshots retain current phase, confirmed/candidate/ceiling
  bytes, capability/fallback reason, probes begun/ACKed/failed, protected and
  fragmentable send results, MTU-related closure, egress contention high
  water, maximum datagram size, DHT/uTP drops, congestion reductions, rate
  allowance, and terminal task/queue/connection counts. Counters saturate.

## Source-First Record

No reference source, fixture, test data, or constants file is copied.

### Normative sources

Managed BEP 29 at BitTorrent BEP commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06` was re-read at
`reference/bittorrent.org/beps/bep_0029.rst`, especially packet-based sequence
numbers, loss recovery, packet sizing, and congestion behavior. A sequenced
uTP DATA packet cannot be repacketized like a TCP byte range, which makes
failed-probe recovery a correctness boundary rather than a tuning detail.

[RFC 8899](https://www.rfc-editor.org/rfc/rfc8899.html) was inspected as the
Datagram Packetization Layer PMTU reference, especially Sections 4--7. The
adopted principles are a conservative base, fragmentation-protected probes,
ordinary use of confirmed sizes, ACK-based confirmation, paced probing,
search completion/revalidation, and bounded black-hole recovery. ICMP Packet
Too Big processing is optional evidence and is not adopted in this slice.

RSTorrent does not claim complete RFC 8899 conformance. RFC 8899 does not
define BEP 29's packet-sequence recovery, while deployed uTP retries the exact
failed application-data probe without fragmentation protection because it
cannot repackage that sequence.

RFC 6817 remains the congestion-control authority selected by Tactical `121`.
Dynamic MTU may change its MSS input but not the fixed-point gain, delay
history, loss epoch, timeout, or pacing contract.

### Pinned libtorrent oracle

Rasterbar libtorrent `2.0.13.0` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

- `src/utp_socket_manager.cpp::{mtu_for_dest,send_packet}` derives the 548-byte
  IPv4 floor and ordinary 1,472-byte Ethernet ceiling and forwards a do-not-
  fragment send intent;
- `include/libtorrent/aux_/utp_socket_manager.hpp::restrict_mtu` retains a
  conservative manager-wide ceiling after terminal oversized failures;
- `src/utp_stream.cpp::{init_mtu,update_mtu_limits,send_pkt,resend_packet,
  ack_packet,tick}` starts ordinary data at the confirmed floor, probes the
  midpoint only after sufficient window, raises on exact ACK, lowers on
  isolated loss or message-too-large, excludes an isolated probe from
  congestion response, retries the same packet without protection, restarts a
  completed search downward after a later failure, and closes after bounded
  resend exhaustion;
- `include/libtorrent/aux_/utp_stream.hpp` and
  `include/libtorrent/aux_/packet_pool.hpp` define the bounded per-connection
  MTU fields and IPv4/IP/UDP/Ethernet constants; and
- `simulation/test_utp.cpp::utp_pmtud` loses one probe, expects two resends,
  and records zero congestion loss and zero timeout.

RSTorrent adopts the conservative confirmed-floor search, isolated-loss
treatment, same-sequence compatibility retry, and bounded terminal behavior.
It intentionally does not adopt libtorrent's socket-manager architecture,
global destination restriction, IPv6/Teredo/proxy calculations, optional
slow start, or GPL simulator fixture. Its tests remain independently authored.

### RSTorrent and JSTorrent findings

Existing RSTorrent code already contains most deterministic behavior:

- `crates/rstorrent-protocol/src/utp/mtu.rs::PathMtuState` owns the 548--1,472
  binary interval, 16-byte completion threshold, isolated outcomes, and one
  fragmentable retry;
- `crates/rstorrent-protocol/src/utp/transport.rs::UtpTransportState` composes
  probe packetization, exact ACK/loss, congestion isolation, and typed
  `MessageTooLarge` feedback;
- `crates/rstorrent-engine/src/utp_runtime.rs` exposes the search only through
  `diagnostic_ipv4_path_mtu` and currently ignores an emission's
  `dont_fragment` intent at the shared send handle;
- `crates/rstorrent-engine/src/session_udp.rs` shares one socket per family and
  lets DHT and uTP send concurrently, so a direct worker-local socket-option
  toggle would be incorrect; and
- Tactical `130`'s independent 1,280-byte black-hole fixture converged to
  1,269 bytes with three acknowledged probes, three failed probes, three
  same-sequence fragmentable retries, exact content, and zero probe-caused
  congestion reductions or timeout collapse. That result is diagnostic, not
  OS fragmentation evidence.

The pure state currently treats search completion as terminal and its floor is
monotonic for the connection. This tactical must add explicit revalidation and
downward recovery before making the diagnostic search a long-lived product
default.

The local JSTorrent sibling at
`9895410beeed6aff554053769bd006a3fbd373ef` has no implemented uTP product
transport. Its archived architecture and release status record TCP-only
behavior, while its BEP 29 copy confirms packet-based sequence numbers. There
is no first-party dynamic-MTU behavior or compatibility setting to preserve.

## Stage 2 Feasibility Evidence

The dependency-free seam now gives every current IPv4 and IPv6 session socket
generation one egress exclusion shared by DHT and uTP. A generation is retired
under that exclusion before replacement, removal, or shutdown can publish the
transition. A queued old-generation uTP send becomes an exact stale-generation
failure; DHT retries once against the current generation. Cancellation drops
its waiter accounting, and snapshots report live/high-water waiters, rejected
retired sends, and IPv4 fragmentation-protection capability. No independent
queue or task was introduced, and ordinary product packetization remains fixed
at 548 bytes.

Enabling the existing workspace `rustix 1.1.4` dependency's `net` feature
provides safe Linux/Android `IP_MTU_DISCOVER` access. Construction reads the
exact prior enum, verifies `IP_PMTUDISC_PROBE`, restores and rereads the prior
value, and refuses to publish a socket if restoration is uncertain. The
approved macOS adapter now reports `Verified` after the equivalent exact bool
round trip. Both Android native ABI checks compile their platform-specific
path; an actual option/send run remains part of the later AVD gate.

No safe macOS IPv4 setter/getter exists in the current dependency graph. Two
current registry candidates were inspected without adding either:

- `dontfrag 1.0.1`, `src/lib.rs` and `src/sys/unix/bsd.rs`, supplies a sealed
  safe extension for Tokio UDP sockets, reads and writes Darwin
  `IP_DONTFRAG`, is licensed `MIT OR Apache-2.0`, and adds only focused
  platform bindings already represented transitively in this workspace. It
  is the recommended target-specific macOS dependency; Linux/Android would
  continue using `rustix` so their exact `IP_MTU_DISCOVER` enum is preserved.
- `nix 0.31.3`, `src/sys/socket/sockopt.rs::IpDontFrag`, supplies safe Apple
  get/set operations under the MIT license, but is a broader general-purpose
  Unix API dependency than this one option requires.

Maintainer review approved target-specific `dontfrag 1.0.1` with its `tokio`
feature on 2026-08-11. The dependency and distribution notice are recorded;
the repository's existing `forbid(unsafe_code)` rules remain unchanged.
Focused macOS evidence reads a false prior value, sets and verifies true, and
restores and rereads the exact false value on an actual UDP socket.

Focused evidence at this checkpoint:

- `cargo fmt --all -- --check`;
- `cargo test -p rstorrent-engine --lib`: 504 passed, 7 ignored;
- `cargo clippy -p rstorrent-engine --all-targets -- -D warnings`;
- `cargo ndk -t x86_64 -t arm64-v8a -P 28 check -p rstorrent-engine --lib`
  with the configured NDK; and
- macOS focused tests prove verified option restoration, shared DHT/uTP
  exclusion, cancellation cleanup, and generation replacement fencing.

## Stage 3 Deterministic Revalidation Evidence

The pure state now names `Base`, `Search`, and `SearchComplete` explicitly and
retains the immutable configured base and maximum separately from its current
confirmed floor and search ceiling. Probe outcomes install a saturating
one-smoothed-RTT guard before another probe. Completion schedules
revalidation after 15 minutes, the midpoint of this tactical's accepted
10--30-minute range: this is deliberately conservative relative to RFC 8899's
requirement to confirm PMTU information over time while avoiding frequent
socket-policy changes. Fixed equal bounds never schedule revalidation.

A revalidation protects one DATA packet at the current confirmed floor. Exact
ACK preserves that floor and schedules the next interval. Isolated loss or
local message-too-large reopens search from the conservative configured base
to one byte below the failed floor, lowers the congestion-controller MSS, and
retries the exact already-sequenced payload once without fragmentation
protection. Ambiguous loss does not lower either bound. Counters distinguish
search probes, revalidations, failures, and downward recoveries; deadline math
saturates at the monotonic-clock limit.

The changed probe cadence exposed a pre-existing deterministic loss-recovery
hazard: repeated SACK evidence could re-signal the same retransmitted sequence
several times before that retransmission had one RTT to arrive. Pinned
libtorrent's `utp_stream.cpp::resend_packet` uses its fast-resend sequence fence
to avoid the same burst. RSTorrent now admits a second fast-loss signal for a
retransmission only after one smoothed RTT, or the minimum RTO before an RTT
sample. Timeout recovery remains independently bounded. The original periodic
1% loss fixture passes unchanged, and the independently bounded forward and
reverse simulation queues now assert their correct combined high-water shape
rather than an accidental single-direction timing value.

Stage 3 evidence:

- nine focused path-MTU transition tests cover phase, cadence, convergence,
  successful revalidation, downward recovery, exact packet identity, fixed
  fallback, local errors, ambiguous loss, and saturating deadlines;
- a composed transport test proves downward MSS replacement and an exact
  same-sequence, same-payload, same-size fragmentable retry;
- retransmission tests prove SACK and duplicate-ACK re-signalling cannot occur
  within one RTT;
- the original fixed periodic 1% loss and TCP-like foreground simulations
  pass; and
- `cargo test -p rstorrent-protocol --lib`: 204 passed, 2 ignored, plus
  protocol all-target Clippy with warnings denied.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Exact `Base`/`Search`/`SearchComplete` transitions; cadence; ACK, isolated and ambiguous loss, message-too-large, revalidation, hard-black-hole closure, sequence preservation, wrapping clocks, and all bounds. |
| Scripted platform/runtime | Verified and unsupported capability; exact policy restore on sent/would-block/too-large/error/cancel/replacement; concurrent DHT/ordinary uTP/probe traffic; 548, 1,280, and 1,500 paths; queue saturation and terminal zero ownership. |
| Controlled interoperability | Pinned libtorrent in both roles through ordinary application `PreferUtp`, exact fixture/hash, actual uTP, selected MTU, fallback controls, no TCP masking, and full cleanup. |
| Rate/performance | Alternating fixed/dynamic clean-path cases; at least 50% DATA-datagram reduction; median time/CPU/RSS/queues; unchanged exact stream-byte cap and torrent/session fairness within Tactical `134` tolerances. |
| Desktop/platform | macOS real-socket option/send/restore and application tests; unsupported-platform fixed fallback where applicable. |
| Android parity | x86_64 and arm64-v8a builds plus no-window API 34 AVD option/send/replacement/application/cleanup evidence. |
| Repository | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and web generation/typecheck/tests only if an application-boundary type unexpectedly changes. |

The performance gate uses packet count as its hard efficiency measure because
short loopback wall time is noisy. A material regression is a greater-than-10%
median slowdown across at least five alternating pairs after excluding setup
time, or a corresponding unexplained CPU/RSS/queue increase. Failing that gate
requires diagnosis, not threshold relaxation.

## Staged Execution And Commit Plan

1. Commit this source-first tactical and activate it as the sole **Now**. No
   behavior changes in this stage. This planning stage completed; the tactical
   was subsequently superseded before Stage 2 by explicit maintainer
   reprioritization to Tactical `138`.
2. Prove the safe platform option boundary and exact shared-socket exclusion
   design with focused tests. Stop for human review before adding a dependency,
   unsafe isolation, or a materially different socket owner. Commit the
   accepted feasibility seam separately. The dependency-free portion is
   complete, including the approved target-specific macOS dependency and
   actual host proof.
3. Extend pure MTU state with explicit revalidation/downward recovery and add
   hostile deterministic cases. Commit without enabling product behavior.
   Complete with a 15-minute interval and an RTT-fenced repeated fast-loss
   repair.
4. Carry protected-send intent and typed feedback through the generation-
   fenced runtime, enable dynamic construction only behind positively verified
   capability, and commit scripted shared-DHT/lifecycle evidence.
5. Extend the controlled application/libtorrent fixture and alternating
   fixed/dynamic rate/performance matrix. Commit exact interoperability,
   efficiency, and resource evidence.
6. Run macOS, both Android native builds, the API 34 AVD, and complete
   repository gates; record actual evidence, reconcile owning topics, and
   commit closure.

## Autonomy And Human Review Contract

Once implementation is explicitly authorized, ordinary pure-state changes,
the shared egress refactor, conservative cadence selection within the declared
range, scripted fixtures, controlled local libtorrent work, no-window AVD use,
diagnostic counters, bounded bug fixes at these owners, documentation updates,
and logical commits proceed autonomously.

Stop for human direction before:

- any new external dependency or changed license/distribution posture;
- any unsafe block, unsafe helper crate, FFI boundary, or relaxation of an
  existing `forbid(unsafe_code)` rule;
- a platform API that cannot restore the exact prior socket policy, a global
  always-DF mode, a separate uncoordinated UDP socket, or a material redesign
  of the shared session UDP owner;
- a user-visible/persisted setting, generated-contract change, migration, or
  different TCP/uTP selection or fallback behavior;
- IPv6, proxy, ICMP parsing, destination persistence, UDP mapping,
  advertisement, NAT traversal, public/WAN/`pimom`, visible-client, or
  physical-device work;
- changing rate-limit semantics, congestion control, or the BEP 29 claim; or
- accepting data loss, indefinite connection retention, or a fragmentable
  retry that changes packet sequence/payload identity.

The next human review is therefore the platform feasibility decision if the
safe macOS route needs new authority. If the current dependency graph proves a
safe portable route, implementation may continue through controlled, AVD, and
repository evidence before the next prudent review at tactical completion.

## Next-Slice Boundary

Completion removes dynamic IPv4 product MTU and portable fragmentation-
protection gaps only for the positively verified platform set. IPv6 PMTU,
shared destination caching, route-change reaction, ICMP assistance, incoming
reachability/advertisement, MSE-over-uTP, racing, and full BEP 29 graduation
remain separately selected work.
