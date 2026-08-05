# Incoming Reachability And Seeding

Topic: `incoming-reachability-and-seeding`

Status: Tacticals
[`078`](../tactical/078-local-single-peer-tcp-seeding.md) and
[`082`](../tactical/082-bounded-multi-peer-upload-ownership.md) complete local
incoming TCP seeding through a bounded multi-peer upload owner with exact
physical-payload accounting. Tactical
[`084`](../tactical/084-persisted-client-connection-and-seeding-settings.md)
completes the first persisted listener, connection-limit, and upload-slot
settings slice through the shared product surface and restarted controlled
seeding. Tactical
[`086`](../tactical/086-long-lived-torrent-peer-runtime.md) is in progress.
Its task-free engine peer state and session per-torrent lifetime owner now
survive download-to-seed transitions, and routed incoming connections attach
to that owner with complete upload observations. Ordinary Peers/Swarm mapping
and the unchanged headless product adapter now consume those facts; controlled
interoperability and terminal resource evidence remain in its final gate.
Actual-port advertisement, non-loopback binding, finite bandwidth, seeding
goals, and NAT mapping remain future slices.

## Purpose And Scope

This topic owns the vertical product story from a locally bound BitTorrent
peer listener through useful seeding and externally reachable incoming
connections. It coordinates listener lifecycle, incoming handshake routing,
verified payload upload, upload scheduling and accounting, actual-port
advertisement, gateway port mapping, application settings, product status,
and the evidence required to claim each step.

This is a campaign and readiness record rather than one implementation
tactical. Each implementing tactical must retain one bounded, falsifiable
end-to-end outcome. The suggested sequence below distinguishes completed,
drafted, and prospective slices; drafting and numbering a slice does not by
itself authorize or prioritize implementation.

The topic does not make PEX, local service discovery, uTP, BEP 55 hole
punching, incoming MSE/PE, a remote daemon, or broad public-swarm seeding part
of the first slice. Those capabilities may depend on this foundation but keep
their own protocol, ownership, security, and evidence requirements.

## Current Truth

RSTorrent can download real v1 torrents through outgoing TCP and can now seed
multiple controlled incoming peers through the application service:

- immutable bootstrap is `Disabled`, `AutomaticLoopback`, or a nonzero
  `FixedLoopback` port; successful bind exposes the actual `127.0.0.1` port
  and fixed bind failure is typed;
- one joined session listener uses a five-entry backlog, bounds eight
  pre-handshake tasks, routes exact v1 info hashes through up to 1,024
  generation-fenced registrations, and admits peers under one session budget
  shared with outgoing connecting and established sockets;
- the ordinary connection default is 200 after descriptor-aware clamping,
  accepted incoming sockets have exactly ten connections of slack, and all
  loopback sockets consume those limits;
- eligible complete, published, desired-running path-backed torrents register
  at completion and application open, and unregister before lifecycle or
  storage-authority changes;
- every peer starts choked; one session coordinator grants at most eight
  upload slots, including one automatically derived optimistic slot, using
  pinned libtorrent's fixed-slot, 15/30-second, and complete-seed round-robin
  defaults;
- each unchoked peer may retain exactly 2,000 validated request descriptors;
  ten shared reads, the existing 40-handle storage pool, a 528,396-byte writer
  charge, and 64 writer descriptors independently bound work and memory;
- peer, torrent, and session accounting records only physical piece payload
  successfully written, and bounded snapshots expose exact totals and rates;
- pure, storage, socket, lifecycle, application-restart, and simultaneous
  two-RSTorrent/two-libtorrent evidence passes for single-file and cross-file
  content, with all four clients independently verifying 67,109,595 bytes and
  the seed recording the exact 268,438,380 uploaded payload bytes;
- one schema-version-9 atomic settings group now persists disabled,
  automatic-loopback, or fixed-loopback listener intent, 1--2,000 ordinary
  peer connections, and 0--50 upload slots; active/effective/bound state stays
  distinct from configured intent and applies on restart;
- the generated application contract and shared browser/Tauri Settings
  surface expose that group, the actual loopback port, descriptor-derived
  effective limit, restart requirement, and typed recoverable bind failure;
- incoming peer tasks now attach routed generations to the retained ordinary
  torrent peer owner and publish complete connection/upload observations;
- existing Peers/Swarm mapping projects the connected incoming generation,
  accepted non-connectable endpoint, identity and negotiated extensions,
  interest/choke/grant, queues, exact upload total/rate, compact flags, and
  exact post-cleanup removal;
- UDP tracker announces still carry provisional port `6881`, which is not
  derived from or guaranteed to match this listener;
- the IPv4 DHT has a real ephemeral UDP query socket, but RSTorrent does not
  use it as a peer listener or send `announce_peer`; and
- UPnP IGD, PCP, and NAT-PMP port mapping are absent.

The implementation adds cohesive `peer_io`, `upload`, `seed_content`,
`incoming`, and session `incoming_seeding` owners instead of extending the
download driver or detached diagnostic seed. One application-lifetime peer ID
is shared by outgoing handshakes, tracker announces, and the listener, so
self-connection rejection is local to an application rather than the
RSTorrent client fingerprint.

[`capability-readiness.md`](capability-readiness.md) remains authoritative for
priority and the `Now`/`Next` queue. Completing these bounded slices does not
promote settings, advertisement, or mapping work ahead of the currently
recorded work.

## Desired End State

The completed campaign should let one first-party in-process engine:

1. bind an explicitly configured or automatically selected peer port;
2. accept and route hostile incoming handshakes to eligible torrents;
3. serve only verified metadata and payload under bounded upload policy;
4. retain a supervised seeding owner after download completion and across
   supported restart paths;
5. advertise the actual usable port through trackers, DHT, and later local
   discovery mechanisms;
6. map that real port through supported gateway mechanisms when mapping is
   enabled and useful;
7. expose configured, bound, mapped, failed, and observed-incoming state
   without conflating them; and
8. stop advertising, accepting, uploading, and mapping through an observable
   joined shutdown path.

Local seeding, LAN reachability, mapped Internet reachability, and observed
incoming success are separate evidence levels. Passing an earlier level must
not be described as proof of a later one.

## Accepted Campaign Invariants

### A listener precedes reachability claims

An actual successful bind is the authority for the local peer port. Tracker,
DHT, extension-handshake, local-discovery, and mapping owners must consume
that observed state rather than a conventional constant or independently
configured copy.

No port is mapped or advertised as reachable before the corresponding
listener is accepting. If a fixed-port bind fails, the product reports that
failure; it does not silently choose another port unless the selected policy
explicitly permits automatic fallback.

A gateway may assign an external port different from the requested local
port. The advertised external port is therefore derived runtime state, not
assumed equality with the listener port and not durable configuration.

### One session owner, bounded torrent routing

The peer listener is shared session infrastructure rather than one listener
per torrent. One identifiable native owner binds it, supervises its accept
loop, bounds pre-handshake work, routes a validated v1 info hash to an
eligible torrent, and joins every intake task during shutdown.

The exact crate and module placement belongs to the first tactical's owner
map, but the hot path remains native and in-process. Platform adapters must
not become socket or piece-payload proxies.

Unknown hashes, inactive or removed torrents, stale registrations, duplicate
connections, and torrents without readable verified content are ordinary
bounded rejection paths. Listener state must not retain a torrent after its
application owner has completed removal or shutdown.

### Completion does not terminate seeding ownership accidentally

Verified download completion and task termination are not synonymous once
seeding exists. A completed torrent that remains eligible to seed needs a
supervised owner capable of reading its verified storage, observing policy,
and terminating on pause, archive, removal, application shutdown, or a
reached seeding goal.

That owner must use the ordinary torrent/session lifecycle. It must not be a
detached diagnostic server that independently opens artifacts and outlives
the application catalog.

### Only verified content is uploaded

Piece verification remains authoritative. Incoming peers may receive only
metadata authorized by the torrent identity and payload ranges backed by
currently verified have state. A bitfield or HAVE message must reflect the
pieces RSTorrent can actually read, including selective-storage cases; it
must not imply that skipped or absent pieces are available.

Every request is validated for message shape, piece index, offset, length,
overflow, verified availability, and negotiated connection state before
storage work is admitted. Cancel, disconnect, pause, recheck, hash-state
change, and removal must make queued reads and responses harmless.

### Unauthenticated intake and upload work are independently bounded

The campaign keeps separate limits for:

- accepted sockets waiting for a handshake;
- handshake bytes and completion time;
- established incoming connections;
- total session and per-torrent connections across both directions;
- queued peer requests and response bytes;
- storage reads in flight;
- peers currently allowed to receive payload;
- aggregate and per-torrent upload bandwidth; and
- diagnostic and recent-history retention.

An incoming connection limit is not an upload-slot limit, and an upload-slot
limit is not a bandwidth or seeding-goal policy. One slow reader or saturated
event consumer must not block listener intake, unrelated peer commands, or
joined shutdown indefinitely.

### Mapping is renewable soft state

UPnP IGD, PCP, and NAT-PMP mappings are volatile leases owned beneath one
session reachability coordinator. They are created only for real bound
protocol endpoints, renewed before expiry, replaced when the relevant
interface or port changes, and removed on clean shutdown when practical.
Crash recovery relies on bounded lease expiry and fresh discovery rather than
persisting a mapping as trusted state.

PCP and NAT-PMP share a gateway and transition relationship: a mature client
tries PCP and falls back to NAT-PMP when the gateway explicitly reports an
unsupported version. UPnP IGD is a separate discovery and SOAP/XML control
path. Supporting one path does not justify claiming the others.

The initial TCP campaign must not invent a `listener + 1` UDP convention.
RSTorrent's DHT currently owns an independently selected ephemeral UDP port,
and uTP is absent. UDP mapping waits for an actual UDP capability whose owner
and advertised-port semantics are defined.

### Configuration, actual state, and evidence remain distinct

User intent such as `Automatic` listening is not proof that a socket bound.
A bound listener is not proof that a gateway mapping succeeded. A successful
mapping response is not proof that an external peer connected. Product state
and diagnostics must preserve those distinctions.

Settings are added only with an enforcing owner. Presentation may follow the
semantic contract, but a UI control must not precede behavior or expose an
unenforced placeholder.

## Settings Vocabulary

The campaign should use explicit settings rather than one ambiguous
"seeding limit":

| Setting or policy | Meaning |
| --- | --- |
| Listener policy | `Disabled`, OS-selected `Automatic`, or `Fixed` local TCP port. |
| Pending-handshake limit | Maximum unauthenticated sockets admitted before torrent routing. |
| Incoming-connection limit | Maximum established inbound peers, coordinated with total connection budgets. |
| Upload-slot limit | Maximum peers currently allowed to receive requested payload. |
| Upload bandwidth limit | Aggregate and, if evidence requires it, per-torrent payload rate. |
| Seeding goal | Deliberate lifecycle policy such as until stopped, ratio target, or elapsed-time target. |
| Automatic mapping policy | Whether supported gateway mapping mechanisms may expose real bound endpoints. |

The first local slice may use conservative immutable configuration supplied
by its harness and product bootstrap. Persistence and mutation belong to a
later application-settings slice. Ratio and elapsed-time goals wait for exact
upload accounting and completed-torrent lifecycle; they are not approximated
from diagnostic counters.

Listener and reachability observation should at least distinguish:

- disabled;
- binding;
- listening with an actual local address and port;
- bind failed with bounded actionable context;
- mapping in progress;
- unmapped or mapping unavailable;
- mapped with mechanism, interface, external address, port, and lease age;
- mapping renewal failed while the local listener remains usable; and
- an incoming peer handshake actually observed.

These are runtime observations rather than a single durable status enum.

## Suggested Tactical Implementation Order

The entries below are prospective slices, not created tacticals. Before each
slice begins, its tactical must follow the source-first campaign contract,
settle its owner/task/cancellation map and resource bounds, and name one
falsifiable stopping condition.

### 1. [Local single-peer TCP seeding](../tactical/078-local-single-peer-tcp-seeding.md) — complete

Tactical `078` establishes the smallest real product path from a
session-owned TCP listener to verified upload:

- bind an explicit loopback or controlled local address and report the actual
  port;
- bound accept and pre-handshake intake, route the v1 info hash, and reject
  invalid or unavailable torrents;
- retain or start the supervised torrent upload owner needed after verified
  completion;
- support the minimum peer-wire and BEP 9 behavior needed to advertise
  available pieces and serve bounded verified requests; and
- join the listener, pending handshakes, upload reads, and peer tasks.

The controlled stopping condition passed: libtorrent `2.0.13` verified exact
payload after dialing the listener with its incoming side disabled, and an
RSTorrent magnet leecher acquired BEP 9 metadata plus complete content from
the restarted application seed using only `x.pe`. The fixtures cover ordinary
single-file data and multi-file reads crossing boundaries with a short final
piece. Scripted evidence covers silent and malformed handshakes, unknown and
self hashes/identities, saturation, invalid and excessive requests,
cancellation, lifecycle fences, and joined shutdown.

NAT mapping, public discovery, settings UI, uTP, incoming encryption,
multi-peer choking strategy, and ratio/time goals remain out of scope.

### 2. [Bounded multi-peer upload ownership and accounting](../tactical/082-bounded-multi-peer-upload-ownership.md) — complete

Grow from one useful incoming peer to a coherent bounded upload owner:

- coordinate inbound and outbound session/torrent connection budgets;
- admit, choke, unchoke, and rotate a bounded number of upload slots;
- bound queued requests, reads, and serialized responses per peer and across
  the torrent;
- add exact protocol and physical piece-payload upload accounting plus rates;
- prevent slow readers and storage latency from starving unrelated work; and
- retain prompt pause, completion-policy, removal, and shutdown joins.

Controlled simultaneous RSTorrent and libtorrent evidence now proves exact
content, declared high-water marks, and cleanup. Two clients of each kind
overlapped against one seed and independently verified the 67,109,595-byte
fixture; exact physical upload was four copies, or 268,438,380 bytes. Scripted
ten-peer evidence proves the eight-slot ceiling, one optimistic slot, fair
rotation, slow-writer isolation, cancellation fencing, and joined cleanup.
Downloading-torrent tit-for-tat and public performance tuning remain later.

### 3. Persisted client connection and seeding settings — complete

Tactical
[`084`](../tactical/084-persisted-client-connection-and-seeding-settings.md)
implements one typed, atomic settings group for loopback listener policy, the
ordinary session-wide peer ceiling, and piece-payload upload slots. It adopts
the pinned libtorrent defaults where existing owners have equivalent
semantics while preserving listener-disabled as RSTorrent's default.

The slice now owns validation, typed SQLite persistence, restart-applied
configured-versus-active semantics, startup enforcement, generated contracts,
the shared browser/Tauri Settings surface, and equivalent headless behavior.
Pending-handshake and incoming-slack tuning remain internal safety policy.
Finite bandwidth and ratio/time seeding goals wait for their own enforcing and
durability owners.

Controlled product evidence persists automatic/37/one through the production
web gateway, reopens onto an observed nonzero listener, and seeds an exact
2,097,152-byte payload to outbound-only libtorrent. A held fixed port reopens
as typed address-in-use, remains command-accessible, repairs to automatic, and
reopens successfully. Seeding high water remains within all declared
connection, slot, request, read, writer, and 40-handle limits; every joined
generation ends with zero incoming, gateway-connection, storage, cache, and
platform-request owners.

### 4. [Long-lived torrent peer runtime](../tactical/086-long-lived-torrent-peer-runtime.md) — in progress

Move ordinary peer state from the active download operation into one
application-generation per-torrent runtime, then use incoming integration as
the vertical proof:

- preserve the current one-active-download policy while making the peer
  registry, connection generations, IDs, and snapshot sink survive download
  completion;
- attach a routed incoming socket to that state after its info hash is known;
- project upload lifecycle, exact totals/rates, endpoints, identity,
  capabilities, choke/interest, and optimistic grant through the existing
  Peers model and compact flags;
- add one bounded non-connectable `incoming` Swarm observation for the remote
  ephemeral endpoint; and
- prove download-to-seed continuity, restart, lifecycle fencing, keyed row
  removal, and joined shutdown with RSTorrent and pinned libtorrent peers.

This slice does not add concurrent multi-torrent work, change any listener or
upload limit, implement mature duplicate-peer policy, or add advertisement,
mapping, protocol, settings, or UI breadth.

### 5. Truthful tracker and DHT reachability

Replace the provisional tracker port with state derived from the real
listener and any authoritative external mapping:

- tracker announces consume the current advertised peer port and react to
  listener changes without independent constants;
- DHT `announce_peer` begins only when the torrent and listener are eligible
  and the port claim is truthful;
- private-torrent gating remains exact;
- advertisement stops before listener shutdown or torrent ineligibility; and
- mapped external-port changes trigger bounded corrective announcements.

Controlled tracker and DHT peers should discover and complete from RSTorrent
without an explicit peer hint. BEP 10 listen-port advertisement and LSD may
be added here only if their full bounds and private-policy behavior remain a
coherent part of the slice; otherwise they stay separate.

### 6. Gateway mapping and reachability coordination

Add mapping only after listener and advertised-port ownership are proven:

- PCP MAP with nonce, lease, external endpoint, renewal, and PCP-to-NAT-PMP
  unsupported-version fallback;
- NAT-PMP external-address and TCP mapping behavior with bounded serial
  requests and lease recovery;
- UPnP IGD discovery, bounded device-description parsing, WAN IP/PPP service
  selection, external-address lookup, add/delete, lease renewal, and IGD v1/v2
  behavior selected from the pinned oracle audit;
- per-interface ownership, network-change replacement, explicit disable, and
  joined clean shutdown; and
- one reachability projection that can represent multiple mechanisms without
  treating duplicate successes as independent listener ports.

Deterministic codecs and state transitions precede scripted gateway servers.
Controlled LAN or namespace evidence precedes any opt-in physical-router or
public reachability smoke. Router mapping success alone is not an external
incoming-connectivity claim.

### 7. Product settings, status, and platform evidence

Expose the proven semantic settings and reachability states through the
appropriate product surfaces. Desktop/web and Android may present different
controls, but both consume the same application meaning.

The UI distinguishes configured port, actual local port, mapped external
endpoint, mapping mechanism and renewal health, and observed incoming
success. Platform work includes Android local-network permission and
foreground lifecycle, interface and VPN changes, desktop firewall guidance,
and accessibility. Physical-device or public-network evidence remains
explicitly authorized and accurately scoped.

## Cross-Topic Ownership

- [`peer-lifecycle.md`](peer-lifecycle.md) owns connection observations,
  records, direction, lifecycle, duplicate-peer treatment, and connection
  usefulness once incoming peers join the swarm.
- [`download-correctness.md`](download-correctness.md) owns verified-piece
  authority, hash failure, request validity, and storage/read correctness.
- [`protocol-support.md`](protocol-support.md) owns the exact peer-wire, BEP 9,
  tracker, DHT, LSD, uTP, and hole-punching support claims.
- [`tracker-discovery.md`](tracker-discovery.md) owns tracker announce state and
  how the current advertised port enters that protocol.
- [`dht-discovery.md`](dht-discovery.md) owns self-announcement, token, private
  gating, and DHT participation once a peer port is usable.
- [`client-persistence.md`](client-persistence.md) owns durable settings,
  completed-torrent restart, counters needed for seeding goals, storage-root
  identity, and recheck authority.
- [`application-control.md`](application-control.md) owns semantic setting and
  lifecycle operations above the engine.
- [`client-surfaces.md`](client-surfaces.md) owns desktop/web and Android
  presentation and platform-specific adaptation.
- [`capability-readiness.md`](capability-readiness.md) owns campaign priority,
  current implementation state, and support-evidence roll-up.

This topic owns the dependency order and end-to-end reachability story across
those boundaries. A tactical updates every focused topic whose truth it
materially changes rather than moving all detail into this document.

## Reference And Source-First Requirements

Each implementing tactical must inspect the exact libtorrent revision pinned
in [`../../reference/pins.toml`](../../reference/pins.toml), including tests,
before finalizing the relevant state shape. The campaign is expected to draw
from, at minimum:

- libtorrent listen-socket, incoming-peer, upload, choking, port mapping,
  tracker-port, DHT announcement, settings, alert, and shutdown paths;
- `reference/libtorrent/src/upnp.cpp`, `src/natpmp.cpp`, and their exact tests
  when mapping enters scope; and
- the local JSTorrent listener, incoming routing, seeding, settings, UPnP,
  PCP, and NAT-PMP paths for first-party product and platform history.

The tactical records exact source and test paths, edge cases adopted,
intentional differences, and license/provenance decisions. Reference code is
a completeness oracle, not an architecture template or source donor.

Normative protocol reading must match the slice: the BitTorrent peer-wire and
extension specifications for upload, tracker and DHT specifications for
advertisement, and the applicable UPnP IGD, PCP, and NAT-PMP specifications
for mapping. No support claim comes from source inspection alone.

## Validation Ladder

Evidence graduates in layers:

1. **Deterministic state and codec tests:** listener policy, handshake routing,
   request validation, verified availability, upload admission, accounting,
   mapping leases, advertisement selection, and stale-event rejection.
2. **Scripted runtime failures:** bind conflicts, silent and malformed peers,
   limit saturation, slow readers, storage delays, cancellation at every
   owner boundary, interface replacement, gateway loss, renewal failure, and
   exact socket/task cleanup.
3. **Controlled interoperability:** RSTorrent-to-RSTorrent and
   RSTorrent-to-libtorrent metadata and payload seeding, then controlled
   tracker/DHT discovery and simulated mapping gateways.
4. **Product and platform evidence:** headless shared web behavior, desktop
   lifecycle, Android AVD behavior, and authorized physical-device checks
   when the slice changes those paths.
5. **Optional representative evidence:** controlled LAN/router and public
   incoming observations under the opt-in live-evidence policy.

Every layer records the exact listener address, advertised endpoint, mapping
mechanism, peer direction, transferred and verified bytes, resource
high-water marks, terminal owner counts, and what the evidence does not prove.

## Open Decisions

After completed Tactical `084`, the campaign direction does not yet settle:

- whether automatic port fallback is ever allowed after a fixed bind fails;
- whether the temporary rule that a desired-running complete torrent seeds
  automatically should become a distinct durable seeding intent;
- whether a later slice should make listener, connection-limit, or upload-slot
  changes live, including the required rebind, eviction, and regrant behavior;
- when ratio and elapsed-time goals become product settings;
- how to choose among multiple interfaces or successful external mappings;
- how VPN, metered networks, Android background lifecycle, and local-network
  permission affect listening and mapping;
- the eventual relationship among the DHT UDP port, future uTP, and UDP
  mapping; and
- when incoming MSE/PE, IPv6 firewall pinholes, LSD, PEX, or BEP 55 become
  independently justified tacticals.

Tactical `084` resolves the initial default, bounds, persistence authority,
restart semantics, and corrupt-row behavior. External-port and
multi-interface policy remain focused later slices.

## Campaign Checkpoint And Next Action

Tacticals
[`082`](../tactical/082-bounded-multi-peer-upload-ownership.md) and
[`084`](../tactical/084-persisted-client-connection-and-seeding-settings.md)
now complete bounded multi-peer upload ownership and the restart-applied
product settings boundary. Planned Tactical
[`086`](../tactical/086-long-lived-torrent-peer-runtime.md) is the next coherent
local slice: it establishes one long-lived per-torrent peer owner and proves it
through truthful incoming projection in the ordinary Swarm/Peers model.
Actual-port tracker/DHT advertisement and gateway mapping remain later
independent slices.
