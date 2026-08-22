# Tactical 153: Wired-LAN uTP Data-Plane Scalability

Status: **Decision-complete and Later (reconciled 2026-08-22).** The explicit
beta-readiness campaign and active Tactical `157` supersede this tactical's
former **Now** position without invalidating its measurement design. It begins
only after a future readiness-queue selection and when the physical wired
workstation is available.

Topics: `utp-transport-campaign`, `performance-and-live-evidence`,
`capability-readiness`, `oracle-driven-engine-campaign`, `protocol-support`

Dependencies: completed Tacticals
[`137`](137-product-utp-path-mtu-discovery.md),
[`142`](142-wan-transport-performance-matrix.md),
[`145`](145-sustained-utp-reliability-and-throughput-near-parity.md), and
[`150`](150-bounded-utp-sender-startup.md); the existing exact role, fixture,
resource-sampling, transport-forcing, integrity, revision, and cleanup owners;
and the separate `machine-control` checkout for target selection and platform
readiness when an inventory route is available.

## Decision And Desired Outcome

Measure RSTorrent's practical single-flow TCP and uTP ceiling over a quiet,
wired, private IPv4 LAN. One endpoint is the development Mac attached through
its available 1 GbE USB-C adapter. The other is a desktop-class workstation
with a 2.5 GbE NIC running native Linux or native Windows. The effective first
path is therefore gigabit, not 2.5 gigabit: the slowest negotiated link owns
the ceiling. A future result may claim 2.5 GbE only after both endpoints and
the switch path negotiate at least 2.5 GbE.

At a roughly 940-Mbit/s payload ceiling and 1,457-byte uTP datagrams, the test
can approach 80,000 DATA datagrams per second plus acknowledgements. That is
roughly forty times the packet rate of Tactical `150`'s WAN cohort and can
expose per-datagram allocation, routing, queueing, pacing, wakeup, syscall, or
platform-socket costs that the WAN path could not measure.

The primary question is not whether uTP can win an absolute headline number.
It is whether RSTorrent uTP remains close to its same-direction RSTorrent TCP
control and, where a matched native role is available, the pinned libtorrent
uTP oracle when storage and path limits are accounted for. The evidence must
distinguish:

- a physical link, adapter, switch, storage, or CPU ceiling;
- kernel TCP offload advantage rather than a uTP implementation defect;
- an RSTorrent-independent userspace UDP ceiling;
- an RSTorrent shared-UDP, transport-worker, packetization, or application
  composition ceiling; and
- a native Linux/macOS versus native Windows socket-stack difference.

This tactical measures and attributes first. It may repair the harness,
telemetry, or a demonstrated correctness defect within existing owners and
bounds. It stops for review before a production batching architecture,
platform-specific syscall path, new dependency, queue/resource-bound increase,
socket-buffer policy, congestion-policy change, or OS tuning.

## Normative, Source, And Product Oracle

The normative source is the pinned BEP 29 text at
`reference/bittorrent.org/beps/bep_0029.rst`, especially packet sizes and
congestion control. It explicitly expects large packets at high rates while
retaining delay-based yielding. Gigabit throughput cannot weaken the 100 ms
steady-state target, the bounded startup policy, loss response, or fairness
contract.

Planning inspection used exact pinned libtorrent `2.0.13` revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` and covered:

- `src/udp_socket.cpp::udp_socket::read`, which currently performs one
  nonblocking receive and explicitly leaves socket draining or `recvmmsg()` as
  future work;
- `src/utp_socket_manager.cpp::{send_packet,incoming_packet,socket_drained}`,
  which owns shared UDP dispatch, the last-socket fast path, deferred ACKs,
  and writable/drained notification;
- `src/utp_stream.cpp::{send_pkt,incoming_packet,do_ledbat,tick}`, which owns
  packet filling, window admission, receive processing, congestion, and
  retransmission timers;
- `include/libtorrent/aux_/set_socket_buffer.hpp`, which applies explicit
  session socket-buffer settings when configured; and
- `simulation/test_utp.cpp::{utp_plain,utp_buffer_bloat,utp_straw,
  utp_small_kernel_send_buf}` plus `test/test_utp.cpp::utp`, which cover clean,
  competing, buffered, constrained-kernel-buffer, and real-socket behavior but
  do not provide a physical high-rate cross-platform gate.

The source finding matters in both directions: libtorrent remains the
interoperability and performance oracle, but its one-packet receive path means
batching is not presumed necessary or copied without evidence.

Relevant RSTorrent paths are:

- `crates/rstorrent-engine/src/session_udp.rs::run_receive_loop`, currently one
  `recv_from`, one bounded datagram copy, and a 256-datagram shared uTP queue;
- `crates/rstorrent-engine/src/utp_runtime.rs::{handle_datagram,
  route_existing_datagram}`, which decode and route into a bounded
  256-datagram per-connection queue;
- the uTP worker's `drain_emissions`, which sends at most 64 emissions per turn
  through the generation-fenced shared egress; and
- the existing application and WAN role adapters, which already report exact
  transport, connection, payload, queue, congestion, receive, process,
  revision, integrity, and cleanup evidence.

Revalidate the pin and these paths before implementation. They are attribution
candidates, not conclusions. The local JSTorrent reference contains no uTP
data plane: `docs/archive/engine/legacy-migration/architecture_analysis.md`
records the historical engine as TCP-only, and
`docs/archive/performance/chromeos-companion-throughput.md` only proposed uTP
as an untested hypothesis. It supplies no high-rate implementation behavior to
preserve.

## Physical Topology And Platform Contract

```text
development Mac
  native macOS RSTorrent + pinned libtorrent
  wired 1 GbE USB-C adapter
             |
             | one quiet private IPv4 LAN, MTU 1500
             v
desktop workstation
  wired 2.5 GbE NIC
  epoch A: native Linux or native Windows RSTorrent
  epoch B: native Windows RSTorrent when separately available
```

The first platform epoch uses whichever native workstation OS is available.
A native Windows epoch is deliberately planned because Tokio/Windows UDP,
Windows Firewall, timer scheduling, and kernel counters can differ materially
from Linux and macOS. WSL does not satisfy that gate. If native Windows is not
available, the tactical may close a Linux-only measurement checkpoint but
must say that Windows remains unmeasured; it cannot claim cross-platform
completion.

Use `~/code/machine-control/bin/machine-control targets`, private inventory
status, and the relevant logical-target doctor when that checkout exposes the
machine. Concrete selectors, addresses, credentials, and inventory values stay
outside this repository. Ordinary authenticated target-local shell execution
is sufficient; no visible desktop automation is required. A missing physical
Linux or Windows inventory adapter is recorded honestly rather than replaced
with an undocumented legacy testbed.

Planning also inspected `machine-control/README.md`'s common target entry and
the canonical `platforms/{linux,windows}/README.md` guides. They establish the
common target/doctor front door, private inventory boundary, native Windows
OpenSSH/PowerShell route, and current Linux VM-versus-physical capability
distinction. This tactical owns RSTorrent-specific launch and evidence; it
does not add private machine facts to either repository.

For every platform epoch:

- record OS/build, architecture, CPU, memory, NIC/driver, adapter, negotiated
  link speed/duplex, route/interface, MTU, and filesystem facts;
- require a direct private LAN route with no Wi-Fi, VPN, Tailscale, public NAT,
  UPnP, tracker, DHT, or Internet dependency;
- start with ordinary MTU 1500 and verify the expected dynamic IPv4 uTP
  packetization; jumbo frames are a separate experiment;
- use native release binaries built from one clean committed archive and
  record toolchain, target, size, and SHA-256 provenance;
- use only an exact, temporary private-profile firewall opening when needed,
  independently verify it, and delete it during joined cleanup; and
- capture before/after NIC and OS UDP/TCP counters without treating unrelated
  host-global traffic as process-attributed evidence.

Any permanent firewall change, route change, adapter reconfiguration, OS
network tuning, package installation, or switch configuration requires review
at execution time.

## Owner, Task, Cancellation, And Data Flow

```text
case-addressable LAN matrix owner
  exact revision + fixture + platform epoch + rotated case journal
        |
        +--> local native role process and bounded resource sampler
        |
        +--> authenticated target-native role process and sampler
        |
        +--> exact hash/integrity verification and joined cleanup audit
        v
bounded result JSON
  setup/active time + rates + packet/ACK cadence + CPU/RSS + OS/NIC deltas
  + existing uTP/TCP transport, queue, loss, delay, MTU, and owner evidence
```

One matrix process owns one case at a time. Each role has a startup deadline,
payload deadline, cancellation path, terminal result, process join, and exact
artifact cleanup. A target-local helper owns only its role process and bounded
sampling interval; it is not a daemon or product architecture. The test adds
no production task, socket, endpoint, IPC service, setting, or unbounded
timeline.

The harness should reuse the Tactical `142` role protocol and journal rather
than fork another measurement vocabulary. Platform-specific command launch,
process sampling, path quoting, and cleanup live behind the harness boundary;
they do not enter engine or protocol state.

## Matrix And Measurement Contract

The core matrix does not require a native pinned-libtorrent build on the
workstation. It exercises native RSTorrent there in both roles against both
RSTorrent and the exact pinned libtorrent role on macOS:

| Physical direction | Seed | Leecher | Forced transports |
| --- | --- | --- | --- |
| Mac -> workstation | RSTorrent | RSTorrent | TCP, uTP |
| Mac -> workstation | libtorrent | RSTorrent | TCP, uTP |
| workstation -> Mac | RSTorrent | RSTorrent | TCP, uTP |
| workstation -> Mac | RSTorrent | libtorrent | TCP, uTP |

If a reproducible exact pinned-libtorrent role is available natively on the
workstation without an unapproved dependency or system mutation, expand the
epoch to all four implementation pairings in both directions. Native Windows
libtorrent is useful additional oracle evidence but is not required to measure
the Windows RSTorrent stack against its own TCP and the macOS oracle.

Use one exact deterministic payload and metainfo per selected size:

- 1 GiB once per pairing/transport as a smoke and warm-up, excluded from the
  stable median unless explicitly promoted before execution;
- 8 GiB for three rotating repetitions of every primary case; and
- 16 GiB instead of 8 GiB only when the calibrated effective path makes an
  8 GiB active interval shorter than 30 seconds. No case exceeds 32 GiB.

One platform epoch is capped at 512 GiB transferred payload including failed
attempts. Keep at least two fixture sizes plus 2 GiB free on every filesystem,
run only one case at a time, and clean each leecher payload before the next
case. A case deadline is 15 minutes at 8 GiB and 30 minutes at 16 GiB. If the
three-sample rate range exceeds 10%, run at most two additional repetitions;
do not chase stability without a finite bound.

Seed content is preverified and warmed before timing. The leecher destination
uses the ordinary fast local storage path, and disk service time, bytes, and
CPU are recorded. If storage or hashing approaches the measured rate ceiling,
add a bounded transport-only generated-stream/checksum profile before changing
network code. That profile must stream incrementally, retain at most the
existing connection/application byte bounds, and have matched TCP/uTP roles;
it may not allocate or retain the full payload.

Each case records at least:

- setup, connection-inclusive, active-payload, and steady-window rates;
- exact useful bytes, wire bytes, DATA/STATE counts and per-second cadence;
- selected datagram size, MTU probes/fallback, startup exit, congestion and
  flight high waters, RTT and queue-delay distribution;
- ACK cadence, retransmissions, loss reductions, timeout collapses, reorder
  positions/bytes, receive-window drops, and connection/retry counts;
- shared/session and connection queue high waters/drops, egress waiters,
  protected-send attempts/`WouldBlock`, and task/owner terminal zeroes;
- process user/system CPU, wall time, RSS, threads, voluntary/involuntary
  context switches where portable, and storage bytes/service time; and
- link/NIC/OS UDP/TCP packet, byte, error, discard, and drop deltas with their
  attribution limits stated.

Do not enable packet capture by default at gigabit rates. A later diagnostic
capture is header-only, snaplen-bounded, duration-bounded, and selected only
after counters identify a narrow interval.

## Classification And Acceptance Gates

The fastest valid same-direction TCP control defines the effective path
ceiling only when it reaches at least 80% of the slowest negotiated link and
shows no storage, CPU, error, or drop bottleneck. A lower TCP result is still
evidence, but the epoch is path-limited and cannot support a line-rate claim.

For each native RSTorrent role and direction, compare the stable uTP median to
the matching RSTorrent TCP median and every valid same-direction pinned-
libtorrent uTP control. An epoch without a native workstation libtorrent role
may establish RSTorrent TCP/uTP scaling and mixed interoperability, but cannot
claim matched libtorrent parity for the missing role:

- **Near parity:** at least `0.90x` own TCP and every available matched oracle,
  one connection, exact integrity, zero unexplained ingress/connection drop,
  and no retry exhaustion or terminal failure. State which oracle roles were
  unavailable.
- **Usable but attributable gap:** `0.85x`--`0.90x` either valid control;
  retain the distributions and identify the first saturated owner.
- **Selected data-plane bottleneck:** below `0.85x` any valid control in a
  stable cohort, or rate plateaus while a bounded queue, CPU, socket, or
  scheduling owner demonstrably saturates.
- **Inconclusive:** unstable physical path, insufficient TCP/oracle control,
  storage saturation, host-global counter contamination, or missing platform
  evidence prevents attribution.

The result reports medians and ranges, not just ratios. TCP kernel offload is
part of the platform truth; uTP is not required to match an offload-assisted
TCP number before the libtorrent uTP comparison is considered. Conversely,
libtorrent parity does not excuse an avoidable RSTorrent queue drop or CPU
hot path.

Correctness and preservation gates remain exact: one forced transport, no
fallback masking, one connection, complete payload hash, unchanged congestion
and resource bounds, bounded delay/fairness behavior, deterministic loss/MTU
regressions, joined task/process termination, and zero temporary artifact or
firewall residue. Both Android native ABIs and the normal repository gates run
only if production engine code changes.

## Implementation Stages

1. **Target and route preflight.** Select the authorized native workstation,
   record the slower effective link, confirm direct private-LAN routing and
   MTU, check storage headroom and target readiness, and stop before any
   permanent system change.
2. **Portable role and evidence seam.** Extend the existing matrix/journal,
   native launch, sampling, quoting, deadline, integrity, and cleanup owners
   for the selected Linux or Windows target. Add deterministic contracts for
   spaces, interruption, stale processes, partial output, and firewall cleanup.
3. **TCP and fixture calibration.** Prove exact 1 GiB smoke cells, warm the
   selected fixture, establish the effective TCP/path/storage ceiling in both
   directions, and reject a path-limited epoch before interpreting uTP ratios.
4. **Primary uTP epoch.** Run three rotating 8 or 16 GiB repetitions for the
   core matrix, with at most two bounded stability repetitions. Preserve all
   valid, invalid, failed, and operator-stopped cases in the atomic journal.
5. **Causal attribution.** Use packet cadence, CPU, queues, `WouldBlock`, OS
   counters, and storage evidence to select or reject shared receive, routing,
   connection worker, egress, packetization, controller, and product storage.
   Add the transport-only profile only if product composition prevents that
   distinction.
6. **Native Windows epoch.** Repeat the core matrix on native Windows when the
   platform is available. A Linux result cannot stand in for it, and WSL is
   excluded. Record Windows Firewall and native UDP/TCP counter cleanup.
7. **Review and closure.** If no stable gap exists, close with the measured
   ceiling and platform scope. If evidence selects a production bottleneck,
   propose one bounded repair tactical with the exact owner and expected gain.
   Reconcile the performance/uTP topics and remove owned artifacts.

## Human Review And Stopping Condition

Routine harness work, exact native release builds on the desktop-class target,
the bounded matrix, and read-only diagnosis may proceed autonomously once this
tactical becomes Now. Stop before:

- installing a system package or permanent service;
- changing a persistent firewall, route, NIC, switch, MTU, socket-buffer, or
  OS tuning value;
- adding a dependency or platform-specific production syscall implementation;
- increasing a byte, datagram, queue, turn, task, connection, or retry bound;
- changing startup, steady-state LEDBAT, pacing, loss, ACK, or MTU policy;
- introducing unsafe code or a separate networking daemon; or
- claiming 2.5 GbE, Windows, broader BEP 29 support, or multi-flow fairness
  without the corresponding completed evidence.

The tactical stops when one native workstation epoch has a stable, exact,
both-direction TCP/uTP matrix and causal ceiling classification, plus a native
Windows epoch if Windows hardware is available during execution. A Linux-only
checkpoint is explicitly platform-limited. A stable gap may close this
measurement tactical with a recommended focused repair; it does not authorize
speculative optimization merely to make a benchmark number larger.

## Non-Goals And Next Boundary

This tactical does not test a public route, Wi-Fi, VPN, NAT, UPnP, IPv6 uTP,
MSE-over-uTP, proxies, concurrent torrents, multiple uTP flows, TCP fairness,
jumbo frames, 10 GbE, mobile platforms, or a public swarm. It does not treat
1 GbE through the Mac adapter as 2.5 GbE, make persistent OS/network changes,
replace product storage evidence with an unrealistic sink, expose a product
setting, or graduate BEP 29 beyond **Partial**.

A later 2.5 GbE epoch requires two endpoints and a complete switch path
negotiated at 2.5 GbE or faster. Multi-flow/fairness, jumbo MTU, and any
evidence-selected receive/send batching remain separately bounded decisions.
