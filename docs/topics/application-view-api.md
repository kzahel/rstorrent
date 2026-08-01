# Application View API

Topic: `application-view-api`

Status: Semantic direction accepted. The existing per-projection subscriptions
from Tactical `008` remain the implemented proof; a bounded view-set tactical
is the next product implementation slice. No stable public remote wire
compatibility is claimed yet.

## Purpose And Scope

The desktop inspection surface needs a recoverable, typed local replica of the
application state currently relevant to one UI. The same semantics must work
through an in-process Tauri adapter, a headless browser gateway, periodic
polling, and a later low-latency stream without making transport or encoding
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
foundations. The next design aggregates the views visible to one client into
one view set and adds pull consumption. It should evolve those owners rather
than preserve the current one-subscription-per-projection public shape.

## Owner And Task Map

| Owner | Responsibility | Lifetime and termination |
| --- | --- | --- |
| Application view hub | Current projection source state and fan-out into view sets | One application-service instance; closes all view sets during joined service shutdown |
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
bounded idle lease. Exact lease time, grace period after a streaming
disconnect, and global/per-principal limits belong to the implementation
tactical and must be measured rather than embedded in this topic.

Updating the requested specifications changes the existing view set. Added or
changed view snapshots enter its single ordered update feed; the configuration
response is only an acknowledgement. It does not create a competing second
state response. Removed views release producer state and produce an explicit
`view_removed` update at the correct cursor boundary.

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
  | { type: "diagnostics"; torrent_id?: TorrentId }
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

Projection rules are:

| Data shape | Patch and coalescing rule |
| --- | --- |
| Torrent, peer, file, and tracker collections | Full-row keyed upsert plus removed IDs; repeated upserts retain the newest complete row |
| Selected summary or other singleton | Replace the value |
| Rates, counters, current peer state | Latest value may conflate within the delivery interval |
| Verified pieces and bounded current piece activity | Typed range or bitmap additions, clears, and replacements |
| Diagnostics and ordered events | Ordered append with explicit dropped count; never silently latest-value conflated |
| Queue or priority order | Explicit sortable fields; array position is not meaning unless the projection states it |

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

The same operation may later permit a bounded `max_wait_ms` for long polling.
A WebSocket or Tauri Channel attaches to the same view set and cursor and emits
the same `UpdateBatch`. If retained history cannot satisfy the cursor, it emits
or returns a reset rather than silently starting at the tail.

Low-latency delivery does not put `requestAnimationFrame` into the Rust or wire
contract. The server may deliver current-state changes immediately with
bounded latest-value coalescing; the client store and renderer decide when to
paint. Ordered events retain their separate loss and backpressure semantics.

## Provisional Remote Routes

The remote adapter reserves this shape for the first tactical:

```text
GET    /api/v1/hello
POST   /api/v1/commands
POST   /api/v1/view-sets
PUT    /api/v1/view-sets/{id}/views
GET    /api/v1/view-sets/{id}/updates?after=...&wait_ms=...
DELETE /api/v1/view-sets/{id}
GET    /api/v1/stream
```

`/api/v1/stream` is a WebSocket upgrade that attaches an existing view set and
cursor after authentication. The exact authentication mechanism is outside
this topic. The current loopback `/control` WebSocket remains a proof and may
be adapted or retired; preserving its path is not a compatibility requirement.

The local Tauri product maps these semantic calls to commands and later
Channels. It does not bind a loopback port or serialize through HTTP merely to
share the browser adapter's URLs.

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

Files, trackers, pieces, disk activity, speed history, swarm state, and DHT
follow through named views according to inspection value. Unsupported views
must report unsupported or unavailable explicitly rather than fabricate empty
data.

## Validation And Evidence

Before the React surface depends on view sets, the foundation tactical should
prove:

- deterministic opening, specification changes, per-view snapshots, keyed
  upserts, row removals, view removal and re-add, atomic batches, and close;
- independent view sets with different desired projections and consumption
  speeds;
- one-consumer enforcement and poll-to-stream cursor continuity;
- lost-response replay and acknowledgement only after client reduction;
- queue coalescing, exact high-water accounting, overflow, per-view reset, and
  fresh-snapshot convergence;
- cursor expiry, epoch mismatch, lease expiry, application shutdown, and
  prompt joined task termination;
- authenticated ownership, request/frame bounds, and a view-set ID that is not
  treated as a credential in the loopback gateway;
- generated TypeScript plus JSON Schema drift checks and independently tested
  semantic limits;
- a pure TypeScript reducer and CLI client that can materialize the same
  snapshots and diffs through polling; and
- one controlled libtorrent-seeded download observed through that CLI from
  add through verified publication and clean shutdown.

Actual torrent deletion is not a prerequisite for testing collection
`removed` diffs. A deterministic fixture or a torrent leaving a selected
category can prove projection removal. Adding destructive or content-deleting
commands requires its own application-control semantics and safety evidence.

Public swarms and visible Tauri launch are unnecessary for this foundation.
The browser gateway, temporary profiles, controlled libtorrent peer, and pure
fixtures provide higher-signal headless evidence without disturbing the
interactive machine.

## Recommended Implementation Sequence

1. Open one bounded tactical for the view-set domain owner, generated
   TypeScript/schema contract, polling adapter, pure reducer, and headless
   TypeScript CLI evidence.
2. Add the stable peer projection and its hostile/scale fixtures once the
   view-set mechanics are proven.
3. Establish the Zustand store and React shell on fixture data, then connect
   torrent-list and peer views through the controller.
4. Add streaming as an interchangeable delivery adapter only after polling
   behavior and reducer recovery are stable.
5. Measure update volume, decode/reduce cost, rendering, and memory before
   selecting binary encoding or finer-grained row patches.

No implementation tactical is active yet.

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

- exact initial fields and privacy/redaction policy for torrent and peer rows;
- concrete resource limits, lease durations, and retained cursor window;
- whether category filtering is producer-side for the first list view or
  initially client-side within a bounded complete torrent list;
- exact reset granularity when one view snapshot itself exceeds its bound;
- remote authentication, pairing, TLS, exposure, and deployment posture;
- browser router and navigation URL details;
- the measured threshold for streaming, specialized field patches, or binary
  encoding; and
- durable archive, label, removal, deletion, and queue semantics.
