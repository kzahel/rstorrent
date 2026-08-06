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
import gained independent byte and structure profiles; Tactical `081`
subsequently scales those profiles and adds product byte intake.

Tactical [`075`](../tactical/075-ephemeral-application-state.md) is complete.
The application can now select private bounded in-memory session and metrics
stores, preserve the ordinary semantic and owner lifecycle while open, report
page exhaustion as a resource limit, and close without creating profile or
payload artifacts in the metadata-only case. Durable persistence behavior is
unchanged.

Tactical [`078`](../tactical/078-local-single-peer-tcp-seeding.md) is complete.
One application-owned IPv4 loopback listener now routes bounded handshakes to
eligible complete path-backed torrents and serves verified BEP 9 metadata and
payload to one peer across download-task completion and durable application
restart. Scripted engine/application evidence and controlled libtorrent and
RSTorrent single-/multi-file transfers pass. This is not persisted settings,
multi-peer upload policy, listener advertisement, LAN/public binding, or NAT
reachability.

Tactical [`081`](../tactical/081-v1-torrent-byte-intake.md) is complete. Exact
source and operational metadata remain distinct; SQLite retains bounded
original source bytes in durable or ephemeral mode; metainfo tracker tiers are
operational state; and one semantic byte operation is adapted to WebSocket,
HTTP automation, and raw Tauri IPC. The implemented compatibility target
follows pinned libtorrent's v1 limits: 30-MiB BEP 9 receive, 64-MiB
explicit/durable/source input and local metadata upload, 2,097,152 pieces,
536,854,528-byte pieces, and measured calibration where parser, catalog, path,
view, or platform representations are not apples-to-apples.

Tactical
[`082`](../tactical/082-bounded-multi-peer-upload-ownership.md) is complete.
Incoming and outgoing sockets share one descriptor-aware session budget; eight
fixed upload slots, one optimistic grant, exact request/read/writer bounds, and
physical peer/torrent/session payload accounting now enforce multi-peer
seeding. Two RSTorrent and two libtorrent 2.0.13 clients overlapped against one
seed, independently verified 67,109,595 bytes each, and produced the exact
268,438,380-byte physical upload total. This explicitly directed completion
does not change the `Now` product-surface decision.

Tactical [`083`](../tactical/083-shared-torrent-file-picker.md) is complete.
Empty Add now opens the shared browser/Tauri single-file chooser, reuses the
root/start options, and submits one bounded `ArrayBuffer` through the active
adapter with initial selection `all`. Rust derives the source digest while
preserving legacy receipt replay. Headless Chrome proves one chooser, one
WebSocket binary frame, zero semantic HTTP calls, a visible imported row,
empty serious/critical axe findings, joined cleanup, and no metadata-only
payload artifacts.

Tactical
[`086`](../tactical/086-long-lived-torrent-peer-runtime.md) is complete. One
task-free torrent peer state and application-generation runtime now retain
ordinary peer authority across download and completed-seed lifetimes. Routed
incoming sockets populate the existing Peers/Swarm contract and compact flags.
An authenticated gateway run observed simultaneous pinned libtorrent 2.0.13
and RSTorrent rows while both independently verified 67,109,595 bytes, then
observed exact removal, inactive empty pause state, exact 134,219,190-byte
physical upload, and terminal zero ownership.

Tactical [`088`](../tactical/088-upnp-mapped-external-tcp-seeding.md) is
complete. Schema version 10 adds explicitly enabled local-network listening
and mapping policy while migrations stay disabled. One joined coordinator and
bounded IGD v2 `WANIPConnection:2` runtime install, query, renew, and delete a
finite TCP mapping. A controlled off-LAN peer directly hash-verified all 257
pieces and 4,195,035 payload bytes through the mapped public endpoint;
ordinary Peers/Swarm state, exact upload, independent deletion, failed
reconnect, and terminal-zero evidence pass.

Tactical [`089`](../tactical/089-coordinated-session-listen-sockets.md) is
complete. Schema version 11 adds the restart-applied preferred port with
default `6881`. One allocator holds coordinated TCP and UDP sockets through a
shared ten-conflict retry budget and system fallback; one 64-datagram session
UDP route feeds DHT. Controlled loopback and eligible local-network traffic
matches the separately reported TCP listener and DHT UDP source, while fixed
TCP failure retains independent DHT service and all tasks join terminally.

Tactical
[`095`](../tactical/095-bounded-http-https-tracker-transport.md) is complete.
UDP, HTTP, and HTTPS rows now share the long-lived tracker schedule and exact
session-wide eight-operation ceiling. Bounded HTTP/1.1, gzip, redirects,
Basic authentication, compact/noncompact IPv4/IPv6 peers, AAAA-only tracker
connectivity, only-`peers6` outbound transfer, controlled libtorrent
introduction, and Android HTTPS product evidence pass. HTTPS is deliberately
encrypted but unauthenticated; certificate and hostname validation is the
next security boundary. An opt-in official Ubuntu smoke also reached both
HTTPS rows and verified metadata. Tactical `096` repairs the metadata-only
activation gap exposed by that first smoke; its repeat reached both HTTPS
rows, verified metadata with no payload artifacts, and projected each actual
last-successful connection family.

## Current Queue

### Now

**Tactical
[`096`](../tactical/096-metadata-tracker-activation-and-family-observability.md)
is complete.** Metadata-only adds activate the existing discovery owner only
while their owned metadata task is live, then return to durable paused intent.
The actual last successful tracker connection family is visible without an
address. Controlled and official Ubuntu gates pass with no payload artifacts.

### Next

- After live settings Tactical
  [`097`](../tactical/097-live-client-settings-and-replaceable-session-generations.md),
  execute planned Tactical
  [`098`](../tactical/098-authenticated-https-tracker-platform-trust.md) to add
  default authenticated tracker HTTPS through desktop and Android platform
  trust, one hidden compatibility policy, and explicit failure/projection
  evidence before making a secure-HTTPS claim.
- Add PCP and NAT-PMP only with their own bounded tactical and suitable
  controlled or physical gateway evidence; pinned source inspection is not a
  support claim.

### Later

Complete IPv6 DHT operation, live listener/upload-setting application, finite
bandwidth and seeding goals, multi-interface and IPv6 binding, PEX,
local service discovery, uTP, NAT traversal, v2 and hybrid torrents,
playback-oriented file priorities, dynamic VPN and metered-network controls,
and production remote access remain important. After core parity,
common-denominator versus full-reference deltas and the protocol evidence
matrix choose BEP breadth; visible novelty alone does not.

Availability-ranked activation is complete in Tactical
[`091`](../tactical/091-availability-ranked-piece-activation.md). The remaining
three planned post-**Now** download-liveness slices retain this default order:
peer-ID duplicate resolution in Tactical
[`090`](../tactical/090-peer-id-duplicate-connection-resolution.md), the complete
BEP 6 request lifecycle in Tactical
[`093`](../tactical/093-bep6-fast-request-lifecycle.md), and bounded BEP 11 PEX
in Tactical [`094`](../tactical/094-bounded-bep11-peer-exchange.md). Full snub
and parole selection remain evidence-gated rather than preplanned slices.

## Capability Scoreboard

### Input, Identity, And Metadata

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded bencode and v1 info dictionaries | Implemented | deterministic, runtime, interop | Generic, 30-MiB peer BEP 9, and 64-MiB explicit/durable/local-upload profiles independently bound bytes, decoded items, depth, collections, files, pieces, paths, and trackers. Product v1 `.torrent` ingestion passes; v2 and hybrid info dictionaries are rejected. | [`protocol-support`](protocol-support.md) |
| Product add from a v1 magnet | Implemented | deterministic, runtime, interop, web, AVD, physical, live | Only a v1 `btih` identity and supported magnet fields survive canonicalization. Controlled tracker-only and official Ubuntu metadata-only paths activate discovery during acquisition, remain durably paused, and create no payload artifacts. | [`client-persistence`](client-persistence.md) |
| BEP 9 metadata download | Implemented | deterministic, runtime, interop, live | One bounded torrent owner assembles blocks across up to eight workers, accepts an authoritative piece-zero size up to 30 MiB, and recovers from expiry, rejection, and hash failure. Pinned libtorrent transfers the exact 31,457,280-byte maximum profile in 1,920 blocks. | [`peer-lifecycle`](peer-lifecycle.md) |
| Bounded metadata upload | Implemented | deterministic, runtime, interop | The diagnostic server remains metadata-only; the application listener shares immutable registration-owned metadata across bounded incoming peers and serves every requested 16-KiB block of valid local metadata up to the 64-MiB profile. | [`incoming-reachability-and-seeding`](incoming-reachability-and-seeding.md), [`peer-lifecycle`](peer-lifecycle.md) |
| Product add from a `.torrent` file | Implemented | deterministic, runtime, interop, web, Tauri | One atomic 64-MiB byte operation preserves exact source, operational info and tracker tiers across restart through HTTP, WebSocket, and raw Tauri IPC. Empty Add opens the shared single-file chooser, reuses root/start options, sends selection `all`, and requires no caller digest or secure context. | [`application-control`](application-control.md) |
| v2 and hybrid identity, metadata, and hashing | Absent | deterministic rejection | BEP 52 requires a separate integrity and storage design. | [`protocol-support`](protocol-support.md) |

### Discovery

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Explicit magnet peer hints | Implemented | deterministic, runtime, interop | Hints are bounded and feed the registry, but are not a general discovery mechanism. | [`peer-lifecycle`](peer-lifecycle.md) |
| Scheduled UDP tracker announces | Implemented | deterministic, runtime, interop, web, AVD, live | One long-lived session owner provides UDP connect/announce, fallback, backoff, retransmission, token reuse, interval/corrective reannounce, exact counters, started/completed/stopped lifecycle, the selected TCP endpoint or port-`1` sentinel, and an eight-operation ceiling shared with HTTP/HTTPS. Controlled tracker-only and mapped off-LAN discovery-to-seed evidence passes. | [`tracker-discovery`](tracker-discovery.md) |
| Multiple magnet trackers | Partial | deterministic, runtime, interop, live | Up to eight startup operations contribute peers, but magnet trackers form one synthetic tier because magnets contain no BEP 12 tier structure. | [`tracker-discovery`](tracker-discovery.md) |
| Metainfo tracker tiers | Implemented | deterministic, runtime, interop, web | Outer `announce-list`/`announce`, tier and source survive restart; UDP/HTTP/HTTPS rows share tier scheduling and the eight-operation ceiling, and controlled imported trackers complete content. | [`tracker-discovery`](tracker-discovery.md) |
| HTTP and HTTPS trackers | Implemented | deterministic, runtime, interop, web, AVD, live | Bounded HTTP/1.1 requests, Basic auth, five redirects, gzip/`x-gzip`, permissive hostile bencode, tracker IDs and BEP 31, compact/noncompact IPv4/IPv6 peers, policy/family DNS, lifecycle/cancellation, metadata-only activation, last-successful connection-family projection, controlled libtorrent discovery, and official Ubuntu HTTPS metadata smokes pass. HTTPS encrypts transport but deliberately does not validate certificates or hostnames; proxies, scrape, other authentication, and a public reliability claim are absent. | [`tracker-discovery`](tracker-discovery.md) |
| DHT | Partial | deterministic, runtime, interop, live | A bounded IPv4 participant supports lookup, incoming queries, private gating, revalidated warm restart, repeated public metadata acquisition, and verified-public self-announcement of the selected explicit TCP port to K=8 token-bearing nodes. One session scheduler survives download completion; controlled DHT-only and mapped off-LAN seed discovery pass. IPv6 UDP operation remains absent. | [`dht-discovery`](dht-discovery.md) |
| Peer exchange | Absent | none | BEP 11 depends on peer-ID duplicate resolution, truthful advertisement, bounded extension dispatch, and hostile-source controls recorded in planned Tactical [`094`](../tactical/094-bounded-bep11-peer-exchange.md). | [`peer-lifecycle`](peer-lifecycle.md), [`protocol-support`](protocol-support.md) |
| Local service discovery | Absent | none | Interface, multicast, and local-network policy are unimplemented. | [`protocol-support`](protocol-support.md) |

### Peer And Swarm Lifecycle

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded peer registry and source merging | Implemented | deterministic, runtime | Records are volatile and peer-ID duplicate resolution is absent; planned Tactical [`090`](../tactical/090-peer-id-duplicate-connection-resolution.md) keeps endpoint records separate while admitting one live connection per peer ID. | [`peer-lifecycle`](peer-lifecycle.md) |
| Registry-backed Swarm inspection | Implemented | deterministic, runtime, interop, web | The bounded volatile registry, exact state counts, source merging, retry eligibility, and terminal cleanup are visible; durable history and peer-ID duplicate resolution remain absent. Planned Tactical [`090`](../tactical/090-peer-id-duplicate-connection-resolution.md) closes only the live-connection gap. | [`peer-lifecycle`](peer-lifecycle.md), [`application-view-api`](application-view-api.md) |
| Deterministic dial selection and guarded attempts | Implemented | deterministic, runtime | Selection is intentionally basic; peer-ID duplicate resolution and measured scoring are absent. Planned Tactical [`090`](../tactical/090-peer-id-duplicate-connection-resolution.md) adds post-handshake duplicate admission without introducing peer scoring. | [`peer-lifecycle`](peer-lifecycle.md) |
| Pre-content peer failover | Implemented | deterministic, runtime, interop, live | Bounded parallel metadata peers share one block owner; two tracker cohorts, 10/10 fresh-DHT owner runs, and 12/12 cross-catalog pairs pass. | [`peer-lifecycle`](peer-lifecycle.md) |
| Multiple simultaneous live peers | Implemented | deterministic, runtime, interop, live | Thirty established and thirty half-open attempts remain separate outbound torrent-local defaults beneath one shared session budget whose ordinary default is 200 after descriptor clamping and whose incoming-only slack is ten. Exact saturation, cancellation, mixed-direction release, and simultaneous incoming evidence pass. | [`peer-lifecycle`](peer-lifecycle.md) |
| Transfer request ownership and failover | Implemented | deterministic, runtime, interop, live | Ordinary blocks have one generation; strict endgame adds bounded duplicate attempts, first-response cancellation, and harmless losing payload. | [`download-correctness`](download-correctness.md) |
| Incoming peer connections | Implemented | deterministic, runtime, interop, web | One joined IPv4 listener has a five-entry backlog, eight pending handshake slots, 1,024 generation-fenced registrations, and bounded multi-peer ownership under the shared effective-plus-ten-slack budget. Typed loopback/local-network disabled/automatic/fixed policy, explicit UPnP enablement, preferred port `1024..=65535`, 1--2,000 peers, and 0--50 slots persist atomically and currently apply across restart. Automatic TCP/UDP binding shares ten retries before system fallback, while fixed binding is exact and atomic; actual endpoints remain runtime facts. Tacticals [`086`](../tactical/086-long-lived-torrent-peer-runtime.md), [`088`](../tactical/088-upnp-mapped-external-tcp-seeding.md), and [`089`](../tactical/089-coordinated-session-listen-sockets.md) prove retained incoming ownership, exact mapped off-LAN seeding, and coordinated transport. Tactical [`092`](../tactical/092-truthful-tracker-and-dht-peer-advertisement.md) proves tracker-only, DHT-only, and mapped wire-port discovery through that listener with joined terminal cleanup. Planned Tactical [`097`](../tactical/097-live-client-settings-and-replaceable-session-generations.md) owns live rebind, mapping, admission, and slot convergence without replacing stable peers, DHT, or discovery. | [`incoming-reachability-and-seeding`](incoming-reachability-and-seeding.md), [`peer-lifecycle`](peer-lifecycle.md) |
| Peer reputation and integrity attribution | Partial | deterministic, runtime, live | Exact connection generations receive bounded asymmetric trust; a sole corrupt source is banned and ambiguous sources are only suspected, while full parole selection and persistence are absent. | [`download-correctness`](download-correctness.md) |

### Content Transfer And Completion

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded 16 KiB block pipeline | Implemented | deterministic, runtime, interop, live | Per-connection depth adapts under distinct torrent request and resident-payload limits; desktop uses 256 MiB/32 MiB and Android 128 MiB/16 MiB, with no session-wide multi-torrent budget yet. | [`download-correctness`](download-correctness.md) |
| Sequential multi-piece download | Implemented | deterministic, runtime, interop | BEP 3 `length`, one-entry `files`, and ordinary multi-file torrents share one download, durable resume, repair, and publication pipeline. | [`download-correctness`](download-correctness.md) |
| Availability-aware piece selection | Implemented | deterministic, runtime, interop, performance | Requestable active work remains first; exact live nonseed counts plus a separate seed count feed a compact incrementally indexed rarest-first default with an in-order baseline. Independent count, byte, peer-ratio, and block-pressure limits pass hostile maximum-geometry and release CPU/memory gates; full request windows skip inactive lookahead work; unique unplanned pieces remain protected; and controlled libtorrent verifies the sole scarce piece first. Streaming urgency, user picker controls, reverse rarity for snubbed peers, and parole remain absent. | [`download-correctness`](download-correctness.md) |
| Choke recovery | Implemented | deterministic, runtime, interop | Requests move to another peer and full choked sets are replaceable; mature choking/reputation policy is absent. | [`download-correctness`](download-correctness.md) |
| Per-request timeout and slow-peer handling | Implemented | deterministic, runtime | Useful response samples derive a bounded inactivity deadline and reduce a stalled peer to one probe; broader snub reputation remains absent. | [`download-correctness`](download-correctness.md) |
| Endgame | Implemented | deterministic, runtime, live | Strict duplicates, core cancels, late-loss safety, exact accounting, and public verified publication pass; throughput parity remains open. | [`download-correctness`](download-correctness.md) |
| Hash-failure recovery | Implemented | deterministic, runtime, interop, live | A failed v1 generation resets the whole piece with bounded contributors; v2 block-level recovery and full parole selection are absent. | [`download-correctness`](download-correctness.md) |
| Reliable completion on ordinary swarms | Partial | deterministic, runtime, interop, live | Multi-peer liveness, endgame, corrupt-generation retry, and bounded storage completion pass, but completion latency is not yet comparable and public corruption was not induced. | [`download-correctness`](download-correctness.md) |
| Payload upload and seeding | Implemented | deterministic, runtime, interop, web | Completed published path storage serves exact verified/readable bitfields and bounded 16-KiB requests to multiple peers under currently restart-applied 0--50 session slots, with default eight and one optimistic grant, 2,000 descriptors per peer, ten reads, the shared 40-handle pool, and a 528,396-byte/64-descriptor writer bound. Exact multi-peer accounting passes locally and for a 4,195,035-byte direct off-LAN transfer through a verified UPnP mapping. Planned Tactical [`097`](../tactical/097-live-client-settings-and-replaceable-session-generations.md) owns live slot regrant without losing peer state or counters. Finite bandwidth, ratio/time goals, incomplete-torrent upload, platform-storage evidence, and discovery-driven public seeding remain absent. | [`incoming-reachability-and-seeding`](incoming-reachability-and-seeding.md), [`protocol-support`](protocol-support.md) |

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
| Durable semantic application control | Implemented | deterministic, runtime, interop, web, Tauri, AVD, physical | Archive, fenced keep/delete removal, metadata-only add, atomic v1 torrent-byte add, and joined live file selection are implemented; stable public compatibility and general multi-torrent scheduling remain absent. | [`application-control`](application-control.md) |
| Ephemeral application state | Implemented | deterministic, runtime | Private bounded session and metrics SQLite stores preserve receipts, exact source, metadata, settings, views, DHT and speed state for one joined service lifetime, then disappear without profile files. One maximum source plus info fits the 256-MiB session cap and a second maximum import rolls back with a typed resource limit; payload storage remains external. | [`client-persistence`](client-persistence.md), [`application-control`](application-control.md) |
| Leased application view sets and delivery clients | Implemented | deterministic, runtime, interop, web, Tauri | Named summary, piece, structured diagnostic, active-peer, registry-backed Swarm, paged file and tracker, global Disk, range-selected session Speed, and latest-value session DHT views have bounded replay/reset, independent lease expiry, fresh-snapshot recovery, diagnostic HTTP polling, acknowledged browser WebSocket streaming, and acknowledged in-process Tauri streaming. The retained observer matrices still expose Summary reset storms and trace/all-view serialization pressure; stable public compatibility remains unimplemented. | [`application-view-api`](application-view-api.md), [`application-connection-architecture`](application-connection-architecture.md) |
| Shared web and Tauri desktop UI | Partial | runtime, interop, web, desktop | The responsive surface now has Library, Transfers, and Workbench destinations, truthful bounded torrent-backed cards, shared multi-selection, magnet and local `.torrent` add, canonical magnet copy, metadata-only add, live Normal/Skip file actions, archives, guarded removal, live peer/swarm/file/tracker inspection, global Disk pressure, bounded Canvas Pieces, a smooth exact session Speed history, and the exact routing-space DHT observatory. A real media catalog/playback remains incomplete. | [`client-surfaces`](client-surfaces.md), [`application-interface-direction`](application-interface-direction.md) |
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
