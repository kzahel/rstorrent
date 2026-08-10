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

### Complementary mapped WAN stage

Commit `3600bae` adds the complementary roles without changing ordinary
product behavior. The diagnostic seed binds one concrete local IPv4 UDP
socket, records its exact port before mapping, creates only the same external
UDP port through the engine's existing IGD v2 owner, and deletes the lease
before socket shutdown. The attached `pimom` role runs pinned libtorrent
`2.0.13.0` as a forced-uTP leecher, rejects an overlay route, hash-verifies the
fixture, and exits with one-peer bounds. The controller independently audits
the exact local port and removes only the recorded remote PID and run
directory.

The first physical attempt reached the local gateway and completed exact-port
absence query, then the gateway reset the idempotent
`GetExternalIPAddress` HTTP exchange before any mapping was added. The exact
cleanup audit passed. A bounded two-attempt idempotent SOAP query helper now
retries only transport failures for external-address and specific-entry
queries; mutating add/delete semantics remain under their existing
reconciliation rules. A scripted gateway test reproduces the first-response
reset and proves exactly one retry.

The repaired fresh run passed in 92.140 seconds. Pinned libtorrent received
and SHA-1 verified all 2,097,883 bytes, observed exactly one uTP peer and zero
TCP peers, and reported 3,994 inbound/4,103 outbound uTP packets, one timeout,
and zero packet-loss, fast-retransmit, or resend counters. RSTorrent uploaded
exactly 2,097,883 payload bytes through one incoming uTP generation. Its
smoothed RTT ranged 153.315--156.719 ms, queue delay 0--1.090 ms, congestion
window 1,056--6,864 bytes, connection datagram queue high water two, and fixed
selected MTU 548 bytes. It reported zero malformed, unknown, stale,
connection-queue-drop, session-drop, retransmission, loss-reduction,
timeout-collapse, or worker-panic counters.

The local mapping was exact UDP, query-confirmed with a finite 3,600-second
lease, deleted during joined RSTorrent shutdown, and independently absent
afterward; the audit did not need to recover it. Terminal incoming, uTP, UDP,
queue, process, local temporary-directory, and remote run-directory ownership
was zero. Raw endpoints, gateway identity, peer ID, and transient files were
not retained. This is the first reverse-direction sample, not the three-sample
cohort or a product/support claim.

Validation through this stage:

- all 18 focused engine port-mapping tests pass;
- the diagnostic binary argument test and warning-denying focused Clippy pass;
- all eight WAN controller contract tests pass; and
- both existing forced-uTP loopback roles still transfer the exact fixture
  with terminal cleanup.

### Cohort preparation and transient choke repair

Commit `04aa65c` adds the three-sample-per-direction runner. It alternates
directions after each exact cleanup, reuses one build, retains every redacted
sample, and derives deterministic median/ranges for timing, packet/byte,
delay, RTT/RTO, window, retransmission, MTU, queue, and oracle counters.

The first cohort attempt stopped after earlier samples had cleaned up when the
remote libtorrent seed transiently choked the RSTorrent diagnostic leecher
after 1,043 payload bytes. Remote abort evidence still showed one uTP peer,
zero TCP payload path, four inbound/seven outbound uTP packets, exact mapping
deletion, and no residue. The failure exposed a diagnostic-only peer-wire gap:
ordinary RSTorrent swarm state retains a choked peer and releases/reschedules
its requests, and pinned libtorrent likewise treats `Choke` as a state change,
but the single-peer uTP evidence downloader treated it as terminal.

The controlled downloader now accepts a block already in flight after a
choke, ignores the corresponding Fast-extension rejection while choked,
waits for `Unchoke`, and resends its sole request. It permits at most 16 such
retries across the complete fixed fixture and reports the count. Integrity,
one-outstanding-request shape, role timeout, exact hash, and all transport and
cleanup gates remain unchanged. A pure test proves the exact retry ceiling;
the two-role loopback oracle passes with zero retries.

The restarted cohort exposed the corresponding late-response edge: after a
choke/retry, libtorrent delivered a byte-valid duplicate of piece 0 block 0
while the diagnostic was awaiting block 16384. The abort again deleted the
exact remote mapping and reported no cleanup residue. The diagnostic may now
discard at most 16 blocks only when their index/range is strictly earlier than
the sole current request and their bytes exactly equal already hash-bound
payload/current-piece bytes. Future, overlapping-undelivered, out-of-range,
or byte-different blocks remain terminal protocol errors. The duplicate count
is explicit cohort evidence, and pure tests cover same-piece, prior-piece,
mismatch, future, range, and exact-ceiling cases.

The next restart advanced beyond both peer-wire failures, then one local-seed
sample reached the remote leecher's 180-second transfer bound without exact
completion. Exact local/remote cleanup again passed. Because the remote role
previously emitted no terminal statistics on this path and the controller
discarded the local seed's failed-stop detail, no transport change follows
from that incomplete observation. The attached role now emits one bounded
failure event with content progress and aggregate uTP counters, and the
controller preserves the local seed's partial-upload failure detail. One
single-direction diagnostic retry will distinguish transient path variance
from a reproducible sender stall before implementation changes.

That diagnostic retry passed with exact completion in 58.777 active seconds,
zero libtorrent loss/timeout/resend counters, fixed 548-byte RSTorrent MTU,
exact lease deletion, and zero residue, so the first timeout alone did not
justify a transport change. A later fresh-cohort local-seed case nevertheless
reached the same bound. The remote helper emitted its bounded failure event,
but the controller checked the expected nonzero process status before
validating that event and hid its counters. Validation now precedes the process
status check. The second and final single-direction diagnostic retry owns the
decision between transport repair and recording a bounded path-variance gap.

The second diagnostic retry also passed: 64.528 active/90.957 whole-case
seconds, exact payload/hash, one uTP and zero TCP peers, one libtorrent
timeout with zero loss/resend, fixed 548-byte RSTorrent MTU, exact lease
deletion, and zero residue. The two observed 180-second local-send timeouts are
therefore intermittent and bracketed by clean 58.777- and 64.528-second active
transfers. They are not sufficient to select a transport-state repair without
the bounded failure counters that the earlier controller ordering hid.

The WAN attempt budget is exhausted and no further external run is permitted
in this tactical. The planned six-sample cohort closes evidence-limited rather
than being represented as passed. Three individually captured local-send
successes—the initial 92.140-second case and the two diagnostic retries—show:

- whole-case time 85.798--92.140 seconds, median 90.957; the two samples with
  active timing are 58.777 and 64.528 seconds;
- RSTorrent smoothed-RTT minima 153.315--154.459 ms and maxima
  156.719--177.030 ms; queue-delay maxima 0.807--2.793 ms;
- congestion-window maxima 6,864--8,209 bytes, median 6,984, with the same
  1,056-byte minimum and fixed 548-byte MTU in all three;
- 3,995--3,997 RSTorrent outbound datagrams carrying
  2,179,700--2,179,736 bytes and 4,102--4,107 classified inbound datagrams,
  with connection-queue high water two--four;
- libtorrent 3,994--3,996 inbound and 4,101--4,106 outbound packets, zero
  loss/fast-retransmit/resend and zero--one timeouts; and
- exact 2,097,883-byte upload, one uTP/zero TCP peer, finite 3,599--3,600-
  second leases, joined deletion, independent absence, and zero terminal or
  remote residue in every captured success.

These three successes are not substituted for the missing three-sample
remote-seed direction or the interrupted alternating cohort. Earlier
successful cases inside stopped cohort processes did not emit a retained
summary and are not reconstructed from elapsed time. The two remote-direction
peer-wire failures and both local-direction timeouts independently passed
their mapping/process/directory cleanup paths. Further WAN evidence requires a
later human-authorized attempt budget after controlled impairment/lifecycle
work supplies a deterministic diagnosis target.

### Real-socket impairment implementation

The loopback impairment harness places a two-socket UDP relay between pinned
libtorrent's outgoing-only leecher and the RSTorrent diagnostic seed. The
relay learns exactly one client endpoint, shallowly recognizes only the fixed
20-byte uTP header, and applies the predeclared profiles by packet/DATA
ordinal. It retains the tactical's 10,000-decision, 16-MiB considered-byte,
256-datagram/1-MiB queue, 180-second, and zero-terminal-queue bounds. No
protocol/runtime state depends outward on the relay.

The first clean diagnostic found a libtorrent-only self-connection candidate
beside the established relay peer because the inherited loopback oracle
setting allowed multiple connections from one IP. The candidate's remote and
local endpoint were both the oracle's own listener and it carried no payload;
RSTorrent and the relay each retained one peer/client. The impairment leecher
now disables both incoming uTP and multiple-same-IP connections, neither of
which is needed for its sole outgoing connection. The repaired clean case
passes in 8.046 active seconds with exact hash/accounting, one uTP/zero TCP
peers, 8,047 decisions, 2,263,555 considered/forwarded bytes, 3,991 DATA
datagrams no larger than 548 bytes, queue high water 14 datagrams/1,738 bytes,
zero relay or runtime drops/retransmissions, and terminal zero ownership.

The first full-matrix attempt then reached the existing 30-second fast
loopback-role wrapper during an impaired profile. A distinct loopback-only
`impairment-seed` now uses the tactical's 180-second role bound; ordinary
`seed` stays at 30 seconds and the new role has no WAN mapping path. Its parser
and warning-denying build pass before retrying the matrix.

With the correct bound, the alternating 5/25 ms profile still failed to
finish the exact fixture in 180 seconds. The impairment role now accepts a
diagnostic-only `snapshot` command while it waits for final `stop`; all other
seed scopes reject that command. Timeout handling captures bounded live
incoming/uTP/UDP state, libtorrent progress/stats, and aggregate relay counters
before terminating the failed case. Successful profiles also require a
pre-stop snapshot with exact upload accounting, proving the command path. No
packet payload or per-packet log is emitted.

The first bounded snapshots exposed two separate facts. First, delay/jitter
ordinals must be independent in each relay direction; sharing one ordinal let
interleaved ACK and DATA traffic assign nearly constant delay to one direction.
The policy and tests now enforce independent alternating 5/25 ms sequences.
Second, repeated runs still reached only 196,608--655,360 verified bytes before
the bound despite zero relay drops and relay queue high water no greater than
16. A pinned libtorrent-to-libtorrent control completed the same fixture and
profile in about 24 seconds, although the deliberately severe reordering made
the oracle record 924 seed resends, one seed timeout, 33 leecher resends, and
two leecher timeouts.

RSTorrent's failure snapshots localized one recovery defect: its effective RTO
could climb from 500 ms to the 60-second cap while valid peer traffic remained
active. Pinned `utp_stream.cpp` resets `m_num_timeouts` and restarts the timeout
from `receive_time` after every valid incoming packet, after ACK/SACK parsing.
RSTorrent instead reset consecutive timeout state only on new send-ledger ACK
progress and recomputed from an old transmission time. Established accepted
packets now reset backoff from their receive time; wrong connection IDs and
packets rejected before establishment remain atomic. Deterministic send and
connection regressions, all 84 uTP protocol tests, and warning-denying protocol
clippy pass. A post-repair real-socket retry limited the RTO high water to 4
seconds but still stalled after 196,608 verified bytes. Its final wire summary
showed RSTorrent DATA fully acknowledged while RSTorrent's receive ACK lagged
seven client sequence numbers, so receiver/runtime ingress recovery remains
the next diagnosis rather than sender pacing or timer backoff.

The next bounded run exposed the receiver cause directly: the relay forwarded
every datagram with queue high water 16 and both shared/per-connection runtime
queues stayed at high water 2 with zero drops, but the codec counted 886
malformed packets. BEP 29 requires SACK payloads of at least four bytes in
multiples of four; pinned libtorrent's `send_pkt` instead emits exactly
`(m_inbuf.span() + 7) / 8` bytes, bounded by the effective MTU. RSTorrent now
keeps its own encoder standards-compliant while tolerating received SACKs of
1--252 bytes. Zero length and over-bound inputs remain rejected, all bits
beyond the actual sent sequence range remain inert, and the existing extension
count, packet size, and duplicate-SACK bounds are unchanged.

The same real-socket profile then completed the exact fixture in 21.283 active
seconds with exact SHA-1/accounting, one uTP/zero TCP peers, 9,701 forwarded
decisions, zero relay/runtime/malformed/unknown drops, zero libtorrent loss,
timeout, or resend counters, and terminal zero ownership. RSTorrent recovered
227 datagrams under the deliberate reordering with no timeout collapse; its
RTO remained 500 ms--1 second and queue high water was three datagrams. The
largest observed DATA datagram was 554 bytes despite a selected 548-byte MTU,
revealing that retransmissions can add a current SACK header to payload sized
under an earlier header. Retransmission construction now omits only that
newly-added SACK when necessary to preserve the original ordinary or probe
datagram limit; its cumulative ACK remains current, and an impossible base
header plus retained payload returns an explicit bounded error. The regression
constructs a full 548-byte DATA packet, introduces receiver reordering after
the first send, and proves its retransmission remains exactly 548 bytes rather
than growing to 554. All 86 uTP protocol tests and warning-denying protocol
clippy pass. A second exact real-socket run completed in 21.122 active seconds
with 9,737 decisions, 207 RSTorrent retransmissions, zero timeout collapse or
runtime drops, queue high water 13 at the relay and two in the runtime, exact
terminal cleanup, and a maximum DATA datagram of exactly 548 bytes.

### Real-socket impairment evidence

The complete six-profile matrix now passes in 71.312 seconds. Every case
transferred and hash-verified the exact fixture with one uTP/zero TCP peer,
kept DATA datagrams at or below 548 bytes, stayed within all relay/runtime
queue and byte bounds, recorded zero malformed/unknown/runtime drops and zero
worker panics, drained the relay queue, and terminated with zero uTP tasks,
connections, half-opens, incoming registrations, or temporary artifacts.

The representative matrix observations are:

| Profile | Active seconds | Applied fault/recovery evidence |
| --- | ---: | --- |
| clean | 7.953 | 8,042 decisions; zero drops, duplicates, retransmissions, losses, or timeouts |
| delay-jitter | 21.264 | 9,646 decisions; relay queue high water 14; 210 RSTorrent retransmissions and 196 loss reductions; zero timeout collapse |
| sparse-loss | 11.541 | 40 exact DATA drops among 4,083 DATA datagrams; 82 retransmissions and 62 loss reductions |
| duplicate-reorder | 11.924 | 52 exact duplicates and 78 delayed reorder selections; 158 retransmissions and 117 loss reductions; no duplicate stream delivery |
| burst-loss | 8.386 | exact DATA ordinals 64--66 dropped; five retransmissions and two loss reductions |
| MTU black hole, fixed baseline | 7.751 | zero drops because every DATA datagram remained at the 548-byte floor; zero retransmissions or loss |

Pinned libtorrent reported zero loss, timeout, fast-retransmit, or packet-
resend counters in the completed matrix. The loss and reordering profiles
instead exercised RSTorrent's bounded SACK/fast-retransmit path. Congestion
loss reductions occurred once per RTT as designed and no profile needed a
timeout collapse. The fixed MTU-black-hole row is only the required baseline;
it does not yet exercise diagnostic probes or justify path-MTU discovery.
