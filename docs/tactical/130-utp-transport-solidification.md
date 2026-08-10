# Tactical 130: uTP Transport Solidification

Status: **Active** on 2026-08-10. At Tactical `127`'s post-Stage 4 review, the
maintainer authorized the complete bounded transport-solidification workstream:
temporary exact UDP UPnP leases on the already authorized local and `pimom`
networks, a small bidirectional WAN cohort, controlled real-socket impairment
and hostile lifecycle gates, and evidence-led diagnostic MTU work. Commit each
bounded stage and stop at the final pre-product review.

Topics: `utp-transport-campaign`, `incoming-reachability-and-seeding`,
`performance-and-live-evidence`, `peer-lifecycle`, `protocol-support`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tactical
[`127`](127-mapped-utp-wan-interoperability.md) supplies the exact fixture,
remote pinned oracle, remote-mapped RSTorrent-leecher direction, direct-route
and redaction checks, temporary-lease cleanup contract, and bounded WAN
harness. Completed Tactical
[`125`](125-shared-udp-utp-runtime-and-loopback-interop.md) supplies the shared
UDP/runtime, ordered stream, incoming upload owner, and both loopback roles.
Completed Tacticals [`119`](119-deterministic-utp-transport-core.md) and
[`121`](121-deterministic-utp-loss-congestion-and-mtu.md) supply the hostile
wire state and deterministic loss/congestion/MTU behavior.

Ready but unimplemented Tactical
[`129`](129-bounded-storage-intake-watermark.md) remains intact and queued. The
maintainer's explicit uTP priority moves this tactical to the single
authoritative **Now** without invalidating Tactical `129`'s evidence or plan.

## Decision And Desired Outcome

Turn the current uTP proof into a solid transport-engine baseline before any
product selection or support claim. The tactical has one coherent outcome:
RSTorrent must send and receive exact payload over both controlled WAN
directions, repeat those observations enough to distinguish a result from a
single sample, compose the runtime with deterministic socket-level impairment
and hostile lifecycle pressure, and either prove a safe diagnostic MTU search
path or retain the fixed 548-byte runtime floor with an explicit evidence
limit.

The stages are sequential because each selects the next work:

1. **Complementary mapped WAN:** create one exact temporary local UDP mapping,
   run RSTorrent as seed and bulk sender, and have the pinned `pimom`
   libtorrent leecher dial the redacted public endpoint.
2. **Bidirectional cohort:** run three clean samples per direction under the
   same fixture and transport gates. The first Tactical `127` success remains
   historical evidence but does not substitute for the new cohort.
3. **Real-socket impairment and lifecycle:** place one bounded deterministic
   UDP relay between RSTorrent and pinned libtorrent on loopback. Complete the
   exact fixture under fixed delay/jitter, loss, duplication/reordering, burst
   loss, and size black-hole profiles while separately exercising malformed,
   spoofed/unknown, half-open, queue-pressure, cancellation, service/socket-
   generation replacement, and repeated start/stop behavior.
4. **Diagnostic MTU integration:** first measure fixed-548 bulk sending. Then
   connect the already completed `PathMtuState` to real runtime emissions only
   behind an explicit diagnostic configuration and prove its feedback through
   the size-black-hole relay. A probe must be distinguishable from ordinary
   traffic, and a failed probe must be retried at the proven floor without a
   congestion reduction. If the current portable socket surface cannot honor
   fragmentation protection on the real WAN path without a new dependency,
   unsafe platform code, or product policy, retain 548 for ordinary runtime
   and record that limit rather than claiming Internet path-MTU discovery.
5. **Reconciliation:** run the complete baseline, record every external and
   controlled result, retain BEP 29 as **Unsupported**, and return for human
   review before product integration.

Evidence-backed defects inside the accepted protocol/runtime ownership and
RFC 6817 controller are repaired autonomously. A failure may revise later
measurements but does not silently weaken an integrity, route, cleanup,
resource, or transport gate.

## Stopping Condition

This tactical is complete only when all applicable gates pass:

1. one local-mapped direct-public-path transfer completes with RSTorrent as
   seed/bulk sender and pinned libtorrent `2.0.13.0` as leecher; both report one
   uTP peer, zero TCP peers, the exact fixture and SHA-1, and no discovery;
2. three fresh samples in each WAN direction record elapsed/active transfer
   time, payload and packet bytes/counts, RTT/RTO, raw and queue delay,
   congestion and receive windows, retransmissions/loss/timeouts, MTU, queue
   and byte high-waters, and terminal ownership. Medians and ranges are
   observations rather than release thresholds;
3. every WAN sample proves the selected public route is ordinary Internet,
   not Tailscale or SSH forwarding, and exactly one finite UDP lease is
   query-confirmed, explicitly deleted, and independently absent afterward;
4. the fixed real-socket impairment matrix transfers the exact fixture against
   pinned libtorrent or records a reproducible implementation defect before
   repair. Every profile has deterministic packet selection and fixed bounds;
5. hostile and lifecycle gates prove shallow rejection, bounded half-opens and
   queues, generation fencing, cancellation/join, repeated startup/shutdown,
   and zero terminal tasks/connections/half-opens/queued datagrams without a
   worker panic;
6. diagnostic MTU search either converges through real runtime emissions under
   the controlled size black hole, with probe loss isolated from congestion,
   or closes evidence-limited while ordinary runtime remains fixed at 548;
7. the reusable remote oracle may remain, but no mapping, listener, process,
   payload, metainfo, report, run directory, packet capture, or raw endpoint
   artifact remains locally or remotely;
8. focused, loopback, interop, formatting, clippy, and full workspace gates
   pass; and
9. all owning topics and the readiness/campaign checkpoints are reconciled at
   the pre-product review without enabling, advertising, or claiming uTP.

If the local gateway has no eligible UDP UPnP capability, the first gate closes
evidence-limited after read-only capability facts and exact cleanup. Continue
the non-WAN impairment/lifecycle/diagnostic-MTU stages, but do not substitute a
Tailscale path, permanent forwarding rule, different router protocol, or new
host.

## Scope Boundaries And Human Stops

This tactical authorizes:

- exact temporary local and remote UDP UPnP mappings on the two already
  authorized networks, with one mapping at a time and mandatory cleanup;
- SSH to `pimom` as a control plane and the retained isolated oracle only;
- independently authored diagnostic/runtime/test code and evidence-backed
  fixes within the existing sans-IO, shared-UDP, peer-stream, incoming-upload,
  and RFC 6817 ownership;
- repeated bounded external runs and controlled local socket tests; and
- commits at each coherent stage.

Stop before:

- a new dependency, foreign source, unsafe platform socket implementation, or
  change from the accepted RFC 6817 controller;
- a permanent router/firewall/VPN change, another external host, public swarm,
  or physical device;
- ordinary product uTP dialing/listening, TCP/uTP racing/fallback, capability
  advertisement, persisted settings, UI, MSE-over-uTP, or IPv6 uTP;
- relaxing an integrity, cleanup, hostile-input, or resource bound; or
- changing the BEP 29 support claim.

These are early human stops. Routine harness structure, diagnostic fields,
test profiles, exact constant selection within the bounds below, and repairs
that preserve accepted architecture proceed autonomously.

## Invariants And Resource Bounds

- The payload remains the independently generated 2,097,883-byte single file
  with 65,536-byte pieces, 33 pieces, and SHA-1
  `cdce24126a8e65854d876c0b83ad3ba19748f6dc`.
- TCP, MSE, DHT, LSD, trackers, web seeds, automatic libtorrent UPnP, and
  NAT-PMP remain disabled in every uTP evidence role.
- Each transfer admits exactly one peer. The service retains its 64-connection
  global maximum, 64-datagram per-connection queue, 256-datagram shared uTP
  queue, 1 MiB receive credit, 1 MiB unsent bytes, 1,024 sent packets, and
  1 MiB sent-ledger bound.
- At most one temporary UDP mapping exists per sample. Its requested lease is
  at most 3,600 seconds. Internal listener, mapping protocol, external port,
  external address class, query result, deletion, and post-delete absence are
  exact.
- The cohort has six fresh successful samples maximum, plus at most two
  diagnostic retries per direction after a named defect. One sample has a
  180-second role bound and a 210-second whole-case bound. Total staged bytes
  per host stay below 32 MiB and no retained capture is permitted.
- The impairment relay has two endpoints, a 256-datagram/1-MiB event queue,
  16-MiB byte budget per profile, at most 10,000 packet decisions, and a
  180-second bound. Policies use fixed ordinals/intervals, never unrecorded
  randomness.
- Hostile runtime tests send at most 1,024 datagrams per case, create at most
  the existing 64 live connections plus one rejection attempt, and finish in
  30 seconds. Counters saturate and no per-packet production log is added.
- Diagnostic MTU bounds remain 548--1,472 IPv4 UDP payload bytes. One
  connection owns at most one active probe and one fragmentable retry. Ordinary
  product/runtime construction remains fixed at 548 unless later product
  review explicitly accepts different behavior.
- No committed result contains IP addresses, router identifiers, peer IDs,
  machine home paths, SSH material, payload bytes, or unbounded output.

## External Ownership And Cleanup

The local orchestrator owns every child and temporary directory. A local seed
emits its concrete LAN address and UDP port before mapping work so cleanup can
target the exact owner even if mapping or readiness fails. It emits the
external port only after query verification. The mapping owner deletes and
queries the exact UDP lease before UDP socket teardown. The orchestrator then
runs an independent bounded audit; if the primary owner is gone, the audit may
delete only a mapping whose protocol, external port, internal address/port,
description, and finite lease match the recorded run.

The remote leecher is attached to one SSH-controlled run directory and emits
bounded JSON. It checks its ordinary route to the redacted public endpoint,
verifies the exact fixture, removes its torrent/session through normal paths,
and exits with zero peers. Remote cleanup targets only the validated run
directory and PID. The existing remote-mapped direction retains Tactical
`127`'s exact named-lease audit.

Cleanup runs for success, failure, timeout, malformed output, SSH loss, and
interruption. An uncertain local or remote lease is a failed gate and blocks a
second mapping until absence is proved.

## Real-Socket Impairment And Lifecycle Matrix

The relay profiles are fixed before observing results:

| Profile | Controlled behavior | Required result |
| --- | --- | --- |
| clean | 2 ms each way | exact transfer, zero relay drops |
| delay-jitter | alternating 5/25 ms | exact transfer, bounded reorder/queues |
| sparse-loss | drop every 100th eligible DATA datagram | exact transfer, nonzero bounded recovery |
| duplicate-reorder | duplicate every 79th and delay every 53rd eligible datagram behind its successor | exact transfer, no duplicate delivery |
| burst-loss | drop three consecutive eligible DATA datagrams once after establishment | exact transfer, bounded fast/timeout recovery |
| MTU black hole | drop fragmentation-protected diagnostic datagrams above 1,280 bytes | exact transfer at the proven floor; probe failure does not reduce congestion |

The relay parses only enough hostile-bounded uTP shape to distinguish packet
type and size; malformed input follows a fixed pass/drop policy without
allocation from declared lengths. Unit tests prove direction, ordinal, queue,
deadline, and cleanup behavior before it carries an interoperability case.

Separate engine socket tests cover malformed packets, unknown IDs, spoofed
RESET/STATE endpoints, duplicate SYNs, connection/half-open saturation,
per-connection and shared queue saturation, consumer drop during retransmit,
service cancellation, UDP generation replacement, and repeated start/stop.
Existing coverage may satisfy a row only when its assertions include the
required counter and terminal owner state; this tactical does not duplicate a
test merely to rename it.

## Source-First Record

Re-read managed BEP 29 at BitTorrent BEP commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`, especially byte/packet windows,
timestamp-difference feedback, packet sizing, delayed ACKs, loss, timeout,
congestion, and RESET/connection-ID behavior. Re-read RFC 6817 sections 1--5,
especially sender/receiver delay sampling, application-limited window growth,
loss/timeout response, ACK frequency, competing traffic, and experimental
parameter/measurement guidance.

Re-inspected Rasterbar libtorrent commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `test/test_utp.cpp::test_transfer` and `TORRENT_TEST(utp)` for forced-uTP,
  TCP/MSE/discovery-disabled exact transfer and joined shutdown;
- `simulation/test_utp.cpp` cases `utp_pmtud`, `utp_plain`,
  `utp_buffer_bloat`, `utp_straw`, and `utp_small_kernel_send_buf` for PMTU,
  delay, constrained-link, buffer, and recovery outcome selection;
- `src/utp_socket_manager.cpp::mtu_for_dest` and incoming socket setup for the
  manager's initial IP/UDP payload ceiling and per-destination ownership;
- `src/utp_stream.cpp::update_mtu_limits`, `send_pkt`, `resend_packet`,
  `experienced_loss`, `ack_packet`, `incoming_packet`, `do_ledbat`, and `tick`
  for probe isolation, acknowledged-floor growth, black-hole fallback,
  congestion/window updates, timeout, and lifecycle behavior; and
- `src/utp_stream.cpp::init_mtu` for the initial 548-byte IPv4 floor and
  bounded search interval.

Adopted behavior is the evidence ordering and invariants, not libtorrent's
Asio architecture, optional slow start, exact buffers, or test fixtures. Its
GPL-3.0 simulator submodule remains uninitialized and unexecuted; only the BSD-
licensed test driver source was read. No reference source or test vector is
copied.

The local JSTorrent sibling remains at
`9895410beeed6aff554053769bd006a3fbd373ef`. Its implemented engine has no uTP
runtime; archived notes identify uTP as missing, while its retained BEP 29,
BEP 5 implied-port, PEX capability, and hole-punch documents add no product
behavior to preserve. No JSTorrent source or fixture is copied.

## Owner, Task, Cancellation, And Dependency Map

| Owner | Bounded work | Cancellation and termination |
| --- | --- | --- |
| WAN cohort orchestrator | six fresh cases, route/redaction checks, summaries, exact child and artifact ownership | whole-case timeout or interruption stops roles, audits leases, and removes exact run directories |
| Selected gateway owner | one query-confirmed UDP lease | deletes and confirms absence before socket teardown; finite expiry is only a crash backstop |
| RSTorrent diagnostic role | one shared UDP/uTP service, peer stream, verifier or upload owner | normal peer completion/stop, then joined incoming/uTP/UDP shutdown |
| Pinned remote role | one libtorrent session/torrent and at most one peer | attached SSH owner removes torrent, aborts session, emits terminal counters, and exits |
| Impairment relay | two UDP endpoints and one bounded scheduled-event queue | parent cancellation drains/drops bounded events, closes sockets, joins, and removes its directory |
| uTP connection worker | existing connection state, queues, clock, timers, stream events, and send handle | stream/service/generation cancellation aborts state and publishes one terminal result |
| Diagnostic MTU configuration | fixed floor/ceiling supplied at service construction | connection-local state disappears with worker; ordinary default remains fixed |

Protocol packet, reliability, congestion, and MTU state stays independent from
Tokio, sockets, SSH, UPnP, filesystems, and the relay. The engine runtime
depends inward on an explicit plain configuration and continues to translate
send outcomes into sans-IO transitions. WAN orchestration remains outside the
product/application owner.

## Staged Execution And Commit Plan

1. Commit this tactical and authoritative queue/checkpoint reconciliation.
2. Add the remote leecher and local mapped-seed diagnostic with deterministic
   argument, route, output, lease, failure, and cleanup tests. Re-run the
   existing two-role loopback and remote-mapped gates; commit.
3. Run and repair the complementary WAN direction, independently audit
   cleanup, record its first evidence, and commit.
4. Add the bounded cohort summarizer, run three fresh samples per direction,
   record medians/ranges and residue audits, and commit.
5. Add the bounded deterministic UDP relay and missing hostile/lifecycle
   runtime cases. Run the fixed matrix against pinned libtorrent, repair only
   evidence-backed defects, and commit coherent changes.
6. Add explicit diagnostic MTU configuration and compose it with the relay's
   size-black-hole case. Measure fixed and diagnostic behavior. Do not change
   the ordinary runtime floor without the later product review; commit.
7. Run and record:

```text
source ~/.profile
cargo test -p rstorrent-protocol utp
cargo test -p rstorrent-engine utp
cargo test -p rstorrent-engine port_mapping
uv run --project tests/interop --locked \
  python tests/interop/test_utp_wan_contract.py
uv run --project tests/interop --locked \
  python tests/interop/utp_rstorrent_interop.py
uv run --project tests/interop --locked \
  python tests/interop/utp_rstorrent_wan.py --host pimom --cohort 3
uv run --project tests/interop --locked \
  python tests/interop/utp_runtime_impairment.py
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

8. Reconcile every owning topic, leave product behavior and BEP 29 unchanged,
   commit the completed or evidence-limited result, and stop at the pre-product
   human review.

## Result And Evidence

Pending execution.
