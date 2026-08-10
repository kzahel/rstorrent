# Tactical 118: uTP Implementation Decision Spike

Status: At the first human review on 2026-08-10 as the single authoritative
**Now**. Stage 0 evidence is complete and the independent Rust path is
recommended, but no implementation source or dependency has been accepted.
This tactical authorizes source investigation, reference pinning, and a
bounded loopback oracle only; it does not authorize uTP engine implementation
or adoption of a runtime dependency.

Topics: `utp-transport-campaign`, `capability-readiness`,
`oracle-driven-engine-campaign`, `peer-lifecycle`,
`performance-and-live-evidence`, `protocol-support`, `references`

Dependencies: completed Tacticals
[`111`](111-mse-peer-stream-encryption.md),
[`112`](112-dual-stack-transport-and-ipv6-dht.md), and
[`115`](115-mse-policy-advertisement-and-peer-detail.md) establish the
ordered-stream encryption seam, coordinated per-family TCP/UDP socket owner,
and current peer-policy boundary that a later uTP implementation must compose
with rather than replace.

## Decision And Desired Outcome

Produce enough reproducible evidence to choose honestly among:

1. an independently authored Rust sans-IO uTP core with a small RSTorrent
   runtime adapter;
2. a pinned standalone `libutp` C/C++ implementation behind FFI or vendoring;
   and
3. a pinned `librqbit-utp` Rust/Tokio dependency or a deliberately adapted
   derivative.

The decision must account for BEP 29 and RFC 6817 behavior, mature edge cases,
license and source provenance, platform builds, buffer and task ownership,
cancellation and socket-generation replacement, shared-UDP integration, and
the ability to prove forced-uTP interoperability. Convenience or language
match alone is not sufficient.

## Stopping Condition

This spike stops for human review when all of the following are recorded:

1. Exact normative, libtorrent, standalone-libutp, and librqbit-utp revisions,
   source paths, tests, licenses, and maintenance state are reproducible.
2. A retained loopback-only oracle recipe forces pinned libtorrent to use uTP
   with TCP, DHT, LSD, NAT-PMP, and UPnP disabled; it transfers a bounded
   independently generated payload, proves the observed peer transport, checks
   the exact content hash, bounds time and peers, joins both sessions, and
   removes temporary state.
3. The current post-Tactical-112 socket, UDP receive, peer-stream, MSE, task,
   and generation boundaries are mapped against each candidate.
4. Each candidate has an explicit dependency and transitive-cost summary,
   platform-build result or documented limitation, lifecycle/resource-risk
   assessment, provenance obligations, and rejected or unresolved conditions.
5. One recommendation and a bounded Stage 1 tactical shape are presented,
   while the repository still contains no uTP product code, dependency,
   vendored implementation, setting, advertisement, or support claim.

## Normative And Oracle Sources

- BEP 29 from managed `reference/bittorrent.org` commit
  `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06` is the wire and deployed uTP
  starting point.
- RFC 6817 is the LEDBAT congestion-control reference, including its maximum
  100 ms target, gain and growth bounds, base/current delay histories, loss
  response, idle behavior, and congestion-timeout requirements.
- Rasterbar libtorrent `2.0.13`, commit
  `7d7fc38fac61177fa5e02148f791b2f65250b09d`, is the primary completeness and
  executable interoperability oracle.
- Standalone BitTorrent `libutp` is pinned at
  `2b364cbb0650bdab64a5de2abb4518f9f228ec44`. Crates.io `librqbit-utp`
  `0.7.0` is pinned at its embedded VCS revision
  `c26f57b2debbe35ed0ace1ad419de529f7a5bf95` and package checksum
  `4f3bfdc73944bc76cab24d5690a98816770040a654c449edf5ff2b9ba22626aa`.
  Both remain comparison candidates only.

No source, fixture, or test data from an oracle is copied into RSTorrent. The
GPL `simulation/libsimulator` submodule may be inspected through libtorrent's
tracked simulation test but is not initialized, linked, run, or distributed.

## Investigation Boundaries

### Current RSTorrent owner map

- `rstorrent-protocol` owns hostile codecs and deterministic protocol state;
  it must remain independent from Tokio, sockets, task handles, and clocks.
- The application transport generation owns one coordinated TCP/UDP pair per
  enabled address family and fences replacement generations.
- `SessionUdpService` owns one receive task per enabled family. Its current
  1,025-byte receive buffer and one 64-entry DHT ingress queue cannot be reused
  unchanged for uTP payloads or independent queue pressure.
- A later uTP runtime owner must own connection lookup, bounded per-connection
  state, timers, wakeups, cancellation, and joined termination while borrowing
  the session UDP socket generation.
- The existing peer runtime continues to own peer identity, duplicate
  resolution, BitTorrent handshake, request scheduling, and payload work over
  an ordered byte stream. MSE remains a stream wrapper rather than part of the
  uTP packet core.

### Resource and external-action bounds

- The executable oracle is loopback-only and uses at most two libtorrent
  sessions, one torrent, one peer connection per side, a payload no larger
  than 4 MiB, and a 30-second overall deadline.
- Discovery, TCP peer transport, port mapping, public trackers, DHT, LSD,
  public swarms, `pimom`, physical devices, emulators, packet capture, and
  externally reachable listeners are out of scope.
- Reference builds use isolated ignored or temporary targets. Temporary
  payloads, profiles, logs, and build products are removed after evidence is
  extracted.
- No RSTorrent `Cargo.toml`, lockfile, product source, generated client,
  persistence contract, or platform package changes in this spike.

## Required Source And Test Survey

The execution record must name the exact functions or cases inspected,
including at least:

- libtorrent `utp_socket_impl::incoming_packet`, `parse_sack`, `ack_packet`,
  `experienced_loss`, `do_ledbat`, `packet_timeout`, `tick`, send/receive
  buffering, FIN/RESET handling, and MTU probing;
- libtorrent `utp_socket_manager::incoming_packet`, `tick`, deferred ACK and
  socket-drain behavior, writability, connection-ID lookup, socket removal,
  and incoming-SYN limits;
- libtorrent `test/test_utp.cpp`, `simulation/test_utp.cpp`, and
  `fuzzers/src/utp.cpp`;
- standalone libutp's packet parser, callbacks, timeout/loss/LEDBAT/MTU state,
  build targets, tests, and API/lifecycle contract; and
- librqbit-utp's codec, sequence arithmetic, recovery/RTO, flow control, MTU,
  connection dispatcher, task/channel ownership, stream API, congestion
  controller, tests, and open TODOs.

## Validation

- Managed reference status passes for the four external uTP sources used by
  this spike; a dirty first-party JSTorrent sibling may be inspected read-only
  and is reported separately rather than modified.
- The retained oracle self-validates its settings, peer endpoints, transport
  counters, peer high-water marks, exact payload, deadline, and cleanup, and
  passes one bounded forced-uTP transfer on this machine.
- Candidate source packages pass their applicable native tests/builds in
  isolated targets. Android or other cross-target checks are run only where
  the candidate claims support and the installed toolchain can exercise it;
  an unavailable target is recorded, not inferred as passing.
- Repository documentation links and formatting are checked. The Rust
  workspace baseline is unnecessary because this tactical changes no Rust or
  product dependency; any broader validation actually run is still recorded.

## Non-Goals

- No uTP codec, connection state, stream adapter, UDP demultiplexer, peer
  integration, transport selection, racing, fallback, advertisement, MSE-over-
  uTP composition, port mapping, pinhole, NAT traversal, or product setting.
- No choice of final congestion constants or Stage 2 acceptance thresholds.
- No WAN, LAN-device, public-swarm, emulator, or physical-device evidence.
- No support change to the BEP 29 protocol matrix.

## Escalation And First Review

Ordinary source inspection, managed reference pinning, independently authored
oracle code, local builds/tests, documentation updates, and cleanup proceed
autonomously. Stop immediately before copying, mechanically translating,
vendoring, linking, wrapping, or adding any uTP runtime implementation or
dependency.

The first human review receives the complete comparison, the recommended
implementation/provenance choice, explicit rejected alternatives, oracle
evidence, unresolved risks, and two or three bounded Stage 1 choices. No Stage
1 implementation tactical starts before that review.

## Execution Record

### Normative behavior and edge-case checklist

The full managed `bep_0029.rst` and RFC 6817 sections 1--5 were read. Stage 1
must preserve the following independent behavior checklist rather than infer
support from a header codec:

- v1's 20-byte header, paired connection IDs, packet-based wrapping sequence
  and acknowledgement numbers, all five packet types, linked extensions with
  safe unknown-extension skipping, and SACK length and bit ordering;
- SYN initialization, DATA/STATE acknowledgement, duplicate and reordered
  packet handling, three-duplicate/SACK loss signals, receive-window gating,
  FIN ordering, RESET handling, and bounded work on malformed chains;
- one-way timestamp feedback, clock and timestamp wrap, base/current delay
  histories, the 100 ms target, RFC 6817 gain/growth limits, congestion-window
  gating, loss response, idle restart, and congestion timeout; and
- RTT/RTO sampling, exponential timeout, the BEP's 500 ms RTO floor and
  small-packet restart behavior, variable packet sizes, path MTU behavior, and
  explicit treatment of deployed behavior that differs from the documents.

BEP 29's two-minute sliding base-delay description, standalone libutp's
13-minute history, and RFC 6817's recommended ten-minute history are not
silently collapsed into one rule. The Stage 2 congestion tactical must choose
and test the behavior explicitly. Libtorrent's one-MSS congestion-window floor
is likewise a deliberate RFC-aligned interoperability choice rather than a
literal adoption of BEP 29's zero-window description.

### Pinned libtorrent survey

The primary oracle was Rasterbar libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`, under BSD-3-Clause for the files
below. No source or fixture was copied.

- `include/libtorrent/aux_/utp_stream.hpp` and `src/utp_stream.cpp`:
  `utp_socket_impl::incoming_packet`, `consume_incoming_data`, `parse_sack`,
  `ack_packet`, `experienced_loss`, `do_ledbat`, `packet_timeout`, `tick`,
  `send_pkt`, `resend_packet`, `write_sack`, `send_syn`, `send_fin`,
  `send_reset`, `init_mtu`, and `update_mtu_limits` were inspected. The
  implementation bounds receive storage to 1 MiB by default, bounds reorder
  distance from that capacity, validates extension lengths, skips unknown
  extensions, ignores impossible future ACKs, distinguishes MTU-probe loss
  from congestion, and makes timeout/retransmission generations explicit.
- `include/libtorrent/aux_/utp_socket_manager.hpp` and
  `src/utp_socket_manager.cpp`:
  `incoming_packet`, `tick`, `mtu_for_dest`, `send_packet`,
  `subscribe_writable`, `writable`, `socket_drained`, `defer_ack`,
  `cancel_deferred_ack`, `subscribe_drained`, `remove_udp_socket`,
  `remove_socket`, and `new_utp_socket` were inspected. The manager rejects
  short/non-v1 datagrams, keys established sockets by endpoint and connection
  ID, admits new sockets only for enabled SYN traffic under a connection
  ceiling, and coordinates deferred ACK and UDP writability state.
- `test/test_utp.cpp` forces uTP by disabling both TCP directions and verifies
  a small transfer plus wrapping comparison. `simulation/test_utp.cpp`
  covers ordinary transfer, PMTU discovery, bufferbloat, small path queues,
  and small kernel send buffers. `fuzzers/src/utp.cpp` feeds hostile datagrams
  through the manager and drains deferred ACKs. The GPL simulation submodule
  was not initialized or executed.

Libtorrent's protocol implementation, Asio stream service, socket manager,
settings, alerts, and object lifetime are tightly integrated. It is the
completeness and executable oracle, not a viable source transplant.

### Standalone libutp survey

BitTorrent libutp at `2b364cbb0650bdab64a5de2abb4518f9f228ec44`
is MIT-licensed and last changed upstream on 2018-05-15. Its README defines a
non-thread-safe, single-threaded asynchronous C callback API over a C++ core,
calls that API permanently unstable, and recommends bundling and testing an
exact revision.

`utp_process_udp`, `utp_process_incoming`, `UTPSocket::apply_ccontrol`,
`UTPSocket::check_timeouts`, selective acknowledgement and retransmit paths,
MTU state, `utp_check_timeouts`, and `utp_issue_deferred_acks` were inspected.
The host must supply send, read/write, clock, random, MTU, state, and error
callbacks and call timeout and deferred-ACK pumps. The implementation uses
1,024-packet outgoing and reorder buffers, a hard-coded incoming socket
ceiling, individually allocated reorder payloads, assertions, and legacy
LEDBAT constants and histories that are not the RFC 6817 contract RSTorrent
would want to expose.

`make -j2 all` passed on this arm64 macOS host, with two unused-constant
warnings, and `make clean` restored the reference checkout. The repository has
old MSVC projects and a POSIX Makefile but no Android build target or modern
cross-platform build description. Its README names an `utp_test` directory
that is absent from the pinned checkout; no maintained automated test suite is
available there. Adopting it would add a C++ toolchain, handwritten or
generated FFI, callback/lifetime synchronization, allocator and panic/abort
auditing, Android build work, a vendored exact pin, and MIT notice tracking.

### librqbit-utp survey

Crates.io `librqbit-utp` `0.7.0` and its matching VCS source are Apache-2.0.
The package has 14 direct normal dependency entries. Its locked host normal
graph contains 57 packages including the root; 14 package names are absent
from the current RSTorrent lockfile: `backon`, `bitvec`, `dontfrag`, `funty`,
`futures`, `librqbit-dualstack-sockets`, `librqbit-utp`, `network-interface`,
`pin-utils`, `radium`, `ringbuf`, `tap`, `tracing-attributes`, and `wyz`. The
optional metrics feature is off by default.

The raw codec, wrapping sequence arithmetic, RTT/RTO estimator, recovery,
SACK, receive flow control, send segmentation, delayed ACK, fast retransmit,
MTU probing, socket dispatcher, stream halves, cancellation helpers, shutdown
tests, and lossy transport tests were inspected. The source has useful numeric
buffer, retransmission, inactivity, live-stream, SYN-cache, and accept limits,
and its `UtpStream` already implements Tokio ordered-stream traits.

It is not a BEP 29 congestion implementation: `src/lib.rs` explicitly leaves
LEDBAT as a TODO and the only selectable controller is CUBIC. Its per-stream
packet ingress and socket control paths use unbounded Tokio channels; the
former is explicitly marked `TODO: make bounded`. Open TODOs include SYN retry,
duplicate ACK behavior, and MTU policy. Its task-per-virtual-socket lifecycle
and socket ownership also overlap RSTorrent's already accepted session UDP and
peer-task owners.

The exact published package passed 76 native tests with 2 lossy tests ignored,
including an end-to-end case against `libutp-rs2`. Its library also passed
`cargo check --locked --target aarch64-linux-android --lib` with the installed
Android target. These are useful implementation and platform signals, but do
not repair the congestion-controller and hostile-ingress ownership mismatch.
Depending on it directly is rejected. An adapted derivative would need enough
congestion, queue, task, and shared-socket surgery that its apparent schedule
advantage is not credible without accepting a long-lived Apache-2.0 fork.

### RSTorrent boundary and JSTorrent findings

The landed owner map confirms a natural independent implementation seam:

- `session_socket.rs` creates the coordinated TCP/UDP socket pair per family.
  `session_udp.rs` owns one cancellable and joined receive task per enabled
  family and generation-aware current sockets. It presently reads at most
  1,025 bytes, allocates one `Vec` per accepted datagram, and routes everything
  through one 64-entry DHT queue. A later runtime slice must classify once,
  give DHT and uTP separate bounded pressure, increase the physical receive
  buffer deliberately, and fence queued datagrams by socket generation.
- `peer_runtime.rs` already owns transport-neutral connection identity,
  direction, lifecycle, and `PeerTransport::{Tcp,Utp}` observation.
  `peer_socket.rs`, `incoming.rs`, and `peer_io.rs` still own concrete
  `TcpStream` values, and outgoing MSE helpers also accept `TcpStream`.
  The first runtime uTP slice must extract the smallest ordered-stream seam
  justified by the second transport while preserving peer budgets, duplicate
  resolution, cancellation, and joined terminal paths.
- MSE's protocol state is already sans-IO, but MSE-over-uTP remains a later
  composition and interoperability slice rather than part of the uTP core.

The current first-party JSTorrent sibling contains no relevant uTP engine or
test implementation; only product copy mentions uTP. It supplied no behavior
to adopt. Its unrelated dirty files were left untouched, so the four external
managed references were checked separately rather than claiming the full
reference set clean.

### Candidate decision matrix

| Candidate | Ownership and bounds | Congestion fidelity | Build/dependency cost | Decision |
| --- | --- | --- | --- | --- |
| Independent Rust sans-IO core | Best fit: deterministic hostile-input state can own exact byte/packet/work limits and emit actions to the existing socket/task owners. | Must be authored and proven; can implement RFC 6817 deliberately while retaining explicit BEP/libtorrent compatibility differences. | No foreign runtime or FFI dependency; pure core is platform-neutral and later adapter uses already-present Tokio. Highest implementation/test effort. | **Recommended**, subject to this review. |
| Standalone libutp FFI/vendor | Callback and global-context lifetime, hard-coded storage, periodic pumps, and assertions require a substantial safety and ownership adapter. | Implements mature legacy LEDBAT, but constants/history and minimum window differ from the desired explicit RFC contract. | Dormant C++ source, unstable API, no pinned tests, no Android build result, vendoring/FFI and MIT notice obligations. | Reject as product dependency; retain as secondary reference peer. |
| librqbit-utp direct/adapted | Useful bounds exist, but unbounded hostile packet/control channels and overlapping socket/task owners violate current invariants. | CUBIC only; LEDBAT is an explicit TODO. | Native tests and Android check pass, but a direct dependency adds new packages; a fork must replace core policy and retain Apache-2.0 provenance. | Reject direct adoption and a fork unless new evidence overturns the scope estimate. |

### Retained forced-uTP oracle

[`tests/interop/utp_reference_oracle.py`](../../tests/interop/utp_reference_oracle.py)
is independently authored and uses the locked Python libtorrent `2.0.13.0`.
It creates two loopback-only sessions and one trackerless v1 torrent, disables
both TCP directions, DHT, LSD, NAT-PMP, UPnP, and MSE, enables both uTP
directions, manually connects exactly one loopback peer, and enforces a
30-second overall deadline. It generates and verifies a 2,097,883-byte
payload, records session uTP/TCP counters and peer high-water marks, pauses and
destroys both sessions, removes temporary state, and emits JSON.

The final 2026-08-10 run completed the transfer in 0.712480 seconds, cleanup in
0.211098 seconds, and the entire scenario in 0.969253 seconds. Both peer
high-water marks were one, both sides sent and received uTP packets, both
recorded zero TCP peers, and the downloaded SHA-1 was
`cdce24126a8e65854d876c0b83ad3ba19748f6dc`. No WAN, LAN device, external host,
public swarm, port mapping, packet capture, or product client was used.

### Recommendation and Stage 1 choices

Approve an independently authored Rust sans-IO path, with libtorrent as the
primary completeness/interoperability oracle and the two candidate libraries
as read-only secondary references. This is the only candidate that keeps
hostile input bounded at the protocol owner, composes with the landed shared
UDP generation, avoids a second socket/task hierarchy, and permits explicit
RFC 6817 decisions without inheriting a fork.

If approved, the recommended next tactical is a pure protocol slice only:

1. add independently authored v1 header/extension codecs and wrapping
   sequence/timestamp arithmetic;
2. add bounded deterministic connection state for SYN, STATE, DATA, FIN,
   RESET, receive ordering/SACK, send/ACK ledgers, loss signals, and timer
   intents, with exact packet/byte/work/half-open bounds fixed in the tactical;
3. validate malformed chains, unknown extensions, wraps, duplicates,
   reordering, zero-window behavior, loss signals, FIN/RESET races, stale
   generation input, and terminal memory counters without Tokio, sockets,
   filesystem, peer-wire, MSE, LEDBAT, pacing, or MTU discovery.

That slice stops before congestion control and real UDP. Its result determines
whether LEDBAT/loss/MTU remain one deterministic Stage 2 or split based on
measured state complexity.

The review alternatives are: (A) approve that recommended pure Rust Stage 1;
(B) instead authorize a libutp FFI/platform feasibility tactical before any
product integration; or (C) instead authorize a librqbit hardening/LEDBAT fork
feasibility tactical. B and C preserve the present no-adoption state until
their own review gates, but both start with the material costs and blockers
recorded above.

### Validation evidence

- `python3 scripts/references.py status --only bittorrent-beps --only
  libtorrent --only libutp --only librqbit-utp`: passed at all four exact
  revisions.
- Full `python3 scripts/references.py status` returned nonzero only because the
  first-party `../jstorrent` sibling already has unrelated working-tree changes
  in one screenshot and three investigation/design records. All five external
  checkouts, including rqbit, reported their exact expected revisions; the
  sibling was not modified.
- `make -C reference/libutp -j2 all`: passed on arm64 macOS; two
  unused-constant warnings; `make clean` passed and the checkout is clean.
- Exact crates.io `librqbit-utp` `0.7.0` `cargo test --locked`: 76 passed, 0
  failed, 2 ignored; isolated 991.5 MiB target cleaned.
- Exact crates.io `librqbit-utp` `0.7.0` Android library check for
  `aarch64-linux-android`: passed; isolated 147.5 MiB target cleaned.
- `uv run --project tests/interop --locked python
  tests/interop/utp_reference_oracle.py`: passed with the result summarized
  above and no retained payload or session state.

No Rust workspace, lockfile, product source, runtime dependency, generated
client, visible client, external host, or physical device changed or ran.
