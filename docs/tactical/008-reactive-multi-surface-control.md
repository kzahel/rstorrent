# Tactical 008: Reactive Multi-Surface Control

Status: planned.

## Motivation And Outcome

Tactical `007` established one durable application-service authority and a
transport-neutral command envelope. Its complete snapshots are sufficient for
diagnostics but not for real interfaces. The Android experiment still calls a
narrow engine binding directly, no desktop product exists, and RSTorrent has
not proved that one application contract remains usable from Kotlin,
TypeScript, local IPC, and a bounded remote transport.

The next architectural pressure should come from actual consumers rather than
another speculative service layer. Build one small real interface thread
across:

1. a browser-hosted web application using an authenticated WebSocket;
2. the same web application inside a Tauri desktop shell using commands and
   ordered channels; and
3. an Android Compose surface using UniFFI and foreground-service ownership.

All three consume generated representations of the same Rust command and
reactive-view types. They display the same controlled magnet download's
torrent summary, verified pieces, and live active block state. The stopping
condition is semantic convergence and lifecycle evidence, not UI polish.

## Dependencies And References

- [`../topics/application-control.md`](../topics/application-control.md)
- [`../topics/client-persistence.md`](../topics/client-persistence.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../engineering-principles.md`](../engineering-principles.md)
- [`007-durable-session-control.md`](007-durable-session-control.md)
- [`004-android-engine-bootstrap.md`](004-android-engine-bootstrap.md)
- The controlled libtorrent session-resume fixture under `tests/interop/`
- UniFFI `0.31.0`, already locked by Tactical `004`
- Tauri v2 commands and ordered channel API
- The sibling JSTorrent checkout recorded by `reference/pins.toml`

The initial JSTorrent source reference is commit
`0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`, licensed under the MIT text in
its `LICENSE.md`. If synchronization changes the managed revision before code
is imported, update this section and the execution record before copying.

## Scope

### Portable contract types

Keep the semantic contract in Rust above `rstorrent-engine`. Add typed,
serde-serializable records and tagged enums for:

- subscription selector and named projection;
- bounded delivery policy;
- torrent summary;
- verified piece ranges and active block state;
- initial view snapshot;
- typed view patch;
- stream update envelope;
- resynchronization and terminal stream conditions; and
- bounded WebSocket client and server envelopes.

Export deterministic TypeScript declarations from those definitions. Kotlin
uses UniFFI-generated records and enums from the same definitions rather than
hand-maintained DTO mirrors.

Represent sequence, revision, and counter values without JavaScript precision
loss. Do not serialize native pointers, task handles, paths, descriptors,
SQLite rows, payload buffers, peer messages, or diagnostic logs.

### Reactive view ownership

Add an instance-owned reactive-view hub to `ApplicationService`. A
subscription:

- receives one coherent snapshot before patches;
- has its own stream identity, epoch, sequence, view revision, queue, delivery
  clock, and close state;
- cannot consume or clear another subscriber's changes;
- bounds queued state before accepting updates;
- coalesces replaceable summary and active-block state;
- unions verified ranges and preserves explicit clears;
- reports overflow or invalid continuity as resynchronization rather than
  silently diverging; and
- wakes and terminates observably without periodic busy polling.

Extend the engine's existing coarse `DownloadControl` observation with a
bounded activity sink for block requested, received, stored, and piece
verified edges. The engine defines these activity facts but remains
independent of UI projections, transports, TypeScript, Kotlin, Tauri, and
WebSocket.

Checkpoint-driven durable state changes refresh the view hub only after their
database transactions succeed. Active block edges never mutate SQLite.

### Shared TypeScript client and web proof

Add one small web workspace containing:

- generated contract declarations;
- runtime validation for untrusted WebSocket envelopes;
- an `ApplicationClient` interface;
- a deterministic snapshot/patch reducer;
- WebSocket and in-memory client implementations; and
- a basic UI for add magnet, torrent summaries, pause/resume, verified pieces,
  and active blocks.

The browser proof connects to a loopback WebSocket gateway with an explicit
bounded token. The gateway:

- defaults to loopback and refuses an empty credential;
- authenticates before commands or subscriptions;
- bounds frame length, connections, subscriptions, and queued writes;
- checks the configured browser origin;
- dispatches the same application commands as local callers;
- gives each connection independent subscriptions;
- cancels forwarding tasks on disconnect; and
- exposes no filesystem path, payload bytes, arbitrary SQL, or generic RPC.

This is transport and client evidence, not production remote access. No
default LAN listener, TLS deployment, pairing UX, relay, account, wake-up, or
public compatibility promise is in scope.

### Tauri proof

Embed the same web build in a minimal Tauri v2 shell. Implement
`ApplicationClient` using:

- Tauri commands for correlated command dispatch; and
- ordered Tauri channels for subscription updates.

The Rust shell owns one `ApplicationService` independently of the webview.
Closing or recreating the window closes its subscriptions but does not imply
application-service shutdown. Native desktop content UI, sidecars, a local
control socket, HTTP playback, autostart, installers, updates, and production
tray policy are not part of this tactical.

Record a comparable synthetic high-rate update trace through Tauri channel
and WebSocket adapters. Do not claim either transport faster without measured
evidence.

### Android proof

Extend the established Android binding so a foreground service owns one
durable application-service object rather than only the Tactical `004`
diagnostic `EngineSession`. Expose:

- open with one app-private profile and path-backed storage root;
- typed command dispatch;
- typed subscription creation;
- suspendable next-update;
- explicit subscription close; and
- shutdown and joined termination.

Add a minimal Compose surface that adapts the JSTorrent piece visualization
and presents the same torrent list, summary, and piece/block state as the web
view. The activity may be recreated or finished while the foreground service
and controlled download remain active.

Any JSTorrent UI source copied into this repository must be identified in the
execution record and covered by retained MIT attribution. QuickJS, daemon,
socket proxy, legacy state payloads, and existing JSTorrent subscription code
are not imported.

Use app-private path storage for this contract/UI proof. Connecting durable
session recovery to SAF remains a later storage tactical; Kotlin still carries
no file or piece payload.

## Contract Shape

The initial subscription dimensions are deliberately named and bounded:

```text
selector:
  torrent collection
  one torrent

projection:
  summary
  piece activity

delivery:
  minimum emission interval
  maximum queued bytes
```

Every update contains:

```text
contract version
stream identity
stream epoch
sequence
base view revision
resulting view revision
snapshot | patch | reset-required | closed
```

Piece activity contains:

- exact piece count;
- a bounded canonical set of verified piece ranges;
- explicit cleared ranges;
- bounded active pieces;
- per-active-piece block length and requested, received, and stored block
  ranges; and
- no piece payload.

Indices are at least 32-bit and tests cross the historical 65,535 boundary.
Range encoding validates ordering, uniqueness, non-overlap, and torrent
bounds before mutation.

## UI Boundary

The shared web presentation sees only `ApplicationClient`. It must build and
run in a normal browser without importing Tauri modules in components or
reducers. Tauri integration is selected in the composition root.

Android presentation sees a Kotlin repository exposing atomic `StateFlow`
values. Snapshot and patch reduction happens once before emission; individual
screens do not independently merge overlapping bitfields and change lists.

Unsupported product capabilities are absent or visibly disabled. The proof
does not synthesize peers, trackers, files, rates, names, DHT, queueing, or
seeding data that the current engine does not own.

## Required Failure And Edge Profiles

### Independent subscribers

Run a zero-delay piece-activity subscriber and a deliberately slow economical
summary subscriber concurrently. Both begin from coherent snapshots and reach
the same terminal torrent state. Neither steals changes from the other.

### Coalescing

Generate repeated summary replacements, verified additions and clears, and
active block replacements faster than a subscriber consumes them. Require
deterministic merge results and a queue high-water no greater than its
configured byte bound.

### Overflow and resynchronization

Use an intentionally tiny queue so an update cannot be preserved. Require an
explicit reset condition, then a fresh snapshot that converges with current
state. Never silently skip a sequence or apply a patch to the wrong base
revision.

### Large indices

Reduce a synthetic piece snapshot and patches containing indices on both sides
of 65,535 and near the configured maximum. Require exact Rust, TypeScript, and
Kotlin representation without truncation.

### Disconnect and cancellation

Close WebSocket, Tauri webview subscription, and Android activity collection
while updates are active. Subscriber tasks terminate, queues release, and the
application service remains under its platform owner. Explicit service stop
cancels and joins the engine.

### Hostile network input

Reject before dispatch:

- unauthenticated messages;
- wrong contract versions;
- unknown tagged variants;
- malformed identifiers and decimal counters;
- oversized frames and collections;
- invalid range ordering or overflow;
- excess subscriptions; and
- stale or unknown subscription identities.

## Validation

### Rust

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Add deterministic unit and integration coverage for reduction, queue bounds,
coalescing, independent subscriptions, overflow, close/wake behavior, engine
activity edges, WebSocket authentication and bounds, and application-service
updates.

Run the existing controlled persistence and protocol regressions in
proportion to changed code.

### Generated TypeScript and web

The generated declarations must be clean and reproducible. Run:

```bash
npm ci
npm run typecheck
npm test
npm run build
```

TypeScript reducer fixtures consume Rust-produced JSON traces and must reach
the recorded final state. Browser tests cover both in-memory and WebSocket
clients.

### Tauri

Run the Tauri Rust checks and build or bundle the development shell where the
configured Linux environment permits. Exercise command correlation, initial
snapshot, ordered patches, subscription close, and window recreation through
the Tauri adapter.

### Android

Cross-compile both established Android ABIs at API 28, regenerate Kotlin from
the exact native library, and build the application. Run the contract reducer
tests on the JVM and one controlled download cycle on the API 34
`jstorrent-tablet` AVD. Run a physical Pixel 7a cycle if it remains attached.

The Android cycle must show foreground-service ownership, activity recreation,
live piece/block updates, terminal completion, explicit subscription close,
joined shutdown, and zero engine-owned buffered payload at termination.

### Interoperability

Run at least one fresh controlled libtorrent magnet/session cycle through the
new application service while recording its view trace. Exact payload
verification and existing bounded high-water assertions remain authoritative.

## Contracts And Invariants

- One application-service instance and profile database remain the mutation
  authority.
- Commands, snapshots, patches, logs, and payload data remain distinct.
- Every stream is recoverable from a coherent snapshot.
- Subscriber queues are bounded independently of swarm, piece, and file size.
- Coalescing preserves final view state or explicitly requires reset.
- One subscriber cannot clear, delay, or authorize another.
- Durable revision does not advance for volatile block edges.
- Generated TypeScript and Kotlin representations originate from the Rust
  contract rather than handwritten mirrors.
- Tauri and WebSocket adapt to the same semantics without forcing local
  networking.
- The browser build contains no ambient Tauri dependency.
- Android Activity and Tauri window lifetimes do not own engine tasks.
- Rust owns peer networking, storage, hashing, and payload movement.
- No unauthenticated network command reaches the dispatcher.
- All adapter and forwarding tasks have cancellation and observable
  termination paths.

## Non-Goals

- a polished or feature-complete product UI
- tracker, DHT, PeX, ordinary peer discovery, or multi-peer scheduling
- general multi-torrent concurrency
- production remote access, relay, accounts, pairing, or push wake-up
- public stable wire compatibility
- HTTP playback URLs or media streaming
- desktop native content UI
- production tray, installer, updater, autostart, or file associations
- Android SAF-backed durable session recovery
- Android background seeding
- importing JSTorrent QuickJS, daemon, or legacy subscription architecture

## Stopping Condition

This tactical is complete when one controlled real download can be added,
observed through coherent summary and live piece/block views, paused or
resumed, and completed from:

- the standalone browser build over authenticated WebSocket;
- the same web build in Tauri over commands and ordered channels; and
- Android Compose over UniFFI under foreground-service ownership.

Generated TypeScript and Kotlin types must come from the Rust contract.
Independent fast and slow subscribers, overflow/resnapshot, large piece
indices, disconnect, and task termination must have recorded executable
evidence. Exact UI polish, public remote deployment, SAF session recovery, and
HTTP playback remain explicitly bounded later work.
