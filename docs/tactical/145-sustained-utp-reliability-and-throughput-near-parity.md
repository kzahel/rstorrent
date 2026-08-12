# Tactical 145: Sustained uTP Reliability And Throughput Near-Parity

Status: **Ready under active parent Tactical 142; implementation has not
started.** Explicit maintainer direction selects continued uTP performance
work with the goal of approaching pinned-libtorrent uTP throughput. This
tactical may autonomously diagnose and repair causal defects in existing uTP,
ordered-stream, and peer-I/O owners. A production congestion-policy change
remains a human-review gate.

Topics: `utp-transport-campaign`, `performance-and-live-evidence`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: active parent Tactical
[`142`](142-wan-transport-performance-matrix.md) and completed Tacticals
[`121`](121-deterministic-utp-loss-congestion-and-mtu.md),
[`125`](125-shared-udp-utp-runtime-and-loopback-interop.md),
[`130`](130-utp-transport-solidification.md),
[`137`](137-product-utp-path-mtu-discovery.md),
[`140`](140-incoming-utp-reachability.md), and
[`144`](144-long-rtt-utp-sender-window-utilization.md).

## Decision And Desired Outcome

Make sustained forced-uTP transfers involving RSTorrent reliable on one
connection, then close the remaining throughput gap against pinned libtorrent
on the same path without sacrificing uTP's delay-sensitive behavior.

The primary near-parity target is a median active payload rate of at least
`0.85x` the alternating same-direction, same-size libtorrent/libtorrent uTP
control for every RSTorrent-containing pairing at 256 MiB and 1 GiB. No valid
sample may reconnect, fall below `0.70x` its reference without a retained
path/resource explanation, use TCP fallback, or omit exact integrity and
cleanup. `0.95x` remains a reported stretch result, not a hidden completion
requirement.

Near parity is an empirical target, not authority to imitate libtorrent's
architecture or tune only one favorable route. Reliability comes first. A
faster transfer assembled from repeated broken connections does not pass, and
a reliable transfer below the target continues into causal utilization work.

## Evidence Selecting This Tactical

### How the evidence was collected

Tactical `142`'s committed lab uses
`tests/interop/wan_transport_matrix.py` as a case-addressable, resumable owner.
It creates deterministic one-file v1 torrents at 8, 64, 256, and 1,024 MiB
with 256 KiB pieces and no tracker, DHT, LSD, PEX, web seed, MSE, or unrelated
peer source. `wan_transport_libtorrent_role.py` supplies pinned libtorrent
`2.0.13.0`; the RSTorrent public probe and incoming seed supply the matching
roles. Every case permits one direct peer and forces TCP or uTP with the other
transport disabled or invalidating the result.

The development host and the authorized `pimom` peer are controlled over SSH;
that control route never carries payload. The seed gateway owns one exact
finite TCP or UDP UPnP mapping, and the leecher dials the mapped public address
over the ordinary Internet. Each sequential case records active and
connect-inclusive timing, payload milestones, forced-transport proof,
RSTorrent/libtorrent counters, process CPU/RSS, endpoint load/storage/iowait,
selected MTU, request/storage backlogs, whole-file and piece integrity,
mapping deletion, joined process cleanup, and artifact removal. One clean
repository revision is staged on both hosts and checked around every case.

The post-repair epoch ran at exact revision
`df68c2f901f66a694cac09436948325cede86780`. It completed all 16 cells at 8,
64, and 256 MiB plus all eight remote-seed 1 GiB cells: 56 exact successes and
13,440 MiB (13.125 GiB) of verified payload. Ignored raw journals and fixtures
were removed after reconciliation; their aggregate results, collection
contract, exclusions, and cleanup audit are committed in Tacticals `142` and
`144` and the owning topics.

One local-seed 1 GiB libtorrent/libtorrent uTP case was maintainer-interrupted
after 8,828 seconds. It retained a typed `ResourceError` and successful
cleanup, is excluded from every ratio, and is environmental evidence only.
The seven remaining local-seed 1 GiB cells were deliberately not run because
the already complete cohorts selected a narrower causal target.

### What the evidence says

Completed Tactical `144` repaired three real defects: new DATA now fills the
already-admitted congestion window, the unchanged upload-writer bound reserves
both possible Piece frames, and a connection's ingress queue now matches the
256-datagram shared uTP ingress stage. Repeated affected 8 MiB medians improve
2.72x--4.48x. The formerly churning remote RSTorrent/RSTorrent 64 MiB cell
improves from 0.125745 to 1.269823 MiB/s, uses one connection instead of
roughly seven, and records zero ingress drops or retry exhaustion.

The larger sizes expose a distinct residual:

| Remote-seed 1 GiB pairing | uTP MiB/s | TCP MiB/s | uTP/TCP |
| --- | ---: | ---: | ---: |
| libtorrent -> libtorrent | 2.739 | 2.609 | 105.0% |
| RSTorrent -> libtorrent | 1.200 | 2.616 | 45.9% |
| libtorrent -> RSTorrent | 0.750 | 2.641 | 28.4% |
| RSTorrent -> RSTorrent | 1.107 | 2.641 | 41.9% |

All four TCP controls cluster within 1.3%, and libtorrent/libtorrent uTP
slightly exceeds its TCP control. The constrained peer's disk, ISP, and the
ordinary path therefore do not explain the RSTorrent-specific gap.

Connection longevity is the strongest discriminator. Remote
libtorrent-seed/RSTorrent-leech uTP starts 2, 4, and 17 connections at 64,
256, and 1,024 MiB and exhausts 1, 3, and 16 connection retries. Remote
RSTorrent/RSTorrent starts 1, 4, and 7 connections and exhausts 0, 3, and 6.
The RSTorrent leecher's last failure is consistently the coarse peer-wire
classification `protocol`; the 1 GiB libtorrent seed emitted 749,186 uTP
payload packets, crossing the 16-bit sequence space many times. RSTorrent
ingress high water remains at most 128 of 256 with zero connection-datagram
drops, so the queue repaired by Tactical `144` is not overflowing.

Resource saturation is also rejected as the first owner. Across the 1 GiB
cohort, RSTorrent RSS high waters remain at most 20.2 MiB while seeding and
27.7 MiB while leeching; pinned libtorrent reaches 684.3 and 1,061.6 MiB in
those roles. Sampled mean CPU is comparable and the TCP controls fill the same
path.

The leading hypothesis is a composed ordered-stream defect at or near reuse
of the 16-bit uTP sequence space. RSTorrent unit tests cover arithmetic and
individual send/receive transitions across one wrap, but no deterministic
transport, runtime, or peer-wire test carries more than 65,536 DATA packets
through one connection. This hypothesis is not yet causal: connection counts
do not equal predicted wrap counts, and the libtorrent-only interrupted case
shows that the route can vary.

## Normative And Source Oracle

Use managed BEP 29 at specification commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06` and RFC 6817 Sections 2.2--2.5
and 3.2 for sequence, acknowledgement, delay, window, loss, and fairness
semantics. A near-parity goal does not override the protocol's requirement to
yield to competing traffic.

Pinned Rasterbar libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` remains the required completeness
and performance oracle:

- `src/utp_stream.cpp::compare_less_wrap`, `consume_incoming_data`, incoming
  ACK validation, cumulative ACK walking, `parse_sack`, and
  `maybe_inc_acked_seq_nr` show the wrap-relative receive, ACK/SACK, loss, and
  ring-slot transitions that must be compared against RSTorrent;
- `src/utp_stream.cpp::send_pkt`, `ack_packet`, `experienced_loss`,
  `packet_timeout`, `tick`, and `do_ledbat` show the separate send admission,
  recovery, startup, and steady-state utilization owners;
- `test/test_utp.cpp::{utp,compare_less_wrap}` proves forced-uTP transfer and
  a small arithmetic wrap comparison, but does not itself prove sustained
  packet-number reuse; and
- `simulation/test_utp.cpp::{utp_plain,utp_buffer_bloat,utp_straw,
  utp_small_kernel_send_buf}` supplies loss, delay, send-buffer, invalid-packet,
  and redundant-packet expectations. Its ordinary payload-packet counts are
  far below a full 16-bit cycle, which reinforces the need for an independently
  authored sustained-cycle regression.

Libtorrent starts in slow start and uses its configured gain behavior.
RSTorrent deliberately retains no slow start, RFC 6817 `GAIN = 1`,
`TARGET = 100 ms`, and `ALLOWED_INCREASE = 1`. These differences are
throughput hypotheses only after connection reliability passes. No source,
fixture, vector, controller value, dependency, or architecture is copied.

Tactical `142` already inspected the local JSTorrent reference at
`9895410beeed6aff554053769bd006a3fbd373ef`: its useful role helpers are
TCP-oriented and it supplies no sustained uTP implementation oracle. The
checkout has unrelated existing changes and remains read-only.

## Owner, Task, Cancellation, And Dependency Map

```text
runtime-independent protocol
  sequence relation + send ACK ledger + receive/reorder ledger
  transport packetization + congestion + loss + MTU + close state
        |
        v
uTP runtime worker
  UDP generation + ingress + emission + exact terminal result
        |
        v
ordered UtpStream / peer I/O
  frame decoder + content/upload owner + typed close reason
        |
        v
controlled and WAN harnesses
  exact fixture + oracle role + mapping + telemetry + cleanup
```

Sequence, ACK/SACK, receive ordering, congestion, and deterministic cycle
simulation remain in `rstorrent-protocol` without Tokio, sockets, peer frames,
or files. `utp_runtime` owns tasks, UDP generations, channels, wakeups, and
terminal transport errors. Peer I/O owns BitTorrent frame interpretation and
must not collapse a transport error, EOF, frame-codec failure, semantic peer
rejection, and local cancellation into one undifferentiated diagnostic.

Every added task or test relay has one cancellation source, a deadline, a
joined terminal result, and exact zero-owner assertions. A test-only initial
sequence or controller variant must be injected through a narrow diagnostic
configuration and cannot enter ordinary product construction.

The concrete boundary improvement is typed terminal provenance from uTP
transport through the peer owner and performance evidence. It makes a
connection restart attributable without adding an unbounded trace or leaking
runtime infrastructure into protocol state.

## Scope And Implementation Stages

1. **Terminal provenance.** Preserve bounded exact categories and safe detail
   for uTP transport/codec failure, local or remote EOF, peer-frame decode,
   content semantic rejection, timeout, retry exhaustion, and cancellation.
   Add scalar DATA/ACK sequence-cycle, last-sequence, duplicate/stale/future/
   ambiguous ACK, too-far-ahead, reorder, RESET, FIN, and terminal high-water
   evidence sufficient to place failure relative to a cycle.
2. **Cycle reproduction.** Add a runtime-independent exact transfer spanning
   at least two complete 16-bit DATA sequence cycles with one connection.
   Cover initial values on both sides of wrap, ACK/SACK across wrap, delayed
   and duplicate ACKs, bounded reorder, one isolated loss, retransmission,
   MTU probes, and FIN after reuse. The common clean path must fail before a
   sequence repair is accepted.
3. **Composed reproduction.** Carry an exact bounded stream through the real
   runtime and peer framing for more than two cycles at fixed-548 and dynamic
   MTU profiles. Run RSTorrent/RSTorrent and both pinned-libtorrent mixed
   directions over clean loopback and the existing clean 160 ms relay. Require
   one connection, monotonically exact bytes, no fallback, and terminal zero
   ownership.
4. **Causal reliability repair.** If the cycle or terminal evidence selects an
   existing sequence/ACK/reorder/runtime boundary, repair that boundary with
   its hostile and resource cases. If it selects another existing uTP or
   peer-stream owner, pivot within this tactical only when the evidence is
   equally causal and the preserved contracts do not change.
5. **Reliability scaling gate.** Run alternating exact 64/256 MiB controlled
   cohorts and affected 256 MiB WAN cells. Zero unexplained reconnects,
   retries, integrity failures, ingress drops, or transport masking are
   required before throughput tuning.
6. **Residual throughput attribution.** On one reliable connection, compare
   ACK cadence, startup duration, bytes in flight, congestion and advertised
   windows, queue delay, loss/retransmission, application feed, MTU, request
   backlog, storage backlog, and CPU. Add deterministic diagnostic-only A/B
   variants for startup and gain only if ordinary utilization owners no longer
   explain the gap.
7. **Bounded throughput repair.** Repair causally selected scheduling,
   batching, ACK, feed, window-accounting, or other existing implementation
   defects autonomously. Do not change production `TARGET`, `GAIN`,
   `ALLOWED_INCREASE`, the no-slow-start choice, or loss reduction without the
   review gate below.
8. **Near-parity WAN cohort.** Run three rotating alternating repetitions for
   all four implementation pairings over forced uTP and matching TCP controls
   at 256 MiB in both physical directions. Run the same cohort at 1 GiB on the
   stable remote-seed direction and one local-seed 1 GiB scaling smoke. Retain
   every sample and exclude only typed invalid/failure records.
9. **Closure.** Rerun delay/fairness, loss, timeout, MTU, receive-pressure,
   hostile runtime, product integration, both Android native builds, and
   complete repository gates. Reconcile Tactical `142`, living topics, and
   the support claim before removing raw owned artifacts.

## Preserved Invariants And Resource Limits

- One connection retains at most 1,024 sent packets, 1 MiB sent bytes, 1 MiB
  unsent bytes, 1 MiB receive bytes, 64 reordered packets, one pending
  emission, one MTU probe, and eight transmission attempts per packet.
- Shared uTP ingress and each connection ingress remain 256 datagrams; one
  runtime turn emits at most 64 datagrams. Any proposed change to these bounds
  requires measured high-water evidence and the same or tighter aggregate
  memory calculation.
- Sequence ordering never invents an order at the half range. Packet identity
  reuse cannot alias an outstanding packet, retransmission, MTU probe, FIN, or
  stale ACK from another cycle.
- ACK/SACK input cannot acknowledge unsent data, create window credit, inject
  loss beyond the sent window, or retain peer-controlled unbounded state.
- Exact ordered bytes are delivered once. Duplicate datagrams are harmless;
  conflicting duplicates, after-FIN payload, malformed extensions, invalid
  RESETs, and hostile window violations remain bounded and typed.
- Existing dynamic-MTU protection/fallback, product `PreferUtp` selection,
  sequential TCP fallback, mapping policy, rate limits, and Android lifecycle
  remain unchanged.
- New diagnostics are saturating scalars, enums, and bounded safe strings.
  They retain no payload, address, public endpoint, peer ID, per-packet log,
  task handle, or unbounded timeline.
- WAN runs remain sequential with one peer and one finite mapping. The
  existing 12-hour per-1-GiB case deadline, two-times-payload wire stop,
  bounded capture, disk headroom, atomic journal, revision fence, redaction,
  exact cleanup, and authorized traffic scope remain in force.

## Validation And Acceptance Gates

| Layer | Required evidence |
| --- | --- |
| Pure sequence/transport | More than two full DATA cycles; exact ACK/SACK, reorder, loss, retry, MTU, FIN, hostile input, and terminal resources across reuse |
| Scripted runtime | Fixed and dynamic MTU, clean and impaired links, no starvation, bounded queues/turns, cancellation and socket-generation replacement, exact terminal cause |
| Controlled interop | RSTorrent/RSTorrent and both mixed directions, one connection, forced uTP, exact hashes, more than two cycles, clean 160 ms and relevant impairment profiles |
| Reliability WAN | Alternating 256 MiB affected cells, zero unexplained reconnect/retry/drop/fallback, exact integrity and cleanup |
| Throughput WAN | Three rotating 256 MiB repetitions in both directions and remote-seed 1 GiB repetitions; every RSTorrent pairing median at least `0.85x` matched libtorrent/libtorrent uTP |
| Fairness | Existing queue-delay, bufferbloat, TCP-like competitor share/yield, recovery, loss, timeout, receive-pressure, and dynamic-MTU thresholds do not regress |
| Platform/repository | Both Android native builds; formatting; warning-denying workspace Clippy; workspace tests; relevant Python contracts and controlled profiles |

TCP controls must remain within their retained path range or carry a concrete
resource/path explanation. Libtorrent/libtorrent uTP must demonstrate a usable
reference path in each cohort. One-off ratios are observations; the near-
parity claim uses three valid alternating repetitions and reports the median
and range.

This tactical completes only when sustained transfers remain on one
connection, exact integrity and cleanup pass, every RSTorrent-containing
pairing meets the `0.85x` median target, and all fairness/resource/platform
gates pass. If a stable path cannot supply a valid oracle cohort, close only
with typed environmental evidence and no parity claim.

## Human Review Gate

Implementation may proceed autonomously through terminal attribution, causal
reliability repair, existing-owner throughput repairs, controlled/WAN reruns,
and documentation. If one reliable connection remains below `0.85x` and
diagnostic A/B evidence selects slow start, a different gain, `TARGET`,
`ALLOWED_INCREASE`, loss response, or another congestion-policy change, stop
for human review with:

- at least three alternating controlled samples for current and candidate
  behavior;
- window/flight, delay, loss, completion, and resource distributions;
- existing TCP-like competitor yield and recovery results;
- the exact libtorrent source/settings difference being considered; and
- a recommendation distinguishing startup-only benefit from steady-state
  gain.

Approval of the tactical is not approval of that later production policy
change. Diagnostic-only bounded variants may be used to make the decision
evidence complete.

## Non-Goals And Next Boundary

This tactical does not fix the separate remote-placement RSTorrent TCP seed
disconnect, optimize `pimom` storage or ISP service, finish low-value matrix
cells merely for cardinality, add a dependency, copy libtorrent, add IPv6 or
MSE-over-uTP, change NAT traversal, expose a product setting, add UI, run a
public swarm, or broaden the existing **Partial** uTP support claim without
the required evidence.

If the first causal failure is outside uTP transport and ordered peer-stream
ownership, record it and stop before expanding scope. After near parity, any
broader controller modernization, multi-flow fairness campaign, IPv6/MSE
composition, or support-claim graduation requires its own decision.
