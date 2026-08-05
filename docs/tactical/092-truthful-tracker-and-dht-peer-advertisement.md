# Tactical 092: Truthful Tracker And DHT Peer Advertisement

Status: Planned on 2026-08-05. This tactical is the next executable slice in
the incoming-reachability campaign; implementation has not started.

Topics: `incoming-reachability-and-seeding`, `tracker-discovery`,
`dht-discovery`, `application-view-api`, `protocol-support`,
`code-organization-and-refactoring`, `capability-readiness`

Dependencies: completed Tacticals
[`086`](086-long-lived-torrent-peer-runtime.md),
[`088`](088-upnp-mapped-external-tcp-seeding.md), and
[`089`](089-coordinated-session-listen-sockets.md) provide the retained
per-torrent peer lifetime, actual TCP listener, UPnP mapping state, and shared
session UDP/DHT transport required by this slice.

## Decision And Motivation

Replace the provisional tracker port and lookup-only DHT behavior with one
generation-fenced peer-advertisement owner. It derives the port from the
actual accepting TCP listener, prefers the current external TCP port while an
authoritative mapping lease remains active, and supplies that same selected
port to UDP trackers and DHT `announce_peer`. The preferred listen setting and
the session UDP endpoint are never peer-port authorities.

This slice also repairs the lifetime boundary that allowed the provisional
behavior to survive. The current tracker manager and repeated DHT lookup are
nested in an active download driver. They can disappear at download
completion even though `TorrentRuntime` and incoming seeding deliberately
continue. Moving only the port value into that driver would make a completed
seed listen without remaining discoverable. Instead, one session discovery
and advertisement service owns tracker scheduling and DHT traversal for
registered torrent generations; the long-lived torrent runtime owns each
registration and peer-observation destination. There is no timer or detached
task per retained torrent.

The endpoint policy follows pinned libtorrent where its behavior matches
RSTorrent's current product:

- an active external TCP mapping port wins, otherwise the actual bound TCP
  listener port is used;
- public trackers may still receive the actual local listener port when no
  mapping is active, because mapping success and externally observed
  reachability are evidence states rather than prerequisites for useful
  tracker discovery;
- if no TCP listener exists or this torrent is not registered for incoming
  routing, tracker-only outbound discovery uses port `1` as libtorrent's
  explicit non-listening compatibility sentinel rather than retaining the
  plausible but false conventional port `6881`; and
- DHT always carries the selected TCP port explicitly with
  `implied_port = 0`, because RSTorrent has no uTP listener and its TCP and UDP
  numeric ports may legitimately differ.

Port `1` is not an advertised endpoint, readiness claim, persisted setting,
or DHT input. RSTorrent's settings reject ports below `1024`, so the sentinel
cannot be mistaken for one of its listeners. This bounded exception preserves
tracker-assisted downloads while listening is disabled, failed, or
deliberately unavailable to an incomplete torrent. Product state reports that
condition as outbound-only tracker participation.

## Stopping Condition

This tactical is complete only when all of the following hold:

1. one task-free endpoint selector publishes a monotonic generation and
   distinguishes unavailable, local-listener, mapped-external, and stopping
   states from configured preference, UDP transport, and observed incoming
   evidence;
2. UDP tracker announces use the selected TCP port only for a torrent whose
   incoming registration is active, otherwise use the explicit port-`1`
   outbound-only sentinel, and no runtime path retains
   `DEFAULT_ADVERTISED_PEER_PORT`;
3. tracker lifecycle sends bounded `started`, periodic/corrective `none`,
   exactly-once eligible `completed`, and best-effort `stopped` events with
   current transfer counters and bytes left, while retaining the existing
   16-KiB unknown-left convention only before metadata is known;
4. BEP 5 lookup retains tokens from exactly correlated `get_peers` responses
   and announces the selected explicit TCP port to at most the K=8 closest
   token-bearing responders, never before verified public metadata and never
   for a private or ineligible torrent;
5. listener, mapping-port, mapping-validity, and torrent-eligibility changes
   coalesce to the newest generation, cause bounded corrective tracker/DHT
   work, and cannot let stale operation results restore an old port or private
   participation;
6. tracker discovery and DHT lookup/advertisement outlive download completion
   with the ordinary `TorrentRuntime`, feed their peer results into the same
   bounded peer registry, and stop on pause, archive, removal, replacement,
   or session shutdown;
7. tracker `stopped` work finishes or reaches the five-second shutdown bound
   before mapping deletion and listener shutdown; DHT cancellation prevents
   all new traversal and reannouncement before those resources stop;
8. independent controlled tracker-only and DHT-only leechers discover a
   completed RSTorrent seed without an explicit peer hint, connect to the
   selected TCP endpoint, and hash-verify the complete payload; and
9. deterministic state tests, scripted adverse protocol tests, controlled
   mapping-change and shutdown tests, product-contract checks, and the full
   workspace/product baselines pass with recorded owner high-water and
   terminal-zero counts.

BEP 5 has no peer-withdrawal query. Completion therefore does not claim that
remote DHT nodes delete an already stored peer immediately. Required evidence
shows no post-stop `announce_peer`, eventual expiry in a controlled short-TTL
node, and failed connection to the stopped listener even while a stale remote
entry may remain.

## Scope

- Add a portable advertised-peer-endpoint value and pure selector driven by
  current listener state, mapping lease state, network scope, and an explicit
  stopping fence.
- Preserve mapping validity through transient renewal failure only until its
  known finite lease deadline. Successful renewal advances that deadline;
  expiry, confirmed deletion, or a newer mapping generation removes the
  external-port authority.
- Extract tracker scheduling and repeated DHT peer discovery from the active
  download driver into one bounded session service registered by long-lived
  torrent runtimes. Reuse the existing pure tracker schedule, tracker codec,
  DHT actor, peer registry, and typed observation stream.
- Let magnet tracker discovery begin before metadata as it does today, using
  outbound-only port `1` until the torrent becomes incoming-routable. DHT peer
  lookup may also continue before metadata, but DHT self-announcement waits
  for verified public metadata and active incoming registration so neither an
  unknown private flag nor an unroutable info hash can leak participation.
- Carry current per-torrent announce counters into every tracker operation:
  accepted payload bytes downloaded, physical payload bytes uploaded, and
  verified wanted bytes left. Imported or restarted complete seeds report
  `left = 0`; unknown magnet metadata retains `left = 16 KiB`.
- Model tracker lifecycle explicitly enough to know whether `started` was
  successfully sent, whether `completed` remains due, which port generation
  was last announced, and whether `stopped` is required.
- Extend the DHT command/state boundary with one announce traversal that
  reuses normal `get_peers` peer results while retaining bounded tokens for
  the immediate `announce_peer` phase only.
- Publish selected advertisement status and bounded tracker/DHT activity
  through the existing application snapshot/event boundaries and generated
  product contracts. Do not make logs an application-state transport.
- Add structured diagnostics for selection source, generation, event,
  correction reason, token-bearing node count, success/failure counts,
  cancellation, and terminal ownership without logging peer IDs, tokens,
  metainfo, or packet payloads.

## Advertised Endpoint Semantics

### State and authority

The selector is task-free and produces one immutable value:

```text
AdvertisedPeerEndpointState
  Unavailable { generation, reason }
  Local { generation, local_endpoint, scope }
  Mapped {
    generation,
    local_endpoint,
    external_endpoint,
    mapping_generation,
    valid_until,
    renewal_health,
  }
  Stopping { generation, last_endpoint }
```

`generation` advances on every effective state change. Equal repeated input
is a no-op. The selected wire port is the mapped external TCP port in `Mapped`
and the actual listener TCP port in `Local`. The value records why the port
was selected; it does not say that a mapping is externally dialable until an
incoming connection has actually been observed.

The listener must still be accepting and compatible with the selected network
scope. A loopback listener is eligible only for loopback destinations and
controlled loopback evidence. It is never announced to public trackers or
public DHT nodes. A non-loopback listener may publish its actual local port
without active automatic mapping, matching ordinary BitTorrent behavior for
manual forwarding, permissive NAT, and direct public interfaces. Per-torrent
wire selection additionally requires a current incoming registration for the
same info hash and catalog generation. Runtime status keeps this unverified
local-port selection distinct from an active mapping, an incoming-routable
torrent, and an observed incoming connection.

`RenewalFailed` does not prove immediate mapping loss. The last verified
external port remains selected until its finite lease deadline while renewal
retries continue. A successful retry extends the deadline without changing
the advertised generation when the endpoint is unchanged. If the deadline
passes without verification, selection falls back to the live local listener
and schedules correction. The owner never extrapolates validity beyond a
known lease.

### Tracker port policy

For every operation, the tracker owner captures one endpoint generation and
one port:

1. mapped external TCP port while the mapping lease is current;
2. otherwise the actual compatible TCP listener port; or
3. port `1` when the torrent remains eligible for outbound tracker discovery
   but no compatible TCP listener or matching incoming registration exists.

The first two choices apply only when the torrent's incoming registration is
active. Before metadata and during an incomplete download, the current engine
cannot route or upload for that info hash, so the tracker uses port `1` even
if the session listener exists. Verified completion activates readable seed
registration before the `completed` event promotes the real or mapped port.

An endpoint change queues a high-priority event-`none` correction for trackers
that have started successfully. The newest generation supersedes older
pending correction work. A result from an older generation may contribute
bounded diagnostic history but cannot advance the row's current schedule,
last-port state, or peer registry.

Port `1` deliberately matches pinned libtorrent's zero-port conversion. It is
an unconnectable compatibility sentinel, not a claimed listener. DHT
self-announcement is suppressed in this state. A torrent that pauses, is
archived, removed, replaced, or enters session shutdown sends `stopped`
instead of remaining in outbound-only tracker mode.

### DHT port policy

DHT self-announcement requires all of these at the same generation:

- verified metadata whose private flag is false;
- a desired-running torrent lifetime with readable verified content, a peer
  registry, and a matching active incoming registration;
- a compatible actual TCP listener; and
- an endpoint selector state of `Local` or `Mapped`.

The query always encodes the selected TCP port and `implied_port = 0`. The
session UDP source remains the DHT node endpoint only. No UDP mapping, DHT
PORT message, or future uTP assumption changes this rule in this tactical.

One announce attempt performs or joins the torrent's iterative `get_peers`
lookup, keeps a token paired with the exact responding node endpoint, and
sends `announce_peer` to no more than the K=8 closest token-bearing responding
nodes. Tokens are not persisted, shared between nodes, or reused after that
traversal. Missing, malformed, unsolicited, wrong-source, wrong-transaction,
late, or superseded-generation tokens never authorize an announce.

The default periodic DHT announce interval is 15 minutes, copied from pinned
libtorrent. One session scheduler spreads work across eligible torrents so a
large restored catalog does not produce a timer or simultaneous traversal per
torrent. Endpoint changes and first eligibility may request priority, but
they still obey the global traversal/transaction ceilings and coalesce by
torrent generation.

## Tracker Lifecycle And Counters

Each tracker record extends its deterministic schedule with a small protocol
lifecycle:

```text
NeverStarted
  -> Started { last_port, endpoint_generation }
  -> Completed { last_port, endpoint_generation }
  -> Stopping
  -> Stopped
```

Failure and retry history remains orthogonal. `completed` is emitted once
when a successfully started incomplete torrent becomes verified complete; an
imported complete seed starts with `left = 0` and does not fabricate a
download-completion transition. `stopped` is attempted only for tracker rows
that successfully entered a started lifecycle in the current torrent
generation. Its request uses `num_want = 0` and the last coherent counter
snapshot. Completion, correction, and stopped events bypass the ordinary
reannounce deadline but remain bounded by scheduler and operation limits.

After verified metadata exists:

- `downloaded` is accepted non-corrupt payload received in the current
  tracker session;
- `uploaded` is physical piece payload successfully written to peers in the
  current tracker session; and
- `left` is exact wanted content not yet verified, saturating safely at zero.

Restart does not invent historical transfer totals. A new application tracker
session begins from the counters the retained runtime can truthfully support.
Persisting lifetime traffic totals is a separate product/accounting decision.
Before metadata, `left = 16 KiB` follows pinned libtorrent and the existing
RSTorrent convention while downloaded/uploaded remain exact known counters.

The default stopped-event budget is five seconds, copied from pinned
libtorrent. Timeout or tracker failure is recorded and shutdown proceeds; a
remote tracker cannot retain the application indefinitely. Abrupt process
death cannot send `stopped` and remains ordinary tracker lease expiry rather
than a recoverable local lifecycle guarantee.

## Owner, Task, Cancellation, And Dependency Map

```text
ApplicationService generation
  -> AdvertisedPeerEndpointSelector (task-free)
       consumes actual TCP listener + reachability/mapping lease state
       publishes latest generation through a current-value watch
  -> DiscoveryAdvertisementService (one joined session task)
       pure per-torrent registration/schedule table
       bounded command queue and eight-operation tracker JoinSet
       consumes endpoint watch + DhtHandle + session NetworkConfig
       emits peer observations into registered TorrentPeerHandle values
       -> UDP tracker operations (bounded, independently cancellable)
       -> DhtService announce/lookup commands (existing actor bounds)
  -> TorrentRuntime generations (task-free registrations)
       own info hash, tracker catalog, metadata privacy/completion state,
       current counters, desired lifecycle, and peer-observation destination
```

The concrete refactor removes `TrackerManager` and repeated DHT timers from
the active download driver's lifetime. Tracker codecs, URL values, pure
schedule transitions, DHT KRPC values, and token validation remain engine
components independent from application settings and Tokio task handles.
`rstorrent-session` composes actual listener/mapping state with the engine
service and makes each `TorrentRuntime` registration authoritative for one
catalog generation.

The service retains no content bytes, storage handles, peer sockets, or packet
payloads. It obtains small immutable lifecycle/counter snapshots and a bounded
peer-registry handle. A replaced torrent generation cannot receive results
from its predecessor. Closing a result destination cancels only that
registration and does not terminate the session owner.

Normal session shutdown is ordered:

1. reject new torrent registrations and endpoint promotions;
2. fence every registration as stopping and schedule required tracker
   `stopped` operations;
3. wait at most five seconds for stopped operations, cancel remaining tracker
   work, cancel DHT announce traversals, and join the discovery/advertisement
   owner;
4. stop the reachability coordinator and delete the mapping;
5. stop incoming intake and peer/upload owners;
6. stop DHT and persist its routing sample; and
7. stop the shared session UDP owner and remaining application services.

Torrent pause, archive, removal, and replacement apply the equivalent scoped
ordering before incoming registration or readable-storage authority is
removed. `Drop` cancellation remains a fallback and is not successful
shutdown evidence.

## Resource And Failure Bounds

- The service has one task and one finite command queue; registration calls
  backpressure rather than dropping lifecycle changes. Endpoint watch updates
  carry only the latest state.
- Existing catalog and incoming-registration bounds cap retained torrent
  registrations. No per-torrent task, timer, socket, or unbounded token map is
  introduced.
- At most eight UDP tracker operations run session-wide. This replaces the
  current effective per-active-download ceiling rather than multiplying eight
  operations by the number of retained seeds.
- Existing DHT bounds remain authoritative: at most 16 active lookups, 256
  active transactions, 256 lookup candidates, and 200 returned peers.
  `announce_peer` adds at most K=8 sends from one traversal and consumes the
  same transaction ceiling.
- Only one correction generation per torrent is pending. A newer endpoint,
  privacy, completion, pause, or removal transition supersedes older queued
  work without aborting unrelated torrents.
- Tracker DNS, UDP packets, DHT bencoding, compact endpoints, intervals,
  tokens, and error text retain their existing hostile-input and size bounds.
- A tracker interval below current policy is clamped as today. Priority
  lifecycle events do not reset failure history into a tight retry loop.
- Mapping renewal failure is finite. The selector uses a known deadline and
  never treats a retry loop, stale view, or SOAP success from an older
  application generation as an active lease.
- Tracker stopped timeout, DHT cancellation, closed queues, socket error,
  task panic, and partial startup all produce typed terminal observations and
  cannot detach a task.

## Privacy And Security Invariants

- Verified private metadata suppresses DHT lookup and self-announcement and
  purges DHT-only peers under the existing BEP 27 transition. Any in-flight
  public-unknown lookup is cancelled before content scheduling when private
  metadata arrives.
- DHT self-announcement never begins while privacy is unknown. Tracker
  discovery may continue because the metainfo-selected tracker is the allowed
  discovery path for a private torrent.
- A DHT token authorizes only an immediate announce to the exact node that
  returned it in an exactly correlated response. Tokens are neither logged
  nor persisted.
- A tracker or DHT response cannot select the advertised port. Only the local
  endpoint selector owns that authority.
- Port mapping success is not presented as observed external connectivity.
  The existing incoming-connection evidence remains the stronger state.
- Stale DHT peer entries are expected soft state. After local cancellation,
  they cannot reopen a stopped listener or recreate a deleted mapping.
- Network policy is checked for every tracker destination, DHT node, and
  advertised listener scope. Loopback evidence cannot leak into public
  participation.

## Normative And Reference Dossier

### Specifications

Pinned BEPs revision `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`
from [`reference/pins.toml`](../../reference/pins.toml) was inspected:

- `reference/bittorrent.org/beps/bep_0005.rst`: a DHT node UDP endpoint is
  distinct from the TCP peer port; `get_peers` responses carry node-specific
  tokens; `announce_peer` requires a recent token and peer port; and
  `implied_port = 1` replaces the explicit port with the observed UDP source
  port. The BEP defines no peer-withdrawal query.
- `reference/bittorrent.org/beps/bep_0015.rst`: UDP announce requests carry
  downloaded, left, uploaded, event, and peer port fields; events are none,
  completed, started, and stopped; and clients reannounce on the supplied
  interval or a lifecycle event.
- `reference/bittorrent.org/beps/bep_0027.rst`: a private torrent uses only
  its private tracker and must not use DHT, PEX, or local discovery.

### Pinned libtorrent oracle

Revision `7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

- `reference/libtorrent/include/libtorrent/aux_/session_impl.hpp`,
  `listen_socket_t::tcp_external_port`: returns an active TCP NAT mapping port
  and otherwise the actual local TCP listener port;
- `reference/libtorrent/src/session_impl.cpp`, `make_announce_port`,
  `session_impl::queue_tracker_request`, and `session_impl::listen_port`:
  tracker requests use the selected listen socket's external/local TCP port
  and convert unavailable port zero to the compatibility value `1`;
- `reference/libtorrent/src/torrent.cpp`, `torrent::announce_with_tracker`,
  `torrent::stop_announcing`, and `torrent::dht_announce`: tracker events use
  real transfer/left state and stopped lifecycle; private/paused/ineligible
  torrents do not announce to DHT; and implied DHT port is selected only for
  incoming uTP when no explicit override exists;
- `reference/libtorrent/src/kademlia/node.cpp`, `node::announce` and
  `announce_fun`: `get_peers` traversal supplies token-bearing closest nodes,
  then `announce_peer` stores on at most the closest K nodes;
- `reference/libtorrent/src/session_impl.cpp`,
  `session_impl::on_dht_announce`, plus
  `reference/libtorrent/src/settings_pack.cpp`: the session spreads DHT work
  across torrents and defaults `dht_announce_interval` to 15 minutes and
  `stop_tracker_timeout` to five seconds;
- `reference/libtorrent/test/test_dht.cpp`, `test_get_peers` exercised by
  `get_peers_v4` and `get_peers_v6`: tokens returned during iterative lookup
  lead to outbound `announce_peer` while all discovered peers reach the
  callback;
- `reference/libtorrent/test/test_tracker.cpp`, `udp_tracker_v4` and
  `udp_tracker_v6`: a seeded torrent advertises the expected peer endpoint and
  removal produces stopped announces; and
- `reference/libtorrent/test/test_tracker.cpp`, `stop_tracker_timeout` and
  `stop_tracker_timeout_zero_timeout`: shutdown waiting is explicitly bounded
  and may be disabled by policy.

Adopted behavior is mapped-port preference, actual-listener fallback,
port-`1` tracker compatibility without an incoming-routable torrent, explicit
TCP port for DHT without uTP, 15-minute DHT reannouncement, five-second
tracker stopping, real announce counters, and one session scheduler that
spreads work.

Intentional differences:

- RSTorrent remains one IPv4 listen-socket generation rather than
  libtorrent's multi-interface/IPv4/IPv6 collection;
- its endpoint selector, protocol schedules, async runtime, application
  lifetime, and product projection remain separate modules rather than
  copying libtorrent ownership;
- DHT announcement waits for verified public metadata and an active incoming
  registration rather than advertising a premetadata, private-unknown, or
  currently unroutable torrent;
- port `1` is exposed explicitly as outbound-only compatibility, never as an
  endpoint; and
- mapping soft-state validity is fenced by the known finite UPnP lease and
  application generation.

Libtorrent is BSD-3-Clause and is used only as a completeness and executable
interoperability oracle. No source, fixture, or test data is imported.

### JSTorrent product history

The local sibling's current `main` checkout was inspected:

- `packages/engine/src/tracker/udp-tracker.ts` sends
  `engine.listeningPort`, tracker events, and supplied transfer counters;
- `packages/engine/src/tracker/tracker-manager.ts` obtains current counters
  from its torrent owner and can send started, update, completed, and stopped
  events;
- `packages/engine/src/core/torrent.ts`, `getAnnounceStats`,
  `startDHTLookup`, `requestDHTPeers`, and shutdown paths calculate exact
  known left, gate DHT on private state, retain tokens from iterative lookup,
  self-announce an explicit TCP port, and stop periodic work with the torrent;
- `packages/engine/src/dht/dht-node.ts`, `announcePeer` and `announce`, sends
  token-authenticated explicit-port queries; and
- `packages/engine/test/dht/dht-node-queries.test.ts` and
  `iterative-lookup.test.ts` cover explicit/implied encoding, response/error/
  timeout behavior, and token retention; tracker tests cover the configured
  listening port on the wire.

JSTorrent confirms the product value of torrent-owned counters, exact private
gating, token-to-announce continuity, and stopped lifecycle. Its five-minute
per-torrent timer and direct use of one engine port are not adopted: RSTorrent
uses the pinned libtorrent 15-minute session-spread default and its mapped/
local endpoint selector. Unlike libtorrent and JSTorrent, RSTorrent does not
yet accept incomplete-torrent incoming peers, so incomplete downloads remain
tracker-outbound-only and do not self-announce in DHT. No JSTorrent source or
fixture is copied.

## Validation Plan

| Layer | Required evidence |
| --- | --- |
| Pure endpoint state | Disabled/failed/listening/mapped/stopping transitions; mapping renewal success, transient failure before deadline, expiry, changed external port, and stale generation; loopback/public scope; port-`1` fallback without listener or torrent registration; no DHT port without both. |
| Pure tracker state | Started/reannounce/correction/completed/stopped ordering; imported seed; premetadata 16-KiB left; exact postmetadata counters; last-port capture; stale result fencing; stopped only after successful start; five-second bound; no retry spin. |
| Tracker runtime | Scripted UDP tracker decodes every field and returns peers under loss, timeout, malformed, and mapping-change cases; endpoint correction replaces the old peer row; pause/remove/session shutdown observe stopped before listener or mapping teardown. |
| Pure DHT state | Token retained only with exact source/transaction; K=8 closest selection; missing/invalid/stale token; explicit port with implied zero; private/unknown privacy suppression; endpoint-generation correction; scheduler fairness and capacity. |
| DHT runtime | Scripted nodes complete get-peers-to-announce flow, return peers concurrently, reject stale tokens, observe mapped/local port changes, and see no post-cancel announce. A short-TTL fixture proves eventual remote expiry without inventing a withdraw query. |
| Lifetime/refactor | Discovery starts before metadata where allowed, survives download-to-seed transition, feeds one peer registry, restarts from retained tracker catalog/public metadata, and stops on every torrent generation transition. No duplicate tracker or DHT scheduler remains in the active driver. |
| Controlled local vertical | A tracker-only leecher and a DHT-only leecher independently discover the seed without `x.pe`, connect to the reported TCP endpoint, download all bytes, and hash-verify them. An incomplete/unregistered torrent uses port `1`, still obtains tracker peers, rejects incoming routing, and emits no DHT announce; completion registers first and then corrects both mechanisms. |
| Mapped external vertical | A controlled gateway and outside-network leecher observe the actual external port on tracker/DHT wire traffic, connect through the mapping, and complete. Scripted changed-port renewal proves correction; shutdown observes tracker stopped and DHT cancellation before mapping deletion, then failed reconnect. No private machine name enters repository artifacts. |
| Product/observability | Generated Rust/JSON Schema/TypeScript/UniFFI contracts distinguish unavailable, outbound-only, local, mapped, renewal-unhealthy, stopping, and observed-incoming meanings; web and Android fixtures render them without inferring reachability. |
| Resources/baseline | Record registration, task, tracker-operation, DHT traversal/transaction, command-queue, and token high-water/terminal counts. Run Rust fmt/clippy/workspace tests plus established web and Android generation/test/build gates. |

Physical off-LAN evidence is environment-scoped. The controlled protocol and
gateway fixtures remain the deterministic regression authority. Any public
tracker/DHT smoke follows the repository's opt-in live-evidence policy and is
supporting evidence only.

## Implementation Slices

1. **Endpoint selection and contract.** Land the task-free state machine,
   mapping lease-deadline semantics, current-value watch, runtime/product
   projection, and pure transition tests. Commit after focused Rust and
   generated-contract gates pass.
2. **Long-lived tracker ownership.** Extract tracker discovery from the active
   driver into the session service, register it from `TorrentRuntime`, add
   endpoint generations, counters and completed/stopped lifecycle, and prove
   download-to-seed continuity under the session-wide eight-operation bound.
3. **DHT self-announcement.** Retain exactly correlated lookup tokens, add the
   bounded explicit-port announce phase and 15-minute spread scheduler, retain
   private/unknown gating, and prove local/mapped correction and cancellation.
4. **Vertical evidence and closure.** Prove independent tracker-only,
   DHT-only, and mapped external discovery-to-download paths; prove shutdown
   order and DHT expiry semantics; record high-water marks and exact commands;
   update all owning topics and protocol/readiness claims.

Implementation may proceed through these logical commits without additional
architecture decisions. Stop for direction if it requires a new tracker
transport, a different mapping mechanism, settings-policy changes beyond the
status contract, a dependency with meaningful tradeoffs, or external action
beyond the already authorized controlled evidence pattern.

## Deliberate Deferrals

- HTTP/HTTPS/WebSocket trackers, scrape, authentication, proxies, BEP 41 URL
  data, and a general tracker transport framework.
- DHT IPv6 runtime, BEP 5 PORT messages, BEP 10 listen-port messages, PEX,
  local service discovery, uTP, hole punching, and UDP gateway mapping.
- PCP, NAT-PMP, IGD v1, WANPPP, IPv6 pinholes, multiple interfaces, dynamic
  interface rebinding, VPN/metered policy, firewall automation, and Android
  local-network permission work.
- Upload from incomplete torrents, finite bandwidth, seeding ratios/time
  goals, tit-for-tat policy, and persisted lifetime traffic accounting.
- Immediate deletion of remote DHT peer state, which BEP 5 does not define.
- A full BEP 27 tracker-tier compliance claim; this tactical preserves exact
  DHT privacy gating and the existing retained tracker behavior without
  broadening private-tracker scheduling policy.
- Public-swarm reliability or general Internet reachability claims from the
  existence of announce code or one environment's successful mapping.

## Completion Evidence

Not yet recorded.
