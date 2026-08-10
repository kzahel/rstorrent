# Tactical 124: Duplex Verified-Piece Upload During Download

Status: In progress on 2026-08-10 after the maintainer authorized autonomous
end-to-end implementation and bounded commits. The first deterministic slice
now implements the compact availability epoch/revision/timeline authority,
dynamic upload-request revalidation, exact ordinary/Fast initial forms, and
generation-stamped active selective-storage read plans across staging,
retained, part-file, cross-file, and padding routes. Outgoing and accepted TCP
content sockets now both carry duplex payload through the same swarm and
upload owners. Tracker/DHT advertisement now follows actual active-route
routability independently from completion. Lifecycle/adversarial,
interoperability, and Android gates remain in progress. This
very-high-priority correctness repair ranks ahead of finite bandwidth controls
and seeding-goal policy.

Topics: `incoming-reachability-and-seeding`, `peer-lifecycle`,
`download-correctness`, `protocol-support`,
`storage-throughput-architecture`, `android-saf-storage`,
`client-persistence`, `code-organization-and-refactoring`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tacticals
[`052`](052-batched-durability-checkpoints.md),
[`073`](073-unified-storage-and-complete-recheck.md),
[`078`](078-local-single-peer-tcp-seeding.md),
[`082`](082-bounded-multi-peer-upload-ownership.md),
[`086`](086-long-lived-torrent-peer-runtime.md),
[`089`](089-coordinated-session-listen-sockets.md),
[`090`](090-peer-id-duplicate-connection-resolution.md),
[`092`](092-truthful-tracker-and-dht-peer-advertisement.md),
[`093`](093-bep6-fast-request-lifecycle.md),
[`097`](097-live-client-settings-and-replaceable-session-generations.md),
[`105`](105-fact-based-persistence-and-recheck-containment.md),
[`108`](108-serialized-torrent-control-and-observable-checking.md),
[`114`](114-session-wide-concurrent-torrent-admission.md), and
[`116`](116-platform-storage-coherence-and-ios-feasibility.md) establish the
integrity, storage, upload, peer, discovery, lifecycle, and platform owners
this slice must join. Tactical `120` is not a dependency: conservative recheck
remains sufficient to restore partial have authority. uTP Tacticals `118`
through `121` and performance Tactical `122` are also not dependencies.

## Decision And Priority

An RSTorrent peer with verified pieces is a peer, not a download-only client.
Once v1 metadata is verified and content networking is admitted, every
established BitTorrent connection is duplex regardless of which side opened
the TCP socket or whether the torrent as a whole is complete. RSTorrent will
announce and upload each piece as soon as the complete piece hash passes and
all writes in that piece generation have succeeded, provided the bytes remain
readable through the current storage-routing generation.

Torrent completion and publication remain lifecycle facts. They no longer
define whether a piece may be uploaded. Upload authority is the conjunction:

```text
uploadable(piece, epoch) =
    hash_verified(piece, epoch)
    and complete_generation_writes_succeeded(piece, epoch)
    and every real byte is readable through the current storage route
```

`checkpoint_dirty` does not make an otherwise established piece unavailable
inside the running process. A crash may lose that recent resume claim and
cause recheck or redownload, but durability and current-process hash authority
remain distinct as established by Tactical `052`.

The existing session-wide upload slots, request validation, Fast terminals,
read admission, storage-file pool, writer bounds, accounting, and peer
observations remain the upload mechanism. This tactical must not create a
second active-download uploader or an unbounded shortcut around those owners.
It extends the established choker comparison with the pinned libtorrent
reciprocity input: interested peers that supplied more recent physical payload
are preferred for ordinary slots, while peers with no download contribution
retain the completed-seed round-robin fallback and the existing optimistic
slot.

## Current Defect

The present code has the wrong whole-torrent gate in four connected places:

- `crates/rstorrent-engine/src/driver.rs` ignores `Request` and `Cancel` in
  the active content message path;
- an outgoing active content connection sends Fast `HaveNone` or omits the
  ordinary bitfield even when resumed or newly verified pieces exist;
- `crates/rstorrent-session/src/incoming_seeding.rs::eligibility_reason`
  rejects any active download and requires durable `Complete` plus
  `Published` state before installing an incoming route; and
- `SeedContent` is constructed only for the published namespace, so the
  otherwise reusable upload runtime cannot read verified staging and part-file
  routes during download.

Consequently RSTorrent currently downloads over connections that are only
half-used, reports no useful availability until the completion handoff, cannot
accept a useful incoming content peer for an incomplete torrent, advertises
the outbound-only tracker port and performs no DHT peer announcement for that
torrent, and contributes nothing to rare-piece propagation or reciprocity.
Selective completion softens the wording—`Complete` can mean all selected
required work rather than every metainfo piece—but it does not provide
incremental upload while content work is active.

This is a correctness and interoperability defect, not merely a missing
seeding preference. Bandwidth limits and ratio/time goals cannot repair it and
therefore follow rather than precede this slice.

## Desired Outcome And Stopping Condition

The tactical stops only when all of the following are true:

1. One task-free per-torrent availability authority represents exact
   hash-verified and currently readable v1 pieces under a storage epoch. The
   download picker, wire bitfield, HAVE publication, request validation,
   active upload reader, Peers/Pieces projection, and completion handoff do not
   maintain contradictory have sets.
2. Every outgoing or incoming content connection receives exactly one legal
   initial availability message derived from the current authority: Fast
   `HaveAll`, Fast `HaveNone`, a correctly padded bitfield, or the BEP 3
   no-message form for a non-Fast peer with no pieces.
3. A newly established piece is published to eligible live connection
   generations only after its complete hash/write join. A peer already known
   to have that piece may suppress the redundant HAVE; no connection is told
   about unverified, stale, absent, or unreadable bytes.
4. Active outgoing content peers process interest, request, cancel, choke,
   unchoke, Fast allowed-request, and terminal response behavior while they
   continue supplying download payload. Upload backpressure cannot block
   peer-message intake, download scheduling, storage completions, or cleanup.
5. Once verified v1 metadata has entered an admitted content generation, the
   application listener routes eligible incoming sockets to the same torrent
   peer and content owners. An incoming connection may both request local
   verified pieces and supply missing pieces; it is not confined to the
   completed-seed loop.
6. Active path and supported local SAF storage can plan and execute bounded
   upload reads across wanted staging files, retained skipped-file sources,
   part-file slots, cross-file blocks, and synthetic padding without copying
   payload through Kotlin or another platform adapter.
7. Every request revalidates connection generation, Fast/choke state,
   geometry, availability, storage epoch, and readable plan before admission,
   before the read, and before the first response byte. Existing Tactical
   `093` exactly-one-terminal behavior remains true.
8. Piece hash failure, route replacement, file-selection reconciliation,
   failed observation/open/read, root loss, pause, force recheck, repair,
   archive, removal, and shutdown cannot expose stale bytes. If previously
   announced availability must be withdrawn and no negotiated wire message
   can express that safely, affected connection generations close and later
   peers receive a fresh initial snapshot.
9. Session choking retains eight total upload slots including one optimistic
   slot. At each ordinary evaluation, physical payload downloaded from an
   interested peer in the last round is the first comparison after the
   existing equal priority; seed-style quota, achieved upload, waiting time,
   and stable generation order remain the fallback.
10. Tracker announces for a metadata-verified, desired-running active content
    generation carry the actual publishable listener port when an incoming
    route is installed and retain exact `left`, downloaded, and uploaded
    counters. Public torrents announce that same independently observed TCP
    port through DHT; private torrents continue suppressing DHT and PEX.
11. The extension handshake never claims BEP 21 `upload_only=1` while the
    torrent still intends to download. Complete seeding may retain the current
    truthful value after the publication handoff.
12. Completion fences new upload reads, joins admitted work, and transfers to
    the existing published-content registration without a false bitfield or a
    period in which unverified storage is served. This slice may deliberately
    close peer sockets at the namespace-publication fence; seamless socket
    retention across publication is not required.
13. Active-torrent Peers and session accounting show concurrent physical
    payload download and upload on the same connection, with exact totals,
    rates, slot state, queues, and terminal zero ownership. Tracker uploaded
    counters use the same successful-wire-payload authority.
14. Deterministic transitions, adversarial runtime failures, controlled
    RSTorrent/RSTorrent transfer, and pinned-libtorrent transfer prove upload
    before either participant completes. Both TCP initiation directions,
    Fast and ordinary availability, cross-file/part storage, and forced MSE
    regression are covered.
15. Both Android Rust ABIs, generated-boundary compatibility, Android build,
    and a no-window API 34-or-newer AVD SAF run prove the same partial upload,
    storage failure, lifecycle, and resource semantics. No Android UI control
    is required.
16. The tactical, owning topics, readiness queue, protocol claims, and exact
    evidence are reconciled before completion, and all tasks, socket leases,
    read jobs, writer charges, file handles, routes, and temporary fixtures
    return to zero.

A partial bitfield unit test, upload after `Complete`, or an outgoing-only
loopback success does not satisfy this stopping condition.

## Stable Scenarios

| ID | Scenario | Required outcome |
| --- | --- | --- |
| T124-C01 | Empty active peer | A metadata-verified active torrent with zero verified pieces accepts a routed incoming peer, sends Fast `HaveNone` or the ordinary no-bitfield form, advertises its real eligible port, and may download from that peer without offering payload. |
| T124-C02 | Resumed partial initial state | After conservative recheck restores a sparse have set, both initiated directions send the exact sparse initial bitfield before any HAVE update. Fast `HaveAll` is forbidden unless every metainfo piece is readable. |
| T124-C03 | Newly verified piece | The write/hash join establishes one piece, the shared availability revision advances once, every current eligible peer eventually receives one nonredundant HAVE, and a subsequent legal request returns the exact bytes before torrent completion. |
| T124-C04 | Request racing verification | A request received before authority publication cannot reserve a read. A Fast peer gets its exact legal terminal; an ordinary peer follows the existing choke/request policy. The same request after HAVE succeeds. |
| T124-C05 | Outgoing duplex | RSTorrent dials a peer, downloads one piece from it, prefers it at the next ordinary unchoke evaluation, and uploads a different verified piece over that same socket while both torrents remain incomplete. |
| T124-C06 | Incoming duplex | A peer dials RSTorrent, is attached to the active content generation, supplies one missing piece, then requests and receives a different local verified piece on that same socket. |
| T124-C07 | Complementary swarms | RSTorrent and an independent peer start with disjoint verified subsets. Wire evidence proves at least one payload block in each direction before either reaches complete; both final payloads independently match all metainfo hashes. |
| T124-C08 | Fast lifecycle | Choke, Allowed Fast, request, cancel, read completion, writer pressure, and disconnect retain Tactical `093`'s exactly-one-piece-or-reject terminal behavior during active download. |
| T124-C09 | Slow HAVE consumer | A peer whose writer cannot consume availability changes does not stall hashing or other peers. It is closed once its bounded change cursor falls behind; a replacement receives one current initial snapshot. |
| T124-C10 | Cross-file and padding | A 16 KiB request crossing wanted files and padding returns exact concatenated bytes and zeroes. Segment count, handles, read jobs, and response charge stay within declared limits. |
| T124-C11 | Part-backed verified piece | A verified boundary piece whose skipped spans live in the current part slot is advertised and served. RSTorrent does not inherit JSTorrent's inability to serve `.parts` data. |
| T124-C12 | Selection route change | Promotion or demotion enters the existing storage fence, drains or cancels affected upload plans, retains availability when the exact bytes remain readable, and closes informed generations before clearing any bit that becomes unreadable. |
| T124-C13 | Read observation failure | Truncation, wrong kind/length, provider rejection, stale namespace generation, open failure, or short read retracts every affected piece, fails the current request safely, closes every connection that may rely on the old advertisement, and enters existing root/repair policy when applicable. |
| T124-C14 | Hash failure | A failed received generation is never advertised, no upload read is planned from it, contributor recovery remains bounded, and a later successful generation produces exactly one availability transition. |
| T124-C15 | Pause and force recheck | The serialized controller removes discovery/routing eligibility, chokes and joins uploads, closes peers, and begins checking only after no old-generation response can start. Re-entry publishes only the new verified/readable snapshot. |
| T124-C16 | Publication handoff | The last required piece may be uploaded while incomplete. Publication then fences reads and either explicitly closes or safely drains each socket before the current complete registration takes authority; no request is served from mixed namespaces. |
| T124-C17 | Tracker, DHT, and privacy | A public incomplete torrent with a route emits actual-port tracker and DHT traffic with nonzero `left`; a private counterpart emits actual-port tracker traffic but no DHT or PEX. Completed is sent only on real selected completion. |
| T124-C18 | Mixed session pressure | At least three active incomplete torrents and existing complete seeds compete beneath the same eight slots, ten reads, 40 handles, and session peer budget. Download contributors rank first, optimistic rotation remains live, and no torrent creates a private upload pool. |
| T124-C19 | Restart boundary | A checkpoint-dirty piece is uploadable before process death. After restart it is unavailable until conservative checking—or Tactical `120` if separately completed—re-establishes authority; no persisted bit is trusted merely because it was previously announced. |
| T124-C20 | SAF parity | An AVD peer exchanges complementary pieces through current SAF staging and part routes, then a simulated grant/provider failure causes joined retraction and awaiting-storage behavior with no Kotlin payload callback and no leaked broker request. |

## State And Wire Contract

### Torrent eligibility

The content route uses explicit facts rather than `Complete` as a proxy:

| Torrent/storage state | Incoming content route | Advertised payload | Discovery port |
| --- | --- | --- | --- |
| Metadata unavailable/acquiring | Out of this slice; retain current behavior | None | Current outbound-only behavior |
| Full check queued or active | No | None | Outbound-only; announcements stopped as Tactical `108` requires |
| Metadata verified, active, zero have | Yes | None | Actual eligible listener port |
| Metadata verified, active, partial have | Yes | Exact verified/readable set | Actual eligible listener port |
| Publication fence | Temporarily no new route/read admission | Frozen only until admitted work drains; then no old-generation authority | Reconciliation may briefly use outbound-only state but cannot claim an unroutable endpoint |
| Complete and published, desired running | Existing complete registration | Exact verified/readable set | Actual eligible listener port |
| Paused, archived, repairing, awaiting storage, removing, or removed | No | None | Outbound-only or stopped according to existing lifecycle |

Metadata-only incoming acquisition is not smuggled into this slice. The new
route begins when verified metadata and an admitted content owner exist.

### Initial and incremental availability

One dense task-free availability value owns:

- a storage epoch;
- the exact piece count and compact bits;
- a monotonic availability revision; and
- a bounded timeline of newly available piece indices for established peers.

The bit value may advance from false to true only after the complete
write/hash join and a readable route exist in the same epoch. It may remain
true across selection changes only when the storage fence proves every logical
byte still has a current readable route. Clearing a previously announced bit
requires fencing affected plans and replacing informed connection generations
because RSTorrent implements no negotiated `DONT_HAVE` message.

At handshake completion the connection captures one coherent bitfield and
timeline cursor. Fast peers receive `HaveAll` only when every bit is set and
`HaveNone` only when none is set. Non-Fast peers receive a bitfield when any
bit is set and otherwise use BEP 3's omitted-bitfield form. Spare bits are
zero. The initial availability frame precedes incremental HAVEs for that
connection.

Piece verification appends at most one timeline item for a false-to-true
transition. A connection drains no more than the existing 16 peer-command
entries per wake. The writer suppresses a redundant HAVE when the remote
already advertised that piece. A connection more than 4,096 availability
events behind closes instead of pinning history or blocking the content
supervisor; reconnect produces a current initial snapshot.

### Request and read authority

`UploadPeerState` must consume current availability and epoch instead of its
present immutable `Arc<[bool]>` snapshot. Its deterministic transition logic
remains independent from Tokio, sockets, and storage. A legal request retains:

- connection and request generation;
- torrent availability revision and storage epoch;
- exact BEP 3 request geometry;
- Fast negotiation, choke, and allowed-set state; and
- a bounded immutable logical read plan produced under the current storage
  route owner.

The plan maps only the requested range, not a whole piece or torrent. It may
reference current wanted staging, an existing retained skipped source, the
current part-file slot, and padding. Each non-padding segment has a stable
`StorageFileReference`, exact expected artifact observation, offset, length,
and namespace generation. A 16 KiB request can contain at most 16,384
positive-length logical segments; zero-length metainfo files add none. Plans
over that mathematical bound are rejected as invariant failures.

The existing session upload-read owner executes at most ten plans. A stale
epoch, retracted bit, failed observation, changed length/kind, failed open,
short read, choke/cancel generation, or closed connection prevents the first
wire byte. Once any byte of a Piece frame is written, the existing writer
finishes the frame or closes the socket rather than corrupting framing.

### Choking and reciprocity

Tactical `082` already adopted pinned libtorrent's fixed eight-slot choker,
one automatic optimistic slot, 15-second ordinary evaluation, 30-second
optimistic rotation, and complete-seed quota. Preserve those values.

Extend the task-free scheduler input with physical payload downloaded from the
peer in the last completed ordinary round. For equal upload priority, compare
that value descending before the existing seed fallback. This matches pinned
`choker.cpp::compare_peers`: an active downloader favors peers contributing
payload; when every candidate contributed zero, the existing round-robin,
achieved upload, quota, waiting-time, and stable-generation ordering applies.
RSTorrent has no peer-class priority surface in this slice, so all ordinary
peers retain equal priority.

The comparison is session-wide. Active torrents do not receive reserved slots
and complete seeds do not get a second pool. The optimistic slot keeps a new
or noncontributing peer able to bootstrap. Upload/download bandwidth rate
limits, torrent priority, share mode, and configurable choking algorithms are
not introduced.

### Discovery and extension truth

Replace the discovery registration's overloaded `incoming_registered`
meaning with an internal fact equivalent to `incoming_routable`. Tracker port
selection consumes that fact independently of `complete`. DHT self-announcement
requires desired-running, verified-public metadata, a routable same-family TCP
endpoint, and nonprivate policy; it does not require `left == 0` or a payload
piece. DHT lookup remains useful even when no endpoint is advertisable.

Tracker `left` remains selection-aware current work, and downloaded/uploaded
counters remain exact physical payload counters. The started/update/completed/
stopped schedule remains unchanged except that a route or endpoint-generation
change requests an immediate corrective announce. Private torrents keep
tracker behavior but suppress DHT and PEX before any socket becomes useful.

BEP 21 `upload_only` describes intent, not local piece count. An incomplete
downloader sends false or omits the value. A selected-complete published seed
may send true even when intentionally skipped metainfo pieces remain absent,
provided its exact bitfield still tells the truth.

## Owner, Task, Cancellation, And Dependency Map

```text
ApplicationService / per-torrent serialized controller
  -> desired run, check, selection, storage/root, publication lifecycle
  -> application-lifetime TorrentRuntime
       -> task-free peer registry and connection generations
       -> active-content ingress registration (present only while admitted)
       -> tracker/DHT counters and discovery registration

active content supervisor in rstorrent-engine
  -> SwarmState / picker / request ownership
  -> ContentStoragePipeline / SelectiveStorage route and hash-write joins
  -> task-free VerifiedPieceAvailability { epoch, bits, revision, timeline }
  -> outgoing PeerSocketSet and bounded incoming-content ingress
  -> shared upload-coordinator registration for every established peer

session IncomingPeerService
  -> accept, budget, plain/MSE classification, handshake, info-hash routing
  -> transfer admitted active socket to the torrent content ingress
  -> retain current completed-content loop when no active content owner exists

session UploadCoordinator
  -> one task-free UploadScheduler and timer adapter
  -> eight grants across active and complete torrents
  -> one ten-job read admission owner and existing writer/peer cleanup

StorageFilePool and platform broker
  -> 40 native handle leases shared by download, hash, check, and upload
  -> bounded path/SAF opens and observations; no protocol or availability state
```

The active content supervisor remains the sole owner that can publish a new
piece or storage epoch. `SelectiveStorage` remains the state-transition owner
for selection routes, part slots, materialization, and publication. It gains a
bounded task-free read-plan seam; upload must not take a mutable storage
reference into a peer task or make `SeedContent` a second write-side owner.

`IncomingPeerService` retains unauthenticated intake and role-specific
plain/MSE bootstrap. After identity and duplicate admission, an active socket
crosses an eight-entry handoff bounded by the existing eight pre-handshake
tasks and the session peer lease it already owns. A full/stopped handoff closes
the socket. Do not unify outgoing initiator and incoming responder bootstrap
behind a generic async trait; only their established content behavior and
upload composition converge.

Every peer socket task remains the owner of one stream generation and its
reader/writer halves. Every upload request is canceled by connection loss,
grant loss, availability/storage epoch replacement, or torrent cancellation.
The storage fence joins admitted planning/read work before route mutation.
Pause, recheck, root loss, repair, publication, removal, and shutdown remove
the active ingress before canceling and joining peers, preventing a late
handshake from reviving the old generation.

No new session-global background task, detached read, per-peer filesystem
task, native host, daemon, REST/WebSocket payload proxy, or platform payload
callback is permitted.

## Exact Resource And Work Bounds

This slice preserves or adds these explicit ceilings:

| Resource | Bound |
| --- | ---: |
| Session ordinary peer target | Existing effective 200 |
| Incoming slack | Existing 10 connections |
| Pre-handshake tasks | Existing 8 |
| Active-content socket handoff | 8 admitted sockets; each already owns a session peer lease |
| Outgoing per-torrent pending/live working sets | Existing 30 / 30 beneath the session ceiling |
| Session upload slots | Existing 8, including one optimistic slot |
| Validated queued requests | Existing 2,000 per peer; at most 32,768,000 logical requested bytes |
| Upload reads | Existing 10 session-wide |
| Storage file handles | Existing 40 session-wide |
| Per-peer writer descriptors | Existing 64 |
| Per-peer writer charge | Existing 528,396 bytes; 4,227,168 across eight full slots |
| Piece count | Existing maximum 2,097,152 |
| Dynamic availability bits | At most 262,144 bytes per active incomplete torrent |
| Availability timeline | 4,096 entries at at most 16 bytes each, or 65,536 bytes per active incomplete torrent |
| Availability drain | At most 16 HAVE commands per connection per wake |
| Initial bitfield | At most 262,144 bytes, charged to the existing writer and shared when generations match |
| Upload plan segments | At most request length, therefore 16,384 positive-length segments per 16 KiB request |
| Active incomplete torrents | Existing configured maximum 20; dynamic availability/timeline ceiling 6,553,600 bytes at that maximum |

Availability history is trimmed to the minimum live cursor. A lagging peer is
closed at the fixed timeline limit. Initial bitfields, metadata, geometry, and
file references use immutable shared storage where generations match; they are
not copied into an uncharged queue. A bitfield or HAVE storm cannot await a
slow peer from the write/hash completion path.

The ten read plans are independently charged from payload buffers. Record
high-water marks for plan segments, current and queued plans, read bytes,
file leases, writer bytes/descriptors, availability entries, cursor lag,
handoff depth, upload slots, session connections, and all terminal owner
counts. Tightening a limit is allowed from deterministic evidence; raising one
requires reference or measured resource evidence and a tactical update.

## Source-First Record

Reference inspection used the revisions pinned in
[`../references.md`](../references.md) and
[`../../reference/pins.toml`](../../reference/pins.toml). Source is a behavior
and completeness oracle only; no reference source, fixture, task graph, class
graph, or persistence format is imported.

### Normative specifications

- `reference/bittorrent.org/beps/bep_0003.rst` defines connections as an
  ongoing bidirectional message stream, interest/choke transfer conditions,
  exact bitfield ordering and padding, HAVE as a newly completed hash-checked
  piece, 16 KiB requests, cancels, piece frames, tracker `port`, and `left`.
- `reference/bittorrent.org/beps/bep_0006.rst` defines replacement
  `HaveAll`/`HaveNone`, allowed-fast behavior, and exactly one Piece or Reject
  terminal for every Fast request through choke and cancel races.
- `reference/bittorrent.org/beps/bep_0005.rst` defines `announce_peer` as
  advertising the peer's BitTorrent protocol port; it does not restrict
  announcements to seeds.
- `reference/bittorrent.org/beps/bep_0015.rst` retains the UDP tracker port and
  transfer-counter wire fields already implemented by RSTorrent.
- `reference/bittorrent.org/beps/bep_0021.rst` makes `upload_only` an intent
  signal. Merely having some verified pieces does not make a downloader
  upload-only.

### Pinned libtorrent oracle

Pinned libtorrent commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected at:

- `src/bt_peer_connection.cpp::write_bitfield`, which sends a sparse picker
  bitfield for a nonseed, `HaveNone` only for zero have, and `HaveAll` only for
  a true seed when Fast is negotiated;
- `src/torrent.cpp::we_have`, which documents complete hash verification and
  successful disk write, then calls `announce_piece` on every attached peer;
- `src/peer_connection.cpp::{announce_piece,incoming_request}`, which
  suppresses redundant HAVEs and validates a requested piece against current
  local have state, geometry, queue bounds, interest, choke, and Fast policy;
- `src/torrent.cpp::attach_peer` and
  `src/peer_connection.cpp`'s torrent attachment path, which attach incoming
  connections to an ordinary downloading torrent rather than a seed-only
  service;
- `src/choker.cpp::{compare_peers,unchoke_compare_rr,unchoke_sort}`, which
  ranks equal-priority peers first by payload downloaded from them in the last
  round, then falls through to seed rotation and achieved upload behavior;
- `src/session_impl.cpp::{recalculate_unchoke_slots,
  recalculate_optimistic_unchoke_slots}`, which applies the session-wide
  ordinary and optimistic sets; and
- `src/settings_pack.cpp` plus `include/libtorrent/settings_pack.hpp`, which
  retain fixed slots, eight unchokes, automatic one-fifth optimistic slots,
  30-second optimistic rotation, and round-robin seed behavior.

Relevant pinned tests inspected:

- `test/swarm_suite.cpp::test_swarm` connects one seed and two constrained
  downloaders so both active downloaders participate in a real three-peer
  swarm before completion;
- `simulation/test_peer_connection.cpp::{alternate_have_all_have_none,
  bitfield_and_have_none,bitfield_and_have_all,invalid_request,short_bitfield,
  long_bitfield}` covers availability replacement and hostile request/bitfield
  behavior;
- `test/test_fast_extension.cpp::{reject_fast,invalid_request,
  outgoing_have_all,incoming_have_all}` covers Fast request and both connection
  directions;
- `simulation/test_swarm.cpp::{plain,block_uploaded_alert,redundant_have,
  unchoke_slots_limit}` covers ordinary swarm completion, physical upload
  observation, HAVE suppression, and slot limits; and
- `simulation/test_optimistic_unchoke.cpp::optimistic_unchoke` covers bounded
  optimistic rotation across many peers.

RSTorrent adopts the observable per-piece availability, ordinary incoming
attachment, reciprocity ordering, and existing default slot policy. It does
not adopt libtorrent's class graph, disk thread pool, predictive-piece
announcement, super-seeding, peer classes, or seamless namespace model.
RSTorrent's current explicit storage-generation fence may close connections at
publication, which is an intentional bounded difference in this tactical.

### JSTorrent product history

The local JSTorrent `main` checkout was inspected at:

- `packages/engine/src/core/torrent-peer-handler.ts::setupListeners`, which
  sends current availability for every peer and routes `request` events to the
  same torrent uploader used while downloading;
- `packages/engine/src/core/torrent-uploader.ts::{queueRequest,
  fillSendBuffers}`, which validates current serveability and performs bounded
  per-peer queued uploads from the ordinary torrent content reader;
- `packages/engine/src/core/torrent.ts::{getAdvertisedBitfield,
  canServePiece}` and its piece-completion paths, which mark verified pieces
  and queue HAVE while the torrent is active;
- `packages/engine/src/core/torrent-tick-loop.ts::{queueHave,flushHaves}`,
  which batches HAVE delivery rather than blocking piece completion; and
- `packages/engine/test/core/{advertised-bitfield.test.ts,
  torrent-uploader.test.ts}`, which exercise partial availability and bounded
  request/read behavior.

JSTorrent confirms the intended first-party product behavior: downloading and
uploading share one torrent peer set. Its current advertised bitfield masks
pieces held in `.parts` because that uploader cannot serve them. RSTorrent
does not adopt that limitation: its native selective storage already owns
safe part-slot reads, so T124-C11 requires those verified bytes to be served
under the common route epoch.

## Staged Implementation

1. **Lock deterministic semantics.** Add failing task-free tests for exact
   sparse initial availability, false-to-true publication, stale epoch,
   retraction, Fast request races, cursor overflow, and reciprocity ordering.
   Gate on no sockets, filesystem, Tokio clock, or platform types in the
   transition owner.
2. **Extract dynamic availability.** Replace immutable per-peer availability
   copies with the shared epoch/revision/bit/timeline authority. Connect
   existing picker and verified-piece events, preserve Pieces projection, and
   prove the 2,097,152-piece and 4,096-event bounds before adding reads.
3. **Add active read planning.** Let `SelectiveStorage` produce bounded
   generation-stamped request plans through the existing content-storage
   fence. Exercise staging, retained source, part, cross-file, padding,
   selection, observation failure, and root loss for path and fake platform
   references under the existing ten reads and 40 handles.
4. **Compose outgoing upload.** Feed interest/request/cancel and current
   download contribution into the shared upload coordinator from established
   outgoing content peers. Gate on simultaneous download/upload, slow writer,
   Fast terminals, accounting, and terminal zero ownership.
5. **Route incoming active peers.** Install the bounded content ingress for an
   admitted metadata-known download, transfer post-handshake sockets into the
   ordinary content peer set, apply duplicate resolution, and prove both
   directions without changing role-specific plain/MSE bootstrap.
6. **Make discovery truthful.** Separate routability from completeness,
   correct tracker and DHT port eligibility, retain private gating and exact
   counters, and test endpoint/root/lifecycle generation changes.
7. **Fence lifecycle and publication.** Exercise selection, pause, recheck,
   archive, root failure/repair, completion publication, replacement, removal,
   and shutdown. Close old informed generations whenever availability is
   withdrawn and prove no old read starts.
8. **Run controlled interoperability.** Use an independently authored fixture
   with complementary verified subsets. Run RSTorrent-initiated and
   libtorrent-initiated plaintext TCP cases, one forced-MSE case, one
   RSTorrent/RSTorrent case, and cross-file/part storage. Capture a Piece frame
   in each direction before either side completes and independently verify all
   final hashes.
9. **Close Android parity.** Build both Android Rust ABIs, regenerate or prove
   byte-identical generated boundaries, assemble and test Android, then run the
   no-window API 34-or-newer AVD SAF complementary-piece and provider-failure
   profile with exact broker/handle/read/task cleanup.
10. **Reconcile evidence.** Run the proportional workspace baseline, update
    this execution record and every materially changed topic/claim, reconcile
    the authoritative queue, and remove controlled payloads, logs, captures,
    AVD app data, and other temporary artifacts.

Each stage must preserve the prior deterministic and runtime gates. An
ordinary internal name or private module placement does not require approval.
Do not skip directly to a happy-path socket test while availability, route
epochs, or withdrawal remain undefined.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Task-free state | Exact initial availability for none/sparse/all; padding bits; one verification transition; no premature HAVE; redundant suppression; dynamic request eligibility; stale epoch; withdrawal; 4,096-event lag; reciprocity and optimistic ordering; exact counters and limits |
| Engine storage/runtime | Path and fake-platform staging/retained-source/part/padding reads; 16,384-segment edge; ten reads; 40 handles; slow writer; concurrent peer intake; Fast request/cancel/choke/read races; selection and publication fences; read failure retraction; all task/queue/byte owners zero |
| Session/application | Incoming route install/remove; duplicate direction; three active torrents plus complete seeds; actual tracker/DHT port; private gating; pause/recheck/root loss/repair/archive/remove/shutdown; active Peers row carrying simultaneous download/upload; exact uploaded tracker counter |
| Crash/restart | Death before verification, after verification before checkpoint, after checkpoint, during route change, and during publication; no false post-restart advertisement; conservative checker restoration independent of Tactical `120` |
| Controlled interoperability | RSTorrent and pinned libtorrent with complementary subsets, both TCP initiation directions, ordinary and Fast availability, one forced-MSE case, captured bidirectional pre-completion Piece frames, exact independent final SHA-1 verification, and terminal cleanup |
| Android build/runtime | Both Rust ABIs; generated compatibility; Android unit/JVM/build gates; API 34+ no-window AVD SAF partial exchange, part-backed request, provider failure, awaiting-storage recovery, and exact broker/read/handle/task high-water evidence |
| Regression | Existing upload, BEP 6, peer duplicate, selection, checkpoint, force-recheck, publication, discovery, MSE, concurrent-torrent, path, SAF, and complete-seed suites remain green |
| Live/public | None required. Public-swarm performance and reliability evidence belongs to Tactical `122` or a separately authorized live run. |

The normal proportional Rust baseline is:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Run the Android and controlled interoperability commands recorded in
[`DEVELOPMENT.md`](../../DEVELOPMENT.md) that apply to the landed boundary.
Regenerate the TypeScript and Kotlin/application contracts only if a crossing
type changes; otherwise record a byte-identical/no-change check. Do not launch
visible desktop clients merely to exercise engine behavior.

## Non-Goals And Intentional Deferrals

- finite upload or download bandwidth limits, token buckets, schedules, or
  per-torrent rate policy;
- ratio, uploaded-byte, idle-time, elapsed-time, or other seeding stop goals;
- durable lifetime upload totals needed by future ratio goals;
- user-facing settings or web/Android presentation for bandwidth, slots, or
  goals;
- super-seeding, share mode, predictive-piece announcements, peer classes,
  configurable choking algorithms, or torrent upload priority;
- metadata-only incoming acquisition before verified metadata enters a
  content generation;
- seamless preservation of established sockets across the staging-to-
  published namespace transition;
- `DONT_HAVE` or another new peer-wire extension for availability withdrawal;
- uTP socket/runtime integration, hole punching, local service discovery,
  web seeds, HTTP streaming, v2/hybrid torrents, or verified HTTP serving;
- public-swarm performance tuning or a comparative speed claim;
- a generic filesystem trait, new storage backend, rewritten part format,
  broad peer-bootstrap unification, native host, companion server, or payload
  proxy; and
- an iOS product or physical-iPhone rerun. The shared native engine/storage
  semantics are applicable to a future iOS client, but no such product surface
  currently exists.

The next separate policy tactical may add finite bandwidth enforcement and
seeding goals after this slice provides truthful upload opportunity and
accounting. Its policy must not reopen per-piece integrity or duplex peer
ownership.

## Escalation Contract

When implementation is separately authorized, its in-scope authority includes
private module extraction, adding the task-free availability/read-plan values,
extending the current upload scheduler input, changing internal discovery
eligibility vocabulary, adding bounded channels and tests within the declared
ceilings, fixing same-boundary bugs exposed by adversarial cases, running the
controlled local interop fixture and headless AVD, and updating generated
artifacts if an in-scope crossing type truly changes.

Stop for maintainer direction if evidence requires:

- a new durable schema field, public setting, visible product behavior, or
  changed file-selection meaning;
- uploading a hash-verified piece whose bytes cannot be proven readable under
  the current route, or trusting persisted have without recheck outside
  Tactical `120`;
- a second upload scheduler/read pool, a platform payload proxy, a new runtime
  dependency, unsafe code, or a new protocol transport;
- increasing a declared resource ceiling without pinned or measured evidence;
- public-network activity, a visible product client, or physical-device use;
- preserving sockets across publication by weakening namespace fencing; or
- importing reference source, fixtures, or data under a new license posture.

An ordinary test failure, internal owner extraction, stable tie-break choice,
controlled loopback timeout, or a reference architecture that differs from
RSTorrent is not by itself an escalation.

## Planning Record

On 2026-08-10, repository and reference inspection established the current
whole-torrent gate, the reusable upload/storage owners, pinned libtorrent's
per-piece and reciprocity behavior, and JSTorrent's first-party duplex history.
No Rust, TypeScript, Kotlin, schema, generated contract, test fixture, or
runtime behavior changed while drafting this tactical. The execution record
below supersedes that planning-time state as implementation lands.

## Execution Record

### Deterministic availability and active-read foundation

- `PieceAvailability` owns at most 2,097,152 compact bits, one storage epoch,
  a monotonic revision, and a 4,096-entry false-to-true timeline. Initial
  snapshots choose `HaveAll`, `HaveNone`, sparse Bitfield, or the ordinary
  no-message form exactly; cursors drain at most 16 HAVEs and classify epoch
  replacement or lag without blocking the publisher.
- `UploadPeerState` now consumes the shared authority instead of retaining an
  immutable per-peer bool slice. Request admission, read start, and response
  serialization retain and revalidate the availability epoch/revision; a
  withdrawn or replaced route cannot emit payload.
- `SelectiveStorage::prepare_upload_read` validates current verification,
  request geometry, and route epoch, then maps only the requested range to
  immutable staging/retained-file, part-slot, or padding spans under the exact
  16,384-segment ceiling. Execution uses existing positional file references
  and returns a short/open/read failure as a typed storage error.
- Focused tests pass for none/sparse/all initial forms, spare-bit clearing,
  one-time publication, 16-item drains, 4,096-event lag, epoch replacement,
  maximum geometry, cross-file/padding reconstruction, and a verified
  part-backed boundary read.

Validation at this checkpoint:

```text
cargo test -p rstorrent-engine piece_availability
cargo test -p rstorrent-engine upload::tests
cargo test -p rstorrent-engine active_upload_read
cargo fmt --all -- --check
cargo clippy -p rstorrent-engine -- -D warnings
```

### Active-route discovery truth

- Discovery registration now names the operative fact `incoming_routable`
  instead of overloading completed-seed registration. The active content owner
  sets that fact only after the listener accepts its registration and clears
  it before unregistering; a session wake drives a corrective advertisement
  on both transitions.
- UDP/HTTP/HTTPS tracker announces select the current eligible family TCP port
  for an incomplete routed torrent while retaining exact nonzero `left` and
  physical transfer counters. Completion requests the real `completed` event
  independently of whether an incoming endpoint is currently available.
- Public DHT lookup-and-announce requires desired-running, verified-public,
  active-route, and eligible endpoint facts, but no longer requires torrent
  completion. A family with no eligible TCP endpoint performs lookup without
  announcing a port-1 fiction; at least one real family is required to enter
  announce mode. Private torrents still suppress DHT, and RSTorrent continues
  to omit unsupported BEP 21 `upload_only` rather than claiming seeding intent
  during download.
- Deterministic UDP evidence observes `started` with port 1, then a
  pre-completion corrective update carrying the actual port and unchanged
  nonzero `left`. Dual-family DHT evidence proves a configured IPv4-only
  announcement stores no IPv6 peer value. The application HTTP tracker test
  observes the live listener replacement port while its payload remains held
  and the torrent is incomplete.

Validation at this checkpoint:

```text
cargo test -p rstorrent-engine advertisement::tests --lib
cargo test -p rstorrent-engine \
  dht::tests::product_announcement_uses_each_familys_port --lib
cargo test -p rstorrent-session \
  http_tracker_only_peers6_completes_hash_verified_application_transfer
cargo clippy -p rstorrent-engine -p rstorrent-session -- -D warnings
```

### Live active route and outgoing duplex upload

- The content-storage task now accepts a separate bounded 16-item planning
  channel. It returns immutable active-read plans while retaining exclusive
  ownership of mutable selective storage; execution remains under the existing
  session-wide 10-read semaphore and storage-file pool.
- Resumable content downloads install an active incoming registration as soon
  as verified metadata and storage exist. That route serves the exact initial
  sparse availability, upload requests, and later HAVEs from the same dynamic
  authority without requiring `Complete` or `Published` state.
- Every outgoing content connection now owns one `UploadPeerState`, one
  availability cursor, at most one read task, and one membership in the
  existing session upload coordinator. Accepted requests use the active
  storage route; completion waits briefly for already-admitted upload work
  before the publication boundary closes sockets.
- The session scheduler retains eight total slots including one automatic
  optimistic slot at the default configuration. Incomplete-torrent regular
  peers are reranked on the 15-second ordinary round by payload physically
  downloaded from that connection in the preceding round; complete-seed peers
  retain the existing upload-quota fallback.
- Deterministic loopback evidence uses complementary pieces. On the outgoing
  connection, RSTorrent advertises piece zero, downloads piece one, and emits
  the requested piece-zero `Piece` frame before completion. On the incoming
  connection, an active sparse route serves piece zero and later emits
  `Have(1)` after publication through the availability authority.

Validation at this checkpoint:

```text
cargo test -p rstorrent-engine active_incomplete_registration --lib
cargo test -p rstorrent-engine upload_scheduler --lib
cargo test -p rstorrent-engine \
  outgoing_connection_uploads_verified_piece_before_torrent_completion --lib
cargo clippy -p rstorrent-engine -p rstorrent-session -- -D warnings
```

### Accepted-socket duplex content path

- The incoming socket task remains the sole owner of its framed IO, upload
  state, metadata/PEX state, and cancellation. A generation-fenced per-torrent
  route connects it to the existing content supervisor through a bounded
  64-event channel and a bounded 16-command channel; there is no second
  request scheduler or socket owner.
- The content supervisor admits accepted connections into the same
  `SwarmState`, connection ceiling, picker, request/cancel lifecycle, storage
  pipeline, contributor accounting, and peer-integrity policy as initiated
  sockets. Commands use nonblocking bounded admission so a peer flooding the
  event side cannot create a cross-channel wait cycle; saturation closes the
  content role through the registered connection cancellation owner.
- Attachment generations fence late events. Closed command routes are pruned
  even if the bounded best-effort terminal diagnostic cannot be enqueued, and
  known-bad incoming contributors retain a pending ban until their last active
  connection is removed.
- Deterministic loopback evidence now proves the stronger incoming-duplex
  case: a peer dials the application listener, receives the exact sparse
  bitfield, uploads missing piece one, and receives verified piece zero over
  that same accepted TCP connection before RSTorrent completes or publishes.

Validation at this checkpoint:

```text
cargo test -p rstorrent-engine \
  accepted_connection_uploads_and_downloads_before_torrent_completion
cargo test -p rstorrent-engine \
  known_bad_incoming_contributor_is_banned_after_disconnect
cargo clippy -p rstorrent-engine -- -D warnings
```
