# Protocol Support And Conformance

Topic: `protocol-support`

Status: RSTorrent implements a bounded subset of the v1 protocol sufficient
for controlled verified downloads, BEP 9 metadata exchange, and scheduled
BEP 15 UDP tracker announces. It does not claim complete BEP 3 support, general
swarm reliability, seeding, decentralized discovery, HTTP trackers, uTP, or v2
support.

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
| [BEP 3: The BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html) | Partial | Strict bounded bencoding; v1 single- and multi-file info dictionaries; raw-info SHA-1 identity; TCP handshake; keepalive, choke, unchoke, interested, not-interested, have, bitfield, request, and piece messages; bounded block pipelining; full-piece SHA-1 verification. Deterministic tests and controlled libtorrent downloads cover the implemented subset. | No cancel message, general payload upload, incoming peer service, multi-peer content scheduling, endgame, choking algorithm, HTTP tracker, or reliable ordinary-swarm completion. Multi-piece single-file execution is rejected. |
| [BEP 5: DHT Protocol](https://www.bittorrent.org/beps/bep_0005.html) | Unsupported | None. | Requires bounded routing state, hostile-message policy, bootstrap and persistence ownership, network-policy integration, and BEP 27 private-torrent gating. |
| [BEP 6: Fast Extension](https://www.bittorrent.org/beps/bep_0006.html) | Unsupported | None. | Have-all, have-none, suggest, reject-request, and allowed-fast negotiation and state are absent. |
| [BEP 7: IPv6 Tracker Extension](https://www.bittorrent.org/beps/bep_0007.html) | Unsupported | None. | HTTP trackers are absent. BEP 15 UDP response parsing can represent bounded IPv6 compact peers, which is not a BEP 7 support claim. |
| [BEP 9: Extension for Peers to Send Metadata Files](https://www.bittorrent.org/beps/bep_0009.html) | Supported | Bounded v1 `btih` magnets, extension negotiation, metadata size and block bounds, at most two acquisition requests in flight, request/data/reject messages, duplicate and ordering validation, assembled info-hash verification, and a bounded diagnostic metadata uploader. Controlled libtorrent runs pass in both directions. | The claim is v1 metadata exchange only. Simultaneous metadata peers, v2 identities, BEP 53 selection, and general seeding are outside it. |
| [BEP 10: Extension Protocol](https://www.bittorrent.org/beps/bep_0010.html) | Partial | Reserved-bit negotiation, extended message framing, directional extension IDs, and bounded extended handshake fields needed for `ut_metadata`; deterministic and libtorrent evidence exists. | There is no general extension registry or support for PEX, hole punching, upload-only, client-version fields, or arbitrary extensions. |
| [BEP 11: Peer Exchange](https://www.bittorrent.org/beps/bep_0011.html) | Unsupported | None. | Requires BEP 10 extension growth, multiple live peers, source diversity limits, deduplication, rate bounds, private-torrent gating, and connection lifecycle evidence. |
| [BEP 12: Multitracker Metadata Extension](https://www.bittorrent.org/beps/bep_0012.html) | Unsupported | None for `.torrent` `announce-list`. | Multiple magnet `tr` values work as one shuffled synthetic tier, but magnets do not encode BEP 12 tiers and that behavior is not a BEP 12 claim. |
| [BEP 14: Local Service Discovery](https://www.bittorrent.org/beps/bep_0014.html) | Unsupported | None. | Requires an advertised incoming port, per-interface multicast ownership, local-network permission, and private-torrent policy. |
| [BEP 15: UDP Tracker Protocol for BitTorrent](https://www.bittorrent.org/beps/bep_0015.html) | Partial | Bounded connect and announce codecs, source/action/transaction/stride validation, IPv4 and IPv6 compact response parsing, DNS and destination policy, connect/announce retransmission, 30-second aggregate operation bounds, 60-second connection-token cache, multi-tracker fallback, failure backoff, success promotion, and interval reannounce. Scripted loss tests and controlled libtorrent tracker-only downloads pass. | Runtime announces still use placeholder transfer counters and port zero; completed and stopped events, scrape, authentication, proxies, shared session budgets, and multiple simultaneous operations are absent. |
| [BEP 17: HTTP Seeding (Hoffman-style)](https://www.bittorrent.org/beps/bep_0017.html) | Unsupported | None. | Web seeds need their own hostile-response, range, retry, integrity, and resource policy. |
| [BEP 19: HTTP/FTP Seeding (GetRight-style)](https://www.bittorrent.org/beps/bep_0019.html) | Unsupported | None. | Web seeds are outside the current peer-transfer owner. |
| [BEP 23: Tracker Returns Compact Peer Lists](https://www.bittorrent.org/beps/bep_0023.html) | Unsupported | None for HTTP tracker responses. | BEP 15 has its own compact UDP response shapes. HTTP tracker request and bencoded response handling are absent. |
| [BEP 27: Private Torrents](https://www.bittorrent.org/beps/bep_0027.html) | Unsupported | Current lack of DHT, PEX, and LSD prevents those sources from leaking peers, but the private flag is not retained as product policy. | Must be implemented before any decentralized or local discovery source can be enabled. Tracker-tier interaction and persisted intent require tests. |
| [BEP 29: uTorrent Transport Protocol](https://www.bittorrent.org/beps/bep_0029.html) | Unsupported | None. | Peer transport is TCP only. Congestion control, socket ownership, MTU, timers, and network binding require a dedicated tactical. |
| [BEP 40: Canonical Peer Priority](https://www.bittorrent.org/beps/bep_0040.html) | Unsupported | None. | Current deterministic selection is local policy, not canonical peer priority. Revisit with a bounded live-peer set and incoming connections. |
| [BEP 41: UDP Tracker Protocol Extensions](https://www.bittorrent.org/beps/bep_0041.html) | Unsupported | The BEP 15 parser tolerates datagram length according to its own bounds, but emits no extension fields. | URL data, authentication, and future extension negotiation are absent. |
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

Protocol breadth follows the ownership needed for correctness:

1. finish bounded multi-peer request ownership and completion recovery under
   the existing BEP 3 subset;
2. add the core cancel message and endgame behavior, then decide whether the
   BEP 6 reject semantics materially improve the scheduler;
3. retain outer `.torrent` announce data and implement BEP 12 tiers plus HTTP
   and HTTPS trackers, including BEP 23 responses;
4. retain and enforce BEP 27 private intent;
5. add BEP 5 DHT and BEP 11 PEX as separately bounded discovery sources; and
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
