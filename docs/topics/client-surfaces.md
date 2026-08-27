# Client Surfaces

Topic: `client-surfaces`

Status: Accepted and implemented across a browser-hosted web view, a Tauri
desktop webview, Android Compose, and the maintained iOS SwiftUI client. The
same mature browser presentation is also packaged by completed Tactical `170`
for one configured Linux headless service. Completed Tactical `174` attaches
that one application owner to an exact LAN gateway and an exact loopback
gateway behind Tailscale Serve without adding a second backend process.
Completed
Tactical
[`143`](../tactical/143-dual-identity-and-persistence-foundation.md) makes
React, Tauri, media, Compose, and Android service routing use canonical opaque
torrent IDs while explicit v1 hashes remain available for display and export.
Completed Tactical
[`151`](../tactical/151-complete-source-pure-v2-runtime-vertical.md) admits
strict complete-source pure-v2 `.torrent` files through the same Add,
selection, progress, Files, Force recheck, export, restart, and seeding
surfaces. Clients gain no format-specific workflow or storage policy.
Completed Tactical
[`155`](../tactical/155-v2-magnet-authenticated-hash-exchange.md) sends
pure-v2 `btmh` magnets through that same Add, BEP 53 selection, progress,
Files, export, restart, promotion, removal, and seeding surface. Protocol and
Merkle policy remain in the in-process engine; clients gain no hash-specific
controls. Completed Tactical
[`156`](../tactical/156-hybrid-dual-swarm-runtime-closure.md) sends complete or
single-/dual-topic hybrid input through that same one-row workflow. Both exact
identities remain visible/exportable while selection, promotion, progress,
Files, restart, recheck, seeding, and removal remain format-neutral. The
production browser and Android API 34 SAF profiles pass; Tauri builds and the
unsigned iOS archive require no presentation fork or hybrid control. The
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
receives the generated contract; Tactical `117` now presents these backed
settings in Compose.
Tactical [`111`](../tactical/111-mse-peer-stream-encryption.md)'s implemented
slice adds the generated four-value MSE/PE policy and live effective state to
that same contract. Browser and Tauri share the labelled non-security control
and the truthful encrypted-or-obfuscated peer flag; Android carries and
compiles the enum/default, exposes bounded DH-owner evidence to the retained
product harness. Tactical `117` now presents the backed encryption setting and
exact peer detail. The named
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
and named API 37 Pixel 7a. Tactical `117` now presents the backed IPv6 setting
and renders both DHT families within one bounded Compose view rather than
adding a second page or client-owned inference.
Completed Tactical
[`114`](../tactical/114-session-wide-concurrent-torrent-admission.md) adds
authoritative operational state and queue position to shared torrent rows,
configured/effective active-download limits and clamp state to Settings, and
Move to top/Move to bottom actions to Transfers. Headless Chrome proves those
settings and actions through the generated contract. Android consumes the
same additive contract and a physical Pixel 7a proves configured three/
effective two plus promotion. Tactical `117` now exposes the same queue and
active-download controls in Compose.
Maintainer direction on 2026-08-09 accepted iOS as a first-party in-process
product. Completed Tactical
[`116`](../tactical/116-platform-storage-coherence-and-ios-feasibility.md)
adds the historical physical-device feasibility harness around the real Rust
storage and direct-networking seams. Explicit direction on 2026-08-13 then
completed Tacticals `147`--`149`, adding the fourth supported surface with
generated Swift bindings, qualified external folders, direct first-party
JSTorrent SwiftUI reuse, and physical lifecycle evidence.
Completed Tactical
[`117`](../tactical/117-jstorrent-shaped-android-product-ui.md) supplies the
first UI-complete Android product for the current application boundary. Its
single-stack Material 3 Library, six-tab torrent detail, Speed, dual-family
DHT, structured Logs, and Settings hierarchy deliberately follow JSTorrent
Android standalone with RSTorrent branding and honest capability gaps.
Completed Tactical
[`172`](../tactical/172-provisional-magnet-display-name.md) gives the shared
web, Android, iOS, and desktop-notification presentation boundary one
consistent torrent-name order: verified metainfo name, bounded provisional
magnet `dn`, then the surface's existing opaque fallback. Android file-open
and all other path-authority behavior continue to require the verified name;
the full retained magnet never crosses the application view boundary.
Active Tactical
[`113`](../tactical/113-ipv6-firewall-pinhole-and-incoming-reachability.md)
adds no setting. Browser and Tauri render the existing IPv4 mapping status and
the sibling IPv6 firewall-pinhole status independently, including service
absence, gateway-reported unfiltered state, finite lease, renewal, and cleanup
failure. Product wording says that an accepted pinhole is gateway evidence,
not an observed incoming peer. Android receives the additive generated binding
and passes both native ABI builds, UniFFI/Kotlin regeneration, `assembleDebug`,
and `testDebugUnitTest`, but deliberately gains no Compose surface or physical
pinhole gate.

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
Completed Tactical
[`176`](../tactical/176-durable-high-file-priority.md) carries
High/Normal/Skip through the shared generated contract. React/Tauri exposes
the three values through its existing grouped file actions, Android Compose
replaces the binary checkbox with a three-value menu, and iOS SwiftUI displays
and sets the same values. This presentation does not add a second scheduler or
progressive Android/iOS playback.

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

Completed Tactical
[`134`](../tactical/134-hierarchical-transfer-rate-enforcement.md) makes All
torrents upload/download limits and per-torrent upload/download limits
operable in both first-party product families. Browser/Tauri uses Connection
& seeding settings plus General torrent detail; Compose uses Speed &
Connection Limits plus torrent detail. Both present semantic Unlimited and
KiB/s finite values without inferring effective session state. Headless Chrome
proves wide, compact, phone, keyboard, retained-value, save, and accessibility
behavior; the API 34 no-window AVD proves durable limited concurrent transfer
through the Android product boundary.

Completed Tactical
[`138`](../tactical/138-verified-http-file-serving.md) adds one `Open` action
to eligible rows in the shared React Files table. Browser hosting reserves an
opener-isolated tab before the asynchronous capability call; Tauri validates
the returned URL against its current exact-loopback media origin before using
the system opener. Failure closes the reserved tab and remains typed
presentation state. Android deliberately retains its existing complete-file
`content://` action and starts no HTTP listener. There is no embedded player,
copy-link action, or MIME-specific UI.
Completed Tactical
[`139`](../tactical/139-incomplete-file-streaming-demand.md) extends the same
React/Tauri `Open` action to typed `streamable` rows. Browser and Tauri retain
their existing opener, origin-validation, failure, and lifecycle behavior;
the progressive wait and scheduling stay behind the capability URL. Android
continues to present only completed-file native open and gains no listener or
streaming presentation.

## Scope

This topic owns the product-client surfaces above the shared application
service: the reusable web application, desktop shell, Android presentation
adapter, generated client types, and the boundary between presentation and
application control.

It does not own torrent protocol behavior, payload storage, the future remote
account or relay system, or the HTTP payload-serving implementation.

## Accepted Product Shape

RSTorrent has three initial user-interface surfaces:

- a browser-hosted web application capable of using an authenticated remote
  control transport;
- a Tauri desktop application embedding the same web application; and
- an Android Compose application adapted from the existing JSTorrent Android
  product.

The maintained iOS 16+ client is a first-party SwiftUI product using the same
in-process Rust engine and typed application semantics. Completed Tactical
`147` supplies generated Swift UniFFI, durable app-owned and qualified selected
roots, and a coordinated descriptor-release seam. Completed Tactical `148`
directly reuses the first-party JSTorrent iOS views/assets/localizations while
replacing its JavaScriptCore controller and omitting Search. Completed Tactical
`149` owns finite background, process-death recovery, cold/warm handoff, and
reproducible archives without publication. Completed Tactical `152` narrows
long-lived selected-root coordination to exact files and physically proves
controlled multifile publication plus the exact Big Buck Bunny public torrent
through Apple system playback. Completed Tactical `154` reserves 100% and
Finished for Complete/Published state and changes **Open using** from a generic
activity sheet to direct Quick Look. A physical real-swarm rerun proves one-tap
native video presentation, advancing playback, lease-backed dismissal, and
exact cleanup. Physical iPhone evidence supports the
foreground and finite-background product claim; indefinite background,
iCloud/File Provider roots, migration, and public distribution remain absent.

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

Tactical
[`167`](../tactical/167-chromeos-crostini-bundled-web-launcher.md) makes one
bounded refinement for the first ChromeOS Linux product: its package serves
this same mature React application and same-origin gateway directly, while the
beta extension launches, focuses, and explains that backend. This does not
foreclose an extension-hosted presentation later and does not change the
separate Android or desktop profile owners. The exact local x86_64 package is
implemented and passes warm, twice-stopped-VM, repeated-singleton,
detachable-transfer, uninstall/reinstall, and purge evidence on the physical
Chromebook. Full reboot remains a conditional gap because the testbed exposes
no approved ChromeOS profile-login credential; signed/public packages,
updating, and broader lifecycle claims remain separate work.

Completed Tactical
[`168`](../tactical/168-platform-aware-extension-launcher.md) makes the small
beta extension launcher platform-relevant. Desktop Chrome sees only the native
desktop bootstrap. ChromeOS sees the exact published JSTorrent Android listing
and the ChromeOS Linux open/recovery controls, plus explicit separate-library
copy; it does not claim Play or app availability. The deterministic `0.3.0`
package and physical ChromeOS chooser, Play destination, and warm Crostini
handoff pass without new permissions.

Completed Tactical
[`169`](../tactical/169-hosted-crostini-bootstrap-and-release.md) adds the
release-ready website bootstrap and strict two-architecture signed-manifest
workflow around that same bundled backend/frontend package. Its physical
x86_64 local signed fixture repairs the real installation, preserves profile
bytes, launches the same UI, and rejects tampered or incompatible inputs before
installation mutation. No exact public package, website deployment, or ARM64
runtime claim followed from the bounded source plumbing itself. Subsequent
explicit release authorization published non-latest `crostini-v0.1.0`,
deployed the pinned website bootstrap, independently verified both public
archives and the signed manifest, and passed the exact public install,
Launcher, connected React surface, and stop/relaunch path on the physical
x86_64 Chromebook. ARM64 retains native hosted build/archive evidence without
a physical runtime claim.

Tactical
[`166`](../tactical/166-desktop-native-bootstrap-and-extension-scaffold.md)
implements only the bounded predecessor seam: a self-contained Manifest V3
JSTorrent Beta popup/service worker and the distinct
`com.jstorrent.rstorrent.native` compatibility/launch host. The host owns no
torrent, profile, listener, or application service. Desktop first launch
repairs exact per-user Chrome registration to a content-versioned stable host;
AppImage launch metadata points at the stable AppImage rather than its
temporary mount. Chrome Web Store item `gcgoepclopkgijmclmlheafaglmbjlcc` and
its public key are pinned, validation independently derives that identity, and
the host manifest allows only its exact beta origin beside the production
JSTorrent origin. The installed Chrome `hello`/launch smoke is complete:
Chrome 151 on an installed unsigned macOS arm64 app reports the exact beta ID,
completes native `hello` while the app is stopped, and cold-launches the
desktop product. This seam does not claim an extension control surface.

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

Tactical `117` makes that reuse direction concrete: a single-stack Material 3
Library leads to one torrent detail route with Details, Status, Files,
Trackers, Peers, and Pieces tabs, while Speed, dual-family DHT, structured
Logs, and Settings remain global routes. Preserve JSTorrent's hierarchy,
interaction rhythm, and density with RSTorrent branding. Replace QuickJS-
specific health presentation with Rust-native application metrics, and never
enable a setting whose durable/effective behavior is absent.

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

The Tauri bootstrap now injects a separate desktop updater controller while
browser/demo/live-gateway entry points omit it. Only that capability adds
About & updates and the accessible availability notice; update state does not
enter the generated application contract or torrent presentation owner.

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

Completed Tactical
[`170`](../tactical/170-configured-linux-headless-service.md) turns that shared
browser surface into a first-party ordinary-user Linux service package. Its
local-session and private-host Basic modes retain same-origin HTTP/WebSocket
semantics, while the service owns the application/profile independently from
every attached tab. It adds no extension transport or owner E2E remote claim.

Completed Tactical
[`174`](../tactical/174-exact-tailnet-headless-access.md) permits the same
installed service to expose multiple explicit endpoint adapters around that
one application owner. The current machine retains its exact LAN HTTP/WS
authority and adds one exact loopback backend behind a tailnet-only Tailscale
Serve HTTPS/WSS authority. Each gateway enforces its own exact Host and
Origin, and media capability URLs use the origin through which they were
created. The shared React client reports the credential-free network posture,
shows its full-owner explanation once per browser origin, and retains
`No auth` after dismissal. Tailnet ACL admission remains an operator boundary,
not RSTorrent owner E2E authentication.

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
  may remain alive in the tray. Completed Tactical `162` registers the
  single-instance owner before service startup, defaults close to hidden
  background operation, persists that shell policy independently from the
  profile, restores the existing window from the tray or a second launch, and
  joins service/media shutdown before close, tray Quit, or updater restart can
  terminate the process.
- Completed Tactical
  [`164`](../tactical/164-desktop-completion-and-attention-notifications.md)
  adds a Tauri-only native Rust owner for edge-triggered completion and
  fatal/repair notifications. It consumes the existing authoritative
  in-process torrent-list view independently from React, does not replay
  initial/reset terminal state, and exposes only typed shell preferences to a
  capability-gated desktop Settings category. The standard Tauri Rust backend
  owns macOS/Windows; Linux retains the underlying native handle directly for
  GNOME display and bounded click activation. Android and iOS keep their own
  platform notification owners.
- Completed Tactical
  [`165`](../tactical/165-cross-platform-active-download-sleep-inhibition.md)
  adds a separate Tauri-only default-on Power preference and a native Rust
  level owner over authoritative torrent-list state. macOS/Windows use exact
  `keepawake` 0.6.1, GNOME uses its suspend inhibitor, and other Linux sessions
  use a bounded XDG portal fallback. `Starting`, `Downloading`, and `Checking`
  hold one system-idle/suspend request; pause, queue, completion, seeding,
  failure, disablement, and joined shutdown release it without suppressing
  display blanking.
- Android's foreground service owns the application service independently of
  activities. Tactical `165` also gives that service one default-on native
  preference and exactly one partial CPU wake lock for the same active state
  set while removing its Wi-Fi lock. iOS retains finite background work and
  exposes no misleading general keep-awake control.
- The Linux headless process owns one application service, profile, one or
  more explicitly configured gateways, and engine independently from browser
  connections. Its systemd user unit forwards stop to the adapter, whose
  cancellation path joins every gateway and the application owner before
  exiting; no synthetic view is retained while idle. Tactical
  [`171`](../tactical/171-signed-headless-release-and-lan-service.md) adds one
  serialized backend signed-release checker. Browser startup/daily checks are
  quiet on current or network failure, manual results are visible, and apply
  remains an explicit installed shell command; the browser never owns package
  replacement or service restart.
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

Tactical `170` adds installed-service evidence beyond the temporary browser
harness. On x86_64 Ubuntu, every presentation detached during an 8-MiB
controlled transfer, a fresh presentation recovered complete durable state,
RSTorrent then seeded the exact payload back to pinned libtorrent, and the
service remained healthy while idle with zero gateway sessions. Joined
restart, rollback-safe repair, preservation-safe uninstall, and exact cleanup
also pass through the same production React/WebSocket surface.

Tactical `174` adds an installed multi-endpoint presentation proof. One
systemd user PID owns the exact LAN and loopback-proxy sockets around one
application/media owner. Both the LAN HTTP/WS authority and the Tailscale
Serve HTTPS/WSS authority pass health, API, application WebSocket, and real
media-capability creation; the returned capability uses the serving
authority's exact origin. A 456-by-1024 tailnet browser run loads without page
errors, opens WSS, dismisses the network notice, and retains dismissal after
reload. Wrong Host and cross-origin requests return `403`. A physical off-LAN
phone retry is not yet claimed.

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

Tactical `151` extends that authenticated production-browser seam from paused
intake through an exact pure-v2 lifecycle. One v1 control remains paused while
the v2 row selects a skipped file, completes wanted bytes over uTP, exposes
verified file behavior, force-rechecks, restarts without another source
upload, and retains selection and exact output. The generations observe one
binary source attachment where expected, no semantic HTTP control calls, no
serious/critical accessibility findings, no part artifact, and joined gateway
cleanup. Tauri's direct adapters pass without a visible launch. The Android
Compose application independently passes complete and selective pure-v2 SAF
profiles on an owned API 34 no-window AVD, including restart/recheck,
publication, exact upload, bounded handles, and cleanup.

Tactical `155` adds the production magnet lifecycle to that seam. The browser
uses exact `btmh`, one peer hint, and select-only intent; it completes wanted
files, leaves the skipped file absent, exports the canonical magnet, restarts
without peer traffic, and removes exact managed data with no semantic HTTP
control or accessibility regression. The Android AVD independently interrupts
an incomplete verified candidate, refetches hashes after restart, completes
and promotes selection, serves a magnet-only libtorrent leecher, and cleans
SAF state within the existing 40-handle and 16-request bounds. Tauri and iOS
require no new presentation contract; their adapter/build gates pass.

Tactical `156` adds the hybrid lifecycle to the same production seams. The
browser reconciles separate exact `btih` and `btmh` additions into one row,
retains both identities, applies exact selection, restarts complete, serves
hash and payload requests, and removes exact managed state with bounded
gateway ownership and zero Axe violations. The Android AVD runs a six-file
SAF hybrid, promotes selection, serves both direct-v2 and upgraded routes,
synthesizes padding, restarts/rechecks without peer payload, and cleans within
the existing handle/request bounds. Desktop and iOS require no new contract.

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

## HTTP Playback Data Plane

Desktop/browser hosting now exposes bounded ephemeral capability URLs for
published and eligible active torrent files. That server is a content data
plane, not the command and reactive-view transport. Android deliberately has
no HTTP listener or incomplete-file presentation.

Tacticals `138` and `139` implement exact interface binding, capabilities,
expiry/revocation, `HEAD` and single-range behavior, verified-only active
reads, bounded incomplete-file scheduling, body/backpressure limits, and
joined lifecycle ownership. File bytes flow between Rust storage and HTTP
clients without crossing the UI contract. Embedded playback, stable sharing,
remote exposure, and Android streaming remain independent product decisions.

## Current Gaps

- The loopback WebSocket gateway and configured private-host Basic service are
  not a production owner-remote-access design. RSTorrent has no built-in TLS,
  passphrase/device principal model, relay, wake-up path, or public wire
  compatibility promise.
- The configured Linux headless surface now also supports exact RFC 1918
  `lan-none`, with a one-time per-origin notice that authentication is absent
  and every LAN client has full owner control. Dismissal survives reload while
  a compact `No auth` header status remains. Exact Host/Origin checks and a
  physical Android phone load pass on the installed current-host service, but
  this is neither encrypted nor authenticated remote access. Strict
  two-architecture signed release, bootstrap, and explicit check/apply source
  machinery exists; no public `headless-v*` candidate or stable manifest has
  been promoted, and native Raspberry Pi service/update evidence remains
  absent.
- The same service also supports one exact loopback endpoint behind an exact
  Tailscale Serve HTTPS authority. This keeps the application off wildcard and
  Tailscale interface binds, but it remains credential-free inside the
  admitted tailnet policy: every reachable identity is an owner. It is not a
  passphrase/device/relay remote-access claim, and physical off-LAN phone plus
  native Raspberry Pi evidence remain absent.
- The Tauri shell has basic macOS close-and-reopen behavior, provisional icon
  assets, ordinary local bundle configuration, implemented updater UI/
  lifecycle behavior, and hosted signed package evidence across macOS arm64/
  x86_64, Windows x86_64, and Linux arm64/x86_64. Public `desktop-v0.1.0` and
  `desktop-v0.1.1`, one installed macOS arm64 launch/updater-init smoke, and an
  exact macOS arm64 and Linux arm64 replacement/relaunch campaigns with
  updater-ID continuity pass. Windows x86_64 replacement/relaunch passes under
  an automatic-loopback profile, but fresh-default local-network listener
  selection prevents startup in those public builds. Completed Tacticals `160`
  and `161` repair that startup path and add the parented native picker on
  `main`; an unsigned installed Windows campaign passes fresh-profile cancel,
  selection, default persistence, unavailable-root repair, and controlled
  process restart. The next signed update, installed Linux x86_64 picker/update
  evidence, and Windows firewall-consent characterization remain open;
  installed Intel macOS testing is deliberately omitted. Completed Tactical
  `162` adds the selected production tray policy, one desktop instance,
  default-on persisted background operation, visible tray update checking,
  joined shutdown/restart, native Linux arm64 packaging, and installed Windows
  x86_64/Linux arm64 lifecycle/icon evidence. Completed Tactical
  [`163`](../tactical/163-desktop-external-torrent-intake.md) adds bounded
  installed `magnet:` and local `.torrent` association/handoff through that
  same owner and the existing Add flow. Deterministic Rust/React and package
  gates pass, as do installed Linux arm64, Windows x86_64-application, and
  macOS arm64 cold/visible/hidden/cancel/failure/duplicate/Quit campaigns. The
  Windows package was a real x86_64 PE exercised under Windows 11 arm64 x64
  emulation. The macOS campaign preserved JSTorrent's inherited default while
  targeting the registered incubation bundle through LaunchServices. The exact
  hosted eight-job run also passes. Completed Tactical `164` closes the
  bounded native desktop-notification gap with deterministic/package gates and
  installed macOS arm64, Windows x86_64, and Linux arm64 evidence. Completed
  Tactical `165` closes active-work sleep inhibition with installed macOS
  arm64, Windows arm64, Linux arm64, native Windows x86_64, physical Android
  API 37, and physical iOS evidence through guest/device-native
  machine-control routes. The native x86_64 repeat used the exact unsigned
  package from all-green workflow run `32884674167`; package trust and the
  integrated signed update remain with Tactical `158`. Broader cross-platform
  window/platform-capability evidence remains open. The release gates live in
  [`beta-release-readiness.md`](beta-release-readiness.md).
- The maintained Android product module now lives at `clients/android`; the
  unreleased `org.rstorrent.bootstrap` identity and retained diagnostic service
  remain explicit release-cleanup decisions rather than evidence that the
  Compose client is still an experiment.
- Android durable SAF session storage and provider publication are proven for
  one persisted root. The Compose product presents root setup/health/repair,
  bounded High/Normal/Skip file priority, `Download now`, and completed-file
  open. General
  root migration and cloud/removable-media policy remain product gaps.
- The Android Compose product now presents authoritative multi-torrent queue
  and concurrent-admission state plus live Peers, Files, Trackers, Pieces,
  Disk, Swarm, Logs, Speed, and dual-family DHT projections. Search/plugins,
  playback, dynamic network policy, and tracker mutation remain explicitly
  unavailable rather than discarded or simulated. Manual session and torrent
  peer-transfer limits are implemented. Completed Tactical `175` carries
  exact retained Swarm payload download/upload totals through the generated
  UniFFI/Kotlin record and dual-ABI build. Compose currently presents only the
  Swarm catalog summary, not peer rows, so it does not fabricate a parallel
  row presentation for those fields.
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
- Android now has Compose connection/seeding settings for backed peer, upload-
  slot, active-download, listener, port-mapping, IPv6, and encryption values,
  including configured/effective/application truth, plus backed session and
  per-torrent upload/download limits. VPN, metered-network, proxy, and power
  policy remain separate product/engine slices.
- Tracker HTTPS authentication is intentionally absent from ordinary React
  and Compose settings. The advanced typed `disabled` override exists for
  compatibility/debug use and remains visibly unauthenticated in tracker
  rows; custom roots, pins, and certificate-management UI are absent.
- No embedded playback UI, stable media sharing, remote media listener, or
  Android HTTP playback server exists.
- The maintained RSTorrent iOS product now supports the bounded scope recorded
  by completed Tacticals `147`--`149`, `152`, and `154`: qualified selected
  folders,
  exact-target single- and multifile storage, publication, restart/Force
  recheck, truthful Complete/Published progress, completed-file handoff, direct
  Apple Quick Look/video playback after completion, the adapted first-party
  JSTorrent presentation, cold/warm intake, finite
  lifecycle and force-close recovery, and local archive creation. Search,
  cloud/provider roots, migration, indefinite background work, embedded or
  progressive playback, and public distribution remain absent.

Tacticals `008` and `009` record the implemented contract, exact controlled
evidence, and bounded deferrals. Tactical `012` records the completed
cross-surface observability slice motivated by the first public-magnet desktop
run. Tactical `013` records explicit product and harness network selection.
Tacticals `033` through `035` record the leased view-set boundary, responsive
demo surface, and first live peer projection plus recovery evidence.
Completed Tactical `117` records the Android product-presentation closure.
Production remote authorization, dynamic Android network controls, and
broader desktop lifecycle work remain later capability boundaries. Signed
distribution, updating, CI, product identity, and beta MVP triage are tracked
in [`beta-release-readiness.md`](beta-release-readiness.md).
