# Tactical 139: Incomplete-File Streaming Demand

Status: **Complete on 2026-08-11.** The compact demand model, generation-
fenced active reads, time-critical scheduler overlay, progressive HTTP
fulfillment, shared browser/Tauri action, Android-compatible engine boundary,
controlled pinned-libtorrent evidence, and complete repository gates pass.

Topics: `http-file-serving-and-streaming`, `capability-readiness`,
`oracle-driven-engine-campaign`, `download-correctness`, `peer-lifecycle`,
`storage-throughput-architecture`, `application-control`,
`application-connection-architecture`, `client-surfaces`,
`android-saf-storage`

Dependencies: completed Tactical
[`116`](116-platform-storage-coherence-and-ios-feasibility.md) supplies common
path/SAF observation, root health, and shared file-handle ownership; completed
Tactical [`120`](120-per-torrent-trusting-fast-resume.md) supplies durable
verified-piece authority; completed Tactical
[`124`](124-duplex-verified-piece-upload.md) supplies coherent verified-piece
availability plus active incomplete-storage read and upload ownership;
completed Tactical
[`134`](134-hierarchical-transfer-rate-enforcement.md) supplies rate policy
that urgent scheduling must obey; completed Tactical
[`135`](135-controlled-tcp-storage-near-parity.md) supplies current storage
intake and hash behavior; and completed Tactical
[`138`](138-verified-http-file-serving.md) supplies the capability, HTTP,
logical-file, client, and cancellation boundary this slice extends.

## Decision And Desired Outcome

Extend the existing file-scoped HTTP capability so an eligible active torrent
can serve an incomplete non-padding file. Each admitted `GET` advances in the
existing 64-KiB chunks. Before a chunk is read or emitted, the response owner:

1. maps the exact logical file range to its intersecting torrent pieces;
2. publishes bounded transient **current** and **ahead** demand to the active
   torrent generation;
3. waits until every current piece has passed the ordinary hash-verification
   transition in that generation;
4. obtains an immutable logical-range read plan from the existing mutable
   storage owner; and
5. reads and emits only the requested file bytes.

The torrent scheduler gives current pieces time-critical treatment across the
peers most likely to deliver them soon. This is not global sequential mode.
Ordinary work remains rarest-first outside the overlay, durable file selection
does not change, and local reads and urgent peer requests remain subject to all
existing storage, connection, outstanding-byte, active-piece, integrity,
transfer-rate, and cancellation limits.

Completed and already verified files retain Tactical `138` behavior, apart
from the explicit clarification below that successful body progress counts as
capability use. Creating or reading a streaming capability never starts,
resumes, restores, unskips, repairs, rechecks, or durably reprioritizes a
torrent.

## Scope And Stopping Condition

This tactical owns one end-to-end slice:

1. compact runtime-independent current/ahead demand intervals, deterministic
   merge and ordering, and a generation-fenced transient demand lease;
2. active logical-file eligibility plus wait-then-read access through the
   existing incomplete storage owner and verified-piece authority;
3. current-piece planning and peer selection ahead of ordinary rarest-first
   work, bounded ordinary-request preemption, and one adaptive urgent
   duplicate attempt;
4. progressive full and single-range HTTP `GET`, no-read/no-demand `HEAD`,
   pre-header first-chunk wait, later body stall handling, and exact lifecycle
   cancellation;
5. typed shared-client eligibility and the existing browser/Tauri `Open` path,
   without an embedded player or Android HTTP listener;
6. bounded diagnostic and resource high-water evidence without capability,
   URL, path, or peer-endpoint leakage; and
7. deterministic, scripted, controlled pinned-libtorrent, web/Tauri,
   proportional Android, and complete repository validation.

The tactical completes only when all of these conditions pass:

- no byte is emitted before every piece intersecting that byte's 64-KiB chunk
  has passed successful hash verification and the live storage route still
  matches the captured torrent/storage generation;
- one corrupted, reset, replaced, skipped, paused, archived, queued, checking,
  removing, errored, or unavailable-root generation cannot wake or feed a
  response from another generation;
- current demand is planned and requested before ordinary work without
  replacing durable selection or allocating memory proportional to a
  metainfo-controlled demand window;
- a slow or stalled urgent block may gain at most one request from a different
  peer, the first accepted response cancels the loser, and every attempt stays
  inside existing byte, connection, torrent, and session ceilings;
- initial head and tail probes, seeks represented by independent HTTP requests,
  overlapping readers, cancellation of one reader, and torrent completion
  during a body behave independently and return exact bytes;
- a disconnected, expired, revoked, timed-out, or superseded body removes its
  demand and releases wait, request, read, and storage ownership terminally;
- complete-file serving and `HEAD` retain the Tactical `138` contract, while
  a valid incomplete `GET` waits rather than returning unverified sparse data;
- the shared web and Tauri `Open` path can obtain and use a streamable URL;
  Android compiles the same engine/scheduler/storage semantics while retaining
  its completed-file-only native open policy; and
- all declared deterministic, runtime, controlled interoperability,
  generated-contract, web, desktop, Android, and repository gates pass with
  recorded high-water marks.

## Eligibility And Lifecycle Contract

An incomplete file is streamable only while all of these statements are true:

- the exact torrent resolves in the current application/profile generation,
  verified metainfo and layout are present, and the active download has a live
  storage owner;
- the file index names a non-padding, nonempty file whose durable selection is
  `Normal`;
- the torrent is actively downloading rather than queued, paused, archived,
  checking, removing, errored, or complete-but-not-yet-published;
- the selected root is available and the current path or platform storage
  generation can plan reads through the established storage authority; and
- media request, per-capability, per-torrent streaming, and application-wide
  admission all have capacity.

The generated file fact distinguishes `streamable` from Tactical `138`'s
already verified `available`. The semantic `create_media_url` operation
accepts either fact and rechecks it authoritatively. A stale presentation fact
may therefore return a bounded typed unavailable reason. HTTP continues to
hide that reason after capability resolution.

Paused, archived, or currently skipped files that already meet Tactical
`138`'s published verified-file contract remain readable through its immutable
reader. Those states are not eligible for incomplete waiting. A streaming
request does not rewrite them to become eligible.

Each active capability captures the profile, torrent, file, active download,
storage-route, and selection generations. Pause, archive, selection change,
force recheck, root transition, fatal storage failure, removal, profile
replacement, capability expiry/revocation, or application shutdown cancels
the active reader and all of its demand leases. A late update, verification,
read plan, or read result from the old generation cannot emit bytes.

Ordinary completion is not an error. If the same storage generation completes
and publishes while a response is live, the storage/application owner hands
the capability to the immutable published reader without changing URL or byte
position. That handoff is allowed only after exact identity, file length,
verification, route, and observation checks. If those checks fail, the body
terminates and the capability is revoked rather than guessing continuity.

## HTTP Fulfillment Contract

The route, capability format, Host/authentication policy, methods, single-range
syntax, response headers, MIME behavior, and `416` shape remain those of
Tactical `138`:

```text
GET|HEAD /media/v1/<capability>
```

`HEAD`, including the selected single-range compatibility behavior, uses
verified metainfo length and MIME information. It registers no demand, waits
for no piece, opens no storage representation, and returns no body.

An incomplete `GET` works as follows:

1. resolve the capability, parse the full or single range, admit one streaming
   lease when bytes are absent, and bind all generations;
2. define the first at-most-64-KiB current chunk and bounded ahead interval;
3. wait for and read the first current chunk before committing successful HTTP
   headers, while retaining at most that one prepared chunk;
4. emit it under body backpressure, advance current/ahead demand, then repeat;
   and
5. remove the lease on completion, disconnect, timeout, error, or cancellation.

Preflighting the first chunk prevents a known lifecycle or storage failure
from committing a misleading `200`/`206`. A valid capability whose first chunk
makes no relevant block, storage, or verification progress for 120 seconds
receives an empty `504 Gateway Timeout` with the ordinary private/no-store
headers. Streaming admission saturation receives the existing empty `503`
with `Retry-After: 1`. Revocation or generation mismatch remains the generic
`404` before headers.

After successful headers are committed, HTTP cannot change status. A later
120-second no-progress timeout, revocation, storage error, or lifecycle change
terminates the body early. Exact `Content-Length` makes the truncation visible
to a conforming client; it must not be padded, replaced with an error document,
or continued from a new owner. A successfully stored block or verified piece
in the request's current interval advances that lease's bounded progress
revision and resets the no-progress timer. The capability's existing 24-hour
absolute deadline remains a hard upper bound even while progress continues.

Each HTTP request is independent. A new range does not implicitly cancel an
older request because real players overlap reads and probe both file ends.
Disconnecting or explicitly abandoning one request removes only that request's
lease. The eight-per-torrent and existing global/per-capability ceilings bound
clients that leave obsolete seeks open.

An admitted body counts as active capability use. Each successfully emitted
chunk refreshes one shared monotonic last-use value without reacquiring the
application-service mutex; registry expiry and body liveness observe that same
value. This clarifies the idle contract so a progressing response may continue
beyond 30 minutes but never beyond the 24-hour absolute deadline. The update
is a timestamp replacement, not an event queue or per-chunk task.

## Verification And Active Storage Boundary

The existing `VerifiedFileReader` remains the immutable published-content
path. Active streaming adds an engine-owned logical reader backed by the same
owner that currently exposes verified incomplete pieces for upload:

```text
ContentSwarmDownload / mutable SelectiveStorage task
  -> successful hash + SelectiveStorage::record_verified
  -> bounded checkpoint-intent queue admission
  -> PieceAvailability publication for this route epoch
  -> latest-value availability progress notification
  -> bounded active logical-range plan request
       -> immutable path/platform/part-file read plan
       -> shared read admission + file-handle pool
```

`ActiveSeedContent` and `ContentStoragePipeline` already demonstrate the
required dependency direction: callers ask the single mutable storage task to
prepare a route-epoch-fenced immutable plan, then execute that plan without
borrowing `SelectiveStorage`. Implementation may generalize that boundary to
an active verified-content owner or add a sibling logical-file planner. It
must not lend `SelectiveStorage`, a raw payload path, SAF identity, or mutable
storage mutex to HTTP or session code.

The active wake point is the current `PieceAvailability` publication, which
occurs only after the full piece hash matches, storage records the piece as
verified, and a required checkpoint intent has entered its bounded queue. The
body need not wait for a later batched SQLite checkpoint flush: live-generation
byte integrity is already established, and any later checkpoint/storage
failure fails the owning download and cancels the response. After restart,
only the existing durable checkpoint policy may re-establish trusted
availability.

A logical file range maps with checked arithmetic through its file offset into
the torrent layout. Boundary pieces still require the entire torrent piece,
including neighboring-file or padding segments, to hash successfully. The
read plan returns only the requested logical file bytes. File existence,
partial-file presence, a completed block, or a successful write alone never
wakes a waiter.

There is no task per piece or demand. The generalized active-content owner adds
one latest-value availability-progress sender for the torrent generation.
Successful publication and route invalidation replace its compact
`AvailabilityCursor`; no piece list is queued. A response observes that signal,
drains the route-epoch `PieceAvailability` authority, and advances a
`next_missing_piece` cursor through its compact current interval. Pieces that
completed out of order are skipped when the cursor reaches them. Lag performs
one bounded snapshot reconciliation; route invalidation cancels rather than
rescanning a replacement bitfield under the old lease.

The active download also publishes one latest-value demand-progress snapshot
containing at most the eight live `(demand_id, revision)` pairs. A successfully
stored block or verified piece advances only the revisions of leases whose
current interval contains that piece. This lets each response reset its own
stall timer even while a large piece is arriving slowly, without a per-block
queue, per-waiter task, or unrelated torrent work extending the wait.

## Runtime-Independent Demand Model

Add a plain engine state machine, provisionally `StreamingDemandSet`, with no
Tokio, socket, HTTP, storage, application, or wall-clock dependencies. A
typed demand ID is unique within one active torrent generation. Each lease
contains:

- one current inclusive piece interval derived from the next response chunk;
- zero or one ahead inclusive piece interval;
- a monotonic update generation and deterministic admission order; and
- a cursor used to make bounded progress without expanding either interval.

The current interval is the JSTorrent-style “eye”: the bytes the response must
see verified before it can advance. Ahead is speculative but bounded useful
work after that eye, never the remainder of the file.

Piece intervals are required rather than `Set<u32>` or one entry per piece.
RSTorrent accepts piece lengths down to one byte, so one 64-KiB response chunk
may intersect 65,536 pieces and a byte-bounded ahead range may span far more.
The retained representation remains constant-size and all iteration has an
explicit per-pass ceiling.

Merging preserves independent lease ownership while producing this order:

1. missing current pieces before every ahead piece;
2. fair round-robin across current leases in admission order;
3. within one lease, ascending distance from its response byte position; and
4. fair round-robin across ahead leases, again in ascending byte order.

Overlaps are scheduled once but retain all interested lease IDs so removing
one request cannot erase another's demand. Verified, out-of-range, padding-
only, or durable-Skip-only pieces are never emitted as candidates. Updating a
lease replaces only its own intervals. Removing the final lease restores the
ordinary picker without reconstructing download state.

A small synchronous lease registry owns at most eight demands per active
torrent. It publishes latest-value bounded snapshots through the download
control boundary, so rapid body updates coalesce instead of filling an async
command queue. Dropping the RAII lease removes its ID synchronously. The
active download generation rejects late updates and never reuses an ID.
Stored-block and verified-piece transitions update the same bounded state and
publish its per-demand progress revisions.

## Scheduler And Peer Policy

Time-critical scheduling is a separate bounded pass ahead of ordinary
peer-centric scheduling. It runs when demand changes, relevant availability or
request state changes, and at least once per second while current demand
remains. It may inspect at most the existing 256 planned-piece candidates per
pass and never increases the current active-piece or planned-byte ceilings.

### Piece admission and ordinary work

- Demand planning calls the existing specific-piece reservation path before
  ordinary `RarestFirst` planning. Outside the overlay, rarest-first behavior
  is unchanged.
- If the planned/active working set is full, the scheduler first removes
  untouched ordinary plans. It may then cancel at most one untouched ordinary
  block attempt per peer in one pass, emitting the ordinary peer-wire cancel
  and releasing exact ownership, until a current piece can enter.
- Received, writing, hashing, or verified bytes are never discarded. A
  partially received ordinary piece keeps its data and may resume after urgent
  demand. If all capacity contains useful progress, urgent work waits rather
  than exceeding memory bounds.
- An accepted maximum-size piece may use the existing sole-over-limit plan
  rule only after other untouched plans have been removed. Streaming adds no
  second exception.
- While any current demand exists, ordinary scheduling may retain at most one
  ordinary outstanding request per eligible peer; remaining queue space is
  managed by the time-critical pass. Ahead demand never preempts current work.

### Peer choice and queue horizon

For a demanded block, select an eligible peer that has the piece and has the
shortest estimated download-queue time. The estimate is outstanding requested
payload bytes divided by an estimated payload rate. Extend the bounded request
window with only the observations required for this calculation:

- use a recent nonzero payload rate when payload arrived within 30 seconds;
- otherwise retain and use the peer's bounded peak payload rate;
- for a newly unchoked peer with less than 32 KiB received and less than five
  seconds of evidence, use the mean productive-peer rate when available; and
- use a conservative 16-KiB/s floor when no better evidence exists.

Exclude a peer that is choked, disconnecting, stalled/snubbed, integrity-
excluded, lacks the piece, already owns that block attempt, has no request or
session capacity, or cannot currently pass transfer-rate admission. When at
least ten otherwise eligible peers exist, exclude the slowest queue-time
decile only if every demanded piece still has an eligible holder. Never drop
the sole holder of a demanded piece.

Stop adding urgent requests when the best eligible peer's estimated queue is
greater than two seconds. Recompute after each reservation. Urgent requests
do not bypass Tactical `134` download caps or fairness; rate-credit delay is
real queueing, not permission to overdraw the limiter.

### Adaptive duplicate

The initial slice deliberately permits less duplication than libtorrent. A
current block that remains outstanding past its origin peer's existing
adaptive request timeout—mean request service plus four mean deviations,
bounded by current minimum/maximum policy—may gain exactly one additional
attempt from a different eligible peer. There are never more than two live
attempts for one urgent block and never two to the same peer.

The duplicate consumes ordinary connection, torrent, session, outstanding-
byte, rate, and active-piece authority. The first valid payload response wins
through the generalized strict duplicate owner; losers receive cancel and
late payload remains harmless. Hash failure resets the piece through the
ordinary integrity/reputation path and does not publish availability. The
streaming layer neither attributes corruption nor invents a separate retry
state.

HTTP byte ranges provide byte position, not media bitrate or presentation
timestamps. This slice therefore models relative current/ahead urgency and a
queue horizon, not fabricated playback deadlines. A later embedded player may
add real presentation deadlines through a separate typed extension without
changing the lease and verified-read ownership established here.

## Exact Initial Bounds

| Resource | Initial bound |
| --- | ---: |
| Live capabilities | existing 128 per application/profile generation |
| HTTP bodies | existing 16 application-wide, 4 per capability |
| Active streaming leases | 16 application-wide, 8 per torrent |
| Logical media read jobs | existing 8 application-wide |
| Body/current chunk | existing 64 KiB, one prepared chunk per response |
| Current demand representation | one constant-size inclusive piece interval |
| Ahead demand | next at most 4 MiB **and** at most 16 pieces |
| Demand candidates inspected | at most 256 per scheduling pass |
| Active storage-plan queue | existing 16 requests per active storage owner |
| Best-peer queue horizon | 2 seconds |
| No-progress timeout | 120 seconds per current chunk |
| Urgent block attempts | at most 2, on distinct peers |
| Shared storage handles | existing 40-handle application pool |
| Capability lifetime | existing 30-minute idle, refreshed by successful body progress; 24-hour absolute |

“At most 4 MiB and at most 16 pieces” means the ahead interval ends at the
earlier bound. A single very large current piece remains one piece; tiny pieces
remain a compact interval. Demand state, wait ownership, and diagnostic state
must not allocate proportional to either byte or piece span.

The 120-second timer is a stall-diagnosis and ownership bound, not an assumed
maximum block download duration. Relevant progress resets it so a healthy
low-rate swarm can continue. Tests use a controlled clock; production uses the
active runtime's monotonic clock.

Implementation may tighten a bound when deterministic or controlled evidence
shows the declared value is unsafe. Raising one, adding a second buffering
lane, or introducing a new unbounded cardinality requires tactical review.

## Ownership, Tasks, Cancellation, And Dependency Direction

```text
ApplicationService / profile generation
  -> bounded media capability registry
       -> published VerifiedFileReader, or
       -> active verified-file reader + exact download/storage generations
            -> StreamingDemandLease (current + ahead)
            -> latest-value availability and demand-progress signals
            -> PieceAvailability wait
            -> active storage plan request
  -> shared media router / Tauri media-only listener
       -> bounded response future (also owns the wait; no child waiter task)
            -> preflight first chunk
            -> body backpressure and later chunks

ContentSwarmDownload / active torrent generation
  -> runtime-independent StreamingDemandSet
  -> time-critical scheduling pass
  -> ordinary SwarmState request and strict-duplicate ownership
  -> single mutable ContentStoragePipeline task
       -> immutable verified logical-range plans

shutdown or invalidation
  -> revoke capability / invalidate route epoch
  -> cancel response and wake wait
  -> drop demand lease
  -> release permits and immutable plans
  -> join response/listener/download/storage owners normally
```

Pure file-to-piece geometry, demand merging, urgency ordering, queue-time
calculation, duplicate eligibility, and scheduler transitions remain runtime
independent. Engine control may depend outward on Tokio synchronization, but
those types do not enter protocol values or the pure demand/scheduler state.
Session code depends on engine readers and controls. `rstorrent-media` depends
on the session service. Neither engine nor session depends on Axum, HTTP
headers, Tauri, React, or Android presentation.

No new long-lived task is required. The existing HTTP response future owns its
wait, the active download owns scheduling, and the storage pipeline owns
mutable storage. Every owner already has a cancellation and join path that the
new lease must compose with.

## Security, Integrity, And Privacy Invariants

- Never emit a byte from a block merely because it was received or written;
  the full intersecting piece must hash successfully first.
- Treat metainfo geometry, file/range offsets, peer availability, demand
  updates, capability text, and HTTP headers as hostile and use checked,
  bounded representations before changing state.
- A capability still names one logical file, never a storage path, part file,
  root locator, SAF document, piece, neighboring file, or scheduling command.
- Never log capability values, complete URLs, query strings, storage paths,
  peer endpoints, or authorization headers. Structured diagnostics use counts,
  durations, byte totals, and typed reason categories only.
- Demand cannot make a durable-Skip-only piece wanted, change file selection,
  resurrect a stopped torrent, bypass root health, or survive its captured
  generation.
- Urgency never bypasses rate, outstanding-byte, active-piece, storage,
  descriptor, connection, or session limits.
- Hash failure, storage error, or route invalidation cannot be masked by a
  cached read plan. The existing integrity and owner failure paths remain
  authoritative.
- Nonloopback authentication/TLS, loopback Host enforcement, media-listener
  isolation, no-port-mapping policy, and capability indistinguishability remain
  Tactical `138` invariants.

## Observability

Extend existing bounded resource and diagnostic snapshots with:

- active streaming leases and their high water, application-wide and per
  torrent;
- current and ahead interval counts plus candidate inspections, without file
  names or byte positions;
- current wait age, per-demand progress revisions, stall timeouts, and typed
  cancellation causes;
- demand-specific plans admitted and untouched ordinary plans/attempts
  preempted;
- time-critical primary and duplicate attempts, winning responses, loser
  cancels, and redundant late payload;
- demanded pieces verified, demanded bytes read, and demanded bytes served;
  and
- active storage-plan, read-job, body-chunk, and file-handle high waters.

These are diagnostics/resource facts, not a new application view, command,
persisted history, player timeline, or stable public API. Existing peer and
piece views remain truthful; a demanded piece may use an existing compact flag
only if its meaning is exact and bounded.

## Source-First Record

No reference source, fixture, or test data is imported.

### Normative protocol and HTTP sources

The pinned BEP checkout at `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`
was rechecked for BEP 3 piece hashes and peer-wire request/cancel behavior.
Streaming changes request order only; it does not weaken the rule that a piece
is accepted after its complete SHA-1 hash matches.

[RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html), especially Sections
14.1, 14.2, 14.4, 15.3.7, and 15.5.17, remains the HTTP range source recorded
by Tactical `138`. This tactical does not change its selected inclusive,
open-ended, suffix, single-range, `206`, `416`, or range-bearing `HEAD`
behavior. Early body termination is deliberately visible as an incomplete
declared representation rather than a replacement response.

### Pinned libtorrent oracle

Rasterbar libtorrent `2.0.13.0` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

- `docs/streaming.rst` distinguishes deadline-driven streaming from sequential
  download and records peer queue-time sorting, the slowest-decile filter,
  two-second queue horizon, adaptive stall duplication, distinct-peer
  attempts, and batched request emission;
- `include/libtorrent/torrent_handle.hpp::{set_piece_deadline,
  reset_piece_deadline,clear_piece_deadlines}` defines mutable relative
  deadlines, earlier-deadline priority, cancellation, and optional
  read-when-available notification;
- `src/torrent.cpp::{set_piece_deadline,remove_time_critical_piece,
  clear_time_critical}` validates piece identity, sorts changed deadlines,
  promotes existing outstanding attempts, restores ordinary priority, and
  cancels noncritical requests when the first critical group arrives;
- `src/torrent.cpp::{request_time_critical_pieces,
  pick_time_critical_block}` selects by estimated peer download-queue time,
  excludes unusable peers, stops beyond two seconds, and progressively permits
  distinct-peer busy requests after adaptive timeouts;
- `src/request_blocks.cpp::request_a_block` limits ordinary request filling
  while time-critical work owns the remaining queue;
- `test/test_time_critical.cpp::{time_crititcal,
  time_crititcal_zero_prio}` covers demanded and priority-zero pieces;
- `test/swarm_suite.cpp::test_swarm` applies deadlines to pieces 2, 5, and 8 at
  0, 1,000, and 2,000 milliseconds;
- `test/test_read_piece.cpp::{time_critical,read_piece}` covers completion
  notification and reads; and
- `test/test_transfer.cpp::piece_deadline` covers a transfer with deadlines
  across the piece range.

RSTorrent adopts active peer-queue management, current-before-ahead ordering,
bounded ordinary preemption, queue-time peer choice, cancellation, and
adaptive distinct-peer retry. It does not adopt libtorrent's torrent/storage
architecture, priority-zero override, full-piece read allocation, fabricated
media deadlines, or progressively unbounded busy-request count. The initial
one-extra-attempt rule fits RSTorrent's existing strict duplicate ownership and
is an intentional conservative difference to validate before expansion.

### JSTorrent product oracle

The local JSTorrent reference at
`9895410beeed6aff554053769bd006a3fbd373ef` was inspected:

- `packages/engine/src/core/streaming-scheduler.ts` merges tokenized
  metadata/next/file/now demand, protects demanded pieces, suppresses low-
  progress ordinary work, and preserves durable Skip;
- `packages/engine/src/streaming/streaming-playback-session.ts` owns tokenized
  file/current/ahead demand, wait-then-read behavior, signal reuse, abort, and
  close cleanup;
- `packages/engine/src/node-io-daemon/engine-http-stream-bridge.ts` binds a
  token to one torrent/file session, returns already available ranges
  immediately, waits for active incomplete ranges, and rejects ineligible
  lifecycle states;
- `packages/engine/src/node-io-daemon/daemon-runtime.ts` streams in 256-KiB
  wait-then-read chunks and terminates on cancellation or owner failure;
- `packages/engine/test/core/streaming-scheduler.test.ts` covers urgency merge,
  protected and skipped pieces, suppression retention/release, token updates,
  file locks, and selection hints;
- `packages/engine/test/streaming/streaming-playback-session.test.ts` and
  `packages/engine/test/streaming/torrent-source.test.ts` cover byte-to-piece
  mapping, immediate and blocking reads, multi-piece waits, abort, demand
  reuse, and cleanup; and
- `packages/engine/test/node-io-daemon/daemon-media-streaming-integration.test.ts`
  and
  `packages/engine/test/node-io-daemon/daemon-backed-engine-streaming.test.ts`
  cover real blocking ranges, multi-chunk/multi-piece reads, concurrent
  readers, cancellation isolation, completed reads, stopped/skipped/error
  policy, and removal fan-out.

RSTorrent adopts tokenized independent demand, current/ahead separation,
wait-then-verified-read, concurrent-reader isolation, and lifecycle cleanup. It
does not adopt the companion daemon, whole-remainder lookahead, durable
priority rewriting, HLS/player layers, `409` lifecycle disclosure, or serving
from a caller-supplied path.

### Current RSTorrent seams reviewed

- `crates/rstorrent-session/src/media.rs` owns 128 volatile capabilities, 16
  requests, four requests per capability, eight read jobs, idle/absolute
  expiry, and immutable `VerifiedFileReader` leases.
- `crates/rstorrent-media/src/lib.rs` owns exact HTTP parsing and 64-KiB
  backpressured chunks.
- `crates/rstorrent-engine/src/{seed_content,active_seed_content}.rs` separates
  published logical reads from route-epoch-fenced verified active-piece reads.
- `crates/rstorrent-engine/src/driver/storage_pipeline.rs` owns one mutable
  selective-storage task, a 16-request active read-plan channel, immutable
  execution plans, hash completion, verified recording, and checkpoint intent.
- `crates/rstorrent-engine/src/driver.rs::ContentSwarmDownload` owns layout,
  selection, `PieceAvailability`, the active content handle, a 256-piece plan
  window, and the successful hash-to-publication transition.
- `crates/rstorrent-engine/src/{piece_picker,swarm}.rs` owns specific-piece
  reservation, rarest-first ordinary work, active attempts, strict endgame
  duplication, per-peer observed payload rate, adaptive request timeout, and
  existing active/outstanding resource ceilings.

The concrete boundary improvement is to generalize the active upload-only
verified read seam into a logical active-content read owner without leaking
mutable storage. The pure demand overlay belongs beside scheduling state, not
inside HTTP, application views, or `SelectiveStorage`.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure geometry/demand | Checked file/range-to-piece mapping for exact boundaries, shared-file pieces, padding neighbors, zero length, suffix/tail ranges, maximum accepted piece length, one-byte pieces, overflow, compact intervals, overlap merge, round-robin fairness, update/remove, late generation, seek replacement within one lease, independent requests, and restoration to ordinary policy. |
| Pure scheduler | Current before ahead before ordinary; specific-piece admission; no durable Skip override; bounded candidate traversal; untouched-plan/request preemption; retained partial progress; best-peer queue estimates and fallbacks; slowest-decile safety; two-second horizon; rate-credit deferral; one adaptive distinct-peer duplicate; first-response cancel; late payload; hash failure/retry; and exact resource counters. |
| Scripted runtime | Head/tail probes, full and range waits, out-of-order pieces, piece/file boundaries, corruption then retry, stalled and recovering peers, slow productive peers, storage backpressure, active-piece saturation, 1-byte and maximum-size pieces, low rate caps, concurrent torrents/readers, one-reader cancellation, expiry, pause/archive/Skip/check/removal/root loss/profile replacement/shutdown, completion-to-published handoff, and terminal zero ownership. |
| HTTP/storage | Immediate verified chunks, first-chunk blocking then exact `200`/`206`, subsequent chunk waits, no-read/no-demand `HEAD`, `404` generation revocation, pre-header `503`/`504`, post-header truncation, disconnect, exact path and fake-platform bytes, part-file boundary data, no neighboring-file bytes, and body/read/plan/handle high waters. |
| Controlled interoperability | Independently generated multi-file fixture served by RSTorrent while pinned libtorrent supplies demanded pieces; scripted player-shaped head/tail/seek/overlap trace; exact response hashes; recorded requested-piece/peer ordering and first-range latency compared qualitatively with the pinned oracle, without requiring timing or architecture parity. |
| Client/platform | Generated Rust/TypeScript/Kotlin contract; browser and Tauri streamable `Open`; existing completed-file path unchanged; no embedded player; Android fake-platform active-read semantics; both Android native ABIs; and a no-window AVD lifecycle/storage smoke if the implementation changes the Android-reachable engine boundary. |
| Repository | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, web generation/typecheck/unit tests, applicable Playwright and Tauri checks, Android builds, and tactical/topic evidence reconciliation. |

Controlled fixtures are independently generated from deterministic bytes. No
public swarm, WAN host, downloadable fixture, visible desktop client, physical
device, or external service is required or authorized. A bounded local pinned-
libtorrent process and no-window AVD are authorized only after implementation
of this tactical is explicitly approved.

## Execution Record

1. Commit `0b6d3a5` activated this source-first tactical, exact bounds,
   ownership maps, reference record, and queue selection without behavior
   change.
2. Commits `c39cfc5`, `0fd052f`, `3e608a2`, and `06478ac` added compact pure
   demand, generation-scoped RAII leases, current/ahead ordering, peer queue-
   time selection, the slowest-decile safeguard, two-second horizon, one
   strict adaptive duplicate, and bounded untouched ordinary preemption.
3. Commits `b68de7d`, `a5b2387`, `1781214`, and `c950365` added active logical-
   file planning, verified waits, progressive 64-KiB HTTP chunks, initial
   error mapping, exact completion handoff, and the final storage-close-to-
   publication race repair.
4. Commits `e31c8d1` and `8b48bfd` added typed `streamable` client eligibility,
   the existing shared `Open` action, and bounded demand/read/body/resource
   observations without adding a stable application view.
5. Commits `f12fabf`, `1985fa8`, and `f90c54a` made fresh loopback gateway
   profiles operational within their network boundary and added the
   deterministic pinned-libtorrent HTTP/wire harness. Commit `efba0de`
   corrected an older choke-reassignment mock that rejected ordinary `Have`
   broadcasts once faster scheduling exposed the valid ordering.

### Controlled interoperability

`tests/interop/incomplete_file_streaming.py` independently generates a
four-file, 13-piece fixture with a 393,549-byte media file and a 32-KiB piece
size. Pinned libtorrent `2.0.13.0` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` seeds at 96 KiB/s through a
TCP capture proxy. RSTorrent serves concurrent head, tail, seek, and overlap
ranges, then an active full `GET` through the same capability while the
torrent completes and publishes.

The retained run returned exact SHA-1 for every range and the full file. Head,
tail, seek, and overlap latencies were 1.937, 2.441, 2.941, and 3.443 seconds;
publication was visible 0.006 seconds after the full body. The 21 post-baseline
requests covered every demanded piece and began `10, 10, 0, 0, 11, 11, 5, 2,
5, 12`; the complete captured order then reached ordinary gap pieces `8, 9,
3`. This is qualitative scheduling evidence, not a latency-parity claim. The
finalized scenario passed three times and joined the seed, proxy, gateway,
response owners, and temporary state.

### Bounds and high waters

- Demand storage stays at eight constant-size leases per torrent and the
  deterministic two-lease test records `streaming_demands_high_water = 2`.
- The exact handoff test records one active body, one active streaming lease,
  one active read, 128 demanded bytes read, 64 demanded bytes marked served,
  two per-lease publication handoffs, and terminal zero active bodies, reads,
  and leases. Its deliberate 200-ms publication gate proves a read survives
  the closed-planner window without changing generation or URL.
- The controlled four-reader trace remains inside the declared 16 global,
  four-per-capability body, eight-read, eight-per-torrent demand, 16 active-
  plan, and 40-handle ceilings. The Android AVD retained a 7/40 storage-handle
  and 2/16 pending-request high water with exact cleanup.
- No dependency, schema, unsafe code, daemon, listener exposure, durable
  priority, automatic lifecycle change, Android HTTP listener, or embedded
  player was added.

### Validation completed

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets -- --test-threads=1`
- web contract generation, typecheck, 247 passing unit tests, production/CSP
  build, and 33 passing Playwright tests with 11 opt-in live tests skipped
- `cargo test -p rstorrent-desktop --all-targets` and matching all-target
  Clippy
- pinned-libtorrent controlled streaming three times, including the active
  full-body publication transition
- Android x86_64 and arm64-v8a release libraries, generated Kotlin, JVM unit
  tests, and debug APK assembly
- one API 34 `product-incomplete-duplex` AVD run with exact cleanup

The first complete serial workspace run exposed the older choke mock described
above; its focused test passed three times after correction, and the complete
workspace passed on both subsequent serial runs. No public swarm, WAN host,
visible desktop client, or physical device was used.

After explicit implementation authorization, ordinary owner-local refactors,
internal names/module layout, independently authored fixtures, adversarial
cases implied by these invariants, generated adapters, conservative bound
tightening, and bounded bug fixes at the same owner may proceed autonomously
in logical commits.

Stop for human direction before adding an external dependency, persistence or
schema migration, nonloopback listener or remote authorization, public/WAN or
physical-device work, automatic lifecycle/selection changes, serving any
pre-verification byte, raising a declared resource bound, adding a player/HLS/
transcoding surface, or materially changing the completed-file HTTP contract.

## Deliberate Non-Goals And Next Boundary

- Embedded playback UI, media-library/catalog work, thumbnails, metadata or
  container probing, codecs, subtitles, HLS, remuxing, and transcoding.
- Whole-file lookahead, global sequential download, persistent streaming
  priority, durable file-selection changes, automatic start/resume/restore/
  unskip, or seeding ratio/time goals.
- Multipart ranges, uploads, stable links, LAN/public media serving, port
  mapping, accounts, friend sharing, relay delivery, or remote access.
- A daemon, native host, companion server, REST control plane, socket proxy,
  arbitrary path server, or storage API exposed to HTTP.
- Android HTTP serving, streaming `ContentProvider`, Compose player, iOS
  presentation, extension integration, or a new product setting.
- Absolute media presentation deadlines inferred from byte offsets, more than
  one urgent duplicate, snub/parole policy beyond existing integrity state, or
  measured micro-optimization unrelated to the stopping condition.

The next boundary after this tactical is evidence-driven. A real embedded
player could supply actual presentation deadlines and bounded seek semantics;
a media catalog could decide which files deserve playback presentation; and a
separate Android tactical could design a native streaming provider. None is
implied by verified incomplete HTTP fulfillment.
