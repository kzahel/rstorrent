# Tactical 141: Product WAN TCP/uTP Comparison

Status: **Active on 2026-08-12.** Explicit maintainer direction selected the
paired comparison proposed after Tactical
[`140`](140-incoming-utp-reachability.md). Autonomous source review,
implementation, the already established `pimom` control peer, bounded public
TCP/uTP traffic, exact cleanup, and logical commits are authorized. This is a
measurement tactical, not authorization to tune congestion control or change
product transport policy from the resulting ratio.

Topics: `utp-transport-campaign`, `performance-and-live-evidence`,
`incoming-reachability-and-seeding`, `capability-readiness`,
`oracle-driven-engine-campaign`

Dependencies: completed Tacticals
[`088`](088-upnp-mapped-external-tcp-seeding.md),
[`125`](125-shared-udp-utp-runtime-and-loopback-interop.md),
[`130`](130-utp-transport-solidification.md),
[`133`](133-utp-product-default-enablement.md),
[`135`](135-controlled-tcp-storage-near-parity.md),
[`137`](137-product-utp-path-mtu-discovery.md), and
[`140`](140-incoming-utp-reachability.md) supply the product seed, both peer
transports, controlled performance method, dynamic uTP MTU, independent TCP
and UDP mappings, off-LAN oracle, and cleanup foundations.

## Decision And Desired Outcome

Measure TCP and uTP as alternative incoming transports to the same ordinary
RSTorrent product seed over the same mapped public path. Each pair uses one
product process, one fixture, one remote libtorrent version, one public
address, simultaneously verified finite TCP and UDP mappings, and sequential
single-peer transfers. The forced remote transport and order vary; the engine,
storage, payload, route, and gateway stay fixed within the pair.

The primary comparison is exact payload bytes divided by time from first
payload progress through verified completion. Connection-to-completion time is
retained separately. This avoids calling UPnP discovery, process startup, SSH
staging, or initial peer connection latency transfer throughput. The result is
the median of within-pair TCP/uTP throughput ratios, accompanied by every
sample and range. It is an observation, not a pass threshold.

Use an 8 MiB plus 731-byte deterministic v1 single-file fixture with 256 KiB
pieces. It is four times the Tactical `140` fixture while retaining 33 pieces,
so the comparison spends materially more time moving blocks without allowing
piece-count or metadata scale to dominate.

## Scope And Stopping Condition

This tactical owns one bounded measurement slice:

1. generalize the retained remote libtorrent leecher so an explicit closed
   `tcp | utp` transport selects mutually exclusive libtorrent settings and
   validates the same metainfo-derived fixture bounds;
2. retain first-payload, 25%, 50%, 75%, and completion timing plus exact
   payload, hash, peer, and transport counters without retaining endpoints;
3. add one product-owned WAN comparator that creates and independently queries
   both product mappings, then runs the two transports sequentially against
   that unchanged session;
4. alternate order by pair (`TCP/uTP`, `uTP/TCP`, `TCP/uTP`) and wait for exact
   peer-owner drain before the second transfer;
5. require three complete pairs and permit at most one replacement pair after
   a typed, exactly cleaned transport or environmental failure;
6. compute per-case active and connection-inclusive MiB/s, within-pair uTP/TCP
   ratios, medians, ranges, and order strata only from complete pairs; and
7. delete and independently confirm both finite mappings after every pair,
   remove every local/remote artifact and process, then reconcile the evidence.

The tactical completes positively only when three pairs each prove:

- both external mappings target the actual product TCP and shared UDP/uTP
  listener ports and share the same eligible public IPv4 address;
- the off-LAN route to each endpoint is ordinary Internet, never the
  SSH/Tailscale control route;
- exactly one remote peer transfers at a time, the selected libtorrent
  transport is enabled, the alternative is disabled, and observed counters do
  not show transport masking;
- the exact 8,389,339 bytes and every piece hash-verify independently for both
  transports;
- the first peer generation drains before the second begins, total product
  payload accounting equals two fixtures, and no worker panics occur;
- joined shutdown removes TCP and UDP mappings, terminal mapping tasks and
  mappings are zero, the remote process/run directory and local temporary
  root are absent, and an independent gateway inventory finds no owned
  residue; and
- the report contains three valid within-pair active-throughput ratios with
  no endpoint or machine identity.

If four safely cleaned pair attempts cannot produce three complete pairs, the
tactical closes evidence-limited. It records typed failures and individual
complete cases but emits no cohort median or comparative conclusion.

## Normative And Source Oracle

BEP 29 at managed specification commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06` describes uTP as a window-based,
delay-sensitive background transport intended to yield to competing TCP. It
does not promise TCP-equivalent throughput on an idle path. This tactical
therefore measures rather than assumes parity and does not treat a lower uTP
rate by itself as a protocol failure.

Pinned Rasterbar libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected as behavior and
measurement oracle, not source to copy:

- `include/libtorrent/settings_pack.hpp` and `src/settings_pack.cpp` expose
  independent incoming/outgoing TCP and uTP switches;
- `test/test_utp.cpp` disables both TCP directions for its controlled uTP
  transfer, while `test/test_transfer.cpp` disables both uTP directions for
  its TCP transfer control;
- `include/libtorrent/torrent_handle.hpp::connect_peer` attaches the uTP
  capability flag by default, and `src/torrent.cpp::try_connect_peer` selects
  uTP only when settings and peer capability permit it, otherwise TCP when
  enabled;
- `src/session_impl.cpp::incoming_connection` rejects an incoming transport
  whose corresponding setting is disabled;
- `include/libtorrent/performance_counters.hpp` provides separate live TCP and
  uTP peer gauges; and
- `include/libtorrent/torrent_status.hpp` plus
  `bindings/python/src/torrent_status.cpp` expose exact payload progress and
  payload-rate counters used to locate first progress and completion.

Extracted edge cases are: settings must be mutually exclusive before the
direct peer is introduced; `connect_peer` capability flags must not silently
select uTP in the TCP case; final live peer gauges may fall to zero after
completion, so applied settings, peer high water, and uTP packet counters are
joint evidence; total payload rather than wire bytes defines throughput;
first progress and completion must come from one monotonic clock; and a failed
case must never enter a cohort ratio.

The local JSTorrent reference at
`9895410beeed6aff554053769bd006a3fbd373ef` was inspected for product history:

- `packages/engine/integration/python/libtorrent_utils.py` and
  `test_seeder_leecher.py` explicitly disable uTP and provide only TCP oracle
  coverage; and
- `packages/engine/src/core/bt-engine.ts::enableUPnP` maps TCP plus an invented
  `port + 1` UDP/DHT port, not a product uTP listener.

JSTorrent therefore supplies no TCP/uTP result to inherit. Its useful lesson
is to keep discovery/setup milestones separate from payload movement. No
reference source, fixture, or test data is copied.

## Owner, Task, Cancellation, And Data Flow

```text
cohort owner (local Python process)
  -> one deterministic fixture and built RSTorrent seed binary
  -> at most four pair attempts, strictly sequential
       -> one ApplicationService seed process
            -> existing TCP listener + shared UDP/uTP socket
            -> existing reachability owner -> finite TCP + UDP leases
            -> one registered complete torrent
       -> remote leecher case A (forced transport)
       -> wait for product peer drain
       -> remote leecher case B (other forced transport)
       -> stop/join product -> delete/query both mappings
       -> remove remote run and local pair roots
  -> pure complete-pair aggregation and redacted JSON
```

The Python cohort owns orchestration, deadlines, processes, pair ordering,
temporary paths, and cleanup. The existing application service owns sockets,
peer generations, torrent reads, mapping leases, and joined shutdown. The
remote helper owns one libtorrent session and output root per case. No new
engine task, production metric, socket, application contract, setting, or
platform boundary is introduced.

## Invariants And Resource Bounds

- fixture: exactly 8,389,339 bytes, 33 pieces, one file, no tracker or web
  seed, and independently checked SHA-1;
- cohort: three required complete pairs, four maximum attempts, two sequential
  transfers per pair, one live remote peer maximum, and no concurrent pair;
- transport: exactly one of TCP/uTP enabled in each remote direction; no DHT,
  LSD, tracker, UPnP, NAT-PMP, MSE, proxy, or incoming peer source on the
  remote oracle;
- mapping: one finite product-owned TCP lease and one finite product-owned UDP
  lease maximum, each no longer than 3,600 seconds, verified and cleaned by
  protocol and exact port;
- time: 30 seconds for product/mapping readiness, 600 seconds per transfer,
  20 seconds for peer drain, 15 seconds for process shutdown, and 25 minutes
  per pair including cleanup;
- data: no more than 67,114,712 payload bytes across four pair attempts;
  temporary local data remains below 32 MiB and remote data below 80 MiB;
- observations: five monotonic milestones, bounded libtorrent statistics,
  bounded diagnostics, and aggregate product snapshots; no packet capture or
  per-packet production log; and
- security/privacy: commands use validated aliases and generated bounded
  paths; reports redact public/private endpoints, peer IDs, gateway URLs, and
  machine identity. Cleanup deletes only exact owned mappings and validated
  per-run temporary paths.

All network and metainfo input remains hostile. A remote value cannot select a
local path, mapping target, fixture size, timeout, cohort count, or cleanup
target.

## Measurement Contract

For each transport case retain:

- `connect_to_complete_seconds`: immediately before `connect_peer` through
  verified `is_seeding` completion;
- `first_payload_seconds`: connection start through first positive
  `total_wanted_done` observation;
- `active_payload_seconds`: first positive payload through verified
  completion;
- 25%, 50%, 75%, and completion milestone seconds from connection start;
- `active_mib_per_second` and `connect_mib_per_second` from exact payload bytes;
- exact payload bytes, pieces, SHA-1, peer high water, applied transport
  settings, final transport counters, and bounded diagnostics; and
- product cumulative payload and transport snapshots before/after the case.

The primary within-pair ratio is:

```text
uTP active MiB/s / TCP active MiB/s
```

The secondary ratio uses connection-to-completion rates. Report medians and
ranges only across the three complete pairs. Do not pool bytes and time across
pairs, discard slow successful samples, substitute whole-case orchestration
time, or infer a stable Internet capacity from this bounded cohort.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure Python | argument bounds, order schedule, fixture geometry, transport-specific settings/evidence, milestone monotonicity, pair aggregation, incomplete-pair rejection, redaction |
| Controlled local | one product seed accepts sequential forced TCP and uTP libtorrent leechers with exact hashes and peer drain; both mappings use scripted or real existing lifecycle ownership as applicable |
| Physical WAN | three complete 8 MiB pairs over the same ordinary public path, exact independent mapping queries, strict transport evidence, per-pair cleanup |
| Repository | Python compilation/unit tests, focused incoming-seed/session gates if touched, formatting, warning-denying clippy, diff cleanliness |

Android is inapplicable to this measurement-only harness: Tactical `140`
already proves the same product listener/mapping semantics and generated
status on both Android ABIs and API 34. This slice changes no Rust, generated
contract, packaging, lifecycle, or Android behavior. Any engine change exposed
by the measurement would require proportional Android evidence in its own
implementation tactical.

## Commit And Execution Sequence

1. commit this source-first tactical and select it as the sole **Now**;
2. generalize the remote helper and fixture behind backwards-compatible
   defaults, add pure contract tests, and commit;
3. add the product paired-WAN owner, exact dual-mapping and sequential-peer
   validation, aggregation, and cleanup tests, then commit;
4. run focused/local validation and the authorized physical cohort, preserving
   only redacted structured results; and
5. update this tactical, living topics, readiness queue, and campaign
   checkpoint, run proportional repository gates, and commit closure.

## Non-Goals And Next Boundary

This tactical does not tune uTP, change RFC 6817 behavior, alter TCP, add a
rate limit, select a speed threshold, claim WAN capacity, implement a new NAT
mechanism, use public DHT or trackers, run a public swarm, add IPv6 uTP or
MSE-over-uTP, change product policy, or produce a complete BEP 29 claim.

A clear, repeatable uTP deficit may select a separate diagnosis tactical. That
future slice would need to explain congestion-window growth, RTT/RTO, receive
credit, pacing, loss, packetization, and possible path shaping before changing
the transport. A noisy or incomplete cohort instead selects measurement or
environmental follow-up, not speculative optimization.

## Escalation Contract

No review is required for backwards-compatible harness refactoring, pure
measurement/cleanup tests, one clean replacement pair within the declared
budget, or the already authorized `pimom` runs.

Stop for human direction before changing engine transport behavior, product
policy or persistence, dependencies or license posture, gateway configuration
beyond exact temporary leases, the remote oracle installation, permanent
network/VPN state, another host, public-swarm participation, a destructive
action, or a broader protocol/support claim.
