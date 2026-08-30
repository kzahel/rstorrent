# Tactical 194: ChromeOS Android Extension Control And Retained SAF Roots

Status: **Active as of 2026-08-30.** Explicit maintainer direction selects a
focused Android release-parity campaign against current JSTorrent Android and
makes ChromeOS companion presentation the first migration-critical slice.
This tactical authorizes design and implementation in the repository, but it
does not authorize Chrome Web Store or Google Play publication, production
JSTorrent extension mutation, release signing, or legacy-profile import.

Topics:
[`product-surfaces-and-migration`](../topics/product-surfaces-and-migration.md),
[`client-surfaces`](../topics/client-surfaces.md),
[`application-connection-architecture`](../topics/application-connection-architecture.md),
[`application-control`](../topics/application-control.md),
[`application-view-api`](../topics/application-view-api.md),
[`download-roots`](../topics/download-roots.md),
[`download-root-acquisition`](../topics/download-root-acquisition.md),
[`android-saf-storage`](../topics/android-saf-storage.md),
[`beta-release-readiness`](../topics/beta-release-readiness.md), and
[`capability-readiness`](../topics/capability-readiness.md).

Dependencies: the Android foreground-service/application owner, SAF storage,
the generated application contract, the mature React presentation, the
bounded WebSocket application adapter, and the pinned JSTorrent checkout at
revision `25e4b701433fd815398ba89526546f5e4f072e3f`. Android's durable release
package identity remains an independent `AND-002` gate. Companion launch
metadata must consume that identity from one validated build source rather
than hard-code today's provisional `org.rstorrent.bootstrap` value.

## Decision And Desired Outcome

RSTorrent will support the useful **JSTorrent companion user journey** on
ChromeOS, but not JSTorrent's companion architecture.

JSTorrent currently keeps the torrent engine in the extension and uses the
Android app as an authenticated raw socket, filesystem, hashing, key/value,
power, and media daemon. RSTorrent instead keeps its one Rust application
service, engine, profile, networking, hashing, and SAF authority in the
Android foreground service. The extension packages the shared React
presentation and attaches only through the typed application connection.

```text
JSTorrent today
  extension UI + TypeScript engine
             |
      raw IO companion protocol
             |
  Android sockets/files/hash/KV/media

RSTorrent selected shape
  Compose UI -----------\
                         > Android ProductEngineService
  extension React UI ---/    -> one ApplicationService/profile
                                  -> Rust engine + SAF storage
```

There is no Android **companion mode** that replaces standalone mode. Compose
and the extension may attach at the same time, see the same profile and
revision stream, and issue ordinary commands through the same application
owner. Closing every Android activity and extension tab detaches
presentations; it does not stop an active transfer. Android's foreground
notification and explicit Stop action retain lifecycle authority.

The migration promise is **workflow continuity**, not automatic data
migration. A current ChromeOS user can install the distinct RSTorrent Android
app, pair the extension, choose a download root from the shared React flow
through Android's SAF picker, use the familiar dense browser UI, and leave the
Android service running without a Compose activity. JSTorrent torrents,
settings, pairing tokens, verified state, and payload authority are not
imported by this slice.

## Stopping Condition

This tactical stops when a physical x86_64 Chromebook proves the following
fresh, reconnect, coexistence, failure, and cleanup path with a release-built
but unpublished RSTorrent Android package and the exact pinned beta-extension
identity:

1. The extension's explicit **Connect Android app** action requests only its
   exact ARC host permission, survives the current Chrome Local Network Access
   prompt, and performs the documented best-effort Android launch. ChromeOS
   may still require **Open with**; the UI never claims that the app opened.
2. The Android activity starts the existing foreground owner, shows one
   bounded pairing request, and requires explicit user approval before any
   library, root, or command access.
3. The extension opens its packaged shared React application, identifies the
   backend as Android, displays the exact backend/profile/protocol identity,
   and presents the same **Choose folder...** and **Repair...** interaction as
   Crostini. That platform capability opens Android's SAF picker, persists and
   probes the grant under a stable opaque root ID, returns only the resulting
   root snapshot, and never exposes a SAF URI, document ID, or descriptor to
   React or the connection.
4. Selecting a new healthy SAF tree makes it the one current/default root for
   future downloads. Earlier roots and grants remain registered for torrents
   already bound to them. Repair replaces the grant behind the same root ID;
   it never moves or redirects an existing torrent. Unreferenced non-current
   roots can be removed with exact grant release.
5. Picker selection, cancellation, permission denial, duplicate selection,
   activity recreation, extension disconnect, revoked-grant repair, root
   removal, and restart persistence settle without a process-static callback,
   duplicate picker, false root, or leaked pending request. If Android cannot
   immediately raise the picker, an ordinary actionable notification lets the
   user complete that requested selection in Android.
6. The extension completes magnet and local `.torrent` intake plus ordinary
   library, detail, file-priority, queue, pause/resume, recheck, remove,
   settings, and diagnostic operations through the semantic application
   connection, using the current Android root for new transfers and each
   retained torrent's bound root thereafter.
7. Closing the Compose activity and every extension page during a controlled
   transfer leaves one Android application/engine/profile owner running. A
   newly opened extension page recovers the same torrent and exact current
   state without a second engine, copied profile, or takeover prompt.
8. Opening Compose while the extension remains attached shows the same
   torrent and converges after commands from either presentation. A conflict
   follows existing receipt/revision behavior rather than client-side
   last-writer invention.
9. Wrong Host, wrong extension Origin, missing/invalid/revoked credential,
   replayed or expired pairing request, excessive probes, oversized frames,
   incompatible protocol, stale endpoint, and service shutdown all fail
   closed with bounded recovery. Revocation terminates every socket belonging
   to the revoked pairing.
10. A same-LAN device cannot reach an application route. If the Android bind
   must be wildcard for ARC reachability, physical routing evidence must still
   prove that authenticated traffic remains on the same-device ARC bridge;
   otherwise the tactical stops for a different OS transport rather than
   shipping a bearer token over a LAN-reachable cleartext listener.
11. Stopping the foreground service closes and joins the listener, connection
   pumps, subscriptions, pairing work, SAF work, application service, and
   engine. Unpairing or uninstalling removes only RSTorrent-owned credentials
   and state; controlled payload cleanup remains exact.

Phone Android builds and tests must also prove that the ChromeOS listener
does not start from ordinary phone launch or restart.

## Product And Capability Boundary

The extension contains the release-built shared React assets. It does not
load executable code from Android or the network, and Android does not serve a
second copy of the application shell. The extension uses its foreground page
as the presentation owner; a service worker may perform launch/focus and
pairing bootstrap work but must not keep a hidden view set or pretend to own
the engine lifecycle.

The first companion capability profile includes:

- application handshake, commands, snapshots, patches, acknowledgements,
  reconnect, and the existing bounded `.torrent` byte attachment;
- honest Android backend, instance, profile, product-version, protocol-range,
  pairing, root-health, and connection status;
- extension-local presentation preferences; and
- the shared root-selection platform capability, backed by Android's SAF
  picker for add, current/default selection, repair, and bounded removal.

The React root interaction is intentionally the same for Crostini and Android;
the trusted acquisition adapter differs. Crostini launches its native Linux
picker and installs or repairs a path root. Android launches the system SAF
picker and installs or repairs a platform-owned grant. React receives the
resulting `StorageRootSnapshot`, never an ambient path or SAF URI. On Android,
one root is current/default for all new downloads; older roots remain visible
and available to the torrents already bound to them, and a healthy one may be
made current explicitly. Selecting or changing the current root never
relocates existing torrents.

The profile disables or hides updating, tray/window controls, desktop file
associations, remote relay configuration, media-capability creation, browser
playback, and complete-file open from the extension. Compose retains
completed-file open. Progressive or complete media delivery across the ARC
listener is a later Android playback slice with its own byte-path and exposure
review.

Negotiated capability facts, not a user-agent or caller-supplied backend name,
drive the small platform difference. Android advertises SAF acquisition,
retained roots, one-current-root add policy, repair, and joined platform
removal. Its add dialog uses only the current/default root; Settings lists all
retained roots and permits **Make current**, **Repair**, or valid removal.
Crostini keeps its existing path-root and per-add selection behavior. Both use
the same React components and root snapshots rather than separate product
shells.

The beta extension may remember separate Android and Crostini pairings, but
one visible application page selects one backend at a time and labels the
choice. Switching backends never merges libraries or roots. Crostini's
`penguin.linux.test:3030` flow and profile remain unchanged.

## Android SAF Root And Picker Contract

The shared React application already expresses folder acquisition through
`ApplicationViewClient::chooseDownloadRoot`. Tactical `194` preserves that
platform-capability seam. It does not add a portable torrent command, an
Android-shaped application command, or a raw filesystem endpoint.

Android replaces its current singleton `tree-uri` preference and fixed
`downloads` configuration with a bounded app-private SAF root registry. Each
record contains a generated opaque root ID, bounded display label, persisted
tree URI, namespace generation, and lifecycle-journal state. Grant presence and
root availability are re-derived and probed rather than persisted as current
truth. The URI remains Android platform state; the ordinary application
database retains the same opaque root ID, root/default settings, and
per-torrent binding used by desktop. The platform broker resolves every
request's root ID through this registry so simultaneous torrents may safely
use different retained grants under the one shared 40-handle/16-request
resource bounds.

The registry migration is RSTorrent-local compatibility, not JSTorrent state
import. On first upgraded start, an existing valid singleton `tree-uri` is
installed exactly once under the existing `downloads` root ID so current
RSTorrent torrents, default state, and grant survive without relocation or a
new picker. An absent or revoked legacy grant produces the same unavailable
`downloads` root and repair path. The migration is versioned, idempotent, and
must not release the legacy grant until the new record is durable.

The accepted Android policy is deliberately narrower than desktop's per-add
root selection. **Current** is Android product wording for the existing durable
`default_root`; it is not a second pointer or platform preference:

- at most one configured root is current/default for new downloads. If it is
  unavailable, new downloads wait for repair or an explicit current-root
  change rather than silently choosing another retained root;
- selecting a previously unregistered SAF tree adds a stable root and commits
  it as current/default only after its grant and probe succeed;
- selecting an already registered healthy tree deduplicates by canonical SAF
  identity and makes the existing root current rather than creating an alias;
- changing the current root affects future adds only. Existing torrents keep
  their durable root IDs and continue through their earlier grants;
- any healthy retained root may be made current explicitly, but a non-current
  root is not offered as an ad hoc per-torrent override in the Android add
  flow;
- repair targets one unavailable root ID, replaces only its platform locator,
  advances its namespace generation, preserves its torrent bindings, and does
  not make it current unless it was already current. Selecting another
  retained root's URI as the repair target is rejected rather than aliasing or
  merging the records; and
- a root may be removed and its grant released only when no retained torrent
  references it and it is not current. Choosing a replacement current root is
  required before removing the current root.

An authenticated, capability-advertised request creates at most one pending
picker operation. The foreground service owns its random bounded request ID,
kind (add or exact-root repair), initiating Compose owner or authenticated
pairing, generation, and two-minute expiry. Concurrent requests fail as busy
rather than opening multiple activities. A dedicated lifecycle-safe activity
uses `ACTION_OPEN_DOCUMENT_TREE` or the equivalent Activity Result contract. A
user-initiated direct launch is attempted; if ChromeOS background-activity
policy prevents it, the service posts an ordinary actionable notification for
the same pending request. It does not claim call/alarm eligibility or abuse a
full-screen intent.

Success takes the persistable read/write grant and commits the platform and
application records only after bounded local-provider qualification succeeds.
The response is `StorageRootSnapshot`; user cancellation returns `null`.
Permission denial, unsupported provider, expiry, service shutdown, or failed
application commit leaves the prior registry/current root unchanged and
releases any newly acquired grant not already retained. Root-state changes are
published to every attached presentation.

Activity recreation reads pending state from the service owner; there is no
process-static callback. Process death invalidates the pending request rather
than treating it as approved. A picker completion may safely install or
repair the root after its requesting extension page disconnects, but no
response is delivered to a replaced request generation. On restart, every
persisted grant is re-probed independently; one unavailable old root blocks
only torrents bound to it and does not displace a healthy current root.

SAF URIs, provider document IDs, descriptors, and intent contents never enter
application frames, snapshots, browser storage, logs, diagnostics,
notification text, or extension state. Root relocation, merging, automatic
fallback, silent release of referenced grants, cloud/removable provider
expansion, and choosing an old retained root for a new per-torrent override
are not implied by this contract.

## Endpoint, Permission, And Pairing Contract

ChromeOS exposes the Android app at the established ARC host
`100.115.92.2`. The RSTorrent listener tries exactly five RSTorrent-owned
ports in order: `3030`, `3031`, `3032`, `3033`, and `3034`. This stays
separate from JSTorrent's `7800`-family companion ports, allowing both Android
apps to coexist during incubation. The port list is versioned extension
configuration, not general network discovery; there is no mDNS, subnet scan,
caller-supplied host, or arbitrary port entry.

The extension declares only `http://100.115.92.2/*` as an optional host
permission and asks for it from the user's **Connect Android app** action.
Its extension-page CSP admits connections only to the exact ARC host over
HTTP and WebSocket. Current Chrome separately prompts for Local Network Access,
including WebSocket access in current milestones; deterministic and physical
tests must exercise grant, denial, later revocation, and withheld host access.
Chrome's host permission and Local Network Access grant improve least
privilege and user understanding, but neither is treated as server
authentication.

Only a ChromeOS launch intent or a previously enabled, still-paired ChromeOS
configuration starts the companion listener. Cold launch remains visible:
the Android activity starts the foreground service and may require the
ChromeOS **Open with** chooser. Android cannot promise to open or focus the
extension in return. Once the service is running, the extension probes every
two seconds with a two-second per-port timeout until connected or explicitly
cancelled; only one probe loop exists per extension runtime.

The unauthenticated bootstrap surface is limited to versioned `hello` and
pairing operations beneath `/rstorrent/companion/v1/`. `hello` returns only a
bounded product/backend kind, protocol range, selected port, fresh nonce, and
paired/not-paired status; profile identity, torrents, settings, roots, logs,
and network state require authentication.

The authenticated platform surface adds exactly one non-semantic operation:
`POST /rstorrent/companion/v1/platform/download-root`. Its bounded body is the
existing optional `repair_root` request; it never accepts a path, URI, label,
provider ID, default ID, or caller-selected new root ID. The extension's
platform client uses this endpoint to implement `chooseDownloadRoot`, while
the application WebSocket remains the only torrent/settings command and view
transport. The response is the ordinary root snapshot or cancellation. Root
inventory arrives only through the authenticated application view; there is no
parallel `/roots` authority. `SetDefaultStorageRoot` remains the semantic way
to make a healthy retained root current. `RemoveStorageRoot` must complete the
Android root-lifecycle handshake and grant release before acknowledging its
durable mutation.

Pairing has these initial bounds and invariants:

- only the exact beta origin
  `chrome-extension://gcgoepclopkgijmclmlheafaglmbjlcc` and production
  JSTorrent origin
  `chrome-extension://dbokmlpefliilbjldladbimlcfgbolhk` may request pairing;
- at most one pending request exists, expires after two minutes, and carries
  a server-generated random identifier plus fresh nonces bound to the exact
  extension ID and extension-generated installation ID;
- five rejected, expired, or malformed requests in ten minutes suppress new
  pairing work for ten minutes without creating unbounded durable rows or
  Android activities;
- Android displays the exact recognized extension name/ID and requires an
  explicit approve/reject action; no process-static callback owns the result;
- approval creates a random 256-bit bearer credential, returns it once to the
  requesting extension, persists only its digest and bounded metadata in
  Android app-private state, and stores the credential only in that
  extension's `chrome.storage.local`;
- at most four pairing records exist so beta and future production identities
  can coexist during migration; the app never silently evicts one to admit
  another;
- repair rotates the exact record generation; unpair, app-data reset, service
  disablement, or explicit revoke closes its active sockets before removing
  authority; and
- credentials do not cross to Crostini, desktop, another extension ID, or a
  future RSTorrent application installation.

Every authenticated HTTP or WebSocket request requires the selected exact
`Host`, an allow-listed exact extension `Origin`, and the bearer credential.
Bearer comparison is constant-time. Preflight and error responses reveal no
credentials, use no wildcard origin, and admit no credentialed cross-origin
browser request. No token, pairing nonce, magnet, torrent name, root URI, or
application frame enters logs, diagnostics, notification text, intents, or
support artifacts.

The first listener admits at most four application WebSockets, eight pending
HTTP requests, one pending `.torrent` upload, 64 KiB text frames, the existing
64 MiB declared `.torrent` attachment, and the application connection's
existing bounded calls, streams, queues, leases, and heartbeat. Implementers
may tighten these values from test evidence without escalation, but may not
raise them or add another endpoint family without reconciling this tactical.

## Ownership, Tasks, And Data Flow

`ProductEngineService` remains the sole Android lifecycle owner. Its Rust
`AndroidApplicationClient` is refactored as needed so Compose subscriptions
and the companion gateway share one `Arc<tokio::sync::Mutex<ApplicationService>>`
instead of opening a second service. The Android bridge owns platform storage
and exposes no filesystem path, SAF URI, or document descriptor to the
gateway.

The application service remains authoritative for root inventory, the one
current/default root, and every torrent's durable root ID. It gains bounded
install/repair/remove operations for platform roots parallel to the existing
path-root operations. The Android adapter owns a versioned registry of at most
the application's existing 32-root limit and maps those IDs to grants. Client
startup loads every retained record into `ConfiguredStorageRoot::platform`
instead of always manufacturing `downloads`; every broker request already
carries `root_id` and must resolve that exact record rather than the current
singleton tree. Complete-file open, removal, observation, repair, and test
paths make the same root-specific change.

Because Android grant state and the application database have separate
durability owners, root add, repair, and removal use a versioned pending
operation journal. Startup deterministically completes or rolls back each
operation, releases orphan grants, projects a database root with no usable
grant as unavailable, and never aliases one URI to two IDs. A crash at any
commit boundary must leave the complete old generation or complete new
generation, not a new default whose locator was lost. The exact recovery
state machine is finalized and tested in the pure contract gate before Kotlin
or database mutation lands.

The foreground service supervises:

- the one Rust application service and maintenance owner;
- the existing Compose presentation repository when an activity attaches;
- the optional ChromeOS companion listener and its bounded connection tasks;
- one durable pairing store and one pending approval owner;
- one bounded SAF root registry, one pending picker owner, and SAF request
  workers; and
- the existing authoritative active-work CPU wake lock and foreground
  notification.

The listener owns only admission and delivery. After authentication it reuses
the existing Rust application WebSocket core; it does not translate semantic
operations in Kotlin or add REST-shaped torrent commands. Connection close
detaches/leases its views normally. Listener cancellation closes admission,
terminates connection pumps, waits for their joins, and then returns to the
foreground owner. Application shutdown cancels the listener before shutting
down the shared service and SAF broker.

The pairing owner persists complete generations atomically. A pending request
is runtime state and cannot survive process death as approved. Activity
recreation reads the pending record from the service owner; it does not rely
on a static callback. Crash during credential replacement resolves to the
complete prior or complete new generation and never admits a credential that
the Android settings surface cannot revoke.

Backend identity is generated and persisted by the Android application
profile. The authenticated handshake exposes backend kind `android`, stable
backend instance ID, stable profile ID, product version, application protocol
range, and capability profile. The extension keys endpoint and credential
state to those values and blocks on unexpected identity change until the user
re-pairs; a reachable port is never sufficient identity.

## Pinned JSTorrent Survey And Adopted Lessons

The current sibling revision was inspected at:

- `android/README.md` and
  `android/app/src/main/java/com/jstorrent/app/mode/ModeDetector.kt` for the
  two-mode product and ARC selection;
- `android/app/src/main/java/com/jstorrent/app/MainActivity.kt` and
  `service/IoDaemonService.kt` for launch, foreground, background, wake,
  connection, idle, quit, and mode-exclusion behavior;
- `android/app/src/main/java/com/jstorrent/app/auth/TokenStore.kt` and
  `PairingApprovalActivity.kt` for token and explicit approval behavior;
- `android/app/src/main/java/com/jstorrent/app/storage/RootStore.kt` and
  `AddRootActivity.kt` for multiple persisted grants, URI deduplication,
  provider qualification, the translucent SAF picker, and root-change
  publication;
- `android/app/src/main/java/com/jstorrent/app/CompanionServerDepsImpl.kt` and
  `android/companion-server/src/main/java/com/jstorrent/companion/server/`
  `CompanionHttpServer.kt`, `NettyHttpServer.kt`, and
  `ControlWebSocketHandler.kt` for endpoint, raw-IO, session, and resource
  ownership;
- `extension/src/lib/chromeos-bootstrap.ts`, `daemon-bridge.ts`, and
  `extension/public/manifest.json` for the fixed ARC host, five-port probe,
  launch intent, token storage, reconnect, host permission, power hint, and
  media behavior; and
- `docs/archive/reports/2026-07-24-chromeos-fresh-install-exploratory-test.md`
  for the physical extension-to-Play-to-Android-to-pair-to-SAF-to-download
  journey and its two-stage Play, **Open with**, SAF, and pairing friction.

RSTorrent adopts the recognizable launch/pair/reconnect journey, fixed ARC
host with a bounded port list, explicit approval, foreground survival, clear
background notification, extension-triggered SAF picker, retained grant list,
root-change publication, and physical fresh-install evidence. Its application
database, rather than an extension-local engine preference, remains
authoritative for the current/default root and per-torrent bindings. It
rejects the extension-owned engine; raw socket, file, hash, KV, and media
endpoints; power hints from the extension; Wi-Fi locks; mutually exclusive
standalone/companion libraries; process-static pairing callbacks; and copying
legacy JS state into a second authority.

Chrome's current official extension guidance was also reviewed:

- <https://developer.chrome.com/docs/extensions/develop/concepts/network-requests>
  for explicit cross-origin host permission;
- <https://developer.chrome.com/docs/extensions/reference/api/permissions>
  for optional runtime permission and upgrade behavior; and
- <https://developer.chrome.com/blog/local-network-access> plus the Chrome 147
  beta note for current Local Network Access and WebSocket prompts.

The retained-root portion was also checked against pinned libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. Its
`include/libtorrent/add_torrent_params.hpp`, `src/session_handle.cpp`,
`src/write_resume_data.cpp`, `src/read_resume_data.cpp`, and
`test/test_session.cpp::async_add_torrent_no_save_path` retain and restore a
required save path per torrent. `test/test_storage.cpp`'s move-storage cases
reinforce that changing an existing torrent's destination is a distinct,
failure-bearing operation. RSTorrent adopts the retained per-torrent binding
and explicit missing-root failure, but not path locators or libtorrent's
storage architecture. This tactical changes no BitTorrent wire or piece
storage semantics: selecting a new Android current root affects future adds
and never invokes relocation for retained torrents.

## JSTorrent Android Parity Triage

This tactical does not redefine Android release readiness as complete
JSTorrent parity. The source comparison identifies the following missing
product capabilities in priority order:

| Priority | JSTorrent Android capability missing from RSTorrent | Release decision |
| --- | --- | --- |
| P0 | ChromeOS extension companion presentation and its extension-triggered retained SAF-root workflow | This tactical; required for the extension-controlled ChromeOS lane and migration workflow continuity |
| P1 | Completion and actionable failure notifications while Compose is absent | Next bounded Android release slice; core background-product behavior |
| P1 | Metered/unmetered enforcement and an explicit VPN-only policy with leak-safe Android network binding | Next bounded engine/platform slice; metered safety is required before a supported phone beta, while a VPN-only claim requires race and rebind evidence |
| P2 | Native progressive video playback and picture-in-picture | High-value product feature; RSTorrent has engine-side progressive reads but no Android presentation or reviewed ARC media exposure |
| P3 | Sandboxed search plugins | Product differentiation, not core downloader or migration safety |
| P3 | SOCKS5 routing, tracker mutation, companion auto-close policy, and broader advanced controls | Separate protocol/platform/policy tacticals; no control may appear before its behavior exists |

Search/plugins, playback, proxying, tracker editing, and the notification/
network-policy slices are not implementation scope here. Retained Android
multi-root management is part of the P0 companion workflow because the shared
extension root action must not destroy authority for existing downloads.
Recording them prevents smoke, validation, and the companion transport from
being mistaken for full Android feature readiness.

## Implementation Stages

1. **Pure contract gate:** define backend/capability identity, port selection,
   pairing generations, rate limits, exact Host/Origin admission, revocation,
   root-registry/default transitions, pending-picker ownership, and crash
   recovery without Android, sockets, or UI. Prove all hostile transition and
   bound cases.
2. **Retained SAF-root gate:** replace the singleton URI/fixed `downloads`
   configuration with the versioned bounded registry and root-specific broker
   lookup. Add application-service platform-root install/repair/removal and
   make-current operations without exposing platform locators. Prove two
   simultaneous roots, first/current selection, URI deduplication, future-add
   defaulting, retained per-torrent binding, independent grant loss/repair,
   reference/current removal rejection, exact grant release, singleton
   `downloads` migration, restart, and crash recovery at every journal
   boundary.
3. **Shared-owner gate:** refactor the Android Rust owner so Compose and a
   prepared gateway use exactly one application service. Prove simultaneous
   independent view sets, command convergence, shutdown ordering, and no
   second profile/engine in Rust tests.
4. **Android lifecycle gate:** add ChromeOS-only intent/listener supervision,
   durable pairing state, approval/revocation UI, extension-triggered SAF
   activity and notification fallback, truthful foreground notification, and
   process/activity recovery. Compose uses the same retained-root owner. Pass
   JVM/instrumented tests including select/cancel/busy/repair/remove, page
   disconnect during selection, phone inapplicability, and denied
   notification/foreground cases.
5. **Extension gate:** package the shared React assets under the existing beta
   identity, add optional exact ARC host permission and strict CSP, implement
   Android/Crostini backend selection, launch/pair/reconnect, and capability-
   gated root UI. Package validation rejects remote code, arbitrary hosts,
   leaked tokens or SAF locators, unreviewed assets, and permission drift.
6. **Controlled integration gate:** run one local/AVD semantic trace with
   Compose and extension adapters against the same application owner, plus
   wrong-origin/auth/bounds/crash/rotation tests and two controlled transfers:
   one remains bound to root A while root B becomes current for the second.
   Revoke and repair A without interrupting B.
7. **Physical ChromeOS gate:** before device work, read
   `~/code/machine-control/platforms/chromeos/skills/SKILL.md`, run the common
   Machine Control doctor, then execute the exact stopping-condition matrix on
   the named x86_64 Chromebook. Capture
   Chrome version, ARC topology, Local Network Access behavior, endpoint
   reachability from the Chromebook and another LAN device, process IDs,
   service notification/lifecycle, transfer result, resource high waters,
   and zero-residue cleanup.
8. **Closeout gate:** update the tactical with exact commands, commits,
   artifacts, high-water marks, failures, physical evidence, known gaps, and
   the selected next P1 Android parity tactical. Reconcile every owning topic
   before marking complete.

The proportional source baseline is:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
npm run build --prefix clients/web
npm test --prefix clients/extension
npm run package --prefix clients/extension
./gradlew --project-dir clients/android lintDebug testDebugUnitTest assembleDebug
```

Run the repository's generated-contract drift, deterministic browser,
instrumented Android, controlled interoperability, and ChromeOS package/device
commands documented by `DEVELOPMENT.md` in proportion to the landed paths.
No public-swarm result is required for this transport slice.

## Non-Goals

- Moving the engine, peer sockets, tracker/DHT work, hashing, scheduler, SAF
  descriptors, piece bytes, or SQLite into the extension.
- Recreating JSTorrent's IO-daemon protocol or preserving its ports, token,
  QuickJS database, extension KV store, root IDs, or mode-switch semantics.
- A LAN, tailnet, Internet, relay, third-party, or generic Android remote API.
- Serving React assets or media bytes from Android, progressive playback,
  complete-file extension open, arbitrary file upload/download, or Android
  filesystem browsing.
- Automatic one-action cold start, bypassing **Open with**, silent pairing,
  automatic root grants, or claiming the Android app can focus the extension.
- Importing JSTorrent torrents/settings/state, treating old payload as
  verified, sharing live state with Crostini, or silently changing the
  extension's selected backend.
- Freezing the Android package ID, signing an App Bundle, Play closed testing,
  Chrome Web Store upload/publication, changing the production JSTorrent
  extension, or taking over its store identity.
- Root relocation, automatic migration between roots, using retained old roots
  for new per-torrent overrides, cloud/removable provider expansion, or
  silent release of a current or referenced grant.
- Completion/error notifications, metered/VPN enforcement, search/plugins,
  proxying, tracker mutation, and native Android playback.

## Escalation Contract

Ordinary refactoring needed to share the one application owner, new pure
pairing/admission modules, Android app-private versioned records, generated
capability fields, extension build-mode wiring, strict permission/CSP changes,
auth adversarial tests, and physical device interaction described by the
stopping condition are in scope once implementation begins. Conservative
resource-bound tightening and same-boundary bug fixes do not require further
approval.

Stop for maintainer direction if evidence requires a LAN-reachable cleartext
listener, TLS or a new cryptographic dependency, a general remote-control
claim, more than the five fixed ports or four pairings/connections, a stable
public protocol promise, Android package-identity selection, production
extension modification, store/signing/release action, legacy-state import,
media byte delivery, or a different engine/profile ownership model.
