# Tactical 144: Long-RTT uTP Sender Window Utilization

Status: **Active under Tactical 142's approved repair stage.** The complete
same-revision 8 MiB WAN matrix isolates an RSTorrent uTP sender window-growth
defect on an idle approximately 155--180 ms RTT path. This tactical may repair
that existing owner autonomously and commit in logical stages. It does not
authorize slow start or another congestion controller.

Topics: `utp-transport-campaign`, `performance-and-live-evidence`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tacticals
[`121`](121-deterministic-utp-loss-congestion-and-mtu.md),
[`125`](125-shared-udp-utp-runtime-and-loopback-interop.md),
[`130`](130-utp-transport-solidification.md),
[`137`](137-product-utp-path-mtu-discovery.md), and active parent Tactical
[`142`](142-wan-transport-performance-matrix.md).

## Decision And Desired Outcome

Restore RFC 6817 congestion-window utilization on a clean high-bandwidth,
long-RTT path while preserving RSTorrent's accepted conservative no-slow-start
controller. The repair must make queued RSTorrent uTP upload data fill the
window that LEDBAT already permits, grow that window at the existing bounded
rate when queue delay is below target, and continue to yield under congestion.

The pre-repair WAN evidence is role-specific and receiver-independent. With
the development host seeding, RSTorrent TCP reaches 2.144--2.777 MiB/s while
RSTorrent uTP reaches 0.106--0.121 MiB/s. Pinned libtorrent uTP reaches
3.270--3.528 MiB/s on the same physical path. Replacing only the RSTorrent
leecher with libtorrent does not improve the RSTorrent uTP seed.

Across both physical directions and both leecher implementations, the
RSTorrent uTP sender has continuously queued bytes, 154--180 ms sampled RTT,
0--6.8 ms queue delay, zero retransmissions, zero timeout collapses, zero
retry exhaustion, and a congestion/flight high water of only 22--36 KiB.
This is not loss response, receive credit, MTU convergence, CPU, storage, or
the remote network ceiling. The window itself predicts the observed rate.

The leading causal hypothesis is the composition between RSTorrent's advisory
per-packet pacer and RFC 6817's flight-size growth cap. The pacer schedules one
MSS at `RTT * MSS / cwnd`; ordinary scheduling overhead can keep actual
pre-ACK flight at least one MSS below `cwnd` on a long RTT path. RFC 6817 then
correctly permits no growth beyond flight plus one MSS. Pinned libtorrent does
not use this separate pacer: it drains queued payload until the congestion or
advertised window is full. This hypothesis must fail deterministically before
behavior changes and pass after them; it is not accepted merely from code
inspection.

## Scope And Stopping Condition

1. Add bounded deterministic and runtime diagnostics that count congestion
   ACKs, newly acknowledged payload, window-growth ACKs, sender-underfilled
   ACKs with queued data, and remote-window-limited ACKs.
2. Add a clean 160 ms RTT/high-capacity deterministic transfer that reproduces
   the low flight/window growth without loss, queue pressure, or receive
   pressure.
3. Repair only normal new-DATA emission pacing if the counters confirm the
   hypothesis. Congestion and advertised windows remain the admission gates;
   retransmissions remain prioritized and bounded.
4. Retain every existing loss, delay, competitor, MTU, receive-pressure,
   hostile runtime, and mixed-engine test. Add a burst/resource assertion for
   the repaired emission behavior.
5. Rerun the controlled eight-cell local role matrix, then the affected WAN
   RSTorrent-seed uTP cells against both leechers in both directions. Run at
   least three rotating 8 MiB repetitions and one 64 MiB scaling cohort.
6. Run TCP and pinned-libtorrent-seed uTP regressions, exact integrity and
   cleanup audits, proportional Android cross-build evidence, and complete
   repository gates.
7. Reconcile Tactical 142 and the living topics before returning to the
   remaining larger baseline.

This tactical completes when the deterministic long-RTT defect is causal and
fixed, the existing fairness/resource bounds still pass, the affected WAN
cohorts show a material repeatable improvement without loss or transport
masking, and all owners clean exactly. If the correctly utilized conservative
controller remains materially slower only because it has no slow start, stop
for human review rather than silently changing the controller selected by
Tactical 121.

## Preserved Controller And Resource Invariants

- Keep RFC 6817 `TARGET = 100 ms`, `GAIN = 1`,
  `ALLOWED_INCREASE = 1`, two-MSS initial/ordinary minimum, no slow start,
  flight-plus-one-MSS growth cap, one loss reduction per RTT, and one-MSS
  timeout collapse.
- Keep the remote advertised window and local 1 MiB sent/unsent bounds as
  independent hard gates. No ACK can create window credit or payload.
- One deterministic transport poll still emits at most one datagram. The
  runtime drains at most 64 emissions per turn and yields; UDP backpressure
  retains the exact pending datagram.
- Keep at most 1,024 sent packets, 1 MiB sent bytes, 1 MiB unsent bytes, one
  pending emission, one MTU probe, and the existing eight-attempt retry bound.
- ACK-only traffic remains unpaced. Retransmissions remain ahead of new data
  and retain their original packet identity and existing loss/timeout gates.
- Dynamic MTU search, revalidation, safe platform send options, and the
  fixed-548 fallback do not change.
- Diagnostics are scalar saturating counters/high waters. They add no packet,
  address, peer ID, payload, task, channel, or unbounded history retention.
- Product transport defaults, fallback/suppression policy, rate settings,
  mapping behavior, TCP behavior, and Android presentation do not change.

## Normative And Source Oracle

Re-read managed BEP 29 at pinned BitTorrent BEP commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`, especially
`beps/bep_0029.rst` congestion control and its byte-window relationship to
send rate. Re-read RFC 6817 Sections 2.2--2.5 and 3.2 at the RFC Editor. The
complete sender algorithm defines `flightsize` as data outstanding before the
ACK, applies the LEDBAT gain, and clamps `cwnd` to flight plus
`ALLOWED_INCREASE * MSS`. Slow start is optional, not required; this tactical
retains the accepted no-slow-start choice.

Pinned Rasterbar libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` remains the completeness and
interop oracle:

- `src/utp_stream.cpp::send_pkt` drains queued bytes up to `min(cwnd,
  adv_wnd)` and records when the window is full;
- `src/utp_stream.cpp` incoming ACK handling passes pre-ACK flight into
  `do_ledbat` and then repeatedly calls `send_pkt` while window space remains;
- `src/utp_stream.cpp::do_ledbat` distinguishes an upper-layer-saturated
  window, uses acknowledged bytes over flight for its linear gain, and has an
  optional slow-start branch that is intentionally not adopted here;
- `include/libtorrent/settings_pack.hpp` and `src/settings_pack.cpp` document
  the deployed 3,000-byte-per-RTT gain default, also intentionally not adopted;
  and
- `test/test_utp.cpp` plus `simulation/test_utp.cpp::{utp_plain,
  utp_buffer_bloat,utp_straw,utp_small_kernel_send_buf}` retain exact transfer,
  delay, fairness, and constrained-send-buffer edges.

No source, fixture, test vector, slow-start policy, gain value, dependency, or
architecture is copied.

## Owner And Dependency Direction

`utp::congestion` remains the runtime-independent delay/window/loss owner.
`utp::transport` remains the runtime-independent queue, flight, window,
packetization, retransmission, MTU, and emission owner. It may expose the new
scalar ACK/window counters and change when already-admissible new DATA becomes
pollable. `utp_runtime` remains the outer socket/task owner and aggregates
bounded evidence; it does not choose congestion policy. Incoming upload and
ordinary peer-wire owners continue to feed an ordered `UtpStream` without
transport-specific tuning.

## Validation And Acceptance Thresholds

- The new clean 160 ms RTT, at least 8 MiB deterministic scenario completes
  exact and loss-free, reaches at least 64 KiB congestion/flight, and improves
  active duration by at least 3x over a checked-in pre-repair observation.
- Existing deterministic clean utilization, queue-delay p95, TCP-like
  competitor share/recovery, loss, timeout, receive-pressure, jitter/reorder,
  and MTU black-hole assertions remain unchanged.
- Runtime tests prove at most 64 emissions per turn, no starvation of ingress,
  controls, or cancellation, bounded queues, and terminal zero ownership.
- Controlled loopback retains exact hashes and one forced transport for all
  eight RSTorrent/libtorrent pairings without a material regression.
- Each post-repair WAN result is exact, single-peer, forced-uTP, ordinary-route,
  same-revision, and cleanup-complete. The affected 8 MiB RSTorrent-seed median
  must improve at least 3x over the corresponding 0.106--0.172 MiB/s baseline;
  64 MiB must show continued scaling rather than a fixed low-window ceiling.
- TCP and libtorrent-seed uTP controls must remain within the path's observed
  run-to-run range unless retained resource/path evidence explains a change.

Repository validation follows `DEVELOPMENT.md`: focused protocol/runtime and
interop tests during implementation, then formatting, warning-denying
workspace Clippy, workspace tests, the relevant Python contracts, and
proportional Android target checks.

## Non-Goals And Escalation

This tactical does not add slow start, change LEDBAT constants, select another
controller, tune for TCP parity, add a user setting, alter TCP incoming upload,
fix the separate remote-placement TCP disconnect observed by Tactical 142,
change bandwidth fairness, add IPv6/MSE uTP, or broaden protocol claims.

Ordinary diagnostic fields, deterministic fixtures, and a causally proven
new-DATA pacing repair proceed autonomously. Stop for human review before
adding slow start, changing `GAIN`, `TARGET`, or `ALLOWED_INCREASE`, weakening
an existing fairness/resource threshold, adding a dependency, or broadening
the repair to the separate TCP disconnect.
