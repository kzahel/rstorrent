# Tactical 145: Sustained uTP Reliability And Throughput Near-Parity

Status: **Paused at the production congestion-policy human-review gate under
parent Tactical 142.** Terminal provenance, repeated-cycle gates, release recovery,
packetization and receive-position repairs, off-device WAN builds, and the
diagnostic startup A/B are implemented. Maintainer direction on 2026-08-13
selected this tactical after Tactical `143` completed and activated continued
uTP performance work with the goal of approaching pinned-libtorrent uTP
throughput. Later explicit direction the same day superseded it with the iOS
client campaign beginning at Tactical `147`; this checkpoint remains the exact
restart point. Ordinary product construction still uses the existing linear
LEDBAT controller; no slow-start policy has been enabled.

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

## Implementation Evidence

### Stage 1: terminal provenance

The uTP worker now retains one bounded, endpoint-free last-failure record for
RESET, retry exhaustion, protocol failure, runtime I/O failure, or worker
panic. It includes newly sent and received DATA counts, sent and received
16-bit sequence-cycle counts, last DATA sequence values, duplicate/stale/
future/ambiguous ACK counts, duplicate/too-far-ahead/ambiguous receive counts,
and FIN/RESET counts. Protocol detail is UTF-8 bounded to 256 bytes; runtime
I/O, panic, payload, and endpoint detail is deliberately withheld.

`rstorrent-public-probe`, `rstorrent-incoming-seed`, and the controlled uTP
role serialize the same optional `last_failure` object. The download owner
also retains the bounded exact content-peer task error separately from its
coarse reconnect policy classification. Thus a future WAN reconnect can be
placed at peer framing/I/O or inside uTP and, for uTP, relative to sequence
reuse without retaining a packet timeline.

Before the first post-instrumentation WAN reproduction, retry evidence was
sharpened to retain the exact exhausted sequence number and actual/maximum
attempt count. The failed worker also retains retransmission DATA count and
last sequence, received loss-signal count, final outstanding/in-flight/
pending-retransmission ownership, congestion and remote windows, smoothed RTT,
effective RTO, consecutive timeouts, loss reductions, and timeout collapses.
These remain bounded endpoint-free scalars and are captured before terminal
abort clears protocol ownership.

Focused validation on 2026-08-13 passes 22 uTP runtime tests, the driver-control
tests, both affected binary build surfaces, and 24 WAN/matrix Python contract
tests. The next executable action is the pure repeated-cycle transfer; no
transport or congestion behavior changed in this stage.

### Stage 2: clean repeated-cycle gate

An independently authored deterministic transport regression carries 131,075
one-byte DATA packets through one fixed-548 connection initialized at sequence
`65533`. It crosses `65535 -> 0` three times, applies ordinary delayed ACKs and
periodic duplicate ACKs, verifies every byte exactly once, drains send,
in-flight, retransmission, receive, reorder, and unsent ownership to zero, then
completes bidirectional FIN and final-ACK teardown. The receiver ends at exact
acknowledgement number zero after reuse.

The focused optimized test completes in 0.06 seconds. The clean common path
therefore does not fail across repeated 16-bit sequence reuse, so no sequence
arithmetic or ledger repair is accepted from the original hypothesis. The
remaining Stage 2 impairment cases and the composed runtime reproduction will
instead test whether loss/retransmission or an async ownership boundary fails
during a sustained stream.

### Stage 3: release-only recovery defect and causal repair

The first post-instrumentation WAN reproduction ran the historically affected
256 MiB remote libtorrent-seed/RSTorrent-leecher forced-uTP cell at exact clean
revision `bcbf7a898cec40cd2c89661f8bf79ca99792038d`. The ordinary-public-route
transfer verified all 268,435,456 bytes and 1,024 pieces at 1.639029 MiB/s, but
required two connections and retained one exact retry exhaustion. The failed
worker received 94,277 DATA datagrams across two sequence cycles and sent
5,227 new DATA datagrams before sequence `16567` exhausted eight of eight
attempts. It emitted exactly seven retransmissions of that sequence after only
one received loss signal and zero timeout collapses. Its terminal state still
owned the packet plus 24 others; ingress had zero drops. This fingerprint
rejects ordinary repeated loss and selects retransmission-work lifecycle.

Inspection found the queue-removal mutation inside
`debug_assert!(complete_front(...))`. Debug profiles executed the mutation;
release profiles compiled away the complete call and retained the same work
item. The existing isolated-timeout transport regression demonstrably failed
under `cargo test --release` with one pending retransmission before the repair.
Queue completion is now unconditional and only its Boolean invariant remains
debug-asserted. That exact release test and the three-wrap release test pass
afterward; the complete protocol suite and warning-denying protocol Clippy also
pass. The only other uTP `debug_assert!` has its map insertion outside the
assertion, so this release-semantics audit found no sibling defect.

The WAN harness recorded successful finite UDP mapping deletion, joined
processes, exact payload cleanup, and no remote run directory or matching role
process after the case.

The same 256 MiB mixed-direction cell at repaired revision
`03c2f5b6e5541fcb1e0224454da67d6c06c989fd` completes on one connection with
zero retry exhaustion, last failure, peer failure, TCP peers, ingress drops,
or unknown-connection datagrams. All 268,435,456 bytes and 1,024 pieces verify;
the libtorrent seed emits 185,178 payload packets, proving more than two real
sequence cycles, while RSTorrent handles nine ordinary retransmissions without
churn. Active rate rises from the pre-repair 1.639029 to 2.093211 MiB/s, a
27.7% improvement. RSTorrent RSS reaches 13.8 MiB, sender/receive ownership
high waters remain within their existing bounds, and terminal UDP/uTP task and
queue ownership is zero. A fresh cleanup audit again finds no mapping-owned
run directory or role process; the local ignored journal and fixture are
removed after these aggregates are recorded.

This one sample passes the first causal repair gate, not the alternating
reliability cohort or throughput target. Fixed-profile and RSTorrent-sender
composition plus repeated 64/256 MiB cells remain required before throughput
attribution.

Two explicit high-cost real-socket runtime regressions now stream deterministic
bytes rather than retaining the payload. The fixed-548 case transfers
69,210,112 bytes and the dynamic-IPv4 case transfers 190,320,640 bytes; each
forces the sending service beyond 131,072 DATA datagrams on one connection,
verifies every byte in order, performs bidirectional shutdown, and asserts zero
terminal uTP/UDP tasks, queues, waiters, retry exhaustion, panic, or failure.
The dynamic case additionally proves an acknowledged protected MTU probe and a
selected datagram size of at least 1,456 bytes. Both pass optimized debug and
`--release` profiles in 1.16--1.41 seconds of transfer time on the development
host. They are ignored from routine runs because together they move roughly
248 MiB over loopback, but remain named release acceptance gates.

The extended controlled role matrix then runs all four forced-uTP pairings at
256 MiB with one direct loopback peer. Every case verifies 268,435,456 bytes
and 1,024 pieces, joins both roles, removes the output, and records one
connection with no retry exhaustion, last failure, ingress drop, or worker
panic for each RSTorrent role. RSTorrent seeding emits 196,696--199,015 DATA
datagrams, so both RSTorrent-sender pairings cross more than two sequence
cycles with dynamic MTU. Active rates are 166.991 MiB/s RSTorrent/RSTorrent,
137.999 MiB/s RSTorrent/libtorrent, 86.507 MiB/s libtorrent/RSTorrent, and
176.691 MiB/s libtorrent/libtorrent. Thus local RSTorrent/RSTorrent reaches
94.5% of the oracle control after the reliability repair, while the two mixed
directions remain useful throughput-attribution targets rather than failures.
All four cases and fixture cleanup finish in 12.21 seconds.

The matching 256 MiB TCP controls also pass all four one-connection integrity
and cleanup cases at 425.629, 333.542, 381.406, and 258.280 MiB/s in the same
pairing order. Loopback is CPU/syscall sensitive and is not a WAN parity
target, but the result confirms that neither RSTorrent's torrent scheduler nor
storage path imposes the mixed-uTP ceilings. The uTP-specific local ratios are
39.2%, 41.4%, 22.7%, and 68.4% of each pairing's own TCP result; controller,
datagram, ACK, and runtime-turn utilization therefore remain the selected
owners after reliability.

### Stage 4: blocked fast recovery and WAN verification

The first one-repetition remote-seed 256 MiB cohort at exact revision
`bafe3c3` established the pre-fast-recovery comparison:

| Seed -> leecher | Active MiB/s | RSTorrent connections | Oracle ratio |
| --- | ---: | ---: | ---: |
| libtorrent -> libtorrent | 2.741 | n/a | 100.0% |
| libtorrent -> RSTorrent | 2.705 | 1 | 98.7% |
| RSTorrent -> libtorrent | 2.140 | 1 | 78.1% |
| RSTorrent -> RSTorrent | 1.154 | 2 | 42.1% |

Every cell verified the exact payload and cleanup. The two mixed directions
were stable on one connection; only RSTorrent/RSTorrent reconnected, making
its retained terminal state the causal next sample rather than treating the
four rates as a parity cohort.

The first exact RSTorrent/RSTorrent 256 MiB WAN reproduction after terminal
provenance completed only after a peer reconnect. The leecher rejected 751
later DATA packets outside its unchanged 64-packet reorder allowance, while
the seed retained 752 outstanding packets and one pending fast
retransmission. One SACK loss signal had halved the congestion window below
the already admitted later flight, and ordinary window admission prevented
the missing packet from being emitted. The peer owner consequently timed out
after 15 seconds and restarted the connection. Ingress remained within its
existing bounds and neither endpoint reported retry exhaustion, protocol
error, or runtime I/O error.

Pinned libtorrent's `src/utp_stream.cpp::resend_packet` bypasses window
admission for a SACK-selected fast retransmission while retaining the window
check for timer retransmission. RSTorrent now carries the same distinction as
bounded retransmission-work provenance: ACK/SACK loss and message-too-large
recovery are fast, timeout recovery is ordinary, and an already queued item
can only upgrade to fast. Fast recovery may emit the missing already-admitted
packet despite the reduced window; new DATA and timeout retransmission remain
unchanged. An independently authored nine-packet regression proves the
missing packet can be emitted after loss reduction leaves the later flight
above the new window. `TARGET`, `GAIN`, `ALLOWED_INCREASE`, loss reduction,
resource bounds, and new-DATA admission do not change.

At exact revision `8c7e985dcd5cf8124b0b8a8a93b0cb54995df3a7`, the same
ordinary-Internet RSTorrent/RSTorrent 256 MiB case then verifies every byte
and all 1,024 pieces on one connection. The seed emits 766 retransmission
datagrams after one loss reduction with zero retry exhaustion; the leecher
records zero peer failure or content error. Active rate improves from 1.154
to 1.504 MiB/s, a 30.4% increase. The matrix summary's `stable: false` for
this cell means only that fewer than three repetitions exist; it is not a
transport-instability observation. Repeated alternating reliability samples
remain required.

### Off-device ARM64 WAN builder

Remote preparation no longer runs Cargo or rustc on `pimom`. The harness uses
the private-inventory-backed `machine-control --target linux` route to start
and inspect the existing Ubuntu 24.04 UTM guest, then stages an exact
`git archive` into `/opt/rstorrent-builder`. The guest is native `aarch64`,
uses glibc 2.39, has exact Rust/Cargo 1.97.0, builds with four jobs, and keeps
only Cargo registry and target caches between revisions. Per-revision source,
compressed outputs, and host staging are removed after each build.

The development host independently requires a Linux ARM64 ELF and computes
its SHA-256 and size. The Pi accepts the binary only when its glibc is at least
the builder's, the uploaded size and digest match, `file` reports ARM64 ELF,
`ldd` finds every dependency, and an atomic mode-0755 install succeeds. A
revision-bound manifest records those digests plus minimized builder/runtime
facts. The staged-revision fence is published last, after artifacts and exact
fixtures succeed; a partial preparation cannot be resumed.

The cold proof built both WAN roles in about two minutes. A later full
same-revision VM-to-Pi preparation at exact revision
`e78486713fe7b6a87a5d374868c45fb0dc7aa785` completed the cached builder and
artifact staging phase in 87.880 seconds, verified both installed hashes, and
left no VM revision stage, Pi upload stage, Cargo process, or rustc process.
The retained VM caches occupy 486 MiB of target data and 271 MiB of registry
data. The Pi is now an execution and fixture endpoint only.

An execution smoke at exact revision
`7da0902e6dc938ffa849224d4ae93265a7140ac1` then uses that path to build and
stage only `rstorrent-incoming-seed` in 67.834 seconds. The VM-built artifact
serves an exact 8 MiB forced-uTP RSTorrent/RSTorrent WAN transfer on one
connection at 0.468380 MiB/s with zero retry, peer error, or content error.
All 32 pieces verify, the UDP mapping is removed, role processes join, and the
remote run plus VM revision stage are absent afterward. This is an execution
and cleanup proof for artifact staging, not a throughput baseline.

### Stage 5: repeated reliability and packetization attribution

A rotating three-repetition remote-seed 256 MiB forced-uTP cohort at exact
revision `c66583540bc413293fa2073bd05b4dd9bd0fd0f5` completes all 12 cases with
exact bytes and pieces, successful mapping/process/artifact cleanup, and no
invalid or failed records:

| Seed -> leecher | Min MiB/s | Median MiB/s | Max MiB/s | Oracle median ratio |
| --- | ---: | ---: | ---: | ---: |
| libtorrent -> libtorrent | 2.728 | 2.741 | 2.742 | 100.0% |
| libtorrent -> RSTorrent | 2.665 | 2.701 | 2.703 | 98.5% |
| RSTorrent -> libtorrent | 1.793 | 2.130 | 2.144 | 77.7% |
| RSTorrent -> RSTorrent | 1.505 | 1.519 | 1.539 | 55.4% |

Every RSTorrent role uses one connection with zero retry exhaustion, peer
failure, or content error. RSTorrent/RSTorrent's 2.27% max/min spread closes
the affected remote-seed 256 MiB reliability gate and turns the remaining gap
into utilization attribution rather than connection-lifecycle diagnosis.

RSTorrent seeding to libtorrent sends 220,461--224,277 DATA datagrams and
273.06--273.32 million datagram bytes, handles 39,155--43,619 congestion ACK
events, and reaches 647--659 KiB of flight/window. Its first two samples need
only one or two retransmissions; the slower third needs 147 retransmissions
and three timeout collapses but still remains on one connection. The same
seed to RSTorrent sends 318,008--322,143 DATA datagrams for nearly the same
275.61--275.71 million wire bytes, handles 159,514--160,223 congestion ACK
events, and reaches the same 647--649 KiB flight/window. Each sample has
750--778 retransmissions, 614--638 timeout collapses, and a leecher high
water of 746--776 DATA datagrams rejected beyond the unchanged 64-packet
reorder allowance.

The composition therefore turns the same 256 MiB stream into roughly 45%
more DATA datagrams and four times the ACK work before CPU, storage, path MTU,
or admitted-byte window diverge. The exact pinned-libtorrent `send_pkt` owner
waits when the desired DATA payload does not fit the remaining congestion and
remote window; RSTorrent's `new_payload_bytes` instead shrinks ordinary DATA
to every residual window sliver. This selects a deterministic packetization
A/B before any controller change; it does not establish that libtorrent's
full-payload rule composes unchanged with RSTorrent's deliberate
no-slow-start controller.

### Stage 6: bounded residual-fragment repair

An independently authored transport regression first proves that a queued
full segment is no longer reduced to a seven-byte residual-window fragment,
that the worker does not advertise immediate work while that fragment is
withheld, and that a subsequent ACK releases one full segment. Legitimate
short queue tails and a zero-flight progress escape for a genuinely tiny
remote window remain admitted.

The first full-payload-or-wait implementation matched pinned libtorrent but
failed retained RSTorrent controller gates. Clean 160 ms simulation completion
rose to 19.885 seconds beyond the 19-second ceiling, and a TCP-like competitor
received only 58.26% of overlap traffic against the 70% minimum. Production
therefore suppresses only a residual fragment smaller than half its intended
payload. This deliberate difference preserves enough ACK feedback for the
no-slow-start controller: the same simulations complete in 18.316 seconds and
give the competitor 70.37% overlap share. All 214 routine protocol tests and
warning-denying protocol Clippy pass without changing `TARGET`, `GAIN`,
`ALLOWED_INCREASE`, loss response, or any byte/packet bound.

The 64 MiB product RSTorrent/RSTorrent gate over the controlled 160 ms relay
then completes all 256 pieces at 1.206339 MiB/s on one connection per role.
The seed emits 47,076 DATA datagrams against a 46,701 full-1,437-byte-payload
lower bound, selects a 1,457-byte MTU, has zero retransmission or datagram
drop, and reaches 66 of 256 ingress datagrams plus 435,379 in-flight bytes.
Both roles and the relay terminate with zero queue ownership and the output is
removed.

A three-repetition exact WAN RSTorrent/RSTorrent 256 MiB cohort at revision
`b6c69cd0f9d207ce1eeb3305f00d360068f90b1f` is likewise exact and clean:

| Metric | Pre-repair median | Post-repair median | Change |
| --- | ---: | ---: | ---: |
| Active MiB/s | 1.518572 | 1.563165 | +2.94% |
| DATA datagrams | 318,008 | 188,149 | -40.84% |
| Congestion ACK events | 159,731 | 96,269 | -39.73% |
| Retransmissions | 754 | 399 | -47.08% |
| Timeout collapses | 619 | 326 | -47.33% |
| Too-far-ahead DATA | 751 | 398 | -47.00% |

The three rates span only 1.562916--1.564864 MiB/s and every case stays on one
connection with zero retry exhaustion, peer/content error, fallback, or
cleanup failure. The seed still fills 461 packets and 663,094 bytes in flight
at the median while the receiver retains only 97,716--124,888 bytes at high
water and advertises at least 925,125 bytes of unused credit. One loss
reduction nevertheless leaves 397--400 later DATA datagrams beyond the fixed
64-packet receive-reorder distance.

Pinned libtorrent independently bounds this same owner from its 1 MiB receive
capacity, allowing `max(16, capacity / 1100) = 953` packet positions before a
too-far drop. RSTorrent advertises the same 1 MiB byte window but only 64
positions. A resource-accounted receive-reorder bound repair, with packet and
byte high-water telemetry and unchanged 1 MiB payload credit, is now the next
executable action.

### Stage 7: receive-position repair

RSTorrent now derives 953 reorder positions from the unchanged 1 MiB receive
credit at one position per 1,100 bytes, matching the exact pinned-libtorrent
owner. The 954th position is the first typed too-far disposition. The furthest
admissible position produces a bounded 120-byte SACK, remains within the
existing 252-byte hostile-wire limit, and rejects the next position without
state mutation. The existing 1 MiB shared delivered/reorder byte limit is
unchanged.

The aggregate 64-connection budget therefore remains 64 MiB of receive
payload. A conservative 256-byte metadata allowance for each of the 60,992
positions adds at most 14.891 MiB, while fixed SACK arrays add 7,680 bytes.
Runtime evidence now exposes separate reorder-packet and total-buffered-byte
high waters. All 215 routine protocol tests, 23 routine uTP runtime tests,
warning-denying protocol/engine/session Clippy, and 26 Python evidence
contracts pass.

The post-repair controlled 64 MiB product transfer over the 160 ms relay stays
on one connection per role and completes at 1.181999 MiB/s with 47,082 DATA
datagrams, zero retransmission/drop, exact integrity, and zero cleanup
ownership. It selects a 1,457-byte MTU, and the new receive fields report zero
reordered packets plus 107,775 buffered bytes on the leecher's clean path.

The exact three-repetition WAN RSTorrent/RSTorrent 256 MiB cohort at revision
`dba267731d9bf061fdf7170e9901b5b1010beead` then produces the causal result:

| Metric | Before position repair | After position repair | Change |
| --- | ---: | ---: | ---: |
| Active MiB/s | 1.563165 | 2.139183 | +36.85% |
| DATA datagrams | 188,149 | 187,622 | -0.28% |
| Congestion ACK events | 96,269 | 95,857 | -0.43% |
| Retransmissions | 399 | 1 | -99.75% |
| Timeout collapses | 326 | 0 | -100% |
| Too-far-ahead DATA | 398 | 0 | -100% |

Rates span 2.136027--2.139756 MiB/s. Every case verifies all 256 MiB and
1,024 pieces on one connection with zero retry exhaustion, peer/content
error, fallback, or cleanup failure. Reorder high water is 462--464 packets
and 664,614--668,205 bytes, proving that the previous 64-position limit—not
payload credit—caused the recovery cascade. The seed emits only one, one, and
two retransmissions; two samples have zero timeout collapse and the third has
one without a rate penalty. Ingress reaches 43--59 of 256 datagrams, and the
VM/Pi audit finds no compiler, role process, mapping run, or revision stage.

The repaired RSTorrent/RSTorrent median is 78.0% of the retained 2.741167
MiB/s libtorrent/libtorrent control and effectively matches the pre-repair
2.129650 MiB/s RSTorrent/libtorrent median. Receiver composition is therefore
closed as the primary owner. Residual sender utilization and startup
attribution, including a bounded diagnostic-only controller A/B if ordinary
telemetry cannot explain the gap, is therefore the remaining review-gated
owner.

### Stage 8: sender-startup attribution and policy A/B

The first post-position-repair WAN sample supplies a clean sender diagnosis.
Its 256 MiB transfer is active for 119.849 seconds. Payload rate grows from
about 64 KiB/s at second 3 to 432 KiB/s at second 10, 979 KiB/s at second 20,
roughly 1.48 MiB/s at second 30, and about 3 MiB/s only near second 90. The
sender retains a 1 MiB unsent high water, records zero sender-underfilled and
zero remote-window-limited congestion acknowledgements, and is congestion
limited on 95,678 of 95,857 feedback events. It reaches about 663 KiB of
flight with 156--266 ms RTT and 0--80.889 ms queue delay. One loss near second
99 temporarily lowers rate, but late instantaneous rate returns to the path
ceiling. Storage, application feed, remote credit, packetization, receive
distance, and steady-state path capacity are therefore rejected as the first
owner; linear startup is the remaining causal owner.

Pinned libtorrent starts every connection with `m_slow_start = true` in
`src/utp_stream.cpp::utp_socket_impl`. In `do_ledbat`, a congestion-limited
ACK grows the window by at least the acknowledged bytes until delay reaches
the configured target or a remembered slow-start threshold would be crossed.
`experienced_loss` exits slow start and records the reduced window as the
threshold; `tick` re-enters it after a retransmission timeout. RSTorrent
instead starts at two MSS and applies only linear RFC 6817 gain, capped by its
existing allowed-increase rule.

Commit `aec9235` adds a narrow test-only startup injection. Product
`TransportState::initiate` and accepted inbound connections still select
`LinearLedbat`; the alternative cannot be constructed in a non-test build.
The comparator reports completion, congestion/flight, delay, loss, retry,
MTU, fairness/recovery, and bounded send/link high waters without sockets,
tasks, payload retention, or a new runtime owner.

The direct libtorrent-style rule under RSTorrent's existing bounds is not the
recommendation. On the clean 80 ms one-way, 3,000,000-byte/s, 8 MiB profile it
reduces completion from 18.3815 to 4.4540 seconds, but reaches the complete
1 MiB window/flight and produces 193.750 ms p95 and 194.000 ms maximum queue
delay, above the retained 150 ms gate. Exploratory early-exit variants also
fail the TCP-like foreground-share gate: 50 ms without a window clamp gives
the competitor 43.60%, 10 ms without a clamp 53.78%, a 50% exit window
68.67%, and a 40% exit window 69.57%, each below the required 70%.

The selected diagnostic candidate uses exponential acknowledged-byte growth
only during startup, exits on the first 10 ms queue-delay signal, and retains
30% of the pre-exit window. Its post-exit `TARGET = 100 ms`, linear `GAIN = 1`,
allowed-increase rule, loss reduction, pacing, and every packet/byte bound are
unchanged. Three alternating current/candidate 8 MiB comparisons vary
one-way base delay across 70, 80, and 90 ms on the same 3,000,000-byte/s link:

| One-way delay | Current completion / MiB/s | Candidate completion / MiB/s | Current / candidate maximum cwnd | Current / candidate maximum flight |
| ---: | ---: | ---: | ---: | ---: |
| 70 ms | 16.178750 s / 0.494476 | 8.549500 s / 0.935727 | 153,651 / 400,089 B | 153,034 / 399,014 B |
| 80 ms | 18.381500 s / 0.435220 | 9.753750 s / 0.820197 | 153,585 / 400,089 B | 153,049 / 399,014 B |
| 90 ms | 20.728500 s / 0.385942 | 10.908750 s / 0.733356 | 153,656 / 400,089 B | 153,471 / 399,014 B |

The candidate is 1.88x--1.90x faster across the paired profiles. Every sample
has exact integrity, zero retransmission, loss, timeout, queue drop, or MTU
drop. Current p95/maximum queue delay is 0.500/0.500 ms; candidate p95 is
1.000--1.500 ms and maximum is 45.000 ms. Current send-ledger high water is
107--108 packets and 153,034--153,471 bytes; candidate is 284 packets and
399,014 bytes. Link event high water grows from 92--98 events and
113,205--130,565 bytes to 275 events and 379,422 bytes, within the unchanged
131,072-event/8 MiB fixture bounds and the transport's 1,024-packet/1 MiB
send limits.

The existing TCP-like competitor profile also passes. Foreground overlap
share improves from 70.367% current to 82.647% candidate; p95 queue delay is
126.942 versus 131.944 ms. uTP recovery takes 230.000 versus 293.000 ms and
remains far below ten measured RTTs. Both modes make two loss reductions;
queue drops are 12 versus 13 and aggregate directional queue high water stays
below 75 KiB plus one acknowledgement datagram.

Candidate-specific impairment evidence transfers the exact stream through 47
scripted noncongestive drops in 8.699750 seconds with zero queue drop, 21
retransmissions/loss reductions, zero timeout collapse, and at most two sends
of a sequence. Its MTU black-hole profile completes in 8.619000 seconds,
isolates three failed probes from congestion reduction, converges to a
1,269-byte datagram floor, and records six probes, three retransmissions, zero
loss reduction, and four ignored loss events. Controller unit tests prove
loss exit and threshold-bounded timeout restart.

This evidence selects a startup-only policy change, not a larger steady-state
gain. Recommendation A is to promote the bounded 10 ms/30% startup behavior,
then run the controlled product and WAN cohort before making any parity claim.
The exact libtorrent rule is rejected, and changing steady-state gain,
`TARGET`, ordinary allowed increase, or loss multiplication is not
recommended. Production implementation now stops at the required human
review gate.

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
  unsent bytes, 1 MiB receive bytes, 953 reordered packets, one pending
  emission, one MTU probe, and eight transmission attempts per packet. The
  reorder position count is derived from the unchanged receive credit at one
  position per 1,100 bytes. Across all 64 uTP connections, payload ownership
  therefore remains 64 MiB; a conservative 256-byte metadata budget per
  position adds at most 14.891 MiB, and fixed SACK storage adds 7,680 bytes.
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
- Remote RSTorrent artifacts are built only by the guarded ARM64 Linux VM.
  Builder glibc must not exceed the Pi runtime, exact Rust 1.97.0 and a clean
  committed archive are required, a single flock-guarded four-job build owns
  the persistent cache, and size/SHA-256/ELF/`ldd` checks precede atomic
  installation. The WAN peer does not run Cargo or rustc.

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
