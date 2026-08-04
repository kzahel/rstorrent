# Tactical 065: DHT Observatory

Status: Planned; direction accepted and visualization revised on 2026-08-03.
Implementation has not begun.

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
visual re-encodes the fixed 160 Kademlia buckets as shared-prefix depth relative
to the local node ID. It shows depths `0` through `31` directly and preserves
depths `32` through `159` as one explicit tail summary. Good and questionable
live occupancy rise above a fixed baseline; replacement candidates mirror
below it on the same `K = 8` scale, and a freshness rail shows the oldest live
response age against the existing 15-minute questionable threshold. Aggregate
facts and bounded active-lookup convergence summaries explain current activity.
No graph layout, globe, or decorative network animation implies relationships
the engine does not know.

## Desired Outcome And Stopping Condition

The session-scoped DHT tab works with no selected torrent. It distinguishes
offline, bootstrap-empty, and participating states; shows exact routing,
freshness, transaction, lookup-convergence, traffic, rejection, bootstrap,
refresh, and discovery facts; renders every routing bucket through the direct
depth range plus truthful collapsed-tail summary; and shows at most 16 active
lookup summaries without node endpoints. The visualization is readable as a
static automatically updating observation. A presentation-only toggle swaps
between the normalized depth encoding and the literal 160-slot engine array so
the transformation is teachable and screenshots remain diagnostically useful;
neither mode requires selection, zoom, or drill-down. Controlled loopback
activity moves the correct depth band, lookup, and counters, and shutdown clears
the view only after the DHT owner and observer are joined.

The tactical stops when the pure routing inspection snapshot, actor observation
path, named session view, generated contracts, visualization, deterministic
scenario, and controlled DHT harness evidence pass. It does not expand BEP
support or change DHT routing, lookup, persistence, or network policy.

## Dependencies And Sequence

- Tactical `016` owns the DHT protocol/runtime foundation and its reference
  dossier. Tacticals `033`, `048`, and `060` own leased view delivery.
- Tactical `064` precedes this slice and centralizes torrent-versus-session tab
  scope. DHT reuses that vocabulary rather than adding another exception.
- Tactical `066` was completed first under direct authorization. Its Speed
  implementation is independent of this still-planned DHT slice.

## Scope

- Add a runtime-independent immutable inspection snapshot for all 160 IPv4
  routing buckets, including oldest live-response age, without exposing mutable
  nodes or socket types.
- Add an immutable actor observation containing lifecycle/policy, current
  bounds, aggregate counters, exact datagram byte counters, bucket occupancy,
  and bounded active lookup summaries.
- Expose the latest observation through a bounded latest-value channel and one
  application-owned, cancellable, joined forwarding task.
- Add capability `session_dht`, `ViewSpec::SessionDht`, singleton replacement
  snapshots/patches, generated TypeScript/schema/UniFFI/Kotlin contracts,
  strict decoding, and reducer coverage.
- Replace the DHT scaffold with aggregate facts, a static shared-prefix-depth
  routing distribution, and an active-lookups table with convergence state.
- Add a permanent deterministic scenario plus accessibility, responsive,
  theme, stale/reset, scale, and controlled-loopback evidence.

## Non-Goals

- A raw routing-node or replacement-node table, node endpoints, individual
  transaction rows, packet capture, or KRPC message log.
- Force-directed graphs, geographic maps, globes, inferred edges, latency
  topology, or continuous decorative animation.
- DHT controls, bootstrap editing, node pinging, routing-table mutation,
  lookup cancellation, or arbitrary target lookup from the UI.
- Required chart inspection, a bucket slider, pan, zoom, or drill-down. The
  normalized/literal presentation toggle and optional pointer detail do not
  change engine state or request different data; no meaning or exact state may
  depend on using them.
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
  +--> compact aggregate facts
  +--> static Canvas shared-prefix-depth distribution
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
  oldest_live_response_age_millis?,
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
  closest_responded_prefix_bits?,
  last_convergence_improvement_age_millis?,
}

DhtInspectionView {
  lifecycle,
  network_policy,
  local_node_id,
  captured_millis,
  routing_nodes_v4,
  occupied_buckets_v4,
  deepest_shared_prefix_bits_v4?,
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

For bucket index `i`, shared-prefix depth is `159 - i`. Depth `0` is the
farthest half of the keyspace, depth `1` is the next quarter, and every
additional shared bit halves the distance band. The bucket age is the maximum
age of any retained live node's last correlated response; it is absent for an
empty bucket. `deepest_shared_prefix_bits_v4` is the maximum depth with live
occupancy and is absent when the table is empty. None of these fields estimates
the total Mainline population or an attainable routing-node denominator.

Lookup convergence is based on the closest **responded** candidate, not simply
the first distance-sorted candidate, which may still be unqueried or may have
failed. The actor records the best responded prefix depth and the monotonic age
since that value last improved. This is observation-only state and cannot
change candidate ordering, lookup deadlines, or protocol behavior. The UI
shows the exact depth and last-improvement age rather than inventing a generic
health score.

The session view is small and bounded, so snapshot and patch both replace one
complete `DhtInspectionView` rather than maintaining 160 independently keyed
rows. Equality/coalescing suppresses unchanged replacements. View reset and
lease recovery return one coherent latest observation.

## Shared-Prefix Routing Distribution Contract

The principal visualization is a high-DPI Canvas ordered by shared-prefix depth
with the local node ID. It is an inspection of XOR-distance occupancy, not a
network map. Re-encoding the engine's bucket index is necessary for legibility:
under an ordinary uniformly distributed network, nearly all realistically
occupied buckets lie in a few dozen far-distance bands, so drawing all 160 as
equal-width columns would compress useful data into one edge.

One compact segmented presentation control switches between **Depth ·
normalized** and **Buckets · literal**. The normalized mode is the default and
is locally persisted with other presentation preferences. The literal mode
draws the exact engine array as 160 equal-width columns in bucket-index order
from `0` closest through `159` farthest. It is literal to storage layout, not
proportional to keyspace area. Both modes consume the same immutable observation
and retain the same `K = 8` scales, colors, freshness semantics, lifecycle, and
aggregate facts; switching performs no application command or new view request.
The active mode and axis labels remain visible in screenshots.

- The primary x-axis is fixed at depths `0` through `31`. Depth `0` represents
  bucket `159` and half of the keyspace; depth `31` represents bucket `128`.
  Moving right means one more shared bit and a keyspace band half as large.
- Each primary column represents exactly one depth/bucket. Good and
  questionable live nodes stack upward from a fixed baseline on a `0..=8`
  scale. Questionable nodes use a warning treatment with pattern or shape as
  well as color.
- Replacement candidates mirror downward from the same baseline on their own
  fixed `0..=8` scale. They never increase live height. Bad nodes are not shown
  as retained occupancy; their removal remains engine truth.
- A narrow freshness rail below each primary column encodes the oldest live
  response age, clamped from zero through
  `GOOD_NODE_AGE_SECONDS = 15 minutes`. It is explicitly labeled as oldest
  response age. A bucket with no questionable live node and an oldest response
  age from 12 through 15 minutes is **aging**; a node older than 15 minutes is
  already represented in the questionable live segment. Empty buckets have no
  freshness mark.
- Depths `32` through `159` are never described as unreachable or permanently
  empty. They occupy one fixed labeled tail region reporting its 128-depth
  span, summed good/questionable/replacement counts, maximum occupancy, and
  deepest occupied depth when any outlier exists. The ordinary zero state reads
  `128 deeper bands · 0 live nodes`; a nonzero tail must become visibly
  occupied without changing the primary axis or requiring expansion.
- The display never shows an estimated `reachable` denominator. Current live
  nodes, occupied depth bands, and deepest occupied depth are exact; any
  scenario population model remains fixture provenance rather than product
  truth.
- The visualization is complete without interaction. Axis ticks, fixed scale,
  legend, freshness label, aggregate facts, and a concise accessible depth
  table expose its meaning and exact values. Pointer detail may be a progressive
  enhancement, but there is no slider, required selection, pan, zoom, or
  drill-down, and mobile cannot lose information that desktop exposes. The
  optional mode toggle teaches the encoding rather than unlocking hidden data.
- Thirty-two primary columns remain legible without the old 160-column mobile
  aggregation scheme. Compact layouts may reduce gaps and labels but preserve
  all 32 columns plus the tail summary.
- Literal mode always draws all 160 slots, including known empty slots, within
  the available width. It may reduce column gaps to zero but does not aggregate,
  omit, reorder, or horizontally scroll them. Its adjacent visible note states
  that equal pixel widths represent engine slots, not equal keyspace volumes.
- Both modes visibly include capture time and lifecycle, axis direction/range,
  the equation `depth = 159 - bucket index`, fixed `K = 8`, legend/freshness
  semantics, and exact total/tail facts. No screenshot interpretation depends
  on hover state.
- Canvas resolution follows measured CSS size and device pixel ratio capped at
  `3`, matching the Pieces precedent. It redraws only on data, theme, or resize;
  there is no continuous RAF loop.

## Aggregate And Lookup Presentation

- Compact status facts show lifecycle/policy, exact IPv4 routing nodes,
  occupied depth bands, deepest occupied prefix depth, aging/questionable
  state, active lookups/transactions, query/response totals, DHT traffic,
  discovered peers, malformed input, rate limiting, bootstrap attempts, and
  refreshes. They do not show an estimated maximum reachable node count.
- Labels distinguish current gauges from cumulative counters. Traffic counts
  all socket datagram bytes accepted by send/receive boundaries, including a
  received malformed datagram before decode; they are not payload throughput.
- Active lookups use a compact bounded table: Target, closest responded prefix
  depth and last improvement age, candidate-state allocation, elapsed/deadline,
  and Peers. Candidate allocation preserves the exact Unqueried, In flight,
  Responded, and Failed counts while making the part-to-whole state legible.
  Convergence uses the same fixed `0..=31` primary depth scale plus an explicit
  deeper marker. There are at most 16 rows and no virtualization requirement.
- The first slice does not label a lookup with a qualitative score such as
  `healthy` or `not converging`. Exact closest-responded depth, time since its
  last improvement, response count, and deadline provide the evidence without
  inventing a policy threshold.
- Offline and bootstrap-empty are useful states, not errors. Unsupported,
  inactive, disconnected, stale, reset, and overflow remain separately
  rendered. Zero nodes is never presented as healthy participation.
- The tab is in the session group and remains usable with no torrent selected.
  It contains no torrent-scoped placeholder.

## Invariants And Resource Bounds

- Exactly 160 IPv4 bucket summaries are emitted in index order. Each live
  bucket satisfies `good + questionable <= 8`; replacements are `<= 8`.
- Presentation depth is exactly `159 - bucket_index`. Depths `0..=31` map to
  buckets `159..=128`; every bucket `127..=0` contributes to the explicit
  `32..=159` tail summary. No nonzero tail occupancy may be rendered as empty.
- The sum of live occupancy equals `routing_nodes_v4` in the same observation.
  `occupied_buckets_v4` equals the number with nonzero live occupancy.
- `deepest_shared_prefix_bits_v4` equals the greatest occupied presentation
  depth and is absent only when live occupancy is zero. Bucket freshness is
  absent only for an empty live bucket and otherwise equals the maximum
  saturating age since a retained live node's last correlated response.
- Failed/bad nodes cannot be counted as good or questionable. Replacement
  candidates remain visually and semantically separate from validated live
  nodes.
- Active transactions are `<= 256`; lookup rows are `<= 16`; each lookup's
  candidate counts sum to `<= 256`; discovered peers are `<= 200`.
- Lookup convergence considers only responded candidates with known node IDs.
  Its best prefix depth never decreases during one lookup, and its improvement
  age resets only when a strictly closer responded candidate is observed.
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
participating, active-lookup, sparse-table, deep-tail-outlier, rate-limited,
malformed, stale, and inactive states with deterministic node IDs and counters.
Its ordinary participating fixture uses 171 live nodes across 25 occupied depth
bands with deepest prefix depth 24. That shape is an illustrative uniformly
distributed network-density fixture, not a claim about current Mainline
population or a reachable-node ceiling. The outlier state places a real node
beyond depth 31 so the collapsed tail cannot be implemented as an assumed zero.

Implementation and tests must cover:

1. bucket `159` maps to depth `0`, bucket `142` to depth `17`, bucket `135` to
   depth `24`, and bucket `0` to depth `159` in the labeled tail;
2. a nonzero depth beyond `31` remains visible in the collapsed-tail aggregate
   and accessible exact table rather than being called unreachable or empty;
3. normalized and literal modes derive from the same 160 buckets, literal mode
   preserves exact index order and empties, mode switching changes no view
   request, and every screenshot labels the active encoding;
4. good-to-aging-to-questionable transition, failure removal, and replacement
   promotion update occupancy, mirrored replacement geometry, and freshness
   without exposing a node row;
5. closest responded lookup depth advances only for a strictly closer response,
   last-improvement age remains monotonic between advances, a 256-candidate
   lookup stays one bounded summary, and terminal completion removes it;
6. private gating cancels and clears an active lookup;
7. malformed/rate-limited traffic increments the right counters and byte totals;
8. observation backpressure drops intermediate states but retains the latest
   exact cumulative totals;
9. app/view lease reset reconstructs all 160 buckets, tail aggregates,
   freshness, and current lookup convergence; and
10. actor error/shutdown publishes terminal state and joins the observer.

These cases define routing membership, privacy, bounds, and task termination;
they must land with the common path.

## Staged Implementation And Gates

1. **Reference and pure state.** Reconfirm the dossier, add the pure bucket
   snapshot, and test ordering, classification, replacements, totals, and
   fixed bounds without Tokio.
2. **Actor observation.** Add exact byte counters, lookup convergence and
   last-improvement state, latest-value publication, coalescing, and terminal
   lifecycle. Prove no packet-rate queue and joined shutdown.
3. **Application contract.** Add the session view, generated artifacts, strict
   validation, singleton replacement reducer, lease/reset behavior, and shared
   tab-scope selection. Rust and contract tests gate UI work.
4. **Presentation.** Implement compact facts, the static Canvas prefix-depth
   distribution and literal engine-array mode with mirrored replacements,
   freshness and tail summary, the lookup table, deterministic scenarios,
   accessible exact alternatives, themes, and responsive layout.
5. **Controlled proof.** Drive the existing loopback DHT harness through
   bootstrap, routing admission, lookup progress/completion, malformed/rate-
   limited input, and shutdown. Compare view values with owned runtime facts.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Bucket-to-depth mapping, classification, freshness, replacement separation, exact tail/total summaries, convergence transitions, and max bounds. |
| Contract | Snapshot/replace serialization, schema/type generation, precision and oversized/malformed rejection, reset/lease reducer tests. |
| Scripted runtime | Latest-value coalescing, byte/counter exactness, lookup lifecycle, private cancellation, actor error, and joined shutdown. |
| Web UI | Component, static comprehension, normalized/literal equivalence and labeling, accessible exact depth data, light/dark, stale/reset, and narrow/standard/wide visual tests. |
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

A raw node inspector, per-transaction trace, IPv6 depth distribution, history,
DHT controls, and public/live operations remain later work requiring their own
tactical. Tactical `066` has already completed the independent Speed tab; this
tactical now ends the accepted missing-detail-tab sequence when implemented.
