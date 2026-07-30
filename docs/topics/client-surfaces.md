# Client Surfaces

Topic: `client-surfaces`

Status: Accepted and implemented through Tactical `008` across a
browser-hosted web view, a Tauri desktop webview, and Android Compose.

## Scope

This topic owns the product-client surfaces above the shared application
service: the reusable web application, desktop shell, Android presentation
adapter, generated client types, and the boundary between presentation and
application control.

It does not own torrent protocol behavior, payload storage, the future remote
account or relay system, or the future HTTP playback-content server.

## Accepted Product Shape

RSTorrent has three initial user-interface surfaces:

- a browser-hosted web application capable of using an authenticated remote
  control transport;
- a Tauri desktop application embedding the same web application; and
- an Android Compose application adapted from the existing JSTorrent Android
  product.

Desktop product content is web UI. Native desktop code is limited to the
application shell and operating-system integration such as process and window
lifecycle, tray and menus, notifications, startup, file associations, native
dialogs, deep links, and updates.

The Tauri and browser builds share components, reducers, generated TypeScript
contract types, and one transport-neutral `ApplicationClient` interface.
Their transports differ:

- Tauri uses native commands for request/response and ordered channels for
  subscription updates by default.
- The browser uses an authenticated WebSocket gateway.

Transport reuse is not an end in itself. A local desktop product does not open
a listener, allocate a port, serialize through a socket, or acquire network
authentication machinery merely to resemble the remote path. Both transports
adapt to the same semantic dispatcher and reactive-view contract.

Android uses UniFFI-generated Kotlin records, enums, objects, and suspend
functions. A small Kotlin adapter exposes subscription handles as lifecycle-
aware `Flow`s and maps portable application values into presentation models.
Peer payload, piece payload, file payload, and hashing do not cross UniFFI.

## UI Reuse Direction

The existing JSTorrent Android application is the primary Compose product
reference. Reuse or adapt its screen composition, navigation, piece
visualization, notification and power-management behavior, SAF root-selection
experience, and platform lifecycle lessons.

Do not preserve its QuickJS controller, stringly typed subscription topics,
combined mutable `EngineState`, daemon topology, or view-model workarounds as
compatibility requirements. New Kotlin repositories consume typed RSTorrent
views and emit atomically reduced presentation state.

Any copied source must record its exact JSTorrent revision and MIT provenance,
retain required copyright and permission notice coverage, and be reduced to
the portion actually reused. Tactical `008` uses the sibling revision recorded
in `reference/pins.toml` and identifies every imported file in its execution
record.

## Shared Web Application

Web presentation code depends on a narrow generated client package, not on
Tauri globals or WebSocket APIs directly:

```text
shared components and reducers
             |
     ApplicationClient
       /           \
Tauri transport   WebSocket transport
```

Platform-specific capabilities such as folder selection, tray state, external
URL opening, or updater presentation use a separate platform-capability
adapter. They do not enter torrent commands or view patches.

The initial browser surface may prove the network transport on loopback with
explicit test credentials. That is not a claim of safe Internet exposure.
LAN binding, TLS, pairing, accounts, relay operation, wake-up delivery, remote
authorization policy, and deployment require later threat models and
tacticals.

## Reactive Views

Presentation consumes named, versioned application views rather than arbitrary
field queries or engine structs. Each subscription selects:

- an application object or collection;
- a named projection such as summary or piece activity; and
- a bounded delivery policy.

Every stream begins with a coherent snapshot. Typed patches carry a stream
epoch, sequence, base view revision, and resulting view revision. Sequence
gaps, incompatible epochs, invalid patches, or bounded-queue overflow require
resynchronization from a snapshot; events are never the only recovery
authority.

Engine edges feed per-subscriber accumulators. Delivery may coalesce without
losing the final state:

- torrent summaries use keyed latest-value upserts and explicit removal;
- verified piece changes union ranges while recheck can explicitly clear
  ranges;
- active block state uses the latest bounded mask for each active piece;
- counters and rates sample or conflate instead of enqueueing every byte; and
- a slow subscriber cannot consume, clear, or delay another subscriber's
  updates.

Durable application revision and volatile stream sequence are separate.
High-frequency block activity neither writes SQLite nor advances the durable
command revision.

## Portable Contract Types

Rust application-contract records and enums are the canonical semantic source.
The contract uses a deliberately portable subset with explicit representations
for identifiers, byte strings, large counters, optional values, and tagged
variants.

- serde defines the diagnostic and initial network representation;
- generated TypeScript exposes discriminated unions and client inputs;
- UniFFI generates Kotlin values for the in-process Android boundary; and
- generated or fixture-backed validators reject untrusted network values at
  runtime.

JavaScript cannot represent every Rust `u64` exactly. Portable revisions,
sequences, and unbounded counters use an explicit decimal representation or a
proved safe bound rather than silently rounding JSON numbers.

Generated artifacts are deterministic and checked for drift. Rust, TypeScript,
and Kotlin reducer fixtures must converge on the same state.

## Lifecycle

View clients are detachable. Closing a browser tab, Tauri window, or Android
activity closes its subscriptions without stopping the application service or
active download.

- Tauri owns the application service independently of its webview window and
  may remain alive in the tray.
- Android's foreground service owns the application service independently of
  activities.
- A remote WebSocket connection owns only its authenticated commands and
  subscriptions.

Every subscription and adapter task has explicit close, cancellation,
termination observation, and bounded queued state.

## Future HTTP Playback Data Plane

Desktop and Android are expected eventually to expose an embedded HTTP server
that can mint short-lived capability URLs for playing torrent files on local
players, televisions, or other devices. That server is a content data plane,
not the command and reactive-view transport.

It will need explicit interface binding, authentication, expiry and
revocation, `HEAD` and range behavior, verified-range integrity, incomplete
file scheduling, connection and buffer bounds, and platform lifecycle policy.
File bytes flow between Rust storage and HTTP clients without crossing the UI
contract. Tactical `008` does not implement or reserve a wire format for this
future server.

## Current Gaps

- The loopback WebSocket gateway is an authenticated proof, not a production
  remote-access design. It has no pairing, principal/capability model, TLS,
  relay, wake-up path, or public wire compatibility promise.
- The Tauri shell has no production tray/window policy, installers, updates,
  file associations, or platform-capability adapter.
- Android uses app-private path storage for the product proof. Its durable
  application service is not yet connected to the established SAF descriptor
  and storage-root flow.
- The current UI proves one controlled torrent. General multi-torrent
  scheduling and complete product presentation remain absent.
- No HTTP playback server exists.

Tactical `008` records the implemented contract, exact controlled evidence,
and bounded deferrals. The most useful next client slice is one of production
remote authorization, desktop lifecycle/tray integration, or SAF-backed
Android session storage rather than unrelated UI breadth.
