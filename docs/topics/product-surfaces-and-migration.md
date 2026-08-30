# Product Surfaces And JSTorrent Migration

Topic: `product-surfaces-and-migration`

Status: The backend/presentation model, ChromeOS choices, and desktop extension
direction were accepted in product discussion on 2026-08-02. This topic
records graduation direction, not standing authorization for production
remote control, legacy migration, or extension-hosted control. Exact
unimplemented transports and security boundaries remain to be designed and
validated in bounded tacticals. Maintainer direction on
2026-08-09 accepts iOS as an eventual first-party in-process backend; Tactical
[`116`](../tactical/116-platform-storage-coherence-and-ios-feasibility.md)
front-loads the physical storage/network/lifecycle feasibility that shaped the
common engine boundary. Explicit direction on 2026-08-13 schedules the first
maintained SwiftUI product in Tacticals `147`--`149`; that campaign is complete
with qualified external folder selection and physical lifecycle evidence,
without migration or publication.
Maintainer direction on 2026-08-22 starts the independent beta distribution
campaign in [`beta-release-readiness.md`](beta-release-readiness.md). That
campaign does not implicitly authorize legacy JSTorrent takeover, extension
work, or semantic import. Direction on 2026-08-23 records only the broad later
goal: a proven RSTorrent implementation may graduate through JSTorrent's
existing desktop identity and updater channel, with best-effort state migration
scoped at that time. This supersedes the earlier detailed import posture and
does not make migration planning current work.

Explicit maintainer direction on 2026-08-26 authorizes only the bounded
desktop-bootstrap foundation in Tactical
[`166`](../tactical/166-desktop-native-bootstrap-and-extension-scaffold.md): a
distinct typed compatibility/launch host, per-user registration and sidecar
packaging, and a store-seed MV3 extension. That bounded foundation is complete
after exact-ID installed Chrome `hello` and cold-launch evidence. It does not
choose the later headless service, full extension control, or Crostini
topology.

Subsequent maintainer direction on 2026-08-26 selects the first Crostini
product slice in Tactical
[`167`](../tactical/167-chromeos-crostini-bundled-web-launcher.md). The
Crostini package serves the mature React application with its Rust backend;
the extension owns cold handoff, warm open/focus, setup, and later browser
integration rather than carrying the initial full UI bundle. A future
extension-hosted presentation remains possible but is not required for the
first ChromeOS Linux path. That bounded slice is complete after source gates
and the available physical Chromebook warm, twice-stopped-VM,
detachable-transfer, preservation, and purge matrix. Full reboot remains a
conditional gap because the testbed has no approved profile-login credential.
Completed platform-polish Tactical
[`168`](../tactical/168-platform-aware-extension-launcher.md) then makes the
extension choose platform-relevant launch surfaces. ChromeOS explicitly offers
the currently published JSTorrent Android app and the RSTorrent ChromeOS Linux
preview as separate data authorities; desktop systems retain only their native
bootstrap. The extension does not infer Play enablement or Android-app
installation.

Completed Tactical
[`169`](../tactical/169-hosted-crostini-bootstrap-and-release.md) adds the
bounded next distribution step: a website-hosted bootstrap, signed canonical
manifest, and native x86_64/ARM64 `crostini-v*` release workflow using the
existing RSTorrent beta trust root. Its physical x86_64 signed-fixture package
repair and fail-closed matrix pass. Subsequent explicit release authorization
published non-latest `crostini-v0.1.0`, deployed the pinned website bootstrap,
and passed independent public-asset verification plus the exact website
install, Launcher, and stop/relaunch path on the physical x86_64 Chromebook.
The native ARM64 package passes hosted build and archive gates without a
physical ARM64 runtime claim.

Maintainer direction on 2026-08-26 additionally accepts first-class headless
Linux as a product composition: one native service owns its profile and engine
without Tauri or a graphical session, while the backend-served React UI and
later authenticated remote clients attach as detachable presentations.
[`runtime-configurations-and-headless-deployment.md`](runtime-configurations-and-headless-deployment.md)
owns that runtime, service, listener, authentication, and reverse-proxy
direction. Configured Linux headless-service Tactical
[`170`](../tactical/170-configured-linux-headless-service.md) completes the
first implementation slice with one ordinary-user systemd service, detachable
React presentations, strict hosted access, and data-preserving package
lifecycle. Completed signed headless release and trusted-LAN Tactical
[`171`](../tactical/171-signed-headless-release-and-lan-service.md) adds strict
signed source update machinery, one exact no-credential private-LAN mode, and
an enabled healthy current-host x86_64 deployment. No public headless channel,
physical ARM64 service, owner remote cryptography, or relay is claimed.

Explicit maintainer direction on 2026-08-30 selects ChromeOS Android
extension control as migration-critical release-parity work in active Tactical
[`194`](../tactical/194-chromeos-android-extension-control.md). It preserves
the familiar JSTorrent launch, pairing, dense browser presentation, and
background-service journey while reversing the old ownership: Android retains
the only Rust application service, engine, profile, networking, hashing, and
SAF authority; the extension is a detachable typed presentation. The same
React root action used with Crostini invokes Android's SAF picker, and retained
grants keep earlier torrents on their original roots when a new root becomes
current. This is workflow continuity, not legacy-state import, a raw IO
daemon, a public remote API, or production-extension publication.

## Scope

This topic owns the product shape that separates a native engine host from the
UI used to control it, the resulting desktop and ChromeOS configurations,
the eventual iOS backend shape, launch and handoff requirements,
backend-visible state ownership, and the initial migration posture from
current JSTorrent installations.

It complements:

- [`../vision.md`](../vision.md), which owns the long-term JSTorrent succession
  thesis and graduation evidence;
- [`product-direction.md`](product-direction.md), which owns engine and
  platform constraints;
- [`client-surfaces.md`](client-surfaces.md), which owns the currently
  implemented browser, Tauri, and Android presentation adapters;
- [`application-control.md`](application-control.md) and
  [`application-view-api.md`](application-view-api.md), which own the semantic
  command and recoverable view contracts; and
- [`client-persistence.md`](client-persistence.md), which owns the typed SQLite
  authority and verified resume invariants; and
- [`runtime-configurations-and-headless-deployment.md`](runtime-configurations-and-headless-deployment.md),
  which owns visible, background, windowless, and headless runtime
  compositions plus their deployment configuration.

This topic does not select the final public name or domain, duplicate the
platform release sequence now owned by `beta-release-readiness`, expose the
native engine as a general-purpose third-party daemon API, or make product
migration an implicit beta requirement. The accepted first-party Linux
headless service remains a product host around the same application service,
not a peer, filesystem, or payload proxy.

## Product Model: Backend And Presentation Are Separate

The product should distinguish the process that owns torrent state from the
surface through which a user views and controls it.

A **backend** owns one application-service instance, its profile database,
torrent engine, peer and tracker networking, hashing, scheduling, storage
capabilities, and background lifecycle. A **presentation** attaches as a
detachable client using application commands, snapshots, diffs, and events.
Closing or replacing one presentation must not silently create a second engine
or transfer profile ownership.

The web presentation retains substantial JavaScript and TypeScript heritage.
That is intentional. Rust ownership of the engine hot path does not require
removing the browser technology that makes JSTorrent's detailed interface and
browser integration valuable.

The accepted product invariants are:

- peer sockets, tracker and DHT work, hashing, scheduling, SQLite, and piece
  payloads remain in the selected native backend;
- an extension control channel carries bounded semantic commands and
  recoverable views, never raw socket, filesystem, or piece operations;
- every attached UI identifies the exact backend instance and profile it is
  displaying;
- UI preferences are presentation state and do not decide profile ownership;
- more than one view may attach to one backend when the backend and transport
  explicitly support it; and
- different backends never imply a shared live database, storage capability,
  or verified-piece authority merely because the same extension can reach
  both.

## Product Configurations

The likely successor has these useful compositions:

| Platform | Backend | Presentation | State relationship |
| --- | --- | --- | --- |
| Desktop | Desktop Rust application service | Embedded Tauri webview | Owns the desktop profile |
| Desktop | Same desktop Rust application service | Browser extension | Shares the desktop profile |
| Linux server | Headless Rust application service | Backend-served React UI or authenticated remote client | Owns one explicit server profile and its configured roots |
| ChromeOS | Rust application service in Crostini | Backend-served React UI, launched through the browser extension | Owns a Linux profile and roots |
| ChromeOS | Rust application service in the Android app | Browser extension | Shares the Android profile |
| ChromeOS or Android | Same Android application service | Android Compose | Shares the Android backend and profile |
| iOS/iPadOS | Rust application service in the native app | First-party JSTorrent SwiftUI presentation adapted to typed RSTorrent models | Owns an iOS profile, app Documents, and qualified selected on-device roots |

ChromeOS therefore has **two backends and three presentation
configurations**:

1. Crostini plus extension;
2. Android plus extension; or
3. Android alone through Compose.

The second and third configurations are two views of the same Android backend.
The Crostini configuration is a different backend with different state and
storage.

The iOS application is another native backend, not a remote
presentation of desktop or Android. It runs the first-party Rust engine
in-process and owns its own profile, root capabilities, networking, and
lifecycle. An Apple bookmark or File Provider identity cannot be shared with
Android SAF or a desktop path, and matching filenames do not imply shared
verified state. Tactical `116` tests this shape on a physical device without
choosing the final UI toolkit, release channel, minimum OS, or migration path.
Tactical
[`123`](../tactical/123-ios-on-device-root-persistence-and-recovery.md)
records the former app-owned-only boundary and the evidence that iCloud is
ubiquitous while a separate local folder may return no public provider
identity. Completed Tactical `147` supersedes that product decision: iOS 16+
uses
app-owned Documents plus distinct stable qualified selected roots, rejects
iCloud and positively identified providers, and requires a physical capability
gate when lookup fails. Completed Tactical `148` directly reuses the first-
party JSTorrent SwiftUI presentation; completed Tactical `149` owns finite
background and recovery behavior. Completed Tacticals `152` and `154` add
qualified-root multifile correctness, publication-aware progress, and direct
Apple Quick Look/video presentation of complete files under the existing
scoped lease. None authorizes migration or public distribution.

## Desktop Extension And Embedded UI

The embedded desktop webview is not the only legitimate desktop preference.
Some users, including the maintainer, prefer the extension UI even when a
desktop application is installed. The successor should treat the extension as
a first-class desktop presentation rather than only a launcher, installation
probe, or reduced integration surface.

The desired desktop ownership model is one native desktop backend with two
possible web presentations:

```text
Tauri webview ---------\
                       >-- desktop application service --> Rust engine
JSTorrent extension ---/
```

When the extension is chosen, native messaging may start the installed desktop
product without creating its webview window, or attach to an already running
backend. Opening the desktop UI later should attach a webview to that same
backend and profile. It must not start a competing application-service
instance or require the user to take the profile away from the extension.
The preferred successor interpretation is that the native host is a thin
bootstrap and connection surface for the desktop product: **headless** means
the same native application service running without a webview, not a separate
torrent or IO-daemon product.

This is a successor direction rather than a claim about the current JSTorrent
topology. Current JSTorrent separates the extension engine, native host,
profile-scoped KV store, IO daemon, and Tauri application, and exposes
profile-in-use and takeover behavior. That history demonstrates the need for
explicit ownership, but the successor should not preserve takeover as the
ordinary act of switching UIs.

The desktop handoff must eventually define and prove:

- how native messaging finds or starts exactly one backend instance;
- how a headless backend later creates or focuses the embedded desktop UI;
- how an extension page is opened or focused without duplicating it;
- whether both presentations may remain attached simultaneously;
- how update, tray, deep-link, file-association, and shutdown ownership work
  when no webview is open; and
- how stale processes, incompatible versions, and profile conflicts recover
  without presenting an ambiguous takeover prompt.

The current in-process Tauri product remains the implementation and validation
path until an extension-control tactical accepts a different lifecycle and
security boundary. Completed Tactical
[`163`](../tactical/163-desktop-external-torrent-intake.md) adds RSTorrent-owned
`magnet:` and local `.torrent` activation to that current product only. It does
not implement extension routing, reuse JSTorrent identity, or decide the later
successor handoff topology. Installed Linux arm64 and Windows
x86_64-application plus macOS arm64 acceptance pass. The macOS campaign
preserved JSTorrent as the inherited default handler and targeted RSTorrent by
its current incubation bundle identifier. Exact hosted run `32775002484`
passed all eight platform jobs.

Tactical `166` adds only the predecessor seam to that later control work.
Its distinct `com.jstorrent.rstorrent.native` host can identify compatibility
and request launch of the existing desktop application; it owns no profile,
torrent state, listener, or application service. The initial JSTorrent Beta
ZIP established Chrome Web Store item `gcgoepclopkgijmclmlheafaglmbjlcc`; its
public key and exact beta origin are now pinned. The installed Chrome
`hello`/launch smoke passes with Chrome 151 and an installed unsigned macOS
arm64 app: `hello` succeeds while the desktop is stopped, and explicit launch
starts the cold app. Allow-listing the existing JSTorrent extension origin
does not make that extension compatible while it still addresses legacy host
`com.jstorrent.native`.

## ChromeOS Backends

### Crostini plus extension

Crostini is implemented as a first-class ChromeOS backend rather than only an
emergency fallback. The first local x86_64 package bundles and serves the mature
JavaScript/React control surface beside one native Linux process that owns the
Rust application service, database, networking, hashing, and filesystem I/O.
The extension is the Chrome-resident launch, focus, setup, and future
integration surface. It may later host the same detachable React presentation,
but the initial product does not duplicate those assets inside the extension.

The implementation adapts the proven `web-server-chrome` Crostini pattern:

- an owned, versioned, per-user x86_64 native package;
- an on-demand user service and registered non-terminal ChromeOS Launcher
  entry;
- a small launch helper that can wake a stopped Crostini VM and hand off to a
  dormant extension worker;
- a stable local controller endpoint with capability and version negotiation;
- exact local-host admission and persistent browser-session authentication;
- bundled setup, recovery, and uninstall guidance; and
- a normal backend-served tab for routine control with extension setup and
  recovery pages.

RSTorrent adapts that pattern without copying web-server-specific policy. The
physical x86_64 campaign proves warm and twice-stopped-VM launch, singleton
service/listener/UI behavior, continued controlled transfer with no browser
view, persistent-profile recovery, and exact normal-uninstall and purge
ownership. It does not prove full reboot, suspend, signed update/rollback,
ARM64, incoming TCP, UDP tracker/DHT, uTP, or forwarding behavior. A later
physical storage campaign proves that the automatically mounted ChromeOS
Downloads path is not writable before **Share with Linux** and that its shared
9P path is materially slower than Crostini-local Btrfs for torrent-relevant
reads and writes. Tactical `178` keeps Linux `~/Downloads` as the recommended
default, explains its automatic **Linux files > Downloads** visibility, and
gives the exact opt-in sharing and selection steps in the Crostini React UI.

The efficiency advantage is architectural: the control boundary carries UI
state, while all frequent peer and file operations remain in one Rust process.
It does not establish a performance claim until measured against the Android
and desktop products.

### Android plus extension

Active Tactical
[`194`](../tactical/194-chromeos-android-extension-control.md) selects the
Android-plus-extension configuration for implementation. The Android
application operates normally on ChromeOS while exposing a narrow,
ChromeOS-only authenticated application-control surface to the extension.
This is not the current raw IO-companion architecture: the Android foreground
service owns the complete Rust application service and engine in-process,
while the extension packages the shared React application as another
presentation of its commands and views.

The extension and Compose UI therefore see the same Android profile. Either
surface may be attached without copying torrents or starting a second engine,
subject to the same revision, view-set, and lifecycle rules as other clients.

The cold-start handoff is materially less convenient than the Crostini path or
the Android app alone. The current ChromeOS behavior does not provide a
reliable one-action chain:

- the browser can make only a best-effort Android intent launch and ChromeOS
  requires an **Open with** confirmation;
- the remembered-launch choice is not reliable in current maintainer use; and
- the Android app cannot reliably open the browser extension UI in return.

A cold user therefore starts or confirms the Android application and then
opens the extension. Once the Android foreground service is already running,
the extension can reconnect directly. Product copy must describe this rather
than promise seamless launch.

Root acquisition is also presentation-neutral. The shared
`chooseDownloadRoot` interaction uses a Linux path picker for Crostini and an
Android-owned SAF picker for the Android backend. Android keeps the URI and
grant private and returns only the resulting root snapshot. A new selected
tree becomes the one current/default root for future downloads; prior roots
and grants remain registered for the torrents already bound to them, and a
healthy retained root may be made current explicitly. Repair preserves the
root ID, and removal is allowed only when a root is neither current nor
referenced. The extension cannot submit a URI or silently redirect an existing
torrent.

Android plus extension remains a useful option for people who prefer the
extension's dense UI or want the same Android profile in both presentations.
It is now a required migration-continuity lane for current JSTorrent companion
users, but it should not dictate the primary ChromeOS flow while the two-action
cold start remains. The promise is a familiar operating model after a fresh
RSTorrent install, root grant, and pairing; it does not import JSTorrent
torrents, settings, tokens, or verified state.

The selected security and topology boundary is deliberately local and narrow:
the extension uses the fixed ChromeOS ARC host and a bounded RSTorrent-only
port list, requests its exact host permission at the user's connect action,
and authenticates through explicit Android approval. Exact Host, extension
Origin, bearer credential, backend/profile identity, resource bounds,
revocation, same-device routing evidence, and current Chrome Local Network
Access behavior are release gates. There is no network discovery, arbitrary
host input, LAN/tailnet/Internet claim, Android-served application shell, raw
file/socket/hash path, or media path in the first slice.

### Android alone

The Compose application remains the simplest ChromeOS path for a user who
wants Google Play installation and does not need the extension interface. It
uses the same Android backend and profile as extension-controlled Android, so
changing between those two presentations is not a migration.

Completed Tactical
[`117`](../tactical/117-jstorrent-shaped-android-product-ui.md) makes this a
credible primary path by adapting JSTorrent Android standalone's Library,
torrent-detail tabs, Speed, DHT, Logs, and Settings hierarchy to the typed
RSTorrent application. It retains RSTorrent branding and truthful capability
gaps rather than reproducing QuickJS or unsupported feature policy.

### ChromeOS choice and status

The first-run UX should emphasize two backend choices:

- **Use the Android app**, with optional extension remote control after the
  Android backend is running; and
- **Use ChromeOS Linux with the extension**.

The product must not infer that Google Play, Android applications, or Crostini
is available from weak platform heuristics. It may remember an explicit user
choice and recognize a reachable, authenticated backend. The extension should
always display whether it currently controls **Android** or **ChromeOS Linux**.

## Backend Identity, Switching, And Isolation

The extension may remember separate pairings for desktop, Android, and
Crostini. Each successful connection should expose at least a backend kind,
stable backend instance identity, profile identity, application/control
protocol range, and current product version. Authentication tokens and
transport endpoints are scoped to that backend identity.

Switching the extension from Android to Crostini switches libraries. It does
not move torrents, copy content, or reconcile progress. The UI must make that
transition visible so an apparently empty library is not mistaken for data
loss.

Android and Crostini cannot share live data or state:

- each owns its own profile database and verified-piece authority;
- Android SAF grants and document identities are not Linux paths;
- Crostini paths and ChromeOS **Share with Linux** state are not Android
  capabilities; and
- even when both can reach similarly named user files, concurrent ownership is
  unsafe without an explicit transfer and verification design.

The same torrent may be added independently to both backends, but the product
must not imply synchronized intent or progress. A later cross-backend move is
an explicit export/import operation, not live profile synchronization.

## Launch And Handoff UX

The current JSTorrent desktop and ChromeOS products show that a technically
working connection can still be difficult to understand. Product success
requires an explicit handoff model, not only a transport.

Every supported configuration should strive for these properties:

- the user can tell which backend is installed, running, paired, and selected;
- one launch action opens or focuses the preferred presentation when the
  platform permits it;
- an action that merely requests another platform surface never reports that
  the surface definitely opened;
- starting or focusing a UI does not transfer or duplicate backend ownership;
- a stopped backend produces one actionable recovery path rather than a loop;
- closing a view does not silently stop active torrents unless the user chose
  that product behavior; and
- stale endpoints, tokens, versions, or instance identities fail closed and
  offer an understandable repair or re-pair action.

The unavoidable Android two-action cold start is an explicit exception, not a
failure to acknowledge in copy. Crostini may use its registered Linux Launcher
to wake the VM and then hand off to the extension. Desktop native messaging can
start or attach to the native backend, but its exact window and process
lifecycle still requires a tactical.

## Later JSTorrent Graduation

RSTorrent's current work is an independent incubation beta. Desktop beta uses
the `com.jstorrent.rstorrent` identifier, the RSTorrent update route, and its
own updater key so it can coexist safely with current JSTorrent installations.
There is no promise that an RSTorrent beta installation will silently change
into the production JSTorrent application.

The broad later goal is different: once the implementation is ready, ship it
as a normal JSTorrent desktop update while retaining JSTorrent branding,
`com.jstorrent.desktop`, and the updater trust root already embedded in
installed JSTorrent clients. The new release may present a refreshed interface
while using the Rust engine underneath.

Migration of useful legacy settings, torrent intent, and storage references is
best effort. Its exact source versions, fields, UX, and evidence will be chosen
from the products that exist when graduation work begins. Exhaustive parity or
migration of every historical profile and runtime detail is not a prerequisite.
Legacy completion claims still cannot become verified RSTorrent content without
the new engine's ordinary integrity checks.

This general direction reserves the production identity; it does not place the
JSTorrent updater private key in RSTorrent beta automation, authorize a
production update, or add migration work to the current beta checklist.

## Working Name And Product Identity

`RSTorrent` remains a useful incubation and component name because it connects
the Rust engine with JSTorrent's product lineage. Its similarity to the older
`rTorrent` name is acknowledged but is not currently considered a reason to
discard it: the visual and historical connection to JSTorrent is stronger in
this product context. `KTorrent` is not a practical alternative because it is
already the established KDE client name.

Maintainer direction on 2026-08-22 selects RSTorrent as the public product name
for the foreseeable release line, beginning with the incubation beta.
Direction on 2026-08-23 selects
`com.jstorrent.rstorrent` as its current desktop identifier. A later
graduation should not discard JSTorrent's installed application or extension
audience merely to make the engine rewrite visible. Maintainer direction on
2026-08-27 nevertheless makes every `0.1.x` identifier and installation
disposable incubation state rather than a compatibility obligation for the
first supported release. The current value remains the operational default;
changing it still requires an explicit product decision.

## Existing Distribution And Coexistence

The existing JSTorrent desktop and extension identities and their installed
audiences are important assets. A later graduation should normally update
those products to use the proven Rust application contract instead of
launching unrelated replacement identities. RSTorrent remains independently
released during incubation. Exact extension, store, coexistence, and retirement
mechanics are deliberately deferred until graduation work is authorized.

## Current Evidence And References

Current repository evidence already supplies useful parts of this direction:

- RSTorrent's application service has one profile-local typed SQLite authority
  with conservative recheck and transport-neutral commands.
- The leased application-view contract supports independent detachable clients
  and recovery after a view disappears or suspends.
- The Tauri and browser adapters already share one React presentation through
  different transports, while Android consumes the same semantic model through
  UniFFI and Compose.
- Tactical `170` proves the separate Linux-server composition on x86_64: one
  service process retains its application/profile/transfer/seed authority when
  every browser view detaches, then a fresh view recovers the same durable
  state. Its x86_64/ARM64 package construction does not turn desktop or mobile
  into clients of that service.
- The current JSTorrent checkout at `~/code/jstorrent` documents the extension
  engine, native-host bootstrap, profile conflict/takeover, IO daemon, Android
  companion, and Tauri split in `desktop/README.md`, `extension/README.md`, and
  `docs/contracts/`.
- The sibling `~/code/web-server-chrome` checkout records the signed Crostini
  installer, Launcher wake, local extension handoff, authenticated controller,
  and physical ChromeOS evidence in
  `docs/topics/chromeos-crostini-launcher.md`.

These are architecture and behavior references. This topic imports no source,
fixture, or wire contract from either sibling project.

## Open Decisions And Required Evidence

- The exact later stage at which the proven implementation graduates through
  JSTorrent's existing product and updater identity.
- The exact desktop native-host/backend process, single-instance, window, and
  shutdown ownership model. Desktop beta updating now adopts the external
  `desktop-update-v1` contract; its independent route/key, client presentation,
  and release pipeline are implemented through local gates, while hosted
  signed packages, route deployment, and installed evidence remain open in
  [`beta-release-readiness.md`](beta-release-readiness.md).
- Whether Tauri and extension views may remain attached simultaneously on
  desktop, and how command conflicts are presented.
- Production pairing, authentication, origin, rate, replay, token rotation,
  and protocol-compatibility policy for extension transports beyond Tactical
  `194`'s exact same-device ChromeOS Android boundary.
- Physical completion of Tactical `194`'s Android foreground-service,
  fixed-ARC endpoint, optional host permission, Local Network Access,
  pairing/revocation, same-device reachability, and cold-start recovery
  contract.
- Crostini update/rollback, suspend/reboot, shared-storage, physical native
  ARM64 runtime, and broader network contracts beyond the accepted public
  `crostini-v0.1.0` x86_64 installation and hosted two-architecture packages.
- Promoted signed public Linux-headless artifacts/stable manifest, native
  ARM64 systemd/update, representative mount/reboot, and long-run unattended
  evidence beyond the completed local x86_64 package/service campaign and
  signed source lane.
- The bounded best-effort set of legacy state worth migrating at graduation
  time.
- Physical desktop extension-control evidence and ChromeOS recovery from stale
  or incompatible signed installations.
- Physical ChromeOS TCP and UDP torrent behavior and representative Android
  versus Crostini resource and throughput measurements.
- iOS migration source and public distribution/release policy. Tacticals
  `147`--`149` select iOS 16+, SwiftUI, generated UniFFI, qualified on-device
  roots, finite lifecycle behavior, and reproducible development/archive
  packaging without App Store/TestFlight publication.

## Recommended Next Work

Completed Tactical `166` supplies the exact store identity and installed
desktop bootstrap evidence; resume the beta-readiness campaign's signed
RSTorrent package and updater gates. Completed Tactical `167` supplies the
bounded ChromeOS Linux source package and physical handoff evidence. Completed
Tactical `169` plus its separately authorized release operation supply signed
public `crostini-v0.1.0` artifacts and exact x86_64 website-install acceptance;
do not treat that as full extension control, physical ARM64 runtime,
update/rollback, or legacy migration.
Completed Tacticals `170` and `171` supply the first configured ordinary-user
Linux headless backend, signed source update lane, exact trusted-LAN mode, and
installed x86_64 evidence. Treat public candidate/channel promotion,
system-wide ownership, native ARM64 service/update evidence, and owner remote
authentication as separate future operations or slices.
Tactical `194` implements the first migration-critical Android
JSTorrent-parity slice and physically proves the extension-controlled ChromeOS
journey, retained SAF-root selection/repair, and one Android engine/profile
owner. Its companion now binds only to ARC's fixed guest address: the exact
extension connects from ChromeOS while the Chromebook Wi-Fi address refuses
raw TCP and the formerly successful spoofed-Host/Origin request. Completion/
error notifications and metered/VPN enforcement remain the next core Android
release slices rather than additions to its completed transport boundary.
[`android-jstorrent-replacement.md`](android-jstorrent-replacement.md) now owns
the stricter Android production-replacement ledger, feature dispositions, and
coordinated extension rollout. When implementation is authorized, create its
bounded production-handoff tactical to fix the intentionally best-effort
legacy-state scope from then-current evidence before scheduling a store
replacement.
