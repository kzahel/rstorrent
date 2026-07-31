# Capability Readiness

Topic: `capability-readiness`

Status: This is the master roll-up for current product and engine readiness.
It records implemented scope separately from evidence, keeps one explicit next
slice, and links to the topics and tacticals that own detail. RSTorrent can
complete controlled v1 downloads but is not yet a generally reliable torrent
client. A bounded IPv4 DHT foundation with useful warm restart is integrated;
the paired live comparator and multi-peer ownership are the next evidence and
reliability work.

## Purpose And Ownership

This topic answers four recurring questions:

- What can RSTorrent actually do now?
- What evidence supports each claim?
- Which missing capability presents the highest product or correctness risk?
- What bounded implementation slice should come next?

This is a roll-up, not a second source of detailed design truth. Focused topics
own their invariants and decisions, numbered tacticals own implementation
plans and execution records, and tests remain the executable evidence. This
topic links those records and states the current priority.

[`product-direction.md`](product-direction.md) owns durable product posture and
sequence. [`protocol-support.md`](protocol-support.md) owns BEP-level claims.
[`download-correctness.md`](download-correctness.md) owns completion, integrity,
and recovery scenarios. This topic owns the cross-cutting readiness view.

## Reading The Scoreboard

Implementation state and evidence are independent:

- **Absent** means the product has no usable implementation of the named
  capability. Supporting values or a test stub alone do not change this.
- **Partial** means a useful subset exists, but a named common path, failure
  mode, or owner remains missing.
- **Implemented** means the exact bounded scope stated in that row exists. It
  does not imply that the surrounding category or client is complete.

Evidence labels are a set, not a maturity score:

- **deterministic**: runtime-independent unit or state-transition tests;
- **runtime**: scripted sockets, storage, persistence, process death, or task
  lifecycle tests;
- **interop**: controlled exchange with an independent implementation;
- **web**: the shared web product surface driven in headless Chrome;
- **desktop**: a real Tauri shell smoke or lifecycle run;
- **AVD**: the Android Compose product surface on an owned no-window emulator;
- **physical**: an explicitly authorized Android or ChromeOS device run;
- **live**: a representative non-controlled swarm observation.

An observed outcome without enough captured state to reproduce or explain it
is recorded as an observation, not promoted to verified evidence.

## Priority Policy

Choose work in this order unless new evidence justifies an explicit change:

1. prevent data corruption, unsafe input effects, or security boundary leaks;
2. make an otherwise valid download complete under ordinary peer failure;
3. improve interoperability with common swarms and discovery sources;
4. bound or improve resource use, latency, and throughput; and
5. add protocol breadth and product convenience.

Within the highest applicable class, prefer a slice that has a deterministic
failure fixture, establishes an owner needed by later work, and has a
falsifiable end-to-end stopping condition. Keep exactly one item in **Now**
and no more than three in **Next**. A long inventory is useful; competing
current priorities are not.

The completed DHT foundation addressed a common discovery dependency and
established session-owned UDP and warm-restart state needed by later product
policy. Multi-peer request ownership remains immediately adjacent so the
transfer engine can exploit the broader peer set instead of depending on one
peer through metadata and content completion.

## Current Queue

### Now

The next tactical should be `015-headless-live-comparison`: add a bounded CLI
or application-service harness that runs RSTorrent and the pinned libtorrent
reference against one cataloged public torrent in isolated temporary profiles,
verifies completion, and emits a paired JSON result with timing and resource
metadata. It must not launch a visible desktop or mobile client.

The first mode uses only shared tracker and TCP capabilities. Tactical 016's
single-sided DHT smoke is useful evidence but does not replace this paired
result schema. Public speed ratios are recorded baselines, not flaky CI gates.
Detailed rules live in
[`performance-and-live-evidence.md`](performance-and-live-evidence.md).

### Next

1. **Bounded multi-peer request ownership.** Give blocks a torrent-level peer,
   generation, expiry, and reassignment owner; retain useful alternate peers
   across metadata and content failures.
2. **Endgame and integrity recovery.** Add bounded duplicate requests and
   cancel semantics, slow-request expiry, hash-failure reset, and contributor
   attribution sufficient to complete ordinary adverse swarms safely.
3. **Measured connection/picker policy.** Use paired public evidence to tune
   live-peer budgets, availability-aware selection, and resource high-water
   marks before broadening discovery again.

### Later

IPv6 DHT operation, incoming peer listening, payload upload and seeding, PEX,
local service discovery, uTP, NAT traversal, v2 and hybrid torrents,
playback-oriented file priorities, dynamic VPN and metered-network controls,
and production remote access remain important. They do not displace the
explicit current campaign merely because they are individually visible
features.

## Capability Scoreboard

### Input, Identity, And Metadata

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded bencode and v1 info dictionaries | Implemented | deterministic, interop | This is not complete outer `.torrent` ingestion; v2 and hybrid info dictionaries are rejected. | [`product-direction`](product-direction.md) |
| Product add from a v1 magnet | Implemented | deterministic, runtime, interop, web, AVD, physical | Only a v1 `btih` identity and supported magnet fields survive canonicalization. | [`client-persistence`](client-persistence.md) |
| BEP 9 metadata download | Implemented | deterministic, runtime, interop | Acquisition is not yet coordinated across simultaneous metadata peers. | [`peer-lifecycle`](peer-lifecycle.md) |
| Bounded diagnostic metadata upload | Implemented | deterministic, interop | It is not a general incoming listener or payload seeding service. | [`peer-lifecycle`](peer-lifecycle.md) |
| Product add from a `.torrent` file | Absent | deterministic parser only | The application command accepts magnets only and does not retain outer announce fields. | [`application-control`](application-control.md) |
| v2 and hybrid identity, metadata, and hashing | Absent | deterministic rejection | BEP 52 requires a separate integrity and storage design. | [`protocol-support`](protocol-support.md) |

### Discovery

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Explicit magnet peer hints | Implemented | deterministic, runtime, interop | Hints are bounded and feed the registry, but are not a general discovery mechanism. | [`peer-lifecycle`](peer-lifecycle.md) |
| Scheduled UDP tracker announces | Implemented | deterministic, runtime, interop, web, AVD | The implemented scope is UDP connect/announce with fallback, backoff, retransmission, token reuse, and reannounce; completion/stopped events and real transfer counters are absent. | [`tracker-discovery`](tracker-discovery.md) |
| Multiple magnet trackers | Partial | deterministic, runtime, interop | Magnet trackers form one synthetic tier because magnets contain no BEP 12 tier structure. | [`tracker-discovery`](tracker-discovery.md) |
| Metainfo tracker tiers | Absent | none | Outer `announce` and `announce-list` are not retained by the product path. | [`tracker-discovery`](tracker-discovery.md) |
| HTTP and HTTPS trackers | Absent | none | No URL, transport, response, authentication, or redirect owner exists. | [`tracker-discovery`](tracker-discovery.md) |
| DHT | Partial | deterministic, runtime, interop, live | A bounded IPv4 participant supports lookup, incoming queries, private gating, and revalidated warm restart. IPv6 UDP operation and self-announcement are absent; the first public metadata smoke discovered peers but did not complete. | [`dht-discovery`](dht-discovery.md) |
| Peer exchange | Absent | none | BEP 11 depends on a larger live-peer set, extension dispatch, and hostile-source bounds. | [`peer-lifecycle`](peer-lifecycle.md) |
| Local service discovery | Absent | none | Interface, multicast, and local-network policy are unimplemented. | [`protocol-support`](protocol-support.md) |

### Peer And Swarm Lifecycle

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded peer registry and source merging | Implemented | deterministic, runtime | Records are volatile and peer-ID duplicate resolution is absent. | [`peer-lifecycle`](peer-lifecycle.md) |
| Deterministic dial selection and guarded attempts | Implemented | deterministic, runtime | Selection is deliberately basic and permits only one live connection. | [`peer-lifecycle`](peer-lifecycle.md) |
| Pre-content peer failover | Implemented | runtime, interop | Failover covers connect, handshake, extension, and metadata failures, not content transfer. | [`peer-lifecycle`](peer-lifecycle.md) |
| Multiple simultaneous live peers | Absent | none | Tracker results cannot yet improve an active content transfer. | [`peer-lifecycle`](peer-lifecycle.md) |
| Transfer request ownership and failover | Absent | none | Blocks have no torrent-level peer, generation, or expiry owner. | [`download-correctness`](download-correctness.md) |
| Incoming peer connections | Absent | diagnostic metadata listener only | No advertised listen port, accept budget, torrent routing, or shutdown policy exists. | [`peer-lifecycle`](peer-lifecycle.md) |
| Peer reputation and integrity attribution | Absent | none | A bad piece is detected but contributors cannot be attributed or penalized. | [`download-correctness`](download-correctness.md) |

### Content Transfer And Completion

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded 16 KiB block pipeline | Implemented | deterministic, runtime, interop | It is owned by one piece and one peer connection at a time. | [`download-correctness`](download-correctness.md) |
| Sequential multi-piece download | Partial | runtime, interop | Multi-file torrents work; multi-piece single-file execution is rejected. | [`download-correctness`](download-correctness.md) |
| Availability-aware piece selection | Partial | deterministic, runtime | Pieces are traversed in index order against one peer; there is no swarm-wide picker. | [`download-correctness`](download-correctness.md) |
| Choke recovery | Partial | deterministic | Requests are released and may be resent to the same peer after unchoke; there is no alternate-peer assignment. | [`download-correctness`](download-correctness.md) |
| Per-request timeout and slow-peer handling | Absent | connection I/O deadlines only | Timely unrelated messages can keep a connection alive while a block remains stranded. | [`download-correctness`](download-correctness.md) |
| Endgame | Absent | duplicate rejection only | There are no bounded duplicate requests or cancel messages. | [`download-correctness`](download-correctness.md) |
| Hash-failure recovery | Absent | deterministic detection | A mismatch is terminal instead of resetting the piece for another attempt. | [`download-correctness`](download-correctness.md) |
| Reliable completion on ordinary swarms | Partial | runtime, interop (controlled only) | A user observed a torrent remain near 99.9%; multi-peer liveness and endgame are unproved. | [`download-correctness`](download-correctness.md) |
| Payload upload and seeding | Absent | none | Request serving, choking, accounting, listening, and seed lifecycle are unimplemented. | [`protocol-support`](protocol-support.md) |

### Integrity, Storage, And Resume

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| SHA-1 piece verification before have state | Implemented | deterministic, runtime, interop | Failure is detected but not recovered automatically. | [`download-correctness`](download-correctness.md) |
| Multi-file mapping and selective files | Implemented | deterministic, runtime, interop | General product selection changes and priority scheduling are absent. | [`client-persistence`](client-persistence.md) |
| Cross-file, skipped-file, and padding storage | Implemented | deterministic, runtime, interop | BEP 47 symlinks are deliberately rejected. | [`client-persistence`](client-persistence.md) |
| Path-backed staging and publication | Implemented | runtime, interop | Disk-space policy, relocation, and broad filesystem failure profiles remain incomplete. | [`client-persistence`](client-persistence.md) |
| Android SAF storage and publication | Implemented | runtime, AVD, physical | General root management, removable-media policy, and migration remain absent. | [`client-persistence`](client-persistence.md) |
| Durable have state and conservative recheck | Implemented | deterministic, runtime, interop, AVD, physical | It rehashes claimed pieces rather than providing optimized fast resume. | [`client-persistence`](client-persistence.md) |
| Recovery after content hash failure | Absent | deterministic detection | Verified state stays safe, but the active torrent does not retry the failed piece. | [`download-correctness`](download-correctness.md) |

### Application And Product Surfaces

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Durable semantic application control | Implemented | deterministic, runtime, web, AVD, physical | Removal, deletion, stable public compatibility, and general multi-torrent scheduling are absent. | [`application-control`](application-control.md) |
| Shared web and Tauri desktop UI | Partial | runtime, web, desktop | The shell and one-torrent flow work; production desktop integration and complete torrent controls do not. | [`client-surfaces`](client-surfaces.md) |
| Android Compose foreground client | Partial | runtime, AVD, physical | General settings, connectivity policy, and complete torrent controls remain incomplete. | [`client-surfaces`](client-surfaces.md) |
| Derived progress and bounded diagnostics | Implemented | deterministic, runtime, web, AVD | Scheduler and per-peer facts must grow with the corresponding owners. | [`application-control`](application-control.md) |
| Offline, loopback-only, and online egress policy | Implemented | deterministic, runtime, web, AVD | Policy is fixed for one service lifetime; Android VPN and metered-network controls are absent. | [`application-control`](application-control.md) |
| Headless product validation | Implemented | web, AVD | Physical devices and visible desktop automation still require explicit authorization. | [`client-surfaces`](client-surfaces.md) |
| Comparative live performance harness | Absent | none | Public observations are not yet repeatable or comparable with pinned libtorrent behavior. | [`performance-and-live-evidence`](performance-and-live-evidence.md) |
| Multi-torrent queue and resource budgets | Absent | none | The application can retain multiple records but has no mature concurrent scheduling policy. | [`application-control`](application-control.md) |

## Maintenance Contract

Every substantial tactical must update this topic when it changes a row,
evidence label, risk, or queue position. It must also update the focused owner,
the relevant correctness scenarios, and any affected BEP claims.

Every user-observed correctness failure gets a stable observation or scenario
entry before it can disappear into a generic backlog item. Closing it requires
either reproducible passing evidence or a recorded explanation that the
observation was caused outside the claimed product scope.

The **Now** item changes only when it completes, becomes invalidated by new
evidence, or is explicitly superseded. Completed tacticals remain execution
records; this topic should not accumulate their implementation narrative.
