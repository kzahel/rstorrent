# uTP Transport Campaign

Topic: `utp-transport-campaign`

Status: Stage 0 Tactical
[`118`](../tactical/118-utp-implementation-decision-spike.md) is at its first
human review. Its source, platform, ownership, and forced-uTP evidence supports
the independent Rust recommendation; uTP remains **Unsupported** and no
implementing tactical or dependency is accepted.

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

- RSTorrent peer traffic is TCP only. The transport-neutral peer lifecycle in
  [`peer-lifecycle.md`](peer-lifecycle.md) already has incoming/outgoing
  direction and TCP/uTP vocabulary, so uTP should become another connection
  generation rather than a second peer model.
- The session has one bounded UDP receive owner, but
  `crates/rstorrent-engine/src/session_udp.rs` currently sizes ingress for the
  DHT's 1,024-byte maximum and routes every accepted datagram toward DHT. uTP
  needs explicit classification, appropriately bounded packet storage, its own
  queue pressure, and observable drop behavior.
- Completed Tactical
  [`112`](../tactical/112-dual-stack-transport-and-ipv6-dht.md) changed the
  session to one coordinated TCP/UDP socket pair per enabled address family.
  Tactical `118` inspects that landed owner rather than designing around the
  former IPv4-only shape.
- Current UPnP behavior maps TCP only. Outgoing uTP can therefore reach a
  controlled off-LAN peer before incoming public uTP is advertisable. UDP
  mapping or an IPv6 pinhole is a separate reachability decision, not an
  incidental addition to the first transport slice.
- Completed Tacticals
  [`111`](../tactical/111-mse-peer-stream-encryption.md) and
  [`115`](../tactical/115-mse-policy-advertisement-and-peer-detail.md)
  deliberately exclude uTP and MSE over uTP. Their sans-IO MSE state machine
  and policy helpers remain reusable once uTP exposes the same ordered peer-
  stream boundary; Stage 5 must compose and verify that path explicitly.
- The retained
  [`utp_reference_oracle.py`](../../tests/interop/utp_reference_oracle.py)
  proves only that two pinned libtorrent sessions can complete one bounded,
  forced-uTP loopback transfer under the future acceptance controls. No
  RSTorrent uTP state machine, runtime stream, interoperability result, WAN
  result, product policy, or public-swarm support evidence exists.

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

Stage 0 therefore recommends an independently authored Rust implementation
derived from public protocol behavior, with a deterministic sans-IO core and
libtorrent as the primary completeness and interoperability oracle.
Standalone libutp and librqbit-utp remain read-only secondary references and
possible test peers. This recommendation is at the first human gate, not yet
an accepted implementation choice.

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
Tactical `112` supplies the coordinated per-family socket owner; a later uTP
runtime tactical must integrate with and test that landed generation boundary.

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

The host reachable through `ssh pimom` is a promising controlled WAN peer, but
this topic does not authorize connecting to it or changing it. A future live
evidence tactical must follow
[`performance-and-live-evidence.md`](performance-and-live-evidence.md) and
obtain the normal opt-in for external activity.

SSH should orchestrate the reference process, gather bounded metrics, and
retrieve temporary artifacts. The uTP packets under test must traverse the
ordinary direct public route rather than an SSH tunnel or overlay. The first
WAN case should have RSTorrent dial a forced-uTP libtorrent listener on the
remote host, which avoids claiming local incoming UDP reachability. A later
reverse-direction case requires a truthful reachable UDP endpoint and may
therefore belong to a separate mapping/pinhole tactical. Use a controlled
payload, verify its exact hash, capture only what the evidence needs, and
remove remote and local temporary data after the result is recorded.

## Human Review Gates

Pause after the active bounded slice and ask for direction before:

- adopting, copying, mechanically translating, vendoring, wrapping, linking,
  or adding any uTP implementation or new runtime dependency;
- materially redesigning the shared UDP owner beyond the accepted tactical;
- selecting a congestion controller or deliberately departing from BEP 29,
  RFC 6817, or established libtorrent interoperability behavior;
- expanding the slice into UDP port mapping, IPv6 pinholes, NAT traversal,
  hole punching, LSD, or multi-interface policy;
- choosing the user-visible default for TCP/uTP racing, preference, fallback,
  or disabling;
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

Campaign state: **Stage 0 Tactical `118` is at first review; no uTP
implementation accepted**.

Authoritative priority remains
[`capability-readiness.md`](capability-readiness.md). The next action requires
human direction: approve the recommended independent Rust Stage 1, request a
standalone-libutp FFI/platform feasibility slice, or request a librqbit
hardening/LEDBAT-fork feasibility slice. No option starts until selected.
