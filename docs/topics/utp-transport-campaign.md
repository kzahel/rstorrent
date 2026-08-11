# uTP Transport Campaign

Topic: `utp-transport-campaign`

Status: Stage 0 Tactical
[`118`](../tactical/118-utp-implementation-decision-spike.md) and Stage 1
Tactical [`119`](../tactical/119-deterministic-utp-transport-core.md) are
complete. Human review selected recommendation A for deterministic Stage 2
Tactical [`121`](../tactical/121-deterministic-utp-loss-congestion-and-mtu.md),
then selected Stage 3 recommendation A at its stopping condition. Completed
Tactical [`125`](../tactical/125-shared-udp-utp-runtime-and-loopback-interop.md)
now owns bounded shared-UDP/runtime and two-role loopback interoperability.
Human review accepted Stage 4 recommendation A, and Tactical
[`126`](../tactical/126-controlled-outbound-utp-wan-evidence.md) records one
planned outbound-only controlled WAN attempt after a direct-route preflight.
That preflight closed evidence-limited before traffic because the host had no
directly assigned global IPv4 endpoint or exact oracle. Human review then
corrected that precondition and authorized Tactical
[`127`](../tactical/127-mapped-utp-wan-interoperability.md): set up the exact
oracle on the NATed host, try remote UDP UPnP first, and use a local UDP mapping
with reversed roles only if remote reachability capability is absent. Tactical
`127` is complete after one exact remote-mapped direct-public-path transfer and
verified cleanup. Human review then authorized Tactical
[`130`](../tactical/130-utp-transport-solidification.md) to complete the
complementary WAN direction, a small bidirectional cohort, real-socket
impairment/lifecycle hardening, and diagnostic-only MTU integration before the
pre-product review. Tactical `130` is now closed with its WAN cohort evidence-
limited and every controlled gate passing. Human review selected bounded
product-composition recommendation A as Tactical
[`131`](../tactical/131-bounded-product-utp-composition.md): IPv4/plaintext,
fixed 548, and default off, with source-first uTP/TCP selection and sequential
fallback. Tactical `131` is complete, and its required product-enablement
review selected the bounded default-readiness recommendation. Default
readiness Tactical
[`132`](../tactical/132-utp-default-readiness-evidence.md) is now complete
after bounded endpoint memory, mixed real-socket evidence, retained product
interop, and one successful metadata-only public observation. Human review
selected bounded default enablement recommendation A. Tactical
[`133`](../tactical/133-utp-product-default-enablement.md) is complete: the
common application default is now `PreferUtp`, desktop/Android lifecycle and
build gates plus controlled fallback pass, and the implemented BEP 29 subset
is **Partial**. Tacticals `134`, `135`, and `136` subsequently completed their
independent rate-policy, storage, and tracker work. Maintainer direction then
selected ready Tactical
[`137`](../tactical/137-product-utp-path-mtu-discovery.md), but reprioritized
verified HTTP file serving before implementation. Tactical `138` is complete,
and maintainer direction now reactivates Tactical `137` as the authoritative
**Now** with end-to-end implementation and logical commits authorized.

## Scope And Ownership

This topic owns the adaptive campaign for adding
[BEP 29 uTP](https://www.bittorrent.org/beps/bep_0029.html) as a first-party
peer transport. It keeps the implementation-choice investigation, provisional
owner boundaries, evidence ladder, human review gates, and restart checkpoint
coherent across future bounded tacticals.

This is deliberately an umbrella topic rather than one large tactical. uTP is
a reliable ordered byte stream, a loss-recovery protocol, a delay-based
congestion controller, a shared-UDP demultiplexing concern, and an ordinary
BitTorrent peer transport. Its detailed implementation sequence will be
shaped by failures and measurements. Each child tactical must still have fixed
scope, invariants, resource limits, evidence, and a falsifiable stopping
condition before code changes begin.

This topic does not by itself authorize a dependency, source import, or
protocol-support claim. The readiness queue separately controls promotion and
the single authoritative **Now**.

## Current Truth

- Every ordinary product client now inherits `PreferUtp` from the common
  durable/ephemeral application constructor. The session accepts incoming
  IPv4 plaintext uTP, eligible outgoing dials prefer uTP and fall back
  sequentially to TCP after joined transport failure, and the existing peer
  lifecycle reports the actual selected/final transport. `TcpOnly` remains an
  explicit diagnostic construction override. There is no persisted setting,
  presentation, UDP mapping, tracker/DHT incoming-endpoint advertisement, or
  public incoming-reachability claim.
- Reactivated Tactical `137` defines and has begun the next bounded uTP slice:
  activate the existing
  548--1,472-byte IPv4 search only behind positively verified fragmentation-
  protected sends and one shared DHT/uTP egress exclusion boundary. It adds
  path revalidation, conservative fixed-548 fallback, controlled packet-count
  and pinned-libtorrent evidence, macOS proof, and Android build/AVD parity.
  Its dependency-free seam now serializes all DHT/uTP sends per family, fences
  retired generations, reports contention and capability, and uses existing
  `rustix` for safe Linux/Android option verification; both Android ABIs
  cross-build. The approved target-specific `dontfrag 1.0.1` adapter passes
  actual macOS set/get/exact-restore proof. Pure state now revalidates after 15
  minutes, reopens from the conservative base after an isolated floor failure,
  preserves the exact fragmentable retry, and fences repeated fast-loss
  signals by one RTT. Stage 4 now carries protected intent through the shared
  egress transaction, restores and rereads before unlock, repairs a
  contaminated generation through the existing session owner, and selects
  dynamic product MTU only for a positively verified IPv4 socket. A 256 KiB
  macOS product transfer selected at least 1,456 bytes with exact content and
  zero terminal ownership. No unsafe project code, setting, external run, or
  protocol-claim change has landed; controlled interop/rate evidence is the
  current stage.
- Completed Tactical `132` removes the repeated five-second tax without a
  second cache. Each bounded per-torrent endpoint record is unknown,
  advertised, confirmed, or suppressed. Actual uTP transport results update
  the same generation before dial settlement; suppression retries after five
  minutes through a one-hour exponential cap, and valid BEP 11 uTP flags
  restore advertised support. The state is volatile, endpoint-scoped, and
  cannot outlive or prevent eviction of its existing record.
- The session's one bounded UDP receive owner per family now classifies shallow
  uTP shape before DHT and feeds independent 256-entry uTP and 64-entry DHT
  routes. Each connection has a separate 64-datagram queue; generation-tagged
  receive and send fence socket replacement/removal. DHT retains its 1,025-
  byte malformed sentinel and independent pressure/termination behavior.
- Completed Tactical
  [`112`](../tactical/112-dual-stack-transport-and-ipv6-dht.md) changed the
  session to one coordinated TCP/UDP socket pair per enabled address family.
  Tactical `118` inspects that landed owner rather than designing around the
  former IPv4-only shape.
- Current product UPnP behavior maps TCP only. Tactical `127` generalized the
  existing engine mapping owner to an explicit TCP/UDP value while retaining
  every product call as TCP; UDP remains diagnostic-only. The successful run
  used the remote gateway rather than the local-listener fallback. No product
  tracker/DHT incoming-uTP advertisement follows.
- Completed Tacticals
  [`111`](../tactical/111-mse-peer-stream-encryption.md) and
  [`115`](../tactical/115-mse-policy-advertisement-and-peer-detail.md)
  deliberately exclude uTP and MSE over uTP. Their sans-IO MSE state machine
  and policy helpers remain reusable once uTP exposes the same ordered peer-
  stream boundary; Stage 5 must compose and verify that path explicitly.
- The retained
  [`utp_reference_oracle.py`](../../tests/interop/utp_reference_oracle.py)
  proves the forced-uTP libtorrent baseline. The independent
  [`utp_rstorrent_interop.py`](../../tests/interop/utp_rstorrent_interop.py)
  now proves the same exact payload with RSTorrent as leecher and seed against
  pinned libtorrent. Completed Tactical `127` additionally proves the leecher
  role over one mapped direct public path, and Tactical `130` proves the first
  reverse sample. Tactical `132` adds one ordinary-swarm outgoing observation;
  no publicly reachable/advertised product listener or repeatable WAN cohort
  exists.
- The retained
  [`utp_product_integration.py`](../../tests/interop/utp_product_integration.py)
  proves the inherited application default against pinned libtorrent in
  incoming uTP, outgoing uTP, and TCP-only fallback roles. All three transfer
  and independently hash the exact 2,097,883-byte fixture with one application
  peer, actual transport views, bounded fixed-runtime counters, and joined
  shutdown. The explicit `TcpOnly` TCP/Fast/MSE application regression passes
  with no uTP service.
- Completed Tactical `119` now supplies the independently authored,
  dependency-free v1 codec and deterministic bounded connection/reliability
  state in `rstorrent-protocol`. Its 41 focused tests and full workspace
  baseline pass, but the core owns no socket, has exchanged no RSTorrent uTP
  datagram, and does not change TCP-only peer execution or the support claim.
- Completed Tactical `121` composes exact 1-MiB receive credit, a 1-MiB
  unsent stream queue, packetization, delayed ACKs, retransmission execution,
  fixed-point RFC 6817 congestion/pacing, and binary path-MTU discovery into
  one runtime-free transport state. Fixed encoded 2/4/8-MiB transfers pass
  clean, jitter/reorder/duplicate, 1% loss, queue, timestamp wrap/drift,
  receive-pressure, 1,280-byte black-hole, and established TCP-like
  foreground gates. The largest observed sent ledger was 59 packets/61,338
  bytes, receive ownership reached the exact 1-MiB bound, and link events
  reached 81 datagrams/80,239 bytes. This still owns no socket or task and has
  exchanged no RSTorrent datagram with another implementation.
- Completed Tactical `125` composes that state with the shared session UDP
  socket, one supervised service and worker per live connection, bounded
  ordered streams, and one concrete TCP/uTP peer-stream enum. Ten runtime and
  twelve shared-UDP cases pass. Pinned libtorrent transfers the exact
  2,097,883-byte fixture in both roles with one loopback uTP peer, zero TCP
  peers, exact SHA-1, no packet drops or worker panics, and terminal zero
  ownership. Product selection, WAN behavior, IPv6 uTP, MSE-over-uTP, active
  real-socket MTU discovery, and a support claim remain absent.
- Closed evidence-limited Tactical `126` used SSH only for one authorized
  read-only `pimom` preflight. The host exposed loopback, RFC 1918 LAN, and
  Tailscale/shared-range IPv4 addresses but no directly assigned global IPv4,
  and system Python lacked libtorrent. No fixture, listener, uTP packet,
  package install, network change, or WAN interoperability result followed.
- Completed Tactical `127` treats those same interface facts correctly as a
  NATed peer. It owns isolated pinned-oracle setup, verified finite UDP UPnP
  mapping on the remote gateway or capability-gated local fallback, direct-
  route proof, exact transfer evidence, and mapping/process/artifact cleanup.
  Its first checkpoint installed the exact official libtorrent `2.0.13.0`
  ARM64/Python 3.13 wheel in a dedicated user environment without system
  packages, and non-mutating discovery found a connected remote UPnP gateway
  reporting an eligible external IPv4 address. The first mapping attempt sent
  no uTP traffic because libtorrent automatically installed both TCP and UDP
  leases. An independent audit caught that both survived the initial cleanup;
  exact deletion then proved zero residue. The repaired harness disables that
  dual mapper, uses one explicitly named MiniUPnP UDP lease, and audits cleanup
  by PID, description, port, and directory even before readiness. Its final
  run transferred the exact 2,097,883-byte fixture from the mapped remote
  libtorrent seed to RSTorrent in 82.239 seconds with one uTP peer, zero TCP
  peers, exact SHA-1, zero loss/retransmission counters, bounded queues, and
  terminal zero ownership. Independent post-run audit found no mapping,
  process, or per-run artifact residue.
- Closed Tactical `130` owns the pre-product transport-solidification record:
  RSTorrent bulk sending through one exact temporary local UDP mapping, an
  evidence-limited bidirectional WAN cohort, passing fixed real-socket
  impairment and hostile lifecycle gates, and an explicit diagnostic MTU
  configuration. Ordinary runtime remains fixed at 548 bytes pending a
  separately authorized product decision.
- Tactical `130`'s real-socket delay/jitter gate found two independently
  repaired interoperability defects. Accepted established packets now reset
  timeout backoff from receive time, matching pinned libtorrent without
  allowing wrong connection IDs to affect state. The inbound codec also
  tolerates libtorrent's bounded 1--252-byte SACK payloads while the RSTorrent
  encoder retains BEP 29's at-least-four-byte, four-byte-aligned shape. This
  explicit compatibility exception turned the prior 886 malformed-packet
  stall into a 21.283-second exact transfer with zero malformed or queue drops
  and terminal zero ownership. A follow-up repair prevents retransmissions
  from growing beyond their selected ordinary/probe MTU when current SACK
  state needs a larger header; the repeated exact profile completed in 21.122
  seconds with a maximum 548-byte DATA datagram. Ordinary product uTP remains
  disabled.
- Tactical `130`'s complete six-profile real-socket matrix passes in 71.312
  seconds. Clean, alternating delay/jitter, one-percent sparse DATA loss,
  exact duplicate/reorder ordinals, a three-DATA burst loss, and the fixed-MTU
  black-hole baseline all produce the exact stream and terminal zero
  ownership. The applied profiles observe up to 40 relay drops, 52 duplicates,
  78 explicit reorder selections, 210 RSTorrent retransmissions, and 196 loss
  reductions without timeout collapse, runtime queue loss, worker panic, or a
  DATA datagram above 548 bytes. The final fixed-matrix rerun also passes with
  every ordinary candidate fixed at 548 and zero probe counters.
- Tactical `130`'s hostile/lifecycle gate now combines 14 uTP runtime cases
  with 11 shared-session UDP cases. It proves endpoint-scoped spoofed
  RESET/STATE isolation, duplicate-SYN reuse, exact 16 half-open/incoming and
  64 global connection caps, 64-datagram per-connection and 256-datagram
  shared queue pressure, consumer drop during retransmission, service and UDP
  generation cancellation, and eight repeated start/stop cycles. Every
  terminal assertion reports zero active connections/half-opens and no worker
  panic.
- Tactical `130` now exposes the existing 548--1,472-byte path-MTU search only
  through an explicit loopback diagnostic service configuration. Ordinary
  runtime construction remains fixed at 548 bytes and its duplex regression
  observes zero probes. Structured runtime evidence separates the proven MTU
  floor, current candidate, probe outcomes, probe datagrams, and same-sequence
  fragmentable retries. The controlled real-socket size-black-hole proof
  transfers and verifies the exact fixture in 4.200 active seconds: six probes
  split into three acknowledgements and three losses, all three same-sequence
  fragmentable retries pass, the proven floor converges to 1,269 bytes below a
  1,280-byte limit, and no congestion reduction or timeout collapse follows.
  The unchanged fixed-runtime matrix also passes again with zero probes and a
  548-byte maximum. Because the portable shared UDP sender does not apply the
  transport's per-emission fragmentation intent to the OS socket, this is a
  controlled diagnostic result rather than Internet PMTU evidence. No product
  or support claim follows.
- Tactical `132`'s mixed socket evidence proves first uTP-to-TCP fallback,
  a repeated suppressed dial with no new uTP datagram, and exact-deadline
  recovery to confirmed uTP under one record and one permit per logical dial.
  Cancellation before a result retains no evidence; cancellation or peer-wire
  failure after transport establishment retains confirmed capability. PEX
  refresh, stale/duplicate results, saturation, overflow, eviction, and source
  removal pass deterministic coverage.
- The one authorized 2026-08-11 `product-utp` Big Buck Bunny public attempt
  verified metadata in 2.862383 seconds. Endpoint-free high waters observed
  one TCP and one uTP peer, seven confirmed and eight suppressed endpoints,
  fixed 548-byte MTU, 30 uTP connections, 137 sent datagrams with 56
  retransmissions, and bounded 16-entry connection/shared-uTP queues. There
  were no packet/queue drops or worker panics, and shutdown retained zero UDP
  tasks, uTP connections/half-opens, or queued datagrams. The temporary root
  was empty and removed. This supports outbound default readiness; it is not
  incoming reachability, a performance threshold, or full BEP 29 support.

## Why The Campaign Must Be Adaptive

A useful plan can identify stable responsibilities and evidence gates, but it
should not pretend the full implementation sequence is already known. The
hardest risks are behavioral: sequence and timestamp wrap, reordered or
duplicated packets, selective acknowledgements, timeout and fast-retransmit
interaction, receive-window pressure, MTU discovery, clock drift, delay
history, LEDBAT response under competing traffic, UDP queue loss, connection
teardown, and client interoperability.

The campaign therefore uses these rules:

1. The umbrella direction and evidence ladder may be revised at a human
   checkpoint when measurements reveal a different next risk.
2. Once a child tactical starts, its stopping condition and required evidence
   do not move silently. New findings are fixed within scope, recorded as a
   bounded deferral, or used to stop and seek direction.
3. Evidence chooses the next child tactical. A prewritten sequence is a
   hypothesis, not an obligation to continue past a failed premise.
4. Every checkpoint records what is proven, what failed, resource high-water
   marks, intentional oracle differences, and the recommended next slice.
5. Ordinary implementation details inside an accepted child tactical proceed
   autonomously. The explicit review gates below remain human decisions.

## Stage 0 Source Survey

The normative starting points are BEP 29 for the wire protocol and
[RFC 6817](https://www.rfc-editor.org/rfc/rfc6817.html) for LEDBAT. Deployed
behavior and mature failure cases still require implementation and
interoperability oracles.

Tactical `137` additionally uses
[RFC 8899](https://www.rfc-editor.org/rfc/rfc8899.html) for Datagram
Packetization Layer PMTU base, protected-probe, confirmation, revalidation,
and black-hole behavior. It records uTP's packet-sequenced fragmentable retry
as an intentional compatibility difference rather than claiming complete RFC
8899 conformance.

### Pinned Rasterbar libtorrent

The managed `reference/libtorrent` checkout is pinned to commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d` (`v2.0.13`). Its uTP
implementation is integrated into libtorrent rather than offered as a small
standalone library. The main surface is already more than 5,200 physical lines
before its tests:

| Role | Pinned paths | Initial observations |
| --- | --- | --- |
| Stream and protocol state | `include/libtorrent/aux_/utp_stream.hpp`, `src/utp_stream.cpp` | Connection state, packet queues, SACK, retransmission, receive flow control, MTU, LEDBAT, timers, wrapping sequence numbers, and async stream adaptation are intertwined. |
| Shared UDP routing | `include/libtorrent/aux_/utp_socket_manager.hpp`, `src/utp_socket_manager.cpp` | Classifies incoming packets, owns connection-ID lookup, socket creation, deferred acknowledgements, writability, MTU selection, and periodic ticks. |
| Local test | `test/test_utp.cpp` | Forces uTP by disabling incoming and outgoing TCP, transfers data, and tests wrapping comparisons. |
| Simulation | `simulation/test_utp.cpp` | Exercises PMTU discovery, an ordinary transfer, bufferbloat, a constrained path, and a small kernel send buffer. |
| Fuzzing | `fuzzers/src/utp.cpp` | Supplies hostile packet input to the protocol path. |

Tactical `118` inspected `utp_socket_impl` packet parsing, extension/SACK,
acknowledgement, loss, LEDBAT, timeout, send, receive, FIN/RESET, and MTU paths,
plus `utp_socket_manager` classification, lookup, admission, deferred-ACK,
writability, drain, tick, and removal paths. The exact function list and
resulting edge-case checklist are retained in its execution record. The
simulator source was read, but the GPL-3.0 `simulation/libsimulator` submodule
was not initialized, linked, run, or distributed.

### Standalone libutp

[BitTorrent's standalone libutp](https://github.com/bittorrent/libutp) is
managed at `2b364cbb0650bdab64a5de2abb4518f9f228ec44`. It is an MIT-licensed
C++ core with a non-thread-safe C callback API, hard-coded packet/socket
storage, host-driven timeout and deferred-ACK pumps, and legacy LEDBAT
behavior. The API is documented as unstable; the last pinned commit is from
2018, the named test directory is absent, and the repository has no Android
build target. Its native arm64 macOS build passed, but FFI, C++ packaging,
lifetime, allocation, platform, and notice costs make it a secondary reference
rather than the recommended product path.

### librqbit-utp

Apache-2.0 `librqbit-utp` `0.7.0` is managed at
`c26f57b2debbe35ed0ace1ad419de529f7a5bf95` with its crates.io checksum
recorded in [`references.md`](../references.md). Its 76 passing native tests
and Android library check are useful evidence, and it has several explicit
socket, stream, retry, timeout, and buffer limits. Direct adoption is rejected:
LEDBAT is an explicit TODO, CUBIC is the only controller, and hostile per-stream
packet ingress and socket control use unbounded channels. Adapting it would
replace enough congestion, queue, task, and shared-socket ownership to create a
long-lived fork without a credible scope advantage.

## Source-Reuse And License Decision

Rasterbar libtorrent's main uTP files carry the BSD-3-Clause license. That
license is permissive and can be distributed alongside an MIT-licensed
project, but a literal or mechanically derived copy would remain BSD-3-Clause
material. Source distributions must retain its copyright notice, conditions,
and disclaimer; binary distributions must reproduce them in accompanying
materials; names cannot be used for endorsement. RSTorrent would also record
the exact revision, copied files, modifications, and attribution in
[`docs/references.md`](../references.md) and
[`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md).

License compatibility does not make copying the most reasonable engineering
choice. Libtorrent's implementation is large, C++/Asio-integrated, and shaped
around libtorrent's socket manager, buffers, settings, metrics, and lifecycle.
Extracting or translating it mechanically would inherit provenance and much
of that architecture while still demanding a difficult Rust/runtime
adaptation.

Stage 0 therefore recommended an independently authored Rust implementation
derived from public protocol behavior, with a deterministic sans-IO core and
libtorrent as the primary completeness and interoperability oracle.
Standalone libutp and librqbit-utp remain read-only secondary references and
possible test peers. Human review accepted that choice, and Tactical `119`
implemented its first bounded core slice without adding foreign source or a
dependency.

Any decision to copy, mechanically translate, vendor, wrap, link, or add a uTP
dependency is a human review gate. It must include exact provenance, licenses,
notices, dependency and platform costs, the ownership fit, and why independent
implementation is no longer preferred.

## Recommended Owner And Dependency Shape

Stage 0 recommends the following shape and revises it only from later evidence:

- a runtime-independent protocol component owns header and extension codecs,
  wrapping arithmetic, connection transitions, acknowledgement and
  retransmission state, flow control, congestion state, and deterministic
  timer decisions;
- the session UDP service owns sockets and the one receive loop per address
  family, performs bounded classification, and routes DHT and uTP traffic into
  separate bounded consumers without making either protocol depend on Tokio;
- a bounded uTP runtime owner supplies datagrams, a clock, wakeups, and an
  ordered byte-stream adapter while owning every task, connection, queue,
  cancellation token, and joined terminal path;
- the existing torrent peer runtime owns peer identity, duplicate peer-ID
  resolution, connection generations, BitTorrent handshake, scheduling, and
  payload work regardless of whether the byte stream is TCP or uTP; and
- application and platform layers select policy and display evidence. They do
  not own datagram or peer-wire hot paths, and no companion daemon or socket
  proxy is introduced.

Socket replacement must define what happens to live uTP connections. No
connection may silently continue on a new local endpoint or UDP generation,
and no old connection or timer may mutate a replacement generation. Completed
Tactical `112` supplies the coordinated per-family socket owner; completed
Tactical `125` integrates with and tests replacement and removal at that
generation boundary.

## Candidate Child Slices

These are evidence stages, not preaccepted tacticals. A review may split,
combine, reorder, or stop them before the next child is drafted.

| Stage | Bounded outcome | Evidence that selects the next step |
| --- | --- | --- |
| 0. Reproducible decision spike | Complete the exact source/test/license dossier; pin every executable oracle; exercise a forced-uTP reference transfer; compare independent Rust, standalone-libutp FFI/vendor, and librqbit-utp dependency paths against current platform and owner constraints. | A reviewed implementation choice, explicit rejected alternatives, an executable oracle recipe, and concrete risks for the first implementation tactical. |
| 1. Deterministic transport core | Independently implement hostile bounded parsing and deterministic connection behavior for SYN, STATE, DATA, FIN, RESET, extensions, sequence/timestamp wrap, SACK, retransmission, receive windows, timers, and teardown without real sockets. | Pure transition and model/scenario tests close the state-shape risks; failures identify whether reliability or API shape needs a separate slice. |
| 2. Loss, congestion, and MTU | Add or complete LEDBAT, loss recovery, path-MTU behavior, and resource-pressure handling in a deterministic impaired-network harness. | Recorded utilization, queue delay, fairness, loss, retry, MTU, and memory/queue results under fixed scenarios and TCP cross-traffic. Acceptance thresholds are fixed in that child tactical before implementation. |
| 3. Shared UDP and loopback interoperability | Add bounded session classification and runtime ownership, expose an ordered stream to the existing peer connection, and transfer in both roles against pinned libtorrent with TCP disabled. | Exact payload hashes, forced-uTP proof, packet/connection/task cleanup, generation replacement, adverse runtime failures, and unchanged DHT service. |
| 4. Controlled WAN evidence | Run direct public-path transfers between the development machine and an authorized remote peer such as `pimom`, initially with RSTorrent dialing outward and later with remote-initiated traffic only after UDP reachability exists. | Both transfer directions where reachable, exact hashes, captures/metrics proving uTP rather than TCP, realistic RTT/loss/MTU observations, and terminal resource counts. |
| 5. Ordinary swarm and product integration | Decide TCP/uTP selection, racing, fallback, advertisement, duplicate ownership, MSE-over-uTP composition, incoming UDP reachability, settings, and status only as evidence requires. | Controlled compatibility plus representative opt-in observations establish the exact default policy and any remaining platform gaps. |
| 6. Claim graduation | Reconcile every required deterministic, runtime, interop, WAN, resource, restart, and product result. | The BEP 29 row changes only to the narrow claim supported by recorded evidence. |

The readiness queue promoted the campaign on 2026-08-10. Tactical `118` has
reached its bounded Stage 0 stop with a reproducible oracle harness and the
implementation/provenance recommendation above. It did not start source import
or transport implementation.

## Stage 1 Result And Review Choices

Tactical `119` reached its bounded stop on 2026-08-10 in commits `6c580a5`,
`b9e86f5`, `c4c2459`, `a5e2829`, and `a83d226`. It added a borrowed hostile
v1 codec; explicit sequence/timestamp wrap; exact handshake IDs; 64-packet and
1-MiB receive reordering; a 1,024-packet and 1-MiB sent ledger; cumulative and
selective ACK release; clipped future-SACK influence; one-shot loss signals;
Karn-safe RTT and bounded RTO/retransmission state; FIN close readiness; and
terminal zero ownership. It added no manifest, unsafe code, runtime, socket,
task, entropy owner, peer stream, or support claim.

The implementation exposed no reliability-state or API failure requiring a
repair slice. It did expose two state-order requirements now covered by tests:
future SACK bits cannot influence loss beyond the actual sent range, and a
receive-limit rejection must occur before the same datagram can release sent
ownership through its ACK.

The next human choice is:

1. **A — deterministic Stage 2 (recommended):** draft one bounded tactical
   that adds the impairment harness and completes receive-window,
   packetization, delayed-ACK, LEDBAT/congestion, loss response, pacing, and
   MTU behavior together. Start from BEP 29 and RFC 6817, use pinned
   libtorrent as the interoperability oracle, and record deliberate choices
   where their base-delay histories or congestion floors differ.
2. **B — harness-only Stage 2a:** first land only the deterministic datagram
   network, scenario vocabulary, and reference traces, then return for a
   second review before selecting the controller. This lowers one tactical's
   implementation breadth but adds a checkpoint before any data-plane result.
3. **C — runtime-first loopback:** integrate the current reliability core with
   shared UDP under a temporary conservative send policy before LEDBAT and MTU
   are complete. This can reveal owner/API problems earlier, but it knowingly
   creates a transport path that cannot yet satisfy the congestion gate and is
   likely to require runtime rework; it is not recommended.

None of these choices authorizes WAN, `pimom`, public-swarm, physical-device,
port-mapping, pinhole, product-policy, or support-claim work.

Human review selected choice A on 2026-08-10. Tactical
[`121`](../tactical/121-deterministic-utp-loss-congestion-and-mtu.md) fixes the
Stage 2 controller, delayed-ACK, flow-control, packetization, recovery, MTU,
impairment, resource, and acceptance contracts before implementation. The
separate uncommitted fast-resume plan already owns number `120`; Stage 2 does
not supersede or absorb that work.

## Stage 2 Result And Review Choices

Tactical `121` reached its bounded stop on 2026-08-10 in commits `ccb93a5`
through `e8fba52`. Its independently authored state and fixed scenarios meet
every pre-agreed utilization, fairness, delay, loss, receive-pressure, MTU,
attempt, work, and ownership threshold. The TCP-like foreground receives
77.0876% during overlap with a 124.144-ms queue-delay p95; uTP recovers to 70%
utilization in 3.27 measured RTTs. A 1,280-byte DF black hole converges in six
probe outcomes to a 13-byte interval with no congestion reduction attributable
to the probe. The tactical records all scenario and resource high-water
values.

The implementation exposed one integration defect and repaired it within
scope: stale SACKs for an already-in-flight fragmentable MTU retry could create
duplicate retry traffic and a false subsequent congestion loss. Retry identity
now remains isolated and coalesced until acknowledgement or ordinary timeout.
No threshold, RFC controller choice, or resource bound was weakened.

The next human choice is:

1. **A — Stage 3 shared UDP and loopback interoperability (recommended):**
   draft one bounded tactical for uTP/DHT datagram classification, connection
   lookup/admission, runtime task/cancellation ownership, ordered-stream
   adaptation, socket-generation replacement, and forced-uTP pinned-libtorrent
   transfers in both roles. Keep product policy, WAN, mapping/pinhole, and MSE-
   over-uTP out of scope.
2. **B — split Stage 3 at the runtime seam:** first land only bounded shared-
   UDP classification, connection/runtime ownership, ordered-stream adaptation,
   and scripted failure/cleanup evidence, then return for another review before
   any independent-implementation exchange. This reduces one tactical's breadth
   but delays the first interoperability verdict.
3. **C — pause the uTP campaign:** retain the completed deterministic core and
   return the authoritative **Now** to the readiness queue without adding a
   runtime path.

No choice authorizes `pimom`, another external network, a public swarm,
physical-device work, UDP reachability changes, or a support claim.

Human review selected choice A on 2026-08-10. Tactical
[`125`](../tactical/125-shared-udp-utp-runtime-and-loopback-interop.md) fixed
the shared-UDP, runtime, peer-stream, controlled-peer, resource, and stopping
contracts before implementation.

## Stage 3 Result And Review Choices

Tactical `125` reached its bounded stop on 2026-08-10 in commits `2d33516`,
`5dd6d3c`, `c9ab011`, `7de2974`, `fed430c`, `2384d7c`, and `dc5ab32`. One
session UDP receiver now isolates DHT and uTP into independent bounded queues;
one supervisor owns generation-fenced endpoint/ID lookup, SYN admission,
timers, workers, cancellation, ordered streams, and joined cleanup; and the
concrete peer byte-stream boundary carries TCP or uTP without pushing UDP into
peer-wire state. Incoming controlled uTP reuses the existing pending-
handshake, peer-budget, identity, upload, content-read, observation, and
cleanup owners. Ordinary product dialing and listening remain TCP.

The fixed loopback oracle transferred the same 2,097,883-byte, 65,536-byte-
piece fixture in both roles against libtorrent `2.0.13.0`, with SHA-1
`cdce24126a8e65854d876c0b83ad3ba19748f6dc`. RSTorrent leecher completed 129
requests in 0.557320 seconds; RSTorrent seed completed in 0.805350 seconds.
Each side observed exactly one loopback uTP peer and zero TCP peers. All
RSTorrent malformed, stale, unknown, drop, and panic counters and all
libtorrent loss, timeout, and resend counters were zero. Queue and byte high-
waters remained below their declared bounds, and both cases ended with zero
session-UDP tasks, uTP connections/half-opens, peer owners, registrations, and
queued datagrams. Both service snapshots retained bounded RTT/RTO, raw base-
delay, queue-delay, congestion-window, advertised receive-window, and selected-
MTU ranges; the selected MTU remained 548 bytes. The tactical records the
exact per-role packet, transport, and resource values. Focused, full workspace,
and controlled interoperability gates pass.

No structural interoperability defect appeared, so a local repair slice is
not indicated. The next human choice is:

1. **A — Stage 4 controlled outbound WAN evidence (recommended):** draft one
   bounded tactical for RSTorrent to dial a forced-uTP libtorrent seed on the
   already identified authorized remote host over its ordinary routed path.
   Retain the exact payload/hash and terminal-owner gates, add bounded path
   RTT/loss/MTU/controller observations, and defer reverse incoming traffic
   until truthful UDP reachability exists.
2. **B — pause before external evidence:** keep the completed diagnostic
   runtime and return the authoritative **Now** to another readiness item.
   Product uTP remains disabled and the campaign resumes later at Stage 4.
3. **C — plan product policy before WAN evidence:** define TCP/uTP selection,
   fallback, advertisement, and presentation as a design-only tactical without
   enabling them. This can clarify eventual controls, but it moves policy
   ahead of realistic path evidence and is not recommended.

Choice A requires explicit authorization for the remote host and its external
network. None of these choices authorizes reverse-direction incoming UDP,
mapping/pinhole work, a public swarm, physical devices, MSE-over-uTP, IPv6 uTP,
product enablement, a dependency, or a support-claim change.

Human review selected choice A on 2026-08-10. Tactical
[`126`](../tactical/126-controlled-outbound-utp-wan-evidence.md) fixes the
direct-route preflight, diagnostic-only online role, remote ownership,
transport evidence, resource, cleanup, and evidence-limited stopping contracts
before external execution.

## Stage 4 Result And Review Choices

Tactical `126` first reached an evidence-limited stop because it incorrectly
required a global address directly on the remote interface. Human review
corrected that premise and authorized Tactical `127` to establish the exact
oracle on the NATed `pimom`, try its UDP UPnP capability, and retain a local-
mapping fallback only if the remote gateway was incapable.

Tactical `127` completed on 2026-08-10. The remote gateway reported a global
external IPv4 address, installed exactly one query-confirmed finite UDP lease,
and exposed the forced-uTP libtorrent `2.0.13.0` seed. RSTorrent's route to the
redacted endpoint used the ordinary Internet interface, not Tailscale or SSH
forwarding. The exact 2,097,883-byte, 33-piece fixture completed in 82.239
seconds with the expected SHA-1, one uTP peer, zero TCP peers, and no discovery
mechanism.

Libtorrent reported 1,807 outbound and 909 inbound uTP packets, zero loss,
timeout, fast-retransmit, or resend counters, and the exact payload-byte count.
RSTorrent classified all 1,807 received UDP datagrams as uTP with zero drops,
one live-connection high-water, a 13-datagram queue high-water, zero
retransmissions/loss reductions/timeout collapses, 155.655--168.723 ms
smoothed RTT, 500--1,000 ms RTO, 0--2.211 ms queue delay, a fixed 548-byte MTU,
and terminal zero connection/task/queue ownership. Its 1,056-byte send
congestion window applied only to request/control traffic in this leecher
direction and does not establish bulk-send performance.

The exact UDP lease was deleted and query-confirmed absent. Normal cleanup
removed both per-run directories and the remote helper; an independent audit
found zero owned mappings, processes, or run artifacts. The reusable isolated
oracle remains intentionally installed. The local-mapping fallback was not
needed or attempted. The first dual-mapping cleanup defect, its exact repair,
and the earlier 30-second timeout are retained in Tactical `127` rather than
hidden by the passing result.

The next human choice is:

1. **A — complementary mapped-WAN sender evidence (recommended):** draft one
   bounded tactical that intentionally creates a temporary local UDP mapping,
   runs RSTorrent as the seed and bulk sender, and has the pinned `pimom`
   oracle dial the public endpoint. This closes the unmeasured WAN direction
   and directly observes RSTorrent's congestion controller, throughput,
   cleanup, and local gateway capability before product policy.
2. **B — pause uTP and resume the readiness queue:** retain the completed
   deterministic, loopback, and one-direction WAN evidence, keep product uTP
   disabled, and make focused-driver HTTP(S) tracker dispatch the next
   executable engine slice.
3. **C — plan Stage 5 product integration now:** define TCP/uTP selection,
   racing, fallback, advertisement, incoming reachability, and MSE composition
   from the current evidence. This is not recommended while RSTorrent's WAN
   bulk-send direction is unmeasured.

No choice is implicit. Choice A requires explicit authority to use the local
gateway mapping even though Tactical `127`'s capability-gated fallback was not
triggered. None of the choices authorizes a permanent network change, another
remote host, IPv6 uTP, a public swarm, a dependency, or a support-claim change.

Human review selected choice A and additionally authorized the bounded
bidirectional cohort, real-socket impairment/lifecycle work, and evidence-led
diagnostic MTU stage. Tactical
[`130`](../tactical/130-utp-transport-solidification.md) fixes those route,
lease, resource, cleanup, hostile-runtime, and stopping contracts. It may
repair evidence-backed defects autonomously and commits by stage, then stops
at the pre-product review.

## Bounded Stage 5 Composition Result And Review Choices

Human review selected bounded composition recommendation A after Tactical
`130`. Completed Tactical `131` now supplies one explicit, default-off
application policy without claiming the rest of broad Stage 5:

- one fixed-548 uTP service shares the session UDP owner and joins before UDP
  shutdown;
- incoming streams reuse the ordinary handshake, peer budget, torrent route,
  upload, identity, view, and cancellation owners;
- IPv4 plaintext outgoing dials prefer uTP, while IPv6 and encryption
  `prefer`/`required` stay on TCP;
- uTP transport failure joins to zero before one sequential TCP fallback under
  the same logical dial, peer generation, and permit; and
- shipped/default clients, mapping, advertisement, settings, presentation,
  and the BEP 29 claim are unchanged.

The controlled application gate passes against pinned libtorrent `2.0.13.0`
in both uTP roles and against a TCP-only seed. Every case independently
verifies the exact 2,097,883-byte fixture and one final application peer. The
uTP cases retain fixed 548-byte MTU observations and zero worker panics. The
fallback case sends three uTP datagrams, joins that worker to zero, reports one
final TCP peer, and completes after 5.314 seconds. The unchanged default path
passes ordinary initiated TCP, accepted Fast TCP, RSTorrent Fast TCP, and
forced MSE with no uTP service. Formatting, workspace clippy, and the complete
workspace test baseline pass.

The fallback result exposes the next product risk: a first TCP-only endpoint
pays the full five-second uTP timeout, and the result is not remembered across
later logical attempts. The review considered these choices:

1. **A — bounded default-readiness evidence (recommended):** keep shipped
   defaults TCP-only and draft a source-first tactical for endpoint-scoped uTP
   success/failure memory, retry/backoff semantics, a mixed TCP/uTP controlled
   cohort, and an explicitly authorized opt-in ordinary-swarm observation.
   Return to review before settings, default enablement, mapping, incoming-
   endpoint advertisement, or claim graduation.
2. **B — enable `PreferUtp` by default now:** this is not recommended because
   the controlled TCP-only endpoint pays five seconds, repeated failures are
   uncached, and no ordinary-swarm product cohort exists.
3. **C — close the active campaign at default-off composition:** retain the
   programmatic capability and return the readiness queue to Tactical `129`.

The maintainer selected choice A on 2026-08-11. Tactical
[`132`](../tactical/132-utp-default-readiness-evidence.md) executed that
default-readiness work and its single bounded public observation. It is now
complete; default enablement remains a separate human product-policy decision.

## Product-Default Result

Tactical `132` is complete. The registry, socket, driver, application, and
public-probe evidence now agree on one bounded policy: try unknown eligible
IPv4/plaintext endpoints once, remember transport success or failure within
the existing record, use direct TCP during suppression, and permit recovery by
deadline or explicit PEX advertisement. The retained pinned-libtorrent product
suite again hash-verifies incoming uTP, outgoing uTP, and TCP fallback; the
default TCP/Fast/MSE suite still starts no uTP owner. The single public profile
found both transports and cleaned every owner without changing a default.

The review choices were:

1. **A — bounded default enablement (recommended):** draft one tactical to
   change the application construction default to the existing `PreferUtp`,
   validate affected desktop/Android startup and shutdown plus controlled
   fallback, and graduate only the implemented BEP 29 subset to **Partial**.
   Retain fixed 548-byte IPv4/plaintext selection and add no user setting,
   mapping, tracker/DHT incoming-endpoint advertisement, MSE-over-uTP, IPv6
   uTP, race, or dynamic MTU.
2. **B — close at default-off composition:** retain the proven programmatic
   path, keep the BEP 29 claim **Unsupported**, and promote queued Tactical
   `129`.
3. **C — require incoming reachability first:** plan UDP mapping and truthful
   advertisement before any default change. This is not recommended because
   outbound selection and fallback are now independently ready, while NAT
   reachability and announce-port semantics require a broader product policy.

The maintainer selected choice A on 2026-08-11. Tactical
[`133`](../tactical/133-utp-product-default-enablement.md) completed that
bounded change. Ordinary application construction now inherits `PreferUtp`;
an explicit `TcpOnly` override retains the isolated TCP/Fast/MSE baseline. The
exact pinned-libtorrent application fixture passes default incoming uTP in
1.374587 seconds, default outgoing uTP in 0.290656 seconds, and joined TCP
fallback in 5.339730 seconds, with exact content, actual final transport,
fixed 548-byte uTP datagrams, zero worker panics, and clean shutdown.
Desktop, Android, both supported Android native builds, focused lifecycle and
recovery, formatting, warning-denying clippy, and complete workspace gates
pass.

The implemented BEP 29 subset is now **Partial**. UDP mapping, tracker/DHT
incoming-endpoint advertisement, public incoming reachability, persisted
presentation, IPv6 uTP, MSE-over-uTP, racing, portable per-datagram
fragmentation protection, dynamic product MTU, and the incomplete repeatable
WAN cohort remain explicit limits. No additional public, WAN, emulator, or
physical-device run was needed or performed.

## Validation Contract

Validation grows in layers; later evidence never substitutes for an earlier
layer:

1. **Pure protocol transitions:** independently authored vectors and
   state-machine scenarios cover malformed/truncated packets, unknown and
   repeated extensions, wrap boundaries, duplicate/reordered packets,
   selective acknowledgements, zero windows, timer backoff, FIN/RESET races,
   stale generations, and exact bounds.
2. **Scripted impairments:** a deterministic datagram network controls delay,
   jitter, drop, duplication, reordering, bandwidth, queue capacity, MTU and
   black holes, clock behavior, and competing TCP-like traffic.
3. **Controlled interoperability:** pinned libtorrent runs in both seed and
   leecher roles with incoming and outgoing TCP disabled. Exact content hashes
   and observed transport prove the result.
4. **Controlled WAN interoperability:** an authorized remote host supplies a
   real routed path, not merely LAN conditions. Exact endpoint, direction,
   version, network conditions, and artifact retention are recorded.
5. **Representative live observation:** opt-in public-swarm runs may find
   compatibility and performance surprises. They cannot by themselves prove
   loss recovery, congestion correctness, reachability, or reliability.
6. **Product/platform evidence:** desktop and Android/ChromeOS checks are added
   only when a slice changes their settings, lifecycle, network permissions,
   packaging, or runtime behavior.

Every transport-bearing result records at least payload hash, TCP-disabled or
otherwise unambiguous transport proof, direction and endpoints, packet and
byte counts, loss and retransmission counts, RTT/RTO observations, delay/base
delay and congestion-window behavior, advertised receive window, selected
MTU, queue/reorder/half-open high-water marks, and terminal task/connection
counts. Metrics must remain bounded and useful without packet-level logging in
ordinary operation.

## `pimom` WAN Evidence

The host reachable through `ssh pimom` is the authorized Stage 4 control peer.
SSH remains control-plane only. Tactical `127` established a reusable isolated
libtorrent `2.0.13.0` oracle without system packages and proved one direct
public-path transfer through a query-confirmed remote UDP UPnP mapping. The
Tailscale/shared-range SSH endpoint did not carry uTP traffic.

The exact payload hash, transport observations, resource high-waters, and
cleanup result are recorded above and in Tactical `127`; identifying endpoint
and gateway data are deliberately redacted. Every per-run payload, metainfo,
directory, log, mapping, listener, and process was removed. The only retained
remote state is the documented user-owned oracle environment.

Closed Tactical `130` also proves the complementary first sample:
RSTorrent exposed its diagnostic seed through one exact finite local UDP UPnP
lease and pinned libtorrent on `pimom` downloaded and hash-verified the same
fixture over an ordinary Internet route. The 92.140-second run observed one
uTP/zero TCP peers, exact RSTorrent upload accounting, fixed 548-byte runtime
MTU, bounded controller/resource high-waters, joined lease deletion, and an
independent absent audit. One gateway reset of an idempotent external-address
query produced a bounded query-only retry regression test. This is not yet the
bidirectional cohort, impairment/lifecycle matrix, MTU result, product policy,
or support claim.

The bounded Tactical `130` WAN cohort closes evidence-limited. Two
remote-receive attempts exposed diagnostic-only transient-choke and stale-
duplicate handling gaps; both are repaired with exact counters and integrity
bounds. Two local-send cases reached the unchanged 180-second limit, while two
single-direction diagnostic retries passed in 58.777 and 64.528 active
seconds. Together with the initial sample, three captured local-send successes
retain exact hash, one uTP/zero TCP peers, 153--177 ms RTT observations, fixed
548-byte MTU, finite exact lease cleanup, and zero residue. No compliant fresh
three-sample remote-receive summary was retained before the external attempt
budget expired. Do not infer a cohort baseline or silently run more WAN cases.
The deterministic real-socket impairment/lifecycle and diagnostic-MTU gates
subsequently passed; the incomplete cohort remains the explicit external
evidence limit at pre-product review.

## Human Review Gates

Pause and ask for direction before any further uTP work that would require:

- adopting, copying, mechanically translating, vendoring, wrapping, linking,
  or adding any uTP implementation or new runtime dependency;
- materially redesigning the shared UDP owner beyond the accepted tactical;
- selecting a congestion controller or deliberately departing from BEP 29,
  RFC 6817, or established libtorrent interoperability behavior;
- expanding the slice into UDP port mapping, IPv6 pinholes, NAT traversal,
  hole punching, LSD, or multi-interface policy;
- adding persisted/user-visible transport policy, TCP/uTP racing, or a
  materially different fallback policy;
- expanding MSE work beyond composing the completed stream boundary;
- running on `pimom`, public swarms, physical devices, or other externally
  consequential environments not already authorized by the active tactical;
  or
- changing the protocol claim or announcing uTP as supported.

At each review, present the completed evidence, important failures, deliberate
deferrals, and two or three concrete next choices. Do not ask the user to
decide routine internal representation or test details already bounded by the
accepted tactical.

## Non-Goals Of This Umbrella

- No uTP code, dependency, vendored source, fixture, port mapping, or product
  setting is added by this topic.
- No current MSE, IPv6, DHT, listener, or reachability tactical grows to include
  uTP implicitly.
- No BEP 55 hole punching, LSD, NAT-PMP, PCP, or general NAT traversal is
  authorized.
- No generic transport framework is created before two concrete transports
  expose an actual shared boundary.
- No public-swarm run is treated as congestion-control proof.
- No support claim follows from code paths or a same-LAN happy path.

## Restart Checkpoint

Campaign state: **Stage 0 Tactical `118`, deterministic Stage 1 Tactical
`119`, deterministic Stage 2 Tactical `121`, shared-UDP/runtime Stage 3
Tactical `125`, and remote-mapped Stage 4 Tactical `127` complete; outbound-
only WAN Tactical `126` remains closed evidence-limited at its superseded
direct-interface preflight; post-Stage 4 solidification Tactical `130` is
closed with the WAN cohort evidence-limited and every controlled gate
passing; bounded default-off product composition Tactical `131` and
default-readiness Tactical `132` are complete; and product-default Tactical
`133` is complete with the exact fixed-548 IPv4/plaintext subset graduated to
Partial; product Tactical `138` completed, and Tactical `137` is reactivated;
its Stage 2 shared-egress and safe platform-option boundary passes, including
the approved macOS adapter; deterministic Stage 3 revalidation/downward
recovery and Stage 4 protected-send/product runtime integration pass, and
Stage 5 controlled interop/rate evidence is current.**

Authoritative priority remains
[`capability-readiness.md`](capability-readiness.md). Execute Tactical `137`'s
controlled application/libtorrent and alternating fixed/dynamic efficiency
stage. Reachability, tracker/DHT incoming-endpoint advertisement,
presentation, permanent network change, different host, and a broader uTP
support claim remain separate decisions; no public-swarm attempt is authorized
by this tactical.
