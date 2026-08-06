# Tactical 097: Live Client Settings And Replaceable Session Generations

Status: In progress on 2026-08-06. Gate 1 is complete: the public runtime
contract now exposes configured intent, effective values for four independently
converging domains, and bounded applying/applied/degraded state; the task-free
attempt/domain generation model rejects stale results and nonzero-generation
overflow. Transport ownership and live reconciliation begin at Gate 2. This document does not
reorder the existing tactical queue; completed Tactical
[`096`](096-metadata-tracker-activation-and-family-observability.md) retained
priority and closed independently later that day.

Topics: `incoming-reachability-and-seeding`, `client-persistence`,
`application-control`, `application-view-api`, `peer-lifecycle`,
`dht-discovery`, `tracker-discovery`, `code-organization-and-refactoring`,
`capability-readiness`

Dependencies: completed Tacticals
[`084`](084-persisted-client-connection-and-seeding-settings.md),
[`086`](086-long-lived-torrent-peer-runtime.md),
[`088`](088-upnp-mapped-external-tcp-seeding.md),
[`089`](089-coordinated-session-listen-sockets.md), and
[`092`](092-truthful-tracker-and-dht-peer-advertisement.md) establish the
persisted settings waist, stable per-torrent peer owner, reachability owner,
coordinated TCP/UDP sockets, and long-lived discovery/advertisement lifetime
that this slice must preserve.

## Decision And Motivation

Every field in the current `ClientSettings` group must apply to the running
application without closing and reopening the profile:

- listener policy;
- preferred listen port;
- port-mapping policy;
- ordinary session peer-connection limit; and
- payload upload slots.

The current implementation cannot meet that contract by adding socket calls
to `SetClientSettings`. `ApplicationService::open` constructs a coupled set of
TCP and UDP listeners, incoming-peer state, DHT, discovery advertisement,
reachability, peer admission, and upload scheduling for the whole application
generation. `ApplicationService::handle` persists client settings but performs
no runtime application, and shutdown manually unwinds those owners. Replacing
the current `IncomingPeerService` would also discard torrent registrations,
connected incoming peers, upload grants, and counters; replacing DHT to move
its UDP socket would discard its node identity, routing state, and in-flight
session ownership.

Introduce one private, concrete `SessionNetworkRuntime` in
`rstorrent-session`. It owns stable session networking and reconciles persisted
desired settings onto replaceable transport and reachability generations.
Established peer tasks, per-torrent registrations, upload accounting, DHT
identity/state, and discovery registrations remain stable across a listener or
UDP handover. This is a feature-driven lifetime extraction from
`ApplicationService`, not a generic service framework, settings framework, new
crate, daemon, or IPC boundary.

The same stable owner is the intended home for later session bandwidth policy,
but this tactical must not add speculative limiter traits, buckets, rate-limit
fields, durable totals, or per-torrent policy. The lifetime seam is justified
now by the five fields that already exist.

## Stopping Condition

This tactical is complete when all of the following hold:

1. every valid current client setting is accepted and reconciled in both
   durable and ephemeral profiles without an application restart;
2. the public runtime contract replaces monolithic
   `configured/active/restart_required` state with configured intent, effective
   per-domain state, and `applying`, `applied`, or bounded `degraded` status;
3. changing listener policy or an applicable preferred port replaces the
   coordinated TCP/UDP generation while established incoming and outgoing
   payload transfers, seed registrations, peer observations, upload totals,
   and the upload scheduler remain alive;
4. the new TCP endpoint accepts after a successful handover, the retired
   endpoint stops accepting, the UDP transport reports the new actual
   endpoint, and the DHT retains the same node ID and usable routing/session
   state;
5. a candidate TCP or UDP bind failure retains the prior effective transport
   generation, publishes the attempted failure without falsely reporting a
   restart requirement, and an unchanged settings save can retry convergence;
6. enabling, disabling, or replacing UPnP mapping reconciles against the
   currently effective listener, fences stale generations from advertisement,
   and joins or truthfully reports old finite-lease cleanup;
7. increasing the peer limit admits against the new descriptor-clamped value
   immediately, while decreasing it blocks excess admission first and
   deterministically cancels connections until the existing
   effective-plus-ten-slack absolute bound is restored;
8. changing upload slots immediately recomputes all grants, including exact
   zero-slot choking and the existing optimistic-slot rule, without replacing
   peer tasks or losing counters;
9. rapid desired changes, a no-op command, exact request replay, shutdown
   during reconciliation, and a late mapping or socket event cannot restore an
   older effective generation;
10. settings presentation reports accepted, applying, applied, and degraded
    outcomes without restart instructions and preserves the user's draft on a
    validation or persistence failure; and
11. pure transition, scripted runtime, controlled interoperability, generated
    contract, web, desktop-build, Android cross-build, resource high-water,
    and terminal-zero gates pass with the exact evidence recorded here and in
    the owning topics.

This stopping condition proves live application of the five existing session
settings. It does not implement upload/download bandwidth caps, per-torrent
limits, durable transfer accounting, ratio/time goals, queue auto-management,
or a general settings subsystem.

## Stable Product And Runtime Contract

### Persisted desired state

`SessionStore` remains the sole authority for the atomic `ClientSettings`
group. A successful `SetClientSettings` means that validated desired state was
accepted by the selected profile. For a durable profile, the SQLite
transaction commits before runtime reconciliation is requested. A crash after
commit but before convergence therefore replays the desired settings during
the next open. A persistence or validation failure changes neither desired nor
effective state and starts no runtime work.

An ephemeral profile accepts the same mutation, stores it for that in-memory
profile lifetime, increments revision under the existing rules, and applies it
live. It makes no cross-process durability promise. Remove the current
ephemeral rejection whose only justification is restart application.

Application request serialization remains the command-order authority. After
any successful settings response, including an unchanged mutation or an exact
request replay, `ApplicationService` submits the authoritative current group
to the network reconciler with a fresh runtime attempt generation. A no-op save
is therefore a supported retry after an environmental failure even though it
does not create a durable revision.

The command response need not wait for socket handover, peer eviction, router
I/O, or every resulting view patch. It must return configured intent with at
least `applying` status before background convergence can publish later
`applied` or `degraded` replacement state.

### Runtime view

Replace `ClientSettingsRuntimeView.active` and `restart_required`; one active
copy cannot describe independently converging transport, mapping, admission,
and scheduler domains. The replacement contract must carry:

- the complete configured `ClientSettings` group;
- an optional effective listener/preferred-port pair, absent only when an
  enabled listener has never committed a usable generation;
- the effective port-mapping policy;
- the effective descriptor-clamped peer limit;
- the effective upload-slot count;
- one application state for each of `transport`, `port_mapping`,
  `peer_connections`, and `upload_slots`; and
- the existing concrete listener, UDP, mapping, and advertised-endpoint facts.

Each domain state is one of `applying`, `applied`, or `degraded`. Degraded
state carries a stable coarse reason and UTF-8-safe detail bounded to the
existing 512-byte diagnostic ceiling. Internal attempt and child generations
must be monotonic and structured diagnostics must record them, but the product
contract does not need to expose an unbounded history or a second durable
revision.

`effective` means the policy currently owned by the runtime, not merely a copy
of persisted intent and not a claim that the environment produced the desired
external outcome. For example, UPnP can be the effective policy while its
concrete status is `failed`, and an old effective listener remains visible
when a replacement bind fails. On the first enabled-listener failure there is
no effective peer-listener generation; the existing typed bind failure
remains the concrete fact. Disabled listening is a successfully applied
effective transport policy rather than missing state.

When a previously confirmed mapping cannot be deleted, its prior policy
remains effective and degraded until deletion succeeds or the finite lease is
known to have expired. The concrete mapping status must retain the last known
external endpoint and bounded remaining-lease/cleanup truth; configured
`disabled` must not masquerade as effective while the gateway may still
forward it.

Product presentation must make configured versus effective differences
readable, show live progress/failure, and remove every instruction that says a
restart is required. A fixed-port bind failure is recoverable by changing or
resaving settings. There is no restart button and no UI-owned retry loop.
After this tactical, a newly introduced runtime setting must define live
application and failure semantics; `restart_required` must not be reintroduced
without a separate explicit product decision.

### Per-setting semantics

| Desired change | Live behavior | Failure/effective behavior |
| --- | --- | --- |
| `listener` | Prepare and commit a coordinated peer TCP/session UDP generation. Disabling removes only incoming acceptance and moves DHT to an independent eligible UDP socket; established peers survive. | Candidate failure drops the candidate and retains the prior transport. With no prior transport, retain DHT on its current or startup fallback UDP socket and report no effective peer listener. |
| `preferred_listen_port` | Rebind only when it can change automatic listener selection. Under fixed or disabled policy, update the effective preference without disturbing sockets. Automatic retry and system fallback remain Tactical `089` policy. | Same atomic candidate rule as listener changes. No wrap at `65535` and no silent substitution for fixed ports. |
| `port_mapping` | Start, stop, or replace the reachability generation against the currently effective eligible listener. It never forces a socket rebind. | Local listening remains effective after discovery/add/verify/renew/delete failure. Stale or uncertain mappings are never advertised; an unconfirmed delete retains the prior effective mapping policy as degraded until deletion or expiry. |
| `peer_connection_limit` | Recompute the existing file-descriptor clamp and update admission atomically. Increases take effect before the domain becomes applied; decreases cancel excess connections through their owners. | Configured and effective values remain distinct. Domain stays applying until permits fall within the effective-plus-ten-slack absolute bound; cancellation/join failure is degraded, not restart-required. |
| `upload_slots` | Reconfigure the one stable scheduler, preserve memberships/counters, recompute optimistic capacity, and publish new grants immediately. | Zero chokes all payload upload. A frame already admitted to the bounded writer may complete and is accounted exactly; no new request begins after its grant is revoked. |

Changing preferred port under fixed or disabled listening is still applied
live: the runtime records it as the effective future automatic preference, but
does not churn an unaffected socket. A combined atomic save may converge in
more than one domain outcome. For example, a failed listener replacement does
not prevent upload slots from applying or port mapping from being disabled.
If a listener-policy transition resolves to the already bound address and
port, it may adopt the new effective policy without rebinding; candidate-first
handover applies when the concrete transport must change.

## Convergence And Failure Semantics

One latest-value settings channel carries `(attempt_generation,
ClientSettings)` from `ApplicationService` to the session-network owner. It is
a coalescing watch-style cell, not an unbounded command queue. Every accepted
save uses a new nonzero runtime generation even when the values and durable
revision are unchanged. The reconciler owns one joined task and serializes
state-changing handovers; rapid A-to-B-to-C changes may skip obsolete pending
work, but after bounded cleanup C must be the only desired generation allowed
to publish as current.

Every asynchronous socket, mapping, eviction, and status event carries the
runtime attempt plus its domain generation. An event may update current state
only if both still match. A stale operation still owns cleanup for any socket,
task, permit, or mapping it created; generation rejection is not permission to
detach work.

Convergence follows these rules:

1. publish configured intent and affected domains as applying;
2. apply independent in-memory resource policy without waiting for network
   I/O;
3. prepare any required transport candidate while the current generation
   remains usable;
4. on candidate failure, drop it completely and publish degraded transport
   while retaining current effective endpoints and peer/discovery lifetime;
5. on candidate success, withdraw the old advertisable endpoint, fence and
   stop its reachability generation, install the candidate's TCP accept and
   UDP receive/send generation, publish the new effective endpoints, retire
   the old accept/UDP generation, and start reachability for the new endpoint;
6. publish each domain applied only after its effective state and observable
   runtime facts agree; and
7. on shutdown, close the desired-settings sender, cancel/join reconciliation,
   then shut down the stable network children in their dependency order.

Preparation must make post-commit installation infallible or retain enough
candidate ownership to roll back before endpoint publication. Mapping failure
after a transport commit never rolls back a working listener. Transport
replacement may briefly publish an outbound-only advertisement while the old
endpoint is withdrawn and the new endpoint commits. No newly constructed
tracker or DHT operation may select an endpoint that no active listener
generation owns. An operation already sent to a remote tracker or DHT node
cannot be synchronously revoked; the endpoint change must trigger Tactical
`092`'s bounded correction work, and remote stale state remains subject to
that correction or ordinary expiry rather than a false local guarantee.

If deletion of an old finite UPnP lease cannot be confirmed, fence it from
advertisement, retain bounded cleanup state, and do not request another mapping
that could create an unbounded series of uncertain leases. Retry deletion
through the existing bounded mapping timeouts and allow the recorded finite
lease to expire before a replacement mapping is attempted. The new local
listener may still commit and operate while mapping remains degraded. Only
confirmed deletion or expiry lets the mapping domain adopt configured
`disabled` or begin a new UPnP generation.

No automatic infinite retry loop is added for an exact listener bind failure.
A later changed or unchanged accepted save is the explicit retry trigger.
Existing UPnP renewal/retry policy remains owned by reachability.

## Concrete Ownership Refactor

### Session owner

Add a private `session_network` subsystem in `rstorrent-session` with one
`SessionNetworkRuntime` facade. `ApplicationService` continues to own the
store, storage roots, torrent catalog and `TorrentRuntime` map, application
commands, and view hub. It passes validated startup config and persisted DHT
state inward, receives stable incoming/discovery/endpoint handles for torrent
composition, and receives a terminal report including DHT state during joined
shutdown.

`SessionNetworkRuntime` owns:

- the desired-settings cell and one reconciliation task;
- one stable incoming-peer runtime and upload scheduler;
- the shared `PeerBudget` and connection-eviction registrations;
- an optional replaceable TCP acceptor generation;
- one stable session UDP router with a replaceable socket generation;
- one stable DHT service and node identity;
- one stable discovery-advertisement service and its registrations;
- one stable advertised-endpoint selector;
- an optional replaceable reachability generation; and
- effective settings, owner counters, and structured lifecycle diagnostics.

This owner is private and concrete. Do not add a `Service` trait, dependency
container, repository abstraction, settings callback registry, global actor
framework, new workspace crate, native host, or out-of-process coordinator.

### Stable incoming runtime and replaceable acceptor

Split the engine incoming owner along its existing lifetime fault line:

- stable runtime state owns registration generations, handshake routing,
  peer tasks, upload scheduling/memberships, upload reads, counters,
  observations, and final cancellation; and
- an acceptor generation owns only one supplied `TcpListener`, its accept
  task, pre-handshake admission, and generation cancellation/join.

Stopping an acceptor must prevent new accepts and join pending handshake work
from that generation without cancelling already routed peer tasks. Stable
runtime shutdown still stops registration, cancels/joins every peer, stops the
upload scheduler, and proves terminal zero. Preserve a convenience
`IncomingPeerService::{bind,start}` facade for focused engine callers if it can
compose the same split without duplicating behavior; the application path uses
the split owner directly.

### Stable UDP transport and DHT

Refactor `SessionUdpService` so the one `SessionUdpTransport` consumed by DHT
has stable ingress and a send path that resolves the current socket
generation. Each socket generation owns its concrete `UdpSocket`, local
address, receive cancellation, and joined receive task. A handover starts the
candidate receiver into the same bounded ingress route, atomically changes the
send/current-address generation, and then cancels/joins the old receiver.

DHT must not be restarted for a socket change. Its actor, node ID, routing
table, command queue, observations, and discovery handle remain stable. A
response arriving through the old receiver during the bounded overlap may
still enter the same DHT ingress. The frozen `DhtService::local_address` fact
must become a current transport observation rather than an application-
generation constant. Standalone DHT construction may retain its convenience
owned-UDP path by composing the same stable transport.

### Stable endpoint and replaceable reachability

`AdvertisedPeerEndpointSelector` survives transport generations. Add an exact
listener-generation transition that clears incoming-observed evidence and any
mapped endpoint from the old listener before publishing the new local
endpoint. Only final application shutdown enters its terminal `stopping`
state; stopping one reachability generation must no longer stop the selector.

`ReachabilityCoordinator` remains a focused, replaceable owner for one
effective listener and mapping policy. It must cancel and join its task,
delete or retain truthful bounded cleanup state for its mapping, and reject
late events before the next generation can publish. Discovery advertisement
retains its stable registrations and consumes the selector's unchanged watch
waist throughout.

### Live admission and upload policy

`PeerBudget` remains the single accounting authority and gains atomic
reconfiguration. Admission reads the new effective limit before any eviction
request is sent. Each live permit registers a bounded cancellation handle with
its generation and phase; release removes it exactly once. Reconfiguration
collects cancellation targets under the budget lock and invokes cancellation
after releasing the lock.

When the current total exceeds the new effective limit plus the fixed ten
incoming slack, cancel connecting generations before established generations,
and within each class cancel newest admission generation first. This simple
policy preserves older established work and is deterministic. It deliberately
does not invent peer scoring, torrent fairness, auto-management, or durable
reputation. The domain remains applying until permit accounting reaches the
existing absolute bound. The fixed slack continues to admit incoming
candidates above the normal ceiling and never becomes a user setting.

`UploadCoordinator` and the task-free `UploadScheduler` gain focused
reconfiguration rather than replacement. Existing peer IDs, interest,
payload totals, quota history, timers, and watch senders remain. A slot change
recomputes grants immediately; scheduler cadence and the 15/30-second policy
remain unchanged.

## Owner, Task, Cancellation, And Dependency Map

```text
ApplicationService
  -> SessionStore desired ClientSettings
  -> TorrentRuntime map
       -> stable incoming registration handles
       -> stable discovery registrations
       -> stable advertised-endpoint observation
  -> SessionNetworkRuntime (private session owner)
       -> latest desired-settings cell
       -> one reconciliation task
       -> stable IncomingPeerRuntime
            -> replaceable TCP AcceptorGeneration
            -> stable upload scheduler + peer tasks + counters
       -> stable PeerBudget + bounded permit cancellation entries
       -> stable SessionUdpRuntime
            -> replaceable UdpSocketGeneration
       -> stable DhtService
       -> stable DiscoveryAdvertisementService
       -> stable AdvertisedPeerEndpointSelector
       -> replaceable ReachabilityCoordinator generation
```

Dependency direction remains inward. Pure settings validation and candidate
selection do not depend on Tokio, sockets, SQLite, or views. Engine transport
and runtime owners do not import session persistence or product contracts.
The session network owner translates persisted policy into engine config and
projects engine observations into the existing view hub. Product adapters see
only the generated application contract.

Shutdown ordering is exact:

1. stop accepting new application commands and close the settings sender;
2. cancel/join reconciliation and prevent child replacement;
3. stop discovery scheduling/registrations and the effective reachability
   generation, fencing external endpoint publication;
4. stop the TCP acceptor, then stable incoming registration and peer owners;
5. stop DHT while its stable UDP transport is still alive and collect its
   terminal snapshot;
6. stop and join the active UDP generation; and
7. return a terminal report only after all task, socket, mapping, permit, and
   registration counts are zero.

An error in one shutdown child is accumulated and reported after the remaining
children receive cancellation and join attempts. Drop remains a last-resort
abort safety net, not the successful lifetime path.

## Resource And Security Boundaries

- The desired-settings path owns one latest-value cell and one reconciliation
  task. It adds no per-change queue or history.
- At steady state there is at most one TCP acceptor generation, one UDP socket
  generation, one reachability generation, one DHT actor, one discovery
  service, and one upload scheduler.
- A transport handover may hold at most the current and one candidate TCP/UDP
  socket set and at most two accept tasks and two UDP receive tasks until the
  old generation joins. The shared pending-handshake semaphore remains eight
  across both acceptors. Handover does not double peer-task, registration,
  DHT, discovery, or upload-owner limits.
- The existing five-entry listen backlog, eight pending handshakes, 1,024
  registrations, 64-datagram DHT route, session peer limit plus ten incoming
  slack, upload read/writer/file limits, and diagnostic string bounds remain
  exact.
- Peer-eviction cancellation entries are bounded by live peer-budget permits,
  including slack. They retain cancellation capability and small identifiers,
  not sockets, payload buffers, peer messages, or history.
- One confirmed or uncertain finite UPnP lease may exist for the owned
  generation. A failed delete blocks creation of another owned lease until
  cleanup succeeds or expiry is established; repeated settings changes cannot
  accumulate mappings.
- Listener changes do not expand binding scope, enable all-interface access,
  alter fixed-port exactness, add IPv6 listening, or infer public
  reachability. Tracker and DHT advertisement continue to consume only the
  selected truthful TCP endpoint.
- Settings and diagnostics never expose gateway credentials, packet payloads,
  peer addresses beyond existing bounded product observations, raw OS errors
  beyond bounded classified detail, or a persisted actual local/external
  endpoint.

Validation must record steady-state and handover high-water counts for bound
sockets, accept/receive/reconcile/mapping tasks, peer permits, mappings,
registrations, and queued UDP datagrams. A successful shutdown declares
terminal zero for every owned child.

## Normative And Reference Dossier

These settings are product/session policy rather than a new BitTorrent wire
extension. BEP 3 peer lifetime, BEP 5 DHT behavior, tracker advertisement, and
UPnP behavior remain governed by the completed tacticals that introduced
them. This slice changes ownership and live state transitions without changing
wire codecs.

### Pinned libtorrent 2.0.13

The required checkout is `reference/libtorrent` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` (`v2.0.13`). Inspected paths and
cases are:

- `src/settings_pack.cpp` attaches update callbacks to
  `listen_interfaces`, `connections_limit`, `unchoke_slots_limit`, and the
  upload/download rate limits that will matter to a later slice;
- `src/session_impl.cpp::{apply_settings_pack_impl,
  update_listen_interfaces,reopen_listen_sockets}` detects listener changes
  and rebuilds live listen sockets;
- `src/session_impl.cpp::update_connections_limit` normalizes the limit and
  disconnects excess peers across running torrents;
- `src/session_impl.cpp::update_unchoke_limit` updates fixed-slot policy and
  triggers reevaluation or unchokes all for its unlimited sentinel;
- `test/test_session.cpp::session` and `test/test_transfer.cpp` apply slot
  values repeatedly to a running session;
- `simulation/test_session.cpp::tie_listen_ports` proves coordinated TCP/UDP
  numeric ports;
- `simulation/test_swarm.cpp::{default_connections_limit,
  default_connections_limit_negative,unchoke_slots_limit,
  unchoke_slots_limit_negative,settings_stress_test}` exercises unusual and
  repeated runtime settings; and
- `simulation/test_tracker.cpp::clear_error` includes live listener-interface
  application in an active tracker scenario.

RSTorrent adopts live listener replacement, admission reduction, immediate
slot reevaluation, and settings-stress coverage as completeness behaviors. It
does not copy libtorrent's session implementation or exact handover order.
Libtorrent removes obsolete sockets before opening some replacements to avoid
self-conflict; RSTorrent's narrower explicit-address policies permit a
candidate-first handover and require retention of the prior effective
generation when preparation fails. RSTorrent also retains fixed-port
exactness, loopback-by-default product posture, bounded positive connection
values, bounded nonnegative slot values, coordinated TCP/UDP policy, explicit
desired/effective/degraded product state, and its existing incoming slack.

The rate-limit callbacks are recorded because the stable session owner must
not preclude that next feature. No rate-limit behavior is adopted in this
tactical.

### JSTorrent product history

The local checkout is `../jstorrent` at
`9895410beeed6aff554053769bd006a3fbd373ef`. The relevant inspected paths from
Tactical `084` remain:

- `packages/engine/src/config/config-schema.ts` defines typed settings,
  numeric bounds, and restart-required port fields;
- `packages/engine/src/config/base-config-hub.ts` separates effective cached
  values from persisted pending-restart values and publishes subscriptions;
- `packages/engine/test/config/{config-hub.test.ts,
  native-config-hub.test.ts,config-engine-integration.test.ts}` tests
  validation, pending values, subscriptions, and propagation; and
- `packages/client/src/components/SettingsOverlay.tsx::NetworkTab` presents
  configured/current ports, connection/slot values, and restart copy.

RSTorrent retains JSTorrent's useful distinction between configured and
effective truth but deliberately removes restart as the convergence mechanism.
It does not copy the large configuration hub, per-key persistence, UI schema,
or JavaScript runtime ownership. No source, fixture, or UI text is imported.

### Current RSTorrent pressure points

The implementation survey that selected this boundary found:

- `crates/rstorrent-session/src/application.rs::{ApplicationService::open,
  ApplicationService::handle,ApplicationService::shutdown}` constructs,
  ignores runtime settings mutation for, and manually stops the coupled
  session network owners;
- `crates/rstorrent-session/src/settings/{contract,runtime}.rs` exposes the
  five-field group and monolithic restart-applied view;
- `crates/rstorrent-session/src/store.rs::apply_mutation` rejects ephemeral
  settings only because restart is currently required;
- `crates/rstorrent-engine/src/incoming.rs::IncomingPeerService` couples one
  listener/accept task to stable registrations, peer tasks, upload scheduling,
  and counters under one cancellation token;
- `crates/rstorrent-engine/src/session_udp.rs::{SessionUdpService,
  SessionUdpTransport}` fixes one socket and address for its whole lifetime;
- `crates/rstorrent-engine/src/dht.rs::DhtService::start_with_transport`
  retains that transport and a frozen local address;
- `crates/rstorrent-engine/src/peer_budget.rs::PeerBudget` has immutable
  configuration and accounting but no live eviction capability;
- `crates/rstorrent-engine/src/incoming/upload_runtime.rs::UploadCoordinator`
  has stable memberships but immutable scheduler configuration;
- `crates/rstorrent-session/src/{advertised_endpoint,reachability}.rs` owns
  generation fencing but treats selector stop as application-terminal; and
- `clients/web/src/inspection/components/ConnectionSeedingSettingsSection.tsx`
  presents restart-required outcomes.

These are lifetime and convergence pressures, not evidence for changing the
crate graph or rewriting unrelated torrent, storage, view, or gateway owners.

## Shape-Changing Edge Cases

The common implementation and tests must include:

- startup with disabled, automatic loopback, fixed loopback, automatic local-
  network, and fixed local-network policy;
- successful disabled-to-enabled, enabled-to-disabled, automatic-to-fixed,
  fixed-to-automatic, address-scope, and preferred-port transitions;
- a preferred-port change under fixed and disabled policy that updates
  effective settings without needless socket churn;
- TCP candidate success followed by UDP conflict, fixed exact conflict,
  non-`AddressInUse` failure, automatic retry exhaustion/system fallback, and
  port `65535` without wrap;
- first-start bind failure with independent DHT UDP service and later no-op
  retry success;
- replacement bind failure while an old listener, DHT transport, mapped
  endpoint, connected incoming upload, and outgoing download remain active;
- an established incoming payload write crossing listener handover with exact
  once-only byte accounting;
- a DHT request/response crossing UDP generation overlap, unchanged node ID,
  retained routing state, and the new source endpoint used afterward;
- endpoint withdrawal and republish with incoming-observed evidence reset,
  tracker/DHT port-`1` fallback during the gap, and no stale mapped endpoint;
- UPnP enable/disable, discovery/add/verify/renew/delete failure, listener
  replacement with a live mapping, uncertain delete, expiry, and retry without
  accumulating mappings;
- peer-limit increase below/above the descriptor clamp and decrease with mixed
  outgoing/incoming connecting/established permits, incoming slack, concurrent
  release, exact-once cancellation, and deterministic victim order;
- slot changes `8 -> 0 -> 1 -> 8 -> 50` with interested/uninterested peers,
  optimistic grants, queued requests, partial writes, and unchanged totals;
- combined atomic settings where transport degrades while connection, slot,
  or mapping policy still converges;
- A-to-B-to-C changes before A cleanup completes, exact request replay, same-
  value retry, stale endpoint/mapping events, and runtime generation overflow
  handled without wrapping to a valid old generation;
- store failure before publish, view publication failure after persistence,
  client disconnect during apply, application shutdown during candidate bind
  or mapping deletion, task panic, and error aggregation with terminal cleanup;
  and
- durable reopen from desired state plus ephemeral mutation/application with
  no cross-process durability claim.

## Staged Implementation And Intermediate Gates

### Gate 1: contract and pure convergence model

Add effective-domain and application-state contract values, remove
restart-required semantics, define the monotonic desired/domain transition
model, and cover all pure transitions and stale-event rejection. Update
generated TypeScript/JSON Schema/UniFFI output and fixtures. No socket lifetime
changes land behind an ambiguous public state.

Completed evidence on 2026-08-06:

- `ClientSettingsRuntimeView` now carries optional effective listener policy,
  effective mapping/descriptor-clamped peer/slot values, and four typed domain
  application states. The old `active` and `restart_required` fields are
  absent from Rust, generated TypeScript, JSON Schema, validators, fixtures,
  React presentation, and the Android reducer fixture.
- the pure convergence model assigns a fresh nonzero attempt and four domain
  generations to same-value retries, accepts domain outcomes independently,
  fences A-to-B-to-C stale results, bounds degraded detail to 512 UTF-8 bytes,
  and refuses attempt or domain overflow without mutation;
- `cargo test -p rstorrent-session --no-fail-fast` passed 167 executable tests
  (160 library, two profile, one seed CLI, four main; one maximum-allocation
  library case remained ignored), and focused session/gateway Clippy with
  warnings denied plus the UniFFI feature check passed; and
- generated web artifacts were regenerated, TypeScript typecheck passed, and
  Vitest passed 178 tests with two intentionally skipped. Full workspace,
  production build, desktop, generated Kotlin, and Android cross-build gates
  remain Gate 5 evidence.

### Gate 2: stable engine owners without product behavior change

Split stable incoming state from its acceptor, make session UDP transport
socket-generation aware, keep DHT stable across a scripted replacement, and
add focused peer-budget/upload-scheduler reconfiguration. Preserve existing
convenience entry points and prove the current startup/shutdown behavior before
wiring settings.

### Gate 3: private session-network owner

Extract current network composition and shutdown from `ApplicationService`
into `SessionNetworkRuntime`. Start and stop the same effective initial
configuration, expose only the stable handles required by `TorrentRuntime`,
and prove byte-for-byte-compatible runtime views except for the intentional
new settings contract. This gate must reduce manual coupled lifetime in the
application root; merely wrapping unchanged replace-all services is
insufficient.

### Gate 4: live resource and transport convergence

Wire successful settings commands to the latest-value reconciler. Land peer
limit and upload-slot changes, coordinated socket handover, stable DHT,
endpoint fencing, reachability replacement/cleanup, partial failure, no-op
retry, and rapid-generation cases. Preserve active transfers throughout the
controlled handover cases.

### Gate 5: persistence and product closure

Allow ephemeral mutation, update response/view semantics and all adapters,
replace restart copy with applying/applied/degraded presentation, and run the
complete validation matrix. Update this tactical and owning topics with exact
owner high-water marks, interop outcomes, commands run, deviations, and
remaining rate/accounting/goal gaps.

Each gate must leave the workspace buildable and its selected tests green.
Do not temporarily emulate live application by restarting `ApplicationService`
or all network children; that would erase the lifetime property this tactical
exists to prove.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Settings validation; desired/effective domain transitions; no-op and replay generations; A/B/C stale rejection; bind candidate policy; peer eviction order; slot reconfiguration; mapping cleanup state; overflow and bounded-detail cases. |
| Engine scripted runtime | Stable incoming registrations/peer/upload state across acceptor replacement; stable UDP ingress/send and DHT node/routing state across socket replacement; mixed peer-permit reconfiguration; exact upload grant and partial-write behavior; task panic/cancellation and terminal owner counts. |
| Session/application | Atomic durable save before apply; successful ephemeral mutation; response/view ordering; independent domain convergence; failed candidate retaining old effective facts; startup/reopen; shutdown during every transition; no detached child. |
| Controlled interoperability | One active RSTorrent upload and one active download survive a successful listener/UDP handover; old TCP endpoint rejects and new endpoint accepts; DHT continues a controlled exchange and discovery registration; a pinned libtorrent peer completes and hash-verifies transferred payload after handover. |
| Reachability | Scripted IGD enable/disable/remap, deletion failure/expiry, generation fencing, no second uncertain lease, exact advertised endpoint, and zero owned mappings at terminal shutdown. Physical router or public-swarm evidence is not required for this ownership slice. |
| Product contracts | Generated schema, TypeScript, validators, UniFFI/Kotlin bindings, default fixtures, reducers, demo state, and hostile validation agree on the new contract with no `restart_required` field. |
| Web and desktop | Settings save shows applying then applied/degraded; configured/effective values remain readable; same-value retry works; validation/persistence failure retains the draft; focused tests, typecheck, production build, CSP/build scan, and Tauri compile pass. No visible client launch is needed. |
| Android/platform | Generated bindings and established Android target cross-builds pass. This tactical adds no Compose screen, foreground-service policy, AVD, physical-device, or ChromeOS requirement. |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, architecture/dependency checks, and generated-artifact drift gates pass. |

Controlled tests must record actual configured/effective settings, TCP and UDP
endpoints before and after, DHT node ID, peer and registration identity,
mapping generations, exact transferred/verified bytes, high-water owners, and
terminal counts. Do not claim public reachability, router compatibility, or
performance improvement from loopback evidence.

## Non-Goals And Deliberate Deferrals

- No upload/download byte-per-second fields or limiter implementation.
- No per-torrent rate caps, priorities, traffic classes, alternate-rate mode,
  IP overhead accounting, or scheduler auto-tuning.
- No durable per-torrent/session uploaded or downloaded totals, payload versus
  protocol-overhead ledger, reset epoch, or historical rate series.
- No ratio, seed-time, idle-time, share-mode, active-torrent queue,
  seed-rank, or automatic stop/remove policy.
- No change to the existing fixed ten incoming slack, five-entry listen
  backlog, pending handshake bound, per-torrent outbound working sets,
  upload-read/write bounds, or file-handle pool.
- No peer-quality scoring, fair distribution of evictions among torrents,
  duplicate-peer resolution, or durable reputation. Planned Tactical
  [`090`](090-peer-id-duplicate-connection-resolution.md) remains independent.
- No IPv6 listener, uTP, UDP tracker socket sharing, NAT-PMP, PCP, firewall
  pinhole, LSD, VPN/interface preference, metered-network policy, or Android
  background-policy expansion.
- No settings schema framework, callback registry modeled after libtorrent,
  generic actor/service abstraction, new crate, daemon, REST settings
  resource, or stable public API version claim.
- No restart of the application, DHT, discovery service, stable incoming
  runtime, existing peer tasks, or `TorrentRuntime` map as an implementation
  shortcut.

## Next-Slice Boundary

After this tactical, the recommended sequence for the original rate/seeding
completeness gap is:

1. define and persist an exact payload/protocol accounting ledger with
   session and per-torrent lifetime/reset semantics;
2. add hierarchical live session and per-torrent upload/download bandwidth
   allocation through the stable network owner and torrent children;
3. add ratio/time goals and their durable state machine on top of the ledger;
   and
4. add queue/auto-management only after those facts and policies are proven.

Each remains its own bounded, source-first tactical. This slice must leave a
concrete insertion point for a later bandwidth controller, but it must not
pre-decide allocator algorithms or create unused abstractions.

## Escalation Contract

Implementation may proceed without routine maintainer input for private
module/file layout, exact internal type names, moving existing tests,
conservative tightening of declared bounds, same-boundary bug fixes,
generated-contract updates, and adversarial cases implied by these
invariants.

Stop for direction if evidence requires a different visible settings meaning,
loss of established transfers during ordinary handover, a persistence schema
or compatibility policy beyond the stated contract, a new external
dependency, a new crate/process/IPC boundary, changes to network exposure or
fixed-port policy, accumulation of uncertain router mappings, destructive
data action, physical-device/public-network use, or implementation of a
deferred rate/accounting/seeding-goal feature.
