# Tactical 121: Deterministic uTP Loss, Congestion, And MTU

Status: Complete at the runtime-free Stage 2 stopping condition on 2026-08-10;
awaiting the required human review before any Stage 3 shared-UDP or runtime
work. uTP remains unsupported.

Topics: `utp-transport-campaign`, `capability-readiness`,
`oracle-driven-engine-campaign`, `protocol-support`, `peer-lifecycle`

Dependencies: completed Tactical
[`119`](119-deterministic-utp-transport-core.md) owns the hostile v1 codec,
wrapping arithmetic, connection lifecycle, ordered receive state, sent-packet
ledger, ACK/SACK loss signals, RTT/RTO decisions, and terminal cleanup. This
tactical composes those owners without adding a socket, task, channel, or
engine dependency.

## Decision And Desired Outcome

Complete the deterministic transport behavior needed before uTP may enter the
shared UDP runtime. Add one bounded, runtime-free transmission controller and
one deterministic impaired-datagram test harness that together make the
following behavior executable:

1. exact receive-window accounting across reorder storage and bytes delivered
   to, but not yet consumed by, the future stream adapter;
2. a bounded unsent-byte queue, MTU-aware packetization, congestion and remote-
   window admission, delayed acknowledgements, pacing, and actual execution of
   fast-retransmit and timeout intents;
3. an RFC 6817 LEDBAT controller with bounded one-way-delay histories,
   application-limited growth control, loss response, and timeout collapse;
4. packetization-layer path-MTU discovery with one isolated probe, binary
   convergence, explicit too-large feedback, and black-hole behavior; and
5. fixed simulations for clean transfer, delay and jitter, reordering,
   non-congestive loss, congestion loss, constrained receive windows, clock
   offset and drift, MTU black holes, and TCP-like competing traffic.

The outcome is still a sans-IO component. It accepts hostile decoded packets,
explicit monotonic time, local timestamp readings, stream-buffer release, and
datagram-send results; it returns bounded packet intents, payload ownership,
deadlines, delivery, and structured snapshots. The future runtime supplies
all actual I/O and wakeups.

## Stopping Condition

The tactical is complete only when:

1. every controller decision is deterministic under an explicit clock and no
   protocol module depends on Tokio, a socket, a task, a channel, entropy, or
   an engine type;
2. receive and send admission are atomic at every byte, packet, sequence,
   congestion-window, advertised-window, MTU, and queue limit;
3. retransmissions preserve the original sequence and payload, are prioritized
   ahead of new data, and update the Tactical `119` attempt/RTO owner exactly
   once per emitted retransmission;
4. the fixed impaired-link scenarios meet all thresholds below and record
   exact queue/resource high-water marks;
5. full Rust formatting, clippy, and workspace tests pass; and
6. the uTP campaign and support truth are reconciled without adding runtime,
   interoperability, WAN, product, or BEP 29 support claims.

The next human review occurs at this stopping condition. Stage 3 shared-UDP
classification, runtime ownership, and loopback interoperability do not begin
without that review.

## Exact Resource And Work Bounds

- Total receive payload credited to one connection is at most 1 MiB. This
  includes both out-of-order payload retained by `ReceiveState` and contiguous
  bytes already handed to the future stream adapter but not yet reported
  consumed. Moving bytes between those categories never creates additional
  window credit. The existing 64-packet reorder cap remains.
- One connection additionally queues at most 1 MiB of unsent application
  bytes. The existing sent ledger separately retains at most 1,024 packets and
  1 MiB until acknowledgement. Queue rejection reports requested, current, and
  maximum bytes and makes no partial append.
- A send poll emits at most one datagram intent. It inspects at most the 1,024
  sent records, 64 reordered receive packets, 32 current delay samples, ten
  base-delay buckets, and one MTU probe. No input or tick starts an unbounded
  loop.
- The advertised remote window and congestion window are byte counts. New
  payload is admitted only when its entire payload fits both remaining
  windows. ACK-only packets do not consume either payload window.
- The base-delay estimator stores ten one-minute minima. The current-delay
  minimum stores at most 32 samples and discards samples older than one
  smoothed RTT; before an RTT exists it uses the one-second initial timeout as
  the horizon. Idle minute rollover expires all ten buckets rather than
  retaining stale path history indefinitely.
- Delayed acknowledgement state owns one deadline and a count capped at two.
  It does not queue packets or samples.
- The path-MTU owner stores one search interval and at most one in-flight
  probe. MTU values are UDP payload bytes including the uTP header and
  extensions, but excluding IP and UDP headers. Construction requires
  `150 <= floor <= ceiling <= 65,535`; ordinary datagrams use only the proven
  floor and probes use the binary midpoint.
- The deterministic link has explicit finite event, datagram-byte, queue-byte,
  and simulated-time limits. Scenario helpers reject rather than silently
  truncate when any limit is exceeded. No randomized test is evidence for the
  stopping condition.

Snapshots expose current and high-water unsent bytes, sent packets/bytes,
receive reorder and delivered bytes, advertised window, in-flight bytes,
congestion window, delay histories, pacing deadline, pending ACK state,
retransmission work, MTU interval/probe, link queue bytes, drops, duplicates,
reorders, and terminal ownership.

## Controller Contract

This tactical selects the conservative complete algorithm in RFC 6817 rather
than libtorrent's optional slow start:

- `TARGET = 100 ms`, `GAIN = 1`, `ALLOWED_INCREASE = 1`, and no slow start;
- initial and ordinary minimum congestion windows are two current MSS values;
- each newly acknowledged payload applies
  `GAIN * off_target * acknowledged_bytes * MSS / cwnd` using saturating
  fixed-point integer arithmetic;
- positive growth is capped at the pre-ACK flight size plus one MSS and is
  suppressed when the sender was application- or remote-window-limited;
- a queue-delay sample is the current minimum minus the ten-minute base
  minimum, using wrapping `u32` one-way samples and clamping impossible values
  to the Karn-safe RTT sample when one exists;
- a congestion loss halves the window, but no more than once per smoothed RTT,
  with a two-MSS floor; and
- a retransmission timeout collapses the window to one MSS and uses the
  existing exponentially backed-off RTO. The next successful controller ACK
  restores the ordinary two-MSS floor.

The controller never treats an isolated failed MTU probe as congestion. A
probe is isolated only when it is the sole newly identified loss immediately
after the preceding ordinary packets are acknowledged; otherwise ordinary
loss response also applies. Duplicate ACK and SACK signals continue to come
from Tactical `119`, so packet loss has one reliability owner and one
congestion response owner.

Pacing spaces new DATA and retransmissions by
`payload_bytes * smoothed_rtt / cwnd`, with a one-microsecond minimum when the
quotient is nonzero. ACK-only traffic is not paced. The explicit next-send
deadline is advisory to the future runtime but mandatory in deterministic
polling.

### Recorded Algorithm Differences

- BEP 29 describes a two-minute sliding base minimum and permits windows down
  to a 150-byte packet. RFC 6817 instead recommends ten one-minute minima and
  two-MSS initial/minimum windows. This tactical follows RFC 6817 because it is
  the complete congestion-control specification and retains BEP 29's 100-ms
  target and 150-byte absolute packet lower bound.
- Pinned libtorrent 2.0.13 keeps twenty minute buckets after enough samples,
  begins at one MTU with slow start, uses a configurable 3,000-byte-per-RTT
  gain, and keeps an ordinary one-MTU floor. Standalone libutp keeps thirteen
  minute buckets, a three-sample current minimum, slow start, and a legacy
  ten-byte numeric floor. These are deployed tuning choices, not wire
  interoperability requirements. RSTorrent follows RFC 6817's conservative
  no-slow-start algorithm and fixed bounds so its evidence is auditable.
- RFC 6817 requires at least one ACK per RTT but leaves delayed-ACK policy to
  the framing protocol. RSTorrent immediately ACKs out-of-order data, FIN,
  duplicates that may drive recovery, the second contiguous DATA packet, and
  reopening a zero window. One otherwise-contiguous DATA packet is delayed
  until `min(25 ms, smoothed_rtt / 4)`, never later than one RTT. Any outgoing
  DATA or FIN piggybacks and clears that pending ACK.

## Packetization, Flow-Control, And Recovery Contract

- The application queue is a byte stream, not a queue of pre-segmented
  packets. New DATA takes the largest prefix fitting the current ordinary or
  probe datagram after the exact 20-byte header and current SACK extension.
  Payload is removed from the unsent queue only after the connection ledger
  accepts it.
- New DATA is never emitted when either remaining congestion or remote receive
  window is zero. A smaller positive window produces a smaller packet instead
  of stranding an older segmentation choice. Zero-window reopening schedules
  an immediate STATE response; active zero-window probing is deferred to the
  runtime/interoperability slice because the BEP does not define it.
- The local advertised window is `1 MiB - reorder bytes - delivered but
  unconsumed bytes`. Duplicate and rejected data cannot consume it. Explicit
  stream consumption returns exactly that credit, and over-release is an
  error without mutation.
- Retransmissions preserve their original type, sequence number, and payload;
  they are never repackaged around a smaller window or MTU. A retransmission
  larger than the current congestion window may proceed only when no other
  payload is in flight, matching the deployed deadlock escape.
- Fast-retransmit work is coalesced by sequence number. Timeout marks the
  existing ledger's bounded outstanding set, but one poll emits only the
  oldest required retransmission. Transmission-attempt exhaustion is a typed
  terminal condition for the future runtime.
- Every DATA/FIN/STATE intent carries the current advertised window, ACK/SACK,
  timestamp, and most recent measured remote timestamp difference. Those wire
  fields are generated at emission time; stored retransmission payload and
  sequence identity do not change.

## Path-MTU Contract

The packetization-layer search follows the pinned libtorrent and standalone
libutp behavior where it is independent of their runtimes:

- the adapter supplies an initial proven UDP-payload floor and interface/path
  ceiling. The Stage 2 IPv4 profile uses 548 and 1,472 bytes, derived from the
  576-byte IPv4 minimum and 1,500-byte Ethernet MTU after IPv4/UDP headers;
- ordinary packets use the proven floor. After three floor-sized ordinary
  packets can surround a probe and the congestion window exceeds three floors,
  one DATA packet may use the binary midpoint with a do-not-fragment intent;
- acknowledging the probe raises the floor. An explicit message-too-large
  result, three later ACKs with the probe as the missing packet, or a timeout
  where it is the only outstanding packet lowers the ceiling to probe size
  minus one. Search converges when the interval is at most 16 bytes;
- an isolated probe failure does not halve the congestion window or increment
  congestion timeout backoff. The same sequence and payload may be retried
  without the do-not-fragment flag, preserving uTP packet identity and allowing
  IP fragmentation as the deployed compatibility fallback; and
- if a non-probe at the proven floor receives a local message-too-large result,
  the adapter contract is false and the connection reports a typed
  minimum-MTU failure. It does not split an already sequenced uTP packet.

The MTU black-hole scenario drops do-not-fragment datagrams over a fixed path
limit without returning ICMP, while allowing the same retransmission without
that flag. It must converge to within 16 bytes of the path limit, preserve the
payload hash, and record zero congestion reductions attributable solely to
the isolated probes.

## Deterministic Impairment Harness And Thresholds

The test-only harness models two endpoint clocks and a bounded, serializing
datagram link. A fixed script controls propagation delay, signed jitter, drops,
duplication, reordering, bandwidth, queue capacity, MTU/DF behavior, clock
offset, and clock drift. A TCP-like competitor uses Reno additive increase,
multiplicative decrease, and the same bottleneck queue only to provide a
deterministic foreground load; it is not a TCP implementation or product code.

The stopping scenarios use at least 4 MiB where transfer duration is relevant
and must prove:

| Scenario | Required result |
| --- | --- |
| Clean 20-ms RTT | Exact bytes and hash; zero loss/retransmit; link utilization at least 80% after the first second; every queue and owner within its bound. |
| Fixed jitter and reordering | Exact bytes and hash; no spurious terminal state; reorder high-water no more than 64 packets; delayed ACK never exceeds one RTT. |
| Scripted 1% non-congestive loss | Exact bytes and hash; every loss recovered within the eight-attempt limit; terminal queued, sent, receive, and link bytes are zero. |
| Queue congestion | Exact bytes and hash; queue delay reaches the controller target neighborhood, and p95 after warmup is at most 150 ms; congestion loss halves at most once per RTT. |
| TCP-like competitor | During overlap, the competitor receives at least 70% of delivered bottleneck bytes and its p95 queue delay is no more than 150 ms; after it ends, uTP returns to at least 70% utilization within ten RTTs. |
| Receive pressure | Advertised window reaches zero without exceeding 1 MiB, no further new payload is admitted, and consuming bytes reopens exactly the released credit. |
| Clock offset, wrap, and bounded drift | Offsets and `u32` wrap do not change queue delay; fixed drift stays bounded by the rolling history and does not create unchecked growth or terminal state. |
| MTU black hole | Exact bytes and hash; search finishes within ten probe outcomes and 16 bytes of the path limit; isolated probes do not reduce cwnd; no datagram exceeds the supplied ceiling. |

If the fixed competitor threshold cannot be met without departing from RFC
6817 or if a normative/deployed conflict changes stream correctness, stop and
return for human review rather than tuning the threshold after seeing the
result.

## Owner And Dependency Direction

- `utp::receive` retains sequence ordering and additionally owns exact local
  receive-credit accounting because that state defines the advertised window.
- `utp::congestion` owns delay history, congestion window, loss epochs, timeout
  collapse, and pacing math. It depends only on integer values and explicit
  time.
- `utp::mtu` owns the path interval and the single probe lifecycle. It does not
  inspect sockets or operating-system errors; the caller maps a send result to
  its typed input.
- `utp::transport` owns the unsent queue, remote advertised window,
  retransmission work, delayed ACK, packetization, and composition with
  `ConnectionState`, congestion, and MTU. It returns one emission per poll.
- `utp::simulation` is compiled only for tests and owns deterministic link,
  endpoint-clock, impairment-script, and competitor fixtures.

The dependency order is packet/sequence and deterministic state inward,
transport composition outward, and the later engine runtime outermost. No
generic network simulator, transport trait, or async abstraction is introduced.

## Source-First Record

Normative sources re-read before fixing this tactical:

- managed BEP 29 at pinned BitTorrent BEP commit
  `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`, especially congestion window,
  advertised window, timestamp feedback, loss, timeout, packet-size, and
  100-ms congestion-control sections; and
- RFC 6817 Sections 2.2--2.5 and 3--5 at the RFC Editor, especially the full
  sender algorithm, receiver ACK requirement, ten-bucket base history,
  current-delay age/sample guidance, application-limited cap, two-MSS values,
  loss epoch, timeout collapse, and competition requirements.

Primary completeness oracle at pinned libtorrent commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/aux_/utp_stream.hpp` fields and `timestamp_history`;
- `src/utp_stream.cpp` `send_deferred_ack`, `update_mtu_limits`, `send_pkt`,
  `resend_packet`, `experienced_loss`, `ack_packet`, incoming ACK/delay/window
  handling, `do_ledbat`, `packet_timeout`, `tick`, and MTU initialization;
- `src/utp_socket_manager.cpp` deferred-ACK drain and MTU restriction;
- `src/settings_pack.cpp` uTP target, gain, timeout, resend, and loss defaults;
- `test/test_utp.cpp` forced-uTP transfer and wrap cases; and
- `simulation/test_utp.cpp` PMTU, plain, bufferbloat, constrained-path, and
  small-kernel-buffer scenarios. The GPL simulator remains read-only and is
  not initialized, linked, run, copied, or distributed.

Secondary edge inventories:

- standalone libutp commit
  `2b364cbb0650bdab64a5de2abb4518f9f228ec44`, `utp_internal.cpp`
  `DelayHist`, `schedule_ack`, receive-window accounting, `send_packet`,
  `apply_ccontrol`, timeout/loss processing, MTU search, and ICMP handling;
- librqbit-utp commit
  `c26f57b2debbe35ed0ace1ad419de529f7a5bf95`, especially delayed-ACK,
  flow-control, fast-retransmit, retransmit-timer, congestion, lossy-socket,
  and MTU-probing tests. Its CUBIC controller is not adopted because LEDBAT
  remains its explicit TODO; and
- JSTorrent sibling commit
  `9895410beeed6aff554053769bd006a3fbd373ef`, whose active engine has no uTP or
  LEDBAT implementation to preserve. Its BEP copies and archived aspirations
  add no product behavior requirement.

No source, simulator code, constant table, fixture, or test vector is copied.
Tests are independently authored from public protocol behavior and the edge
cases recorded here.

## Validation

Focused deterministic tests cover every formula boundary, history rollover,
clock wrap, app-limited gate, loss epoch, timeout, pace deadline, queue/window
limit, delayed-ACK transition, exact packetization size, retransmission
priority and identity, probe transition, failed send, and scenario threshold.

Run:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

No Python oracle, client, Android, WAN, public swarm, `pimom`, or physical-
device run is authorized or useful for this runtime-free slice.

## Non-Goals

- No UDP socket, session datagram classification, endpoint map, half-open or
  established connection collection, task, channel, cancellation token,
  generation replacement, real timer, OS pacing, or kernel-buffer policy.
- No ordered async stream, peer handshake, peer-set integration, MSE-over-uTP,
  DHT routing change, tracker/PEX capability flag, TCP/uTP racing or fallback,
  listen advertisement, setting, status, or product surface.
- No UDP port mapping, IPv6 pinhole, NAT traversal, hole punching, LSD, WAN,
  `pimom`, public swarm, or physical-device work.
- No active zero-window probes, generic Nagle layer, adaptive controller
  tuning, bandwidth setting, multiple congestion algorithms, AQM model, or
  production network simulator.
- No source copying, mechanical translation, vendoring, FFI, dependency,
  manifest change, third-party notice change, or support-claim promotion.

## Escalation

Ordinary fixed-point representation, module layout, deterministic fixture,
and bounded queue decisions within this contract proceed autonomously. Stop
for direction before weakening a threshold or bound, changing the selected
RFC 6817 controller, adding slow start or another algorithm, accepting foreign
source, adding a dependency, changing shared UDP/runtime ownership, or using
any real/external network.

## Execution Record

Stage 2 landed in these coherent commits:

- `ccb93a5` fixed this tactical's controller, resource, MTU, impairment, and
  acceptance contracts before implementation;
- `1795b66` added the bounded bidirectional serializing link, scripted
  impairments, endpoint clocks, and Reno-like foreground controller;
- `93de3e6` made the 1-MiB receive credit exact across reordered and delivered-
  but-unconsumed bytes;
- `fbed590`, `73624ad`, and `5213edf` added bounded packetization/ACK/recovery,
  RFC 6817 fixed-point congestion and pacing, and binary path-MTU state;
- `cdbceb1` composed those owners into one runtime-independent transport poll
  and datagram-result API;
- `f7d1d7a` and `c58db59` drove exact encoded transfers through the link and
  added clean, jitter/reorder/duplicate, and scripted-loss evidence;
- `a585668` added queue, clock, and MTU black-hole evidence and fixed stale
  SACK handling for a fragmentable probe retry;
- `d485a65` proved exact zero-window pressure and reopening; and
- `360dde2` added the shared-bottleneck TCP-like foreground result.

The composed implementation retains the Stage 1 connection ledger as the
sole packet/reliability owner. `utp::transport` additionally owns the 1-MiB
unsent stream queue, exact local and remote receive-window admission, delayed
ACK state, one-datagram polls, retransmission work, pacing, congestion, and
MTU composition. `utp::congestion` uses ten one-minute base buckets, at most
32 current samples, saturating fixed-point RFC 6817 gain, no slow start, one
loss reduction per RTT, and one-MSS timeout collapse. `utp::mtu` uses UDP-
payload units, one active binary probe, the 548-byte IPv4 floor, a 1,472-byte
ceiling, and same-sequence fragmentable retry. No socket, task, channel,
runtime, entropy source, unsafe code, manifest change, or foreign source was
added.

One integration failure was found and repaired. After an oversized probe was
identified from three later acknowledgements, already-in-flight ACKs could
repeat the same SACK loss signal after the fragmentable retry had been sent.
Treating the retry as ordinary DATA produced duplicate retries, duplicate
ACKs, and a false congestion reduction for the next packet. The transport now
coalesces such a repeated signal while that exact retry remains in flight and
keeps it MTU-isolated; a genuinely lost fragmentable retry still reaches the
ordinary timeout owner. The fixed 1,280-byte black-hole test covers this
transition.

### Fixed Scenario Results

All byte-stream scenarios compare exact length and SHA-1 identity. Virtual
durations and high-water values below come from the final deterministic run:

| Scenario | Result |
| --- | --- |
| Clean 20-ms RTT, 4 MiB | Completed in 8.7235 s with zero drops/retransmissions and 98.34% post-first-second utilization. Queue-delay p95 was 86.75 ms; queue high-water was 47,721 bytes. |
| Jitter/reorder/duplicate, 4 MiB | Completed in 9.2875 s with 36 reordered and 16 duplicated link datagrams, zero retransmissions, and receive reorder high-waters of six packets/9,175 bytes. |
| Fixed 1% ordinal loss, 4 MiB | Recovered 58 scripted drops with 229 retransmissions, no timeout, and at most seven transmissions for one sequence, below the eight-attempt limit. All terminal byte/event owners were zero. |
| Bounded queue, 4 MiB | Maximum controller queue delay was 89.25 ms and p95 was 86.75 ms against the 150-ms gate; link queue high-water was 47,721 of 55,000 bytes. |
| Clock offset/wrap/drift, 4 MiB | Completed in 8.722 s across wrapping endpoint timestamps and opposing 1,000-ppm drift. Queue-delay p95/max were 96.225/98.806 ms and congestion state remained bounded. |
| DF black hole at 1,280 bytes, 4 MiB | Six outcomes (three black holes and three acknowledgements) converged to `1269..=1282`, a 13-byte interval containing the path limit. Three same-sequence retries completed; all six probe/retry loss signals were congestion-isolated, with zero congestion reductions or timeout collapses. |
| Receive pressure, 2 MiB | Receive ownership reached exactly 1,048,576 bytes and a zero advertised window. Releasing 65,536 bytes reopened exactly that credit; no new DATA was emitted while the sender observed zero, and all retained bytes drained. |
| Established TCP-like foreground, 8 MiB uTP stream | During the fixed overlap the foreground received 77.0876% of payload bytes and its queue-delay p95 was 124.144 ms. Twelve drop-tail losses exercised both controllers; the foreground recorded ten retransmissions/two loss reductions and uTP recorded two loss reductions. After foreground stop, uTP reached 70% utilization in 475.25 ms, 3.27 of the measured 145.134-ms RTT and below the ten-RTT gate. |

Across these scenarios, the observed per-connection unsent-byte high-water was
the exact 1-MiB limit; sent-ledger high-water was 59 packets/61,338 bytes;
receive high-water was 26 reordered packets and the exact 1-MiB pressure
limit; retransmission-work high-water was one; link queue high-water was
74,824 of 75,000 bytes; and pending-link high-water was 81 datagrams/80,239
bytes. These are below the explicit 1,024-packet, 1-MiB sent, 64-packet,
1-MiB receive, 1,024-retransmission, 131,072-event, 8-MiB event-byte, and
4-MiB link-queue bounds. Every scenario finishes with zero unsent, sent,
in-flight, retransmission, receive, pending-event, pending-event-byte, and
link-queue ownership.

### Validation And Deliberate Deferrals

Final validation on 2026-08-10 passes:

- `cargo test -p rstorrent-protocol utp::`: 81 passed, 0 failed;
- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`; and
- `cargo test --workspace`.

The workspace commands completed without failures. No Python oracle, client,
Android, WAN, public swarm, `pimom`, or physical device was run.

The stopping condition is met. Stage 3 still needs a separately reviewed
tactical for shared-UDP classification, connection/runtime ownership,
ordered-stream adaptation, DHT coexistence, loopback interoperability in both
roles, cancellation, generation replacement, and terminal task/resource
evidence. Active zero-window probes remain deferred as specified. This result
does not change TCP-only execution or the BEP 29 **Unsupported** claim.
