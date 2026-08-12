# Tactical 142: WAN Transport Performance Matrix

Status: **Active.** Explicit maintainer direction authorizes this tactical,
autonomous implementation and diagnosis, setup of the existing `pimom`
control peer, repeatable multi-gigabyte direct-public-path TCP/uTP traffic,
and logical commits. Tailscale remains available for SSH control but must not
carry payload traffic. There is no metered-traffic ceiling; exact per-case
time, disk, process, mapping, integrity, and cleanup bounds still apply.

Topics: `utp-transport-campaign`, `performance-and-live-evidence`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tacticals
[`122`](122-paired-public-download-performance-cohorts.md),
[`127`](127-mapped-utp-wan-interoperability.md),
[`130`](130-utp-transport-solidification.md),
[`135`](135-controlled-tcp-storage-near-parity.md),
[`137`](137-product-utp-path-mtu-discovery.md),
[`140`](140-incoming-utp-reachability.md), and closed evidence-limited
[`141`](141-product-wan-tcp-utp-comparison.md).

## Decision And Desired Outcome

Build one case-addressable, restartable WAN performance lab that can compare
RSTorrent and pinned libtorrent in either seed or leecher role, over forced
TCP or forced uTP, in both physical directions between the development host
and `pimom`. Use it to establish a complete cross-engine baseline over several
payload sizes, distinguish host/storage/path limitations from transport
implementation behavior, and isolate any RSTorrent-specific uTP throughput
defect before changing the engine.

The complete logical matrix for each size is:

```text
physical direction: development seed -> remote leech | remote seed -> development leech
seed implementation: RSTorrent | libtorrent
leech implementation: RSTorrent | libtorrent
transport: TCP | uTP
```

That is 16 cells per size. The fixed sizes are 8 MiB, 64 MiB, 256 MiB, and
1 GiB with 256 KiB v1 pieces, one file, no trackers or web seeds, and exact
independent payload verification. Every cell runs at least once. Cohorts that
support a diagnosis run three rotating repetitions; a one-off successful cell
is reported as an observation, never as a stable ratio.

The baseline itself is not a parity gate. BEP 29 makes uTP a delay-sensitive
background transport, so lower idle-path throughput can be legitimate. A
candidate RSTorrent defect requires role- and transport-specific evidence
against the libtorrent controls on the same physical path, then a controlled
local or scripted reproduction of the implicated state transition.

## Scope And Stopping Condition

This tactical owns the measurement and diagnosis workstream:

1. add exact direct TCP and direct uTP profiles to the existing RSTorrent
   public probe without changing ordinary product policy;
2. generalize the pinned-libtorrent WAN helper into one bounded seed/leecher
   role owner with mutually exclusive TCP/uTP settings and comparable timing,
   transport, integrity, and resource evidence;
3. add one orchestrator with deterministic fixtures, preflight, native remote
   RSTorrent staging, public mapping ownership, both physical directions,
   case selection, atomic result journaling, resume, rotation, aggregation,
   redaction, and exact cleanup;
4. calibrate host storage and route ceilings, prove every implementation/role/
   transport combination at 8 MiB, then execute all sizes;
5. classify any slow cells using the controls below and repeat the implicated
   cohorts three times;
6. if and only if the evidence isolates an existing RSTorrent uTP ownership
   boundary, open a focused successor implementation tactical before changing
   production transport code; then implement the bounded repair, rerun its
   deterministic/controlled gates and the affected WAN matrix, and retain TCP
   and unaffected-role regressions; and
7. reconcile the tactical, focused topics, readiness queue, and exact evidence.

Tactical `142` completes when the reusable lab is committed and validated,
every baseline cell either has an exact successful result or a typed retained
environment/capability failure, all successful output hash-verifies and all
resources clean, and the data selects either a focused RSTorrent repair or a
documented non-RSTorrent/environmental limit. It does not require inventing a
transport change when the controls do not isolate one.

## Classification Contract

For a fixed direction, size, and transport:

| Result | Initial interpretation |
| --- | --- |
| libtorrent/libtorrent is slow | path, ISP, host, or storage ceiling; not RSTorrent evidence |
| RSTorrent seed is selectively slow | RSTorrent upload, uTP send, read, or pacing owner |
| RSTorrent leecher is selectively slow | RSTorrent receive, ACK/window, request, hash, or write owner |
| only RSTorrent/RSTorrent is slow | interaction or feedback coupling between both owners |
| RSTorrent is also slow over TCP | generic engine/storage/request scheduling, not uTP first |
| result changes with physical direction | asymmetric ISP, gateway, route, CPU, or SD-card influence |
| loss/retransmit is low but window/flight stays low | ACK, RTT, receive-credit, pacing, or application-feed diagnosis |
| libtorrent uTP shows the same behavior | expected LEDBAT/path behavior until a stronger control disproves it |

These are hypotheses, not automatic conclusions. Process CPU/RSS, storage
throughput/iowait, uTP packet/loss/timeout/resend counts, selected MTU,
RTT/RTO/delay, congestion/flight/advertised windows, request backlog, storage
backlog, and progress curves decide among them.

## Normative And Source Oracle

BEP 29 at managed specification commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06` defines uTP's window-based,
delay-sensitive congestion behavior and its intent to yield to competing TCP.
The matrix runs sequentially without intentional competing traffic and records
the path rather than treating TCP equivalence as normative.

Pinned Rasterbar libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected as the completeness,
comparison, and edge-case oracle, not an architecture template or source
donor:

- `include/libtorrent/settings_pack.hpp`, `src/settings_pack.cpp`, and
  `simulation/utils.cpp::utp_only` define exact mutually exclusive transport
  controls;
- `test/test_utp.cpp::test_transfer`, `test/test_transfer.cpp`, and
  `simulation/test_transfer.cpp` prove forced-role transfers rather than
  inferring the socket type from configuration alone;
- `src/utp_socket_manager.cpp::mtu_for_dest` and
  `src/utp_stream.cpp::{init_mtu,update_mtu_limits,send_pkt,resend_packet,
  do_ledbat,packet_timeout,tick}` expose the packetization, congestion, loss,
  timer, and send-window boundaries relevant to a throughput diagnosis;
- `simulation/test_utp.cpp::{utp_pmtud,utp_plain,utp_buffer_bloat,utp_straw,
  utp_small_kernel_send_buf}` retains packet/loss/timeout and delay-controller
  expectations under distinct path conditions; and
- `include/libtorrent/performance_counters.hpp`, `torrent_status.hpp`, and the
  Python status binding provide the transport counters and payload milestones
  used by the lab.

Extracted edge cases are: disable the alternative transport before introducing
the peer; retain applied settings and observed counters; permit final live-peer
gauges to return to zero after completion; distinguish payload receipt from
hash-verified seeding; keep monotonic connect-inclusive and active-payload
clocks; treat MTU probes separately from congestion loss; retain zero-window,
small-send-buffer, timeout, retransmit, and delay-above-target evidence; and
exclude failed or mixed-transport cells from ratios without deleting them.

The local JSTorrent reference at
`9895410beeed6aff554053769bd006a3fbd373ef` was inspected read-only with its
pre-existing untracked files left untouched. Its
`packages/engine/integration/python/libtorrent_utils.py`,
`test_seeder_leecher.py`, `test_large_download.py`, and `seed_for_test.py`
disable uTP and provide local TCP-oriented role helpers, not a cross-host
TCP/uTP matrix. The useful inherited lessons are explicit machine-readable
readiness, role separation, whole-file hashing, and teardown. No reference
source, fixture, or test data is copied.

## Owner, Task, Cancellation, And Data Flow

```text
matrix owner (development-host Python process)
  -> immutable manifest + atomic append-only case journal
  -> deterministic fixture owner (one payload/metainfo per size)
  -> endpoint adapters
       -> local process owner
       -> SSH control owner for pimom
  -> one case at a time
       -> seed role (RSTorrent or pinned libtorrent)
            -> exact finite TCP or UDP public mapping
       -> leech role (RSTorrent or pinned libtorrent)
            -> direct ordinary-Internet dial to mapped endpoint
       -> periodic endpoint/process/storage telemetry
       -> exact hash and transport validation
       -> stop/join roles, delete mapping, remove case artifacts
  -> pure redacted aggregation and diagnosis tables
```

The orchestrator owns order, case identity, deadlines, SSH children, journal,
and cleanup. Each role helper owns exactly one engine session/process and its
files. Existing RSTorrent owners retain UDP/uTP, peer, storage, and mapping
lifecycle. No orchestration object enters protocol or production state.

Cancellation is staged: request graceful role stop, join within the case
grace, terminate only the exact owned PID if needed, delete only the exact
owned mapping and generated case directory, independently query absence, and
write the terminal journal record. Ctrl-C follows the same cleanup path. A
case is never marked successful before cleanup is recorded.

## Invariants And Resource Bounds

- matrix: 64 required baseline cells, strictly sequential, at least one and
  at most three retained successful repetitions per cell in one baseline
  epoch; diagnostic or post-repair epochs have distinct identities;
- fixture: exactly 8/64/256/1,024 MiB, 256 KiB pieces, one v1 file, no tracker,
  DHT, LSD, PEX, web seed, MSE, proxy, or unrelated peer source;
- transport: one mapped protocol and one selected transport per case; the
  alternative is disabled by libtorrent and must remain unobserved by both
  engines; a RSTorrent uTP dial may retain its production TCP fallback path,
  but any fallback makes the uTP case typed invalid rather than successful;
- peers: one direct peer, one live connection, and one transfer maximum;
- time: readiness 60 seconds, cleanup 30 seconds, and transfer deadlines of
  15 minutes/45 minutes/3 hours/12 hours for 8/64/256/1,024 MiB respectively;
- wire: each case stops if counted payload exceeds twice the fixture size;
  there is no aggregate traffic cap because the maintainer explicitly permits
  repeated multi-gigabyte traffic;
- disk: preflight requires fixture plus output plus 25% headroom; the remote
  1 GiB tier uses its persistent ext4 SD-card filesystem because its tmpfs is
  smaller than the fixture. Storage class is constant across matrix sizes;
- memory: RSTorrent retains production desktop/Linux limits; helper capture is
  bounded to 1,024 time samples, 100 diagnostics, and 1 MiB stdout/stderr per
  role. Libtorrent uses one connection and an eight-entry peer list;
- mapping: one finite lease of at most 3,600 seconds, exact protocol/port/
  target verification, deletion, and absent inventory per case;
- identity: fixture SHA-1 and metainfo info hash must agree on both hosts;
- privacy: committed reports contain role classes, direction classes, route
  class, versions, and aggregate network observations, never public/private
  addresses, SSH endpoints, peer IDs, gateway control URLs, usernames, or
  machine identity; and
- remote setup: a per-user pinned Rust toolchain/build and the existing pinned
  libtorrent environment are isolated below the documented oracle root. No
  system package, service, firewall, permanent mapping, or Tailscale change is
  permitted.

The baseline payload total is 21,632 MiB for one complete 64-cell epoch.
Repetitions and a post-repair epoch are deliberately traffic-unbounded but
remain manifest-bounded, sequential, case-addressable, and exactly journaled.

## Measurement And Aggregation Contract

Every case retains:

- manifest identity: epoch, repetition, size, piece count, physical direction,
  seed implementation, leech implementation, transport, and execution order;
- monotonic readiness, dial, first payload, 25/50/75%, last payload,
  hash-verified completion, and cleanup milestones where the role exposes them;
- active-payload and connect-inclusive MiB/s computed from exact bytes;
- exact payload size, pieces, whole-file SHA-1, info hash, selected transport,
  applied settings, peer high water, and terminal owner counts;
- bounded per-second process CPU/RSS and endpoint storage/iowait/thermal data;
- RSTorrent request, storage, peer-method, UDP/uTP, RTT/RTO, delay,
  congestion/flight/window, MTU, loss, resend, timeout, pacing, and queue high
  waters already exposed by its snapshots; and
- matching available libtorrent session counters and bounded diagnostics.

Aggregate only exact successful cells. Report each sample, median and range
for repeated cells, uTP/TCP within-engine-role ratios, RSTorrent/libtorrent
role ratios, physical-direction ratios, and scaling between sizes. Do not pool
bytes across cases, discard slow successful samples, silently replace a typed
failure, compare different storage classes as equivalent, or call a one-run
ratio stable.

## Implementation Stages And Gates

1. **Contracts:** pure manifest/case-key, rotation, deadline, journal, resume,
   aggregation, privacy, and classification tests.
2. **Role adapters:** direct forced libtorrent roles and exact RSTorrent probe
   profiles, each passing local TCP/uTP mixed-engine transfers and cleanup.
3. **Remote setup:** same-revision native RSTorrent binaries on `pimom`, pinned
   version/hash evidence, free-space/storage calibration, and zero live case
   residue.
4. **WAN smoke:** every engine pairing, role placement, and transport at
   8 MiB, with ordinary-route and mapping proof.
5. **Baseline:** all 64 cells, atomic progress after each case, then three-run
   cohorts for the comparisons that determine the diagnosis.
6. **Isolation:** reproduce any implicated RSTorrent boundary locally or in
   the scripted impairment harness before production changes.
7. **Repair:** create a focused successor tactical, implement one causal
   repair, and rerun deterministic, controlled, affected WAN, TCP regression,
   and proportional Android/build evidence.
8. **Closure:** redact and record aggregate evidence, remove raw captures and
   payloads, audit both gateways/hosts, and reconcile living topics.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure Python | manifest cardinality, case identity, rotation, atomic journal/recovery, aggregation, failure retention, redaction, path and cleanup bounds |
| Rust harness | exact direct transport profiles, applied/effective settings, observed peer transport, telemetry, cancellation and terminal ownership |
| Controlled local | all four engine pairings over TCP/uTP, exact hashes, one peer, no masking, repeated cleanup |
| Remote preflight | versions/build hashes, ordinary payload route, storage/network calibration, finite mapping lifecycle, no residue |
| Physical WAN | all 64 baseline cells or typed retained failures, diagnostic repetitions, exact integrity/transport/resources/cleanup |
| Repair if selected | focused deterministic/scripted reproduction, mixed interop, affected WAN rerun, TCP/unaffected-role regression, Android cross-build or stronger platform evidence as applicable |
| Repository | Python compilation/unit tests, focused Rust tests, formatting, warning-denying workspace Clippy, workspace tests, clean diff |

## Non-Goals And Escalation

This tactical does not promise TCP-equivalent uTP, add a product performance
setting, change transport defaults, run concurrent competing-flow fairness,
add IPv6 uTP or MSE-over-uTP, install system packages, change gateway/Tailscale
policy, use a public swarm, or optimize the Pi SD card or ISP.

Ordinary harness repair, isolated per-user remote toolchain setup, repeated
authorized cases, conservative tighter limits, and a causally scoped fix at
an already accepted uTP owner do not require routine review. Stop for human
direction if evidence calls for a new dependency, protocol/product policy,
permanent network change, different external host, destructive non-owned data
action, broader engine architecture, or more than one unrelated production
bottleneck. An environmental timeout or slow run is retained and resumed; it
is not by itself a reason to abandon the matrix.
