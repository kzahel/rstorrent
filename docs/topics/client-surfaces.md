# Client Surfaces

Topic: `client-surfaces`

Status: Accepted and implemented through Tacticals `008` and `009` across a
browser-hosted web view, a Tauri desktop webview, and Android Compose. The
Android foreground product client now uses durable SAF storage without
placing platform capabilities in the portable UI command contract. The
one-command desktop launcher is smoke-tested on Apple silicon macOS as well as
the original Linux development host. Closing its macOS window detaches that
view while Dock activation recreates and focuses it without restarting the
application service. Tactical `012` completed isolated headless Chrome and
no-window Android AVD harnesses plus equivalent bounded diagnostics and
progress explanations on the shared web and Android Compose surfaces.
Tactical `013` made desktop and Android select online torrent networking while
the browser gateway and presentation harnesses explicitly remain
loopback-only. The accepted
[`desktop-inspection-surface`](desktop-inspection-surface.md) direction now
allows desktop/web and Android presentation to diverge: desktop/web becomes a
dense JSTorrent-derived inspection surface, while Android remains a separate
platform-appropriate product without automatic tab or diagnostic parity. The
fresh React, CSS Modules, adaptive navigation, touch, and accessibility
direction for that shared presentation lives in
[`web-ui-design.md`](web-ui-design.md). Tactical `033` now supplies its bounded
generated TypeScript/schema boundary, authenticated polling client, pure
reducer, and lifecycle controller. Tactical `034` adds the new responsive
React inspection application behind an explicit demo route without replacing
the live browser/Tauri entry or changing Android. Tactical `035` connects that
application to live torrent and active-peer views, including semantic
responsive view selection and recovery after a suspended tab outlives its
Rust view-set lease. The live path is proven through the headless browser
gateway; Android remains intentionally unchanged.
Tactical `036` packages that same production-built live browser path behind
`./scripts/webui`, with isolated persistent state, default online networking,
normal-browser opening, and joined terminal-owned shutdown. Tactical `048`
selects the React inspection application for Tauri and supplies it with the
same leased view-set semantics over acknowledged in-process Channel delivery;
the desktop opens no loopback server. Tactical `037` adds the first live
mutation to the new surface: bounded magnet intake through the semantic
command boundary. Tactical `038`
adds five curated public test-torrent shortcuts without adding a debug backend
command or bypassing application policy. Android remains intentionally
unchanged. Tactical `049` replaces the React product's generic Logs table with
the structured ordered console through both browser polling and Tauri Channel
delivery. Shared generated Rust/TypeScript/Kotlin semantic artifacts continue
to compile, but Android presentation intentionally does not mirror this dense
desktop diagnostic surface.

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

- Tauri uses native commands for request/response and an ordered Channel with
  explicit post-application acknowledgement for low-latency view updates.
- The browser ordinarily uses one authenticated multiplexed WebSocket for all
  semantic calls and acknowledged view updates. Bounded HTTP long polling is
  an explicitly selected loopback diagnostic comparison only; it is not a
  visible preference, automatic fallback or concurrent second lane.

Transport reuse is not an end in itself. A local desktop product does not open
a listener, allocate a port, serialize through a socket, or acquire network
authentication machinery merely to resemble the remote path. Both transports
adapt to the same semantic dispatcher and reactive-view contract.

The graduation direction adds the existing JSTorrent extension as a possible
first-class presentation for a native desktop, Android, or Crostini backend.
Desktop extension and Tauri presentations should attach to one desktop
profile; ChromeOS Android and Crostini remain separate backend and data
authorities. That future product topology, handoff UX, and manual migration
posture are recorded in
[`product-surfaces-and-migration.md`](product-surfaces-and-migration.md). It
does not make a production extension transport part of the currently
implemented client surface.

Android uses UniFFI-generated Kotlin records, enums, objects, and suspend
functions. A small Kotlin adapter exposes subscription handles as lifecycle-
aware `Flow`s and maps portable application values into presentation models.
Peer payload, piece payload, file payload, and hashing do not cross UniFFI.

Shared application semantics do not require identical presentation. Existing
cross-surface contracts remain valid where already implemented, but new
desktop inspection views do not imply Android UI work unless a later product
decision says otherwise.

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
Tauri adapter     Remote adapter
```

Platform-specific capabilities such as folder selection, tray state, external
URL opening, or updater presentation use a separate platform-capability
adapter. They do not enter torrent commands or view patches.

The shared presentation is a strict-TypeScript React application using
component-scoped CSS Modules and a permanent named-demo adapter.
It preserves JSTorrent's information hierarchy without inheriting its source
architecture and adapts one library/list/detail model from wide desktop to
phone-sized browser layouts. Stable Rust torrent and peer views are now mapped
through the live application adapter. Tactical `060` deleted the older
direct-DOM gateway surface, made the named demo the no-mode root, and moved
ordinary live-browser calls and view sets onto one multiplexed
WebSocket. HTTP remains available only as an explicit loopback diagnostic
query, while Tauri stays in process. The detailed direction and open choices
live in
[`web-ui-design.md`](web-ui-design.md).

The initial browser surface may prove the network transport on loopback with
explicit test credentials. That is not a claim of safe Internet exposure.
LAN binding, TLS, pairing, accounts, relay operation, wake-up delivery, remote
authorization policy, and deployment require later threat models and
tacticals.

Tactical `035` also adds an explicit unauthenticated development mode for
local UI bring-up and headless evidence. It binds only an OS-assigned loopback
listener, requires one exact configured loopback Origin, retains resource
checks, and isolates opaque view-set owners. It is a development convenience,
not a production browser-control posture; the authenticated mode remains.

The manual launcher serves the production bundle rather than the Vite
development entry and stores its profile beneath ignored `.local/webui` by
default. It is a maintainer-facing local bridge, not a change to the accepted
in-process Tauri product architecture. Tactical `048` makes that same React
application the Tauri product entry through an in-process adapter. The React
application currently supports inspection, magnet add, pause, and resume, but
not `.torrent` file intake or the full application command set. React emits a
transport-neutral magnet intent; only the live adapter constructs the
generated application request.
Typed and curated magnet intake share that path. A deterministic catalog test
keeps the UI shortcuts identical to `tests/live/torrents.json`; public swarm
availability remains variable evidence rather than a UI guarantee.

## Reactive Views

Presentation consumes named, versioned application views rather than arbitrary
field queries or engine structs. The accepted successor contract in
[`application-view-api.md`](application-view-api.md) groups one client's
currently relevant projections into a leased view set with one epoch, cursor,
and bounded update accumulator. Each named view selects:

- an application object or collection;
- a named projection such as summary or piece activity; and
- a bounded delivery policy.

Every view begins with a coherent snapshot. Typed update batches carry a
view-set epoch, base cursor, resulting cursor, durable revision, and per-view
snapshot, patch, or reset values. Cursor gaps, incompatible epochs, invalid
patches, expiry, or bounded-queue overflow require resynchronization from a
snapshot; events are never the only recovery authority.

Engine edges feed independent per-view accumulators within each view set.
Delivery may coalesce without losing the final state:

- torrent summaries use keyed latest-value upserts and explicit removal;
- verified piece changes union ranges while recheck can explicitly clear
  ranges;
- active block state uses the latest bounded mask for each active piece;
- counters and rates sample or conflate instead of enqueueing every byte; and
- a slow subscriber cannot consume, clear, or delay another subscriber's
  updates.

Durable application revision and volatile view-set cursor are separate.
High-frequency block activity neither writes SQLite nor advances the durable
command revision.

## Portable Contract Types

Rust application-contract records and enums are the canonical semantic source.
The contract uses a deliberately portable subset with explicit representations
for identifiers, byte strings, large counters, optional values, and tagged
variants.

- serde defines the diagnostic and initial network representation;
- generated TypeScript exposes discriminated unions and client inputs;
- generated JSON Schema validates the structural network representation while
  handwritten checks retain semantic and resource bounds;
- UniFFI generates Kotlin values for the in-process Android boundary; and
- handwritten semantic validators reject invalid bounds and cross-field
  relationships at runtime.

JavaScript cannot represent every Rust `u64` exactly. Portable revisions,
sequences, and unbounded counters use an explicit decimal representation or a
proved safe bound rather than silently rounding JSON numbers.

Generated artifacts are deterministic and checked for drift. Rust, TypeScript,
and Kotlin reducer fixtures must converge on the same state.

The web application materializes validated view batches through a pure reducer
into one per-application Zustand vanilla store. A separate `ViewController`
owns the view-set identifier, cursor, polling or streaming task, retries,
cancellation, and connection status. React components use narrow selectors;
transport tasks and handles do not live in the store.

## Lifecycle

View clients are detachable. Closing a browser tab, Tauri window, or Android
activity closes its view set or subscriptions without stopping the application
service or active download.

A browser tab may be suspended without running close handlers or polling
timers. The application-owned five-minute lease therefore destroys a silent
view set independently from later client traffic. On resume, the web
controller marks retained values stale, opens one replacement set from its
latest desired views, and atomically installs fresh snapshots before applying
new patches. Visibility and online events may accelerate this path but do not
own correctness.

- Tauri owns the application service independently of its webview window and
  may remain alive in the tray.
- Android's foreground service owns the application service independently of
  activities.
- A remote WebSocket connection owns only its authenticated commands and
  subscriptions.

Every subscription and adapter task has explicit close, cancellation,
termination observation, and bounded queued state.

## Headless Presentation Validation

The authenticated loopback browser gateway is the preferred automated seam
for shared web presentation. Repository harnesses run the real application
service and shared browser UI with temporary profile, storage, and browser
directories, then drive a headless Chrome process. This proves the same web
components embedded by Tauri without launching or focusing a desktop window.
Tauri compiles without launching a window and its pull/stream transport
adapters are directly tested; the gateway does not become the local desktop
product transport.

Tactical `034` adds a still smaller deterministic seam: named demo scenarios
implement the same frontend application port without starting Rust, a gateway,
or torrent networking. Headless Chrome now retains wide, compact, phone,
accessibility, keyboard, command, and large-collection evidence against that
adapter. Real adapter tests remain necessary when Rust projections connect;
demo evidence does not claim engine behavior.

Tactical `035` supplies that real-adapter evidence. The production-built web
surface connects to a temporary loopback application and controlled
libtorrent seed, observes active requests through verified completion, and
removes the peer row after cleanup. A deliberately silent browser client
outlives a shortened test lease, displays retained state as stale, then opens
a distinct view set and installs a fresh coherent snapshot. Wide, compact,
phone, accessibility, payload-hash, process-join, and temporary-cleanup checks
run without a visible browser or Tauri window.

Android presentation requires separate evidence because it is implemented in
Compose. Routine automation targets an explicitly owned no-window AVD and
uses generated-contract, reducer, Compose, and UIAutomator checks. Physical
devices, visible emulators, and host GUI automation require explicit user
authorization.

The browser gateway's listener policy and the application service's torrent
egress policy are independent. Routine browser evidence binds the gateway and
limits engine egress to loopback. Product desktop control remains in-process
even though its engine permits routed peers and trackers. Android likewise
runs the online engine inside its foreground service rather than through a
socket proxy.

## Diagnostics Surfaces

Tactical `012` gave the original shared web/Tauri surface and Android Compose
the same semantic progress dispositions, reason codes, diagnostic categories,
severity filters, default profiles, dropped-event indication, and bounded copy
behavior. Tactical `049` now gives desktop/web a richer global ordered console
with structured expansion, producer capture controls, and explicit source,
delivery, and local loss. Android retains its smaller product-appropriate
summary and shares compatible generated record types; detailed presentation
parity is deliberately not required.

Progress assessment remains a deliberate product view. Diagnostic records are
a separate bounded timeline and are never scraped to decide torrent state or
actions.

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
- The Tauri shell has basic macOS close-and-reopen behavior but no production
  tray or cross-platform window policy, installers, updates, file associations,
  or platform-capability adapter.
- Android durable SAF session storage and provider publication are proven for
  one persisted root. General root management, root migration, removable
  media policy, and file-selection presentation remain product gaps.
- The current UI proves one controlled torrent. General multi-torrent
  scheduling and complete product presentation remain absent. Peers, Files,
  Trackers, Pieces, and global Disk are live; Swarm, Logs, Speed, and DHT
  remain truthful scaffolds or later feeds.
- Diagnostics currently cover application lifecycle, discovery exhaustion,
  network restriction, tracker policy rejection, metadata, storage, piece,
  and integrity edges.
  Deeper typed peer negotiation, tracker-attempt, scheduler, and performance
  instrumentation remains to be added as those runtime owners grow.
- Android has no connectivity, metered-network, or VPN-only settings yet.
  Those controls should extend application network prerequisites while
  preserving torrent intent; VPN-grade leak prevention requires explicit
  Android network binding and race analysis.
- No HTTP playback server exists.

Tacticals `008` and `009` record the implemented contract, exact controlled
evidence, and bounded deferrals. Tactical `012` records the completed
cross-surface observability slice motivated by the first public-magnet desktop
run. Tactical `013` records explicit product and harness network selection.
Tacticals `033` through `035` record the leased view-set boundary, responsive
demo surface, and first live peer projection plus recovery evidence.
Production remote authorization, dynamic Android network controls, general
multi-torrent scheduling, and broader desktop lifecycle work remain later
boundaries.
