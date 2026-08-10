# Protocol Support And Conformance

Topic: `protocol-support`

Status: RSTorrent implements a bounded subset of the v1 protocol sufficient
for controlled verified downloads, BEP 9 metadata exchange, scheduled BEP 15
UDP tracker announces, an IPv4 Mainline DHT foundation, and bounded
multi-peer payload seeding including one externally verified UPnP-mapped TCP
path. UDP, HTTP, and HTTPS trackers share the selected family-correct
advertisement lifecycle; DHT advertises each selected real-family TCP endpoint
for an eligible verified-public incoming route independently of completion.
Completed Tactical
[`124`](../tactical/124-duplex-verified-piece-upload.md) proves exact sparse
availability and bidirectional payload over initiated and accepted TCP
connections before completion. Controlled pinned-libtorrent ordinary, Fast,
forced-MSE, cross-file, part-backed, and Android SAF gates pass. The bounded
`WANIPv6FirewallControl:1` subset is implemented and passes deterministic and
scripted-gateway evidence. Its live negative control passes, but the observed
gateway returns typed `606` to `AddPinhole`, so positive physical capability is
unknown on the current hardware and its off-LAN proof does not pass. It does
not claim complete
BEP 3 or BEP 5 support, full BEP 7 announcing, public-swarm advertisement
reliability on public incomplete swarms, uTP, or v2 support.
HTTPS now defaults to authenticated desktop/Android platform trust; one explicit hidden
compatibility policy remains encrypted but unauthenticated.
Tactical [`111`](../tactical/111-mse-peer-stream-encryption.md)'s implemented
slice additionally supports the de facto MSE/PE protocol over TCP in both
directions under a bounded four-value session policy. Its claim is peer
compatibility and header obfuscation, never transport security.
Completed follow-up Tactical
[`115`](../tactical/115-mse-policy-advertisement-and-peer-detail.md) aligns the
default `allow` responder selection with stock libtorrent, advertises incoming
MSE capability to HTTP trackers, and carries the exact method as optional peer
detail without adding a method-preference setting.
Completed Tactical
[`125`](../tactical/125-shared-udp-utp-runtime-and-loopback-interop.md) adds a
bounded engine-only IPv4 uTP runtime and proves exact loopback transfer against
pinned libtorrent in both roles. The BEP 29 claim remains **Unsupported**
because ordinary product dialing/listening and UDP reachability, WAN evidence,
IPv6 uTP, MSE composition, transport policy, and product presentation are not
implemented.

Tactical [`074`](../tactical/074-context-specific-metainfo-limits.md) replaced
the former global one-MiB relationship with context-specific metainfo limits.
Tactical `081` subsequently scales the explicit and durable profiles to
64 MiB, peer-controlled BEP 9 receive to 30 MiB, local metadata upload to the
actual valid retained size up to 64 MiB, and v1 geometry to 2,097,152 pieces.
Every profile retains independent depth, decoded-item, file, piece, path and
tracker bounds.

Tactical [`078`](../tactical/078-local-single-peer-tcp-seeding.md) adds the
application-owned IPv4 loopback listener, exact bitfield and bounded
request/cancel/piece upload, and BEP 9 metadata followed by payload on the same
socket. Tactical
[`082`](../tactical/082-bounded-multi-peer-upload-ownership.md) adds a shared
connection budget, eight fixed upload slots with one optimistic slot, bounded
request/read/writer pipelines, and exact physical payload accounting. Scripted
ten-peer tests and simultaneous two-RSTorrent/two-libtorrent transfers pass.
That tactical's claim remains loopback and complete-torrent only; Tactical
`088` expands the binding and reachability evidence without changing upload
content or scheduling policy.

Tactical [`086`](../tactical/086-long-lived-torrent-peer-runtime.md) retains
one ordinary peer authority across download and completed-seed lifetimes and
attaches routed incoming sockets to it. Controlled authenticated-gateway
evidence observes simultaneous pinned libtorrent and RSTorrent Peers/Swarm
rows while both independently verify exact content, then observes exact row
removal, pause, and terminal zero ownership. This expands inspection and
lifecycle evidence without expanding the claimed network scope.

Tactical [`088`](../tactical/088-upnp-mapped-external-tcp-seeding.md) adds a
bounded IPv4 UPnP IGD v2 `WANIPConnection:2` control-point subset and proves
one exact 4,195,035-byte payload transfer from an independent off-LAN peer
through the queried public TCP mapping. Mapping deletion, absent-query,
failed-reconnect, and terminal-zero evidence pass. Tracker/DHT
self-announcement remained absent in that prerequisite slice.

Tactical [`089`](../tactical/089-coordinated-session-listen-sockets.md) adds
no new BEP claim. It establishes the prerequisite transport truth: automatic
TCP and UDP bind from a persisted preferred port under a shared bounded retry
policy, fixed binding is exact, one session UDP owner carries application DHT
traffic, and actual TCP/UDP endpoints remain separate runtime facts.

Completed Tactical
[`092`](../tactical/092-truthful-tracker-and-dht-peer-advertisement.md) adds
selected TCP-port tracker lifecycle, explicit-port token-authenticated DHT
self-announcement after verified public and incoming-routable state, mapping
correction, and ordered stopping. Controlled tracker-only and DHT-only
libtorrent leechers complete without explicit peer hints. A physical run
decodes the mapped external port from both wire mechanisms and completes from
an off-LAN peer through that port.

Tactical [`081`](../tactical/081-v1-torrent-byte-intake.md) implements bounded
v1 outer-metainfo intake, libtorrent-aligned large-v1 limits, and BEP 12 tier
retention over the existing UDP tracker runtime. Deterministic, maximum-
resource, restart, transport and controlled pinned-libtorrent evidence pass.
Completed Tactical
[`095`](../tactical/095-bounded-http-https-tracker-transport.md) extends those
tiers through bounded HTTP and HTTPS transport, compact/noncompact IPv4/IPv6
peer intake, and outbound IPv6 transfer. Completed Tactical
[`098`](../tactical/098-authenticated-https-tracker-platform-trust.md) adds
default platform certificate-chain/requested-name validation, live bounded
client-pair replacement, truthful operation-captured projection, desktop and
Android runtime evidence, and controlled authenticated tracker introduction
to pinned libtorrent. V2/hybrid integrity remains absent.

## Purpose And Claim Policy

This topic maps product behavior to normative BitTorrent Enhancement Proposals
and records the exact supported subset, limits, and evidence. It is a
traceability ledger, not the primary feature backlog. Completion liveness,
request ownership, storage recovery, and resource policy frequently span one
broad BEP or are implementation responsibilities not usefully represented by
a BEP checkbox.

The [official BEP index](https://www.bittorrent.org/beps/bep_0000.html) is the
normative starting point. [`../references.md`](../references.md) owns reference
provenance and the pinned offline specification policy. Focused topics and
tacticals own RSTorrent's design; a reference implementation does not silently
set that design.

Claims use these states:

- **Supported**: the exact subset named in the row is implemented and has
  independent interoperability evidence where interoperability is practical.
- **Partial**: meaningful protocol behavior exists, but a common required
  behavior or named part of the proposal is absent.
- **Unsupported**: RSTorrent does not implement the proposal as a product
  capability. Parsing or safely rejecting a related value is not support.
- **Not assessed**: no support claim or product decision has been made.

No row may become **Supported** because codecs compile or unit tests pass
alone. The row must name its scope and evidence. Draft or accepted status of a
BEP is external protocol metadata, not RSTorrent readiness.

## Current Matrix

| Specification | Claim | Implemented subset and evidence | Deliberate limits and dependencies |
| --- | --- | --- | --- |
| [BEP 3: The BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html) | Partial | Strict bounded bencoding; distinct `length` and `files` publication shapes over one v1 storage/resume pipeline; raw-info SHA-1 identity; outgoing and bounded multi-peer incoming TCP handshakes; exact self and duplicate peer-ID admission; keepalive, choke, unchoke, interested, not-interested, have, bitfield, request, cancel, and piece messages; bounded multi-peer download scheduling; and bounded verified metadata/payload upload from published or active selective storage under eight shared slots with exact physical accounting. Active initiated and accepted connections prove complementary Piece frames before completion; controlled ordinary/Fast/MSE pinned-libtorrent and Android SAF runs add sparse, cross-file, part-backed, lifecycle, and final-hash evidence. Ordinary routed Peers/Swarm observation, completed-seed interop, restart/crash/publication, authenticated tracker/DHT discovery, and mapped off-LAN evidence cover the broader implemented subset. | Peer IDs are spoofable live claims rather than authenticated durable identities. Finite-bandwidth/goal policy, public incomplete-swarm reliability, and comparable ordinary-swarm performance remain absent. |
| [BEP 5: DHT Protocol](https://www.bittorrent.org/beps/bep_0005.html) | Partial | Bounded KRPC, independent IPv4/IPv6 K=8 routing and replacements, alpha-3 iterative lookup, exact transaction/source correlation, incoming query handling, token-authenticated peer announcements, native-family bootstrap/refresh, warm persistence, network-policy integration, and ordinary peer-registry delivery. One long-lived session scheduler fans a lookup across active family nodes and self-announces each selected real-family TCP port to at most K=8 token-bearing responders for a desired-running verified-public incoming route, whether active or complete; a family without an endpoint performs lookup without announcing port 1. Deterministic, scripted runtime, controlled IPv4 and IPv6 DHT-only libtorrent completion, mapped off-LAN IPv4 wire-port, repeated public metadata acquisition, dual-family public-bootstrap, and pre-completion complementary-piece evidence pass. | BEP 5 `PORT` messages remain absent. BEP 5 has no immediate withdrawal query; stopped announcements expire as remote soft state. Foreign-family bootstrap optimization, public incomplete-swarm reliability, and incoming IPv6 reachability are not part of this claim. |
| [BEP 6: Fast Extension](https://www.bittorrent.org/beps/bep_0006.html) | Supported | Bilateral reserved-bit negotiation; strict suggest, have-all, have-none, reject-request, and allowed-fast codecs; exactly-one initial availability; request retention across choke; exact reject ownership and immediate refill; one terminal upload response through cancel/read/choke/shutdown races; bounded advisory ranking; and canonical IPv4 allowed-fast generation. Deterministic race/bound tests and controlled capture prove both pinned-libtorrent `2.0.13.0` transfer directions, magnet metadata-to-content continuity, and an explicit sub-millisecond reject/refill path with exact payload verification. | Predictive requests and super-seeding are optional behavior and remain absent. BEP 6 defines no IPv6 allowed-fast generation, so IPv6 retains the negotiated reject/availability lifecycle without an invented set. Tactical [`093`](../tactical/093-bep6-fast-request-lifecycle.md) is the completed execution record. |
| [BEP 7: IPv6 Tracker Extension](https://www.bittorrent.org/beps/bep_0007.html) | Partial | HTTP/HTTPS tracker connections may use IPv6 literals or AAAA-only names; family selection precedes query construction and source binding; an eligible IPv6 request uses the probe-selected global-unicast source and its real listener port, otherwise port `1`; compact `peers6` is accepted independently of tracker family; and returned endpoints feed the ordinary outbound IPv6 TCP path. Listener-backed `GlobalUnicast`, gateway-reported `Unfiltered`, and accepted `Pinholed` scopes retain the same real port while exposing different local evidence. Scripted AAAA-only/only-`peers6`, family-source/port observation, application hash-verified outbound IPv6 transfer, Android HTTPS, native public IPv6 DHT, and scripted pinhole evidence pass. | There is one coordinated listener pair per family, not multi-interface binding or simultaneous per-local-address tracker fan-out. The physical negative control passes, but the gateway rejects `AddPinhole` with typed `606`; no observed incoming IPv6 transfer or full BEP 7 claim is made. |
| [BEP 9: Extension for Peers to Send Metadata Files](https://www.bittorrent.org/beps/bep_0009.html) | Supported | Bounded v1 `btih` magnets, extension negotiation, metadata size and block bounds, at most two acquisition requests per download peer, request/data/reject messages, duplicate and ordering validation, cross-peer block assembly, assembled info-hash verification and generation recovery, up to eight simultaneous dial/metadata work items, an independent progress deadline, inspectable acquisition state, and bounded diagnostic plus shared multi-peer application-listener metadata upload. Controlled libtorrent runs pass in both directions; the maximum receive proof transfers an exact 31,457,280-byte info dictionary in 1,920 blocks and requests, and local upload serves every block of valid metadata up to 64 MiB. | The first complete hash-verified download generation wins. V2 identities and BEP 53 selection are outside the claim; public-swarm discovery and completion remain variable rather than a reliability claim. |
| [BEP 10: Extension Protocol](https://www.bittorrent.org/beps/bep_0010.html) | Supported subset | Reserved-bit negotiation, bounded extended framing, separate local/remote directional IDs, recognized `ut_metadata` and `ut_pex` maps, repeated additive handshakes, disable-by-zero, unknown-name ignore behavior, and bounded listen-port `p`. Existing BEP 9 evidence and Tactical [`094`](../tactical/094-bounded-bep11-peer-exchange.md) deterministic plus pinned-libtorrent evidence cover both recognized extensions. | This is deliberately a recognized-extension map, not an arbitrary plugin registry. Hole punching, upload-only, client-version policy, arbitrary retained names, and other BEP 10 extensions remain absent. |
| [BEP 11: Peer Exchange](https://www.bittorrent.org/beps/bep_0011.html) | Supported subset | Verified-public incoming and outgoing `ut_pex`; exact compact IPv4/IPv6 add/drop and flags; 16-KiB frames; 50-add/50-drop message bounds; one-minute cadence; per-source and per-torrent quotas; duplicate-IP, self, local-network, and source-provenance controls; established-only snapshots and bounded diffs/cursors. Deterministic lifecycle and a controlled pinned-libtorrent `2.0.13.0` complementary two-hop transfer pass, including a captured compact addition, PEX-only dialing, an oracle-observed RSTorrent drop, and exact 16-MiB hash verification. | Recently disconnected underpopulated-list exemptions, BEP 40 canonical priority, durable PEX state, PEX-derived trust, and PEX-carried uTP, encryption, upload-only, and hole-punch capability flags remain absent. Tactical [`094`](../tactical/094-bounded-bep11-peer-exchange.md) is the completed record. |
| [BEP 12: Multitracker Metadata Extension](https://www.bittorrent.org/beps/bep_0012.html) | Supported | Bounded outer `announce-list` parsing preserves tier order and falls back to `announce`; exact tiers and URL source survive restart. UDP/HTTP/HTTPS rows share deterministic tier scheduling and an eight-operation ceiling; pure tests retain 300 trackers in three tiers, controlled byte intake completes through an imported tracker, and mixed-transport lifecycle tests pass. HTTPS defaults to platform chain/name validation and the explicit compatibility value is projected as unauthenticated. | Magnet `tr` values still form one synthetic tier because magnets carry no BEP 12 structure. |
| [BEP 14: Local Service Discovery](https://www.bittorrent.org/beps/bep_0014.html) | Unsupported | None. | Requires an advertised incoming port, per-interface multicast ownership, local-network permission, and private-torrent policy. |
| [BEP 15: UDP Tracker Protocol for BitTorrent](https://www.bittorrent.org/beps/bep_0015.html) | Partial | Bounded connect and announce codecs, source/action/transaction/stride validation, IPv4 and IPv6 compact response parsing, DNS and destination policy, connect/announce retransmission, 30-second aggregate operation bounds, 60-second connection-token cache, session-wide eight-operation startup fan-out, multi-tracker fallback, failure backoff, success promotion, interval/corrective reannounce, exact current counters, selected TCP port or port-`1` outbound-only sentinel, and started/completed/stopped lifecycle. One application-lifetime peer ID is shared by tracker and peer handshakes. Scripted loss/lifecycle tests, controlled libtorrent tracker-only completion, mapped off-LAN wire-port transfer, and public metadata acquisitions pass. | Scrape, authentication, proxies, BEP 41 URL data, durable lifetime traffic accounting, and a public-tracker reliability claim remain absent. |
| [BEP 17: HTTP Seeding (Hoffman-style)](https://www.bittorrent.org/beps/bep_0017.html) | Unsupported | None. | Web seeds need their own hostile-response, range, retry, integrity, and resource policy. |
| [BEP 19: HTTP/FTP Seeding (GetRight-style)](https://www.bittorrent.org/beps/bep_0019.html) | Unsupported | None. | Web seeds are outside the current peer-transfer owner. |
| [BEP 23: Tracker Returns Compact Peer Lists](https://www.bittorrent.org/beps/bep_0023.html) | Supported | HTTP/HTTPS announces request `compact=1`; bounded response parsing accepts ordered six-byte compact IPv4 peers, discards one incomplete trailing suffix without shifting alignment, deduplicates endpoints, and continues to accept advisory noncompact responses. Deterministic hostile parsing, scripted HTTP, controlled libtorrent tracker discovery, and application transfer evidence pass. | The 200-peer response cap is deliberate. BEP 15 retains its independent UDP compact response shape. |
| [BEP 27: Private Torrents](https://www.bittorrent.org/beps/bep_0027.html) | Partial | Verified v1 metadata retains the private flag. Verified private resume suppresses DHT and PEX before discovery; private metadata learned after premetadata discovery cancels DHT and purges DHT- and PEX-only peers before content scheduling. Exact outer `.torrent` intake and announce-tier retention use the same gating. Deterministic and application runtime tests cover the transitions. | LSD remains absent rather than a gated implementation. Private trackers and ordinary manually supplied peers retain their existing policy; no PEX input is accepted before verified public metadata. |
| [BEP 29: uTorrent Transport Protocol](https://www.bittorrent.org/beps/bep_0029.html) | Unsupported | Completed Tacticals [`119`](../tactical/119-deterministic-utp-transport-core.md) and [`121`](../tactical/121-deterministic-utp-loss-congestion-and-mtu.md) add the dependency-free v1 codec and bounded deterministic connection/reliability/receive state, exact receive credit and packetization, delayed ACKs, retransmission execution, fixed-point RFC 6817 congestion and pacing, and binary path-MTU discovery. Fixed exact-hash simulations pass clean, jitter/reorder/duplicate, 1% loss, queue, clock wrap/drift, zero-window pressure, DF black-hole, and established TCP-like foreground thresholds. Completed Tactical [`125`](../tactical/125-shared-udp-utp-runtime-and-loopback-interop.md) adds independent bounded DHT/uTP routing on the shared session UDP receiver, a supervised generation-fenced IPv4 runtime, ordered peer streams, controlled plaintext peer-wire/incoming-upload composition, and both pinned-libtorrent `2.0.13.0` loopback roles. Each role transfers the exact 2,097,883-byte fixture with one uTP peer, zero TCP peers, exact SHA-1, bounded resources, and terminal zero ownership. Closed Tactical [`126`](../tactical/126-controlled-outbound-utp-wan-evidence.md) records that the authorized remote host lacked both a direct IPv4 endpoint and installed oracle and therefore stopped before uTP traffic. | Ordinary product peer dialing, listening, selection, fallback, advertisement, and presentation remain TCP-only. WAN evidence, IPv6 uTP, active real-socket path-MTU probing, MSE-over-uTP, UDP mapping/pinhole reachability, ordinary swarm evidence, and claim graduation remain absent. |
| [BEP 32: IPv6 extension for DHT](https://www.bittorrent.org/beps/bep_0032.html) | Partial | KRPC codecs support `want`, `nodes6`, and compact IPv6 peer values. One actor owns independent IPv4 and IPv6 identities, routing, tokens, transactions, lookups, peer values, native-family bootstrap, and persisted samples over family-selected UDP sockets. Controlled pinned-libtorrent IPv6 DHT-only discovery/download and incoming query coverage pass; a native public IPv6 node reached 40 routing nodes, K=8 in 1.218 seconds, and 41 valid responses during successful merged metadata acquisition. | BEP 32's foreign-family bootstrap/saved-node optimization, interface enumeration, address-change rebinding, and BEP 5 `PORT` messages remain absent. The public IPv6 leg returned no peer value in that one sample, and no incoming-reachability claim follows. |
| [BEP 40: Canonical Peer Priority](https://www.bittorrent.org/beps/bep_0040.html) | Unsupported | None. | Current deterministic selection is local policy, not canonical peer priority. Completed PEX Tactical [`094`](../tactical/094-bounded-bep11-peer-exchange.md) supplies source and address diversity without claiming BEP 40; measured evidence may select a later canonical-priority slice. |
| [BEP 41: UDP Tracker Protocol Extensions](https://www.bittorrent.org/beps/bep_0041.html) | Unsupported | The BEP 15 parser tolerates datagram length according to its own bounds, but emits no extension fields. | URL data, authentication, and future extension negotiation are absent. |
| [BEP 42: DHT Security Extension](https://www.bittorrent.org/beps/bep_0042.html) | Partial | Address-bound IPv4 and IPv6 node-ID generation/validation follows published vectors, including libtorrent's IPv6 mask cases. External-address votes and identity replacement are isolated per family, invalid remote IDs are excluded from routing, and warm state retains address-keyed identities. Controlled IPv6 interoperability and public external-address observation pass. | Broader public-network identity-change and address-rotation evidence are absent. The BEP's local/private-address exemption is retained. |
| [BEP 43: Read-only DHT Nodes](https://www.bittorrent.org/beps/bep_0043.html) | Partial | KRPC parses and emits `ro`; nodes declaring read-only are not admitted from incoming queries. Deterministic codec and admission tests pass. | Product policy does not yet select read-only mode for uncontactable, metered, VPN, or Android lifecycle states. |
| [BEP 47: Padding files and extended file attributes](https://www.bittorrent.org/beps/bep_0047.html) | Partial | Multi-file `p` attributes produce synthetic zero ranges for verification without writing padding files. Deterministic storage-layout and controlled selective-file evidence passes. | Symlinks are explicitly rejected. Executable, hidden, and per-file SHA-1 attributes are not product behavior. |
| [BEP 48: Tracker Protocol Extension: Scrape](https://www.bittorrent.org/beps/bep_0048.html) | Unsupported | None. | Tracker scrape values and application presentation are absent. |
| [BEP 52: The BitTorrent Protocol Specification v2](https://www.bittorrent.org/beps/bep_0052.html) | Unsupported | Metainfo and magnet parsers explicitly reject v2 and hybrid identities. | SHA-256 identities, file trees, piece layers, aligned storage, hybrid validation, and v2 peer behavior require a separate correctness design. |
| [BEP 53: Magnet URI extension - Select specific file indices for download](https://www.bittorrent.org/beps/bep_0053.html) | Supported | Strict bounded repeated `so` parsing and canonical compact ranges; pre-metadata restart; skipped-default plus bounded wanted exceptions; metadata-time catalog/padding filtering; additive duplicate promotion; typed idempotent duplicate outcomes; active-owner fencing; generated adapters; and React reveal/feedback pass. The pinned libtorrent magnet suite passes its select-only, malformed, bounds, and round-trip cases. | Zero-based indices are limited to the product's 374,998-file catalog and at most 4,096 materialized exceptions. Ordinary duplicates remain no-ops; only explicit `so` promotes files. BEP 53 adds no peer-wire message, so interoperability is an intake/oracle and existing hash-verified payload composition claim, not wire observation of `so`. |
| [BEP 55: Holepunch extension](https://www.bittorrent.org/beps/bep_0055.html) | Unsupported | None. | Depends on PEX, uTP, extension negotiation, incoming reachability, address policy, and NAT behavior. |

## Non-BEP Peer Protocols

| Protocol | Claim | Implemented subset and evidence | Deliberate limits |
| --- | --- | --- | --- |
| [MSE/PE](../tactical/111-mse-peer-stream-encryption.md) | Supported subset | TCP initiator and responder roles; DH-768 with exact 160-bit local exponents and degenerate-key rejection; bounded pads, request-hash torrent lookup, and IA; RC4-drop1024 handshake protection; negotiated plaintext-payload (`0x01`) and RC4 (`0x02`) streams; `disabled`/`allow`/`prefer`/`required` session policy; `allow` selects plaintext payload when both methods are offered while `prefer`/`required` select RC4; one bounded early-transport plaintext fallback; HTTP `supportcrypto=1` derived from incoming policy; exact method/failure diagnostics, optional peer detail, and the compact `E` flag. Deterministic hostile/truncation/state tests, scripted runtime and resource tests, all 29 controlled pinned-libtorrent `2.0.13.0` cases, forced methods in both directions, exact transfer hashes, setup/flight evidence, six paired 1 GiB performance runs per implementation with RSTorrent retaining more relative RC4 throughput than libtorrent, Android ABI builds, one API 34 AVD product run, and one API 37 physical Pixel 7a product run with five forced-RC4 sessions, exact publication, bounded DH/storage/descriptors, full owner drain, and cleanup pass. | MSE is legacy protocol obfuscation with no authentication, integrity, privacy, or security claim. TCP only; no uTP, per-torrent policy, public-swarm reliability claim, or user-selectable method preference. |

## Gateway Control Protocols

| Specification | Claim | Implemented subset and evidence | Deliberate limits and dependencies |
| --- | --- | --- | --- |
| UPnP Device Architecture 2.0 and Internet Gateway Device v2 | Supported subset | Source-bound SSDP root-device discovery and bounded same-host IPv4 HTTP/XML/SOAP drive independent `WANIPConnection:2` and `WANIPv6FirewallControl:1` slots under one joined reachability coordinator. The IPv4 subset retains external-address lookup, exact specific-entry query, and finite TCP add/verify/renew/delete. The IPv6 subset reads firewall status; distinguishes unfiltered, disallowed, absent-service, absent-action, and authorization states; creates one wildcard-remote numeric-protocol-6 pinhole for the actual global-unicast listener; renews and deletes its finite lease; and bounds ambiguous create/update/delete ownership. Deterministic and scripted gateway tests pass for both, including independent success/failure and terminal cleanup. Existing physical evidence covers one IPv4 IGD v2 mapping: tracker and DHT wire traffic carried its external TCP port, an off-LAN peer hash-verified 4,195,035 bytes, and independent absence plus failed reconnect proved cleanup. The IPv6 physical negative control passes, then the observed gateway returns typed `606` to `AddPinhole`; no pinhole is created. | The IPv6 subset has not passed its opt-in packet-count, off-LAN transfer, typed-`704`, and failed-reconnect gate, so positive physical capability is unknown on current hardware and no incoming IPv6 claim follows. A future retry requires different gateway hardware or separately authorized control-transport work. Only one TCP mechanism per family, finite 3,600/3,601-second pinhole leases, and one gateway service version are in scope. IGD v1, WANPPP, HTTPS, multi-device policy, UDP pinholes/mappings, PCP, and NAT-PMP remain absent. |

## Non-BEP Interoperability Responsibilities

A useful client also needs behavior that is not captured by marking proposals
supported:

- DNS, IPv4 and IPv6 address selection and alternate-address fallback;
- HTTP status, redirect, TLS, proxy, and authentication policy for trackers;
- connection, request, piece, torrent, and session resource budgets;
- request expiry, slow-peer treatment, endgame, and hash-failure recovery;
- tracker and peer scheduling that remains live under partial failures;
- safe paths, storage errors, crash ordering, recheck, and publication;
- platform network binding, VPN leak prevention, metered-network policy, and
  background lifecycle; and
- structured state and diagnostics sufficient to explain stalls.

These belong in the capability and correctness topics even when a BEP offers
useful reference behavior.

## Support Promotion Rules

Before changing a protocol row to **Supported**:

1. state the exact accepted subset and rejected or deferred values;
2. keep codecs and deterministic state independent from sockets and runtimes;
3. test malformed, stale, duplicate, oversized, reordered, and resource-
   exhaustion inputs relevant to the protocol;
4. obtain controlled independent interoperability evidence when practical;
5. connect the feature through the ordinary engine and application owners;
6. update cross-surface state or diagnostics when users need to understand it;
   and
7. link the implementing tactical and exact evidence from the owning topic.

Live public behavior can strengthen a claim but cannot replace deterministic
and controlled evidence. A mature reference implementation informs edge cases
without becoming RSTorrent's architecture or runtime dependency.

## Recommended Protocol Sequence

Protocol breadth follows the current ownership campaign:

1. reach comparable verified-publication performance through the active
   source-first storage and transfer-owner campaign;
2. confirm common-denominator completion across the public catalog and use
   full-reference deltas to select missing protocol breadth;
3. retain completed dual-stack DHT and family-port ownership from Tactical
   [`112`](../tactical/112-dual-stack-transport-and-ipv6-dht.md), then add
   IPv6 firewall-pinhole and incoming evidence through Tactical `113`;
4. retain completed availability-ranked activation from Tactical
   [`091`](../tactical/091-availability-ranked-piece-activation.md), execute
   peer-ID duplicate resolution in Tactical
   [`090`](../tactical/090-peer-id-duplicate-connection-resolution.md), retain
   the completed BEP 6 lifecycle from Tactical
   [`093`](../tactical/093-bep6-fast-request-lifecycle.md), and evaluate
   the completed bounded BEP 11 slice in Tactical
   [`094`](../tactical/094-bounded-bep11-peer-exchange.md) against the
   established peer owner;
5. retain completed platform certificate/hostname validation and per-family
   advertisement ownership while keeping multi-address fan-out outside any
   full BEP 7 claim; and
6. evaluate incoming service, uTP, hole punching, web seeds, and v2 only after
   their prerequisite owners and validation plans exist.

This order is a default, not a promise to implement every listed proposal.
New real-swarm evidence may reorder common interoperability work, but it must
not weaken integrity, privacy, or lifecycle invariants.

The four planned tacticals are child slices, not a competing campaign or a
change to the authoritative **Now** item. Existing topics remain the parent
tracking layer. Full snub semantics and parole isolation do not yet have
tacticals: current stalled-peer probing and corruption recovery already pass,
so a stable failing scenario or comparative result must select those changes
before their ownership is planned.

## Maintenance Contract

Every protocol tactical updates this matrix, its focused topic, and
[`capability-readiness.md`](capability-readiness.md). A row links to evidence
through its owning topic or tactical instead of copying execution logs here.

When a BEP changes upstream or deployed clients disagree, record the exact
specification revision and observed compatibility behavior in the tactical.
Never silently upgrade a support claim or describe unverified behavior as
interoperable.
