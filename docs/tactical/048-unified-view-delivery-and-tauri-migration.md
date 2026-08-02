# Unified View Delivery And Tauri Migration

Status: Planned.

Topics: `application-view-api`, `client-surfaces`, `web-ui-design`,
`desktop-inspection-surface`, `application-control`

## Motivation

The leased view-set contract now drives the live React inspection surface over
authenticated HTTP long polling. The desktop product still selects the legacy
direct-DOM frontend and Tactical `008`'s independent Tauri push subscriptions.
The retained `/control` WebSocket client implements that same older stream
contract. Rust deliberately fans application changes into both systems during
the transition, but the frontend does not yet have interchangeable delivery
adapters for one semantic API.

Adding the categorized Logs experience directly on this transition would leave
its lifetime, filtering, and ordered-event pressure coupled differently to the
browser and desktop products. Complete the client migration first: HTTP long
polling and a Tauri Channel must consume the same `UpdateBatch` values, through
the same continuity reducer and Zustand transaction, from the same leased view
set. The desktop must then host the already-proven React inspection application
without binding a loopback control server.

This is a client and platform-adapter cleanup. It does not redesign the Rust
view owners, add a production remote service, or implement the later Logs
capture/filtering slice.

## Stable Scenarios

- The browser-hosted inspection application continues to open one leased view
  set and consume its batches through bounded HTTP long polling.
- The Tauri-hosted inspection application opens the same semantic view set
  through in-process commands and consumes the same `UpdateBatch` values over
  one Tauri Channel.
- A streamed batch is acknowledged only after validation, continuity
  reduction, and the synchronous application-state callback complete. A batch
  rejected by validation or reduction is never acknowledged.
- A stream disconnect can reattach at the last applied cursor and receive a
  retained replay. An expired/closed view set causes the existing controller
  to open a fresh set and atomically install coherent snapshots.
- Desired-view replacement, commands, shutdown, browser suspension recovery,
  window destruction, and macOS close/reopen retain their current semantics.
- Closing a React application, destroying its Tauri window, or shutting down
  the product cancels and joins every stream pump and closes the corresponding
  view sets without stopping downloads merely because a window closes.
- A future WebSocket adapter can implement the same stream interface without
  changing `InspectionApplication`, the reducer, Zustand, or React.
- A future binary codec can decode frames into the same generated semantic DTOs
  without creating another view API. This slice selects no binary encoding.

## Scope

### One semantic TypeScript client

Keep the existing v1 operations and generated values as the application API:

- `hello`;
- command dispatch;
- open and update a leased view set;
- pull or stream `UpdateBatch` values from an applied cursor; and
- close the view set and client.

Refine the TypeScript client boundary so delivery is an adapter capability
rather than a polling assumption in `ViewController`. HTTP retains one
in-flight `nextUpdates` pull. A streaming client supplies one closable async
batch stream. The controller chooses the available delivery, but both paths
run the same validation, `reduceUpdateBatch`, state callback, retry, reopen,
and cancellation logic.

Introduce one transport-neutral application-view error carrying the structured
error code used for recovery decisions. The HTTP error remains an adapter
specialization; Tauri invocation errors map to the same semantic codes.

The update stream owns transport acknowledgements. Requesting the next
iterator item acknowledges the previously yielded batch, which can happen only
after the controller has successfully reduced and applied it. Stream framing,
Tauri channels, sockets, codecs, and abort handles remain outside reducer and
Zustand state.

### In-process Tauri view-set adapter

Add Tauri commands for:

- adapter-specific `hello` capability reporting;
- view-set open and desired-view update;
- stream attach, acknowledgement, and close;
- explicit view-set close; and
- the existing semantic command dispatch.

Each webview generation receives a trusted in-process `ViewSetOwner`; no owner
identity arrives from JavaScript. The desktop tracks only the view-set IDs and
stream pumps allocated by that window generation. A Tauri stream attaches to
an already-open view set at an explicit cursor and sends tagged batch or error
events through a Channel.

The Rust pump retains at most one unacknowledged batch. After sending it, the
pump waits for an exact cursor acknowledgement before asking the view set for
the next batch. The existing view-set cursor therefore remains the continuity
and replay authority rather than a Tauri-local sequence. Empty long-wait
heartbeats also require acknowledgement so an unresponsive webview stops
refreshing its lease and expires normally.

Window destruction cancels and joins legacy subscriptions and new stream
pumps belonging to that exact generation, then closes its tracked view sets.
Application shutdown performs the same cleanup before joined service shutdown.
Reopening the macOS window installs a fresh generation and cannot inherit the
destroyed webview's view resources.

### Desktop frontend migration

Detect the Tauri runtime at the web entry point and construct the React
`InspectionApplication` with the new in-process client. Demo URLs and explicit
`?live=` loopback-browser URLs retain their current adapters. A plain non-Tauri
browser may retain the legacy proof entry during this slice; it is not the
product desktop path.

`./scripts/desktop` must continue to install locked dependencies when needed,
build the production frontend, and launch the in-process online product. The
automated gate builds Tauri without launching or focusing a window.

### Compatibility and retirement boundary

Preserve Tactical `008`'s legacy `ApplicationClient`, Tauri subscription, old
WebSocket `/control`, Android UniFFI subscriptions, generated legacy types, and
their existing tests. They remain explicitly compatibility paths rather than
the architecture for new React views. Deleting or migrating Android and the
old WebSocket proof is separate work after all consumers are inventoried.

The new Tauri adapter does not reuse the loopback HTTP gateway internally.
The browser gateway remains the headless automation host and later remote
WebSocket streaming remains a delivery adapter, not a local desktop daemon.

## Reference Dossier

There is no BitTorrent protocol or engine transition in this slice, so no BEP
or libtorrent source dossier applies.

Repository architecture and prior evidence:

- `docs/topics/application-view-api.md` defines one semantic view set with
  interchangeable pull/stream delivery and JSON/future-binary codecs.
- Tactical `033` implements the task-free leased owner, generated v1 contract,
  JSON long-poll adapter, pure reducer, and controller while deliberately
  preserving the legacy subscriptions.
- Tactical `035` implements self-expiring leases, browser-suspension recovery,
  semantic desired views, and the live `InspectionApplication`.
- Tactical `008` owns the legacy WebSocket/Tauri/Android subscriptions that
  remain compatible during this migration.
- Locked Tauri Rust `2.11.5` `src/ipc/channel.rs` guarantees ordered Channel
  messages and exposes a synchronous send result; locked
  `@tauri-apps/api` `2.11.1` supplies the typed JavaScript Channel callback.

The current RSTorrent clients provide the failure oracle:

- `clients/web/src/view-controller.ts` hardcodes the pull loop;
- `clients/web/src/api/client.ts` exposes the new view-set operations only
  through the HTTP client;
- `clients/web/src/tauri-client.ts` and
  `clients/desktop/src-tauri/src/lib.rs` implement the legacy independent
  subscription path; and
- `clients/web/src/main.ts` routes default Tauri startup into
  `legacy-main.ts`.

No reference source, test fixture, or UI asset is copied.

## Accepted Architecture

```text
Rust application view owners
        -> leased view set (epoch, cursor, bounded accumulator)
             |
             +-- HTTP pull + JSON -----------+
             |                                |
             +-- Tauri Channel + explicit ack+--> validated UpdateBatch
             |                                |           |
             +-- later WebSocket/codec -------+      pure reducer
                                                            |
                                                     Zustand + React
```

Transport responsibilities end at a decoded `UpdateBatch`. Delivery choice
does not alter named view specifications, snapshot/patch meaning, cursor
continuity, ordered diagnostic semantics, or client materialization.

The first Tauri streaming acknowledgement sequence is:

```text
Rust next_updates(applied cursor)
  -> Channel batch
  -> TypeScript runtime validation
  -> reduceUpdateBatch
  -> synchronous state callback/store update
  -> iterator requests next item
  -> Tauri ack(batch cursor)
  -> Rust next_updates(acknowledged cursor)
```

## Owners, Tasks, And Cancellation

| Owner | State and work | Termination |
| --- | --- | --- |
| Rust `ViewHub` | Semantic snapshots/diffs and leased view sets | Application shutdown joins the lease reaper and closes all sets |
| Tauri window generation | Trusted owner identity and tracked view-set IDs | Exact window destruction or application shutdown closes its sets |
| Desktop stream registry entry | Cancellation token, bounded ack sender, pump task, view-set association | Explicit stream close, view-set close, window destruction, or shutdown cancels and awaits it |
| Stream pump | One pending `next_updates`, one sent batch, and one expected acknowledgement | Cancellation wins every wait; terminal view-set/channel error is reported once and the task exits |
| TypeScript stream adapter | Channel callback, at most one delivered/unacknowledged batch, waiters, and close | Abort, iterator return, client close, or terminal event invokes stream close |
| `ViewController` | Desired specs, current ID/epoch/cursor, one pull or stream consumer, retry/reopen | Application close aborts and joins consumption before closing the view set |

No React component owns a stream, poll, retry timer, or Tauri invocation. No
task handle enters the semantic view owner, pure reducer, or Zustand store.

## Invariants And Bounds

- HTTP and Tauri consume the same generated `UpdateBatch` semantics and use
  the same pure reducer.
- A view-set ID locates a resource but never authenticates a remote caller.
  Tauri owner authority is installed by the native adapter.
- One view set has only one active consumer. Switching or retrying delivery
  closes the old consumer before attaching another.
- A streamed batch is never acknowledged on Channel send, receipt, validation
  failure, reducer failure, or state-callback failure.
- At most one batch is awaiting a Tauri acknowledgement and the ack channel is
  bounded to one entry. Existing view-set queue and snapshot bounds remain the
  authoritative producer memory ceilings.
- Wrong, stale, duplicate, or out-of-order acknowledgements cannot advance the
  Rust cursor. Continuity failure becomes an explicit stream error/reset path.
- Closing and cancellation are idempotent. Every native stream task has an
  observable awaited termination path.
- Window-generation cleanup cannot close resources created by a later macOS
  window generation with the same label.
- A suspended or abandoned client cannot be kept alive by engine publication
  or unconditional native heartbeats.
- Adapter `hello` reports only delivery modes and encodings that adapter can
  actually use. Tauri reports stream; HTTP reports poll/long-poll.
- JSON remains the only implemented wire codec. Codec choice stays below
  generated DTO validation and cannot change semantic field meaning.
- Commands, current-state views, ordered diagnostics, and transport failures
  remain distinct; neither UI nor adapter parses log text as state.
- The desktop continues to use `NetworkPolicy::Online`; adapter migration does
  not alter torrent networking, storage, persistence, or scheduling.

## Shape-Changing Edge Cases

- a Channel batch arrives before the invoke that attached the stream resolves;
- close or window destruction races stream attach, a blocked view wait, batch
  send, or acknowledgement;
- a command or view change occurs while a stream waits;
- a batch fails runtime schema validation, epoch/base-cursor continuity, or
  the state callback;
- stream termination before and after one unacknowledged batch;
- wrong, duplicate, stale, and future acknowledgement cursors;
- server view-set expiry while the Tauri stream is blocked or waiting for ack;
- macOS close, dock-icon reopen, and a late cleanup callback from the old
  generation;
- application shutdown with legacy subscriptions and new streams together;
- browser tab suspension and replacement-set recovery on the retained HTTP
  adapter; and
- an adapter advertises stream but fails to attach, requiring bounded retry
  without creating a second consumer.

## Implementation Order

1. Record this tactical and the transition/retirement boundaries.
2. Add the transport-neutral application-view error and closable async stream
   capability; refactor `ViewController` to share reduction and recovery across
   pull and stream consumption.
3. Add deterministic TypeScript adversarial tests for post-reducer ack,
   rejected-batch non-acknowledgement, replay/retry, stream close, and lease
   reopen without weakening existing poll tests.
4. Add native Tauri view-set commands, trusted window owners, explicit stream
   ack, bounded registries, and exact-generation joined cleanup.
5. Add native pump tests for single-unacknowledged-batch behavior, exact ack,
   cancellation, and error mapping.
6. Implement the Tauri application-view client, validate every IPC value with
   the generated contract, and test Channel ordering, early delivery, errors,
   abort, and close through an injected bridge.
7. Route Tauri startup to the React inspection application while retaining demo,
   explicit browser-live, and legacy browser proof entries.
8. Run TypeScript, production browser, Tauri no-window build, Rust, and
   controlled headless live evidence; update the owning topics and this record.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure client | identical reducer state for pull and stream traces; ack only after successful application; failure remains unacknowledged |
| Tauri TypeScript adapter | IPC value validation, early Channel batch, ordered one-at-a-time consumption, structured errors, abort, and idempotent close |
| Native Tauri adapter | trusted per-generation ownership, exact ack, bounded stream registry, cancellation/join, view-set close, and shutdown cleanup |
| Browser regression | existing long-poll live, suspension recovery, responsive UI, and command paths remain green |
| Desktop packaging | production web build and `tauri build --no-bundle` compile the new default entry without launching a window |
| Repository | generated-contract drift, formatting, Clippy with warnings denied, workspace tests, frontend typecheck/tests/build/E2E |

Automated work must not launch a visible Tauri window, normal browser, emulator,
or physical device. The existing headless loopback gateway remains the live UI
evidence seam. No public swarm is required.

## Non-goals

- the categorized/filterable Logs UI, diagnostic capture-interest policy, or
  two-tier log retention;
- implementation or selection of WebSocket view streaming, CBOR, MessagePack,
  protobuf, or another binary codec;
- deletion of the old `/control` WebSocket, legacy frontend, Android
  subscription adapter, or generated legacy contract;
- a loopback server inside the Tauri product, LAN/remote access, pairing,
  accounts, TLS, relay, or public API compatibility;
- Android UI or UniFFI migration;
- engine, tracker, DHT, peer, scheduler, storage, torrent lifecycle, or
  performance behavior changes;
- a new dependency, router, state library, or framework; or
- visible/manual desktop validation owned by automation.

## Stopping Condition

This slice is complete when the production Tauri entry uses the React
inspection application in-process; HTTP long polling and acknowledged Tauri
streaming apply the same generated batches through the same controller,
reducer, and Zustand path; cancellation, replay, expiry, window destruction,
and shutdown are bounded and tested; the legacy adapters remain green but are
clearly outside the new-view path; no visible product client was launched; all
proportional and full repository gates pass; the owning topics record the
implemented status and deliberate deferrals; and every logical slice is
committed with a clean worktree.

## Escalation Contract

Transport-interface refactoring, Tauri commands and Channel pumps, exact
window-generation resource tracking, structured adapter errors, production
entry migration, direct/headless tests, generated artifacts caused by accepted
semantic shape changes, and topic updates are authorized. Stop for direction
if evidence requires changing view-set cursor semantics, adding a dependency,
exposing a network listener in Tauri, selecting a binary encoding, deleting an
Android/legacy compatibility path, changing durable commands or torrent data,
launching a visible/physical client, or expanding into the Logs feature itself.

## Implementation And Evidence

Pending.
