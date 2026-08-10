# Tactical 117: JSTorrent-Shaped Android Product UI

Status: Completed on 2026-08-10. The first-party Compose product, Android
application bridge, generated bindings, deterministic presentation tests,
both packaged ABIs, and controlled API 34 AVD evidence pass. No connected
physical device was used.

Topics: `client-surfaces`, `capability-readiness`, `product-direction`,
`application-control`, `application-view-api`, `android-saf-storage`,
`download-roots`, `product-surfaces-and-migration`,
`code-organization-and-refactoring`

Dependencies: Tactical `114` must first establish the durable multi-torrent
queue, truthful operational states, queue commands, and Android active-
download cap that the library presents. Tactical `116` must then settle root
health, SAF observation, publication/read coherence, complete-file access,
and Android lifecycle semantics so this tactical does not build presentation
around an adapter already scheduled for replacement. Completed Tacticals
[`008`](008-reactive-multi-surface-control.md),
[`009`](009-android-saf-session-storage.md),
[`012`](012-bounded-diagnostics-progress.md),
[`067`](067-dynamic-platform-file-acquisition.md),
[`081`](081-v1-torrent-byte-intake.md),
[`084`](084-persisted-client-connection-and-seeding-settings.md),
[`097`](097-live-client-settings-and-replaceable-session-generations.md),
[`108`](108-serialized-torrent-control-and-observable-checking.md), and
[`115`](115-mse-policy-advertisement-and-peer-detail.md) provide the existing
application, settings, view, platform-storage, and generated-client seams.

## Decision And Desired Outcome

Replace the Android bootstrap/diagnostic page with a first-party Compose
product whose information architecture, navigation rhythm, interaction
patterns, and Material 3 feel deliberately follow the JSTorrent Android
standalone application. Use the RSTorrent name and logo, and back every
enabled action and displayed fact with RSTorrent's typed application contract.

This is a single-back-stack phone and ChromeOS Android application, not a
bottom-navigation adaptation of the desktop UI:

```text
Library
  +-- Add magnet or .torrent
  +-- multi-select actions
  +-- Torrent detail
  |     +-- Details
  |     +-- Status
  |     +-- Files
  |     +-- Trackers
  |     +-- Peers
  |     `-- Pieces
  +-- Speed
  +-- DHT info
  +-- Logs
  `-- Settings
        +-- Storage
        +-- Speed & connection limits
        +-- Notifications
        +-- Network & privacy
        +-- Power management
        `-- Advanced
```

“UI-complete” in this tactical means that every useful capability currently
implemented by the RSTorrent Android application boundary is reachable and
truthfully presented, including loading, unavailable, stale, denied, error,
restart, and empty states. It does not mean engine parity with every JSTorrent
feature. Search plugins, playback, absent rate limiting, and other unsupported
engine or platform policies do not become real because a matching row can be
drawn.

The settings hub and its JSTorrent-shaped categories remain part of the
navigation structure. A setting is interactive only when the application or
Android platform owns a durable, observable value. Missing controls are
visibly disabled and labelled “Not available yet,” or omitted from a focused
subscreen when an empty page would be less clear. There are no inert toggles,
client-only engine settings, or optimistic success messages.

## Reference Comparison And Provenance

The primary product oracle is the sibling JSTorrent checkout at exact revision
`9895410beeed6aff554053769bd006a3fbd373ef`, licensed under MIT. Planning
inspection covered these exact sources:

- `android/app/src/main/java/com/jstorrent/app/NativeStandaloneActivity.kt`;
- `android/app/src/main/java/com/jstorrent/app/ui/navigation/Navigation.kt`;
- `ui/screens/TorrentListScreen.kt`, `TorrentDetailScreen.kt`,
  `SearchScreen.kt`, `SpeedHistoryScreen.kt`, `DhtInfoScreen.kt`, and
  `LogViewerScreen.kt` beneath the same Java package root;
- the settings hub plus Storage, Speed/Connection, Notifications, Network,
  Power, Advanced, and Search Plugin screens; and
- `ui/components/TorrentCard.kt`, `PlayPauseButton.kt`, and `PieceMap.kt`.

The current JSTorrent debug APK was also inspected on a clean API 34 AVD. It
confirmed the source-derived structure: notification pre-prompt and download-
folder setup, one library route with All/Active/Queued/Finished filters,
search/sort/overflow actions, a floating Add button, long-press selection,
one torrent-detail route with six horizontally swipeable tabs, and global
Speed, DHT, Logs, and Settings routes. The ordinary theme is Material 3 with
system light/dark and dynamic color; the logo and title supply the principal
brand identity.

This is a source and behavior reference, not an architecture donor. Do not
copy JSTorrent's QuickJS controller, string subscription topics, combined
mutable engine state, daemon topology, or ViewModel workarounds. If Compose
source is copied rather than independently adapted, record every copied file
and exact revision in this tactical's execution record, preserve the required
MIT copyright and permission notice coverage, and remove any unused legacy
state assumptions.

There is no protocol specification or pinned libtorrent survey for the
presentation-only portion of this tactical. If implementation reveals a new
engine behavior rather than an application projection or Android adapter gap,
stop that expansion and route it through the engine campaign contract.

## Stopping Condition

This tactical is complete only when all of the following hold:

1. The Android product opens into the JSTorrent-shaped Library rather than the
   bootstrap diagnostic column and uses one Material 3 system/dynamic-color
   theme with the RSTorrent name, icon, and launch/notification branding.
2. Setup, notification-permission explanation, persisted SAF-root health and
   repair, empty library, Add, duplicate, loading, and application-failure
   states have purposeful product presentation rather than diagnostic text.
3. The Library supports All/Active/Queued/Finished filtering, name/date-or-
   stable-order/download-rate sorting where authoritative data exists,
   pause/resume, queue movement, archive/restore where applicable, guarded
   keep/delete removal, long-press multi-selection, and truthful progress,
   rate, ETA, and operational-state cards for multiple torrents.
4. Add accepts both a pasted v1 magnet and one Android document-picker
   `.torrent` file through the ordinary bounded application operations,
   retains pending root/start choices safely across activity recreation, and
   never passes a filesystem path as portable identity.
5. Torrent detail uses the same six-tab order—Details, Status, Files,
   Trackers, Peers, Pieces—with back, swipe, compact pause/resume, recheck,
   queue, archive/restore, removal, and other enabled actions governed by
   authoritative capability fields. Every tab is live, bounded, and honest
   when metadata or a projection is unavailable.
6. Files provides bounded/lazy presentation and atomic Normal/Skip plus
   `Download now` behavior; Trackers is a truthful read-only list; Peers,
   Status, and Pieces use the current typed projections without client-side
   engine inference. Unsupported High priority and tracker mutation are not
   exposed as working actions.
7. Speed, DHT Info, and Logs retain JSTorrent's top-level navigation role and
   visual hierarchy but present RSTorrent-native information. Speed has no JS
   thread-health fiction; DHT distinguishes IPv4 and IPv6; Logs uses structured
   diagnostic severity/category/context plus explicit source/delivery/local
   loss.
8. Settings has the JSTorrent-shaped hub. Storage and every currently backed
   connection/seeding value are functional and reconcile configured,
   effective, applying, degraded, and rejected states. Android-owned
   notification/theme controls are functional where implemented. Missing
   bandwidth, VPN/metered, proxy, power, search-plugin, and similar controls
   are clearly unavailable rather than simulated.
9. One Android presentation repository owns application subscriptions and
   atomically reduces generated values into immutable `StateFlow` screen
   models. Screens and route ViewModels do not own the Rust application,
   descriptors, payload, raw patches, or independent copies of engine policy.
10. Activity recreation, process death, service reconnect, subscription reset,
    denied/revoked SAF access, notification denial, action replay, and shutdown
    all converge without duplicate commands, leaked handles, stuck pending
    UI, or fabricated success.
11. The existing experimental Android product is promoted to a maintained
    `clients/android` product boundary, or implementation records a stronger
    repository-layout reason not to move it. The retained engine/SAF evidence
    harness has one clear owner and does not duplicate product code.
12. Pure Kotlin, Compose, Gradle/ABI, generated-contract, controlled engine,
    no-window AVD, scale, lifecycle, accessibility, and screenshot-comparison
    evidence below passes, and owning topics/readiness rows describe the
    landed product and remaining engine gaps exactly.

## Screen Contract

| Screen or flow | JSTorrent shape to preserve | RSTorrent backing and deliberate delta |
| --- | --- | --- |
| Setup | Notification explanation, required download-folder card, direct recovery action | Android permission API plus Tactical `116` root health/repair; never claim a usable root before the capability is exercised |
| Library | Logo/title/live indicator, sort/search/overflow area, All/Active/Queued/Finished tabs, cards, Add FAB, long-press selection | `TorrentList` summary plus Tactical `114` operational state and queue position; RSTorrent logo; no working search action until a real search capability exists |
| Add | Magnet paste and `.torrent` browse, optional destination/start choices, duplicate feedback | Existing semantic magnet and byte intake; add a narrow Android raw-byte bridge rather than encoding bytes or passing a path through JSON |
| Torrent card | Play/pause, state, progress, ETA, rates, selected size and queue status | Current authoritative summary fields; do not sum visible peers or infer historic upload totals to fill missing JSTorrent labels |
| Detail shell | Back/title, compact play/pause, overflow actions, six swipeable tabs | Typed torrent identity and capability fields; action availability comes from the application, not state-name matching in Compose |
| Details | Identity, transfer and metainfo facts | Add a lightweight detail projection only for bounded facts the store/metainfo already owns; absent comment/creator/date facts remain unavailable |
| Status | Operational, transfer, storage, tracker and error state | Summary plus bounded Disk/Swarm facts selected for the torrent; no diagnostic-log scraping |
| Files | Hierarchy/list, completion and priority actions | Existing paged file view and Normal/Skip/Download-now commands; lazy/page loading is mandatory and High priority is absent |
| Trackers | Tiered tracker status | Existing paged tracker view; read-only because no semantic add/edit/remove tracker command exists |
| Peers | Live peer rows and protocol facts | Existing bounded peer view including exact encryption method when known; no durable peer-history claim |
| Pieces | Canvas/grid piece activity and legend | Existing bounded piece view, adapted from the JSTorrent visual rhythm without passing piece payload |
| Speed | Window selector, chart, current rates, supporting metrics | Session Speed ranges and counters; graph download, upload, and staged write first, with optional verified/wire/protocol/storage series instead of JS latency/queue health |
| DHT Info | Global DHT inspection route | Existing latest-value DHT view rendered as separate IPv4/IPv6 cards and exact degraded/unavailable states |
| Logs | Filterable global log route | Structured Diagnostics with bounded filter, context, loss, reset, and stale presentation—not a raw QuickJS console |
| Settings | Storage, Speed & Connection, Notifications, Network, Power, Advanced hub | Enable only durable application/platform values; preserve hidden secure tracker-trust policy; unsupported rows are disabled/labelled, not persisted locally as engine policy |
| Search | Search route in JSTorrent | Search plugins remain outside this tactical. Do not ship an empty or deceptive route. Adding search requires its own product/security tactical and explicit reversal of the current first-client non-goal |

The visual target is close family resemblance, not screenshot pixel identity
across Android versions. Preserve hierarchy, spacing rhythm, icon/action
placement, card density, transitions, gestures, typography roles, and
light/dark behavior. Permit RSTorrent branding, dynamic system colors,
accessibility corrections, larger-screen adaptation, and differences caused
by truthful RSTorrent data.

## Required Contract And Adapter Closure

The current application already owns most of the needed behavior. The
following gaps are part of this client tactical because they expose existing
capability rather than adding torrent-engine breadth:

- expose bounded `AddTorrentBytesRequest` through the Android UniFFI wrapper
  and read the selected document once under the existing 64 MiB application
  limit;
- adapt all existing Android-relevant views—Summary, Files, Trackers, Peers,
  Swarm, Piece Activity, Session Disk, Session Speed, Session DHT, and
  Diagnostics—rather than silently discarding them in the current reducer;
- expose every currently useful semantic action through typed Kotlin methods
  or the generic dispatcher, including recheck, queue movement, file
  selection, `Download now`, archive/restore, removal, settings, root actions,
  magnet export, and shutdown;
- add a small per-torrent detail projection for already-owned metadata and
  cumulative facts only where loading them into every library row would break
  the 500-row bound; and
- add an Android platform-capability action for opening a completed selected
  file/root with a safe content URI when Tactical `116` proves the underlying
  logical object readable. URI construction remains outside portable Rust
  state.

The following are candidate projection improvements, not permission to invent
facts: added/completed timestamps for date sorting, authoritative per-torrent
uploaded totals/rate/share ratio, total size and piece geometry, file/tracker
counts, private flag, and bounded source metadata. Implement only values with
an existing durable/runtime owner. A missing authoritative value produces a
deliberately reduced row, not a Kotlin approximation. If supplying one of
these values requires a new engine owner or persistence contract, record it as
a follow-up gap unless it is essential to a stopping condition above.

## Explicit Capability Gaps After The Prerequisites

The tactical must begin with a fresh audit after `114` and `116`, but the
current known boundary is:

| Gap | Treatment in this tactical |
| --- | --- |
| Search and search plugins | Omit the toolbar action and Search Plugin settings action, or show the latter as unavailable; requires a separate sandbox/network/security design |
| Upload/download rate limits | Show the settings category but no active rate controls; peer limit, upload slots, and active-download count remain functional |
| VPN-only, metered-network, proxy, and interface selection | Clearly unavailable; they require Android network-policy ownership and leak/race analysis |
| Notification preferences | Request/manage Android permission and channel where useful; completion policy beyond current behavior requires platform work and must not be implied |
| Power-management policy | Explain current foreground-service requirement; do not create a client-only wakelock or battery setting |
| High/low file priority and streaming urgency | Offer only Normal, Skip, and existing atomic Download now |
| Tracker mutation | Present the read-only tracker state; no client-owned edits |
| Playback or local HTTP streaming | Out of scope; opening a complete file through Android is the maximum file-launch behavior in this slice |
| Fast/trusting resume | Independent engine policy follow-up; current checking progress and Force recheck remain fully presentable |
| General cloud/removable multi-root equivalence | Do not widen Tactical `116`'s provider claim; present the roots and health semantics actually supported |

These gaps do not prevent UI completion when their rows are honest. A missing
Android bridge to an already implemented RSTorrent view or command does.

## Ownership, Dependency, And Lifecycle Map

```text
Compose screen / route ViewModel
             |
       immutable UI StateFlow
             |
   Android presentation repository
      | reduce snapshots/resets
      | dispatch typed commands
             |
 ProductEngineService (foreground owner)
      | application lifetime
      | SAF/platform capabilities
             |
 AndroidApplicationClient / UniFFI
             |
 Rust application service and engine
```

- `ProductEngineService` remains the sole owner of the in-process application,
  foreground notification, shutdown, SAF broker, and service-scope jobs. An
  Activity or NavHost never creates a second engine.
- One Android presentation repository owns subscription interest, reset and
  stale handling, command receipts, and atomic presentation reductions. Its
  state survives Activity recreation while the service lives and is rebuilt
  from fresh snapshots after service/process restart.
- The Library summary interest stays open for service/product lifetime because
  it also participates in current SAF orchestration. Torrent detail owns one
  selected summary plus only the visible tab's detailed projection. Speed,
  DHT, and Logs open global interests only while their routes need them.
- Route ViewModels may own text-field drafts, selected tab, dialogs, snackbar
  events, and one cancellable command attempt. They do not own descriptors,
  subscriptions, application tasks, or mutable copies of torrent truth.
- Kotlin platform adapters own Activity Result contracts, document-picker
  handles, notification permission/channel navigation, content URIs, and
  lifecycle-safe user intents. Portable Rust owns torrent identity, command
  serialization, bounds, and semantic outcomes.
- Every service, subscription, picker continuation, and command coroutine has
  an identifiable cancellation or terminal path. Recreated collectors cannot
  replay an Add, Remove, or settings mutation.

The dependency direction remains Compose -> presentation models -> generated
application client. Generated records may enter the repository reducer but do
not leak through every composable. Android framework values do not enter Rust
domain or application state.

## Resource And State Bounds

- Render a 500-torrent library through stable keys and lazy lists. Never open
  one subscription per card or retain piece/peer/file state for every row.
- Retain at most one active torrent-detail owner per visible navigation entry
  and one detailed-tab projection at a time. Tab changes close or replace the
  prior interest generation.
- Consume Files and Trackers using the application's bounded pages; do not
  concatenate an adversarial 374,998-file catalog into one immutable screen
  model. Visible-page cache size and eviction must be explicit and tested.
- Preserve the existing bounded Piece, Peer, Swarm, Speed, DHT, and Diagnostic
  projection limits. Compose animation state may not create a second
  unbounded history.
- Keep pending picker bytes outside saved-instance state and submit at most
  the application limit. Recreation either resumes from a safe document URI
  grant or asks the user to select again; it never serializes the torrent
  payload into a Bundle.
- Snackbar, dialog, and command-receipt queues are bounded and keyed by stable
  request identity. Latest state is not used as proof that a one-shot action
  ran.
- Test-only fixture/demo state is explicitly separated from production
  construction. Production never displays fixture facts as verified engine
  facts.

## Implementation Stages And Gates

1. **Post-prerequisite audit and product boundary.** Reconcile this document
   with landed `114`/`116` contracts, inventory the existing experimental app,
   and promote or prepare the maintained `clients/android` module without
   duplicating the engine/SAF harness. Gate: both ABIs and existing unit tests
   still build before presentation changes.
2. **Reference shell and deterministic fixture states.** Add the RSTorrent
   Material 3 theme, logo, single NavHost, top-level routes, reusable card/
   progress/action components, and test-only states for setup, empty, active,
   queued, finished, failed, and large catalogs. Gate: light/dark phone and
   large-screen screenshots plus navigation semantics run without an engine.
3. **Repository and library flows.** Replace the combined diagnostic reducer
   with typed immutable screen models; connect Library, setup/root repair,
   Add magnet/file, sort/filter, multi-select, pause/resume, queue, archive,
   and removal. Gate: reducer/replay/recreation tests and a controlled two-
   torrent `114` queue run pass.
4. **Torrent detail depth.** Connect the six tabs using per-route interest and
   bounded paging, then land detail actions and complete-file open behavior.
   Gate: every tab handles available, metadata-pending, reset, stale, error,
   and empty states; the large-file fixture stays within declared cache and
   rendering bounds.
5. **Global inspection and honest settings.** Connect Rust-native Speed, dual-
   family DHT, structured Logs, and the settings hub/subscreens. Gate: settings
   reconcile configured/effective/degraded/rejected values and every missing
   control is visibly unavailable rather than writable.
6. **Lifecycle and product closure.** Polish accessibility, adaptive width,
   system back, tab swipes, permission flows, notification branding, process
   death/reconnect, and shutdown; run the full validation matrix and update
   docs/evidence. Gate: the stopping condition passes as one installed Android
   product, not as disconnected previews.

Stages may land as reviewable commits inside one tactical. Split a new
tactical only if evidence reveals an independently owned engine, security,
persistence, or platform-policy change whose failure cannot be represented
honestly by this UI.

## Validation Matrix

### Pure Kotlin and Compose

- Reducer tests cover snapshots, deltas, resets, stale generations, missing
  selected torrents, paging replacement, command pending/result/replay,
  process reconstruction, and every named unsupported state.
- Formatter and filter tests cover unknown totals, metadata pending, checking,
  queued, active, seeding, paused, error, zero rates, long names, RTL text,
  large byte/rate values, and unavailable ETA.
- Compose tests cover all routes, system back, detail tab click/swipe, dialog
  restoration, long-press selection, minimum target sizes, semantic labels,
  focus order, font scaling, and light/dark/dynamic-color fallbacks.
- Deterministic screenshots compare the RSTorrent shell with the referenced
  JSTorrent hierarchy at representative phone and large-screen widths. They
  document intentional branding/data differences rather than imposing one
  device's pixels on every Android release.

### Generated boundary and platform build

- Regenerate Kotlin whenever the Rust application boundary changes and prove
  no handwritten DTO duplicates a generated semantic value.
- Run Gradle unit tests, lint, and `assembleDebug` for the maintained product;
  build and package both `x86_64` and `arm64-v8a` Rust libraries.
- Run the proportional Rust baseline and every affected application/view/
  Android binding test. Any `clients/web` contract change also runs generation,
  typecheck, and tests there even when the React UI is unchanged.

### Controlled runtime and AVD

- On an owned no-window API 34 AVD, cover first launch, notification denial
  and grant, root selection, root loss/repair, magnet and `.torrent` Add,
  duplicate feedback, two concurrent/queued torrents, pause/resume, queue
  movement, each detail tab, file selection/Download now, recheck, completion,
  complete-file open intent, archive/restore, keep/delete removal, settings,
  shutdown, and exact cleanup.
- Repeat the critical flow across Activity recreation and forced process death.
  Prove one foreground application owner, no duplicate mutation, fresh view
  recovery, and storage/descriptor/task return to the bounds inherited from
  `114` and `116`.
- Use a controlled pinned-libtorrent seed for content and metadata. Public-
  swarm access is unnecessary and remains opt-in.
- Exercise 500 library rows and the maximum recorded file-catalog fixture
  without per-row subscriptions, unbounded page retention, ANR, or payload
  crossing UniFFI. Record frame/render or macrobenchmark evidence sufficient
  to catch visibly unusable scrolling and tab changes; do not invent a release
  performance threshold without baseline evidence.

Visible desktop apps and connected physical devices were not authorized by
this tactical. Implementation used owned no-window emulators.
Pixel, Chromebook, or other physical evidence requires separate explicit
authorization and must name the device, actions, cleanup, and captured
artifacts before use.

## Completion Record

Tactical `117` replaces the launcher's diagnostic column with the maintained
RSTorrent Compose product while retaining the diagnostic harness as an
explicit test-only entry point. The implementation landed in five reviewable
commits: product plan, product shell, live product views, coverage hardening,
and lifecycle hardening.

The resulting Android product has:

- one Material 3, dynamic-color, system light/dark Library with RSTorrent
  launcher and notification branding, setup/repair states, authoritative
  All/Active/Queued/Finished filters, bounded stable sorting, Add, long-press
  selection, queue, archive, pause/resume, and guarded removal flows;
- one six-tab swipeable torrent detail destination in the required Details,
  Status, Files, Trackers, Peers, Pieces order, including live bounded catalog
  pages, file selection and `Download now`, complete-file content-URI launch,
  peer/swarm/piece/disk projections, and the applicable torrent actions;
- top-level RSTorrent-native Speed, separate IPv4/IPv6 DHT, structured Logs,
  and the JSTorrent-shaped Settings hierarchy. Backed settings expose their
  configured/effective/application truth; absent bandwidth, VPN/metered,
  proxy, power, playback, search/plugin, tracker-mutation, and richer file-
  priority features say `Not available yet` instead of becoming local policy;
- one service-scoped `AndroidPresentationRepository` that owns the list,
  visible detail, visible global, and diagnostic interests, atomically reduces
  bounded generated updates, resynchronizes resets and continuity faults, and
  makes rapid route changes last-request-wins; and
- a bounded generated `AddTorrentBytesRequest` bridge. The Activity Result
  flow retains only the persistable document URI and start choice across
  recreation, reads at most 64 MiB, never puts payload in saved state, and
  invokes the in-process application operation without path identity.

No JSTorrent Compose source was copied. The UI is independently authored from
the exact source/AVD behavior inspection recorded above, so no imported MIT
source files or additional attribution artifacts were created.

### Maintained module boundary

The product remains in `experiments/android-engine-bootstrap` rather than
moving to a new `clients/android` directory. That module is already the only
Android package, manifest, generated-UniFFI build, two-ABI build, foreground-
service owner, SAF adapter, controlled runner, and accumulated physical/AVD
evidence target. Moving it would either invalidate those stable evidence paths
or create a second Android packaging and lifecycle authority. Product code is
separated by `Product*`, `AndroidPresentationRepository`, and `ui/` sources;
the older diagnostic activity/service remains an explicitly named evidence
harness. The module README now records this as a maintained product boundary,
not as a disposable UI experiment.

### Evidence

The closing validation on 2026-08-10 passed:

- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace`, including the Rust Android raw-byte application
  intake test;
- `experiments/android-engine-bootstrap/build.sh`, which regenerated both
  UniFFI packages, cross-built and packaged `x86_64` and `arm64-v8a`, then ran
  the debug APK and Kotlin unit-test build;
- Gradle `lintDebug`, `testDebugUnitTest`, and
  `assembleDebugAndroidTest` after the clean two-ABI build;
- two Compose instrumentation tests on the owned no-window API 34
  `jstorrent-tablet` AVD at 2560x1600: first-launch Add/Settings hierarchy and
  injected active-torrent traversal of all six detail tabs plus Speed, DHT,
  Logs, and dark theme;
- manual phone and large-screen first-launch inspection for hierarchy,
  dynamic/system bar colors, setup state, and absence of runtime crashes; the
  captures were investigation artifacts and were not retained; and
- one fresh controlled `product-dynamic-saf` plus
  `product-concurrent-downloads` run. Dynamic SAF completed publication,
  restart/recheck, exact selective publication, upload, pause/removal, and
  cleanup with storage-owned high water `6`, pending high water `2`, and
  process-FD baseline/final/high `118/136/140`. The concurrent profile
  observed configured `3`, effective/active `2`, queued `1`, registered high
  `3`, terminal active/queued `0/0`, storage-owned high water `11`, and
  process-FD baseline/final/high `120/135/148`; exact cleanup passed.

The deterministic reducer suite includes 500-row filter/sort coverage and
bounded file-page replacement. Existing controlled Android profiles continue
to own root revocation/repair, Activity recreation, forced process restart,
and application-owner lifecycle evidence. This presentation slice did not
interact with the connected Pixel and makes no new physical-device UI claim.

### Remaining product and engine gaps

The Android UI side is complete for the currently exposed RSTorrent
capabilities. General cloud/removable root policy, bandwidth limits, Android
VPN/metered/proxy policy, power controls, search plugins, tracker mutation,
high file priority/streaming urgency, incomplete-file playback/HTTP serving,
and fast/trusting resume remain separate engine, platform, security, or
product tacticals. Their routes or rows are absent or explicitly unavailable;
they are not hidden partial implementations in this product.

## Non-Goals

- Exact JSTorrent engine, QuickJS, persistence, daemon, or plugin parity.
- Search plugin execution, arbitrary network-fetched code, or a placeholder
  Search result catalog.
- Incomplete-file playback, an embedded HTTP media server, streaming priority,
  or Android media-player integration.
- New torrent protocol breadth, peer policy, tracker mutation, rate limiting,
  VPN leak prevention, metered-network behavior, proxying, seed goals, or fast
  resume.
- General third-party/cloud SAF support, root migration, or provider claims
  beyond Tactical `116`.
- Pixel-identical rendering across OS versions, copying JSTorrent's ViewModels,
  or forcing Android to mirror the dense desktop React layout.
- A complete iOS UI, shared cross-platform UI framework, Android companion
  daemon, or HTTP/WebSocket control proxy.
- Release signing, store listing, analytics, crash reporting, remote control,
  or publication.

## Escalation And Next Boundary

Within an explicitly authorized implementation of this tactical, ordinary
Compose refactoring, adapting MIT-licensed reference components with recorded
provenance, adding generated projection fields for already-owned bounded facts,
promoting the Android module, tightening presentation caches, and fixing bugs
at the existing Android application boundary do not require repeated approval.

Stop for maintainer direction if completion requires a new engine owner,
durable persistence meaning, unsupported provider claim, security-sensitive
search/plugin execution, new third-party dependency with material tradeoffs,
physical-device interaction, externally visible publication, or a navigation/
product behavior that materially departs from the JSTorrent-shaped decision.

At closure, the next product boundary is chosen from measured gaps rather than
from dormant rows. Likely candidates are Android network/power policy, search
and plugin security, complete-file launch polish, or playback. Engine work
continues under its own authoritative queue; UI completion neither blocks nor
silently authorizes unrelated protocol breadth.
