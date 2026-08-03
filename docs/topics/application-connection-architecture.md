# Application Connection Architecture

Topic: `application-connection-architecture`

Status: Accepted direction; not implemented. HTTP long polling and
acknowledged per-view-set Tauri Channel delivery are implemented. The retained
`/control` WebSocket is a legacy per-projection compatibility proof. The
target browser and future remote shape is one authenticated, multiplexed
application connection that carries typed calls, commands, view-set creation
and mutation, streamed `UpdateBatch` values and exact cursor acknowledgements.
One physical WebSocket serves every view set belonging to that frontend-to-
backend connection. Future relay delivery wraps the same application frames in
an end-to-end encrypted circuit rather than creating another application API.

## Purpose And Scope

The application API needs a low-overhead real-time transport without making
HTTP polling, Tauri IPC, WebSocket framing or relay encryption the owner of
application semantics. It must also retain the simplicity and diagnostic
value of the current long-poll adapter.

This topic owns:

- the logical application-connection boundary above individual transports;
- typed request/response and server-push frame families;
- multiplexing of calls and multiple view attachments on one connection;
- view-set attachment, acknowledgement, reconnect and takeover semantics;
- connection-level fairness, backpressure, heartbeat and resource bounds;
- the relationship between HTTP, WebSocket and in-process Tauri adapters;
- the layering required by future end-to-end encrypted relay control; and
- the reference lessons that implementation must recheck before it proceeds.

[`application-control.md`](application-control.md) owns command meaning,
request identity and durable application revisions.
[`application-view-api.md`](application-view-api.md) owns named projections,
view-set resources, snapshots, patches, epochs, cursors, resets and leases.
[`client-view-delivery-policy.md`](client-view-delivery-policy.md) owns which
views the client requests and how frequently semantic changes may be emitted.
[`client-surfaces.md`](client-surfaces.md) owns platform hosting and lifecycle.

This topic records a future remote-compatibility requirement. It does not by
itself authorize a production listener, relay service, pairing flow, account
system, cryptographic dependency, stable public protocol or retirement of a
current compatibility adapter. Each requires a bounded tactical and the
security or migration decisions appropriate to it.

## Terminology

- An **application connection** is one client-side controller and its
  authenticated relationship with one RSTorrent backend. It may survive more
  than one physical connection generation through bounded reconnect.
- A **connection generation** is one concrete WebSocket, Tauri window
  generation or comparable attachment lifetime.
- A **call** is a correlated request/response operation such as command
  dispatch or opening a view set.
- A **view set** is the leased semantic resource defined by the application
  view API. It can outlive a failed physical connection until its lease
  expires.
- A **view attachment** is the connection-local route and pump that delivers
  one view set from an applied cursor.
- A **relay circuit** is an opaque end-to-end path for one authenticated client
  to one backend through a relay. It may share a physical relay WebSocket with
  other independently encrypted circuits.

Do not use `session` as the general name for this boundary. It is already
overloaded by the BitTorrent engine, application service, remote
authentication and user-facing torrent state.

## Accepted Architecture

One semantic application API has several delivery adapters:

```text
React / ViewController / other first-party client
                    |
          ApplicationViewClient
                    |
       typed application operations
                    |
       Rust application connection core
          /            |             \
         /             |              \
HTTP request/pull   WebSocket mux   Tauri invoke/Channel
         \             |              /
          \            |             /
       ApplicationService + leased ViewHub
```

The WebSocket adapter is the preferred real-time browser and future remote
transport. One authenticated socket carries:

```text
application connection
  +-- correlated calls and results
  +-- command requests and receipts
  +-- view attachment A: batches and acknowledgements
  +-- view attachment B: batches and acknowledgements
  +-- connection health and negotiated capabilities
```

There is never one WebSocket per view, projection or view set. A normal UI
currently needs one view set containing several projections, but the
connection supports a bounded number of independent sets without changing its
physical transport.

One browser tab or Tauri window is normally one logical client. Separate tabs
may use separate connections and view sets so their selection, backpressure
and lifecycle remain independent. Cross-tab `SharedWorker` socket sharing is
not implied by the one-connection rule.

## Semantic Operations On Every Adapter

Every capable adapter exposes the same operations:

- negotiate application version, delivery modes, codec and limits;
- dispatch a semantic command;
- open a view set and receive coherent initial snapshots;
- replace a view set's desired `ViewSpec` values;
- attach streamed delivery at an applied cursor where streaming exists;
- acknowledge an exactly applied batch cursor;
- detach delivery without necessarily destroying the view set;
- close a view set; and
- close the client connection.

WebSocket creation does not depend on an HTTP request. The socket can open,
update and close view sets directly. HTTP remains a complete alternative, not
a mandatory control plane for streaming.

The application protocol uses a closed typed operation union. Do not make an
arbitrary HTTP method and path string the canonical WebSocket request. The
HTTP adapter maps routes into the typed operations, while the WebSocket and
Tauri adapters invoke those operations directly.

## Conceptual Connection Frames

Exact generated names and encoding belong to the implementing tactical. The
protocol nevertheless has these stable frame families:

```text
client -> backend
  client_hello
  call(call_id, typed operation)
  attach(stream_id, view_set_id, after_cursor)
  detach(stream_id)
  ack(stream_id, exact_applied_cursor)
  semantic lease acknowledgement
  connection ping/pong where the platform requires it

backend -> client
  server_hello(capabilities, limits, selected codec)
  result(call_id, typed result)
  call_error(call_id, structured error)
  attached(stream_id)
  view_batch(stream_id, UpdateBatch)
  stream_error(stream_id, structured error)
  semantic lease probe
  connection ping/pong where the platform requires it
```

The client may pipeline a bounded number of calls. `call_id` correlates a
transport response and is distinct from a command's semantic `request_id`,
whose idempotency and retry contract is owned separately by application
control. Reusing a call identifier while it is pending is invalid.

Responses may be correlated independently, but operations that mutate one
view set must be observed in accepted receive order. The implementation must
not accidentally define global causal semantics merely because an early
version awaits every incoming call in one serial socket loop.

Unknown closed frame or operation variants fail according to negotiated API
version and capabilities. All lengths, counts and identifiers are validated
before allocation or application-service mutation.

## View-Set Identity And Attachment

The protocol intentionally retains two identities:

- `view_set_id` locates the leased, resumable server-side semantic resource;
- `stream_id` is a disposable connection-local multiplexing key for one
  attachment.

Neither value is a bearer credential. A remote caller must already have the
authenticated principal and client-instance authority that owns the view set.
Possession of an opaque ID alone grants nothing.

Opening a view set returns its coherent initial `UpdateBatch`. The client
validates and applies that batch before attaching at its resulting cursor. A
client library may present an `openAndAttach` convenience, but creation and
attachment remain separate semantic transitions because reconnect and
delivery-mode switching require attachment independently from creation.

Only one active consumer may drain a view set. A second attachment by an
unrelated owner or client fails. A newly authenticated connection generation
for the same logical client may atomically supersede its older attachment so a
half-open socket cannot block recovery. Generation-checked cleanup from the
old connection must never close or detach the replacement.

## Cursor Acknowledgement And Backpressure

WebSocket delivery preserves the existing view-set cursor contract:

```text
backend next_updates(last applied cursor)
  -> view_batch(base cursor, resulting cursor)
  -> runtime validation
  -> pure continuity reduction
  -> synchronous client-store commit
  -> ack(resulting cursor)
  -> backend may request the next batch
```

Receipt by the socket, transport decoder or JavaScript callback is not an
acknowledgement. A validation, reducer or state-callback failure leaves the
batch unacknowledged and therefore replayable or resettable according to the
existing view-set rules.

Each attachment has at most one emitted, unacknowledged batch. Compatible
current-state changes can continue to coalesce in the bounded view-set
accumulator. Ordered Diagnostics retain their separate ordered and explicit
loss semantics. A slow attachment cannot consume, acknowledge, clear or delay
another view set's semantic accumulator.

Acknowledgements may be consolidated into one small connection frame when
several streams advance together, but each item still names its exact stream
and cursor. A cumulative connection sequence cannot replace the independent
view-set cursors.

## Reconnect And Resume

A physical WebSocket failure detaches its stream pumps but does not
immediately destroy their view sets. The client reconnects, authenticates as
the same logical owner and attempts to attach each retained set at its last
successfully applied cursor.

For each view set, the backend then:

- replays the retained unacknowledged batch when the supplied cursor is its
  base;
- continues after acknowledging that batch when the supplied cursor is its
  result;
- returns an explicit reset when continuity cannot be retained; or
- reports unknown/expired/closed so the controller opens a new set and
  installs coherent snapshots from a fresh epoch.

Reconnect is not global transaction recovery. One view set may resume while
another has expired and reopens. Commands use their own request identity and
must not infer retry success from a view cursor.

A browser reload normally creates a new client instance and fresh view sets.
User delivery preference and desired presentation may persist; view-set IDs,
cursors and materialized engine state remain volatile unless a later tactical
explicitly proves a safe persistence contract.

## Heartbeat And Lease Semantics

Transport health and semantic client progress are different:

- one connection-level WebSocket ping/pong detects a dead path;
- it does not acknowledge a view batch or renew semantic ownership merely
  because the socket stack responded;
- an idle but healthy client periodically proves the exact applied cursor for
  its attached view sets; and
- those semantic lease acknowledgements may be batched across attachments so
  idle connections do not require one heartbeat stream per view set.

The current Tauri pump's empty acknowledged batch supplies this semantic
liveness proof. A multiplexed connection may use an explicit consolidated
lease probe/ack instead, but an unresponsive reducer must eventually stop
renewing its view set. Engine publication and outgoing queue wakeups never
renew a client lease.

## Connection Concurrency And Fairness

One socket must not become one serialized application bottleneck. The target
owner shape is:

```text
WebSocket reader
       |
bounded validation and routing
  +----+--------------------+
  |                         |
call/control owner      per-view attachment pumps
  |                         |
  +------------+------------+
               |
       fair bounded outbound scheduler
               |
         WebSocket writer
```

The reader validates and hands off work without waiting for an unrelated slow
call to complete. Application commands retain their application-service
ordering and idempotency rules. Operations targeting the same view set retain
causal order. Independent stream pumps do not run inside the call handler.

The outbound scheduler must:

- bound total connection bytes and items;
- preserve order within one stream;
- schedule ready attachments fairly rather than draining one indefinitely;
- prioritize bounded control results, errors and liveness over bulk snapshot
  continuations;
- stop reading new work or close the offending stream when declared bounds
  are exceeded; and
- expose queue high water, per-stream bytes, resets and delivery latency.

TCP still has transport-level head-of-line blocking. The application avoids
adding another preventable layer. Once a large WebSocket frame is handed to
the socket it cannot be preempted, so a remote-capable implementation must
either prove its maximum snapshot frame preserves required control latency or
split large encoded snapshots into bounded transport records. Reassembly and
validation finish before the semantic `UpdateBatch` is atomically reduced.

Torrent payload, file download, media playback and upload bulk data do not use
this control connection. A future verified-range content plane remains a
separate bounded service so large data transfer cannot starve pause, resume,
inspection or diagnostics control.

## Adapter Responsibilities

### HTTP long polling

HTTP retains the current v1 calls and `next_updates(after, wait_ms)` behavior.
The next pull supplies the applied cursor and therefore acknowledges the prior
batch. It remains the simplest headless, automation, debugging and
low-frequency adapter.

Long polling is not required to use WebSocket streaming. A client normally
selects one coherent adapter instead of opening via HTTP and streaming over a
socket, although the semantic ownership model does not make such a measured
future transition impossible.

### Browser WebSocket

The modern browser endpoint is a new versioned application connection, not a
silent extension of legacy `/control`. It authenticates once, negotiates
capabilities and carries every typed operation plus all attached view streams
on one socket. The preferred provisional route is `/api/v1/connect`; an
implementing tactical may change the URL only while updating this topic and
the application-view route record together.

JSON text frames are the first diagnostic codec. A future binary codec is a
connection negotiation that produces the same generated DTOs and reducer
inputs; it is not another semantic API version by itself.

### Tauri IPC

Tauri does not bind a loopback socket or pretend IPC is HTTP. Native invokes
may remain the natural request/response lane. The target streaming owner is
one multiplexed outbound Channel per window generation, keyed by stream ID,
with bounded acknowledgement invokes back to Rust.

Tauri and WebSocket share typed operations, the attachment registry, cursor
checks, pump behavior, fairness, errors and joined cancellation. They need not
share byte serialization or force native calls through a fake socket message
bus. The current one-Channel-per-view-stream implementation remains correct
until a bounded unification tactical replaces it.

## Relay-Compatible Layering

Future relay-mediated remote control uses the same inner application frames:

```text
physical client <-> relay WebSocket
  +-- relay circuit A
  |     +-- authenticated end-to-end encrypted records
  |           +-- RSTorrent application connection frames
  |                 +-- calls and commands
  |                 +-- view attachment 1
  |                 +-- view attachment 2
  |
  +-- optional relay circuit B for another backend
        +-- separately authenticated and encrypted records
              +-- independent RSTorrent application connection
```

For one backend there is one circuit; every view set for that backend is
multiplexed inside it. If a future client controls several backends through
one relay, an outer circuit identifier may share one client-to-relay socket
while each circuit retains independent authentication, keys, resume state,
queues and failure lifecycle.

The relay is an opaque bounded router. It may observe routing identity,
connection timing, circuit IDs and ciphertext sizes. It does not parse or
authorize application calls, torrent identities, commands, view
specifications, cursors, diagnostics or content.

The layer order is:

```text
typed application frame
  -> negotiated encoding
  -> optional bounded compression
  -> authenticated end-to-end record with replay protection
  -> optional outer relay-circuit framing
  -> WebSocket/TLS transport
```

Exact authentication, pairing, device identity, cryptography, key rotation,
record sequence, padding, compression-oracle policy and relay discovery are
deliberately not selected here. A security tactical must choose and review
them before remote implementation. The application connection must not need
to know whether an encrypted record traveled directly or through a relay.

## Ownership, Tasks And Cancellation

| Owner | State and work | Termination |
| --- | --- | --- |
| Application service | Commands, durable revision, ViewHub and leased view sets | Joined product shutdown closes all sets and wakes waiters |
| Logical client owner | Authenticated principal plus bounded client-instance identity | Explicit client close, principal revocation or application shutdown |
| Connection generation | Negotiation, call registry, attachment map and global bounds | Socket/channel close, replacement generation or application shutdown |
| Call owner | Unique call ID, validated operation, result/error completion and timeout | Result, cancellation, disconnect or bounded timeout |
| View attachment | Stream ID, view-set handle, applied cursor, pump, expected acknowledgement and queue accounting | Detach, takeover, view-set close, disconnect or shutdown |
| Outbound scheduler | Fair per-stream queues, control lane and writer backpressure | Connection-generation cancellation and awaited writer join |
| TypeScript connection adapter | Pending calls, attachments, validation, ack and reconnect | Client close aborts and joins all consumption before transport close |

Every task has one named cancellation owner. Detaching or replacing a
connection generation cancels and awaits its pumps. Late completion from an
old generation cannot mutate, acknowledge or close a newer attachment.

## Bounds And Hostile Input

The implementation tactical must select and advertise at least:

- maximum physical connections per owner and service;
- maximum attached view sets per connection;
- maximum pending calls and call-request bytes;
- maximum control-frame and encoded-data-record bytes;
- maximum aggregate and per-stream outbound queued bytes;
- maximum invalid frames before connection closure;
- call and handshake timeouts;
- connection idle and semantic lease intervals; and
- snapshot fragmentation/reassembly bytes and timeout if fragmentation is
  implemented.

Existing view-set count, queue, snapshot and lease limits remain authoritative
semantic bounds. Connection bounds are additional containment, not a way to
silently enlarge or bypass them. One stream exceeding a local bound should be
failed or reset without destroying healthy peers when safe; malformed framing,
authentication failure or connection-wide overflow may close the connection.

## YepAnywhere Reference Dossier

The local YepAnywhere sibling was inspected at commit
`b47f945700413fe414542ea51a79f826dd76eae9` on 2026-08-03. It is an
architectural and failure reference, not an RSTorrent dependency or wire
contract. No source, fixtures or protocol values are imported.

Future implementation must re-read the then-current sibling and compare it
with this observed revision, focusing on:

- `packages/shared/src/relay.ts`: one inner typed union carries correlated
  request/response, subscription, event, upload and ping/pong traffic;
- `packages/client/src/lib/connection/RelayProtocol.ts`, especially
  `RelayProtocol`, `routeMessage`, its pending request/upload maps and
  subscription map: one protocol router is composed below more than one
  transport;
- `packages/client/src/lib/connection/WebSocketConnection.ts`, especially
  `WebSocketConnection` and `ensureConnected`: a plain socket composes the
  shared inner router;
- `packages/client/src/lib/connection/SecureConnection.ts`, especially
  `SecureConnection`, authenticated resume, encrypted send/receive and its
  composition of `RelayProtocol`: encryption wraps the same inner operations;
- `topics/relay-client-mux.md`: one physical browser-to-relay socket carries
  bounded independent circuits while preserving legacy per-host fallback;
- `packages/relay/src/mux-handler.ts`, especially `RelayMuxCoordinator`,
  `RelayMuxSession`, `queueServerFrame` and the round-robin drain: per-circuit
  accounting, limits and fair delivery contain one circuit's pressure; and
- `docs/project/relay-head-of-line-blocking.md`: awaiting unrelated request
  work in one inbound message chain can create application-level head-of-line
  blocking even when the physical transport is healthy.

RSTorrent adopts these lessons:

- keep the inner application protocol independent from plain or encrypted
  transport;
- correlate concurrent calls and route multiple streams explicitly;
- keep relay routing opaque to end-to-end application contents;
- isolate circuit/stream failure and queue accounting where possible;
- schedule multiplexed streams fairly; and
- treat reconnect, bounds and malformed input as protocol behavior rather
  than incidental socket errors.

RSTorrent intentionally differs:

- it uses a closed typed application operation union rather than making HTTP
  method/path tunneling its canonical inner protocol;
- its primary stream resource is a resumable view set with exact applied
  cursor acknowledgement, not an event subscription alone;
- a view set may outlive one physical connection generation under a bounded
  lease;
- commands retain application request identity independently from connection
  call correlation;
- torrent payload and other bulk content are excluded from the control
  connection; and
- this architecture does not adopt YepAnywhere's SRP, NaCl, framing constants,
  relay discovery, limits or release-compatibility policy without a separate
  security and dependency decision.

## Migration Direction

Implementation should proceed in bounded slices:

1. Record a tactical with the exact generated frame contract, owner map,
   bounds, threat model, cancellation map and updated YepAnywhere audit.
2. Extract or establish a Rust connection/attachment core that delegates to
   existing application-service and view-set operations without duplicating
   semantic state.
3. Implement a new loopback-only versioned WebSocket adapter and generated
   TypeScript client alongside HTTP long polling and legacy `/control`.
4. Prove WebSocket creation, update, attachment, exact post-reducer ack,
   multi-view-set fairness, reconnect and bounded failure through the same
   reducer traces as HTTP and Tauri.
5. Measure request count, bytes, CPU, allocation, queue high water, resets and
   producer throughput against current HTTP and Tauri baselines.
6. Unify the Tauri attachment owner and, when justified, replace per-stream
   Channels with one window-level multiplexed Channel without routing native
   calls through HTTP or JSON unnecessarily.
7. Inventory and migrate every remaining legacy `/control`, old Tauri and
   Android consumer before deleting compatibility code.
8. Design and implement relay authentication/encryption as its own security
   campaign using the already-proven direct application frames.

Browser WebSocket work must not silently introduce a production remote
listener. Relay work must not be combined into the initial local transport
slice merely because the inner frames were designed for it.

## Required Evidence

Before WebSocket delivery is called implemented, prove:

- semantic state equivalence for the same snapshot/patch trace through HTTP,
  WebSocket and Tauri;
- view-set creation and commands entirely over one WebSocket;
- two or more independent view sets sharing one socket without identity,
  acknowledgement or cleanup interference;
- acknowledgement only after successful validation, reduction and store
  commit;
- exact retained replay, connection-generation takeover, cursor reset, lease
  expiry and fresh-snapshot recovery;
- command and control progress while another view set is slow or delivering a
  large snapshot;
- fair scheduling and declared per-stream/connection high-water bounds;
- malformed, oversized, duplicate, stale and unauthorized frame behavior;
- prompt joined shutdown with calls, waits, pumps and acknowledgements active;
- lower framing/request overhead than real-time long polling without worse
  application producer throughput or reset storms; and
- current long polling remains green as a complete fallback.

Future relay claims additionally require opaque-relay inspection evidence,
end-to-end authentication and encryption tests, replay/tamper rejection,
reconnect/resume, per-circuit isolation, ciphertext and queue bounds and a
controlled direct-versus-relay semantic trace comparison.

## Durable Drift Guards

Implementation may refine names, frame grouping and measured limits, but must
update this topic before changing any of these accepted decisions:

- one semantic API across HTTP, WebSocket, Tauri and future relay delivery;
- WebSocket directly supports calls and view-set creation;
- one WebSocket multiplexes all view sets for one frontend/backend connection;
- view-set identity is separate from connection-local stream identity;
- neither identifier is an authorization token;
- exact per-view-set cursor acknowledgement occurs only after application;
- view sets can resume across physical connection generations;
- connection heartbeat is separate from semantic lease progress;
- per-stream fairness and bounded backpressure are explicit owners;
- relay encryption wraps the same inner application frames; and
- torrent payload and other bulk content stay off the control connection.
