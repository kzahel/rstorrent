# Protocol Support And Conformance

Topic: `protocol-support`

Status: RSTorrent implements a bounded subset of the v1 protocol sufficient
for controlled verified downloads, BEP 9 metadata exchange, scheduled BEP 15
UDP tracker announces, an IPv4 Mainline DHT foundation, and bounded
multi-peer payload seeding including one externally verified UPnP-mapped TCP
path. UDP, HTTP, and HTTPS trackers share the selected family-correct
advertisement lifecycle; DHT advertises the selected
IPv4 TCP endpoint for an eligible completed seed. It does not claim complete
BEP 3 or BEP 5 support, full BEP 7 announcing, public-swarm advertisement
reliability, incomplete-torrent upload policy, uTP, or v2 support. HTTPS now
defaults to authenticated desktop/Android platform trust; one explicit hidden
compatibility policy remains encrypted but unauthenticated.

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
| [BEP 3: The BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html) | Partial | Strict bounded bencoding; distinct `length` and `files` publication shapes over one v1 storage/resume pipeline; raw-info SHA-1 identity; outgoing and bounded multi-peer incoming TCP handshakes; exact self and duplicate peer-ID admission; keepalive, choke, unchoke, interested, not-interested, have, bitfield, request, cancel, and piece messages; bounded multi-peer download scheduling; bounded verified metadata/payload upload from completed published storage under eight fixed seed slots with exact physical accounting; ordinary routed-incoming Peers/Swarm observation; and bounded HTTP tracker announce requests plus success/failure, optional interval/count, tracker-ID, and noncompact-peer responses. Deterministic, scripted adverse, simultaneous controlled libtorrent/RSTorrent crossed connections in both peer-ID orderings, restart, crash, live publication, controlled authenticated tracker/DHT discovery, and mapped off-LAN transfers cover the implemented subset. | Peer IDs are spoofable live claims rather than authenticated durable identities. Upload from incomplete torrents, tit-for-tat and finite-bandwidth/goal policy, and comparable ordinary-swarm completion performance remain absent. |
| [BEP 5: DHT Protocol](https://www.bittorrent.org/beps/bep_0005.html) | Partial | Bounded KRPC, fixed-distance K=8 routing and replacements, alpha-3 iterative lookup, exact transaction/source correlation, incoming query handling, token-authenticated peer announcements, bootstrap/refresh, warm persistence, network-policy integration, and ordinary peer-registry delivery. One long-lived session scheduler self-announces the selected explicit TCP port to at most K=8 token-bearing responders only for verified public incoming-routable seeds; the shared IPv4 UDP endpoint remains separate. Deterministic, scripted runtime, controlled DHT-only libtorrent completion, mapped off-LAN wire-port, repeated public metadata acquisition, and public-bootstrap evidence pass. | IPv6 participation and BEP 5 PORT messages remain absent. BEP 5 has no immediate withdrawal query; stopped announcements expire as remote soft state. |
| [BEP 6: Fast Extension](https://www.bittorrent.org/beps/bep_0006.html) | Supported | Bilateral reserved-bit negotiation; strict suggest, have-all, have-none, reject-request, and allowed-fast codecs; exactly-one initial availability; request retention across choke; exact reject ownership and immediate refill; one terminal upload response through cancel/read/choke/shutdown races; bounded advisory ranking; and canonical IPv4 allowed-fast generation. Deterministic race/bound tests and controlled capture prove both pinned-libtorrent `2.0.13.0` transfer directions, magnet metadata-to-content continuity, and an explicit sub-millisecond reject/refill path with exact payload verification. | Predictive requests and super-seeding are optional behavior and remain absent. BEP 6 defines no IPv6 allowed-fast generation, so IPv6 retains the negotiated reject/availability lifecycle without an invented set. Tactical [`093`](../tactical/093-bep6-fast-request-lifecycle.md) is the completed execution record. |
| [BEP 7: IPv6 Tracker Extension](https://www.bittorrent.org/beps/bep_0007.html) | Partial | HTTP/HTTPS tracker connections may use IPv6 literals or AAAA-only names, family selection precedes query construction, an IPv6 request advertises port `1`, compact `peers6` is accepted independently of tracker family, and returned endpoints feed the ordinary outbound IPv6 TCP path. Scripted AAAA-only/only-`peers6`, application hash-verified IPv6 transfer, and Android HTTPS evidence pass. | The listener, mapping, and advertised reachable endpoint are IPv4-only. There is no dual-stack/multi-interface listener, per-family reachable port, simultaneous family announce, IPv6 pinhole, or physical IPv6 reachability evidence; no full BEP 7 claim is made. |
| [BEP 9: Extension for Peers to Send Metadata Files](https://www.bittorrent.org/beps/bep_0009.html) | Supported | Bounded v1 `btih` magnets, extension negotiation, metadata size and block bounds, at most two acquisition requests per download peer, request/data/reject messages, duplicate and ordering validation, cross-peer block assembly, assembled info-hash verification and generation recovery, up to eight simultaneous dial/metadata work items, an independent progress deadline, inspectable acquisition state, and bounded diagnostic plus shared multi-peer application-listener metadata upload. Controlled libtorrent runs pass in both directions; the maximum receive proof transfers an exact 31,457,280-byte info dictionary in 1,920 blocks and requests, and local upload serves every block of valid metadata up to 64 MiB. | The first complete hash-verified download generation wins. V2 identities and BEP 53 selection are outside the claim; public-swarm discovery and completion remain variable rather than a reliability claim. |
| [BEP 10: Extension Protocol](https://www.bittorrent.org/beps/bep_0010.html) | Partial | Reserved-bit negotiation, extended message framing, directional extension IDs, and bounded extended handshake fields needed for `ut_metadata`; deterministic and controlled libtorrent evidence covers outgoing and application-listener directions, including metadata-to-payload continuity. | There is no general recognized-extension map or support for PEX, hole punching, upload-only, client-version fields, or arbitrary extensions. Planned Tactical [`094`](../tactical/094-bounded-bep11-peer-exchange.md) adds only the bounded per-connection map and listen-port field required for `ut_metadata` plus `ut_pex`; it is not a plugin framework. |
| [BEP 11: Peer Exchange](https://www.bittorrent.org/beps/bep_0011.html) | Unsupported | None. | Requires BEP 10 extension growth, multiple live peers, source diversity limits, deduplication, rate bounds, private-torrent gating, and connection lifecycle evidence. Planned Tactical [`094`](../tactical/094-bounded-bep11-peer-exchange.md) records the complete bounded slice and depends on peer-ID duplicate resolution plus truthful peer advertisement. |
| [BEP 12: Multitracker Metadata Extension](https://www.bittorrent.org/beps/bep_0012.html) | Supported | Bounded outer `announce-list` parsing preserves tier order and falls back to `announce`; exact tiers and URL source survive restart. UDP/HTTP/HTTPS rows share deterministic tier scheduling and an eight-operation ceiling; pure tests retain 300 trackers in three tiers, controlled byte intake completes through an imported tracker, and mixed-transport lifecycle tests pass. HTTPS defaults to platform chain/name validation and the explicit compatibility value is projected as unauthenticated. | Magnet `tr` values still form one synthetic tier because magnets carry no BEP 12 structure. |
| [BEP 14: Local Service Discovery](https://www.bittorrent.org/beps/bep_0014.html) | Unsupported | None. | Requires an advertised incoming port, per-interface multicast ownership, local-network permission, and private-torrent policy. |
| [BEP 15: UDP Tracker Protocol for BitTorrent](https://www.bittorrent.org/beps/bep_0015.html) | Partial | Bounded connect and announce codecs, source/action/transaction/stride validation, IPv4 and IPv6 compact response parsing, DNS and destination policy, connect/announce retransmission, 30-second aggregate operation bounds, 60-second connection-token cache, session-wide eight-operation startup fan-out, multi-tracker fallback, failure backoff, success promotion, interval/corrective reannounce, exact current counters, selected TCP port or port-`1` outbound-only sentinel, and started/completed/stopped lifecycle. One application-lifetime peer ID is shared by tracker and peer handshakes. Scripted loss/lifecycle tests, controlled libtorrent tracker-only completion, mapped off-LAN wire-port transfer, and public metadata acquisitions pass. | Scrape, authentication, proxies, BEP 41 URL data, durable lifetime traffic accounting, and a public-tracker reliability claim remain absent. |
| [BEP 17: HTTP Seeding (Hoffman-style)](https://www.bittorrent.org/beps/bep_0017.html) | Unsupported | None. | Web seeds need their own hostile-response, range, retry, integrity, and resource policy. |
| [BEP 19: HTTP/FTP Seeding (GetRight-style)](https://www.bittorrent.org/beps/bep_0019.html) | Unsupported | None. | Web seeds are outside the current peer-transfer owner. |
| [BEP 23: Tracker Returns Compact Peer Lists](https://www.bittorrent.org/beps/bep_0023.html) | Supported | HTTP/HTTPS announces request `compact=1`; bounded response parsing accepts ordered six-byte compact IPv4 peers, discards one incomplete trailing suffix without shifting alignment, deduplicates endpoints, and continues to accept advisory noncompact responses. Deterministic hostile parsing, scripted HTTP, controlled libtorrent tracker discovery, and application transfer evidence pass. | The 200-peer response cap is deliberate. BEP 15 retains its independent UDP compact response shape. |
| [BEP 27: Private Torrents](https://www.bittorrent.org/beps/bep_0027.html) | Partial | Verified v1 metadata retains the private flag. Verified private resume suppresses DHT before lookup; private metadata learned after premetadata discovery cancels DHT and purges DHT-only peers before content scheduling. Exact outer `.torrent` intake and announce-tier retention use the same gating. Deterministic and application runtime tests cover the transitions. | PEX and LSD remain absent rather than gated implementations. Planned Tactical [`094`](../tactical/094-bounded-bep11-peer-exchange.md) keeps PEX disabled while privacy is unknown and purges PEX-only observations on a private transition. |
| [BEP 29: uTorrent Transport Protocol](https://www.bittorrent.org/beps/bep_0029.html) | Unsupported | None. | Peer transport is TCP only. A shared UDP receive waist now exists, but uTP classification, congestion control, connection state, MTU, timers, and interoperability require a dedicated tactical. |
| [BEP 32: IPv6 extension for DHT](https://www.bittorrent.org/beps/bep_0032.html) | Partial | KRPC codecs support `want`, `nodes6`, and compact IPv6 peer values; DHT state and persisted samples are address-family separated. Deterministic wire and state tests pass. | No IPv6 UDP socket, bootstrap, traversal, or public interoperability evidence exists. |
| [BEP 40: Canonical Peer Priority](https://www.bittorrent.org/beps/bep_0040.html) | Unsupported | None. | Current deterministic selection is local policy, not canonical peer priority. Planned PEX Tactical [`094`](../tactical/094-bounded-bep11-peer-exchange.md) requires source and address diversity without claiming BEP 40; its evidence may select a later canonical-priority slice. |
| [BEP 41: UDP Tracker Protocol Extensions](https://www.bittorrent.org/beps/bep_0041.html) | Unsupported | The BEP 15 parser tolerates datagram length according to its own bounds, but emits no extension fields. | URL data, authentication, and future extension negotiation are absent. |
| [BEP 42: DHT Security Extension](https://www.bittorrent.org/beps/bep_0042.html) | Partial | Address-bound IPv4 node-ID generation/validation follows published vectors. External-address votes can replace an invalid local identity, invalid remote IDs are excluded from routing, and warm state retains the selected identity. | IPv6 runtime behavior and broader public-network identity-change evidence are absent. The BEP's local/private-address exemption is retained. |
| [BEP 43: Read-only DHT Nodes](https://www.bittorrent.org/beps/bep_0043.html) | Partial | KRPC parses and emits `ro`; nodes declaring read-only are not admitted from incoming queries. Deterministic codec and admission tests pass. | Product policy does not yet select read-only mode for uncontactable, metered, VPN, or Android lifecycle states. |
| [BEP 47: Padding files and extended file attributes](https://www.bittorrent.org/beps/bep_0047.html) | Partial | Multi-file `p` attributes produce synthetic zero ranges for verification without writing padding files. Deterministic storage-layout and controlled selective-file evidence passes. | Symlinks are explicitly rejected. Executable, hidden, and per-file SHA-1 attributes are not product behavior. |
| [BEP 48: Tracker Protocol Extension: Scrape](https://www.bittorrent.org/beps/bep_0048.html) | Unsupported | None. | Tracker scrape values and application presentation are absent. |
| [BEP 52: The BitTorrent Protocol Specification v2](https://www.bittorrent.org/beps/bep_0052.html) | Unsupported | Metainfo and magnet parsers explicitly reject v2 and hybrid identities. | SHA-256 identities, file trees, piece layers, aligned storage, hybrid validation, and v2 peer behavior require a separate correctness design. |
| [BEP 53: Magnet URI extension - Select specific file indices for download](https://www.bittorrent.org/beps/bep_0053.html) | Unsupported | None. Planned Tactical [`100`](../tactical/100-bep53-select-only-and-duplicate-add-feedback.md) covers strict bounded `so` parsing, compact pre-metadata intent, default-plus-exceptions persistence, and additive duplicate selection. | Ordinary duplicate adds become typed successful no-ops; only explicit `so` may promote skipped files. Support waits for deterministic, restart, resource, product, and controlled libtorrent evidence. |
| [BEP 55: Holepunch extension](https://www.bittorrent.org/beps/bep_0055.html) | Unsupported | None. | Depends on PEX, uTP, extension negotiation, incoming reachability, address policy, and NAT behavior. |

## Gateway Control Protocols

| Specification | Claim | Implemented subset and evidence | Deliberate limits and dependencies |
| --- | --- | --- | --- |
| UPnP Device Architecture 2.0 and Internet Gateway Device v2 | Supported subset | Source-bound SSDP root-device discovery, bounded same-host IPv4 HTTP/XML/SOAP, complete `WANIPConnection:2` selection, external-address lookup, exact specific-entry query, finite TCP add/verify/renew/delete, typed faults, cancellation, and joined ownership. Deterministic and scripted gateway tests pass; a physical IGD v2 mapping was independently queried, tracker and DHT wire traffic carried its external TCP port, an off-LAN peer hash-verified 4,195,035 bytes through it, and independent absence plus failed reconnect proved cleanup. | Only IPv4 HTTP `WANIPConnection:2`, one TCP mapping, a 3,600-second finite lease, and the observed mechanism are claimed. IGD v1, WANPPP, HTTPS, multi-device policy, UDP, PCP, NAT-PMP, and IPv6 pinholes remain absent. |

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
3. close measured DHT gaps, beginning with IPv6 runtime participation and
   incoming listen-port/self-announcement only when their owners exist;
4. retain completed availability-ranked activation from Tactical
   [`091`](../tactical/091-availability-ranked-piece-activation.md), execute
   peer-ID duplicate resolution in Tactical
   [`090`](../tactical/090-peer-id-duplicate-connection-resolution.md), retain
   the completed BEP 6 lifecycle from Tactical
   [`093`](../tactical/093-bep6-fast-request-lifecycle.md), and evaluate
   bounded BEP 11 in Tactical
   [`094`](../tactical/094-bounded-bep11-peer-exchange.md) against the
   established peer owner;
5. retain completed platform certificate and hostname validation while adding
   dual-stack listener and per-family advertisement ownership before any full
   BEP 7 claim; and
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
