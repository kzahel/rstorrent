# Tactical 008: Reactive Multi-Surface Control

Status: completed on 2026-07-30.

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

Any JSTorrent UI source adapted into this repository must be identified in the
execution record. JSTorrent and RSTorrent have the same author and copyright
holder, so a separate third-party attribution notice is not required. QuickJS,
daemon, socket proxy, legacy state payloads, and existing JSTorrent
subscription code are not imported.

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
snapshot | patch | reset-required
```

Closure is observable outside the update payload: an in-process iterator
terminates, WebSocket emits its correlated `unsubscribed` response or closes,
and Tauri and Android close their platform subscription handles. A terminal
payload was removed from the planned shape because closing a bounded local
queue cannot reliably enqueue into itself and transport loss is already a
terminal condition. No state depends on receiving a final update.

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

## Implementation Record

### Shared contract and reactive ownership

`rstorrent-session::views` now owns the portable selectors, projections,
delivery policy, snapshots, patches, activity values, reset condition, and
stream envelope. Native `u64` revisions, epochs, sequences, and byte counters
cross JSON as canonical decimal strings. Piece and block indices are `u32`,
and the reducer fixtures cross 65,535.

Each `ApplicationService` owns one `ViewHub`. Every subscriber has an
independent 4 KiB to 4 MiB bounded queue, sequence, delivery clock, coalesced
tail, reset state, and wakeup. Durable transactions refresh views only after
commit. Engine activity enters through `DownloadActivitySink`; it never writes
SQLite or carries payload bytes.

The stricter pause/resume surface cycle found that immediate cancellation
could interrupt selective storage while it was creating the staging-tree and
part-file pair. `DownloadControl::cancel_when_safe` now defers a pause only
across that creation/checkpoint critical section. It remains immediately
cancellable during metadata exchange, transfer, verification, and ordinary
storage work. Shutdown retains its explicit immediate cancellation path.

### Browser and generated TypeScript

`rstorrent-gateway` is a loopback-only Axum WebSocket proof. It requires an
explicit nonempty token and exact allowed origin before dispatch, then bounds
connections, subscriptions, input frames, output messages, and the write
queue. It exposes typed dispatch, subscribe, resync, and unsubscribe
operations rather than generic RPC or filesystem access.

`clients/web` contains the transport-neutral `ApplicationClient`, generated
`ts-rs` declarations, hostile-input validation, continuity reducer,
WebSocket/Tauri adapters, and a basic transfer and piece-activity UI. The
controlled composition path exists only in Vite development builds; the
production bundle was checked for its marker and credentials.

### Desktop

`clients/desktop/src-tauri` owns one in-process `ApplicationService`. Tauri
commands carry correlated dispatch and subscription operations; ordered
Channels carry updates. Subscriptions are keyed by window, cancelled and
joined on window destruction, and do not own the application service.
Explicit application shutdown closes all subscriptions, joins the engine, and
then exits. The desktop contains no native product-content UI or local
WebSocket proxy.

### Android

The existing bootstrap application now has a normal product path in which
`ProductEngineService` owns an `AndroidApplicationClient` independently of
the Activity. UniFFI generates the Kotlin application and session contract
values from the Rust types. The Kotlin adapter atomically reduces independent
summary and zero-delay piece streams into one `StateFlow`; this atomic update
was required to prevent concurrent collectors from overwriting each other's
continuity cursor.

The foreground notification exposes explicit Stop, and the service holds
partial CPU and high-performance Wi-Fi locks only while a download state is
active. Activity recreation and backgrounding detach collection without
stopping the service. This proof uses app-private path storage and does not
enable Android seeding.

`experiments/android-engine-bootstrap/app/src/main/java/org/rstorrent/bootstrap/ui/PieceMap.kt`
adapts the grid sizing, state layering, and color semantics from
JSTorrent's
`android/app/src/main/java/com/jstorrent/app/ui/components/PieceMap.kt` at
commit `0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`. Both projects have the same
author and copyright holder, so no separate third-party license notice is
required. No other JSTorrent source was imported.

## Execution Evidence

The implementation landed in these commits:

- `2f63ec5` — bounded reactive application views and engine activity edges;
- `6a46512` — authenticated browser gateway and shared web client;
- `280cbee` — Tauri command/channel shell;
- `0be44ec` — Android foreground client, generated Kotlin, and Compose UI;
- `78b031e` — controlled Android lifecycle cycle;
- `38795ba` — real TypeScript/WebSocket/gateway cycle;
- `3b9b47a` — rendered Chrome and WebKitGTK cycles plus adapter stress;
- `3811fde` — pause/resume proof, safe storage cancellation, and atomic
  Android stream reduction; and
- `ee75bb2` — deterministic cleanup of the controlled Android profile and UI
  hierarchy artifact.

Validation completed on 2026-07-30:

- workspace formatting, Clippy with warnings denied, unit, architecture, and
  documentation tests;
- deterministic TypeScript regeneration with no diff, type checking, five
  normal Vitest cases, and a production Vite build; the opt-in live Vitest
  case was run separately through the real gateway harness;
- a 1,000-update ordered trace and equivalent explicit overflow reset through
  both frontend transport queues;
- release Tauri build without bundling and a real WebKitGTK development
  webview under Xvfb;
- `rstorrent-android` Clippy/tests with all features, API 28 release builds for
  `x86_64` and `arm64-v8a`, exact UniFFI regeneration, APK assembly, and four
  Kotlin JVM tests; and
- fresh libtorrent `2.0.13.0` magnet-metadata and forced-process-death session
  regressions.

The final host commands were:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace

npm ci --prefix clients/web
npm run generate --prefix clients/web
git diff --exit-code -- \
  clients/web/src/generated/contract.ts \
  clients/web/src/fixtures/reactive-trace.json
npm run typecheck --prefix clients/web
npm test --prefix clients/web
npm run build --prefix clients/web
clients/web/node_modules/.bin/tauri build \
  --config clients/desktop/src-tauri/tauri.conf.json --no-bundle

cargo clippy -p rstorrent-android --all-features -- -D warnings
cargo test -p rstorrent-android --all-features
experiments/android-engine-bootstrap/build.sh

uv run --project tests/interop --locked \
  python tests/interop/gateway_reactive_surface.py
uv run --project tests/interop --locked \
  python tests/interop/browser_reactive_surface.py \
  --chrome /usr/bin/google-chrome
uv run --project tests/interop --locked \
  python tests/interop/tauri_reactive_surface.py
uv run --project tests/interop --locked \
  python tests/interop/android_reactive_surface.py \
  --serial emulator-5554 --adb "$HOME/Android/Sdk/platform-tools/adb"
uv run --project tests/interop --locked \
  python tests/interop/session_resume.py --runs 1
uv run --project tests/interop --locked \
  python tests/interop/magnet_metadata.py --runs 1
```

The final controlled surface results were:

| Surface | Result |
| --- | --- |
| TypeScript adapter + real gateway | 3 pieces, positive requested/received/stored activity, exact SHA-1, joined gateway shutdown |
| Headless Chrome + WebSocket | 3 pieces, pause/resume, rendered live activity, exact SHA-1, joined gateway shutdown |
| Tauri WebKitGTK + Channels | 3 pieces, pause/resume, exact SHA-1, joined application shutdown |
| API 34 `jstorrent-tablet` AVD | 8 pieces, 60 view updates, pause/resume, Activity recreation/background, exact SHA-1, notification Stop and joined shutdown |
| Physical Pixel 7a, API 37, `lynx` | 8 pieces, 57 view updates, pause/resume, Activity recreation/background, exact SHA-1, notification Stop and joined shutdown |

The Android harness refuses a locked target and never sends a power or lock
key event. The physical Pixel was unlocked before the run. The harness
targeted its resolved serial, did not touch the other attached Android-class
device, removed its ADB reverse mapping and UI hierarchy, and cleared the
controlled application profile after verification.

The browser, Tauri, and Android runs used loopback libtorrent seeds with
per-handle upload throttling so pause acted during an active transfer. All
published payloads matched their fixture SHA-1. Temporary profiles, payloads,
ADB reverse mappings, and seed sessions were removed.

Follow-up validation on Apple silicon macOS on 2026-07-31 completed the npm
lock with cross-platform optional peer entries and converted the existing
desktop icon from 16-bit to 8-bit RGBA, as required by Tauri's macOS runtime.
`./scripts/desktop` then performed a clean locked dependency install, built the
production web assets and native desktop binary, and remained running until
stopped from its attached terminal. Deterministic TypeScript generation,
type checking, five Vitest cases, the production Vite build, workspace
formatting, Clippy with warnings denied, workspace tests, and one fresh
bidirectional libtorrent `2.0.13.0` magnet-metadata run also passed.

A second macOS follow-up closed the bundled app's only window while confirming
that the same process and application service remained alive. Activating the
app again recreated and focused the configured main window, whose fresh view
subscriptions reconnected to the retained profile state. Subscription cleanup
is tagged with a window generation so delayed destruction work from the old
window cannot cancel subscriptions owned by its replacement.

## Remaining Boundary

This is a product-thread proof, not a production remote-control release. The
next client work should choose one bounded concern: production pairing and
authorization, desktop tray/window policy, or Android SAF-backed durable
session storage. The separate HTTP playback data plane, relay/push wake-up,
multi-torrent scheduling, and stable public compatibility remain later work.
