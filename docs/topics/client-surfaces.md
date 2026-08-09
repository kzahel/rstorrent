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
Tactical `084` splits the shared React Settings dialog into focused
appearance, download, and connection/seeding sections. Browser-hosted and
Tauri clients now share one atomic persisted loopback-listener/global-peer/
upload-slot form and authoritative active/effective/bind status; Android
receives the generated contract but no Compose settings screen.
Tactical [`111`](../tactical/111-mse-peer-stream-encryption.md)'s implemented
slice adds the generated four-value MSE/PE policy and live effective state to
that same contract. Browser and Tauri share the labelled non-security control
and the truthful encrypted-or-obfuscated peer flag; Android carries and
compiles the enum/default, exposes bounded DH-owner evidence to the retained
product harness, and deliberately adds no Compose settings screen. The named
API 37 physical Pixel 7a profile applied `required`, completed five forced-RC4
oracle sessions, published the exact payload, drained the DH owner, and cleaned
up within the recorded descriptor and storage bounds.
Completed follow-up Tactical
[`115`](../tactical/115-mse-policy-advertisement-and-peer-detail.md) carries
the optional exact MSE method through the shared generated contract. The
browser/Tauri peer table uses it only in the existing `E` cell's tooltip and
accessible name. Android receives the additive binding but still gains no
Compose settings or peer-detail work.
Completed Tactical
[`112`](../tactical/112-dual-stack-transport-and-ipv6-dht.md) adds one
default-enabled IPv6 control and configured/effective/application state to the
same generated settings contract. Browser and Tauri share the control and
typed degradation presentation. Android compiles and persists the value and
its retained product harness proves policy/restart behavior on the API 34 AVD
and named API 37 Pixel 7a, but deliberately adds no Compose settings screen.
The DHT inspection surface renders both
families within its existing bounded view rather than adding a second page or
client-owned inference.
Completed Tactical
[`098`](../tactical/098-authenticated-https-tracker-platform-trust.md) adds a
default-secure tracker HTTPS field to that same contract without rendering an
ordinary React or Compose control. The React draft, equality, refresh, and
save paths preserve the hidden authoritative value. Advanced consumers can
use the generated typed command; tracker rows render authenticated versus
explicitly unauthenticated encryption without a client-side inference.
Tactical `036` introduced the production-built browser path behind
`./scripts/webui`. Tactical `109` now gives it one fixed same-origin gateway:
the root URL remains stable across restarts, the gateway serves both the UI
and application transport, and the terminal owns one joined child process.
It retains isolated persistent state, default online networking, and normal
browser opening. Tactical `076`
adds an explicit same-origin production build for one Basic-authenticated
private maintainer host, including phone-sized browser access, without making
remote hosting a general product capability. Tactical `048`
selects the React inspection application for Tauri and supplies it with the
same leased view-set semantics over acknowledged in-process Channel delivery;
the desktop opens no loopback server. Tactical `037` adds the first live
mutation to the new surface: bounded magnet intake through the semantic
command boundary. Tactical `038`
adds five curated public test-torrent shortcuts without adding a debug backend
command or bypassing application policy. Android remains intentionally
unchanged. Tactical `071` adds a local selection-aware canonical magnet copy
action. Tactical `107` replaces its presentation-only hash construction with
a typed read-only application export used by browser and Tauri while keeping
Android presentation unchanged. Tactical `049` replaces the React product's
generic Logs table with the structured ordered console through both browser
polling and Tauri Channel delivery. Shared generated Rust/TypeScript/Kotlin
semantic artifacts continue to compile, but Android presentation intentionally
does not mirror this dense desktop diagnostic surface.
Tactical `085` gives the shared browser/Tauri product one grouped action model
for toolbar, More, and actionable-row context menus, including coordinated
multi-torrent removal and multi-file priority changes. Android receives and
compiles the projected recheck-capability field but intentionally gains no
desktop context-menu presentation.

Tactical `081` adds adapter-level v1 `.torrent` byte intake through
the ordinary browser WebSocket and raw in-process Tauri IPC, plus HTTP
automation. The raw
attachment maximum is 64 MiB while text/JSON remains 64 KiB; file and tracker
inspection move to bounded pages so larger accepted catalogs do not inflate
one view snapshot. Tactical `083` adds the shared presentation: empty Add
opens one advisory single-file chooser in browser and Tauri, reuses the
existing root/start dialog, retains only the `File` while options are pending,
and submits one bounded `ArrayBuffer` through the active adapter. Nonempty Add
remains the magnet path; there is no client digest, upload percentage, path,
or alternate native picker command.

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

The ordinary browser surface proves its network transport on loopback with
explicit test credentials. Tactical `076` separately permits one
maintainer-operated private host behind TLS and Basic authentication, with the
gateway enforcing the same credential and exact HTTPS Origin. That bounded
deployment is not a claim of safe general Internet exposure. Pairing,
accounts, relay operation, wake-up delivery, device authorization, stable
compatibility, and product remote-access policy require later threat models
and tacticals.

Tactical `035` also adds an explicit unauthenticated development mode for
local UI bring-up and headless evidence. It binds only loopback, requires one
exact configured loopback Origin, retains resource checks, and isolates opaque
view-set owners. Harnesses may use an OS-assigned port; Tactical `109` permits
the local hosted launcher to use its explicit stable loopback port. It is a
development convenience, not a production browser-control posture; the
authenticated mode remains.

The manual launcher uses one fixed-origin gateway to serve the production
bundle and the application API, rather than running a Vite preview beside an
ephemeral gateway. The browser derives application authority only from its
page origin, so the visible root URL survives launcher restarts. The launcher
stores its profile beneath ignored `.local/webui` by default. It is a
maintainer-facing local bridge, not a change to the accepted in-process Tauri
product architecture. Tactical `048` makes that same React
application the Tauri product entry through an in-process adapter. The React
application currently supports inspection, magnet and local `.torrent` add,
pause/resume, force recheck, archive/restore, removal, and Normal/Skip file
selection, but not the full application command set. Its HTTP, WebSocket, and
Tauri clients implement the generated transport-neutral byte-intake operation.
React emits transport-neutral magnet or byte intent; only the live adapter
constructs the generated application request, assigns selection `all`, and
chooses the active transport.
Typed and curated magnet intake share that path. A deterministic catalog test
keeps the UI shortcuts identical to `tests/live/torrents.json`; public swarm
availability remains variable evidence rather than a UI guarantee.
Torrent More and context menus copy one v1 magnet per selected torrent and
join them with newlines in stable application order. The application returns
a verified retained magnet verbatim when available; `.torrent`, missing, or
integrity-failed sources synthesize `xt`, verified `dn`, and the normalized
tracker catalog within current magnet bounds. The UI performs one clipboard
write only after every export succeeds and reports bounded tracker omissions.
Routine torrent rows and views still contain no source URI or tracker secrets.

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

The same detachment rule applies when the application service was explicitly
opened with ephemeral state. The service retains its private session and
speed-history databases until its application owner performs joined shutdown;
closing the last tab, window, activity, connection, subscription, or view set
does not clear them. No visible client mode selector is implemented by
Tactical `075`.

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

Tactical `083` adds a focused production-build browser proof without a
visible window. Headless Chrome receives a real `filechooser` event from empty
Add, selects an independently generated 157-byte v1 source, confirms
metadata-only intake, and observes one application WebSocket, one binary
attachment, zero semantic HTTP requests, the exact visible torrent row, and
zero serious/critical axe findings. Gateway metrics record one upload
declaration/admission and zero retained connections after joined shutdown;
the metadata-only run creates no payload artifacts. Tauri reuses these React
components and its tested raw-IPC adapter rather than gaining a path or native
source-file owner.

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

Tactical `098` packages the Cargo-version-matched platform-verifier AAR and
adds one application bootstrap before either Android service can construct a
native network owner. JVM ordering tests, both established ABI builds, APK
inspection, and an API 34 arm64 no-window AVD pass. The AVD rejected a
controlled invalid certificate before HTTP, accepted a public trusted origin
through HTTP, and completed the explicit disabled-policy SAF transfer against
pinned libtorrent. No Compose setting or visible product launch was added.

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
  Trackers, Pieces, global Disk, Swarm, Logs, Speed, and dual-family DHT are
  live; broader product workflows and content presentation remain incomplete.
- Diagnostics currently cover application lifecycle, discovery exhaustion,
  network restriction, tracker policy rejection, metadata, storage, piece,
  integrity, and terminal MSE handshake edges, including role, captured policy,
  exact negotiated method or typed failure, fallback use, byte accounting, and
  exponentiation count. Deeper tracker-attempt, scheduler, and performance
  instrumentation remains to be added as those runtime owners grow.
- Android has no connectivity, metered-network, or VPN-only settings yet.
  Those controls should extend application network prerequisites while
  preserving torrent intent; VPN-grade leak prevention requires explicit
  Android network binding and race analysis.
- Android has no Compose connection/seeding settings screen. Tactical `084`
  deliberately stops at generated contract and build/test coverage there;
  Tacticals `111`, `112`, and `115` follow the same boundary for MSE/PE policy,
  IPv6 policy, and exact peer detail. Mobile restart UX and connectivity
  policy require their own product slice.
- Tracker HTTPS authentication is intentionally absent from ordinary React
  and Compose settings. The advanced typed `disabled` override exists for
  compatibility/debug use and remains visibly unauthenticated in tracker
  rows; custom roots, pins, and certificate-management UI are absent.
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
