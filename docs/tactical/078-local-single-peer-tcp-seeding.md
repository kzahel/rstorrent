# Tactical 078: Local Single-Peer TCP Seeding

Status: Planned from maintainer direction on 2026-08-04. Implementation has
not started, and this tactical does not displace Tactical `075` from the
authoritative `Now` queue.

Topics: `incoming-reachability-and-seeding`, `peer-lifecycle`,
`client-persistence`, `protocol-support`, `capability-readiness`

## Decision And Motivation

Establish the first real incoming BitTorrent path as one bounded vertical
slice: an application-owned, engine-implemented loopback TCP listener routes a
validated v1 handshake to an eligible torrent, serves verified BEP 9 metadata
and payload from published path-backed storage, survives download-task
completion and application restart, and shuts down through joined ownership.

The slice deliberately supports one established incoming peer and one upload
slot. Its registry and lifecycle are multi-torrent-shaped so the next tactical
can add bounded simultaneous uploads without replacing the listener, routing,
storage-read, or cancellation model.

This is not implemented in the existing diagnostic metadata seed. That tool is
loopback-only, one-shot, metadata-only, and independently owns a metainfo path.
The product listener instead follows application catalog authority and
ordinary torrent lifecycle. It also does not extend the download driver after
completion: seeding receives a small read-only owner over verified published
content.

Targeted module extraction is part of the slice because the current outbound
peer connection embeds `DialAttempt`, the metadata seed has a second framed
socket loop, the download driver is already a large mixed owner, and selective
storage has no upload-facing block-read contract. File size alone does not
justify a crate. Protocol values remain in `rstorrent-protocol`, socket and
storage execution remain in `rstorrent-engine`, and application eligibility
remains in `rstorrent-session`.

## Desired Outcome And Stopping Condition

The tactical stops when all of the following are true:

- immutable application bootstrap can disable incoming TCP, bind an
  OS-selected IPv4 loopback port, or require one fixed IPv4 loopback port;
- a successful bind reports the actual local address and port, while a fixed
  bind failure is typed and never silently falls back;
- one session-owned accept loop bounds pre-handshake sockets, reads exactly one
  ordinary v1 handshake, and routes its info hash through a generation-fenced
  torrent registry;
- an eligible completed torrent remains registered after its download task
  ends and is registered again after a supported application restart;
- initial availability advertises only pieces that are both durably verified
  and completely readable from the retained published-content plan;
- one interested peer can be unchoked, pipeline bounded 16 KiB requests, cancel
  them, and receive exact metadata and payload bytes;
- pause, archive, force recheck, repair, removal, registration replacement, and
  application shutdown stop admission and make queued or late reads harmless
  before content authority changes;
- pure, storage, loopback-runtime, lifecycle, saturation, and exact-cleanup
  tests pass with the declared resource high-water marks;
- a libtorrent 2.0.13 leecher connects to the ordinary RSTorrent listener and
  hash-verifies a complete payload without a reverse RSTorrent dial;
- an RSTorrent magnet leecher using the listener as an `x.pe` peer acquires
  verified BEP 9 metadata and the complete payload from an RSTorrent seed; and
- the tactical, owning topic, readiness matrix, peer-lifecycle truth, and
  protocol-support claims record only the evidence that actually passed.

The interoperability fixtures must include an ordinary single-file torrent
and a multi-file torrent with a cross-file request boundary and a shortened
final piece. At least one restart run must seed from durable complete state
rather than from a still-running download owner.

## Stable Scenarios

### Listener and routing

- `Disabled` creates no socket or accept task.
- `AutomaticLoopback` binds `127.0.0.1:0` and exposes the kernel-selected port.
- `FixedLoopback(port)` accepts only a nonzero port and reports a bind conflict
  without choosing another port.
- Non-loopback, unspecified, multicast, and broadcast addresses are not
  expressible through the first bootstrap contract.
- A silent socket, partial handshake, invalid protocol string, mismatched or
  unknown info hash, self peer ID, unavailable torrent, stale registration,
  and saturated limit all terminate through bounded rejection paths.
- The accept loop is rearmed independently of a peer's handshake or storage
  progress. A slow or malicious peer cannot hold the listener owner itself.

### Eligibility and application lifecycle

A torrent is seed-eligible in this slice only when all of these are true:

- durable state is `Complete` with `StorageState::Published`;
- `desired_running` remains true, the torrent is not archived, no removal is
  pending, and no force recheck, repair, or publication transition is active;
- verified raw info reparses under the durable metainfo limits and hashes to
  the catalog identity;
- durable have state has the exact metainfo piece count; and
- the storage root is a currently available path-backed root from which a
  conservative read-only content plan can be opened.

Registration is not a new durable seeding-intent field. For this slice,
existing desired-running intent means a complete, unarchived torrent seeds
while the application is running. Pause and archive unregister it; restore or
start may register it again after eligibility is re-established. A future
settings tactical may replace this temporary policy with explicit seeding
goals or intent without changing listener ownership.

Completion and task reaping reconcile the newly complete torrent into the
registry before reporting seeding readiness. Application open reconciles
eligible durable complete torrents, not only the one download selected for
active work, up to the declared registration cap and diagnoses any remainder.
Unregistration invalidates the exact registration generation, cancels its
peer and read work, and joins that work before recheck, relocation, or
managed-data deletion begins.

Platform-capability and descriptor-backed roots are not registered in this
slice. They remain truthful but unavailable for incoming payload upload until
their read-handle acquisition and platform lifecycle receive separate
evidence.

### Verified availability and reads

The seed-content plan intersects durable have state with readable storage
routing. A piece is advertised only when every non-padding byte in that piece
has a retained readable source. Padding spans synthesize zero bytes. A piece
touching an unavailable skipped-file or part-file source is masked out in its
entirety even if durable have state says it was verified earlier.

The initial peer message after the handshake is the exact BEP 3 bitfield;
spare bits are zero. Dynamic `HAVE` publication is unnecessary because this
slice unregisters during every verification or selection transition and does
not upload from an actively downloading torrent.

Every payload request is checked, with overflow-safe arithmetic, for:

- the exact 13-byte wire shape already enforced by the decoder;
- a positive length no greater than 16 KiB;
- a valid piece index;
- `begin < piece_length_at(index)` and `begin + length` within that piece;
- currently advertised verified availability;
- interested and unchoked connection state; and
- remaining request-count and requested-byte capacity.

Malformed or oversized frames are terminal. Semantically invalid ranges or
requests that exceed a declared queue bound close the peer because this slice
does not negotiate the FAST extension's reject-request message. Requests sent
while choked or before interest are ignored without storage work. Exact
duplicate descriptors consume the same queue budget as any other request.
`Cancel` removes every matching queued request and suppresses a matching
in-flight response; a read may finish physically, but its registration and
request generations must still be current before serialization.

Read failure, short read, missing file, or storage-generation mismatch sends no
partial block. It closes the connection, marks the seed registration
unhealthy, removes its advertised availability, and leaves durable have state
unchanged until the application explicitly rechecks.

### Metadata and extension behavior

An incoming peer advertising BEP 10 receives the ordinary extension handshake
and the existing directional `ut_metadata` implementation. The metadata bytes
are the exact verified raw info dictionary associated with the routed info
hash. The existing 16 KiB block geometry, 256-request flood limit, invalid
piece rejection, remote extension-ID direction, and unknown-extension ignore
behavior remain authoritative.

Serving all metadata blocks does not close the product connection; payload
exchange may follow on the same socket. A peer without extension support can
still receive the bitfield and payload. PEX, FAST, upload-only, hole-punch,
encryption, and listen-port extension fields are not added here.

## Normative And Reference Dossier

No reference source, test, fixture, class graph, or persistence format is
copied.

### Normative behavior

- BEP 3 at `reference/bittorrent.org/beps/bep_0003.rst` defines the incoming
  info-hash-first multiplexing exception, initially choked/not-interested
  state, first-message bitfield and zero spare bits, 16 KiB request practice,
  request/cancel geometry, and implicit request-to-piece correlation.
- BEP 9 at `reference/bittorrent.org/beps/bep_0009.rst` requires complete
  info-hash-verified metadata before upload, 16 KiB metadata blocks, matching
  request/data/reject piece numbers, invalid-piece rejection, and bounded flood
  protection.
- BEP 10 at `reference/bittorrent.org/beps/bep_0010.rst` defines the reserved
  extension bit, message ID 20, connection-local directional extension IDs,
  optional handshake fields, and ignore behavior for unknown names.

### Pinned libtorrent oracle

The required checkout is `reference/libtorrent` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` (`v2.0.13`). Relevant owners and
cases inspected while drafting this tactical are:

- `src/session_impl.cpp::{async_accept,on_accept_connection,
  incoming_connection}` immediately rearms the accept loop, distinguishes
  terminal shutdown from recoverable accept failure, rejects paused/no-torrent
  and over-limit intake before attaching it to a torrent, and owns accepted
  connections at session scope.
- `src/peer_connection.cpp::attach_to_torrent` resolves the handshake info hash
  after intake, rejects unknown, aborted, paused, duplicate, or over-limit
  torrents, and assigns torrent membership only after attachment succeeds.
- `src/bt_peer_connection.cpp::{on_request,on_cancel,write_piece}` validates
  exact message shape and serializes block identity separately from payload.
- `src/peer_connection.cpp::{incoming_request,incoming_cancel,
  fill_send_buffer,on_disk_read_complete}` bounds request admission, validates
  piece ownership and geometry, gates disk reads behind send-buffer capacity,
  separates queued descriptors from asynchronous read bytes, and rechecks
  torrent/connection state before sending.
- `include/libtorrent/torrent.hpp::{have_piece,user_have_piece}` and the seed
  verification path keep advertised ownership tied to verified piece state.
- `test/test_fast_extension.cpp::{invalid_metadata_request,invalid_request,
  incoming_have_all}` exercises invalid metadata and payload requests plus
  incoming seed availability; RSTorrent supplies an ordinary BEP 3 bitfield
  rather than requiring FAST `have_all`.
- `test/test_peer_list.cpp::{incoming_size_limit,double_connection_*}` covers
  incoming admission at a full peer list and simultaneous incoming/outgoing
  duplicate resolution. The first RSTorrent slice closes the extra incoming
  connection rather than displacing an outgoing peer.
- `test/test_listen_socket.cpp` covers endpoint expansion, port replacement,
  device/interface changes, and socket partitioning. Only exact IPv4 loopback
  bind and actual-port observation are adopted here; interface expansion is a
  later reachability concern.
- `test/bittorrent_peer.cpp`, `test/peer_server.{hpp,cpp}`, and
  `test/test_transfer.cpp` provide the independent handshake/message and
  transfer harness patterns used to choose adversarial scenarios, not source
  or fixtures for RSTorrent tests.

Libtorrent's defaults of 2,000 queued incoming requests, roughly 500 KiB send
watermark, eight unchoke slots, and connection slack are mature multi-peer
policy, not appropriate first-slice defaults. RSTorrent starts substantially
smaller. Libtorrent also records that a cancel can arrive after a queued request
has become a disk job; RSTorrent makes that race explicit through request
generation instead of allowing a late response silently.

### JSTorrent behavior and failures

The local first-party reference is `../jstorrent` on `main`, inspected at
`9895410beeed6aff554053769bd006a3fbd373ef`. Relevant paths inspected while
drafting this tactical are:

- `packages/engine/src/core/bt-engine.ts::{startServer,
  handleIncomingConnection,destroy}` observes the actual port selected from a
  port-zero bind, applies global and pending limits, times out pre-handshake
  peers, routes by info hash, and distinguishes observed incoming success.
- The same owner applies its pending limit only after optional MSE work and
  explicitly notes that it did not retain the server required to close and
  join it. RSTorrent bounds intake before protocol work and retains the
  listener as a joined owner from its first implementation.
- `packages/engine/src/core/torrent.ts::addPeer` coordinates incoming and total
  peer limits and rejects duplicate or inactive work. Its multi-peer ratio and
  replacement behavior remain later policy.
- `packages/engine/src/core/torrent-uploader.ts` separates queued request
  descriptors, in-flight read bytes, send-buffer watermark, connection
  revalidation, and content reads. Its 500-request queue, 512 KiB per-peer
  watermark, fire-and-forget reads, and lack of a wired peer-cancel path are
  not adopted.
- `packages/engine/test/core/torrent-uploader.test.ts` and
  `torrent-connection-limits.test.ts` identify queue, watermark, read failure,
  disconnect, choke, incoming-ratio, and upload-slot cases to reproduce with
  independently authored Rust state and loopback tests.
- `packages/engine/integration/python/test_seeding.py` demonstrates the key
  first-party product scenario: recheck existing content, obtain the actual
  listener port, let libtorrent connect inbound, and verify the downloaded
  file hash. RSTorrent's test must not add the reverse seeder-to-leecher dial
  used there as a robustness fallback.

RSTorrent does not adopt JSTorrent's QuickJS/native-server adapters, IO daemon,
MSE path, configuration schema, tick loop, TypeScript content-reader
interface, or persistence model.

## Ownership, Tasks, Cancellation, And Data Flow

```text
ApplicationService
  -> immutable IncomingTcpBootstrap
  -> IncomingPeerService owner (defined in rstorrent-engine)
       -> retained TcpListener + actual bound-address observation
       -> accept task
            -> at most 8 pre-handshake tasks
                 -> v1 info-hash lookup in registration registry
                 -> established permit (global maximum 1)
                 -> IncomingPeer task
                      -> runtime-independent UploadPeerState
                      -> MetadataUpload
                      -> at most 1 SeedContent read task
                      -> direction-neutral framed PeerIo
  -> SeedRegistration generation per eligible torrent
       -> exact raw info + metainfo/layout
       -> immutable verified/readable availability snapshot
       -> read-only SeedContent owner
       -> child cancellation for routed peers and reads
```

The engine service owns TCP, framing, request state, read execution, and task
joins. The application owns which durable torrent is eligible and the order in
which registration is removed relative to lifecycle mutations. A registration
handle or explicit generation token makes removal observable; dropping an
unjoined detached server is not a valid API.

Shutdown order is:

1. stop new registration changes and close the listener;
2. cancel and join pre-handshake tasks;
3. invalidate registrations and cancel routed peer/read tasks;
4. join every peer and read task, releasing file leases and permits;
5. continue existing DHT, speed-history, active-download, store, and view
   shutdown; and
6. publish terminal listener/task observations.

Torrent removal, force recheck, archive, and pause perform steps 3 and 4 for
that registration before their existing storage or durable-state operation.
Service shutdown performs them for every registration.

## Module And Dependency Direction

The expected cohesive boundaries are:

- an engine `peer_io` extraction for direction-neutral framed reads, writes,
  deadlines, decoder queues, and byte metrics;
- an engine `incoming` module for bind policy, observation, registry,
  accept/handshake tasks, permits, and service shutdown;
- a runtime-independent engine `upload` module for connection state, request
  admission, cancellation, generations, and explicit send/read/close actions;
- an engine `seed_content` module for immutable readable-availability planning
  and bounded positional reads from published path storage; and
- a session `incoming_seeding` module for durable eligibility, registration
  reconciliation, and lifecycle ordering.

Exact internal names may follow local conventions. Do not put listener,
storage, task, or application state in `rstorrent-protocol`; metainfo-aware
upload state may depend inward on existing protocol values while remaining
free of Tokio. Do not add a new crate, generic storage framework, speculative
trait hierarchy, one-file-per-type layout, or a second session/application
service.

`peer_socket.rs` retains outbound dial and connection-set ownership. The
direction-neutral extraction must keep its existing duplex backpressure,
deadline, metric, generation, saturation, and cleanup tests green.
`metadata_seed.rs` may adopt the shared framed I/O when that removes concrete
duplication, but its public one-shot diagnostic semantics remain unchanged.
Do not wholesale split `driver.rs`, `selective_storage.rs`, or
`application.rs`; new incoming responsibilities simply must not be added to
those existing mixed files when the coherent module above owns them.

## Initial Resource And Protocol Bounds

| Resource | Initial bound |
| --- | ---: |
| Listener endpoints | One IPv4 loopback TCP socket |
| Active seed registrations | 1,024 unique v1 info hashes |
| Pending pre-handshake sockets/tasks | 8 session-wide |
| Handshake bytes | Exactly 68 |
| Handshake completion | 10 seconds from accept |
| Established incoming connections | 1 session-wide and therefore 1 per torrent |
| Upload slots | 1 |
| Queued payload requests | 32 descriptors |
| Queued requested payload | 512 KiB |
| Payload reads in flight | 1 |
| Payload bytes in flight/resident for upload | One 16 KiB block |
| Open file leases held by upload reads | 1 at a time |
| Metadata block bytes resident | One 16 KiB block |
| Metadata requests per connection | Existing maximum 256 |
| Peer decoder input/frame bounds | Existing `peer_wire` constants |
| Peer command/event queues | Existing 16/64 bounds where reused |
| Retained recent rejection detail | Counts by typed reason plus at most 32 bounded recent records |

The implementation may tighten these values when pure or reference evidence
shows a smaller value is sufficient. Increasing them or adding a second
established peer belongs to the next tactical. Registration beyond the cap
fails with a typed diagnostic and does not alter the durable catalog.
Registration retains storage references rather than open file leases; a
cross-file read acquires and releases its sources sequentially. Request
descriptors use checked fixed-size state and never retain caller-controlled
payload. The connection task owns serialization so no channel contains an
unbounded or multiply buffered sequence of 16 KiB blocks.

Observations record configured mode, actual bind address, bind failure class,
pending and established current/high-water counts, routed info hash only in
bounded diagnostic form, rejection counts, queued request/byte high water,
active read/byte high water, protocol and payload bytes sent, cancellation
reason, and terminal owner/task counts. An actual loopback handshake is not
reported as externally reachable Internet evidence.

## Shape-Changing Edge Cases

The common path must land together with:

- fixed-port bind conflict and port-zero actual-address observation;
- accept errors that do not turn shutdown into an accept retry loop;
- partial/fragmented/coalesced handshakes and frames under one absolute
  operation deadline rather than a refreshable per-fragment timeout;
- unknown, stale, inactive, removed, archived, rechecking, repaired, or
  unreadable torrent routing;
- registration replacement while a handshake or read is in flight;
- exact bitfield size and trailing zero bits for non-byte-aligned piece counts;
- single-file, multi-file, cross-file, padding, shortened-final-piece, skipped,
  absent, truncated, and short-read storage paths;
- arithmetic overflow and every request boundary at zero, maximum, and
  maximum plus one;
- duplicate request descriptors, exact cancel, cancel-after-read-admission,
  disconnect during read, and read completion after cancel;
- interested/not-interested and choke/unchoke transitions with queue cleanup;
- a slow reader and saturated peer event consumer without accept-loop or
  shutdown starvation;
- simultaneous inbound and outbound generations for the same endpoint/peer
  identity, with the extra incoming connection conservatively closed; and
- pause, archive, force recheck, removal, restart, and application shutdown at
  each owner boundary with exact joins.

## Implementation Sequence And Intermediate Gates

1. **Pure state and low-level I/O gate.** Extract only the direction-neutral
   framed peer seam needed by both directions. Add upload state, request
   generations, routing generations, bitfield construction, and table-driven
   boundary tests. Existing outbound and diagnostic metadata tests remain
   green.
2. **Read-only storage gate.** Build a conservative seed-content plan from
   metainfo, durable have state, selection, and published path layout. Prove
   exact reads and masked availability for the full storage matrix without a
   socket.
3. **Engine listener gate.** Bind loopback, accept under permits, route the
   v1 handshake, serve metadata/payload to one scripted peer, and prove
   saturation, timeout, slow-reader, cancellation, and task cleanup.
4. **Application lifecycle gate.** Add immutable bootstrap plus eligibility
   reconciliation after completion and restart. Fence pause/archive/recheck/
   repair/removal before storage authority changes and expose actual listener
   observations to headless application tests.
5. **Controlled interoperability gate.** Add one isolated
   `tests/interop/incoming_seeding.py`-style harness using the already locked
   Python libtorrent 2.0.13 dependency. Run libtorrent inbound-only leeching
   and RSTorrent magnet leeching, verify exact payload hashes, record resource
   high water, terminate both owners, and remove all temporary profiles and
   payloads.
6. **Truth and handoff gate.** Run workspace validation and update the tactical
   and all owning topics with landed modules, exact bounds, evidence, known
   gaps, and the next multi-peer boundary.

Each gate must pass before the next adds a longer-lived owner. A successful
handshake without verified content transfer is not completion evidence.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure protocol/state | Known/unknown/stale routing; exact bitfield and spare bits; request geometry and overflow; choke/interest; queue/byte bounds; duplicate/cancel/late-read generations; metadata directional IDs and flood limit. |
| Storage component | Real temporary single/multi-file published layouts; cross-file and final-piece reads; zero padding; skipped/unreadable masking; missing/truncated/short-read failure; no partial response. |
| Scripted Tokio runtime | Disabled/automatic/fixed bind; actual port; conflict; silent/partial/malformed handshakes; pending and established saturation; fragmented/coalesced frames; exact payload; slow reader; mid-read disconnect; absolute deadlines; joined shutdown. |
| Application lifecycle | Completion retains seeding ownership after the download task; restart re-registers durable complete content; pause/archive/recheck/repair/removal unregister and join first; application shutdown leaves zero listener, peer, read, and file-lease owners. |
| RSTorrent interoperability | A normal magnet download with only the seed's `x.pe` obtains metadata and publishes the hash-verified fixture over the incoming path. |
| Libtorrent interoperability | Libtorrent 2.0.13 connects to the reported RSTorrent port, receives metadata/availability as applicable, downloads without a reverse RSTorrent dial, becomes a seed, and matches independent file hashes. |
| Resource evidence | Current and high-water pending sockets, established peers, queued descriptors/bytes, read jobs/bytes, serialized payload bytes, file leases, and terminal task counts equal the declared limits and return to zero. |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, focused interop commands, and `git diff --check`. |

Tests should use barriers, channels, explicit queue saturation, and absolute
deadlines rather than correctness-sensitive sleeps. Pure tests stay adjacent
to runtime-independent modules; private engine component tests may remain
module tests; public end-to-end behavior goes through the application service
and controlled interop harness. No test-only public network API is introduced
solely to access internals.

## Non-Goals

- More than one established incoming peer, multi-peer upload fairness,
  optimistic unchoking, tit-for-tat, rotation, or peer replacement.
- Upload bandwidth limiting, ratio/time goals, exact durable seeding counters,
  or mutable/persisted seeding policy.
- Tracker or DHT advertisement of the listener port, DHT `announce_peer`, PEX,
  local service discovery, or reachability inference.
- UPnP IGD, PCP, NAT-PMP, firewall pinholes, interface selection, LAN/public
  binding, VPN policy, or mapped external addresses.
- uTP, incoming MSE/PE, FAST, BEP 21 upload-only, BEP 55 hole punching, v2,
  hybrid torrents, or web seeds.
- Upload while a torrent is downloading or checking, predictive pieces,
  dynamic `HAVE`, relocation, or serving an unreadable skipped/part-file piece.
- Platform-capability/descriptor reads, Android/AVD/physical evidence, Tauri
  UI, shared web UI, settings UI, generated contract changes, or schema
  migration.
- A native host, REST/WebSocket payload proxy, remote daemon, separate I/O
  service, or libtorrent runtime dependency.
- Public-swarm, router, or Internet-reachability evidence.

## Escalation And Next Boundary

Once the readiness queue promotes this tactical, implementation may choose
internal names, perform the bounded module extraction above, tighten limits,
add adversarial cases implied by the invariants, and fix bugs at the same
listener/upload/read ownership boundary without further direction.

Stop for maintainer direction if evidence requires a new crate or dependency,
a default non-loopback/product listener, more than one established peer,
durable schema or generated-contract changes, a new seeding-intent policy,
platform-capability upload, active-download upload, tracker/DHT advertisement,
NAT mapping, MSE/uTP/FAST, destructive fixture handling, public network access,
or visible/device work.

The next tactical boundary is bounded multi-peer upload ownership and
accounting: coordinated incoming/total connection budgets, multiple upload
slots, queued/read/response fairness, exact useful-upload counters, slow-reader
isolation, and simultaneous RSTorrent/libtorrent leecher evidence. Listener
settings persistence follows only after that owner exists.
