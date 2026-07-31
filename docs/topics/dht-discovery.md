# DHT Discovery

Topic: `dht-discovery`

Status: Planned as the first major engine feature campaign after a bounded
headless live-comparison harness. RSTorrent currently has no DHT
implementation. The campaign must deliver useful warm-restart peer discovery,
not only KRPC codecs or a disposable query client.

## Why DHT Is First

Tracker discovery is useful but cannot be the only ordinary peer source.
Public trackers are independently operated, can time out, and may not know all
peers in a swarm. DHT is therefore the highest-value breadth improvement now
that scheduled tracker announces and controlled single-peer downloads exist.

DHT and multi-peer transfer remain adjacent priorities:

- DHT supplies peers when trackers are absent, slow, or incomplete.
- Multi-peer ownership lets the transfer engine exploit newly discovered and
  late-arriving peers instead of merely replacing one pre-content connection.
- Endgame and piece-picker work then make that peer set useful near completion
  and at realistic throughput.

The only work ahead of DHT should be enabling evidence infrastructure: a
headless RSTorrent/libtorrent live-smoke comparator and any bounded reference
checkout tooling it requires. That is proof infrastructure, not a competing
product feature.

## Scope And Protocol Baseline

The initial capability is a session-level BEP 5 participant with bounded,
testable state. It includes:

- strict bencoded KRPC request, response, and error handling;
- transaction correlation and endpoint validation;
- IPv4 routing buckets and explicit good, questionable, and bad node state;
- bounded iterative `find_node` and `get_peers` traversals;
- token handling sufficient to respond correctly to ordinary queries;
- incoming `ping`, `find_node`, `get_peers`, and `announce_peer` handling;
- bootstrap, refresh, timeout, retry, replacement, and shutdown behavior;
- integration of returned peers through the existing `PeerObservation` and
  registry boundary;
- BEP 42-aware node identity behavior; and
- BEP 27 private-torrent gating before decentralized discovery is enabled.

The internal model must allow independent IPv4 and IPv6 routing tables as
specified by BEP 32, even if IPv4 interoperability is the first landed slice.
The tactical that defers IPv6 runtime support must name the exact boundary and
must not make IPv4-specific assumptions part of protocol state.

Normative starting points are [BEP 5](https://www.bittorrent.org/beps/bep_0005.html),
[BEP 27](https://www.bittorrent.org/beps/bep_0027.html),
[BEP 32](https://www.bittorrent.org/beps/bep_0032.html),
[BEP 42](https://www.bittorrent.org/beps/bep_0042.html), and
[BEP 43](https://www.bittorrent.org/beps/bep_0043.html). The pinned libtorrent
implementation and tests are the required completeness and edge-case oracle;
they do not dictate RSTorrent's module boundaries.

## Ownership And Boundaries

DHT is shared session infrastructure, not per-torrent state.

- Pure protocol values, message codecs, routing-table transitions, traversal
  decisions, token validation, and snapshot values must not depend on Tokio,
  sockets, filesystems, task handles, or application adapters.
- One engine session owner holds routing tables, outstanding transactions,
  lookup budgets, maintenance timers, and the UDP socket runtime.
- Each background task has an explicit cancellation signal and observable
  termination path. Shutdown stops new work, drains or cancels bounded work,
  snapshots eligible state, and closes the socket.
- Torrent discovery requests enter through a bounded engine command or queue.
  Results leave through the same source-neutral peer-observation boundary used
  by trackers and explicit hints.
- The session persistence layer stores a versioned bounded snapshot supplied
  by the engine. Protocol and routing code do not know about SQLite, Android
  storage, desktop paths, or product lifecycle APIs.
- Existing online, loopback-only, and offline egress policy applies to every
  bootstrap, lookup, maintenance, response, and announce operation. Future VPN
  and metered-network policy extends that owner instead of bypassing it.

Before implementation, each tactical records the concrete owner/task/cancel
map and the intended module dependency direction. A module or crate extraction
needs an ownership, dependency, reuse, lifecycle, or testability reason; DHT is
not permission for speculative framework layers.

## Fast Restart

"Fast-resume DHT" means warm session restart of the DHT network position. It is
separate from torrent piece resume.

A clean or recoverable shutdown persists only bounded durable hints:

- the node identity material needed by the selected BEP 42 policy;
- a bounded, diverse sample of recently responsive IPv4 nodes;
- an independently bounded IPv6 sample once IPv6 DHT is enabled;
- snapshot schema/version and age information needed to reject stale or
  incompatible state; and
- only additional fields proven necessary by the tactical's restart tests.

Persisted nodes return as untrusted bootstrap candidates. They must be
revalidated before becoming good routing entries. Do not persist transaction
IDs, tokens, in-flight traversals, returned torrent peers, task state, or
socket/runtime details.

Warm startup attempts saved candidates before public routers while retaining a
cold-start fallback. Metainfo bootstrap nodes and peer DHT `PORT` messages are
later bootstrap sources unless the initial tactical can add them without
weakening the owner or validation boundary.

The resume evidence must compare cold and warm starts, including corrupted,
oversized, obsolete, address-family-mismatched, and unreachable saved state.
Useful measures include time to first valid response, time to a small healthy
routing threshold, time to first peer, and persistence size.

## Privacy, Security, And Participation

Decentralized discovery must never be enabled merely because a torrent has an
info hash. The product path must retain and enforce the BEP 27 private flag.
The first tactical must explicitly define pre-metadata magnet behavior because
private intent is not known until verified metadata arrives.

All UDP and bencoded input is hostile. The design must cover malformed and
oversized dictionaries, unexpected types, duplicate or unknown keys, spoofed
endpoints, stale or colliding transactions, unsolicited responses, token
abuse, invalid node IDs, self-addresses, duplicate nodes, and address-policy
violations before mutating trusted state.

RSTorrent should be a useful participant, not a write-only crawler. Incoming
queries are part of the first useful capability. However, RSTorrent must not
send `announce_peer` with port zero or claim reachability before a real
incoming peer listener and advertised port exist. Peer lookup is still useful
without self-announcement, so the BEP 5 protocol claim will initially remain
Partial.

BEP 43 read-only behavior is relevant to a future metered or uncontactable
Android mode. It should fit the session policy model, but it is not a substitute
for normal participation on an unrestricted desktop network.

## Resource And Failure Bounds

The implementing tacticals must choose, name, and test finite limits for:

- routing nodes and replacement candidates per address family;
- concurrent transactions and traversals;
- traversal candidates, outstanding requests, and returned peer values;
- per-torrent and session lookup frequency;
- datagram and decoded-message size;
- token secrets, token lifetime, and accepted previous secrets;
- inbound query work and response amplification;
- bootstrap and maintenance retries;
- saved-node count, snapshot size, and diagnostic history; and
- channels, queues, task count, and shutdown duration.

Timeouts, unreachable bootstrap nodes, network-policy changes, socket errors,
clock movement, process restart, and partial persistence failure are ordinary
state transitions. They must not strand a lookup, leak a task, grow state
without bound, or convert untrusted persisted hints directly into trusted
routing state.

## Campaign And Evidence

The expected staged campaign is:

1. **Evidence baseline.** Add the headless live comparator, capture a
   trackerless libtorrent baseline, and record the exact pinned libtorrent DHT
   source and tests studied.
2. **Core state.** Land bounded codecs, routing state, transaction ownership,
   traversal behavior, tokens, and deterministic hostile-input tests without a
   product socket runtime.
3. **Session runtime.** Add the UDP owner, bootstrap and maintenance lifecycle,
   query responses, cancellation, network-policy integration, and controlled
   interop.
4. **Warm restart and torrent integration.** Persist bounded bootstrap hints,
   prove cold/warm recovery, enforce private intent, and feed peers into the
   ordinary registry.
5. **Live proof.** Run trackerless public-smoke comparisons and record discovery
   timing, completion outcome, resource high-water marks, and unexplained
   stalls.

Each slice follows the feature-work contract in the repository instructions:
normative specifications and exact pinned libtorrent source/tests are studied
before design is finalized; shape-changing edge cases land with the common
path; and support claims follow evidence rather than code existence.

## Deliberate Deferrals

The first DHT campaign does not imply:

- `announce_peer` before incoming peer reachability exists;
- BEP 11 PEX, BEP 14 LSD, uTP, NAT traversal, or hole punching;
- DHT scrape, mutable/immutable items, or BEP 45 multi-address announce;
- a product settings or log-window redesign;
- a remote daemon or socket control plane; or
- speed-ratio gates that fail CI on normal public-swarm variance.

These deferrals do not permit a disposable lookup client, an unbounded routing
table, or a runtime that cannot be stopped and resumed cleanly.

## Maintenance Contract

Every DHT tactical updates this topic, the BEP rows in
[`protocol-support.md`](protocol-support.md), the discovery scoreboard in
[`capability-readiness.md`](capability-readiness.md), and the live evidence in
[`performance-and-live-evidence.md`](performance-and-live-evidence.md).

Promotion from Absent requires ordinary engine integration and controlled
interop. Promotion beyond Partial requires the exact participating behavior,
address-family coverage, private policy, and evidence to be named explicitly.
