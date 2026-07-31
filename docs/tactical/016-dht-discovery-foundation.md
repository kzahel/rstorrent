# Tactical 016: DHT Discovery Foundation

Status: Complete

Topic: `dht-discovery`

## Motivation And Desired Outcome

RSTorrent currently depends on explicit hints or UDP trackers for peer
discovery. This tactical adds a functional, session-owned Mainline DHT
foundation that can bootstrap, participate in BEP 5 queries, find peers for a
trackerless magnet, feed them through the existing peer registry, survive
restart with bounded warm state, and stop cleanly.

The desired result is not merely a KRPC codec or one-shot crawler. One engine
session owns the UDP runtime and routing state independently of torrents. A
torrent submits a bounded lookup and receives source-neutral peer observations.
Product clients use the same in-process service; validation remains headless.

## Normative References

- BEP 5: Mainline DHT protocol, KRPC, routing, tokens, and peer lookup
- BEP 27: private torrents
- BEP 32: separate IPv4 and IPv6 DHT address families
- BEP 42: address-bound node-ID security
- BEP 43: read-only DHT nodes

The exact pinned BEP source is
`reference/bittorrent.org@7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`.
The tactical summarizes behavior independently and does not import BEP prose.

## Required Libtorrent Survey

Pinned reference: Rasterbar libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`.

The pre-implementation survey covers at least:

- `src/kademlia/dht_tracker.cpp`: UDP ownership, message dispatch, bootstrap,
  maintenance, and external-address behavior;
- `src/kademlia/node.cpp`: query handling, routing integration, tokens, peer
  lookup, and refresh ownership;
- `src/kademlia/routing_table.cpp` and `node_entry.cpp`: good,
  questionable, and failed nodes, bucket/replacement behavior, diversity, and
  closest-node selection;
- `src/kademlia/rpc_manager.cpp`: transaction correlation, endpoint checks,
  timeout, unsolicited responses, and concurrency bounds;
- `src/kademlia/traversal_algorithm.cpp`, `get_peers.cpp`, and `refresh.cpp`:
  iterative traversal, alpha concurrency, termination, and maintenance;
- `src/kademlia/dht_state.cpp`: durable node identity and bounded IPv4/IPv6
  bootstrap hints;
- `src/kademlia/node_id.cpp` and `dos_blocker.cpp`: BEP 42 validation and
  hostile-source throttling; and
- `test/test_dht.cpp`, `test/test_direct_dht.cpp`,
  `test/test_dht_storage.cpp`, and relevant non-GPL test support.

The GPL libsimulator submodule is not initialized or linked. Its surrounding
test case names may identify edge cases, but RSTorrent authors independent
deterministic fixtures.

## JSTorrent Survey

JSTorrent is correlated behavior evidence, not an independent protocol oracle
and not a source donor. Useful current paths include:

- `packages/engine/src/dht/` and `packages/engine/test/dht/`;
- mocked iterative lookup, packet loss, bootstrap, persistence, and lifecycle
  scenarios;
- DHT sleep/wake refresh and stale-network pruning;
- incoming query rate limiting and amplification protection;
- BEP 27 product gating and peer-source integration; and
- the 2026-02-22 IPv4-bound socket/IPv6-first DNS bootstrap incident across
  desktop and Android.

Intentional differences include a smaller Rust ownership surface, no generic
socket proxy, explicit versioned and bounded warm state, a BEP 32-compatible
state model, BEP 42 behavior in the first campaign, and stronger hostile-input
and shutdown boundaries.

## Ownership And Dependency Map

```text
rstorrent-protocol::dht
  node IDs, KRPC codecs, compact values, routing/traversal state,
  tokens/peer-store transitions, snapshot values
             ^
             |
rstorrent-engine::dht
  one session UDP owner, transactions, clocks, DNS, timers, commands,
  incoming queries, bootstrap/refresh, cancellation and stats
             ^
             |
rstorrent-session::ApplicationService
  one service instance, durable snapshot load/save, torrent lookup policy
             |
             +--> per-torrent PeerObservation::Dht
```

- Protocol state has no Tokio, socket, filesystem, channel, task, SQLite, or
  platform dependency.
- `DhtService` owns one actor task and socket set. `DhtHandle` is a bounded
  command handle; dropping handles does not detach the owner.
- `ApplicationService` starts DHT once, passes a handle to active torrent work,
  snapshots and joins it on shutdown, and persists only validated bounded
  bootstrap hints.
- Torrent pause does not destroy session DHT. Application shutdown always
  cancels and joins it.
- DHT results enter the existing `PeerRegistry` as `PeerSource::Dht`; DHT does
  not own dialing or transfer state.

## Scope

### Pure protocol and state

- A 20-byte node-ID type with XOR ordering and BEP 42 generation/validation.
- Strict KRPC query, response, and error parsing under a 1,024-byte datagram
  ceiling and small per-field/collection limits.
- Canonical encoding for `ping`, `find_node`, `get_peers`, and
  `announce_peer`, including `want`, `nodes`, `nodes6`, `values`, `token`,
  `implied_port`, `ro`, and observed `ip` fields used by the supported subset.
- Separate IPv4 and IPv6 routing tables, even if initial public runtime
  evidence is IPv4-first.
- K=8 routing buckets, bounded replacements, explicit responsive,
  questionable, and failed transitions, endpoint/node deduplication, and
  address-diversity limits.
- Bounded iterative lookup with alpha=3, endpoint and node-ID deduplication,
  exact in-flight ownership, termination, and peer/token collection.
- Rotating source-IP tokens, a bounded expiring peer store, and bounded
  incoming-source rate state.

### Runtime

- One IPv4 UDP socket in the first interoperable slice; runtime types preserve
  the independent IPv6 table and compact codecs for the next address-family
  slice.
- Bounded DNS resolution that selects only addresses matching the bound socket
  family and applies `NetworkPolicy` before every send and accepted mutation.
- Public routers, warm saved nodes, and explicit test bootstrap endpoints.
- Transaction IDs unpredictable enough to avoid trivial off-path correlation,
  unique among active requests, and correlated with exact source endpoint.
- Bootstrap, iterative lookup, query response, timeout, refresh, pruning,
  external-address voting, retry, stats, and shutdown in one supervised actor.
- Incoming query responses required for a useful participant. Invalid tokens,
  source floods, malformed messages, and amplification-prone responses fail
  closed under explicit limits.
- No `announce_peer` self-advertisement while RSTorrent has no real incoming
  peer listen port. Incoming valid announcements may be retained and served.

### Persistence and application integration

- Retain the BEP 27 `private` flag from verified v1 metadata.
- Premetadata magnets may use DHT because private intent is unknowable; once
  verified metadata is private, cancel further DHT lookup and remove DHT-only
  peer observations before content scheduling.
- Add a transactional schema migration for one versioned DHT identity and a
  bounded, address-family-separated node sample.
- Persist no transactions, tokens, peer values, lookup state, task state,
  failure counts, or runtime/socket details.
- Treat restored nodes as untrusted bootstrap candidates requiring a valid
  response before routing-table admission.
- Prefer a diverse recent saved sample, cap it independently of routing-table
  capacity, reject malformed/newer state conservatively, and retain cold
  public-router fallback.
- Add typed DHT activity needed by headless diagnostics without changing web,
  Tauri, or Android UI.

## Initial Resource Limits

The implementation may tighten these values when reference study or tests
justify it, but may not silently remove the bound:

| Resource | Initial bound |
| --- | --- |
| UDP datagram | 1,024 bytes |
| Routing nodes | 8 × 160 per address family |
| Replacement nodes | 8 per bucket |
| Active transactions | 256 session-wide |
| Active traversals | 16 session-wide, one per info hash |
| Traversal candidates | 256 |
| Traversal in flight | alpha 3 |
| Returned peers per lookup | 200 |
| Peer-store info hashes | 256 |
| Peer-store peers per hash | 100 |
| Incoming rate sources | 1,024 |
| Incoming queries | 30/source/minute plus a global bound |
| Persisted bootstrap nodes | 64 IPv4 and 64 IPv6 |
| Actor command queue | 64 |
| Lookup duration | 30 seconds |
| Shutdown join | bounded by task cancellation, with no network wait required |

## Shape-Changing Edge Cases Required In-Slice

- malformed, oversized, deeply nested, duplicate-key, wrong-type, missing,
  and unknown KRPC values;
- stale, unknown, colliding, and wrong-endpoint transaction responses;
- self IDs, duplicate IDs/endpoints, invalid BEP 42 IDs, invalid compact
  lengths, zero/invalid ports, unspecified/multicast/broadcast addresses;
- full buckets, questionable nodes, repeated failures, replacements, address
  diversity, and routing-table restart;
- all bootstrap routers unreachable, one router succeeding late, saved nodes
  stale, and cold fallback after warm failure;
- lookup cycles, duplicate candidates, out-of-order responses, packet loss,
  partial success, alpha bound, cancellation, and timeout;
- token rotation, previous-secret acceptance, invalid token, implied port,
  peer TTL/capacity, incoming source/global rate bounds, and response-size
  truncation;
- offline and loopback policies, IPv6-first DNS on the IPv4 socket, pause,
  shutdown, dropped command receivers, and actor failure;
- corrupt, oversized, unknown-version, and partially written persistence;
- private metadata arriving after premetadata DHT discovery; and
- honest diagnostics when DHT produces no peer rather than a terminal blocked
  state.

## Non-Goals

- No product UI changes.
- No self-announcement before incoming peer listening exists.
- No payload upload, PEX, LSD, uTP, NAT traversal, or hole punching.
- No BEP 44 items, BEP 51 indexing, DHT scrape, or BEP 45 multi-address
  announce.
- No general IPv6 UDP runtime claim in this slice unless controlled evidence
  can be added without weakening the stopping condition.
- No libtorrent/JSTorrent source reuse or architecture port.
- No public speed regression threshold.

## Validation

1. Pure unit tests for codecs, BEP 42, routing, traversal, tokens, peer store,
   persistence values, malformed input, and every declared resource bound.
2. Scripted loopback UDP networks for bootstrap, loss, timeout, source
   correlation, incoming queries, cancellation, warm restart, and private
   transition behavior.
3. Controlled independent interoperability: RSTorrent obtains a peer through
   KRPC and downloads verified metadata/content from a libtorrent peer; a
   separate query verifies RSTorrent's incoming response behavior.
4. Application-service forced restart: first run builds DHT state, shutdown
   persists it, second run reaches a responsive saved node before cold fallback
   and finds the peer.
5. Opt-in public trackerless smoke through Tactical 015, comparing cold and
   warm discovery timing with pinned libtorrent and retaining bounded evidence
   on an actionable mismatch.
6. Full Rust formatting, clippy, tests, architecture gate, locked interop
   environment, no-launch desktop build, and Android Rust cross-build because
   the shared in-process engine changes.

## Stopping Condition

This tactical is complete when a trackerless magnet can obtain peers from DHT
through the ordinary session and peer-registry path, acquire verified metadata,
and complete a supported controlled download; RSTorrent answers the supported
incoming BEP 5 queries; clean restart demonstrably uses only bounded revalidated
warm hints; private metadata stops decentralized discovery; all DHT tasks and
sockets join on shutdown; and controlled plus at least one attempted public
trackerless result are recorded honestly.

The support claim remains Partial because IPv6 runtime operation and
self-announcement are deliberately absent. Bounded multi-peer transfer remains
the next feature campaign after this foundation.

## Implementation Outcome

The completed slice adds:

- runtime-independent DHT endpoint, node-ID, KRPC, BEP 42, and bounded routing
  state in `rstorrent-protocol`;
- one supervised, session-owned IPv4 UDP actor with exact transaction/source
  correlation, alpha-3 iterative lookups, bounded routing and replacement
  state, rotating tokens, an expiring peer store, incoming-query throttling,
  staged warm/cold bootstrap, periodic rebootstrap/refresh, and clean join;
- DHT peer observations through the existing registry and tracker/DHT racing,
  with transient no-node/timeout results retried under bounded backoff rather
  than becoming a terminal blocked state;
- a 60-second successful-lookup requery floor and abandoned-waiter cleanup so
  unavailable or duplicate peer sets do not create a discovery hot loop;
- retained BEP 27 private intent, including post-metadata purge of DHT-only
  peers and preverified-resume gating before any DHT lookup;
- schema version 3 persistence for one node ID and at most 64 validated
  responsive contacts per address family; restored contacts are tried before
  configured routers and must answer before live-table admission; and
- a loopback diagnostic binary plus `--dht-bootstrap` CLI seam used only by
  headless interoperability tests. Product desktop and Android clients obtain
  the same in-process service through `ApplicationService` and default to
  online networking.

The implementation survey changed one parser decision. Mainline DHT traffic
uses the bounded bencode parser in a DHT-only mode that accepts out-of-order
dictionary keys while still rejecting duplicate keys. A live libtorrent router
response exposed this deployed wire behavior. Canonical metainfo and metadata
parsing remain strict.

Traversal candidates, transactions, token rotation, and peer TTLs remain in
the engine actor because their state is inseparable from the actor clock and
in-flight RPC ownership. Codecs, address values, XOR/routing decisions, and
node-state transitions remain runtime independent. This is the concrete
boundary landed by the campaign rather than a generic DHT framework.

## Validation Evidence

Deterministic and controlled evidence completed on 2026-07-31:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace --no-fail-fast`: 65 engine tests passed with three
  opt-in live tests ignored, 58 protocol tests plus the architecture gate, 30
  session tests, and all remaining workspace tests;
- `uv run --project tests/interop --locked python
  tests/interop/dht_magnet.py` against
  libtorrent 2.0.13: an info-hash-only magnet used one `find_node` and one
  `get_peers`, obtained 26,686 bytes of metadata in two blocks, verified and
  published all three payload pieces and 40,000 bytes, independently queried
  RSTorrent with `ping`, `get_peers`, and token-authenticated `announce_peer`,
  and cleaned up in 0.815 seconds;
- the application restart test persisted a responsive node on first shutdown,
  started a second service with no configured router, recontacted that saved
  node, and shut down cleanly;
- the Android Rust library cross-built in release mode for x86_64 and
  arm64-v8a at API 28;
- `npm ci --no-audit --no-fund`, `npm run build`, and
  `cargo build -p rstorrent-desktop`, without launching a product window; and
- pinned BEP and libtorrent reference status at the revisions recorded above.

The opt-in public bootstrap test initially found one malformed packet and no
admitted nodes. Independent packet inspection identified libtorrent's
out-of-order version key; after the DHT-only parser correction, the public
bootstrap test reached a BEP 42-valid node in 0.12 seconds. A bounded 120-second
trackerless Big Buck Bunny metadata attempt then built a 16-node routing table,
received 830 valid responses, and observed 1,563 peer values, but no contacted
peer completed metadata before timeout. This is useful live evidence of the
remaining single-peer lifecycle/reliability gap, not a failed controlled DHT
support claim or a completion claim.

Deliberate remaining limits are IPv4-only socket operation, no self-announce
until an incoming peer port exists, no persisted age/failure metadata beyond a
responsive bounded sample, and no multi-peer content owner. BEP 5, BEP 32,
BEP 42, and BEP 43 therefore remain Partial rather than Supported.
