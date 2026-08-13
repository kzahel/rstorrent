# Tactical 150: Bounded uTP Sender Startup

Status: **Complete on 2026-08-13 at the maintainer-selected bounded evidence
stop.** Recommendation A from Tactical `145` is production behavior, all
preserved controlled/platform/repository gates pass, and the complete
three-repetition remote-seed 256 MiB cohort proves near parity. A bounded
one-to-two-sample 1 GiB follow-up corroborates scaling but is not presented as
a stable cohort. The constrained `pimom` endpoint remained execution-only;
exact ARM64 Linux artifacts were built in the guarded `machine-control` UTM
guest.

Topics: `utp-transport-campaign`, `performance-and-live-evidence`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed parent Tactical
[`145`](145-sustained-utp-reliability-and-throughput-near-parity.md), parent
lab Tactical [`142`](142-wan-transport-performance-matrix.md), and completed
Tacticals [`121`](121-deterministic-utp-loss-congestion-and-mtu.md),
[`125`](125-shared-udp-utp-runtime-and-loopback-interop.md), and
[`144`](144-long-rtt-utp-sender-window-utilization.md).

## Decision And Desired Outcome

RSTorrent's reliable 256 MiB remote-seed RSTorrent/RSTorrent median is
2.139183 MiB/s, 78.0% of the retained 2.741167 MiB/s
libtorrent/libtorrent forced-uTP control. It now matches the earlier
RSTorrent-to-libtorrent sender result, and WAN telemetry shows a continuously
fed, remote-window-unlimited, congestion-limited sender taking roughly 90
seconds to reach path-rate flight. Sender startup—not receive composition,
storage, application feed, or steady-state path capacity—is causal.

Enable exponential acknowledged-byte congestion-window growth only during
startup. Exit on the first 10,000-microsecond queue-delay signal, immediately
retain 30% of the pre-exit window, then return to the existing linear RFC 6817
controller. Loss exits startup after the ordinary window reduction and
records the reduced window as the threshold for a later timeout restart.
Timeout collapse may re-enter startup but must leave it before exceeding that
threshold. Application-limited ACKs do not grow the window.

This is intentionally more conservative than pinned libtorrent's direct
target-delay exit. Tactical `145` rejected that exact diagnostic behavior at
193.750 ms p95 queue delay against the retained 150 ms gate. The accepted
10 ms/30% candidate improved three controlled long-RTT comparisons
1.88x--1.90x, gave the TCP-like foreground flow 82.65% overlap share, and
passed recovery, loss, MTU-isolation, integrity, and resource gates.

The primary empirical outcome remains Tactical `145`'s median active payload
rate of at least `0.85x` the alternating same-direction libtorrent/libtorrent
uTP control for every RSTorrent-containing pairing. The stable claim is made
from the complete three-repetition 256 MiB cohort. The later 1 GiB samples are
bounded corroboration rather than a second stable median because maintainer
review stopped bulk execution once its marginal diagnostic value flattened.

## Normative And Source Oracle

RFC 6817 Sections 2.2--2.5 and 3.2 remain normative for delay measurement,
congestion response, yielding, and permitted slow-start behavior. Managed
BEP 29 remains the uTP wire reference. The exact pinned libtorrent `2.0.13`
revision `7d7fc38fac61177fa5e02148f791b2f65250b09d` supplies the completeness
oracle:

- `reference/libtorrent/src/utp_stream.cpp::utp_socket_impl` initializes
  `m_slow_start` for a new connection;
- `do_ledbat` separates application-limited ACKs, exponential startup,
  target-delay exit, remembered threshold exit, and linear steady state;
- `experienced_loss` exits startup after reducing the window and records the
  reduced threshold;
- `tick` collapses the timeout window and re-enters slow start; and
- `consume_incoming_data` drops DATA that overshoots the advertised receive
  capacity without terminating the connection while the already parsed ACK
  path remains valid; and
- `reference/libtorrent/simulation/test_utp.cpp::{utp_plain,
  utp_buffer_bloat,utp_straw,utp_small_kernel_send_buf}` plus
  `reference/libtorrent/test/test_utp.cpp::utp` supply clean, queue, competing
  flow, constrained send-buffer, and forced-uTP expectations.

The adopted state transitions are independently authored. Intentional
differences are the 10 ms startup exit, immediate 30% retained window,
RSTorrent's existing two-MSS ordinary floor, unchanged `TARGET = 100 ms`,
linear `GAIN = 1`, allowed-increase rule, pacing, and loss multiplier. The
local JSTorrent reference has no uTP controller and adds no startup behavior.

## Owner, Task, Cancellation, And Dependency Map

```text
runtime-independent congestion state
  startup active/exit/threshold + linear steady state + bounded scalars
        |
        v
runtime-independent transport
  new and accepted connections select the one production startup policy
        |
        v
uTP runtime worker and service snapshot
  aggregate startup ACK/exit/active high-water evidence; no new task
        |
        v
session/application roles and WAN harness
  existing cancellation, mapping, revision, integrity, and cleanup owners
```

No task, socket, channel, timer, dependency, or product setting is added. The
transport worker remains the sole mutable owner of one congestion controller.
The shared UDP service retains existing cancellation and joined termination.
Diagnostic current/candidate selection remains test-only; every non-test
transport construction uses the selected production policy.

Dependency direction remains protocol state -> transport -> runtime ->
session/application evidence. Runtime types do not enter congestion state.

## Scope And Implementation Stages

1. **Production state.** Replace the diagnostic-only arbitrary startup shape
   with named production constants and one explicit bounded-startup mode.
   Ordinary initiating and accepting transports select it. Retain a
   test-only linear constructor for paired comparisons.
2. **Deterministic edge cases.** Prove exact exponential growth,
   application-limited suppression, 10 ms queue exit, 30% retained window,
   loss exit, timeout restart, remembered threshold, MTU update, time
   reversal, saturation, and unchanged floors/bounds.
3. **Transport and runtime evidence.** Carry startup counters and active/high-
   water state through existing snapshots and public controlled-role JSON.
   Add no endpoint, payload, packet trace, or unbounded history.
4. **Controlled product gates.** Rerun clean long RTT, bounded queue,
   TCP-like fairness/recovery, fixed loss, timeout, receive pressure, dynamic
   MTU, sustained wrap, real-socket runtime, and the 160 ms product transfer.
5. **WAN cohort.** Build the exact clean committed revision in the guarded
   ARM64 Linux VM, stage verified artifacts to `pimom`, and run three rotating
   remote-seed 256 MiB forced-uTP repetitions for all four implementation
   pairings with matched TCP controls. If stable and informative, run the
   remote-seed 1 GiB cohort and one bounded local-seed scaling smoke required
   by Tactical `145`.
6. **Platform and closure.** Run both Android native ABI builds, formatting,
   warning-denying workspace Clippy, workspace tests, relevant Python
   contracts, reconcile Tactical `142`, Tactical `145`, living topics, and
   the protocol claim, then remove owned temporary artifacts.

## Execution Record

### Production state and deterministic gates

Commit `9522e64` promotes the selected policy to all initiating and accepted
uTP transports. The controller now exposes its active state, remembered
threshold, startup acknowledgements, and exits through bounded snapshots;
the existing runtime high-water owner carries those scalars through the
controlled-role and WAN JSON without adding a task, queue, or history.

The complete 245-test protocol library has 241 routine passes and four
intentional opt-in tests. Focused engine product-runtime evidence passes, as
do warning-denying protocol/engine/session all-target Clippy and all 16 WAN
contract tests. The paired opt-in controller evidence reproduces the approved
three long-RTT improvements, 1.88x--1.90x, with candidate p95 queue delay of
1.0--1.5 ms and a 45 ms maximum. TCP-like foreground overlap share is 82.65%,
recovery is 293 ms, the fixed-loss case has 21 loss reductions with no
timeout, and MTU probe loss remains isolated. The exact libtorrent-like exit
again reaches 193.75 ms p95 queue delay and remains rejected.

### Controlled product evidence

The mixed libtorrent/RSTorrent forced-uTP loopback transfers complete exact in
both directions with one uTP peer and zero TCP peers. The RSTorrent seed
records 22 startup acknowledgements; the leecher correctly records no sender
growth for its acknowledgement-only role.

The retained production-owner 64 MiB RSTorrent/RSTorrent gate over the clean
160 ms relay completes in 27.509258 seconds at 2.326489 MiB/s. This is a 96.8%
increase over Tactical `145`'s post-reorder 1.181999 MiB/s controlled result.
Both roles use one connection; the seed emits 47,039 DATA datagrams with zero
retransmission, loss reduction, or timeout collapse. Startup has 404
congestion-limited acknowledgements and one exit, records a 524,288-byte
threshold, observes 21.036 ms maximum queue delay, and reaches the unchanged
1 MiB congestion/flight bound. The leecher reaches 128 of 256 queued
datagrams, zero reordered packets, and 125,019 buffered bytes. The relay has
zero drop, 694 queued datagrams and 944,361 queued bytes at high water, then
drains completely; exact payload verification and all process cleanup pass.

Raw controlled reports were retained during execution at
`/tmp/rstorrent-t150-mixed.json` and
`/tmp/rstorrent-t150-controlled.json`. They are temporary evidence inputs,
not repository artifacts, and were removed after reconciliation.

### First WAN attempt and receive-window repair

The first exact-revision WAN attempt at `30ee1d0` was stopped after its first
uTP cell exposed invalid evidence. Remote libtorrent-to-local RSTorrent
completed 256 MiB at 1.195967 MiB/s only by dialing three times. RSTorrent
terminated twice when one DATA datagram raised buffered receive payload from
within the advertised 1 MiB window to 1,049,073 bytes, 497 bytes above the
bound. The adjacent TCP control completed at 2.712944 MiB/s. The partial run
is retained only as causal defect evidence; it is not a parity sample.

Pinned libtorrent's `consume_incoming_data` handles this exact peer-window
overshoot by dropping the DATA without storing it or advancing the receive
acknowledgement; packet ACK processing occurs earlier and remains valid.
RSTorrent instead returned a fatal `ReceiveWindowLimit`. The independently
authored repair adds a typed nonfatal disposition and saturating drop count,
retains the exact 1 MiB byte bound, repeats an immediate ACK/window update,
and carries a per-connection drop high water through runtime evidence.
Deterministic receive, connection, and composed-transport tests fill the
window exactly, overshoot by the observed 497 bytes, prove no payload or
receive-ACK mutation, and prove valid piggyback ACK application. The full 246
protocol tests, focused product runtime, warning-denying protocol/engine/
session Clippy, and 19 focused Python contracts pass before the WAN restart.

The next restart proved why the matrix revision fence is a correctness
boundary rather than bookkeeping. Another authorized workstream advanced the
shared checkout after the first exact TCP cell, so the paired uTP cell could
not be attributed to one revision and the remaining cells rejected the dirty
tree. The harness now promotes any mid-run worktree contamination to a fatal
`WanMatrixRevisionError` instead of emitting one invalid record per remaining
case. No result from that attempt enters a throughput ratio.

### Exact 256 MiB WAN cohort

The final cohort runs from a detached worktree pinned to clean revision
`f34f3d0eaaf3b55412162c9364c6e818bcd7771b`. The guarded UTM guest builds the
Linux ARM64 incoming-seed binary in 84.246 seconds with Rust/Cargo 1.97.0,
four jobs, native `aarch64`, and glibc 2.39; the Pi runtime is `aarch64` with
glibc 2.41. Host and remote checks agree on the 23,359,368-byte artifact and
SHA-256
`d7edf7f6c381a06c2ef4951eafc24becef36546b4b8bd5a196497711d2d3d468`.
The Pi runs no Cargo or rustc process.

All 24 rotating remote-seed cases complete: four implementation pairings,
forced TCP and forced uTP, three repetitions each. Every payload verifies all
268,435,456 bytes and 1,024 pieces; all mappings, role processes, and case
artifacts clean exactly. The ordinary-Internet path is stable in every cell.

| Pairing, seed -> leech | TCP median MiB/s | uTP median (range) MiB/s | uTP / oracle | uTP / own TCP |
| --- | ---: | ---: | ---: | ---: |
| libtorrent -> libtorrent | 2.653976 | 2.740845 (2.739835--2.742531) | 100.00% | 103.27% |
| libtorrent -> RSTorrent | 2.684100 | 2.761146 (2.747405--2.764798) | 100.74% | 102.87% |
| RSTorrent -> libtorrent | 2.672904 | 2.668005 (2.444530--2.673802) | 97.34% | 99.82% |
| RSTorrent -> RSTorrent | 2.639728 | 2.599811 (2.427713--2.664425) | 94.85% | 98.49% |

Every RSTorrent-containing uTP median clears the primary `0.85x` oracle gate
and also reaches at least 98.49% of its own matched TCP median. Each RSTorrent
sender sample uses one connection with no retry exhaustion, terminal failure,
or timeout collapse. The RSTorrent/RSTorrent sender records one or two
retransmission datagrams and one loss reduction, 209--408 startup
acknowledgements, one startup exit, 263,036--524,280 threshold bytes,
664,613--1,048,560 bytes of maximum flight, and 77.594--80.150 ms maximum
queue delay. The corresponding RSTorrent receivers use 462--463 reorder
positions and 664,614--666,768 buffered bytes, with no receive-window drop.
RSTorrent RSS remains at most 19.7 MiB while seeding and 17.0 MiB while
leeching. These are bounded exact measurements, not a broader BEP 29 support
claim.

### Bounded 1 GiB scaling follow-up

Bulk execution stopped at maintainer review after 14 exact 1 GiB cases because
the results repeated the 256 MiB conclusion without selecting another repair.
The first repetition completed all eight TCP/uTP pairings; the second
completed both transports for libtorrent/libtorrent and both mixed pairings.
One in-flight RSTorrent/RSTorrent uTP cell and its following TCP cell were
operator-stopped, cleaned successfully, and are excluded. Consequently these
one-to-two-sample summaries are corroboration, not stable three-repetition
medians:

| Pairing, seed -> leech | TCP samples / middle MiB/s | uTP samples / middle MiB/s | uTP / oracle | uTP / own TCP |
| --- | ---: | ---: | ---: | ---: |
| libtorrent -> libtorrent | 2 / 2.647546 | 2 / 2.723313 | 100.00% | 102.86% |
| libtorrent -> RSTorrent | 2 / 2.656844 | 2 / 2.730754 | 100.27% | 102.78% |
| RSTorrent -> libtorrent | 2 / 2.649726 | 2 / 2.692556 | 98.87% | 101.62% |
| RSTorrent -> RSTorrent | 1 / 2.635353 | 1 / 2.698530 | 99.09% | 102.40% |

Every completed RSTorrent uTP role uses one connection with zero retry
exhaustion. RSTorrent/RSTorrent records six retransmissions/loss reductions
and zero timeout collapse; the two RSTorrent-to-libtorrent samples have zero
retry and at most one clean timeout recovery; the two libtorrent-to-RSTorrent
receivers use 714--721 reorder positions and 1,037,465--1,047,622 buffered
bytes within the unchanged 953-position/1 MiB bounds, with zero receive-window
drop. RSTorrent RSS remains at most 20.8 MiB. Together with the 24 exact
256 MiB cases, the closing campaign transfers 20 GiB of exact payload.

### Reverse-direction environment result

One bounded local-seed RSTorrent/RSTorrent 1 GiB uTP smoke used a second exact
UTM-built ARM64 artifact from revision `f34f3d0`. The host and Pi agreed on its
14,188,944-byte size and SHA-256
`c358c6b85a92475def918bf9ecdd556581349a46f67a23040d14b005cadb4136`;
the Pi again ran no compiler. Before any mapping or payload, independent UPnP
discovery on the current local network could not select an accepted mapped
IGD service. The run therefore makes no reverse throughput claim. It cleaned
all role processes and per-run artifacts, and the harness now reports that
pre-payload condition through the existing typed mapping failure rather than
an unclassified exception. The already proven remote-mapped and earlier
local-mapped directions remain intact; this is current-network capability
evidence, not a uTP failure.

### Platform and repository closure

Both Android native release ABIs (`x86_64` and `arm64-v8a`), Kotlin binding
generation, JVM debug unit tests, and debug APK assembly pass. Final
formatting, warning-denying workspace Clippy, the complete serial workspace
test suite, and all 28 focused WAN mapping/matrix/uTP Python contracts pass.
The exact dynamic-MTU, bandwidth-update, queue-saturation, and incoming
half-open resource gates were made deterministic without relaxing their
production bounds.

An evidence-backed defect in the startup, congestion, transport, runtime
telemetry, or existing WAN harness owners may be repaired autonomously. Stop
for human direction only if evidence selects a different production policy,
steady-state controller change, dependency, resource-bound expansion,
protocol-support claim, another host/network mechanism, or destructive or
externally visible scope not authorized here.

## Invariants And Resource Limits

- Production startup begins at the existing two-MSS window. It grows only on
  acknowledged payload while congestion limited and never beyond the existing
  1 MiB congestion/send ceiling.
- The first queue-delay sample at or above 10,000 microseconds exits startup
  once. Retained window is exactly 30% of the pre-exit window subject to the
  unchanged ordinary floor and ceiling.
- Congestion loss applies the existing once-per-RTT reduction before startup
  exit. Isolated MTU-probe loss cannot exit startup or reduce the window.
- Timeout retains the existing one-MSS collapse. Re-entered startup cannot
  grow beyond the last recorded threshold and ordinary ACK recovery restores
  the existing floor.
- `TARGET`, linear gain, pacing, allowed increase, loss multiplier, MTU
  search, delayed ACK, packetization, receive credit, and reorder positions do
  not change.
- Per connection, sent/unsent/receive payload remains at most 1 MiB,
  outstanding packets at most 1,024, reorder positions 953, shared and
  connection ingress queues 256 datagrams, runtime turn 64 datagrams, and
  eight transmission attempts per packet.
- New observations are saturating scalar counters, Boolean/current counts,
  and high waters. They retain no address, peer ID, payload, or timeline.
- WAN runs retain exact revision, builder/runtime, forced-transport, mapping,
  integrity, process, resource, wire-stop, journal, deadline, cleanup, and
  redaction fences. No Cargo or rustc process runs on `pimom`.

## Validation And Stopping Condition

| Layer | Required evidence |
| --- | --- |
| Pure congestion | selected constants and every startup/exit/loss/timeout/application-limit transition; exact current/candidate A/B |
| Deterministic transport | clean/long-RTT, queue, fairness/recovery, loss, timeout, pressure, MTU, wrap, integrity, and zero ownership |
| Scripted runtime | initiating and accepted roles, telemetry aggregation, cancellation, socket replacement, queue bounds, and zero terminal ownership |
| Controlled product | RSTorrent/RSTorrent and mixed roles over clean 160 ms, exact hash, one connection, forced uTP, resource and cleanup proof |
| WAN | alternating three-repetition 256 MiB cohort plus applicable 1 GiB scaling, matching oracle/TCP controls, no fallback/reconnect or unexplained exclusion |
| Platform/repository | both Android native ABIs, format, workspace Clippy/tests, interop contracts, and reconciled documentation |

The tactical completes when production uses the selected bounded startup, all
preserved fairness/resource/correctness gates pass, and the complete stable
256 MiB cohort places every RSTorrent-containing median above `0.85x` the
matched libtorrent uTP control. The originally desired three repetitions at
1 GiB were superseded by explicit maintainer review after one complete and one
partial corroborating repetition stopped producing a new diagnosis. No stable
1 GiB median or reverse-direction cohort is claimed. The local-seed smoke
closed as typed pre-payload environmental evidence because the current local
network exposed no accepted UPnP IGD service.

## Non-Goals And Next Boundary

This tactical does not copy libtorrent, change steady-state gain or target,
raise packet/byte/queue bounds, expose a startup setting, add UI, add a
dependency, modify NAT or IPv6 behavior, compile on the Pi, repair the
separate remote-placement TCP seed disconnect, optimize endpoint storage or
ISP service, run a public swarm, or broaden the existing **Partial** uTP
support claim without the required cohort.

After closure, any steady-state controller modernization, multi-flow campaign,
different startup policy, broader support claim, or unrelated engine feature
requires its own decision.
