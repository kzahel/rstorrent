# Capability Readiness

Topic: `capability-readiness`

Status: Authoritative cross-cutting feature-readiness scoreboard and current
work queue for the functional, unreleased alpha. The tables below record
current support, evidence, and highest-risk gaps; implementation history
remains in the linked tacticals and focused topics.

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

Tactical [`074`](../tactical/074-context-specific-metainfo-limits.md) is
complete. Generic bencode, BEP 9, durable metadata, and parser-only explicit
import now have independent byte and structure profiles; schema 7 admits the
existing 52,428-piece engine ceiling; deterministic, persistence, resource,
and controlled bidirectional libtorrent evidence pass. Product `.torrent`
intake remains absent.

Tactical [`075`](../tactical/075-ephemeral-application-state.md) is complete.
The application can now select private bounded in-memory session and metrics
stores, preserve the ordinary semantic and owner lifecycle while open, report
page exhaustion as a resource limit, and close without creating profile or
payload artifacts in the metadata-only case. Durable persistence behavior is
unchanged.

Tactical [`081`](../tactical/081-v1-torrent-byte-intake.md) records the
accepted persistent-source and v1 `.torrent` intake boundary. It is authorized
but not implemented: exact source and operational metadata remain distinct,
SQLite retains bounded original source bytes in durable or ephemeral mode,
metainfo tracker tiers become operational state, and one semantic byte
operation is adapted to WebSocket, HTTP automation, and raw Tauri IPC.

## Current Queue

### Now

**Execute Tactical
[`081`](../tactical/081-v1-torrent-byte-intake.md).** Implement bounded v1
outer-metainfo parsing and persistence, truthful source fidelity, 16-MiB
explicit/durable metadata, metainfo tracker tiers, one atomic semantic byte
operation, one-frame browser WebSocket intake, raw HTTP automation, raw Tauri
IPC, and controlled libtorrent evidence without adding visible picker UX.

### Next

- Add the shared browser/Tauri `.torrent` file picker and Add flow after
  Tactical `081` records one-frame latency and memory evidence; decide whether
  chunking is then justified rather than assuming it in the first byte path.

### Later

Complete IPv6 DHT operation, incoming peer listening, payload upload and
seeding, PEX, local service discovery, uTP, NAT
traversal, v2 and hybrid torrents, playback-oriented file priorities, dynamic
VPN and metered-network controls, and production remote access remain
important. After core parity, common-denominator versus full-reference deltas
and the protocol evidence matrix choose BEP breadth; visible novelty alone
does not.

## Capability Scoreboard

### Input, Identity, And Metadata

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded bencode and v1 info dictionaries | Implemented | deterministic, runtime, interop | Generic, BEP 9, durable, and 16-MiB parser-only explicit-import profiles independently bound bytes, decoded items, depth, collections, files, pieces, and paths. This is not product `.torrent` ingestion; v2 and hybrid info dictionaries are rejected. | [`protocol-support`](protocol-support.md) |
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
| Registry-backed Swarm inspection | Implemented | deterministic, runtime, interop, web | The bounded volatile registry, exact state counts, source merging, retry eligibility, and terminal cleanup are visible; durable history and peer-ID duplicate resolution remain absent. | [`peer-lifecycle`](peer-lifecycle.md), [`application-view-api`](application-view-api.md) |
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
| Sequential multi-piece download | Implemented | deterministic, runtime, interop | BEP 3 `length`, one-entry `files`, and ordinary multi-file torrents share one download, durable resume, repair, and publication pipeline. | [`download-correctness`](download-correctness.md) |
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
| Multi-file mapping and selective files | Implemented | deterministic, runtime, interop, web, AVD | Path and dynamic-SAF Normal/Skip routing, lazy part storage, boundary materialization, and metadata-only intake pass; high/low scheduling remains absent. | [`client-persistence`](client-persistence.md), [`download-correctness`](download-correctness.md), [`android-saf-storage`](android-saf-storage.md) |
| Cross-file, skipped-file, and padding storage | Implemented | deterministic, runtime, interop, web | Lazy part creation, retained lowered destinations, promotion, and empty-part cleanup pass; BEP 47 symlinks are deliberately rejected. | [`client-persistence`](client-persistence.md) |
| Path-backed staging and publication | Implemented | deterministic, runtime, interop | Explicit file/tree topology, hash-owned internal artifacts, durable publishing intent, atomic no-replace rename, namespace sync, crash reconciliation, and fail-closed removal pass. Disk-space policy, relocation, and broader filesystem/provider coverage remain incomplete. | [`client-persistence`](client-persistence.md), [`download-roots`](download-roots.md) |
| Bounded asynchronous content storage | Implemented | deterministic, runtime, interop, live | Payload sync and batched SQLite checkpoints use a separate bounded joined owner; immutable positional writes and hashes execute with independent bounds and explicit generation joins. Raw-stage sweeps, final defaults, Android concurrency evidence and multi-torrent/root fairness remain open. | [`storage-throughput-architecture`](storage-throughput-architecture.md) |
| Android SAF storage and publication | Implemented | deterministic, runtime, AVD, physical | The product uses lazy dynamic acquisition and one 40-handle path/SAF pool; the current API 34 rename-death run re-enters published full recheck at 40 owned handles and one pending request. General root management, cloud/removable policy, migration, and a current physical dynamic-provider rerun remain absent. | [`android-saf-storage`](android-saf-storage.md), [`client-persistence`](client-persistence.md) |
| Durable have state and conservative recheck | Implemented | deterministic, runtime, interop, AVD, physical | Startup and semantic force recheck hash all wanted managed pieces, recover valid false bits, clear stale true bits, and atomically replace have state; optimized fast resume is deliberately absent. | [`client-persistence`](client-persistence.md) |
| Recovery after content hash failure | Implemented | deterministic, runtime | Sole corrupt and ambiguous multi-source generations retry cleanly with bounded exact-generation attribution. | [`download-correctness`](download-correctness.md) |

### Application And Product Surfaces

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Durable semantic application control | Implemented | deterministic, runtime, web, AVD, physical | Archive, fenced keep/delete removal, metadata-only add, and joined live file selection are implemented; stable public compatibility and general multi-torrent scheduling remain absent. | [`application-control`](application-control.md) |
| Ephemeral application state | Implemented | deterministic, runtime | Private bounded session and metrics SQLite stores preserve receipts, metadata, settings, views, DHT and speed state for one joined service lifetime, then disappear without profile files; payload storage remains an external capability and has no RAM backend. | [`client-persistence`](client-persistence.md), [`application-control`](application-control.md) |
| Leased application view sets and delivery clients | Implemented | deterministic, runtime, interop, web, Tauri | Named summary, piece, structured diagnostic, active-peer, registry-backed Swarm, complete-file, tracker-lifecycle, global Disk, range-selected session Speed, and latest-value session DHT views have bounded replay/reset, independent lease expiry, fresh-snapshot recovery, diagnostic HTTP polling, acknowledged browser WebSocket streaming, and acknowledged in-process Tauri streaming. The retained observer matrices still expose Summary reset storms and trace/all-view serialization pressure; stable public compatibility remains unimplemented. | [`application-view-api`](application-view-api.md), [`application-connection-architecture`](application-connection-architecture.md) |
| Shared web and Tauri desktop UI | Partial | runtime, interop, web, desktop | The responsive surface now has Library, Transfers, and Workbench destinations, truthful bounded torrent-backed cards, shared multi-selection, magnet add and canonical copy, metadata-only add, live Normal/Skip file actions, archives, guarded removal, live peer/swarm/file/tracker inspection, global Disk pressure, bounded Canvas Pieces, a smooth exact session Speed history, and the exact routing-space DHT observatory; a real media catalog/playback and `.torrent` file intake remain incomplete. | [`client-surfaces`](client-surfaces.md), [`application-interface-direction`](application-interface-direction.md) |
| Authenticated private web host | Implemented | deterministic, runtime, web, live | One explicitly configured maintainer host serves the production React bundle and multiplexed application WebSocket behind bounded Basic authentication and exact HTTPS Origin checks. Exact-push isolated build, candidate smoke, supervised restart, authenticated private-listener/public verification, and rollback-on-failure pass; this is not a relay, account, pairing, encryption, or stable public compatibility claim. | [`application-connection-architecture`](application-connection-architecture.md), [`client-surfaces`](client-surfaces.md) |
| Android Compose foreground client | Partial | runtime, AVD, physical | General settings, connectivity policy, and complete torrent controls remain incomplete. | [`client-surfaces`](client-surfaces.md) |
| Derived progress and bounded diagnostics | Implemented | deterministic, runtime, interop, web, AVD | Structured hierarchical records, typed context, capture interest, explicit source/delivery/local loss, and the global ordered console are complete; scheduler and per-peer facts must grow with their corresponding owners. | [`application-control`](application-control.md) |
| Offline, loopback-only, and online egress policy | Implemented | deterministic, runtime, web, AVD | Policy is fixed for one service lifetime; Android VPN and metered-network controls are absent. | [`application-control`](application-control.md) |
| Headless product validation | Implemented | web, AVD | Physical devices and visible desktop automation still require explicit authorization. | [`client-surfaces`](client-surfaces.md) |
| Comparative live performance harness | Implemented | deterministic, interop, web, live | Named hardware profiles retain row-specific 1/10 GiB engine gates, per-view/adversarial application ratios, environment applicability and artifact-producing CI. The opt-in paired browser adapter smoke additionally records HTTP/WebSocket traffic and exact 1 GiB completion without defining a hard floor. The hosted-runner profile is deliberately broad and uncalibrated; public speed remains a distribution rather than a CI threshold. | [`performance-and-live-evidence`](performance-and-live-evidence.md) |
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
