# Capability Readiness

Topic: `capability-readiness`

Status: This is the master roll-up for current product and engine readiness.
RSTorrent now completes controlled v1 downloads through bounded simultaneous
peers, live tracker/DHT discovery, request expiry, and replacement, including
ordinary multi-piece single-file content. The active source-first campaign
now drives metadata, first-piece, sustained-transfer, endgame, and publication
parity through the completed paired libtorrent comparator before measured BEP
breadth. Endgame and v1 hash-failure recovery now have exact bounded evidence.
Bounded storage now remains live independently from peer events, but its
corrected localhost benchmark did not improve. Paired peer-utility timelines
then isolated a source-rich product-path run which retained 119 eligible peers
behind an eight-attempt half-open ceiling. That cohort is now 30 with exact
adversarial evidence. Fair supervisor intake now admits DHT results and fills
that cohort during storage pressure. Coalesced selective hashing now removes
redundant seeks, but a controlled profile remained neutral and public storage
occupancy stayed saturated. The complete common piece hash now runs behind one
bounded blocking positional-I/O boundary, but controlled and public timing
again stayed neutral. The duration slice attributed 93--94% of public wall
time to serialized storage service. Bounded coalesced batches now reduce
roughly 5,700 logical blocks to about 500 physical writes, but controlled wall
time stayed neutral and public write plus hash service still consumed 93--94%.
The engine campaign is paused before any bounded storage-concurrency tactical.
Tactical `033` now completes the leased application view-set and headless
polling foundation. Tactical `034` completes the responsive frontend,
deterministic demo adapter, adaptive inspection hierarchy, and virtual table
foundation. Tactical `035` completes stable Rust torrent and active-peer
inspection projections, the semantic live adapter, independently reaped
leases, and suspended-client recovery through a fresh snapshot. The next
product-observability cross-section should be selected from actual inspection
value before the engine campaign resumes. Tactical `039` also corrects the
product's inherited two-block pipeline: desktop and Android now use explicit,
generous, independently bounded request, received-payload, and active-piece
profiles. Tactical `040` adds durable archive and restartable torrent removal
across path and Android SAF storage, plus the guarded web confirmation flow.
Tactical `041` adds complete live file geometry and progress, Tactical `042`
publishes the verified metainfo name, and Tactical `043` adds an authoritative
schedule-backed tracker lifecycle through the responsive web surface and a
controlled tracker-only browser proof.

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

The completed DHT and multi-peer foundations now let late tracker or DHT
observations improve active content work. The current campaign and restart
checkpoint live in
[`oracle-driven-engine-campaign.md`](oracle-driven-engine-campaign.md). It
uses pinned libtorrent source and tests to choose coherent owner-level changes,
then validates them through deterministic, controlled, and paired public
evidence.

## Current Queue

### Now

**Migrate Tauri to the inspection application.** The production browser path
is usable through `./scripts/webui`, and Tactical
[`037`](../tactical/037-live-magnet-toolbar-intake.md) gives it bounded magnet
intake in addition to live inspection and torrent control. Tactical
[`038`](../tactical/038-curated-test-torrent-menu.md) adds exact shortcuts for
the retained WebTorrent public catalog without a privileged command path.
Files and tracker lifecycle are now live inspection feeds. Add
an in-process view-set adapter and make `./scripts/desktop` select the new React
surface without turning the loopback gateway into a desktop dependency.

### Next

1. **Design and connect categorized Logs.** Use JSTorrent's Logs tab and the
   current legacy logger as product references, preserving structured category
   and severity semantics rather than transplanting the legacy UI.
2. **Resume oracle-driven engine work.** Use the connected inspection evidence
   to decide whether serialized storage execution, another correctness owner,
   or measured BEP breadth becomes the next engine tactical.

### Later

Complete single-file durable resume, IPv6 DHT operation, incoming peer
listening, payload upload and seeding, PEX, local service discovery, uTP, NAT
traversal, v2 and hybrid torrents, playback-oriented file priorities, dynamic
VPN and metered-network controls, and production remote access remain
important. After core parity, common-denominator versus full-reference deltas
and the protocol evidence matrix choose BEP breadth; visible novelty alone
does not.

## Capability Scoreboard

### Input, Identity, And Metadata

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded bencode and v1 info dictionaries | Implemented | deterministic, interop | This is not complete outer `.torrent` ingestion; v2 and hybrid info dictionaries are rejected. | [`product-direction`](product-direction.md) |
| Product add from a v1 magnet | Implemented | deterministic, runtime, interop, web, AVD, physical | Only a v1 `btih` identity and supported magnet fields survive canonicalization. | [`client-persistence`](client-persistence.md) |
| BEP 9 metadata download | Implemented | deterministic, runtime, interop, live | One bounded torrent owner assembles blocks across up to eight workers, paces one-request-at-a-time peers, and recovers from expiry, rejection, and hash failure; tracker parity and catalog breadth pass, while paired DHT latency is blocked by the live reference. | [`peer-lifecycle`](peer-lifecycle.md) |
| Bounded diagnostic metadata upload | Implemented | deterministic, interop | It is not a general incoming listener or payload seeding service. | [`peer-lifecycle`](peer-lifecycle.md) |
| Product add from a `.torrent` file | Absent | deterministic parser only | The application command accepts magnets only and does not retain outer announce fields. | [`application-control`](application-control.md) |
| v2 and hybrid identity, metadata, and hashing | Absent | deterministic rejection | BEP 52 requires a separate integrity and storage design. | [`protocol-support`](protocol-support.md) |

### Discovery

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Explicit magnet peer hints | Implemented | deterministic, runtime, interop | Hints are bounded and feed the registry, but are not a general discovery mechanism. | [`peer-lifecycle`](peer-lifecycle.md) |
| Scheduled UDP tracker announces | Implemented | deterministic, runtime, interop, web, AVD, live | UDP connect/announce, fallback, backoff, retransmission, token reuse, reannounce, and bounded startup fan-out work; port 6881 is not actually bound. | [`tracker-discovery`](tracker-discovery.md) |
| Multiple magnet trackers | Partial | deterministic, runtime, interop, live | Up to eight startup operations contribute peers, but magnet trackers form one synthetic tier because magnets contain no BEP 12 tier structure. | [`tracker-discovery`](tracker-discovery.md) |
| Metainfo tracker tiers | Absent | none | Outer `announce` and `announce-list` are not retained by the product path. | [`tracker-discovery`](tracker-discovery.md) |
| HTTP and HTTPS trackers | Absent | none | No URL, transport, response, authentication, or redirect owner exists. | [`tracker-discovery`](tracker-discovery.md) |
| DHT | Partial | deterministic, runtime, interop, live | A bounded IPv4 participant supports lookup, incoming queries, private gating, revalidated warm restart, and repeated public metadata acquisition. IPv6 UDP operation and self-announcement are absent. | [`dht-discovery`](dht-discovery.md) |
| Peer exchange | Absent | none | BEP 11 depends on a larger live-peer set, extension dispatch, and hostile-source bounds. | [`peer-lifecycle`](peer-lifecycle.md) |
| Local service discovery | Absent | none | Interface, multicast, and local-network policy are unimplemented. | [`protocol-support`](protocol-support.md) |

### Peer And Swarm Lifecycle

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded peer registry and source merging | Implemented | deterministic, runtime | Records are volatile and peer-ID duplicate resolution is absent. | [`peer-lifecycle`](peer-lifecycle.md) |
| Deterministic dial selection and guarded attempts | Implemented | deterministic, runtime | Selection is intentionally basic; peer-ID duplicate resolution and measured scoring are absent. | [`peer-lifecycle`](peer-lifecycle.md) |
| Pre-content peer failover | Implemented | deterministic, runtime, interop, live | Bounded parallel metadata peers share one block owner; two tracker cohorts, 10/10 fresh-DHT owner runs, and 12/12 cross-catalog pairs pass. | [`peer-lifecycle`](peer-lifecycle.md) |
| Multiple simultaneous live peers | Implemented | deterministic, runtime, interop, live | Thirty established and thirty half-open attempts are separate torrent-local defaults with exact saturation and cancellation evidence; no session-wide connection budget exists. | [`peer-lifecycle`](peer-lifecycle.md) |
| Transfer request ownership and failover | Implemented | deterministic, runtime, interop, live | Ordinary blocks have one generation; strict endgame adds bounded duplicate attempts, first-response cancellation, and harmless losing payload. | [`download-correctness`](download-correctness.md) |
| Incoming peer connections | Absent | diagnostic metadata listener only | No bound product listen port, accept budget, torrent routing, NAT mapping, or shutdown policy exists; this is lower priority than correct outbound downloading. | [`peer-lifecycle`](peer-lifecycle.md) |
| Peer reputation and integrity attribution | Partial | deterministic, runtime, live | Exact connection generations receive bounded asymmetric trust; a sole corrupt source is banned and ambiguous sources are only suspected, while full parole selection and persistence are absent. | [`download-correctness`](download-correctness.md) |

### Content Transfer And Completion

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded 16 KiB block pipeline | Implemented | deterministic, runtime, interop, live | Per-connection depth adapts under distinct torrent request and resident-payload limits; desktop uses 256 MiB/32 MiB and Android 128 MiB/16 MiB, with no session-wide multi-torrent budget yet. | [`download-correctness`](download-correctness.md) |
| Sequential multi-piece download | Implemented | runtime, interop | Ordinary single-file and selective multi-file complete; single-file durable resume is absent. | [`download-correctness`](download-correctness.md) |
| Availability-aware piece selection | Partial | deterministic, runtime, interop | Swarm-wide availability, partial-first work, fairness, and unique-piece retention exist; rarest-first and measured scoring are absent. | [`download-correctness`](download-correctness.md) |
| Choke recovery | Implemented | deterministic, runtime, interop | Requests move to another peer and full choked sets are replaceable; mature choking/reputation policy is absent. | [`download-correctness`](download-correctness.md) |
| Per-request timeout and slow-peer handling | Implemented | deterministic, runtime | Useful response samples derive a bounded inactivity deadline and reduce a stalled peer to one probe; broader snub reputation remains absent. | [`download-correctness`](download-correctness.md) |
| Endgame | Implemented | deterministic, runtime, live | Strict duplicates, core cancels, late-loss safety, exact accounting, and public verified publication pass; throughput parity remains open. | [`download-correctness`](download-correctness.md) |
| Hash-failure recovery | Implemented | deterministic, runtime, interop, live | A failed v1 generation resets the whole piece with bounded contributors; v2 block-level recovery and full parole selection are absent. | [`download-correctness`](download-correctness.md) |
| Reliable completion on ordinary swarms | Partial | deterministic, runtime, interop, live | Multi-peer liveness, endgame, corrupt-generation retry, and bounded storage completion pass, but completion latency is not yet comparable and public corruption was not induced. | [`download-correctness`](download-correctness.md) |
| Payload upload and seeding | Absent | none | Request serving, choking, accounting, listening, and seed lifecycle are unimplemented. | [`protocol-support`](protocol-support.md) |

### Integrity, Storage, And Resume

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| SHA-1 piece verification before have state | Implemented | deterministic, runtime, interop | Failure resets only the attempted v1 piece and preserves unrelated verified state. | [`download-correctness`](download-correctness.md) |
| Multi-file mapping and selective files | Implemented | deterministic, runtime, interop | General product selection changes and priority scheduling are absent. | [`client-persistence`](client-persistence.md) |
| Cross-file, skipped-file, and padding storage | Implemented | deterministic, runtime, interop | BEP 47 symlinks are deliberately rejected. | [`client-persistence`](client-persistence.md) |
| Path-backed staging and publication | Implemented | runtime, interop | Disk-space policy, relocation, and broad filesystem failure profiles remain incomplete. | [`client-persistence`](client-persistence.md) |
| Bounded asynchronous content storage | Implemented | deterministic, runtime, interop, live | One torrent-local owner serializes writes and hashes with exact join; live duration evidence attributes about 88% of wall time to 16 KiB writes, so write execution is the active performance gap and this is not a session-wide disk scheduler. | [`download-correctness`](download-correctness.md) |
| Android SAF storage and publication | Implemented | runtime, AVD, physical | General root management, removable-media policy, and migration remain absent. | [`client-persistence`](client-persistence.md) |
| Durable have state and conservative recheck | Implemented | deterministic, runtime, interop, AVD, physical | It rehashes claimed pieces rather than providing optimized fast resume. | [`client-persistence`](client-persistence.md) |
| Recovery after content hash failure | Implemented | deterministic, runtime | Sole corrupt and ambiguous multi-source generations retry cleanly with bounded exact-generation attribution. | [`download-correctness`](download-correctness.md) |

### Application And Product Surfaces

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Durable semantic application control | Implemented | deterministic, runtime, web, AVD, physical | Archive and fenced keep/delete removal are implemented; stable public compatibility and general multi-torrent scheduling remain absent. | [`application-control`](application-control.md) |
| Leased application view sets and polling client | Implemented | deterministic, runtime, interop, web | Named summary, piece, diagnostic, active-peer, complete-file, tracker-lifecycle, and global Disk views have bounded replay/reset, independent lease expiry, and fresh-snapshot recovery; swarm views, streaming, Tauri migration, and stable public compatibility remain absent. | [`application-view-api`](application-view-api.md) |
| Shared web and Tauri desktop UI | Partial | runtime, interop, web, desktop | The responsive web surface adds magnets, archives, guarded removal, live peer/file/tracker inspection, and global Disk pressure; production desktop integration, `.torrent` file intake, and remaining detail feeds are incomplete. | [`client-surfaces`](client-surfaces.md) |
| Android Compose foreground client | Partial | runtime, AVD, physical | General settings, connectivity policy, and complete torrent controls remain incomplete. | [`client-surfaces`](client-surfaces.md) |
| Derived progress and bounded diagnostics | Implemented | deterministic, runtime, web, AVD | Scheduler and per-peer facts must grow with the corresponding owners. | [`application-control`](application-control.md) |
| Offline, loopback-only, and online egress policy | Implemented | deterministic, runtime, web, AVD | Policy is fixed for one service lifetime; Android VPN and metered-network controls are absent. | [`application-control`](application-control.md) |
| Headless product validation | Implemented | web, AVD | Physical devices and visible desktop automation still require explicit authorization. | [`client-surfaces`](client-surfaces.md) |
| Comparative live performance harness | Implemented | deterministic, interop, live | The headless catalog comparator is repeatable and pinned, but public speed remains a distribution rather than a CI threshold. | [`performance-and-live-evidence`](performance-and-live-evidence.md) |
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
