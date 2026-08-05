# Application View API

Topic: `application-view-api`

Status: The bounded leased view-set, authenticated JSON polling adapter,
generated TypeScript/schema contract, pure reducer, and lifecycle controller
are implemented by
[`033-headless-view-set-foundation.md`](../tactical/033-headless-view-set-foundation.md).
Tactical `034` implements the per-application Zustand store and React
inspection model against a deterministic adapter. Tactical `035` adds stable
Rust torrent and active-peer projections, semantic responsive view selection,
independently reaped leases, and browser-suspension recovery through the live
React adapter. Tactical `041` adds the complete selected-torrent file catalog,
distinct stored and verified progress, and a separately bounded large-snapshot
path. Tactical `043` adds the selected torrent's authoritative tracker
lifecycle as bounded keyed rows. Tacticals `044`--`045` add global storage
pressure and selected-torrent verified/active piece views, including keyed
attempt diffs and fresh-epoch bitmap replacement. Tactical `048` makes pull
and stream interchangeable TypeScript delivery capabilities and implements
acknowledged Tauri Channel delivery against the same leased view sets,
controller, reducer, and Zustand transaction. The existing Tactical `008`
subscriptions remain compatibility adapters. Tactical `060` now implements
one multiplexed WebSocket for every ordinary browser call and view stream,
with HTTP retained only as an explicit loopback diagnostic. Binary encoding
remains deferred, and no stable public remote wire compatibility is claimed.
Tactical `049` completes the diagnostics specialization: hierarchical
categories, structured bounded subjects and fields, capture interest,
separate source/delivery/local loss semantics, and one ordered console over
the existing view-set path.
Tactical `051` adds an optional, defaulted, closed `PeerFlagView` set to active
peer rows. Rust computes semantic connection state while clients retain
presentation-only glyph ownership; old v1 producers that omit the field remain
accepted through a bounded typed-fact fallback.
Tactical `056` completes the existing nullable active-peer `client_name`
projection from the handshake peer ID. The Rust protocol utility owns bounded
fingerprint parsing, the application view owns capability state, and clients
continue to render the generated field without a competing parser or a
contract-version change.
Tactical `057` adds a retained producer-throughput matrix over the exact
production view combinations. It changes no semantic API, but makes observer
cost, serialized update volume, queue high water and reset recovery explicit
hardware-profile evidence.
Tactical `081` extends tracker rows with truthful metainfo source and
HTTP/HTTPS configured-but-unsupported state while keeping credential-bearing
URL components out of the projection. It also replaces the former complete
4,096-file and 32-tracker snapshot assumptions with bounded pages carrying a
total count and stable offset, so accepted libtorrent-scale catalogs do not
require one rendered snapshot or whole-catalog patch. The existing 16-MiB
snapshot ceiling is a page bound. It does not make the view or
diagnostics the tracker configuration authority.
Tactical
[`084`](../tactical/084-persisted-client-connection-and-seeding-settings.md)
adds one small complete-replacement client-settings projection to the existing
always-present torrent-list view. It distinguishes configured intent from
active/effective listener and limit state without adding a named view, lease,
queue, task, or persisted runtime observation.
Completed Tactical
[`086`](../tactical/086-long-lived-torrent-peer-runtime.md) changes no public
view kind: it moves ordinary peer publication into a long-lived per-torrent
owner and now populates the existing Peers/Swarm contract from routed incoming
seed connections. Authenticated gateway interoperability follows pinned
libtorrent and RSTorrent rows through transfer, exact removal, pause, and
joined terminal cleanup.

## Purpose And Scope

The desktop inspection surface needs a recoverable, typed local replica of the
application state currently relevant to one UI. The same semantics must work
through an in-process Tauri adapter, a headless browser gateway, periodic
polling, and low-latency streaming without making transport or encoding
choices part of the application model.

This topic owns:

- named application views and their client-selected identities;
- view-set ownership, lifecycle, cursors, leases, and resource bounds;
- snapshot, keyed-diff, reset, and recovery semantics;
- the separation between semantic API, delivery transport, and wire codec;
- generated TypeScript types and runtime schema validation;
- provisional remote URLs and in-process equivalents; and
- the headless evidence required before the React inspection UI depends on
  the contract.

[`application-control.md`](application-control.md) owns commands, durable
application revisions, and the application-service authority.
[`application-connection-architecture.md`](application-connection-architecture.md)
owns multiplexed WebSocket calls and view attachments, IPC adaptation and
future encrypted relay layering.
[`client-view-delivery-policy.md`](client-view-delivery-policy.md) owns
client-selected cadence, low-bandwidth and background policy, and the evidence
required to calibrate those choices.
[`web-ui-design.md`](web-ui-design.md) owns React, Zustand, presentation state,
and rendering. [`client-surfaces.md`](client-surfaces.md) owns the platform
adapters and their lifecycle.

This topic does not authorize a production remote daemon, LAN listener,
account system, TLS or pairing design, payload transport, or public
compatibility promise. It also does not define archive, deletion, queueing, or
label command semantics merely because future views may present those
concepts.

## Terminology

Use **view set**, not **view session**, for the short-lived server-side state
that supports one UI instance. `Session` is already overloaded by the
BitTorrent engine, application-service lifetime, and remote authentication.

A view set is:

- a set of named projections currently requested by one client;
- the materialized producer state needed to calculate their diffs;
- one bounded pending-update accumulator;
- an epoch and opaque consumption cursor; and
- a leased, explicitly closable application resource.

A view set is not authentication, durable user data, or the engine session.
Its identifier locates a resource but never grants authority to use it.

## Accepted Architecture

There is one semantic API and several adapters:

```text
Rust application-view owners
          |
    snapshots and typed diffs
          |
       view set
          |
  +-------+------------------+
  |                          |
pull next updates      stream updates
  |                          |
HTTP / Tauri call      WebSocket / Tauri Channel
  |                          |
JSON first             JSON, then optional binary
  +-------------+------------+
                |
        validated UpdateBatch
                |
        pure TypeScript reducer
                |
         local client store
```

Polling and streaming consume identical update batches. JSON and a future
binary codec decode into identical TypeScript data transfer objects. Adding a
delivery mechanism or codec does not create a second application API.

The existing `ViewHub`, named projections, typed patches, per-subscriber
queues, epochs, sequence continuity, coalescing, and reset behavior are useful
foundations. The implemented successor aggregates the views visible to one
client into one view set and adds pull consumption. It evolves those owners
without making the earlier one-subscription-per-projection proof the future
public shape.

## Owner And Task Map

| Owner | Responsibility | Lifetime and termination |
| --- | --- | --- |
| Application view hub | Current projection source state and fan-out into view sets | One application-service instance; closes all view sets during joined service shutdown |
| View-set lease reaper | One timer that removes client-silent view sets independently from later requests | One application-service instance; cancellation and awaited join during service shutdown |
| View set | Desired view specifications, per-view materialization, epoch, cursor, and bounded accumulated updates | Explicit close, idle-lease expiry, authenticated remote disconnect policy, or application shutdown |
| Transport adapter | Authentication context, framing, request bounds, and delivery of semantic calls | Tauri webview, HTTP request, or WebSocket connection lifetime; never owns application truth |
| TypeScript `ViewController` | Desired view set, current ID/epoch/cursor, one update consumer, reconnect/backoff, validation, and cancellation | One web application instance; closes or abandons its leased Rust view set |
| Pure client reducer | Atomic snapshot and patch application with continuity checks | No I/O or task lifetime |
| Zustand store | Materialized view values and presentation state consumed by React | One web application instance; contains no sockets, promises, or task handles |

Every polling loop, long poll, or stream pump belongs to `ViewController` and
has an abort signal plus observable termination. React components do not own
transport loops merely because they display a view.

## View-Set Lifecycle

Opening a view set accepts a bounded list of `ViewSpec` values. The service
allocates an opaque `view_set_id`, creates an epoch and cursor, installs the
per-view accumulators, and returns coherent initial snapshots:

```text
open_views(specs)
  -> view_set_id
  -> epoch
  -> cursor
  -> lease duration
  -> initial snapshot updates
```

For a remote adapter the identifier should contain enough random entropy to
resist guessing. The adapter associates it with the authenticated principal
and checks ownership on every operation; possession of the ID alone is not
authorization. An in-process Tauri adapter uses the same logical identifier
without introducing HTTP authentication.

The TypeScript controller keeps the identifier, epoch, and last applied cursor
in memory. A temporary transport interruption reuses them. A browser reload
opens a new view set; initially the identifier is not persisted in local or
session storage. Separate tabs receive separate view sets so their selections,
filters, and delivery pressure cannot interfere.

View sets live in process memory, not SQLite. Application restart invalidates
them and recovery begins from fresh snapshots. Abandoned sets expire after a
five-minute idle lease. One application-owned reaper must remove a set no
later than one bounded reaper interval after expiry, release its retained
state, and wake waiters without requiring another client operation. Only an
accepted client open, desired-view update, or new update request refreshes
`last_client_activity`; engine publication, coalescing, queue wakeups, replay,
and response generation never keep an abandoned set alive. Tactical `035`
replaced the foundation's opportunistic pruning with this active owner and
made its cancellation and join part of application shutdown.

Updating the requested specifications changes the existing view set. Added or
changed view snapshots enter its single ordered update feed; the configuration
response is only an acknowledgement. It does not create a competing second
state response. Removed views release producer state and produce an explicit
`view_removed` update at the correct cursor boundary.

The desired set follows the UI that is actually visible. A phone torrent
detail can request one selected summary and one active detail without retaining
the torrent list; a library surface can request the list without torrent
details. Removing a view evicts its materialized application data after the
ordered removal. Presentation navigation context is separate and may remain.

Only one update consumer drains a view set. Poll mode permits one in-flight
`next_updates` operation; streaming mode attaches one stream at the retained
cursor. Switching delivery modes detaches the old consumer before attaching
the new one.

Delivery is not destructively acknowledged merely because bytes were written
to a socket. In pull mode, the next request's `after` cursor acknowledges that
the preceding batch was validated and applied. Repeating a request with the
same `after` value returns a safe replay while it remains retained. In stream
mode, the adapter acknowledges the prior yielded cursor only when its consumer
requests the next item, using an explicit transport acknowledgement where
needed. Unacknowledged updates remain bounded; expiry or overflow produces a
reset rather than possible silent loss.

## Semantic Operations

The transport-neutral interface is conceptually:

```ts
interface ApplicationClient {
  hello(): Promise<ApiHello>;
  dispatch(request: CommandRequest): Promise<CommandResponse>;
  openViews(
    specs: ViewSpec[],
    options?: OpenViewsOptions,
  ): Promise<OpenViewsResponse>;
  updateViews(viewSetId: ViewSetId, specs: ViewSpec[]): Promise<void>;
  nextUpdates(
    viewSetId: ViewSetId,
    after: Cursor,
    options?: { maxWaitMs?: number },
  ): Promise<UpdateBatch>;
  streamUpdates?(
    viewSetId: ViewSetId,
    after: Cursor,
  ): AsyncIterable<UpdateBatch>;
  closeViews(viewSetId: ViewSetId): Promise<void>;
  close(): Promise<void>;
}
```

Names and exact Rust representations remain subject to the tactical, but the
operation boundaries and ownership are accepted.

The `AsyncIterable` adapter owns streaming acknowledgements. A normal
`for await` consumer applies one yielded batch before requesting the next, so
the adapter can acknowledge the prior cursor at that boundary without exposing
transport frames to React or the store.

`hello` reports the supported API range, encodings, delivery modes, named view
and command capabilities, and resource limits. API compatibility, delivery
mode, and encoding negotiation are separate dimensions.

Its first shape is conceptually:

```ts
interface ApiHello {
  api: { current: 1; minimum: 1 };
  encodings: Array<"json" | "cbor">;
  deliveries: Array<"poll" | "long_poll" | "stream">;
  capabilities: string[];
  limits: ApiLimits;
}
```

The actual lists report what the running adapter supports; advertising a name
does not imply that its deferred codec or delivery implementation exists.

Views are named projections rather than an arbitrary field-query language:

```ts
type ViewSpec = {
  view_id: ViewId;
  delivery?: { min_interval_ms: number };
} & (
  | { type: "session_summary" }
  | { type: "torrent_list"; category?: CategoryId }
  | { type: "torrent_detail"; torrent_id: TorrentId }
  | { type: "torrent_peers"; torrent_id: TorrentId }
  | { type: "torrent_files"; torrent_id: TorrentId }
  | {
      type: "diagnostics";
      torrent_id?: TorrentId;
      filter: DiagnosticFilter;
    }
);

interface OpenViewsOptions {
  requested_queue_bytes?: number;
}
```

The client chooses `view_id` as a stable reducer and store key such as
`library`, `selected-peers`, or `logs`. The service validates uniqueness,
view-specific arguments, authorization, and bounds. Named projections keep
exposure and resource policy reviewable and can evolve independently.

The optional interval controls producer emission and streaming coalescing, not
browser paint scheduling. Queue allowance is a whole-view-set request capped
by server policy; the opening response reports the effective interval and
queue bounds. A client can request a deliberately smaller queue for low-memory
operation or reset testing but cannot raise the advertised server ceiling.

## Update Continuity

One delivered batch has this semantic shape:

```ts
interface UpdateBatch {
  api_version: 1;
  view_set_id: ViewSetId;
  epoch: DecimalU64;
  base_cursor: Cursor;
  cursor: Cursor;
  durable_revision: DecimalU64;
  updates: ViewUpdate[];
}
```

Epoch, cursor, and durable application revision have distinct meanings:

- `epoch` identifies the lifetime of the volatile view-set accumulator;
- `base_cursor` is the last batch on which this batch depends;
- `cursor` is the opaque position after atomically applying the batch; and
- `durable_revision` is the latest coherent application command/persistence
  revision observed while deriving it.

The client applies a batch only when its view-set ID and epoch match and its
`base_cursor` equals the last applied cursor. The complete batch is one atomic
reducer/store transaction. A mismatch, expired cursor, overflow, incompatible
epoch, or invalid patch requires an explicit reset and fresh snapshot. A reset
should affect only the invalid view when producer ownership permits; whole-set
reset is the safe fallback.

Cursors are opaque decimal strings even if the first implementation uses a
monotonic integer. Clients compare equality and return them; they do not add,
sort, or infer retention from them.

Per-view updates use a small closed vocabulary:

```ts
type ViewUpdate =
  | { view_id: ViewId; type: "snapshot"; snapshot: ViewSnapshot }
  | { view_id: ViewId; type: "patch"; patch: ViewPatch }
  | { view_id: ViewId; type: "view_removed" }
  | { view_id: ViewId; type: "reset_required"; reason: ResetReason };
```

`view_removed` retires the materialized client value at an ordered cursor. A
later reuse of the same `view_id` begins with a new snapshot before patches.

## Diff Semantics

Do not use general JSON Patch. Paths couple clients to object layout, make
projection evolution brittle, and provide poor type and resource bounds.
Use named, projection-specific snapshots and patches.

Collection views initially use complete-row keyed upserts and explicit
removals:

```ts
interface PeerListPatch {
  type: "torrent_peers";
  upsert: PeerRow[];
  removed: PeerConnectionId[];
}
```

Full-row upserts are easier to validate, reduce, coalesce, and encode than
field-level patches. Field masks are a later measured optimization. Stable row
identity is mandatory. A peer row uses a connection-lifetime identity rather
than only address and port so reconnects remain distinct.

The `torrent_peers` collection contains every currently active connection
generation, including transport connecting, protocol handshaking, connected,
and disconnecting work. A row is removed after its task and owned engine state
finish cleanup; disconnected history is not retained in this current-state
view. The future `torrent_swarm` collection instead projects all retained peer
records, including idle, backed-off, failed, and banned records.

Projection rules are:

| Data shape | Patch and coalescing rule |
| --- | --- |
| Torrent, peer, file, and tracker collections | Full-row keyed upsert plus removed IDs; repeated upserts retain the newest complete row |
| Selected summary or other singleton | Replace the value |
| Rates, counters, current peer state | Latest value may conflate within the delivery interval |
| Verified pieces and bounded current piece activity | Typed range or bitmap additions, clears, and replacements |
| Diagnostics and ordered events | Ordered append with explicit retained-history and delivery-gap metadata; never silently latest-value conflated |
| Queue or priority order | Explicit sortable fields; array position is not meaning unless the projection states it |

Tactical `049` specializes the diagnostics row without creating a second
delivery system. Records carry monotonic decimal sequence, bounded wall-clock
timestamp, severity, forward-compatible hierarchical category, stable code,
optional torrent identity, human message, bounded subjects, and bounded typed
fields. A diagnostics view's profile, severity, category prefixes, and
optional pinned torrent are producer capture and delivery interest. Display
search and filtering remain client presentation state.

The application history is bounded to 2,048 records and 2 MiB; one record is
at most 4 KiB and one append patch at most 128 records or 128 KiB. Snapshot
retention identifies source eviction, view-set reset identifies delivery loss,
and the client separately reports local eviction. These meanings are not
collapsed into a generic dropped count. The generated validator and pure
reducer preserve this structure identically for polling and Tauri streaming.

Within one accumulated collection diff, a later removal wins over an earlier
upsert; a still-later upsert may represent a genuinely new lifecycle only when
its stable identity permits that interpretation. Producers must distinguish
empty, unavailable, unsupported, disconnected, stale, and reset-required
states rather than presenting all of them as an empty list.

## Polling And Streaming

The first remote delivery is bounded periodic JSON polling. An active visible
UI may begin around 250--500 milliseconds, slow down while hidden, issue an
immediate pull after a command or view change, and keep only one pull in
flight. Exact cadence is client policy and should be measured.

Browser suspension may outlive the Rust lease. An unknown, closed, or expired
set marks retained requested views stale/reconnecting, opens one replacement
set from the latest semantic desired views, and atomically replaces the stale
materialization from its coherent initial snapshots. Patches from the new set
are never applied over the old set or epoch. Visibility, page-show, and online
events may wake polling early, but correctness does not depend on browser
timers running while suspended.

The same operation may later permit a bounded `max_wait_ms` for long polling.
A WebSocket or Tauri Channel attaches to the same view set and cursor and emits
the same `UpdateBatch`. If retained history cannot satisfy the cursor, it emits
or returns a reset rather than silently starting at the tail.

Low-latency delivery does not put `requestAnimationFrame` into the Rust or wire
contract. The server may deliver current-state changes immediately with
bounded latest-value coalescing; the client store and renderer decide when to
paint. Ordered events retain their separate loss and backpressure semantics.

## Provisional Remote Routes And Connection

The remote adapter reserves this shape for the first tactical:

```text
GET    /api/v1/hello
POST   /api/v1/commands
POST   /api/v1/view-sets
PUT    /api/v1/view-sets/{id}/views
GET    /api/v1/view-sets/{id}/updates?after=...&wait_ms=...
DELETE /api/v1/view-sets/{id}
GET    /api/v1/connect
```

`/api/v1/connect` is the preferred provisional versioned WebSocket upgrade. It
authenticates once and carries typed hello, command, view-set creation/update/
close, attachment, batch and exact cursor-acknowledgement frames directly. It
does not require an HTTP-created view set, and one socket multiplexes every
bounded view attachment for that client/backend connection. The accepted
framing, ownership, resume and relay-compatible layering live in
[`application-connection-architecture.md`](application-connection-architecture.md).

The exact remote authentication mechanism remains outside this topic. Tactical
`060` deleted RSTorrent's legacy loopback `/control` WebSocket after migrating
its useful origin, authentication, dispatch, bounded-delivery and shutdown
evidence to `/api/v1/connect`. JSTorrent's unrelated I/O-daemon endpoint with
the same path is outside this repository and decision.

The local Tauri product maps these semantic calls to commands and later
Channels. It does not bind a loopback port or serialize through HTTP merely to
share the browser adapter's URLs.

An explicit unauthenticated development mode may be used for initial browser
bring-up when it is impossible to bind beyond loopback, retains exact Origin
and resource checks, assigns opaque owners, and uses a temporary or explicitly
selected profile. The view-set ID remains non-authorizing. This does not alter
the authenticated adapter or establish production remote access.

Browser navigation URLs such as `/library/:category` and
`/torrents/:torrent_id/:tab` are presentation concerns and do not mirror the
API route layout.

## TypeScript Generation And Runtime Validation

Rust records and enums remain the canonical semantic and wire-type source.
Continue using `ts-rs` with Serde compatibility for deterministic TypeScript
declarations. Add `schemars` derivation for deterministic JSON Schema from the
same Rust DTOs.

The web package should expose:

```text
src/api/
  generated/
    v1.ts
    v1.schema.json
  client.ts
  codec.ts
  index.ts
```

Generated artifacts are never hand edited. Components import the stable
handwritten barrel rather than generator output directly. One repository
command regenerates declarations, schema, and cross-language fixtures; CI
fails on drift.

Generated schema validates structural shape at the untrusted JSON boundary.
Small handwritten semantic validators retain limits that schemas cannot
express clearly, such as canonical ranges, safe lengths, cross-field
relationships, and negotiated capabilities. Unknown object fields are
accepted for additive forward compatibility. Unknown closed command, patch,
or control variants fail unless a negotiated capability defines their safe
handling.

The current handwritten TypeScript validator already demonstrates the drift
risk: its storage-state list omits Rust's `prepared` and `needs_repair`
variants. The first contract tactical should replace that duplicated
structural enumeration rather than repair only this instance.

Portable DTO rules are:

- JSON-safe bounded integers may remain numbers;
- revisions, cursors, timestamps, byte counters, and other `u64` values use
  canonical decimal strings;
- wire values do not expose JavaScript `Date`, `Map`, or `BigInt`;
- byte strings, ranges, and bitmaps have explicit representations;
- Rust implementation enums do not leak accidentally into the public tagged
  shape; and
- unknown additive object fields do not invalidate an otherwise compatible
  message.

## Compatibility And Codecs

The major API version appears in remote URLs and top-level envelopes. Within
v1, fields may be added when optional or safely defaulted, but required fields
cannot be removed, renamed, retyped, or silently change meaning. New closed
control variants require capability negotiation or a major-version boundary.

JSON is the initial codec. Transport implementations hide encoding behind a
codec boundary and pass decoded DTOs to the same reducer:

```ts
interface ApiCodec {
  readonly encoding: "json" | "cbor";
  encodeRequest(value: unknown): string | Uint8Array;
  decodeResponse<T>(value: string | ArrayBuffer): T;
}
```

Binary encoding is a measured v1 optimization, not automatically API v2. CBOR
is the leading candidate because it can preserve the same data model; no
binary dependency or format is selected until payload, CPU, allocation, and
latency measurements justify it.

## Commands And Observable State

Commands and views retain separate responsibilities. A command carries its
request identity and optional expected durable revision. Its eventual success
response should contain a command-specific result, the resulting durable
revision, and affected identities when useful, rather than returning the
entire service snapshot after every mutation. The view feed supplies the
authoritative resulting state, and the controller may request an immediate
pull after a command.

Remote retry safety may later use bounded request-ID deduplication in addition
to explicitly idempotent commands. View cursors are not command receipts.
Diagnostics are not parsed to infer command success or product state.

## Initial View Breadth

The first useful contract progression is:

1. application/session summary and advertised capabilities;
2. torrent-list summary;
3. selected-torrent summary;
4. selected-torrent peers; and
5. bounded diagnostics.

Disk activity is implemented as one global `session_disk` view by
[`disk-and-piece-inspection`](disk-and-piece-inspection.md) and Tactical `044`,
including bounded pipeline state, keyed active piece attempts, lease recovery,
and terminal owner cleanup.
The selected-torrent piece activity contract is generalized and rendered by
Tactical `045`. Files is implemented by Tactical `041` and trackers by
Tactical `043`. Tactical
[`064`](../tactical/064-registry-backed-swarm-inspection.md) implements
`torrent_swarm`, and Tactical
[`066`](../tactical/066-smooth-session-speed-history.md) implements
`session_speed`; Tactical
[`065`](../tactical/065-dht-observatory.md) implements `session_dht`. Swarm is
keyed registry state with
active, inactive, and torrent-missing catalogs; DHT is one bounded latest
session observation containing exact 160-bucket occupancy/freshness and at
most 16 lookup-convergence summaries; Speed is a range-selected bounded session
history with coalescible selected-tier replacements and server-owned coarse
retention, so a fresh client receives its requested tier rather than the full
database.
Unsupported views must report unsupported or unavailable explicitly rather
than fabricate empty data.

## Implemented Foundation

Tactical `033` establishes these concrete v1 choices:

- 32 live view sets per application and 8 per authenticated adapter owner;
- 16 named views per set and 64-byte conservative ASCII view IDs;
- 16--512 KiB whole-set queues, defaulting to 256 KiB;
- 512 KiB steady-state queues, a separately retained 16 MiB coherent snapshot
  ceiling and HTTP response reader, 64 KiB requests, 20-second waits,
  60-second maximum delivery intervals, and a five-minute idle lease;
- one emitted but unacknowledged batch, exact replay until the next cursor,
  monotonic cursors across epoch reset, and one active consumer;
- whole-set overflow reset followed by coherent snapshots;
- authenticated loopback JSON pull routes plus the retained Tactical `008`
  WebSocket adapter;
- Rust-derived TypeScript declarations and JSON Schema, Ajv structural
  validation, semantic bounds, and a pure keyed reducer; and
- a bounded Fetch client and `ViewController` with abort, retry, lease reopen,
  immediate post-command pulls, joined close, and no task handles in state.

The controlled libtorrent proof observed a real magnet add, list upsert,
selected summary and piece views, 40,000 requested/received/stored bytes,
three verified pieces, complete publication, exact payload SHA-1, explicit
view-set close, joined gateway shutdown, and temporary cleanup. Deterministic
tests additionally cover owner isolation, concurrent-consumer rejection,
lost-response replay, failed-reduction non-acknowledgement, overflow recovery,
delivery intervals, expiry, generated drift, and shutdown wakeup.

This is a local application boundary and automation seam, not a production
remote-control security or compatibility claim. Per-view retained histories,
per-view overflow reset, browser WebSocket delivery, a binary codec, and
stable public compatibility remain later layers. Tactical `048` implements
stream delivery for Tauri with one explicit post-application acknowledgement
and exact-generation resource cleanup. The
transport-independent Zustand/React presentation foundation and demo adapter
are complete in Tactical `034`; stable peer rows and the Rust-to-React polling
adapter are complete in Tactical `035`; live Files is complete in Tactical
`041`; and live Trackers is complete in Tactical `043`.

## Live Swarm Extension

Tactical `064` implements `torrent_swarm` as the selected torrent's complete
bounded `PeerRegistry` projection, independently of `torrent_peers`. The
generated contract carries closed catalog/source/eligibility/failure
vocabularies, monotonic ages and retry duration, dial/failure history, and
integrity posture. `peer_record_id` is the stable row and patch key. A summary
and its row set always share one capture; Rust and browser validation reject
more than 1,000 records or inconsistent category totals.

Registry mutations and retry-deadline transitions publish through the existing
task-free engine activity boundary. Terminal inactive publication clears rows
only after joined tracker and torrent-peer cleanup. View-set interest affects
only delivery: lease expiry/reopen reconstructs the complete current registry,
and an empty active-connections patch cannot erase retained Swarm rows. The
shared reducer handles keyed updates and terminal removals without joining the
two projections in the browser.

## Live Peer Extension

Tactical `035` implements `torrent_peers` as complete keyed rows for all and
only active connection generations. The generated Rust/TypeScript/schema
contract carries direction, transport, lifecycle, endpoint, source,
capability, request-window, transfer, availability, and cleanup facts without
placing payloads or peer history on the boundary. The application mapper
publishes targeted torrent and peer changes without cloning the complete
torrent collection.

Tactical `051` replaces the web-only opaque flag string with the generated
`peer_flags` semantic set. The application projection currently derives only
incoming direction, uTP transport, negotiated extension facts, and transfer
choke relationships whose nullable inputs are known. Encryption, parole,
snubbed, seed, upload-only, hole-punch, optimistic-unchoke, and connection
endgame values are reserved vocabulary rather than fabricated observations.
The additive field defaults empty and is omitted when empty, preserving the
accepted v1 compatibility shape.

Tactical `056` makes the previously reserved `client_name` field operational.
Once a handshake peer ID is known, the application projection identifies
registered BEP 20 Azureus, Shadow, Mainline, and a bounded set of precisely
specified legacy fingerprints. A recognized value is `available`; missing or
unrecognized evidence is `unavailable`, not `unsupported`. The peer-controlled
fingerprint is a display hint only and cannot affect peer identity, trust, or
scheduling. The existing nullable field and consumer shape do not change.

Completed Tactical
[`086`](../tactical/086-long-lived-torrent-peer-runtime.md) uses this existing
contract as the proof of a corrected engine/session lifetime. A routed
incoming connection is now one ordinary keyed active generation. Its current
endpoints, peer identity/client hint, extension/metadata negotiation,
interest/choke, upload queue, exact payload total/rate, and optimistic grant
populate already-defined nullable fields and flags. Its accepted remote
ephemeral endpoint also appears as a bounded non-connectable `incoming` Swarm
record. Unknown pre-routing sockets remain session diagnostics rather than
torrent rows, and disconnect removes the Peers row only after joined
connection cleanup. Application and leased-view tests cover the full row,
Swarm retention, disconnecting upsert, and exact removal; the unchanged
generated contract passes through the live TypeScript adapter. Authenticated
gateway evidence follows simultaneous pinned libtorrent and RSTorrent rows
through nonzero upload, exact removal, inactive pause, and joined shutdown.

`InspectionApplication` now accepts semantic desired views. The live adapter
maps them to Rust specifications while responsive navigation can retain only a
phone detail, a library, or a wide list-plus-detail set. Removed projections
are evicted rather than becoming an accidental complete engine mirror. Each
materialization retains the distinct not-requested, loading, ready,
unavailable, unsupported, and stale states.

One application reaper checks all sets at a bounded interval. Only accepted
client operations renew `last_client_activity`; engine publications never do.
The advertised hello lease reflects the configured application lease. A
headless controlled run held browser operations past a 500 ms test lease,
observed server expiry and visible stale state, then proved recovery through a
second view-set identity and fresh epoch. The production default remains five
minutes with a reaper interval no greater than five seconds.

The development browser gateway can opt into unauthenticated control only on
an OS-assigned loopback listener with one exact loopback Origin. Opaque owner
identities still isolate view sets. This mode is an automation and local
bring-up boundary, not a production remote-access posture; bearer mode remains
the ordinary gateway configuration.

## Live Files Extension

Tactical `041` implements `torrent_files` as a complete ordered catalog for
the selected torrent. Stable file IDs are metainfo indices. Each row carries
validated relative path components, exact decimal length and offset, inclusive
piece span, wanted/skipped selection for ordinary files, independent padding
identity, and exact Done and Verified byte counts. The snapshot also carries
one filesystem content base; the TypeScript adapter derives optional absolute
storage paths without repeating the base in every wire row. Capability-backed
storage reports no fabricated filesystem path.

The runtime-independent file-progress owner shares immutable catalog geometry
and updates only rows intersected by stored, verified, or hash-failed piece
ranges. Stored-block overlap is deduplicated, verification is idempotent, hash
failure removes only unverified bytes, and durable restart reconstructs
Verified from the checkpoint while conservatively dropping transient Done.
Metadata arrival and catalog replacement use coherent snapshots; steady
progress uses coalesced complete-row keyed patches at a client-requested
250 ms minimum interval.

A legal 4,096-row long-path fixture encodes to 1,481,877 bytes, above the
ordinary 256 KiB default queue but below the advertised 16 MiB snapshot bound.
The view set retains that coherent initial snapshot separately while later
small patches remain governed by the 512 KiB steady-state ceiling. Gateway and
browser readers enforce the same 16 MiB maximum; an oversized response fails
explicitly rather than truncating.

Tactical `081` replaces that whole-catalog contract with offset pages of at
most 1,024 rows. Every snapshot carries the requested offset and limit, the
full total count, and an optional next offset. Steady patches include only
rows in the requested page; changing pages is an ordinary view-spec update
that yields a coherent snapshot. Clients do not infer total accepted file or
tracker count from page length.

Responsive interest requests Files only while that tab is visible and evicts
the materialization after the ordered view removal. A phone detail does not
retain the library. Browser suspension follows the existing stale/reopen
contract: the controlled 500 ms lease proof replaced the expired set and
restored all 122 rows from a fresh epoch while the engine continued.

## Live Trackers Extension

Tactical `043` implements `torrent_trackers` as complete keyed rows derived
from the deterministic engine schedule. The projection carries configured
identity, source and transport, active/inactive lifecycle, current and next
action, attempt and failure counts, accepted response statistics and interval,
monotonic outcome ages and deadline, and bounded failure context. It does not
derive state from diagnostics or retain a second tracker state machine.

Tactical `081` makes this a paged projection and adds metainfo versus magnet
source, original tier, and configured transport/capability state. UDP rows may
be active; retained HTTP and HTTPS rows remain visible as unsupported and
credentials are redacted. The full retained tracker catalog is not constrained
by the manager's independent eight-operation UDP concurrency ceiling.

Same-catalog durable updates preserve live tracker state. Restart reconstructs
configured inactive rows from the magnet without pretending volatile response
history survived. The selected Trackers tab alone requests the view at a
250 ms minimum delivery interval; leaving the tab evicts it. The adapter maps
the delivered deadline to a local wall-clock target once, and the React table
updates countdown text without backend timer patches.

The torrent summary also carries an additive optional configured-tracker
count derived from the same bounded tracker model. Navigation can therefore
show a stable count while the detailed tracker projection is not requested;
it does not retain evicted tracker rows or create another tracker authority.
Older v1 producers may omit the field, which remains distinct from a known
zero configured trackers.

The controlled browser proof observed an intentionally delayed announce in
flight, accepted exact peer/seeder/leecher counts and a reannounce deadline,
completed verified content, removed the active peer row, and joined the
gateway, tracker, and libtorrent seed. Generated Kotlin and UniFFI consumers
compile while Android explicitly ignores this desktop-only presentation view.

## Validation And Evidence

The completed foundation evidence proves:

- deterministic opening, specification changes, per-view snapshots, keyed
  upserts, row removals, view removal and re-add, atomic batches, and close;
- independent view sets with different desired projections and consumption
  speeds;
- one-consumer enforcement and polling cursor continuity;
- lost-response replay and acknowledgement only after client reduction;
- queue coalescing, exact high-water accounting, whole-set overflow reset, and
  fresh-snapshot convergence;
- cursor mismatch, monotonic epoch reset, lease expiry and client reopen,
  application shutdown, and prompt joined task termination;
- authenticated ownership, request/frame bounds, and a view-set ID that is not
  treated as a credential in the loopback gateway;
- generated TypeScript plus JSON Schema drift checks and independently tested
  semantic limits;
- a pure TypeScript reducer and headless client that can materialize the same
  snapshots and diffs through polling; and
- one controlled libtorrent-seeded download observed through that client from
  add through verified publication and clean shutdown.

Tactical `084` additionally proves immediate settings replacement after an
atomic mutation, no substitution of configured intent for active state, and
fresh initial active/effective/listener observation after durable reopen. Four
production-browser generations converge through the existing torrent-list
view across disabled mismatch, automatic listening, fixed bind failure, and
repaired automatic listening without another view owner or transport path.

Tactical `040` now supplies actual torrent lifecycle evidence in addition to
the earlier synthetic `removed` diffs. Archive, removal stage, and managed-data
deletion availability are complete torrent-summary fields. Final catalog
deletion produces the ordinary keyed removed ID; an intermediate or failed
cleanup remains an upsert, so reducers do not infer deletion from command
success or diagnostics.

Tactical `041` adds a controlled libtorrent 2.0.13.0 multi-file proof. The
production web build received 26,731 bytes of multi-block metadata, exposed
all 122 files, and made piece zero cross a 7,000-byte nested prefix into the
40,000-byte payload. It observed first Done and Verified bytes at 20,406 and
20,413 ms from Files selection, recovered after deliberate lease expiry, and
displayed 39.0 KiB Done and Verified for the payload at completion. The
harness independently compared both nonempty files, joined the gateway and
seed, and removed its temporary profile and download tree.

Tactical `042` completes the torrent identity row with an optional
`display_name` derived only from successfully parsed, verified durable
metainfo. Metadata arrival and profile reopen use the same derivation. Since
list and selected-summary views already carry complete `TorrentView` rows,
name arrival is an ordinary keyed upsert in both surfaces rather than a new
view or event. The client validates the metainfo component's 255-byte bound
and retains its info-hash fallback when the field is absent.

Tactical `043` adds deterministic tracker snapshot, keyed-patch, catalog
replacement, removal, reset, and lease-recovery coverage. Its controlled
tracker-only browser run observed `announcing` before a delayed response, then
one accepted peer, 37 seeds, 11 leeches, and a 30-minute reannounce deadline
while the same run completed and verified the seeded payload.

Tactical `081` adds deterministic 1,024-row file and tracker page traversal,
page-scoped patches, total/next-offset validation in Rust and TypeScript, and
generated Kotlin/UniFFI high-cardinality coverage. A 374,998-file Android
fixture is represented by one compact wanted range and one 1,024-row page.

Tactical `046` proves Peers removal on pause rather than only completion or
synthetic replacement. A deterministic application run begins from verified
multi-file metadata, holds one real content TCP generation connected, then
pauses through the ordinary command path. The pause receipt follows socket
EOF; the already-open leased view set receives the exact connection ID in a
Peers `removed` patch and a complete torrent-summary row with zero active peer
connections and zero payload rate. No React filter or durable-state refresh
manufactures the removal.

Tactical `057` measured three rotating 1 GiB cohorts on the local Apple M4 Pro
profile. The idle SQLite application median was 177.9 MiB/s and Library alone
was 166.4 MiB/s with zero resets. The ordinary Library-plus-Summary shape was
160.2 MiB/s but recovered from about 900 queue overflows per transfer. The
worst individual specialization was trace Diagnostics at 98.4 MiB/s and
1.081 GB of serialized update batches; Pieces followed at 123.6 MiB/s. Every
view together fell to 74.0 MiB/s and serialized 1.742 GB while incurring up to
1,737 resets. A deliberately one-second consumer completed at 122.3 MiB/s
with nine reset snapshots and a 16.78 MB queue high water.

These are now reproducible regression observations, not accepted efficiency
targets. Library's zero-reset behavior proves the common Summary/reset path is
already material before detail views amplify it. The next API optimization
should therefore reduce snapshot reconstruction and repeated JSON delivery,
starting with Summary and trace Diagnostics, while preserving cursor,
coalescing and fresh-snapshot recovery semantics. Browser decode/reducer/paint
cost remains a distinct measurement boundary.

Public swarms and visible Tauri launch are unnecessary for this foundation.
The browser gateway, temporary profiles, controlled libtorrent peer, and pure
fixtures provide higher-signal headless evidence without disturbing the
interactive machine.

## Recommended Implementation Sequence

1. The bounded view-set domain owner, generated TypeScript/schema contract,
   polling adapter, pure reducer, and headless TypeScript evidence are complete
   in Tactical `033`.
2. The Zustand store, React shell, frontend inspection model, named demo
   adapter, and virtualized scale fixtures are complete in Tactical `034`.
3. Tactical `035` added stable torrent-list and active-peer projections with
   hostile and scale fixtures, mapped them through the controller into the
   inspection model, and closes lease/suspension lifecycle gaps. This step is
   complete.
4. Tactical `048` adds streaming as an interchangeable delivery adapter after
   polling behavior and reducer recovery are stable, and implements it for the
   in-process Tauri product. This step is complete.
5. Tactical `060` implements one multiplexed WebSocket as the ordinary browser
   application connection while retaining HTTP only as an explicit loopback
   diagnostic comparison. This step is complete.
6. Tactical `065` adds the bounded latest-value DHT view, exact generated
   contract validation, coherent replacement reduction, and joined terminal
   forwarding. This step is complete.
7. Measure update volume, decode/reduce cost, rendering, and memory before
   selecting binary encoding or finer-grained row patches.

Tacticals `033`, `034`, `035`, `048`, `060`, and `065` completed the first six
steps. Further views should follow observed inspection value. Binary encoding
remains a measurement-driven change rather than a prerequisite.

## References And Deliberate Differences

qBittorrent's `sync/maindata` cursor, full-update flag, keyed torrent changes,
and removed IDs demonstrate a useful recoverable polling shape. RSTorrent adds
named projections, per-view status, explicit epochs, bounded view-set
ownership, and transport-independent batches rather than copying that API.

Transmission RPC demonstrates explicit requested fields, object-shaped
results, and removed IDs for recently active torrents. RSTorrent initially
prefers named bounded projections over arbitrary fields and does not adopt
Transmission's positional table encoding.

The exact reference links and dependency roles are recorded in
[`../references.md`](../references.md). No source or fixture is imported by
this design.

## Remaining Open Decisions

- whether measured use justifies raising the implemented resource limits or
  retaining more than one unacknowledged cursor batch;
- whether category filtering is producer-side for the first list view or
  initially client-side within a bounded complete torrent list;
- exact reset granularity when one view snapshot itself exceeds its bound;
- remote authentication, pairing, TLS, exposure, and deployment posture;
- browser router and navigation URL details;
- the measured threshold for specialized field patches or binary encoding;
  and
- durable label and queue semantics; archive and bounded removal/deletion are
  implemented by Tactical `040`.
