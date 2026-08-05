# Tactical 086: Long-Lived Torrent Peer Runtime

Status: Completed on 2026-08-05. All five gates and the stopping condition
pass.

Topics: `incoming-reachability-and-seeding`, `peer-lifecycle`,
`code-organization-and-refactoring`, `application-view-api`,
`peer-flag-vocabulary`, `capability-readiness`

## Decision And Motivation

Make truthful incoming-peer projection the end-to-end proof of one missing
lifetime boundary: peer state belongs to a torrent runtime that survives an
individual download operation, not to the download operation itself.

Tacticals [`078`](078-local-single-peer-tcp-seeding.md) and
[`082`](082-bounded-multi-peer-upload-ownership.md) deliberately established a
separate application listener, routing registry, multi-peer upload owner, and
complete-torrent registration lifecycle. Tactical
[`084`](084-persisted-client-connection-and-seeding-settings.md) then made the
listener and its shared connection/upload limits durable and observable.
Those slices proved useful seeding, but ordinary peer observation still has a
different lifetime:

- `ApplicationService` owns one `Option<ActiveDownload>` and separately owns
  `IncomingSeeding` plus `IncomingPeerService`;
- each `TorrentPeerCoordinator` constructs and owns its own `PeerRegistry` and
  `PeerRuntime` inside the active download task;
- `DownloadControl` is the only production path that publishes those ordinary
  peer and registry snapshots; and
- completion terminates that download owner while registration-owned incoming
  peers continue uploading through `RegistrationRuntime`.

The existing `PeerRuntime::begin_incoming` vocabulary is test-only. The live
incoming loop retains the remote endpoint, remote peer ID, extension support,
interest/choke state, upload grant, queued requests, payload totals, and rates,
but drops most of those facts before the ordinary Peers and Swarm models.
Bridging aggregate incoming snapshots directly into `ViewHub` would preserve
two peer authorities and make download-to-seed completion a permanent special
case.

The selected correction is deliberately between two unsafe extremes:

- do not add a view-only adapter around the separate incoming runtime; and
- do not rewrite the complete application session, introduce general
  multi-torrent scheduling, or reproduce libtorrent's class graph.

Instead, extract one task-free engine peer-state owner and one concrete
session-side per-torrent lifetime owner. Outgoing and routed incoming
connection tasks attach to the same state and publication boundary. Existing
direction-specific socket, scheduler, request, storage, and task supervisors
remain responsible for their current invariants.

## Desired Outcome And Stopping Condition

The tactical stops only when all of the following are true:

- every application catalog torrent has at most one application-generation
  `TorrentRuntime`, keyed by canonical torrent ID and retained across the
  transition from active download to eligible completed seeding;
- that runtime owns one bounded task-free torrent peer state containing the
  ordinary peer registry, active connection generations, collision-free
  connection-ID allocation, and snapshot publication state;
- the current single-active-download policy remains unchanged, but the active
  download borrows the torrent runtime instead of constructing a second peer
  authority;
- an accepted incoming TCP connection remains session-owned before its info
  hash is known, then attaches to the routed torrent runtime during the
  BitTorrent handshake and uses the same lifecycle vocabulary and snapshots
  as an outgoing connection;
- a real incoming seed connection appears in the existing `torrent_peers`
  view with truthful direction, endpoints, peer identity/client hint,
  extension and metadata capability, interest/choke state, upload grant,
  upload queues, exact physical payload total, and sampled payload rate;
- its existing compact flags truthfully include `I`, `U` or `u`, `O`, `x`,
  and `m` exactly when the typed facts warrant them;
- the corresponding bounded Swarm record carries source `incoming` and treats
  the accepted socket's remote ephemeral port as non-connectable unless later
  independent evidence supplies a listening endpoint;
- disconnect exposes `disconnecting` until connection-specific upload,
  request, read, writer, budget, and scheduler cleanup completes, then removes
  the Peers row without allowing a stale generation to affect a replacement;
- pause, archive, recheck, repair, removal, download failure, registration
  replacement, and application shutdown publish final empty state after all
  affected tasks join; and
- controlled RSTorrent and pinned libtorrent peers receive exact verified
  content while the ordinary headless product Peers/Swarm views observe the
  incoming lifecycle and every declared resource count returns to zero.

A struct extraction, a successful handshake, an aggregate incoming counter,
or an `I` flag by itself does not satisfy the stopping condition.

## Architectural Diagnosis

### Current owner mismatch

| Concern | Current owner | Lifetime problem |
| --- | --- | --- |
| Outgoing peer records and active generations | `TorrentPeerCoordinator::{registry,runtime}` | Exists only for one metadata/download operation. |
| Ordinary peer snapshot publication | `DownloadControl` and its download activity sink | Ends with the operation and is unavailable to a completed seed. |
| Incoming torrent eligibility | session `IncomingSeeding` registration map | Correctly survives download completion, but has no ordinary peer state. |
| Incoming connection tasks and upload state | engine `RegistrationRuntime` plus session-wide incoming `Shared` state | Correctly owns joined upload work, but exposes only aggregate/service snapshots. |
| Peers and Swarm presentation | session `ViewHub` | Can already represent incoming direction and upload fields, but receives only outgoing observations. |

This is concrete feature-driven pressure on the source-organization guidance.
`ApplicationService` remains the application root, but active-operation and
incoming-registration reconciliation for one torrent now need one named child
owner. `PeerRuntime` and `PeerRegistry` remain deterministic engine state, but
their lifetime can no longer be nested beneath `TorrentPeerCoordinator`.

### Adopted boundary from the pinned oracle

Libtorrent's relevant boundary is strong even though its implementation is
not an architecture template:

1. `session_impl` owns listening, pre-routing admission, global limits, the
   torrent collection, and the global live-connection collection.
2. An incoming `peer_connection` begins without a torrent and attaches only
   after its handshake identifies an eligible info hash.
3. The long-lived `torrent` owns one peer list and live-connection collection
   across downloading and finished/seeding states.
4. The same `peer_connection` owns bidirectional choke/interest, queues,
   protocol flags, endpoints, totals, and rates regardless of who initiated
   the socket.
5. Torrent and session removal paths maintain explicit membership and
   deferred-destruction invariants.

RSTorrent adopts those lifetime relationships while retaining explicit Rust
values, typed direction, immutable snapshots, cancellation tokens, joined
Tokio task owners, and no raw-pointer cross-links. It does not adopt
`session_impl` or `torrent` as giant objects, libtorrent's single network
thread, its peer ranking/replacement policy, or its deferred `undead_peers`
mechanism.

## Accepted Ownership And Dependency Direction

### Session-wide application owner

`ApplicationService` remains the root owner of:

- the incoming listener and pre-info-hash routing registry;
- the global `PeerBudget`, upload scheduler, DHT service, storage file pool,
  settings, persistence, and `ViewHub`;
- a concrete map of `TorrentRuntime` entries keyed by canonical torrent ID;
  and
- the current policy that admits at most one active metadata/download
  operation.

The map is bounded by the application catalog already loaded by the session.
An inactive entry owns no socket, task, channel, storage handle, or timer.
Construction may be lazy if tests prove that one application generation is
stable from first activation through completion and later ineligibility, but
completion must not replace it. A torrent removal deletes the entry only
after every child owner has joined and final empty observations have been
published.

### Session `TorrentRuntime`

A private `rstorrent-session` child module owns the per-torrent application
lifetime. Exact names may follow local conventions, but its responsibilities
are fixed:

- canonical torrent ID and an application-generation fence;
- the engine torrent-peer handle and its narrow view-publication sink;
- optional active download control/task membership;
- optional incoming seed registration token and reconciliation state; and
- transition ordering among download completion, seed registration,
  pause/archive/recheck/removal, and final runtime teardown.

The existing global one-operation rule may remain as an `active_torrent` ID
or equivalent admission state in `ApplicationService`. Moving
`ActiveDownload` beneath `TorrentRuntime` does not authorize concurrent
downloads, queued-torrent scheduling, torrent priorities, or outbound
connection attempts for finished torrents.

`IncomingSeeding` may become a focused helper beneath this module or retain a
small facade, but it must no longer own a second torrent-ID-to-registration
map that can disagree with the runtime registry. SQLite and catalog mutation
remain owned by `SessionStore` and `ApplicationService`; `TorrentRuntime` is
ephemeral coordination, not a repository or durable domain object.

### Engine `TorrentPeerState`

A focused private engine module owns one task-free deterministic state:

- the existing bounded `PeerRegistry`;
- the existing active `PeerRuntime` state, generalized for both directions;
- one checked monotonic connection-generation allocator per torrent runtime;
- the mapping among outgoing dial attempts, incoming upload memberships,
  peer records, and active connection IDs;
- active/inactive registry projection state; and
- coalescing state for bounded activity snapshots.

One concrete cloneable handle serializes short mutations of that state. It
must not expose a mutex guard, perform network or storage I/O, await while
locked, or call the session/view sink while locked. Lifecycle transitions
publish immediately; high-frequency counter/rate changes retain the existing
100-millisecond coalescing behavior or a demonstrably equivalent bound.

Connection IDs come from this one authority for both directions. Do not reuse
`UploadPeerId`, infer IDs from direction, or let an outgoing `DialAttemptId`
collide with an incoming generation. If the existing one-to-one outgoing ID
assumption changes, retain an explicit generation mapping in the registry and
swarm owners and prove stale-attempt behavior. Checked exhaustion is a typed
terminal error, never wraparound or ID reuse inside an application torrent
generation.

`PeerConnectionObservation` retains common connection facts and separates
direction-neutral state from optional transfer-direction activity:

- common: ID, optional record ID, remote and optional local endpoint,
  sources, direction, transport, lifecycle, role, ages, peer ID/client input,
  BEP 10 support, `ut_metadata` negotiation, and close reason;
- download activity: the existing wanted pieces, request window, queued
  payload, useful bytes/rate, and stall ages; and
- upload activity: remote interest, local choke, regular/optimistic grant,
  queued requests/bytes, read/writer state useful to the existing contract,
  and exact physical payload bytes/rate.

Do not force upload facts into `PeerContentActivity`, infer upload state from
aggregate session counters, or make the view mapper query live task owners.

### Connection task owners

The refactor does not require one new connection actor or task supervisor:

- the incoming service retains accept and pre-routing handshake task
  ownership;
- `RegistrationRuntime` retains joined incoming peer reader/writer/read work
  unless extraction exposes a narrower same-owner child;
- the existing outgoing socket/metadata/content supervisors retain their
  task and request ownership; and
- each task receives one generation-scoped attachment token for the shared
  torrent peer state.

After an incoming handshake identifies an eligible registration, the task
attaches a `ProtocolHandshaking` generation before writing the local
handshake. Successful two-way handshake advances it to `Connected`; write
failure, stale registration, cancellation, or task failure transitions or
force-cleans that exact generation. Unknown info hashes, invalid/self
handshakes, and admission rejected before routing remain session diagnostics
and never create torrent Peers or Swarm rows.

Every ordinary terminal path explicitly enters `Disconnecting`, cleans the
direction-specific scheduler/request/read/writer and registry membership,
releases the peer-budget permit, then removes the row. A generation guard
provides a synchronous last-resort removal on unexpected task unwind so a
panic cannot leak a row; that fallback records a bounded diagnostic and does
not pretend the graceful `Disconnecting` evidence occurred.

### Observation and view owner

Extract peer and registry publication from the download-only activity sink
into one narrow engine-to-session torrent-peer sink justified by both live
directions. The session implementation captures the torrent ID and updates
the existing `ViewHub`; it adds no transport route, view subscription owner,
or browser polling loop.

The current generated `PeerView`, `SwarmPeerView`, and compact flag enum are
already sufficient. This tactical should populate their existing fields and
capabilities rather than add a parallel incoming DTO. Generated contracts
must remain byte-identical unless implementation uncovers a fact that cannot
be represented truthfully; such contract expansion requires maintainer
direction.

## Owner, Task, Lock, And Cancellation Map

| Owner | Mutable state | Tasks or queues | Cancellation and terminal observation |
| --- | --- | --- | --- |
| `ApplicationService` | session services, torrent-runtime map, single-active-torrent admission | existing DHT/history/view reaper and active operation handles | stops admission, drives each runtime shutdown, joins global services, closes views last |
| session `TorrentRuntime` | application generation, peer handle, optional active download and seed registration | no required new task or queue | unregister/cancel children, await their owners, publish final empty snapshots, then become removable |
| engine torrent-peer handle | short locked deterministic registry/connection/ID/snapshot state | no task; no unbounded event queue | generation-fenced transitions; synchronous failure cleanup; empty/inactive terminal snapshot |
| incoming service | listener, pre-routing tasks, routing registry, global upload scheduler and budget references | existing bounded accept, handshake, scheduler, peer, read, and writer owners | stop accept, reject new routing, cancel registrations, join peers/reads/writers, release permits |
| outgoing download owner | discovery, dials, socket workers, scheduler/request/storage coordination | existing bounded tasks and channels | cancel and join exact operation while detaching its generations from the retained peer handle |
| `ViewHub` | per-torrent Peers and Swarm projection plus leased delivery | existing bounded view-set queues only | consumes final removals before view sets and reaper close |

No path awaits while holding the application store mutex, the incoming
registration mutex, or the torrent-peer state lock. Lock order is not used as
a substitute for lifecycle ordering. Snapshot delivery clones only bounded
state after mutation and occurs outside all engine-state locks.

Application shutdown order becomes:

1. reject new application commands and incoming registration changes;
2. stop listener acceptance and prevent new info-hash attachments;
3. cancel each seed registration and active download while their torrent
   runtime and view sink remain alive;
4. join pre-handshake, incoming peer/read/writer, and outgoing operation
   owners, publishing exact disconnect and removal transitions;
5. publish final empty/inactive torrent peer and registry snapshots and remove
   runtime entries;
6. join remaining DHT, storage, speed-history, and view-reaper owners in their
   established safe order; and
7. close leased views only after no peer producer can publish another row.

The implementation must reconcile this with every existing shutdown and
failure test. Merely moving `close_view_sets` later without proving producer
termination is insufficient.

## Normative And Reference Dossier

No reference source, class graph, test fixture, or serialized representation
is copied.

### Normative behavior

- BEP 3 at `reference/bittorrent.org/beps/bep_0003.rst` defines the 68-byte
  info-hash handshake, connection-local choke and interest state, request and
  cancel behavior, and the distinction between a connected peer and a
  discoverable endpoint.
- BEP 10 at `reference/bittorrent.org/beps/bep_0010.rst` defines the reserved
  extension bit, connection-local extension handshake and directional message
  IDs. `x` and `m` must reflect observed negotiation, not client-name guesses.
- BEP 20 at `reference/bittorrent.org/beps/bep_0020.rst` provides conventional
  peer-ID client/version parsing. The derived label remains untrusted display
  information and never becomes connection identity or duplicate policy.

This tactical changes ownership and observation rather than peer-wire support.
All existing hostile-input and resource bounds continue to apply.

### Pinned libtorrent 2.0.13

The required checkout is `reference/libtorrent` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` (`v2.0.13`). The following exact
owners and cases were inspected while drafting:

- `src/session_impl.cpp::{incoming_connection,close_connection}` admits an
  incoming socket under session/interface/global-limit policy, creates an
  initially unattached `bt_peer_connection`, inserts it into the session
  connection collection, and later removes it through a network-thread-owned
  destruction path.
- `src/session_impl.cpp::try_connect_more_peers` shares the session connection
  limit across torrents and explicitly schedules both
  `torrent_want_peers_download` and `torrent_want_peers_finished`; finished
  seeding does not destroy the torrent peer owner.
- `src/bt_peer_connection.cpp::on_receive_impl` calls
  `peer_connection.cpp::attach_to_torrent` after reading the incoming info
  hash and notes that attachment creates incoming peer information before the
  rest of extension negotiation.
- `src/torrent.cpp::{attach_peer,remove_peer,on_remove_peers,get_peer_info}`
  attaches both directions to one torrent connection collection, reconciles
  the peer list and upload/optimistic counts during removal, skips only
  pre-handshake incoming connections not yet attached to a torrent, and
  reports every attached live connection through one peer-info path.
- `src/peer_connection.cpp::get_peer_info` reports upload and download
  payload/protocol rates and totals, endpoints, request/upload queues, pieces,
  and activity from the connection object.
- `src/bt_peer_connection.cpp::get_specific_peer_info` adds connection-local
  interest, choke, extension, outgoing/local-connection, transport,
  encryption, connecting, and handshake flags. RSTorrent retains its clearer
  typed `Incoming`/`Outgoing` direction rather than encoding incoming as the
  absence of `local_connection`.
- `src/peer_list.cpp::new_connection` treats an incoming remote ephemeral port
  as incoming evidence and can later strengthen the endpoint from independent
  listening-port evidence.
- `test/test_peer_list.cpp::{self_connection,double_connection,
  double_connection_loose,double_connection_random,double_connection_win,
  incoming_size_limit}` exercises simultaneous directions, two incoming
  generations, endpoint reversal/self detection, and bounded peer-list
  admission.
- `simulation/test_pause.cpp::{torrent_paused_disconnect,
  session_paused_disconnect,paused_torrent_add_peers,
  session_pause_resume_connect}` proves torrent/session pause prevents or
  disconnects peer work and exposes completion only after the relevant state
  transition.

RSTorrent adopts the session/pre-routing/torrent/connection lifetime
separation, one torrent peer observation path, and explicit removal
invariants. It intentionally defers libtorrent's connection replacement,
per-IP and peer-ID duplicate rules, queued-torrent wakeup, finished-torrent
outgoing dialing, peer classes, encryption, uTP, and alert architecture.

### JSTorrent product history

The local first-party checkout is `../jstorrent` at
`9895410beeed6aff554053769bd006a3fbd373ef`. Relevant paths inspected are:

- `packages/engine/src/core/bt-engine.ts::{startServer,
  handleIncomingConnection,destroy}` accepts at engine scope, bounds pending
  handshakes, routes by info hash, sets `isIncoming`, and then calls the same
  torrent `addPeer` used by outgoing connections. Its known failure to retain
  and close the server reinforces RSTorrent's existing joined listener owner.
- `packages/engine/src/core/torrent.ts::{addPeer,removePeer,getDisplayPeers,
  getPeerInfo}` makes `Swarm` the connection-membership authority and exposes
  both directions through one peer display path with choke, interest,
  uploaded/downloaded totals, rates, source, and incoming direction.
- `packages/engine/src/core/swarm.ts::{addIncomingConnection,markDisconnected,
  getConnectedPeers}` attaches incoming connections to the same bounded
  torrent swarm collection rather than retaining a parallel incoming list.
- `packages/engine/src/core/peer-connection.ts::PeerConnection` owns the
  bidirectional protocol state, peer ID, `isIncoming`, byte totals, and speed
  calculators for either direction.
- `packages/engine/test/core/{torrent-connection-limits.test.ts,
  self-connection.test.ts,peer-connection-stats.test.ts}` records mixed
  pending/incoming capacity, the handshake-before-attachment self-connection
  regression, and per-connection transfer observations worth reproducing.

RSTorrent does not adopt JSTorrent's address-keyed duplicate rule, incoming
ratio, tick loop, mutable object graph, unjoined server, MSE path, UI data
shape, or byte accounting that includes protocol frames. Exact physical piece
payload remains the upload total exposed here.

## Shape-Changing Edge Cases

The common path lands together with independently authored cases for:

- application open with inactive, downloading, complete-seeding, paused,
  archived, rechecking, removing, and corrupt durable torrent rows;
- download completion and publication while the outgoing operation is
  terminating and incoming registration becomes eligible, without replacing
  the torrent runtime or publishing a false inactive terminal state;
- incoming handshake attachment after info-hash routing but before the local
  handshake write, including cancellation or write failure at that boundary;
- unknown, invalid, self, inactive, stale-registration, and over-budget
  handshakes producing no torrent row;
- checked connection-ID exhaustion and stale attach/update/detach callbacks
  after a newer runtime or connection generation exists;
- simultaneous incoming and outgoing generations, two incoming generations
  from one IP with different ephemeral ports, and repeated peer IDs without
  silently merging live rows;
- an incoming ephemeral endpoint already present through tracker, DHT,
  magnet, or manual evidence, including source accumulation only when the
  endpoint identity actually matches;
- registry capacity full of active/banned records and hostile sequential
  incoming churn without exceeding the existing 1,000-record per-torrent
  bound or evicting protected records;
- remote interested/not-interested, regular and optimistic unchoke, rechoke,
  request/cancel, read admission, queued writer data, partial writes, and
  exact payload-rate sampling;
- BEP 10 absent, present without `ut_metadata`, and present with a valid
  connection-local metadata ID, with flags removed or retained exactly;
- peer close, protocol error, activity/no-request/inactivity timeout, storage
  failure, registration cancellation, task panic, and application shutdown at
  each lifecycle phase;
- a slow or saturated view consumer not blocking socket progress, admission,
  state cleanup, or joined shutdown; and
- pause/archive/recheck/repair/removal receipts not completing while an old
  generation can still publish or use storage authority.

Mature peer-ID duplicate resolution is not smuggled into these cases. Multiple
valid generations remain independently visible unless existing exact
endpoint/self policy rejects one; selecting a winning direction or replacing
an established peer is a later policy tactical.

## Resource And Compatibility Bounds

This tactical introduces no larger network or storage limits:

| Resource | Required bound |
| --- | --- |
| Session sockets | Existing configured/effective ordinary limit plus fixed ten incoming slack |
| Pending incoming handshakes | Existing eight tasks and five-entry listen backlog |
| Incoming registrations | Existing 1,024 generation-fenced entries |
| Per-torrent peer records | Existing 1,000-record registry with protected active/banned retention |
| Outgoing torrent work | Existing 30 pending dials and 30 established content peers |
| Upload slots | Existing configured 0--50 value, default eight including one optimistic grant |
| Upload requests/reads/writes | Existing 2,000 descriptors, ten shared reads, 40 handles, 64 writer descriptors, and 528,396-byte writer charge |
| Active observation rows | No more than admitted live connection generations for that torrent |
| Observation cadence | Immediate lifecycle changes; activity coalesced no more frequently than the existing 100 ms interval |
| Inactive torrent runtime | No socket, task, timer, channel, storage handle, or preallocated peer-record population |

Full snapshots remain bounded by these enforcing owners and the existing
leased view queue/snapshot byte limits. The implementation records current and
high-water runtime entries, active peer rows, registry records, pending and
established sockets, upload memberships, reads, writer charge, budget permits,
and terminal producers. It must not add an unbounded event log or retain
disconnected Peers rows as history.

## Implementation Gates And Commit Boundaries

### Gate 1: Task-free engine lifetime waist

Extract `TorrentPeerState` and its concrete handle, move existing outgoing
registry/runtime transitions behind it, establish collision-free IDs and a
narrow peer observation sink, and split download versus upload activity in
the observation value. Preserve current outbound snapshots, Peers/Swarm
behavior, timings, generated contracts, and all stale-generation tests.

Pure attach/update/disconnect/remove, source merge, ID exhaustion, activity
coalescing, and simultaneous-direction cases pass without Tokio, sockets,
storage, or a view hub. This is a useful commit boundary.

Completed on 2026-08-05. `torrent_peer` now owns the bounded registry,
active-generation state, one checked connection-ID allocator, source refresh,
and publication coalescing behind a cloneable task-free handle. Outgoing dial
attempts carry their separately allocated connection ID, removing the former
`ConnectionId == DialAttemptId` assumption while preserving stale-attempt
fencing. Incoming state now has pure routed attach, handshake, upload,
disconnect, and exact removal transitions plus non-connectable registry
membership. The complete 237-test engine library suite passes with three
opt-in live cases ignored; focused all-target engine clippy passes with
warnings denied.

### Gate 2: Session per-torrent lifetime owner

Add the private `TorrentRuntime` module and application-owned runtime map.
Move active-download membership and incoming registration reconciliation to
that owner while preserving the global one-active-operation rule. Prove that
completion transfers from download work to seed registration without
replacing the peer owner, and that pause/removal/shutdown publish empty state
only after exact joins.

No incoming connection attaches yet. Every existing application lifecycle,
restart, receipt, view, and controlled download test remains green. This is a
useful commit boundary.

Completed on 2026-08-05. `ApplicationService` now owns one private
`TorrentRuntime` per catalog torrent plus the unchanged single-active-torrent
admission ID. Each runtime retains its engine peer handle across metadata,
download, recheck, completion, eligible seeding, pause, archive, and resume;
active task membership and exact seed-registration ownership no longer live
in parallel application maps. A checked generation-fenced registration slot
serializes completion and command races without awaiting under its lock or
letting a stale registration become current. Download operations borrow the
runtime handle, retain the session view sink, and reconcile seeding before
their joined task terminates even when no later application command arrives.
Removal publishes final inactive peer state and deletes the runtime only
after its active task and seed registration are gone. Shutdown stops incoming
registration changes, cancels and joins network producers, publishes final
empty/inactive state, removes the runtime map, then joins storage, DHT,
history, and view owners before closing view sets.

The existing complete-seed lifecycle test now proves one runtime generation
survives archive/restore, a complete published recheck back into seeding, and
pause/resume, then disappears after removal. The 237-test engine library suite
passes with three opt-in live cases ignored; focused engine and session
all-target clippy pass with warnings denied; and the 143-test session library
suite passes with one allocation-profile case ignored.

### Gate 3: Routed incoming connection attachment

Carry the torrent peer handle through `SeedRegistration`; after validated
info-hash routing, attach an incoming handshake generation and preserve peer
ID, endpoints, extension negotiation, upload state, scheduler grant, exact
per-peer totals/rate, and terminal reason. Insert one non-connectable incoming
registry observation and detach through the explicit cleanup order.

Exercise malformed/unknown/self/stale handshakes, handshake-response failure,
budget and registry saturation, two directions, optimistic rotation, slow
read/write, timeouts, registration replacement, task failure, and final zero
state with scripted peers.

Completed on 2026-08-05. Every seed registration now carries its retained
torrent peer handle. A validated non-self handshake attaches a non-connectable
incoming record after info-hash routing and before the local handshake write;
global or registration cancellation and write failure synchronously clean that
exact generation. Successful response advances the row to connected and the
existing upload loop publishes the remote endpoint and peer ID, local
endpoint, BEP 10 and `ut_metadata` negotiation, interest/choke and exact
regular/optimistic grant, queued request and writer state, in-flight read,
exact physical payload total, and sampled payload rate.

Terminal paths enter `disconnecting` before writer shutdown. The joined peer
owner then clears request/read work, scheduler and counter membership, the
budget permit, and established accounting before exact row removal. A
generation-scoped drop guard provides synchronous cleanup on unexpected task
unwind. Unknown, invalid, self, stale, and pre-routing budget rejections still
create no torrent row; peer-state admission failure is separately counted.

The existing scripted single-peer test now observes the connected incoming
generation and truthful upload facts, verifies exact metadata and payload,
and records `disconnecting` before the final empty snapshot. The ten-peer test
observes ten distinct torrent connection generations under the existing eight
upload slots and final zero state. The complete 237-test engine library suite
passes with three opt-in live cases ignored, both engine and session
all-target clippy pass with warnings denied, and the complete-seed application
lifecycle remains green.

### Gate 4: Ordinary application and product projection

Map common/download/upload observation facts into the existing `PeerView` and
`SwarmPeerView`. Make field capabilities and compact flags truthful, preserve
one keyed row per active generation, and prove reset/patch/lease behavior,
aggregate peer counts, exact removal, and terminal inactive Swarm state.

No new page, table, route, command, setting, or browser timer is added. Existing
React peer-table components consume the generated contract unchanged. Focused
Rust, TypeScript mapping, reducer, and headless product tests cover the full
incoming row and flags. This is a useful commit boundary.

Completed on 2026-08-05. The existing `PeerView` mapper now projects the
incoming connection's local and remote endpoints, remote identity/client
hint, BEP 10 and `ut_metadata` observations, remote interest, local choke and
regular/optimistic grant, queued requests and bytes including the writer
buffer, connected age, exact physical upload total, and sampled upload rate.
Capabilities distinguish measured, unavailable, and directionally
unsupported facts. Compact flags remain enum-ordered and now derive
`incoming`, `upload_allowed`/`upload_choked`, `extension_protocol`,
`metadata_extension`, and `optimistic_unchoke` directly from those typed
facts.

The complete restarted-seed application test now connects one extension-
capable incoming peer through the production listener, negotiates metadata,
receives an exact payload block, and observes the same generation through
ordinary Peers and Swarm subscriptions. Peers reports a connected incoming
TCP row with exact endpoints, identity, upload facts, field capabilities, and
flags; Swarm reports the accepted ephemeral endpoint as connected,
`incoming`, and non-connectable. After socket close, Peers becomes empty and
Swarm retains the bounded record as non-connectable history. The existing
leased-view test independently proves keyed upload upsert, disconnecting, and
exact removal. The live TypeScript adapter test consumes the unchanged
generated contract and preserves incoming source, upload total/rate, pending
requests, and all five compact flags after view-set epoch recovery.

All 143 session library tests pass with one allocation-profile case ignored;
focused session all-target clippy passes with warnings denied; and the 15
focused live-adapter/flag tests plus TypeScript typecheck pass. Generated
contract artifacts remain unchanged.

### Gate 5: Controlled interoperability and closure

Extend the isolated incoming-seeding harness so outbound-only pinned
libtorrent and RSTorrent leechers connect to a restarted complete RSTorrent
seed while the ordinary application gateway subscribes to that torrent's
Peers and Swarm views. Independently verify exact payload, capture the active
incoming rows and typed facts, close peers, observe exact keyed removal, then
exercise pause and application shutdown.

Record exact resource high water and terminal zero counts, run the complete
validation matrix, update the tactical and all owning topics with only passed
evidence, and identify truthful actual-port advertisement as the next separate
reachability slice.

Completed on 2026-08-05. The isolated harness now prepares its durable
complete seed under the production gateway profile, reopens it behind the
authenticated polling gateway, and holds one outbound-only pinned libtorrent
2.0.13 leecher while an ordinary RSTorrent magnet leecher joins. One leased
view set follows the library, Peers, and Swarm projections throughout the
transfer. It observed two distinct incoming TCP generations identified as
`libtorrent 2.0.13` and `RSTorrent 0.0.0.1`; both rows negotiated BEP 10 and
`ut_metadata`, became remotely interested and locally unchoked, carried
nonzero per-peer upload totals, and exposed `incoming`, `upload_allowed`,
`extension_protocol`, and `metadata_extension`. The default eight-slot policy
gave both peers regular grants, so the absence of `optimistic_unchoke` in this
run is truthful; Gate 4's one-slot application test separately observes that
flag from an optimistic grant.

Both clients independently hash-verified the exact 67,109,595-byte,
4,097-piece single-file fixture. The gateway seed recorded exactly
134,219,190 physical payload bytes. After both sockets closed, Peers was empty
and Swarm retained exactly two `incoming`, non-connectable records. A real
pause command then produced inactive empty Peers/Swarm state. Gateway shutdown
reported zero registrations, pending/established peers, reads/read bytes,
peer-budget membership in every direction and phase, scheduler peers,
interested/regular/optimistic grants, torrent/peer upload records, gateway
connections, storage handles/cache entries, and platform requests.

The gateway run's resource high water was one pending handshake, two
established and budgeted connections, two upload slots, 500 queued requests,
8,192,000 queued bytes, two reads/32,768 read bytes, and 42,441 writer bytes,
all within the accepted bounds. The same harness also repeated the earlier
four-client overlap with exact 268,438,380-byte upload and the independent
8,401,233-byte cross-file/short-final-piece libtorrent and restarted-RSTorrent
transfers.

Final validation passed `cargo fmt --all -- --check`, workspace all-target
clippy with warnings denied, all workspace tests, byte-identical generated
contracts, 178 web tests with two opt-in interop tests skipped, TypeScript
typecheck, production web build and CSP scan, two consecutive controlled
libtorrent 2.0.13/RSTorrent harness runs, Python syntax compilation, and
`git diff --check`. Rust workspace totals include 234 engine library tests
passing with three opt-in public tests ignored and 142 session library tests
passing with one allocation-profile case ignored.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure peer state | One ID authority; outgoing equivalence; incoming attach; non-connectable incoming record; source merge; lifecycle legality; stale runtime/connection fencing; simultaneous directions; registry capacity; checked exhaustion; forced and coalesced snapshots |
| Upload projection | Interest/choke, regular/optimistic grant, queue/read/writer facts, exact physical totals and rates, extension/metadata negotiation, field capability and `I/U/u/O/x/m` derivation |
| Scripted runtime | Handshake attach/write boundary; unknown/self/stale rejection; response failure; mixed-direction limits; multiple peers; timeout/protocol/storage/task failure; slow view consumer; disconnect ordering; terminal zero tasks/permits/rows |
| Application lifetime | One runtime generation through download completion and seed registration; restarted complete seed; pause/archive/recheck/repair/removal fencing; active-download failure; final view delivery before view closure |
| View delivery | Peers/Swarm initial/reset/patch/lease behavior, keyed replacement/removal, active peer aggregates, bounded snapshot/queue behavior, no second incoming DTO or polling owner |
| RSTorrent interoperability | A normal RSTorrent magnet leecher obtains metadata and exact verified content from the reported listener while the seed's ordinary views observe its incoming generation |
| Libtorrent interoperability | Pinned libtorrent 2.0.13 with incoming disabled dials only the reported RSTorrent listener, receives exact content, and is visible with truthful incoming/upload facts before exact removal |
| Product | Existing headless shared web product displays the incoming row and compact flags through the production gateway; no visible browser, desktop shell, emulator, or physical device is required |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, generated-contract drift, web typecheck/tests/build, focused interop, and `git diff --check` |

Tests use injected clocks, barriers, bounded channels, and explicit view
receipts rather than correctness-sensitive sleeps. Lifecycle tests assert the
order of state transitions and owner joins, not merely eventual absence.
Reference fixtures or source are not imported.

## Non-Goals And Deliberate Deferrals

- A general session rewrite, a new `session_impl` analogue, a second
  application service, or a native daemon/IPC boundary.
- Concurrent multi-torrent metadata/download work, torrent queue scheduling,
  priorities, finished-torrent outgoing dialing, or global connection pacing.
- A new crate, generic repository/service hierarchy, runtime plugin system,
  actor framework, unbounded command bus, or one-file-per-type layout.
- Changing the 200/eight defaults, configured bounds, incoming slack,
  pending-handshake/backlog, request/read/writer/file limits, timeouts, or
  upload scheduling cadence.
- Incomplete-torrent upload, tit-for-tat, super seeding, finite bandwidth,
  ratio/time seeding goals, durable per-peer history, or persisted peer
  reputation.
- Mature peer-ID/per-IP duplicate resolution, preferred-direction selection,
  connection replacement, BEP 10 listening-port endpoint repair, or peer
  eviction UI.
- Tracker/DHT/BEP 10 actual-port advertisement, `announce_peer`, non-loopback
  or IPv6 binding, LAN/public reachability, UPnP IGD, PCP, NAT-PMP, firewall,
  VPN, or metered-network work.
- PEX, LSD, uTP execution, MSE/PE, FAST, hole punching, v2/hybrid torrents, or
  new protocol support claims.
- New settings, routes, commands, view kinds, tables, columns, pages, or
  Android/Compose presentation.
- Public-swarm performance tuning, physical-device testing, deployment, or
  external network/router mutation.

## Escalation And Autonomous Implementation Authority

Once implementation is explicitly authorized, ordinary authority includes
the private engine/session module extractions above, a narrow peer-observation
sink, internal connection-ID mapping changes, moving `ActiveDownload` and
incoming registration membership beneath `TorrentRuntime`, generated-artifact
regeneration needed to prove no drift, extending controlled loopback harnesses,
and fixing same-boundary lifecycle/view bugs exposed by the matrix.

Stop for maintainer direction before:

- enabling concurrent active torrents or changing catalog/queue semantics;
- changing a user-visible setting/default/resource limit or duplicate-peer
  policy;
- adding a public contract field, enum, command, route, view kind, or UI
  surface that existing typed fields cannot represent;
- moving DHT, tracker, storage, settings, persistence, or view-set ownership
  into the torrent runtime without a concrete requirement from this slice;
- introducing a new third-party dependency, crate, generic actor/service
  framework, or separate process; or
- using public networking, visible/physical clients, deployment, router
  configuration, or destructive user-data operations.

## Next Slice Boundary

After this tactical, local incoming seeding will use one truthful ordinary
peer lifecycle across download completion and restart. The next reachability
slice remains actual-listener-port advertisement through tracker and DHT
owners without implying gateway mapping or public reachability.

Finite bandwidth and ratio/time goals remain later because they need their
own enforcing and durable policy owners. Gateway mapping remains later still,
after advertisement consumes authoritative listener state. Broader
multi-torrent coordination may build on `TorrentRuntime`, but this tactical
does not authorize it.
