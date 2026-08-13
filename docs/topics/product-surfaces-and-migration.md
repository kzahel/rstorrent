# Product Surfaces And JSTorrent Migration

Topic: `product-surfaces-and-migration`

Status: The backend/presentation model, ChromeOS choices, desktop extension
direction, and user-initiated semantic import posture were accepted in product
discussion on 2026-08-02. This topic records graduation direction, not an
authorization to implement the extension, Crostini packaging, production
remote control, or migration in the current engine tactical. Exact transports,
security boundaries, rollout policy, and imported fields remain to be designed
and validated in bounded tacticals. Maintainer direction on 2026-08-09 accepts
iOS as an eventual first-party in-process backend; Tactical
[`116`](../tactical/116-platform-storage-coherence-and-ios-feasibility.md)
front-loads the physical storage/network/lifecycle feasibility that shaped the
common engine boundary. Explicit direction on 2026-08-13 schedules the first
maintained SwiftUI product in Tacticals `147`--`149`; that campaign is complete
with qualified external folder selection and physical lifecycle evidence,
without migration or publication.

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
  authority and verified resume invariants.

This topic does not select the final public name or domain, promise an exact
release sequence, turn the native engine into a general-purpose daemon, or
make product migration the current engine-campaign priority.

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
| ChromeOS | Rust application service in Crostini | Browser extension | Owns a Linux profile and roots |
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
security boundary.

## ChromeOS Backends

### Crostini plus extension

Crostini should be developed as a first-class ChromeOS backend rather than
only an emergency fallback. The extension remains the full JavaScript/React
control surface while one native Linux process owns the Rust application
service, database, networking, hashing, and filesystem I/O.

The proven `web-server-chrome` Crostini pattern is the starting product
reference:

- a signed per-user x86_64 or ARM64 native component;
- an on-demand user service and registered non-terminal ChromeOS Launcher
  entry;
- a small launch helper that can wake a stopped Crostini VM and hand off to a
  dormant extension worker;
- a stable local controller endpoint with capability and version negotiation;
- contextual host permission, one-time claim, and persistent authentication;
- bundled setup, recovery, update, rollback, and uninstall guidance; and
- a normal extension tab for setup or recovery with a focused extension
  window for routine control.

RSTorrent must adapt that pattern rather than copy its web-server-specific
policy. In particular, torrent lifecycle across Crostini stop, suspend, and
reboot; Linux and ChromeOS-shared storage performance; incoming TCP; UDP
tracker and DHT behavior; uTP; and ChromeOS forwarding constraints require
physical evidence before this path is called complete.

The efficiency advantage is architectural: the control boundary carries UI
state, while all frequent peer and file operations remain in one Rust process.
It does not establish a performance claim until measured against the Android
and desktop products.

### Android plus extension

The Android application may operate normally on ChromeOS while exposing a
narrow authenticated remote-control surface to the extension. This is not the
current raw IO-companion architecture: the Android foreground service owns the
complete Rust application service and engine in-process, while the extension
is another presentation of its commands and views.

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

Android plus extension remains a useful option for people who prefer the
extension's dense UI or want the same Android profile in both presentations.
It should not dictate the primary ChromeOS flow while the two-action cold
start remains.

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

## Migration From Current JSTorrent

### Manual import is the initial policy

Migration may be user-initiated rather than automatic. A current JSTorrent
surface can show a notification, banner, or settings entry when an eligible
successor backend is installed, then let the user inspect and start an import.
Automatic destructive conversion is not required.

The prompt should be dismissible and should explain:

- which source installation and profile were found;
- which destination backend and profile will receive the import;
- which state can be imported;
- which content roots need confirmation or remapping;
- that claimed progress will be checked before it is trusted; and
- that the legacy installation remains available until the user removes it.

Desktop's current multi-profile registry makes source selection explicit. The
successor may still expose only one automatically created destination profile;
supporting every legacy profile as a simultaneously running new profile is not
a migration requirement.

### Import semantics

Import converts bounded semantic state into the typed successor schema. It
does not open the current JSTorrent KV database as the new runtime authority or
preserve the old process topology.

Candidate imported state includes:

- torrent identity and canonical source intent;
- exact metainfo or info-dictionary bytes when available and independently
  hash-authorized;
- desired running, paused, archived, or selected-file intent where the new
  application model has an equivalent;
- supported user settings and organization; and
- content-root references after explicit destination-specific remapping.

Legacy bitfields and completion claims are hints for bounded recheck. They
never establish verified successor content without hashing through the new
engine's ordinary integrity path. The importer may conservatively omit or
clear progress it cannot map safely.

Do not import ephemeral or topology-specific state such as:

- connected or cached peers, DHT routing entries, in-flight blocks, or recent
  transfer rates;
- PIDs, ports, process liveness, takeover ownership, or daemon discovery
  records;
- native-host, IO-daemon, Android-companion, or extension pairing tokens; or
- transient errors, retries, and presentation replicas.

Sensitive tracker or source credentials require a separately reviewed field
and destination policy rather than accidental inclusion in a generic settings
copy.

### Safety, retry, and rollback

The importer should treat legacy files as read-only, identify the exact source
profile and schema it inspected, and retain an import receipt in the
destination. Repeating the same import must not duplicate torrents or apply a
stale source over newer destination intent without confirmation.

An import writes through ordinary destination transactions and leaves the
legacy database and payloads untouched. Content is not copied or deleted
implicitly. If a root cannot be represented on the destination platform, the
user remaps it or imports the torrent without trusted progress. Deleting the
old application, databases, or content is a later explicit action after the
user has inspected the new library and recheck results.

Migration is scoped to a selected backend. Importing a JSTorrent Android
profile into the successor Android backend does not populate Crostini, and
importing a desktop profile does not populate Android. A later move between
successor backends uses the same explicit export/import posture.

## Working Name And Product Identity

`RSTorrent` remains a useful incubation and component name because it connects
the Rust engine with JSTorrent's product lineage. Its similarity to the older
`rTorrent` name is acknowledged but is not currently considered a reason to
discard it: the visual and historical connection to JSTorrent is stronger in
this product context. `KTorrent` is not a practical alternative because it is
already the established KDE client name.

This naming direction is not a final public-brand decision. Domain and package
availability are time-sensitive and must be checked again when a release name
is selected. More importantly, the successor should not discard JSTorrent's
installed extension audience and product discovery merely to make the engine
rewrite visible as a new brand.

## Existing Distribution And Coexistence

The existing JSTorrent extension identity and store discovery are important
assets. The preferred graduation path should evaluate updating that extension
to support the successor application contract rather than launching an
unrelated extension with no installed audience. This does not decide whether
`RSTorrent` is a public preview name, a backend/component name, or only the
incubation repository name.

During coexistence, one extension may need legacy and successor adapters. It
must show which engine generation and backend it is controlling, avoid letting
both claim the same profile or content implicitly, and preserve a usable
rollback path. A reasonable staged experience is:

1. notify the user that a successor backend and manual import are available;
2. let the user select desktop, Android, or Crostini as the destination;
3. preview and run a read-only-source semantic import;
4. recheck claimed content and show exceptions;
5. keep legacy operation available while the user evaluates the result; and
6. offer explicit legacy retirement only after the successor is proven useful.

The exact Chrome Web Store, desktop installer, Android package, versioning,
and rollback mechanics require release-specific evidence and maintainer
approval.

## Current Evidence And References

Current repository evidence already supplies useful parts of this direction:

- RSTorrent's application service has one profile-local typed SQLite authority
  with conservative recheck and transport-neutral commands.
- The leased application-view contract supports independent detachable clients
  and recovery after a view disappears or suspends.
- The Tauri and browser adapters already share one React presentation through
  different transports, while Android consumes the same semantic model through
  UniFFI and Compose.
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

- Whether the public product, extension, and native components use the
  JSTorrent or RSTorrent name at each migration stage.
- The exact desktop native-host/backend process, single-instance, window,
  update, and shutdown ownership model.
- Whether Tauri and extension views may remain attached simultaneously on
  desktop, and how command conflicts are presented.
- Production pairing, authentication, origin, rate, replay, token rotation,
  and protocol-compatibility policy for every extension transport.
- The exact Android remote-control endpoint and its foreground-service,
  permission, ChromeOS networking, and cold-start recovery behavior.
- The exact Crostini package, service, local endpoint, storage, networking,
  update, rollback, and uninstall contracts.
- The migration source versions, imported-field matrix, profile-selection UX,
  root remapping, import receipts, retry behavior, and notification cadence.
- Physical desktop and ChromeOS handoff evidence, including repeated stopped
  backend launches and recovery from stale or incompatible installations.
- Physical ChromeOS TCP and UDP torrent behavior and representative Android
  versus Crostini resource and throughput measurements.
- iOS migration source and public distribution/release policy. Tacticals
  `147`--`149` select iOS 16+, SwiftUI, generated UniFFI, qualified on-device
  roots, finite lifecycle behavior, and reproducible development/archive
  packaging without App Store/TestFlight publication.

## Recommended Next Work

Do not interrupt the active engine tactical merely because this direction is
now recorded. When product migration or extension work is authorized, begin
with one bounded tactical that:

1. inventories the exact supported legacy desktop, extension, and Android
   persistence versions;
2. fixes the backend identity and attachment model without designing a public
   general-purpose daemon;
3. defines the transport threat model and owner/task/cancellation map;
4. defines a versioned semantic import plan and read-only-source fixtures;
5. prototypes the desktop handoff and one ChromeOS backend without weakening
   the other choices; and
6. records deterministic, packaged, and physical acceptance gates before any
   migration notification ships.
