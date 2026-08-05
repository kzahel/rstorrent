# Protocol Support And Conformance

Topic: `protocol-support`

Status: RSTorrent implements a bounded subset of the v1 protocol sufficient
for controlled verified downloads, BEP 9 metadata exchange, scheduled BEP 15
UDP tracker announces, an IPv4 Mainline DHT foundation, and bounded
multi-peer loopback payload seeding. It does not claim complete BEP 3 or BEP 5
support, externally reachable seeding, incomplete-torrent upload policy, HTTP
trackers, uTP, or v2 support.

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
The claim remains loopback and complete-torrent only and does not include
public binding, listener advertisement, NAT traversal, finite bandwidth, or
ratio/time policy.

Tactical [`081`](../tactical/081-v1-torrent-byte-intake.md) implements bounded
v1 outer-metainfo intake, libtorrent-aligned large-v1 limits, and BEP 12 tier
retention over the existing UDP tracker runtime. Deterministic, maximum-
resource, restart, transport and controlled pinned-libtorrent evidence pass.
HTTP/HTTPS tracker transport and v2/hybrid integrity remain absent.

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
| [BEP 3: The BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html) | Partial | Strict bounded bencoding; distinct `length` and `files` publication shapes over one v1 storage/resume pipeline; raw-info SHA-1 identity; outgoing and bounded multi-peer loopback incoming TCP handshakes; keepalive, choke, unchoke, interested, not-interested, have, bitfield, request, cancel, and piece messages; bounded multi-peer download scheduling; bounded verified metadata/payload upload from completed published storage under eight fixed seed slots with exact physical accounting; and ordinary routed-incoming Peers/Swarm observation. Deterministic, scripted adverse, simultaneous controlled libtorrent/RSTorrent, restart, crash, and live publication evidence cover the implemented subset. | Advertised/public incoming reachability, upload from incomplete torrents, tit-for-tat and finite-bandwidth/goal policy, HTTP trackers, and comparable ordinary-swarm completion performance remain absent. |
| [BEP 5: DHT Protocol](https://www.bittorrent.org/beps/bep_0005.html) | Partial | Bounded KRPC, fixed-distance K=8 routing and replacements, alpha-3 iterative lookup, exact transaction/source correlation, incoming query handling, token-authenticated peer announcements, bootstrap/refresh, warm persistence, network-policy integration, and ordinary peer-registry delivery. Deterministic, scripted runtime, controlled libtorrent, repeated public metadata acquisition, and public-bootstrap evidence pass. | The runtime operates one IPv4 UDP socket. IPv6 participation and self-announcement remain absent until their socket and real incoming peer-port owners exist. |
| [BEP 6: Fast Extension](https://www.bittorrent.org/beps/bep_0006.html) | Unsupported | None. | Have-all, have-none, suggest, reject-request, and allowed-fast negotiation and state are absent. |
| [BEP 7: IPv6 Tracker Extension](https://www.bittorrent.org/beps/bep_0007.html) | Unsupported | None. | HTTP trackers are absent. BEP 15 UDP response parsing can represent bounded IPv6 compact peers, which is not a BEP 7 support claim. |
| [BEP 9: Extension for Peers to Send Metadata Files](https://www.bittorrent.org/beps/bep_0009.html) | Supported | Bounded v1 `btih` magnets, extension negotiation, metadata size and block bounds, at most two acquisition requests per download peer, request/data/reject messages, duplicate and ordering validation, cross-peer block assembly, assembled info-hash verification and generation recovery, up to eight simultaneous dial/metadata work items, an independent progress deadline, inspectable acquisition state, and bounded diagnostic plus shared multi-peer application-listener metadata upload. Controlled libtorrent runs pass in both directions; the maximum receive proof transfers an exact 31,457,280-byte info dictionary in 1,920 blocks and requests, and local upload serves every block of valid metadata up to 64 MiB. | The first complete hash-verified download generation wins. V2 identities and BEP 53 selection are outside the claim; public reachability is separately unimplemented. |
| [BEP 10: Extension Protocol](https://www.bittorrent.org/beps/bep_0010.html) | Partial | Reserved-bit negotiation, extended message framing, directional extension IDs, and bounded extended handshake fields needed for `ut_metadata`; deterministic and controlled libtorrent evidence covers outgoing and application-listener directions, including metadata-to-payload continuity. | There is no general extension registry or support for PEX, hole punching, upload-only, client-version fields, or arbitrary extensions. |
| [BEP 11: Peer Exchange](https://www.bittorrent.org/beps/bep_0011.html) | Unsupported | None. | Requires BEP 10 extension growth, multiple live peers, source diversity limits, deduplication, rate bounds, private-torrent gating, and connection lifecycle evidence. |
| [BEP 12: Multitracker Metadata Extension](https://www.bittorrent.org/beps/bep_0012.html) | Supported | Bounded outer `announce-list` parsing preserves tier order and falls back to `announce`; exact tiers, URL source and unsupported transport state survive restart. UDP rows use deterministic tier scheduling, while pure tests retain 300 trackers in three tiers and controlled byte intake completes through the imported tracker. | HTTP and HTTPS tracker rows are retained and visible but their wire transports are absent. Magnet `tr` values still form one synthetic tier because magnets carry no BEP 12 structure. |
| [BEP 14: Local Service Discovery](https://www.bittorrent.org/beps/bep_0014.html) | Unsupported | None. | Requires an advertised incoming port, per-interface multicast ownership, local-network permission, and private-torrent policy. |
| [BEP 15: UDP Tracker Protocol for BitTorrent](https://www.bittorrent.org/beps/bep_0015.html) | Partial | Bounded connect and announce codecs, source/action/transaction/stride validation, IPv4 and IPv6 compact response parsing, DNS and destination policy, connect/announce retransmission, 30-second aggregate operation bounds, 60-second connection-token cache, concurrent startup fan-out, multi-tracker fallback, failure backoff, success promotion, interval reannounce, and an explicit provisional port 6881. One application-lifetime peer ID is now shared by tracker and peer handshakes. Scripted loss tests, controlled libtorrent tracker-only downloads, and public metadata acquisitions pass. | Runtime announces still use placeholder transfer counters, and port 6881 is not derived from or guaranteed to match the loopback listener; completed and stopped events, scrape, authentication, proxies, and a shared session-wide tracker budget are absent. |
| [BEP 17: HTTP Seeding (Hoffman-style)](https://www.bittorrent.org/beps/bep_0017.html) | Unsupported | None. | Web seeds need their own hostile-response, range, retry, integrity, and resource policy. |
| [BEP 19: HTTP/FTP Seeding (GetRight-style)](https://www.bittorrent.org/beps/bep_0019.html) | Unsupported | None. | Web seeds are outside the current peer-transfer owner. |
| [BEP 23: Tracker Returns Compact Peer Lists](https://www.bittorrent.org/beps/bep_0023.html) | Unsupported | None for HTTP tracker responses. | BEP 15 has its own compact UDP response shapes. HTTP tracker request and bencoded response handling are absent. |
| [BEP 27: Private Torrents](https://www.bittorrent.org/beps/bep_0027.html) | Partial | Verified v1 metadata retains the private flag. Verified private resume suppresses DHT before lookup; private metadata learned after premetadata discovery cancels DHT and purges DHT-only peers before content scheduling. Exact outer `.torrent` intake and announce-tier retention use the same gating. Deterministic and application runtime tests cover the transitions. | PEX and LSD remain absent rather than gated implementations. |
| [BEP 29: uTorrent Transport Protocol](https://www.bittorrent.org/beps/bep_0029.html) | Unsupported | None. | Peer transport is TCP only. Congestion control, socket ownership, MTU, timers, and network binding require a dedicated tactical. |
| [BEP 32: IPv6 extension for DHT](https://www.bittorrent.org/beps/bep_0032.html) | Partial | KRPC codecs support `want`, `nodes6`, and compact IPv6 peer values; DHT state and persisted samples are address-family separated. Deterministic wire and state tests pass. | No IPv6 UDP socket, bootstrap, traversal, or public interoperability evidence exists. |
| [BEP 40: Canonical Peer Priority](https://www.bittorrent.org/beps/bep_0040.html) | Unsupported | None. | Current deterministic selection is local policy, not canonical peer priority. Revisit with a bounded live-peer set and incoming connections. |
| [BEP 41: UDP Tracker Protocol Extensions](https://www.bittorrent.org/beps/bep_0041.html) | Unsupported | The BEP 15 parser tolerates datagram length according to its own bounds, but emits no extension fields. | URL data, authentication, and future extension negotiation are absent. |
| [BEP 42: DHT Security Extension](https://www.bittorrent.org/beps/bep_0042.html) | Partial | Address-bound IPv4 node-ID generation/validation follows published vectors. External-address votes can replace an invalid local identity, invalid remote IDs are excluded from routing, and warm state retains the selected identity. | IPv6 runtime behavior and broader public-network identity-change evidence are absent. The BEP's local/private-address exemption is retained. |
| [BEP 43: Read-only DHT Nodes](https://www.bittorrent.org/beps/bep_0043.html) | Partial | KRPC parses and emits `ro`; nodes declaring read-only are not admitted from incoming queries. Deterministic codec and admission tests pass. | Product policy does not yet select read-only mode for uncontactable, metered, VPN, or Android lifecycle states. |
| [BEP 47: Padding files and extended file attributes](https://www.bittorrent.org/beps/bep_0047.html) | Partial | Multi-file `p` attributes produce synthetic zero ranges for verification without writing padding files. Deterministic storage-layout and controlled selective-file evidence passes. | Symlinks are explicitly rejected. Executable, hidden, and per-file SHA-1 attributes are not product behavior. |
| [BEP 48: Tracker Protocol Extension: Scrape](https://www.bittorrent.org/beps/bep_0048.html) | Unsupported | None. | Tracker scrape values and application presentation are absent. |
| [BEP 52: The BitTorrent Protocol Specification v2](https://www.bittorrent.org/beps/bep_0052.html) | Unsupported | Metainfo and magnet parsers explicitly reject v2 and hybrid identities. | SHA-256 identities, file trees, piece layers, aligned storage, hybrid validation, and v2 peer behavior require a separate correctness design. |
| [BEP 53: Magnet URI extension - Select specific file indices for download](https://www.bittorrent.org/beps/bep_0053.html) | Unsupported | The product has an explicit `skip_files` command field after metadata, but does not parse magnet `so`. | Adding `so` requires canonicalization, conflict/idempotency behavior, bounds, and UI intent rules. |
| [BEP 55: Holepunch extension](https://www.bittorrent.org/beps/bep_0055.html) | Unsupported | None. | Depends on PEX, uTP, extension negotiation, incoming reachability, address policy, and NAT behavior. |

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
4. add measured picker and connection-set behavior, then evaluate BEP 6
   reject semantics and BEP 11 PEX against the established peer owner;
5. implement HTTP and HTTPS trackers, including hostile response, redirect,
   credential and BEP 23 policy, over the retained metainfo tracker catalog;
   and
6. evaluate incoming service, uTP, hole punching, web seeds, and v2 only after
   their prerequisite owners and validation plans exist.

This order is a default, not a promise to implement every listed proposal.
New real-swarm evidence may reorder common interoperability work, but it must
not weaken integrity, privacy, or lifecycle invariants.

## Maintenance Contract

Every protocol tactical updates this matrix, its focused topic, and
[`capability-readiness.md`](capability-readiness.md). A row links to evidence
through its owning topic or tactical instead of copying execution logs here.

When a BEP changes upstream or deployed clients disagree, record the exact
specification revision and observed compatibility behavior in the tactical.
Never silently upgrade a support claim or describe unverified behavior as
interoperable.
