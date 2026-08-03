# Tactical 065: DHT Observatory

Status: Planned; direction accepted on 2026-08-03. Implementation has not
begun.

Topics: `dht-discovery`, `application-view-api`, `web-ui-design`,
`desktop-inspection-surface`, `capability-readiness`

## Motivation

RSTorrent owns a real bounded Mainline DHT participant, but its tab is an empty
scaffold and its useful facts are limited to internal statistics and logs. A
raw node table would expose many endpoints while doing a poor job of answering
the important questions: whether the participant is healthy, how its XOR
routing space is populated, whether lookups are progressing, and whether
traffic is being rejected or rate-limited.

This slice makes DHT an observatory rather than a node browser. Its primary
visual is a truthful 160-position Kademlia routing-space ribbon. Each position
is one IPv4 XOR-distance bucket and displays good, questionable, and replacement
occupancy against the existing `K = 8` bound. Aggregate cards and bounded active
lookup summaries explain current activity. No graph layout, globe, or decorative
network animation implies relationships the engine does not know.

## Desired Outcome And Stopping Condition

The session-scoped DHT tab works with no selected torrent. It distinguishes
offline, bootstrap-empty, and participating states; shows exact routing,
transaction, lookup, traffic, rejection, bootstrap, refresh, and discovery
facts; renders the full routing-space ribbon; and shows at most 16 active
lookup summaries without node endpoints. Controlled loopback activity moves
the correct bucket, lookup, and counters, and shutdown clears the view only
after the DHT owner and observer are joined.

The tactical stops when the pure routing inspection snapshot, actor observation
path, named session view, generated contracts, visualization, deterministic
scenario, and controlled DHT harness evidence pass. It does not expand BEP
support or change DHT routing, lookup, persistence, or network policy.

## Dependencies And Sequence

- Tactical `016` owns the DHT protocol/runtime foundation and its reference
  dossier. Tacticals `033`, `048`, and `060` own leased view delivery.
- Tactical `064` precedes this slice and centralizes torrent-versus-session tab
  scope. DHT reuses that vocabulary rather than adding another exception.
- Tactical `066` follows this slice in the accepted detail-tab sequence.

## Scope

- Add a runtime-independent immutable inspection snapshot for all 160 IPv4
  routing buckets without exposing mutable nodes or socket types.
- Add an immutable actor observation containing lifecycle/policy, current
  bounds, aggregate counters, exact datagram byte counters, bucket occupancy,
  and bounded active lookup summaries.
- Expose the latest observation through a bounded latest-value channel and one
  application-owned, cancellable, joined forwarding task.
- Add capability `session_dht`, `ViewSpec::SessionDht`, singleton replacement
  snapshots/patches, generated TypeScript/schema/UniFFI/Kotlin contracts,
  strict decoding, and reducer coverage.
- Replace the DHT scaffold with aggregate cards, the routing-space ribbon, and
  an active-lookups table.
- Add a permanent deterministic scenario plus accessibility, responsive,
  theme, stale/reset, scale, and controlled-loopback evidence.

## Non-Goals

- A raw routing-node or replacement-node table, node endpoints, individual
  transaction rows, packet capture, or KRPC message log.
- Force-directed graphs, geographic maps, globes, inferred edges, latency
  topology, or continuous decorative animation.
- DHT controls, bootstrap editing, node pinging, routing-table mutation,
  lookup cancellation, or arbitrary target lookup from the UI.
- IPv6 DHT enablement, `announce_peer`, PEX, LSD, uTP, NAT traversal, DHT
  scrape/items, or any new protocol-support claim.
- Replacing structured Logs or making diagnostics the source of view truth.
- Persisting UI history, exposing DHT state to an unauthenticated or new remote
  product, public-swarm traffic, or a visible desktop launch.

## Reference Dossier

### Normative specifications

- BEP 5 defines 160-bit node IDs, XOR distance, K-buckets, good/questionable
  node behavior, iterative lookup, and KRPC messages.
- BEP 27 controls private-torrent participation.
- BEP 32 keeps IPv4 and IPv6 DHT address families distinct.
- BEP 42 and BEP 43 define node-ID security and read-only nodes.

Use the pinned BEP checkout at
`reference/bittorrent.org@7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`.
The visual labels must describe exact RSTorrent observations and must not turn
implementation policy into normative BEP terminology.

### Pinned libtorrent oracle

Re-inspect libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`, particularly:

- `src/kademlia/routing_table.cpp` and `node_entry.cpp` for bucket ordering,
  live/replacement membership, and good/questionable/failed classification;
- `src/kademlia/rpc_manager.cpp` for bounded transactions and counters;
- `src/kademlia/traversal_algorithm.cpp`, `get_peers.cpp`, and `refresh.cpp` for
  useful active-lookup observations and termination;
- `src/kademlia/dht_tracker.cpp` and `node.cpp` for session-level traffic and
  lifecycle statistics; and
- `test/test_dht.cpp`, `test/test_direct_dht.cpp`, and relevant independent
  cases for bucket, timeout, malformed-input, and shutdown behavior.

RSTorrent adopts useful inspection distinctions, not libtorrent's alert API,
status structs, C++ ownership graph, or routing policy.

### JSTorrent product history

Inspect local JSTorrent revision
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/ui/src/components/DhtTab.tsx` for the existing product's statistics
  and node-table affordances; and
- `packages/engine/src/dht/` plus focused tests for routing, lookup, bootstrap,
  persistence, rate-limit, and lifecycle observations.

RSTorrent keeps the useful operational questions but intentionally replaces
the endpoint-heavy table with routing-space and lookup summaries. No source or
fixture is copied.

## Existing Boundary And Concrete Improvement

`rstorrent-protocol::dht::RoutingTable` owns 160 fixed XOR-distance buckets,
each with at most eight live nodes and eight replacement candidates. The pure
module already classifies a live node as good, questionable, or bad. The
session-owned DHT actor caps active transactions at 256, active lookups at 16,
lookup candidates at 256, returned peers at 200, and datagrams at 1,024 bytes.
`DhtStats` already carries aggregate query, response, malformed, rate-limit,
discovery, bootstrap, and refresh counters, but no complete inspection view
exists.

The concrete boundary improvement is a pure read-only routing snapshot and a
latest-value actor observation. Protocol state stays independent of Tokio,
sockets, tasks, transports, JSON, and UI. The view consumes facts but cannot
query or mutate routing nodes.

## Owner, Task, Cancellation, And Data Flow

```text
rstorrent-protocol::dht::RoutingTable
  | pure 160-bucket occupancy snapshot
  v
session-owned DHT actor (socket, transactions, lookups, counters)
  | immutable latest observation; bounded watch channel
  v
ApplicationService-owned DHT observation task
  | semantic coalescing
  v
ViewHub session_dht singleton
  |
  v
leased view set -> strict browser adapter -> Zustand
  |
  +--> metric cards
  +--> Canvas routing-space ribbon
  +--> active-lookups table
```

The DHT actor remains the sole mutable runtime owner. It publishes immutable
observations no faster than every 500 ms during ordinary activity and forces
lifecycle/error transitions. The channel retains only the latest value; a slow
view consumer cannot queue packet-rate history.

`ApplicationService` owns exactly one forwarding task, its cancellation token,
and its join handle. On shutdown, the DHT actor publishes terminal inactive
state and closes the observation channel; the forwarder publishes that state,
exits, and is joined along with the DHT service. A startup failure reports the
existing application failure rather than detaching a dead observer. Opening or
closing the tab changes view interest only; it never starts or stops DHT.

## Observation And View Contract

Add capability `session_dht` and `ViewSpec::SessionDht { view_id, delivery }`.
The conceptual contract is:

```text
DhtLifecycle = offline | bootstrap_empty | participating | inactive
DhtNetworkPolicy = offline | loopback_only | online

DhtBucketView {
  bucket_index,              // 0 closest, 159 farthest
  good_nodes,                // 0..8
  questionable_nodes,        // 0..8
  replacement_candidates,   // 0..8
}

DhtLookupView {
  lookup_id,
  target_id,
  age_millis,
  deadline_in_millis,
  unqueried_candidates,
  in_flight_candidates,
  responded_candidates,
  failed_candidates,
  discovered_peers,
}

DhtInspectionView {
  lifecycle,
  network_policy,
  local_node_id,
  captured_millis,
  routing_nodes_v4,
  occupied_buckets_v4,
  active_transactions,
  active_lookups,
  queries_sent,
  responses_received,
  queries_received,
  malformed_received,
  rate_limited,
  discovered_peers,
  bootstrap_attempts,
  routing_refreshes,
  datagram_bytes_sent,
  datagram_bytes_received,
  buckets_v4[160],
  lookups[],
}
```

Exact generated names may follow repository conventions. State and policy are
closed enums. `local_node_id` and `target_id` retain exact 20-byte hexadecimal
identity in the authenticated contract; the presentation abbreviates them and
provides the full value accessibly. No endpoint or peer result appears in this
view. A lookup ID is stable for its actor-owned lifetime and is not a command
handle.

The session view is small and bounded, so snapshot and patch both replace one
complete `DhtInspectionView` rather than maintaining 160 independently keyed
rows. Equality/coalescing suppresses unchanged replacements. View reset and
lease recovery return one coherent latest observation.

## Routing-Space Ribbon Contract

The principal visualization is a high-DPI Canvas with logical bucket order
from **closer to this node** on the left (`0`) to **farther** on the right
(`159`). It is an inspection of XOR-distance occupancy, not a network map.

- Each unaggregated column represents exactly one bucket. Its filled height is
  live occupancy from `0` through `8`.
- Good nodes use the normal accent. Questionable nodes form a visibly distinct
  warning segment with pattern or border as well as color.
- Replacement candidates use an outline/underlay and never add to live
  occupancy. Bad nodes are not shown as retained live occupancy; their removal
  behavior remains engine truth.
- A zero bucket remains visibly empty. The scale stays fixed at `K = 8`; it
  does not autoscale away sparse routing tables.
- At widths too narrow to keep a cell at least three CSS pixels, consecutive
  buckets aggregate by the smallest group size in `{2, 4, 8}` that meets that
  minimum. Aggregated height uses the maximum live occupancy and its tint mix
  represents summed good/questionable proportions; the outline represents the
  maximum replacement occupancy. Hover/focus reports the exact bucket range
  and summed counts, so aggregation remains explicit.
- Pointer hover and keyboard focus use one crosshair/selection model. An
  adjacent text summary announces the selected bucket or range without making
  160 Canvas cells focusable.
- Canvas resolution follows measured CSS size and device pixel ratio capped at
  `3`, matching the Pieces precedent. It redraws only on data, theme, selection,
  or resize. There is no continuous RAF loop.

## Aggregate And Lookup Presentation

- Status cards show lifecycle/policy, IPv4 routing nodes and occupied buckets,
  active lookups/transactions, query/response totals, DHT traffic, discovered
  peers, malformed input, rate limiting, bootstrap attempts, and refreshes.
- Labels distinguish current gauges from cumulative counters. Traffic counts
  all socket datagram bytes accepted by send/receive boundaries, including a
  received malformed datagram before decode; they are not payload throughput.
- Active lookups use a compact bounded table: Target, Age, Deadline, Unqueried,
  In flight, Responded, Failed, and Peers. There are at most 16 rows and no
  virtualization requirement.
- Offline and bootstrap-empty are useful states, not errors. Unsupported,
  inactive, disconnected, stale, reset, and overflow remain separately
  rendered. Zero nodes is never presented as healthy participation.
- The tab is in the session group and remains usable with no torrent selected.
  It contains no torrent-scoped placeholder.

## Invariants And Resource Bounds

- Exactly 160 IPv4 bucket summaries are emitted in index order. Each live
  bucket satisfies `good + questionable <= 8`; replacements are `<= 8`.
- The sum of live occupancy equals `routing_nodes_v4` in the same observation.
  `occupied_buckets_v4` equals the number with nonzero live occupancy.
- Failed/bad nodes cannot be counted as good or questionable. Replacement
  candidates remain visually and semantically separate from validated live
  nodes.
- Active transactions are `<= 256`; lookup rows are `<= 16`; each lookup's
  candidate counts sum to `<= 256`; discovered peers are `<= 200`.
- Incoming and outgoing datagrams remain capped at the protocol's 1,024-byte
  bound. Byte counters use saturating monotonic arithmetic and the existing
  decimal-string JSON convention where JavaScript precision requires it.
- A private torrent has no retained active DHT lookup. Cancellation/removal
  forces an observation so a private target cannot linger in the view.
- No node endpoint, token, transaction ID, peer endpoint, or packet payload
  crosses the view boundary.
- Ordinary observation publication is at most 2 Hz and latest-value only.
  Counters remain exact even when intermediate visual states coalesce.
- View interest never changes bootstrap, refresh, lookup, routing, persistence,
  or socket lifecycle.

## Stable Scenarios And Shape-Changing Cases

The permanent `dht-observatory` scenario includes offline, bootstrap-empty,
participating, active-lookup, sparse-table, densely grouped narrow-width,
rate-limited, malformed, stale, and inactive states with deterministic node IDs
and counters.

Implementation and tests must cover:

1. bucket `0`, a middle bucket, and bucket `159` map to the correct visual end;
2. good-to-questionable transition, failure removal, and replacement promotion
   update exact occupancy without exposing a node row;
3. a 256-candidate lookup stays one bounded summary and terminal completion
   removes it;
4. private gating cancels and clears an active lookup;
5. malformed/rate-limited traffic increments the right counters and byte totals;
6. observation backpressure drops intermediate states but retains the latest
   exact cumulative totals;
7. app/view lease reset reconstructs all 160 buckets and current lookups; and
8. actor error/shutdown publishes terminal state and joins the observer.

These cases define routing membership, privacy, bounds, and task termination;
they must land with the common path.

## Staged Implementation And Gates

1. **Reference and pure state.** Reconfirm the dossier, add the pure bucket
   snapshot, and test ordering, classification, replacements, totals, and
   fixed bounds without Tokio.
2. **Actor observation.** Add exact byte counters, lookup summaries, latest-
   value publication, coalescing, and terminal lifecycle. Prove no packet-rate
   queue and joined shutdown.
3. **Application contract.** Add the session view, generated artifacts, strict
   validation, singleton replacement reducer, lease/reset behavior, and shared
   tab-scope selection. Rust and contract tests gate UI work.
4. **Presentation.** Implement cards, Canvas ribbon, lookup table, deterministic
   scenario, accessible selection, themes, and responsive aggregation.
5. **Controlled proof.** Drive the existing loopback DHT harness through
   bootstrap, routing admission, lookup progress/completion, malformed/rate-
   limited input, and shutdown. Compare view values with owned runtime facts.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Bucket index/order, classification, replacement separation, exact totals, max bounds, and deterministic aggregation tests. |
| Contract | Snapshot/replace serialization, schema/type generation, precision and oversized/malformed rejection, reset/lease reducer tests. |
| Scripted runtime | Latest-value coalescing, byte/counter exactness, lookup lifecycle, private cancellation, actor error, and joined shutdown. |
| Web UI | Component, accessibility, keyboard selection, light/dark, reduced-motion, stale/reset, and narrow/standard/wide visual tests. |
| Controlled interoperability | Existing headless loopback/libtorrent DHT harness with exact routing/lookup observations; no public traffic. |
| Platform | Rust workspace baseline, production web build, and proportional Tauri/Android generated-contract compilation. |
| Public live evidence | Not authorized or required; existing Tactical `016` evidence remains the protocol claim. |

Record exact commands and outcomes in this tactical on completion. Updating the
view does not promote the DHT row in `protocol-support.md`.

## Escalation Contract

When activated, ordinary internal refactoring, exact observation types,
generated-contract changes, Canvas implementation, deterministic fixtures,
and conservative limit tightening are authorized. Stop for direction before
changing DHT routing/lookup behavior, persistence or network policy, exposing
node/peer endpoints, adding controls or a dependency, enabling public traffic,
or changing a protocol-support claim.

## Next Boundary

A raw node inspector, per-transaction trace, IPv6 ribbon, history, DHT controls,
and public/live operations remain later work requiring their own tactical.
Tactical `066` next owns smooth bounded session speed history and its rendering
loop.
