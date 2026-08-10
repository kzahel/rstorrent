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
complete. Schema version 11 adds the preferred port with default `6881`. One
allocator holds coordinated TCP and UDP sockets through a shared ten-conflict
retry budget and system fallback; one 64-datagram session UDP route feeds DHT.
Controlled loopback and eligible local-network traffic matches the separately
reported TCP listener and DHT UDP source, while fixed TCP failure retains
independent DHT service and all tasks join terminally. Tactical `097` now
applies that transport policy live.

Tactical
[`095`](../tactical/095-bounded-http-https-tracker-transport.md) is complete.
UDP, HTTP, and HTTPS rows now share the long-lived tracker schedule and exact
session-wide eight-operation ceiling. Bounded HTTP/1.1, gzip, redirects,
Basic authentication, compact/noncompact IPv4/IPv6 peers, AAAA-only tracker
connectivity, only-`peers6` outbound transfer, controlled libtorrent
introduction, and Android HTTPS product evidence pass. HTTPS is deliberately
encrypted but unauthenticated at that historical checkpoint. An opt-in
official Ubuntu smoke also reached both HTTPS rows and verified metadata.
Tactical `096` repairs the metadata-only activation gap exposed by that first
smoke; its repeat reached both HTTPS rows, verified metadata with no payload
artifacts, and projected each actual last-successful connection family.

Tactical
[`098`](../tactical/098-authenticated-https-tracker-platform-trust.md) is
complete. Schema version 12 defaults every profile to desktop or Android
platform trust, retains one hidden explicit compatibility value, and applies
it live through the existing session reconciler and bounded family client
pair. Generated certificate/name failures, construction fencing, captured
in-flight generations, redacted categories, cross-platform runtimes, Android
packaging/product behavior, and authenticated HTTPS tracker introduction to
pinned libtorrent all pass. The current AVD rejected the credential-free
Ubuntu tracker certificate while accepting another public trusted origin, so
that observation does not become a public-tracker reliability claim.

Tactical [`111`](../tactical/111-mse-peer-stream-encryption.md) is complete.
TCP initiator and responder roles support both negotiated MSE/PE payload
methods under one live persisted four-value policy, bounded session DH work,
exact diagnostics, and the truthful encrypted-or-obfuscated peer flag. All 28
controlled pinned-libtorrent cases, six paired 1 GiB performance runs per
implementation, Android cross-build, API 34 AVD, and API 37 physical Pixel 7a
product evidence pass. The physical run completed five forced-RC4 sessions,
exact publication, a three-job DH high-water under the four-job ceiling, full
owner drain, bounded storage/descriptors, and cleanup. The claim is protocol
compatibility and obfuscation, never transport security.
Completed bounded follow-up Tactical
[`115`](../tactical/115-mse-policy-advertisement-and-peer-detail.md) aligns
default incoming method selection with stock libtorrent, adds live-policy
HTTP tracker capability announcement, and exposes exact method detail through
the existing quiet `E` presentation. Its 29-case controlled matrix passes; it
adds no setting, Android Compose work, uTP support, or broader protocol claim.

Tactical
[`112`](../tactical/112-dual-stack-transport-and-ipv6-dht.md) is complete.
One session transport generation now owns an independent TCP/UDP pair per
enabled family; one DHT actor owns separate IPv4/IPv6 nodes and persisted
state; tracker and DHT advertisement select the same-family listener port; and
one default-enabled persisted policy gates all IPv6 ingress, discovery, and
dials. Controlled IPv6 DHT-only libtorrent discovery/download, a public
dual-family metadata run, web/Tauri contract coverage, both Android ABIs, and
an API 34 AVD degradation/restart profile pass. The named API 37 Pixel 7a
subsequently passed the same default, disable, forced-restart persistence,
degraded re-enable, and cleanup assertions on its current no-eligible-address
network. No IPv6 pinhole or incoming-reachability claim is made.

Tactical
[`114`](../tactical/114-session-wide-concurrent-torrent-admission.md) is
complete. Schema 17 persists one automatic download order and a default-three
active limit; the application restores every runnable intent and admits a
bounded active-generation map. Request, payload, active-piece, storage/hash,
tracker, outbound-turn, connection, and file-handle authority is session-wide.
One-/two-torrent performance gates pass, 1/2/3/4/8 saturation is recorded,
100 runnable and 500 complete catalog scales remain bounded, shared browser/
Tauri controls pass headlessly, and a physical Pixel 7a proves Android's
configured-three/effective-two clamp, promotion, exact payload, and terminal
resource cleanup.

Tactical
[`116`](../tactical/116-platform-storage-coherence-and-ios-feasibility.md) is
complete. Path and supported Android SAF storage now share logical artifact
geometry, typed observations, root-health admission, published reads, the
40-handle pool, and explicit namespace transitions. Full AVD and physical
Android matrices pass, including exact SAF-backed upload and cleanup. A
physical-iPhone harness proves the real Rust storage and direct-network seams,
bookmark restoration, coordination, and bounded lifecycle behavior for an
app-owned fixture; external File Provider support remains unproven. No fast-
resume trust decision was added.

Tactical
[`123`](../tactical/123-ios-on-device-root-persistence-and-recovery.md) is
complete with the accepted app-owned-only outcome. A versioned opaque probe
root, fail-closed eligibility state, per-operation coordination, exact partial-
workspace recovery, and balanced resource evidence pass on a physical iPhone.
The required separate On My iPhone and iCloud controls could not run because
physical XCTest automation timed out and iPhone Mirroring was unavailable, so
picker registration is compiled off and no provider support is claimed.

Tactical
[`117`](../tactical/117-jstorrent-shaped-android-product-ui.md) is complete.
The maintained Compose product now follows JSTorrent Android standalone's
Library, six-tab torrent detail, Speed, DHT, Logs, and Settings hierarchy with
RSTorrent branding. It consumes the existing bounded application views and
commands through one service-scoped presentation owner, adds bounded raw
`.torrent` intake and completed-file launch, and labels unsupported engine or
platform policy unavailable. Workspace, generated two-ABI, Gradle lint/unit,
API 34 Compose navigation, and controlled SAF/concurrent-download evidence
pass; no new physical-device UI claim was made.

Tactical
[`119`](../tactical/119-deterministic-utp-transport-core.md) is complete.
The dependency-free protocol crate now owns a hostile bounded uTP v1 codec,
explicit wrapping arithmetic, exact initiating and accepting connection IDs,
64-packet/1-MiB receive ordering, a 1,024-packet/1-MiB sent ledger,
cumulative/SACK acknowledgement and loss signals, Karn-safe RTT/RTO state,
bounded retransmission attempts, FIN close readiness, and terminal cleanup.
All 41 focused uTP tests and the complete Rust workspace baseline pass. This
is pure state evidence only: no RSTorrent uTP datagram, runtime stream,
interoperability, WAN path, product behavior, or support claim exists.

Tactical
[`121`](../tactical/121-deterministic-utp-loss-congestion-and-mtu.md) is
complete at its required pre-runtime review. The protocol crate now composes
exact receive credit, bounded stream packetization, delayed ACKs,
retransmission execution, fixed-point RFC 6817 congestion/pacing, and binary
path-MTU discovery. Fixed exact-hash scenarios pass clean, impaired, 1% loss,
queue, clock, receive-pressure, black-hole, and TCP-like foreground gates.
This remains deterministic state only: peer execution is still TCP, and no
RSTorrent uTP socket, task, ordered stream, interoperability exchange, WAN
path, product behavior, or support claim exists.

Tactical
[`125`](../tactical/125-shared-udp-utp-runtime-and-loopback-interop.md) is
complete at its required post-Stage 3 review. One shared session UDP receiver
now isolates bounded DHT/uTP routes; one supervised runtime owns generation-
fenced connections and ordered streams; and one concrete peer-stream boundary
feeds controlled plaintext uTP into common framing and incoming peer/upload
owners. Ten runtime cases, twelve shared-UDP cases, and both pinned-libtorrent
roles pass. Each role transfers the exact 2,097,883-byte fixture with one
loopback uTP peer, zero TCP peers, exact SHA-1, bounded high-waters, no drops or
worker panics, and terminal zero ownership. This adds no WAN evidence, product
selection/listener, UDP mapping, IPv6 uTP, MSE-over-uTP, or support claim.

Tactical
[`120`](../tactical/120-per-torrent-trusting-fast-resume.md) is complete.
Ordinary coherent path and supported local SAF resumes now validate exact
per-torrent structure and trust only synchronized committed bits with zero
payload reads or hashes. Structural disagreement invokes the unchanged full
checker only for that torrent; unavailable or malformed ownership retains its
existing recovery state. Force and pending verification remain full. Three
checkpoint-death boundaries, a stable completed neighbor, 500 completed seeds
beside three downloads, same-length mutation plus Force, the complete pinned-
libtorrent oracle, both Android builds, and two API 34 AVD lifecycle gates
pass. No schema, setting, provider, physical-device, or protocol claim was
added.

## Current Queue

### Now

- Tactical
  [`126`](../tactical/126-controlled-outbound-utp-wan-evidence.md) is the
  single authoritative implementation/evidence slice. Human review accepted
  Stage 4 recommendation A and authorized `ssh pimom` for orchestration plus
  one RSTorrent-outbound forced-uTP WAN case. The data path must be direct
  routed IPv4 rather than SSH, LAN, VPN, or overlay; read-only preflight closes
  the tactical evidence-limited if that endpoint or exact libtorrent runtime
  is absent. No mapping/pinhole, reverse incoming traffic, IPv6 uTP, MSE-over-
  uTP, product policy, dependency, public swarm, or support claim is
  authorized.

### Next

- Finite session/per-torrent bandwidth enforcement and durable seeding goals
  are the next policy-planning candidates. They remain separate tacticals:
  rate allocation owns hot-path fairness and cancellation, while ratio/time
  goals own durable accounting and torrent lifecycle. Neither is authorized
  for implementation by Tactical `124`'s completion.

### Later

Finite bandwidth and seeding goals,
multi-interface and BEP 45 multi-address binding,
local service discovery,
NAT traversal, v2 and hybrid torrents,
playback-oriented file priorities, dynamic
VPN and metered-network controls, verified HTTP file serving, incomplete-file
streaming, and production remote access remain
important. Tactical `112` now owns IPv6 DHT operation and dual-stack
listening. Closed Tactical `113` implements IPv6 firewall-pinhole control but
records positive physical capability as unknown on the current hardware after
the live gateway returned typed `606` to `AddPinhole`.
The uTP topic records the adaptive campaign. Tacticals `118`, `119`, `121`,
and `125` are complete; the first bounded runtime plus independent loopback
exchange passes. Tactical `126` now owns direct-route preflight and one
outbound-only controlled WAN attempt. uTP remains unsupported and disabled in
products.
Tactical
[`100`](../tactical/100-bep53-select-only-and-duplicate-add-feedback.md)
completed the BEP 53 slice and its deliberately narrow duplicate-add product
policy. After core parity,
common-denominator versus full-reference deltas and the protocol evidence
matrix choose BEP breadth; visible novelty alone does not.

PCP and NAT-PMP require their own bounded tactical and suitable controlled or
physical gateway evidence; pinned source inspection is not a support claim.

Availability-ranked activation, the complete BEP 6 request lifecycle, and
bounded BEP 11 PEX are complete in Tacticals
[`091`](../tactical/091-availability-ranked-piece-activation.md) and
[`093`](../tactical/093-bep6-fast-request-lifecycle.md), and
[`094`](../tactical/094-bounded-bep11-peer-exchange.md). Full snub
and parole selection remain evidence-gated rather than preplanned slices.

## Capability Scoreboard

### Input, Identity, And Metadata

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded bencode and v1 info dictionaries | Implemented | deterministic, runtime, interop | Generic, 30-MiB peer BEP 9, and 64-MiB explicit/durable/local-upload profiles independently bound bytes, decoded items, depth, collections, files, pieces, paths, and trackers. Product v1 `.torrent` ingestion passes; v2 and hybrid info dictionaries are rejected. | [`protocol-support`](protocol-support.md) |
| Product add from a v1 magnet | Implemented | deterministic, runtime, interop, web, AVD, physical, live | Only a v1 `btih` identity and supported magnet fields survive canonicalization. Controlled tracker-only and official Ubuntu metadata-only paths activate discovery during acquisition, remain durably paused, and create no payload artifacts. | [`client-persistence`](client-persistence.md) |
| BEP 53 select-only magnet intent | Implemented | deterministic, persistence, runtime, oracle, web, Android build | Strict bounded `so` ranges remain compact before metadata and become a skipped default plus at most 4,096 wanted exceptions. Duplicate selection is additive; ordinary duplicates are typed no-ops. The pinned libtorrent magnet suite, maximum-span parser case, 4,097-file atomic rejection, restart/runtime fences, generated adapters, and React reveal behavior pass. | [`protocol-support`](protocol-support.md), [`client-persistence`](client-persistence.md) |
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
| HTTP and HTTPS trackers | Implemented | deterministic, runtime, interop, web, desktop, AVD, live | Bounded HTTP/1.1 requests, Basic auth, five redirects, gzip/`x-gzip`, permissive hostile bencode, tracker IDs and BEP 31, compact/noncompact IPv4/IPv6 peers, policy/family DNS, lifecycle/cancellation, metadata-only activation, last-successful connection-family projection, controlled libtorrent discovery, and official Ubuntu HTTPS metadata smokes pass. HTTPS defaults to desktop/Android platform chain and requested-name validation; one hidden typed compatibility value is explicitly encrypted but unauthenticated. Controlled authenticated tracker-to-libtorrent transfer and macOS/Windows/Linux/AVD trust evidence pass. Proxies, scrape, other authentication, custom roots/pins, and a public reliability claim are absent. | [`tracker-discovery`](tracker-discovery.md) |
| DHT | Partial | deterministic, runtime, interop, live | One bounded actor owns independent IPv4/IPv6 identities, routing, tokens, transactions, traversals, peer values, native-family bootstrap, warm state, incoming queries, private gating, merged product lookups, and family-port self-announcement. One session scheduler survives download completion; controlled DHT-only discovery passes in both families, mapped off-LAN IPv4 seed discovery passes, and a native public IPv6 node reached 40 routing nodes and 41 valid responses during successful merged metadata acquisition. Foreign-family bootstrap optimization, BEP 5 `PORT`, and incoming IPv6 reachability remain absent. | [`dht-discovery`](dht-discovery.md) |
| Peer exchange | Implemented | deterministic, runtime, interop | Verified-public BEP 11 uses bounded directional BEP 10 negotiation, 16-KiB/50-contact messages, 50-per-source and 200-per-torrent admission, a 4,096-event shared timeline, exact provenance/privacy cleanup, and the ordinary registry/dial owner. A controlled complementary two-hop pinned-libtorrent run captures one addition, an oracle-observed RSTorrent drop, and exact 16-MiB completion; underpopulated recent-peer exemptions, BEP 40, and durable PEX state remain absent. | [`peer-lifecycle`](peer-lifecycle.md), [`protocol-support`](protocol-support.md) |
| Local service discovery | Absent | none | Interface, multicast, and local-network policy are unimplemented. | [`protocol-support`](protocol-support.md) |

### Peer And Swarm Lifecycle

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded peer registry and source merging | Implemented | deterministic, runtime, interop | Records remain volatile and endpoint-keyed while the separate exact peer-ID admission index permits at most one established generation per claimed remote ID. Crossed, same-direction, self, stale-removal, saturation, and pinned-libtorrent cases pass without merging provenance or reputation. | [`peer-lifecycle`](peer-lifecycle.md) |
| Registry-backed Swarm inspection | Implemented | deterministic, runtime, interop, web | The bounded volatile registry, exact state counts, source merging, retry eligibility, terminal cleanup, and typed self/duplicate closure reasons are visible; durable history remains absent. | [`peer-lifecycle`](peer-lifecycle.md), [`application-view-api`](application-view-api.md) |
| Deterministic dial selection and guarded attempts | Implemented | deterministic, runtime, interop | Selection remains intentionally basic. Post-handshake peer-ID admission deterministically resolves crossed and repeated connections without introducing peer scoring or treating IDs as durable identity. | [`peer-lifecycle`](peer-lifecycle.md) |
| Pre-content peer failover | Implemented | deterministic, runtime, interop, live | Bounded parallel metadata peers share one block owner; two tracker cohorts, 10/10 fresh-DHT owner runs, and 12/12 cross-catalog pairs pass. | [`peer-lifecycle`](peer-lifecycle.md) |
| Multiple simultaneous live peers | Implemented | deterministic, runtime, interop, live | Thirty established and thirty half-open attempts remain separate outbound torrent-local defaults beneath one shared session budget whose ordinary default is 200 after descriptor clamping and whose incoming-only slack is ten. Exact saturation, cancellation, mixed-direction release, and simultaneous incoming evidence pass. | [`peer-lifecycle`](peer-lifecycle.md) |
| Transfer request ownership and failover | Implemented | deterministic, runtime, interop, live | Ordinary blocks have one generation; strict endgame adds bounded duplicate attempts, first-response cancellation, and harmless losing payload. | [`download-correctness`](download-correctness.md) |
| BEP 6 Fast request lifecycle | Implemented | deterministic, scripted runtime, interop | Bilateral negotiation, exact initial availability, choke-retained requests, exact reject/refill, terminal upload responses, 32-entry advisory bounds, equal-rarity suggestion bias, and canonical ten-entry IPv4 allowed-fast generation pass. Controlled capture verifies both pinned-libtorrent directions and exact 40,000-byte payload hashes; predictive requests, super-seeding, and an invented IPv6 set remain absent. | [`protocol-support`](protocol-support.md), [`download-correctness`](download-correctness.md) |
| Incoming peer connections | Implemented | deterministic, runtime, interop, web | One bounded incoming owner accepts independently bound IPv4 and eligible global-unicast IPv6 listeners, each with a five-entry backlog, under eight pending handshake slots, 1,024 generation-fenced registrations, and the shared effective-plus-ten-slack connection budget. Ordinary automatic/fixed settings still describe one preferred numeric port; each family independently resolves a coordinated TCP/UDP pair and a failed family leaves its sibling serving. The default-enabled persisted IPv6 policy applies live and closes plaintext and MSE IPv6 generations before `Applied`. Existing evidence proves mapped off-LAN IPv4 seeding, live candidate-first replacement, truthful family advertisement, and terminal cleanup. Tactical `113` implements one independent finite-lease IPv6 firewall-pinhole slot and typed product status under the same reachability coordinator; deterministic and scripted-gateway evidence pass. Its live negative control passes, but the observed gateway rejects `AddPinhole` with typed `606`, so no physical off-network incoming IPv6 or cleanup claim is made. | [`incoming-reachability-and-seeding`](incoming-reachability-and-seeding.md), [`peer-lifecycle`](peer-lifecycle.md) |
| uTP peer transport | Partial | deterministic, runtime, interop | Tacticals `119` and `121` prove bounded v1 wire/connection/reliability transitions, exact receive credit and packetization, delayed ACK, retransmission execution, RFC 6817 congestion/pacing, path MTU, and fixed impairment/resource thresholds. Tactical `125` adds independent bounded DHT/uTP session routing, generation-fenced connection/task ownership, ordered-stream and peer-I/O composition, controlled incoming peer/upload integration, and exact pinned-libtorrent loopback transfers in both roles with terminal zero ownership. Ordinary product dialing/listening/selection remains TCP; WAN evidence, IPv6 uTP, real-socket MTU probing, MSE-over-uTP, UDP reachability, product policy, and support graduation remain absent. | [`utp-transport-campaign`](utp-transport-campaign.md), [`protocol-support`](protocol-support.md) |
| Peer reputation and integrity attribution | Partial | deterministic, runtime, live | Exact connection generations receive bounded asymmetric trust; a sole corrupt source is banned and ambiguous sources are only suspected, while full parole selection and persistence are absent. | [`download-correctness`](download-correctness.md) |

### Content Transfer And Completion

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded 16 KiB block pipeline | Implemented | deterministic, runtime, interop, live, physical | Per-connection depth adapts under distinct local bounds while session request/payload/active-piece totals remain 256 MiB/32 MiB/256 MiB on desktop and 128 MiB/16 MiB/128 MiB on Android. Fair generation-scoped admission prevents active torrent count from multiplying them. | [`download-correctness`](download-correctness.md) |
| Sequential multi-piece download | Implemented | deterministic, runtime, interop | BEP 3 `length`, one-entry `files`, and ordinary multi-file torrents share one download, durable resume, repair, and publication pipeline. | [`download-correctness`](download-correctness.md) |
| Availability-aware piece selection | Implemented | deterministic, runtime, interop, performance | Requestable active work remains first; exact live nonseed counts plus a separate seed count feed a compact incrementally indexed rarest-first default with an in-order baseline. Independent count, byte, peer-ratio, and block-pressure limits pass hostile maximum-geometry and release CPU/memory gates; full request windows skip inactive lookahead work; unique unplanned pieces remain protected; and controlled libtorrent verifies the sole scarce piece first. Streaming urgency, user picker controls, reverse rarity for snubbed peers, and parole remain absent. | [`download-correctness`](download-correctness.md) |
| Choke recovery | Implemented | deterministic, runtime, interop | Requests move to another peer and full choked sets are replaceable; mature choking/reputation policy is absent. | [`download-correctness`](download-correctness.md) |
| Per-request timeout and slow-peer handling | Implemented | deterministic, runtime | Useful response samples derive a bounded inactivity deadline and reduce a stalled peer to one probe; broader snub reputation remains absent. | [`download-correctness`](download-correctness.md) |
| Endgame | Implemented | deterministic, runtime, live | Strict duplicates, core cancels, late-loss safety, exact accounting, and public verified publication pass; throughput parity remains open. | [`download-correctness`](download-correctness.md) |
| Hash-failure recovery | Implemented | deterministic, runtime, interop, live | A failed v1 generation resets the whole piece with bounded contributors; v2 block-level recovery and full parole selection are absent. | [`download-correctness`](download-correctness.md) |
| Reliable completion on ordinary swarms | Partial | deterministic, runtime, interop, live | Multi-peer liveness, endgame, corrupt-generation retry, and bounded storage completion pass, but completion latency is not yet comparable and public corruption was not induced. | [`download-correctness`](download-correctness.md) |
| Payload upload and seeding | Implemented | deterministic, runtime, interop, web, AVD, physical | Published and active incomplete torrents serve exact verified/readable availability and bounded 16-KiB requests through initiated or accepted TCP peers under the shared live 0--50 slot, ten-read, 40-handle, and writer bounds. Complementary RSTorrent/libtorrent ordinary, Fast, forced-MSE, cross-file, part-backed, and API 34 SAF transfers capture Piece frames in both directions before completion and independently verify every final hash. Active routed torrents advertise the real tracker/DHT port with nonzero `left`; failure and lifecycle changes retract or replace authority before stale reads. Exact completed-seed local, mapped off-LAN, AVD, and physical evidence remains. Finite bandwidth, ratio/time goals, and discovery-driven public incomplete-swarm reliability remain absent. | [`incoming-reachability-and-seeding`](incoming-reachability-and-seeding.md), [`protocol-support`](protocol-support.md) |

### Integrity, Storage, And Resume

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| SHA-1 piece verification before have state | Implemented | deterministic, runtime, interop | Failure resets only the attempted v1 piece and preserves unrelated verified state. | [`download-correctness`](download-correctness.md) |
| Multi-file mapping and selective files | Implemented | deterministic, runtime, interop, web, AVD | Path and dynamic-SAF Normal/Skip routing, lazy part storage, boundary materialization, and metadata-only intake pass; high/low scheduling remains absent. | [`client-persistence`](client-persistence.md), [`download-correctness`](download-correctness.md), [`android-saf-storage`](android-saf-storage.md) |
| Cross-file, skipped-file, and padding storage | Implemented | deterministic, runtime, interop, web | Lazy part creation, retained lowered destinations, route-epoch promotion/demotion, exact verified-span export, uncertain boundary-piece invalidation, and empty-part cleanup pass; BEP 47 symlinks are deliberately rejected. | [`client-persistence`](client-persistence.md) |
| Path-backed staging and publication | Implemented | deterministic, runtime, interop | Explicit file/tree topology, hash-owned internal artifacts, durable publishing intent, atomic no-replace rename, namespace sync, crash reconciliation, and fail-closed removal pass. Disk-space policy, relocation, and broader filesystem/provider coverage remain incomplete. | [`client-persistence`](client-persistence.md), [`download-roots`](download-roots.md) |
| Bounded asynchronous content storage | Implemented | deterministic, runtime, interop, live, physical | Payload sync and batched SQLite checkpoints use a separate bounded joined owner; immutable positional writes and hashes execute with independent session totals, root/torrent fairness, explicit generation joins, and the shared 40-handle pool. Controlled multi-torrent/root isolation and physical Android concurrency pass; broader provider/root profiles remain open. | [`storage-throughput-architecture`](storage-throughput-architecture.md) |
| Android SAF storage and publication | Implemented | deterministic, runtime, interop, AVD, physical | The product uses lazy dynamic acquisition and one 40-handle path/SAF pool. Typed observations gate root health, trusting ordinary resume, active and published reads, and provider repair; fixed manifests are diagnostic-only. The API 34 Tactical `124` profile persists a two-piece partial torrent, fails closed on grant loss, repairs, exchanges complementary Fast payload with pinned libtorrent before completion, and removes exactly at 7/40 handles and 2/16 pending requests. Tactical `120` trusting-resume and earlier complete AVD and physical Android 17/API 37 matrices retain download, selection, checking, publication, upload, cancellation, concurrency, and cleanup coverage. General root management, cloud/removable policy, migration, and relocation remain absent. | [`android-saf-storage`](android-saf-storage.md), [`client-persistence`](client-persistence.md) |
| Durable have state and per-torrent resume | Implemented | deterministic, persistence, runtime, interop, web, AVD, physical | Schema 14 stores one payload fact and generation-fenced verification evidence. Exact ordinary path/SAF structure admits only synchronized committed bits with zero payload reads/hashes; disagreement invokes the full selection-independent checker only for that torrent, malformed state cannot abort profile open, and Force always hashes. Same-length external mutation is deliberately outside ordinary detection. | [`client-persistence`](client-persistence.md), [`download-correctness`](download-correctness.md) |
| Recovery after content hash failure | Implemented | deterministic, runtime | Sole corrupt and ambiguous multi-source generations retry cleanly with bounded exact-generation attribution. | [`download-correctness`](download-correctness.md) |

### Application And Product Surfaces

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Durable semantic application control | Implemented | deterministic, runtime, interop, web, Tauri, AVD, physical | Archive, fenced keep/delete removal, metadata-only add, atomic v1 torrent-byte add, serialized live file selection, retained checker pause/resume, atomic `Download now`, queue movement, automatic concurrent admission, and exact-or-synthesized magnet export are implemented; stable public compatibility remains absent. | [`application-control`](application-control.md) |
| Ephemeral application state | Implemented | deterministic, runtime | Private bounded session and metrics SQLite stores preserve receipts, exact source, metadata, settings, views, DHT and speed state for one joined service lifetime, then disappear without profile files. One maximum source plus info fits the 256-MiB session cap and a second maximum import rolls back with a typed resource limit; payload storage remains external. | [`client-persistence`](client-persistence.md), [`application-control`](application-control.md) |
| Leased application view sets and delivery clients | Implemented | deterministic, runtime, interop, web, Tauri | Named summary, generation-scoped checker progress, piece, structured diagnostic, active-peer, registry-backed Swarm, paged file and tracker, global Disk, range-selected session Speed, and latest-value session DHT views have bounded replay/reset, independent lease expiry, fresh-snapshot recovery, diagnostic HTTP polling, acknowledged browser WebSocket streaming, and acknowledged in-process Tauri streaming. The retained observer matrices still expose Summary reset storms and trace/all-view serialization pressure; stable public compatibility remains unimplemented. | [`application-view-api`](application-view-api.md), [`application-connection-architecture`](application-connection-architecture.md) |
| Shared web and Tauri desktop UI | Partial | runtime, interop, web, desktop | The responsive surface now has Library, Transfers, and Workbench destinations, truthful bounded torrent-backed cards, accessible determinate/indeterminate checker progress with exact selected-summary counters, shared multi-selection, magnet and local `.torrent` add, source-preserving or name/tracker-rich bounded magnet copy, metadata-only add, live Normal/Skip file actions plus atomic `Download now` for skipped targets, archives, guarded removal, live peer/swarm/file/tracker inspection, global Disk pressure, bounded Canvas Pieces, a smooth exact session Speed history, a one-second download/upload tab title, and the exact routing-space DHT observatory. A real media catalog/playback remains incomplete. | [`client-surfaces`](client-surfaces.md), [`application-interface-direction`](application-interface-direction.md) |
| Authenticated private web host | Implemented | deterministic, runtime, web, live | One explicitly configured maintainer host serves the production React bundle and multiplexed application WebSocket behind bounded Basic authentication and exact HTTPS Origin checks. Exact-push isolated build, candidate smoke, supervised restart, authenticated private-listener/public verification, and rollback-on-failure pass; this is not a relay, account, pairing, encryption, or stable public compatibility claim. | [`application-connection-architecture`](application-connection-architecture.md), [`client-surfaces`](client-surfaces.md) |
| Local headless web authentication | Implemented | deterministic, runtime, web | Fresh loopback profiles have a communicated ten-minute setup choice between local-open and at most 32 rolling remembered-browser sessions. Four-digit one-use approval, five-attempt exhaustion, HttpOnly Strict cookies, exact Host/Origin checks, Settings revocation, typed live-socket termination, restart persistence, and explicit one-browser recovery pass. This is not password, LAN, relay, device-identity, or E2E remote authentication. | [`application-connection-architecture`](application-connection-architecture.md), [`web-ui-design`](web-ui-design.md), [`remote-access-authentication`](remote-access-authentication.md) |
| Android Compose foreground client | Implemented | deterministic, runtime, AVD, physical | The maintained Material 3 product provides the JSTorrent-shaped Library, six-tab torrent detail, Speed, dual-family DHT, structured Logs, and Settings hierarchy with RSTorrent branding. One service-scoped owner consumes every Android-relevant bounded projection; magnet and `.torrent` intake, SAF setup/repair, file selection/open, torrent and queue actions, backed settings, activity/process recovery, and controlled concurrent downloads pass. Search/plugins, playback, bandwidth policy, richer file priority, tracker mutation, and dynamic network/power controls remain explicitly unavailable; Tactical `117` makes no new physical-device UI claim. | [`client-surfaces`](client-surfaces.md) |
| Eventual iOS native client | Absent | deterministic, simulator, physical feasibility | No product target or support claim exists. Tacticals `116` and `123` link the real Rust pool, storage, SHA-1, namespace, TCP, and UDP operations; app-owned Documents persistence, generation-fenced force-close recovery, per-operation coordination, exact cleanup, ordinary expiration, and finite continued processing pass on iOS 26.6. App-owned Documents is the sole graduated root assumption. System-picked local, iCloud, and other File Provider roots are classification-only and compiled out of registration; product persistence, notification policy, and indefinite background operation remain unimplemented. | [`product-direction`](product-direction.md), [`client-surfaces`](client-surfaces.md), [`download-roots`](download-roots.md) |
| Derived progress, torrent ETA, and bounded diagnostics | Implemented | deterministic, runtime, interop, web, AVD | Progress remains an application projection. Selection-aware torrent ETA adds exact required/remaining non-padding peer work, a 184-byte scalar model, one shared cadence, and typed warming/estimate/stalled/unavailable presentation; file ETA, richer priority, and Size/Progress repair remain absent. Structured hierarchical diagnostics, typed context, capture interest, explicit source/delivery/local loss, and the global ordered console are complete. | [`application-control`](application-control.md), [`application-view-api`](application-view-api.md), [`download-correctness`](download-correctness.md) |
| Offline, loopback-only, and online egress policy | Implemented | deterministic, runtime, web, AVD | Policy is fixed for one service lifetime; Android VPN and metered-network controls are absent. | [`application-control`](application-control.md) |
| Headless product validation | Implemented | web, AVD | Physical devices and visible desktop automation still require explicit authorization. | [`client-surfaces`](client-surfaces.md) |
| Comparative live performance harness | Implemented | deterministic, interop, web, live | Named hardware profiles retain row-specific 1/10 GiB engine gates, per-view/adversarial application ratios, environment applicability and artifact-producing CI. The opt-in paired browser adapter smoke additionally records HTTP/WebSocket traffic and exact 1 GiB completion without defining a hard floor. The hosted-runner profile is deliberately broad and uncalibrated; public speed remains a distribution rather than a CI threshold. | [`performance-and-live-evidence`](performance-and-live-evidence.md) |
| Multi-torrent queue and resource budgets | Implemented | deterministic, persistence, runtime, interop, web, physical | Schema 17 stores automatic queue order and configured limit; desktop defaults to three and Android clamps effectively to two. One application owner admits exact generations under shared memory, storage/hash, tracker, outbound, peer, and file-handle ceilings. Controlled performance gates, 100-runnable/500-complete scale, headless queue/settings actions, and physical Pixel promotion/cleanup pass; seed ranking, bandwidth policy, and adaptive platform pressure remain later. | [`application-control`](application-control.md), [`performance-and-live-evidence`](performance-and-live-evidence.md) |

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
