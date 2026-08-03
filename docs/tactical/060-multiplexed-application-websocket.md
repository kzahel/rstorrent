# Tactical 060: Multiplexed Application WebSocket

Status: Proposed.

Topics: `application-connection-architecture`, `application-view-api`,
`client-view-delivery-policy`, `client-surfaces`, `web-ui-design`,
`performance-and-live-evidence`, `capability-readiness`

## Motivation And Outcome

The browser currently implements `ApplicationViewClient` with one HTTP request
per command or view-set mutation and a repeated long poll for each active view
set. This is a complete and useful fallback, but it creates avoidable request
traffic at real-time cadence and does not exercise the accepted connection
architecture.

Make one loopback WebSocket the default live-browser application connection.
That socket directly negotiates the API, dispatches commands, opens, updates
and closes view sets, and multiplexes every acknowledged view stream. Static
web assets remain HTTP; torrent payload and file-content delivery do not enter
the control connection. HTTP long polling remains a complete, explicitly
selectable adapter for debugging, compatibility and low-frequency use.

This is the first implementation slice of
[`application-connection-architecture.md`](../topics/application-connection-architecture.md).
It proves the typed inner application protocol that future Tauri consolidation
and relay encryption can reuse. It does not implement a remote listener or
relay.

The stopping condition is a default live browser that performs all semantic
application work through exactly one `/api/v1/connect` WebSocket, with bounded
multiplexing, exact cursor acknowledgement, generation-safe reconnect,
explicit HTTP fallback, deterministic browser and gateway evidence, and a
recorded paired transport smoke.

## Current-State Evidence

- `clients/web/src/inspection/bootstrap.tsx::startLiveInspection` constructs
  `HttpApplicationClient`; `poll_ms` only changes the long-poll wait.
- `clients/web/src/api/client.ts::ApplicationViewClient` already separates the
  semantic client from its transport. `ViewController` prefers
  `streamUpdates` where present, so React and reducers need no WebSocket
  knowledge.
- `HttpApplicationClient` supplies a stable 32-lowercase-hex owner header and
  implements hello, command dispatch, view-set mutation and long polling.
- `TauriApplicationViewClient` already implements acknowledged
  `streamUpdates`. Its iterator acknowledges the previously applied cursor
  only when the consumer asks for the next batch.
- `clients/desktop/src-tauri/src/view_delivery.rs::run_view_stream` permits one
  outstanding batch and requires its exact cursor acknowledgement. This is
  correct behavior but is currently native-adapter-specific.
- `crates/rstorrent-gateway/src/lib.rs` exposes the modern HTTP v1 routes and a
  legacy `/control` WebSocket. `/control` uses the older per-projection
  `GatewayClientMessage` contract and is not silently repurposed.
- The gateway is loopback-only, checks the exact browser `Origin`, limits
  physical connections to 8, incoming WebSocket messages to 64 KiB and the
  legacy socket to 8 subscriptions. Modern HTTP responses are capped at
  16 MiB.
- The semantic view hub permits 8 sets per owner, 16 views per set, a 512 KiB
  steady-state queue, a 16 MiB snapshot, a 20-second maximum wait and a
  five-minute lease. Those limits remain authoritative.
- Tactical `057` measures application projection cost in process. It records
  the browser-attached comparison as later work, so this tactical adds the
  first paired HTTP/WebSocket adapter smoke without redefining its engine
  baselines.

## Stable Scenarios

- One normal browser tab creates one physical application WebSocket regardless
  of how many supported views or view sets it consumes. A second tab is a
  separate logical client and may create its own socket.
- The socket can negotiate, open a view set and begin streaming without first
  making any semantic HTTP API request.
- Commands and view-set create, update and close calls share the socket with
  update batches. Calls are correlated independently; a slow view consumer
  cannot consume or acknowledge another stream.
- Opening a view set returns the existing coherent initial `UpdateBatch`. The
  browser validates, reduces and commits it before attaching at its resulting
  cursor.
- A received batch is not acknowledged until runtime validation, continuity
  reduction and synchronous client-store publication have succeeded.
- A socket failure detaches delivery but retains the leased view set. A
  replacement generation for the same client instance resumes from the last
  applied cursor or receives an explicit reset. Unknown, expired or closed
  sets reopen through the existing controller path.
- Pending command calls are failed on disconnect and are never automatically
  replayed. Their semantic `request_id`, not the connection's `call_id`, owns
  explicit retry and idempotency.
- The default `?live=...` browser path selects WebSocket. An explicit
  `transport=http` selects the complete current HTTP adapter. There is no
  silent automatic fallback that could hide a broken WebSocket path;
  `poll_ms` is valid only for the HTTP selection.
- Legacy `/control`, the current Tauri adapter and headless HTTP callers remain
  functional while the new endpoint lands.

## Scope

- Define generated typed application operations, results, connection frames,
  errors and negotiated connection limits.
- Add the loopback-only `/api/v1/connect` WebSocket beside legacy `/control`.
- Extract one shared acknowledged-view-stream state machine used by the new
  WebSocket pumps and the current Tauri pump.
- Add connection-generation, call, attachment, fairness, byte-budget,
  heartbeat, cancellation and takeover owners to the gateway.
- Implement `WebSocketApplicationViewClient` behind the existing TypeScript
  interface and make it the default live-browser adapter.
- Preserve explicit HTTP long polling and prove semantic parity across the two
  browser adapters.
- Add bounded metrics and a paired transport smoke that identifies request,
  frame, byte, reset and queue costs.
- Update the owning topics, generated-contract drift checks and capability
  evidence.

## Non-Goals

- No production network listener, remote daemon, relay service, account,
  pairing, end-to-end encryption or remote authentication design.
- No relay circuit framing, compression, CBOR/binary codec, record encryption
  or stable public compatibility promise.
- No torrent payload, file download, media serving or upload bulk data on the
  application socket.
- No one-Channel-per-window Tauri migration. Tauri keeps its current invokes
  and per-stream Channels while sharing the extracted acknowledgement core.
- No Android/UniFFI migration, legacy `/control` removal or HTTP route removal.
- No cross-tab `SharedWorker` connection, persisted view-set IDs or automatic
  command replay.
- No cadence-profile implementation, projection/reset-storm optimization or
  engine throughput policy change. The independent cadence topic remains the
  owner of requested delivery frequency.
- No application-level chunking in this slice unless the recorded large
  snapshot proof shows it is required for correctness. Remote-capable
  latency under large frames remains a later bounded transport slice.

## Typed Semantic Calls

The protocol carries a closed operation union rather than arbitrary HTTP
methods and paths. Transport-independent DTOs live under a new
`rstorrent_session::application_connection` boundary and derive Serde,
Schemars and `ts-rs` contracts.

```text
ApplicationCall
  dispatch(RequestEnvelope)
  open_view_set(OpenViewSetRequest)
  update_view_set(view_set_id, UpdateViewSetRequest)
  close_view_set(view_set_id)

ApplicationCallResult
  command_response(ResponseEnvelope)
  view_set_opened(OpenViewSetResponse)
  view_set_updated
  view_set_closed
```

The HTTP adapter maps its existing routes to the same operations. The
WebSocket adapter invokes them directly. `call_id` is connection-local
correlation and is distinct from `RequestEnvelope.request_id`. A client may
pipeline bounded independent calls, but it may not reuse a pending `call_id`.
Operations accepted for one view set preserve receive order. No global causal
ordering is inferred from the order in which results and view batches happen
to reach the writer.

## Version-One JSON Frames

The first codec is one JSON object per WebSocket text message. Every union is
internally tagged by a snake-case `type`. The examples below select the v1
wire shape; generated Rust and TypeScript definitions, schema snapshots and
runtime decoders must agree exactly.

### Connection handshake

The first client message, received within five seconds of upgrade, is:

```json
{
  "type": "connect",
  "api_version": 1,
  "encoding": "json",
  "client_instance_id": "7f51db1a5d20450ba24acb92fce88c12",
  "token": "loopback-bearer-token"
}
```

The browser supplies `Origin` through the WebSocket upgrade. The gateway
requires the configured exact origin before upgrading. Bearer mode requires a
nonempty token of at most 128 bytes in the first message;
`UnauthenticatedLoopbackDevelopment` requires the field to be absent. Tokens
are never included in `Debug`, logs, metrics, errors or connection evidence.

The successful server response is:

```json
{
  "type": "connected",
  "api_version": 1,
  "encoding": "json",
  "hello": {
    "api": { "current": 1, "minimum": 1 },
    "encodings": ["json"],
    "deliveries": ["poll", "long_poll", "stream"],
    "capabilities": ["torrent_list", "torrent_summary"],
    "limits": {
      "max_view_sets_per_owner": 8,
      "max_views_per_set": 16,
      "max_view_id_bytes": 64,
      "min_queue_bytes": 16384,
      "default_queue_bytes": 262144,
      "max_queue_bytes": 524288,
      "max_snapshot_bytes": 16777216,
      "max_wait_millis": 20000,
      "lease_millis": "300000"
    }
  },
  "connection_limits": {
    "max_attachments": 8,
    "max_pending_calls": 16,
    "max_client_message_bytes": 65536,
    "max_application_payload_bytes": 16777216,
    "heartbeat_idle_millis": 15000,
    "heartbeat_timeout_millis": 10000
  }
}
```

The capability list above is abbreviated only to keep the example readable;
the actual frame carries the complete generated `ApiHello`. Once the endpoint
exists, HTTP hello also advertises `stream` as a service delivery capability.
The `connected` envelope selects JSON for this physical connection.

Authentication, version or handshake-policy rejection after upgrade sends one
bounded error and then closes; origin rejection remains an HTTP `403` before
upgrade:

```json
{
  "type": "connection_error",
  "error": {
    "code": "authentication_failed",
    "message": "application connection authentication failed"
  }
}
```

### Calls and results

```json
{
  "type": "call",
  "call_id": "call-1",
  "operation": {
    "type": "open_view_set",
    "request": {
      "views": [
        {
          "type": "torrent_list",
          "view_id": "library",
          "delivery": { "min_interval_millis": 250 }
        }
      ],
      "options": { "requested_queue_bytes": 262144 }
    }
  }
}
```

```json
{
  "type": "result",
  "call_id": "call-1",
  "result": {
    "type": "view_set_opened",
    "response": {
      "view_set_id": "vs_0123456789abcdef0123456789abcdef",
      "lease_millis": "300000",
      "effective_queue_bytes": 262144,
      "effective_views": [],
      "initial": {
        "api_version": 1,
        "view_set_id": "vs_0123456789abcdef0123456789abcdef",
        "epoch": "42",
        "base_cursor": "0",
        "cursor": "1",
        "durable_revision": "9",
        "updates": []
      }
    }
  }
}
```

The elided `effective_views` and `updates` arrays in the example are populated
normally. Void mutations return their typed result rather than relying on an
empty body. A rejected call returns a correlated error:

```json
{
  "type": "call_error",
  "call_id": "call-1",
  "error": {
    "code": "resource_limit",
    "message": "pending call limit reached"
  }
}
```

### View attachment and acknowledgement

```json
{
  "type": "attach",
  "call_id": "call-2",
  "stream_id": "view-1",
  "view_set_id": "vs_0123456789abcdef0123456789abcdef",
  "after": "1"
}
```

```json
{
  "type": "attached",
  "call_id": "call-2",
  "stream_id": "view-1",
  "view_set_id": "vs_0123456789abcdef0123456789abcdef"
}
```

```json
{
  "type": "view_batch",
  "stream_id": "view-1",
  "batch": {
    "api_version": 1,
    "view_set_id": "vs_0123456789abcdef0123456789abcdef",
    "epoch": "42",
    "base_cursor": "1",
    "cursor": "2",
    "durable_revision": "9",
    "updates": []
  }
}
```

After validation, reduction and store publication:

```json
{ "type": "ack", "stream_id": "view-1", "cursor": "2" }
```

Detach is correlated so local close can await server ownership cleanup:

```json
{ "type": "detach", "call_id": "call-3", "stream_id": "view-1" }
```

```json
{ "type": "detached", "call_id": "call-3", "stream_id": "view-1" }
```

An attachment-specific failure is not disguised as a call result:

```json
{
  "type": "stream_error",
  "stream_id": "view-1",
  "error": { "code": "invalid_cursor", "message": "cursor is not retained" }
}
```

`stream_id` is a connection-local multiplexing key. `view_set_id` names the
leased semantic resource. Neither is authentication authority.

## Example Browser Session

The ordinary wire sequence is:

```text
browser                              gateway / application service
   |--- WebSocket /api/v1/connect ----------------->|
   |--- connect(client instance, auth) ------------>|
   |<-- connected(ApiHello, connection limits) -----|
   |--- call c1: open_view_set -------------------->|
   |<-- result c1: initial UpdateBatch --------------|
   |    validate -> reduce -> commit cursor 1        |
   |--- attach c2: stream v1 after cursor 1 -------->|
   |<-- attached c2 ---------------------------------|
   |<-- view_batch v1: cursor 1 -> 2 ----------------|
   |    validate -> reduce -> commit cursor 2        |
   |--- ack v1 cursor 2 ---------------------------->|
   |--- call c3: pause(request_id r17) ------------->|
   |<-- view_batch v1: cursor 2 -> 3 ----------------|
   |<-- result c3: command receipt r17 --------------|
   |--- ack v1 cursor 3 ---------------------------->|
   |--- call c4: update_view_set ------------------->|
   |<-- result c4: view_set_updated -----------------|
   |--- detach c5 v1 ------------------------------->|
   |<-- detached c5 ---------------------------------|
   |--- call c6: close_view_set -------------------->|
   |<-- result c6: view_set_closed ------------------|
```

The interleaving of `view_batch` and `result` carries no cross-lane ordering
promise. The command's application receipt and subsequent view revisions own
semantic causality.

On unexpected disconnect the browser keeps the same in-memory
`client_instance_id`, coalesces concurrent reconnect demand, applies bounded
increasing backoff, then sends a fresh `connect`. It reattaches each retained
view set with its last applied cursor. The gateway either resumes, produces a
reset under the current view-set rules, or reports unknown/expired so
`ViewController` opens a fresh set. A page reload creates a new client
instance and fresh view sets.

## Error And Close Contract

Add a connection-specific structured error rather than extending the legacy
`GatewayError` union. Initial codes are:

```text
authentication_failed
invalid_version
invalid_message
invalid_call
resource_limit
unknown_view_set
consumer_busy
view_set_closed
unknown_stream
invalid_cursor
response_too_large
internal
```

Call-scoped and stream-scoped faults use `call_error` or `stream_error` and
leave healthy peers attached where safe. Authentication/version failure,
malformed framing, repeated invalid messages and connection-wide budget
failure use `connection_error` where the socket remains writable and close the
connection. The pending-correlation budget includes ordinary calls plus
in-flight attach and detach transitions.

WebSocket close usage follows RFC 6455:

- `1000` for an acknowledged normal client/service close;
- `1001` for service shutdown;
- `1002` for invalid protocol framing or message sequence;
- `1008` for authentication or policy failure after upgrade; and
- `1009` for an over-limit WebSocket message.

Reserved `1006` is observed locally for abnormal loss and is never sent in a
Close frame. Close reasons are bounded and contain no token, request body,
torrent identifier or internal path.

## Identity, Reconnect And Takeover

`WebSocketApplicationViewClient` generates one 32-lowercase-hex
`client_instance_id` for its lifetime and reuses it across physical reconnects.
The gateway derives one owner namespace from authentication plus that client
identity. The HTTP adapter's current internal owner namespace should converge
on the same `gateway-client-{namespace}-{client_id}` form so transport choice
does not alter semantic ownership; the string remains an internal detail.

A gateway-wide attachment registry is keyed by logical owner and view-set ID.
Attaching from a newly authenticated connection generation atomically cancels
and joins the older pump before it drains the set. Cleanup carries the old
generation token and therefore cannot detach, acknowledge or close its
replacement. An unrelated owner receives `consumer_busy`.

The client does not keep both HTTP polling and WebSocket attachment active for
one set. A future measured adapter handoff must detach the old consumer before
attaching the new one.

## Shared Acknowledgement State

Extract a runtime-independent `AcknowledgedViewStream` in `rstorrent-session`.
It owns a `ViewSet`, applied cursor and optional emitted cursor, and exposes
transitions equivalent to:

```text
next_batch()
  allowed only when no acknowledgement is outstanding
  calls next_updates(applied_cursor)
  records the emitted resulting cursor

ack(cursor)
  succeeds only for the exact emitted cursor
  advances applied_cursor and clears the outstanding batch
```

Cancel, detach and lease outcomes remain explicit. Both
`run_view_stream` in Tauri and each gateway attachment pump use this state
machine so exact acknowledgement, empty-batch liveness and future changes
cannot drift between adapters. Serialization, socket tasks and Tauri Channels
remain outside `rstorrent-session`.

## Ownership, Tasks And Cancellation

```text
connection generation
  +-- WebSocket reader / validator / router
  +-- bounded call owner
  +-- attachment registry
  |     +-- acknowledged pump for stream A
  |     +-- acknowledged pump for stream B
  +-- fair outbound scheduler
  +-- WebSocket writer and heartbeat
```

| Owner | State and work | Termination |
| --- | --- | --- |
| Gateway service | Connection semaphore, authenticated-owner generations and takeover registry | Joined gateway/application shutdown |
| Connection generation | Handshake, logical owner, call map, stream map, invalid count and aggregate budgets | Socket close, replacement, fatal policy or shutdown |
| Reader/router | Size check, JSON validation and bounded routing only | Connection cancellation; never awaits an unrelated view batch |
| Call owner | At most 16 IDs and accepted typed operations | Result/error, cancellation or bounded application shutdown |
| Attachment | Stream ID, generation, shared acknowledgement state and one ready/unacknowledged batch | Detach, takeover, view close, disconnect or shutdown |
| Outbound scheduler | Prioritized control lane, round-robin ready streams and byte reservations | Connection cancellation and awaited drain/abort |
| Writer/heartbeat | Single socket sink, ping nonce and pong deadline | Close handshake, I/O failure or cancellation |
| TypeScript client | One socket promise, pending calls, stream iterators, last applied cursors and reconnect demand | `close()` rejects calls, closes iterators and closes socket |

All spawned tasks inherit one cancellation token and are awaited. The reader
does not execute a long poll inline. Call processing may initially be
sequential to retain current `ApplicationService` mutation ordering, but the
reader and stream pumps stay responsive and the wire contract does not promise
global result ordering.

## Fairness And Backpressure

- Each attachment has at most one emitted, unacknowledged batch and at most one
  ready slot in the outbound scheduler.
- Control results, errors, close and liveness have a bounded priority lane.
  Ready stream slots are selected round-robin; one stream is not drained
  repeatedly while another is ready.
- A pump reserves worst-case byte capacity before requesting a potentially
  large batch. With two maximum-frame reservations available, additional
  streams wait before materializing another snapshot rather than creating an
  uncounted memory queue.
- An acknowledged stream may request its next batch only after scheduler and
  byte-budget admission. The semantic view accumulator continues its existing
  bounded coalescing while the stream waits.
- A slow stream cannot renew, acknowledge or clear another set. Ordered
  Diagnostics retain their explicit loss behavior.
- The writer records enqueue-to-send latency and releases the exact encoded
  reservation after the sink accepts or rejects the message.

TCP still has transport head-of-line blocking. JSON v1 sends one complete
semantic message as one WebSocket message, so a maximum snapshot can delay a
small result once writing begins. The first slice is loopback-only and must
measure that delay with the existing 4,096-file snapshot fixture. It does not
claim remote suitability. If that proof violates the accepted command-latency
bound, implementation stops for a bounded chunking/codec decision instead of
silently weakening the test.

## Initial Bounds

| Resource | Version-one bound |
| --- | ---: |
| Physical gateway connections | Existing service maximum 8 |
| View attachments per connection | 8 |
| Pending calls per connection | 16 |
| Queued control/error items | 32 |
| Client JSON message | 64 KiB |
| Encoded semantic response or `UpdateBatch` | Existing 16 MiB |
| Server JSON envelope overhead | 4 KiB |
| Server WebSocket message | 16 MiB + 4 KiB |
| Reserved outbound data bytes | Two maximum server messages |
| Ready/unacknowledged batches | 1 per stream |
| `call_id` and `stream_id` | 1..=64 ASCII `[A-Za-z0-9._-]` |
| Client instance ID | Exactly 32 lowercase hexadecimal characters |
| Handshake time | 5 seconds |
| Idle before server Ping | 15 seconds |
| Matching Pong deadline | 10 seconds |
| Invalid application messages | 3, then protocol close |

The 16 MiB semantic limit remains the same ceiling as modern HTTP. The 4 KiB
allowance covers only the generated connection envelope and bounded IDs; it
does not enlarge a snapshot or call result. Transport configuration must set
both message and frame limits explicitly instead of inheriting Tungstenite
defaults. Client text, identifier and collection lengths are checked before
application mutation.

Native WebSocket Ping/Pong proves the connection path only. It does not renew
a view-set lease or acknowledge a batch. This slice retains the current
per-attachment empty acknowledged batch as the semantic liveness proof. A
later multiplexing refinement may consolidate idle lease probes after proving
identical failure behavior.

## TypeScript Adapter

Add `WebSocketApplicationViewClient implements ApplicationViewClient` with:

- one lazily established and reconnectable socket;
- a cached decoded `connected.hello` returned by `hello()`;
- one bounded `call_id` map and one bounded `stream_id` map;
- runtime validation for every generated server frame and nested DTO;
- `dispatch`, `openViewSet`, `updateViewSet` and `closeViewSet` mapped to typed
  calls;
- `streamUpdates` implemented as an async iterator with one queued item;
- exact acknowledgement of the prior cursor only when iteration requests the
  next item, matching the Tauri adapter;
- abort-aware attach, call and detach cleanup;
- one coalesced `ensureConnected` attempt and bounded increasing reconnect
  backoff; and
- no implicit replay of pending commands.

On physical disconnect, pending calls reject with one normalized
`ApplicationViewError` and update iterators fail in a way that lets the
existing controller retry from its last committed cursor. Validation or
reducer failure leaves the batch unacknowledged and closes that stream rather
than requesting later data. `close()` stops reconnect, rejects pending work,
awaits stream cleanup and performs a bounded normal socket close.

`startLiveInspection` selects this adapter by default. `transport=http` selects
`HttpApplicationClient`; `poll_ms` with WebSocket is rejected as a
configuration error. Browser URL validation remains loopback HTTP because the
adapter derives `ws://host/api/v1/connect` internally. Static application
assets and the initial page remain ordinary HTTP.

## Reference Findings

### RFC 6455

The [WebSocket Protocol](https://www.rfc-editor.org/rfc/rfc6455.html) was
checked for message fragmentation, control frames, closure and recovery:

- section 5.4 permits control frames to be interjected within fragmented data,
  but a WebSocket message is still the unit delivered to the current browser
  API; semantic chunking is therefore a separate protocol decision;
- section 5.5 defines Ping/Pong, requires a Pong response to Ping and bounds
  control frames independently;
- section 7 defines the selected normal, going-away, protocol, policy and
  message-too-big close codes and reserves abnormal-closure `1006`; and
- section 7.2.3 recommends delayed, increasingly backed-off reconnect after
  abnormal closure rather than immediate reconnect storms.

### Locked Rust transport

The locked stack is Axum 0.8.9, Tokio-Tungstenite 0.29.0 and Tungstenite
0.29.0. Exact inspected paths were:

- `axum-0.8.9/src/extract/ws.rs`, where `WebSocketUpgrade` applies explicit
  `max_message_size` and `max_frame_size` configuration;
- `tungstenite-0.29.0/src/protocol/mod.rs`, where default message, frame and
  write-buffer bounds differ from this tactical, Ping is automatically
  answered with Pong while reads are driven, and close state requires an
  orderly sink lifecycle;
- its `size_limiting_text_fragmented` and `size_limiting_binary` tests, which
  prove the configured message bound is applied after fragment assembly; and
- `tokio-tungstenite-0.29.0/src/lib.rs`, whose split stream/sink lifecycle does
  not supply application fairness or task joining for us.

The implementation therefore keeps reading while attached, explicitly drives
the single writer, and treats transport bounds as containment beneath the
application budgets rather than as a scheduler.

### YepAnywhere relay reference

The local YepAnywhere sibling was inspected at
`b47f945700413fe414542ea51a79f826dd76eae9`. This tactical relies on the dossier
in the owning architecture topic, especially:

- `packages/shared/src/relay.ts` for a closed correlated inner message union;
- `packages/client/src/lib/connection/RelayProtocol.ts`,
  `WebSocketConnection.ts` and `SecureConnection.ts` for layered connection
  ownership rather than application HTTP emulation;
- `topics/relay-client-mux.md` and
  `packages/relay/src/mux-handler.ts::RelayMuxCoordinator` for one physical
  relay socket carrying bounded independent circuits; and
- `docs/project/relay-head-of-line-blocking.md` for the concrete failure caused
  by awaiting unrelated request work in one receive path.

RSTorrent adopts the closed inner operation union, correlation and explicit
layering. It does not copy YepAnywhere's authentication, crypto, record
framing, limits, relay protocol or compatibility policy.

## Implementation Sequence

1. Add the transport-independent operation/result DTOs and shared exact-ack
   state machine in `rstorrent-session`; migrate Tauri to that state machine
   with unchanged external behavior.
2. Add generated gateway connection envelopes, errors, limits, schemas and
   TypeScript exports plus negative decoder fixtures.
3. Implement `/api/v1/connect` handshake, authentication, owner generation,
   calls and joined shutdown without view streaming.
4. Add generation-safe attachment takeover, shared acknowledged pumps and
   exact attach/detach/ack transitions.
5. Add the fair outbound scheduler, worst-case byte reservation, heartbeat,
   metrics and large-frame proof.
6. Implement and unit-test `WebSocketApplicationViewClient`, then make it the
   default live-browser adapter with explicit HTTP selection.
7. Run semantic trace parity, reconnect/takeover, browser, controlled live and
   paired transport-performance evidence. Update all owning documents and
   commit reasonable independently green slices.

## Validation Matrix

### Generated and deterministic contract

- Rust and TypeScript schema/generation drift checks cover every frame,
  operation, result, error and limit.
- Golden JSON traces decode on both sides and reject unknown types, fields that
  violate bounds, invalid identifiers, duplicate pending IDs, calls before
  `connect`, attach before initial application, wrong cursor acknowledgements,
  unknown streams and over-limit messages.
- The same snapshot/patch/reset trace produces identical materialized state
  through HTTP long polling, WebSocket and Tauri.
- Shared acknowledgement-state tests cover success, wrong/duplicate ack,
  empty liveness batch, cancellation and close.

### Gateway lifecycle and adversarial concurrency

- Handshake timeout, wrong origin, bad/missing token, version mismatch and the
  third invalid message produce the selected structured outcome and close.
- Two attachments on one socket advance independent cursors. A deliberately
  stalled attachment retains one batch while the other advances and a command
  completes.
- Duplicate owner/view attachment from a replacement generation cancels and
  joins the old pump; late old cleanup cannot touch the replacement.
- Abrupt disconnect retains the view set through its lease. Resume from the
  base and resulting cursors, explicit reset, expiry and fresh reopen are all
  exercised.
- Writer failure, reader failure, application shutdown, client close and
  takeover leave no pump, waiter, call or registry entry unjoined.
- Eight attachments, 16 pending calls, control-queue pressure, two maximum
  reservations and a third waiting stream retain declared memory bounds and
  control fairness.

### Browser and controlled live proof

- Playwright intercepts the default live session and proves exactly one
  `/api/v1/connect` upgrade for a tab and zero semantic HTTP requests to
  `/api/v1/hello`, `/api/v1/commands`, `/api/v1/view-sets` or `/updates` while
  navigating views and dispatching a command.
- A browser test drives two `ViewController` instances through one client and
  observes distinct view-set and stream IDs on the same socket.
- Forced socket loss resumes from the last applied cursor; a shortened lease
  forces the existing fresh-open path. Reducer failure emits no ack.
- `transport=http&poll_ms=100` retains the complete current browser suite and
  records its expected request traffic.
- The existing controlled libtorrent-seeded browser transfer proves exact
  payload, peer/piece/file/disk observation, command receipts, lease and clean
  shutdown under WebSocket delivery.
- The 4,096-file fixture emits an approximately 1.5 MiB snapshot while a
  command is issued. Record snapshot encode/write duration and command-result
  latency; do not claim remote readiness from the loopback result.

### Performance evidence

Record per run:

```text
physical upgrades and reconnects
client/server messages and encoded bytes by frame type
calls/results/errors
view batches, empty batches, acknowledgements and resets
outbound item/byte high water and reservation wait
encode, queue and delivery latency
command latency during ordinary and large-snapshot delivery
final exact payload hash and cleanup outcome
```

Add an opt-in paired 1 GiB `general`-view smoke on the calibrated MacBook using
the same deterministic torrent, pinned libtorrent seeder and run order for
HTTP and WebSocket browser clients. Report throughput and adapter costs; do
not establish a new hard floor from one run. Tactical `057` remains owner of
hardware gates, and later retained calibration may promote a stable
browser-attached ratio there.

### Commands

Run, in proportion to each implementation slice:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cd clients/web
npm run generate
npm run typecheck
npm test
npm run build
npm run e2e
```

Also run `git diff --check`, the targeted gateway lifecycle tests and the
opt-in controlled live/transport smoke. No public swarm or visible desktop
client is required.

## Required Observability

Structured connection observations name only bounded nonsecret identifiers
and aggregate sizes:

- accepted/rejected connections by reason and active/high-water count;
- handshake and connection-generation duration;
- pending-call and attachment count/high water;
- messages and bytes by bounded frame-family label;
- per-stream ready, unacknowledged, reset and delivery-latency counts;
- outbound reserved/queued byte high water and fairness wait;
- ping round-trip, timeout, abnormal close and reconnect count; and
- takeover, retained resume, reset, expiry and fresh-open outcomes.

Do not log bearer tokens, full request/frame bodies, magnets, torrent IDs,
filesystem paths, diagnostics fields or raw view snapshots merely to obtain
transport evidence.

## Stopping And Escalation

This tactical is complete when the default loopback web UI uses one bounded
multiplexed WebSocket for all of its application calls and view sets, exact
acknowledgement and reconnect/takeover proofs are green, HTTP remains an
explicit complete fallback, Tauri behavior is unchanged over the shared ack
core, the controlled browser and paired transport evidence is recorded, the
owning topics are current, and each landed slice is committed.

Routine DTO extraction, gateway task ownership, generated client work, test
fixtures and tuning within the declared bounds do not require more product
direction. Stop and request direction if evidence requires a production
listener, relay/security design, new codec or dependency, public breaking
contract, removal of an existing adapter, semantic cursor change, or
application chunking that materially expands this slice. In particular, do
not weaken the large-snapshot correctness or control-latency proof to avoid a
separate framing decision.
