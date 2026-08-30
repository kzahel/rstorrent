# Android JSTorrent Replacement Readiness

Topic: `android-jstorrent-replacement`

Status: **Active as of 2026-08-30.** This is the authoritative readiness and
feature-disposition ledger for eventually shipping the first-party Rust
Android product as a normal update to the current JSTorrent Android
application. It does not authorize a Google Play release, signing-key use,
production-extension publication, or implementation without bounded
tacticals.

The initial audit compares RSTorrent commit
`fe05d74a7f7aa9e508bb941dc758c8196a2c8864` with the local JSTorrent checkout
at `25e4b701433fd815398ba89526546f5e4f072e3f`. Re-audit both products before a
replacement candidate because this topic records a moving product boundary,
not permanent parity with that one revision.

## Scope And Ownership

This topic owns:

- the stronger readiness bar for updating installed `com.jstorrent.app`
  users, distinct from launching an independent RSTorrent beta;
- the inventory and explicit disposition of current JSTorrent Android
  capabilities;
- Android package, signing, Play, legacy-state, SAF-grant, and payload-safety
  handoff requirements;
- coordinated migration of the production ChromeOS extension/Android
  companion journey; and
- the ordered bounded tacticals and evidence required before replacement.

[`beta-release-readiness.md`](beta-release-readiness.md) continues to own the
independent RSTorrent beta lanes and their signed distribution gates.
[`product-surfaces-and-migration.md`](product-surfaces-and-migration.md) owns
the broader cross-product graduation direction.
[`client-surfaces.md`](client-surfaces.md) owns Android presentation and
lifecycle truth, while [`capability-readiness.md`](capability-readiness.md)
owns the cross-cutting capability scoreboard. Implementing tacticals own exact
contracts, limits, source findings, tests, and execution evidence.

This topic does not require a copy of JSTorrent's QuickJS engine, raw I/O
daemon, service split, or internal storage representation. It does not make
desktop or iOS graduation part of Android replacement. Engine-wide gaps enter
this ledger only when a current Android promise or ordinary replacement
journey depends on them.

## Product Outcome

The desired outcome is a normal Google Play update from a supported current
JSTorrent Android release to the Rust implementation, retaining JSTorrent
branding and the installed application audience. The replacement must:

- retain `com.jstorrent.app`, Play signing continuity, and a monotonic
  `versionCode` when shipped through the existing listing;
- preserve external payload unless the user explicitly requests deletion;
- migrate useful torrent intent, settings, roots, and pairing state only where
  a bounded importer can do so safely, and otherwise use a truthful reset or
  reauthorization flow;
- never convert legacy completion or resume claims into verified content
  without the Rust engine's ordinary integrity checks;
- keep the standalone Android and supported ChromeOS extension journeys
  usable across the rollout; and
- either implement, deliberately retire, or clearly disclose every current
  user-visible JSTorrent capability before the candidate is approved.

The Kotlin namespace and internal class names need not equal the application
ID. Public identifiers, deep links, notification channels, provider
authorities, extension metadata, and store-facing names must nevertheless be
derived from one reviewed production identity rather than today's provisional
`org.rstorrent.bootstrap` values.

## Definition Of Replacement Ready

Android replacement is ready only when:

1. every open **Required** gate below is complete with its proportional
   deterministic, emulator, physical-device, and signed-update evidence;
2. every **Disposition required** capability has an accepted implement,
   retire, or defer-and-disclose decision, with implementation evidence where
   selected;
3. a production-equivalent old JSTorrent fixture upgrades through the actual
   signed Play lane without payload loss, false verified state, or an unusable
   standalone/ChromeOS journey;
4. fresh install, upgrade, interrupted migration, missing/revoked root,
   process death, reboot, uninstall, and explicit data deletion have bounded
   outcomes; and
5. store declarations, privacy/support text, screenshots, permissions, and
   foreground-service behavior describe the candidate exactly.

Feature-for-feature identity is not required. Unclassified regressions are
not acceptable: every deliberate difference must have an owner and a visible
product decision.

## Current Implemented Baseline

The Rust Android product already provides the hard architectural foundation:

- one in-process Rust application service and engine with a generated UniFFI
  boundary and one durable profile owner;
- the maintained Material 3 Library, torrent detail, Files, Peers, Trackers,
  Pieces, Swarm, Disk, Speed, DHT, Logs, and Settings presentations;
- in-application magnet and local `.torrent` intake, queue and torrent
  actions, High/Normal/Skip file intent, completed-file open, and guarded
  keep/delete removal;
- retained multi-root SAF selection, current/default future binding,
  grant-loss repair, direct storage, restart/recheck, upload, and exact
  cleanup;
- peer, upload-slot, active-download, listener, UPnP, IPv6, encryption, and
  session/per-torrent transfer-limit settings;
- activity/process recovery and a foreground service; and
- controlled Android/ChromeOS transfer evidence across both packaged ABIs.

Completed Tactical
[`165`](../tactical/165-cross-platform-active-download-sleep-inhibition.md)
adds default-on active-work sleep inhibition with one service-owned partial
CPU wake lock. It deliberately removes JSTorrent's Wi-Fi lock because the
deprecated Android mode no longer supplies a truthful screen-off guarantee.

Completed Tactical
[`194`](../tactical/194-chromeos-android-extension-control.md) implements and
physically proves the selected same-device companion architecture, retained
SAF-root workflow, fixed ARC-address listener, and same-LAN refusal. It does
not publish or migrate the production JSTorrent extension, import legacy
pairing/profile state, sign a Play release, or authorize the old raw I/O
daemon architecture.

## Required Replacement Gates

- [x] **JAR-001 — Maintain a first-party Android product.** The Compose,
  in-process Rust, generated-contract, SAF, foreground-service, dual-ABI, AVD,
  physical Android, and physical ChromeOS foundations exist.
- [x] **JAR-002 — Use truthful active-work sleep inhibition.** Tactical `165`
  holds only a partial CPU wake lock for authoritative Starting, Downloading,
  and Checking work, releases it on every nonqualifying/reset/shutdown path,
  and does not restore the deprecated Wi-Fi lock.
- [x] **JAR-003 — Prove the replacement companion architecture.** Tactical
  `194` proves one Android engine/profile owner with Compose and packaged React
  clients, explicit pairing, fixed same-device reachability, retained roots,
  detached transfer, restart, revocation, and cleanup.
- [ ] **JAR-004 — Freeze the production application and migration contract.**
  Inventory the then-current released JSTorrent package, schema, private
  files, SharedPreferences, persisted URI grants, public identifiers,
  `versionCode`, signing path, backup behavior, and extension pairing state.
  Select exact import, reset, reauthorization, recheck, interruption, and
  failure behavior. The current `org.rstorrent.bootstrap` application ID and
  RSTorrent branding are not replacement values.
- [ ] **JAR-005 — Coordinate the production extension rollout.** The existing
  JSTorrent extension expects the legacy raw I/O companion, while Tactical
  `194` intentionally selects a typed semantic connection to the Rust
  application owner. Choose and prove an extension-first, app-first-compatible,
  or coordinated rollout that leaves no installed cohort stranded. Do not add
  a permanent raw I/O compatibility daemon merely to avoid rollout planning.
- [ ] **JAR-006 — Add external Android torrent intake.** Register and bound
  `magnet:`, `application/x-bittorrent`, and supported `content://` `.torrent`
  delivery through one exported intake owner. Reuse the application add flow
  for cold, warm, duplicate, malformed, oversized, canceled, and storage-root
  cases; do not create a second engine or profile owner. Ready Tactical
  [`197`](../tactical/197-android-external-torrent-intake.md) owns the exact
  filters, ephemeral intake state, hostile-provider bounds, and AVD evidence.
- [ ] **JAR-007 — Add background completion and actionable failure
  notifications.** This is also beta gate `AND-009`. Implement one native
  edge owner for completion plus fatal/storage-repair attention with
  permission denial, initial/reset suppression, duplicate avoidance,
  tap/action routing, restart, and cleanup. Decide whether the current
  JSTorrent Pause All/Resume All/Quit foreground actions are retained.
- [ ] **JAR-008 — Enforce unmetered-network policy live.** This is the required
  part of beta gate `AND-010`. Present it as **Unmetered networks only**, not
  literal Wi-Fi-only behavior. Preserve each torrent's user intent while
  connectivity changes, fail closed while eligibility is unknown, and cover
  initial startup, Wi-Fi/cellular/Ethernet changes, metered Wi-Fi, VPN
  overlays, process recovery, and every tracker/DHT/peer transport owner.
- [ ] **JAR-009 — Select and implement background lifecycle policy.** Current
  JSTorrent defaults background downloads off, requires notification
  permission to opt in, shuts the engine down when background work is not
  allowed or idle, and separately chooses stop/close versus continued
  seeding. RSTorrent currently starts one sticky foreground service until
  explicit stop. Select truthful foreground/background/idle/seeding behavior,
  task-removal and reboot policy, notification-permission failure, and
  cancellation/join ownership before replacement.
- [ ] **JAR-010 — Qualify the signed Play replacement.** Produce and inspect
  the protected-key release AAB, remove or deliberately retain diagnostic
  components, close current Android API deprecations, complete store/privacy/
  foreground-service declarations, and pass fresh plus signed-upgrade cohorts
  on representative phone and ChromeOS devices. Publication remains a
  separately authorized external operation.

## Capabilities Requiring An Explicit Disposition

These are current JSTorrent behaviors or controls that RSTorrent Android does
not yet match. Each needs an accepted implement, retire, or defer-and-disclose
decision before a replacement candidate. Their absence is not automatically a
blocker for an independent RSTorrent beta.

| Capability | Current comparison | Required disposition |
| --- | --- | --- |
| VPN-only mode | JSTorrent suspends when the active default network is not reported as VPN. RSTorrent has no control. The legacy check does not prove socket binding or leak prevention. | Implement only with Android `Network` binding, fail-closed startup/handover, closure or rebinding of existing TCP/UDP sockets, and peer/tracker/DHT/DNS leakage evidence; otherwise retire and disclose it. |
| SOCKS5 proxy | JSTorrent exposes host, port, optional credentials, and peer/HTTP-tracker/UDP-tracker routing choices. RSTorrent shows a disabled placeholder and has no engine proxy owner. | Use a source-first engine tactical covering DNS, authentication-secret storage, UDP ASSOCIATE or a truthful unsupported state, reconnect, bypass prevention, resource limits, and interoperability. |
| DHT and PEX controls | Both RSTorrent engine capabilities exist, but Compose has no enable/disable controls. | Add backed settings or explicitly retain always-on public-torrent policy. Preserve private-torrent gating regardless. |
| Seeding and queue policy | JSTorrent has an active-seed limit and stop/close versus keep-seeding choice. RSTorrent exposes active-download admission but not equivalent seed goals/policy. | Resolve with `JAR-009` or a separate seeding-policy tactical; do not imply ratio/time goals that do not exist. |
| Low-battery shutdown | JSTorrent offers an opt-in 5–50% threshold. RSTorrent has only the active-work sleep setting and a disabled Battery policy row. | Decide whether Android replacement retains this safety valve. If implemented, define charging, threshold hysteresis, notification, intent preservation, and restart behavior. |
| Companion idle/auto-close | JSTorrent can stop the companion after a configured disconnected interval. Tactical `194` owns a different one-service architecture without this user control. | Decide from the selected production extension and background lifecycle rather than porting the old daemon timer literally. |
| Search and plugins | JSTorrent has search UI plus installed/recommended URL-fetched JavaScript plugins in an Android WebView sandbox. RSTorrent has no search/plugin product capability. | Treat as a separate security and product campaign. Implement only with explicit network-code trust, sandbox, update, disclosure, and Play-review policy; otherwise retire/defer visibly. |
| Native/progressive playback | JSTorrent has Media3 playback for local and incomplete content, subtitles, streaming-aware activity ownership, and picture-in-picture. RSTorrent has completed-file Android open and engine/shared-web streaming foundations, but no native Android playback presentation. | Separate native presentation/lifecycle tactical. Do not equate engine byte availability with qualified Android playback. |
| Localization | JSTorrent currently ships system/app locale selection and numerous translated `values-*` resources. RSTorrent Compose strings are predominantly inline English. | Select the replacement locale set and translation/update workflow; record any reduced first-release set in the listing and release notes. |
| Reset, clear data, and support | JSTorrent exposes reset settings, clear all data with optional payload deletion, and a prefilled report-bug path. RSTorrent shows Reset engine settings as unavailable. | Add safe, separately worded metadata reset and payload deletion operations plus support/diagnostic handoff, or explicitly narrow them. Never combine payload deletion with an implicit migration reset. |
| Add-time file selection | JSTorrent can show a file-selection step during add. RSTorrent selects a root during Add and changes High/Normal/Skip intent from Files after addition. | Decide whether the existing RSTorrent flow is the accepted replacement behavior or whether pre-start selection is required. Preserve bounded BEP 53 selection intent. |
| Download manifest integration | JSTorrent can write a sidecar manifest for external playback integration. RSTorrent does not. | Confirm whether any supported integration consumes it; implement a safe final-path equivalent or retire it. |
| Active-piece memory override | JSTorrent exposes an Android memory-budget override. RSTorrent uses bounded engine-owned resource policy without an equivalent user control. | Prefer measured automatic limits unless physical evidence justifies an advanced setting. Record this as a deliberate difference. |
| Tracker mutation | RSTorrent Android can inspect trackers but not mutate them. | Defer unless current user journeys or ordinary interoperability require it; use a typed application command if implemented. |
| HTTP web seeds and other protocol breadth | Current JSTorrent implements web-seed behavior; RSTorrent's beta boundary still lists BEP 17/19 and several optional discovery extensions as absent. | Keep in the engine capability campaign. Promote only when replacement evidence shows an ordinary advertised journey depends on it. |

Theme selection, dynamic Android colors, ordinary peer encryption, DHT
inspection, UPnP status, IPv6, transfer limits, queue actions, file priorities,
and completed-file external open already have RSTorrent equivalents. Exact UI
layout parity is not required where the replacement preserves the underlying
user outcome truthfully.

## Network And Privacy Decisions

Unmetered policy, VPN-only policy, and proxying are three different contracts:

- **Unmetered-only** is a cost policy over Android network capabilities. It is
  required before supported phone replacement and should count any eligible
  unmetered transport, not merely Wi-Fi.
- **VPN-only** is a privacy boundary. Observing that Android's active default
  network has `TRANSPORT_VPN` and suspending the engine is not sufficient.
  Every newly created and already-open TCP/UDP socket, resolver route, tracker,
  DHT transaction, and peer path must be bound, canceled, or replaced without
  a handover leak.
- **SOCKS5 proxy** is an engine routing feature. It needs explicit coverage of
  peers, HTTP trackers, UDP trackers, name resolution, credentials, fallback,
  and unsupported combinations. Android UI alone cannot implement it.

The three may share Android connectivity observations and product
presentation, but they must not share one ambiguous boolean or a whole-engine
pause/resume shortcut that overwrites torrent intent.

## Background And Power Decisions

Sleep inhibition and background continuation are independent. Tactical `165`
answers whether active work may keep the CPU awake after the product has
already decided it is allowed to run. `JAR-009` must still answer whether work
is allowed to continue when Compose leaves the foreground, when an active
download completes, while only seeding remains, after task removal, after
process recovery, and after reboot.

The replacement policy must identify one owner for:

- activity visibility and user-requested background permission;
- foreground-service start/stop and notification-permission failure;
- active-download, checking, playback, and seeding reasons to remain alive;
- idle shutdown and any low-battery stop;
- wake-lock acquisition/release; and
- joined application shutdown with observable termination.

Do not restore JSTorrent's Wi-Fi lock. Do not keep the engine alive solely to
preserve a UI cache. Disabling background work or hitting a battery policy
must preserve torrent intent so foreground return can recover predictably.

## Production Handoff And Legacy State

`JAR-004` must inspect the then-current production artifact rather than infer
migration from source types alone. At minimum it classifies:

- application-private databases, preferences, files, caches, and version
  markers;
- retained `content://` tree grants and the root identity users see;
- torrent sources, trackers, file-selection intent, queue/pause intent,
  completion claims, and settings worth importing;
- external payload locations and any sidecar/part/manifest artifacts;
- companion pairing credentials and the installed production extension's
  protocol/version expectations; and
- notification channels, deep links, provider authorities, backup/restore,
  and public component names whose behavior survives an Android update.

The default safe direction is to import compact user intent, not runtime
authority. Legacy verified/completed bits, peer state, in-flight writes,
credentials of uncertain provenance, and ambiguous roots fail closed or
require reauthorization. Existing payload remains untouched and becomes
verified only through the ordinary checker. Migration failure must not fall
through to deleting payload or silently starting unrestricted networking.

The tactical must select whether an old app can be relaunched after an
interrupted or failed rollout, and ensure schema mutation does not create a
half-readable profile. A staged import or versioned cutover needs an explicit
commit point and repeatable crash cases.

## Production Extension Rollout

Tactical `194` proves the new companion implementation but deliberately
excludes the production JSTorrent extension. Replacement therefore needs a
coordinated rollout contract:

1. identify every supported installed extension/app version pair;
2. select which side can understand both the old and new launch/pairing state
   during the transition, or require an explicit paired update;
3. update production extension permissions, Android package/deep-link
   metadata, connection versioning, and recovery presentation;
4. prove extension-first, app-first, stale-extension, revoked-pairing,
   offline-update, and clean-install outcomes; and
5. retain Tactical `194`'s fixed ARC endpoint, origin/Host checks, explicit
   approval, token rotation/revocation, and same-LAN refusal.

The rollout must not reopen a wildcard LAN listener or copy the legacy raw I/O
daemon into the Rust product. A temporarily incompatible pair must fail with
an actionable update path rather than silently hanging or exposing storage.

## Source Audit

### RSTorrent

The initial audit used:

- `clients/android/app/build.gradle.kts`: provisional application ID and
  version;
- `clients/android/app/src/main/AndroidManifest.xml`: launcher, companion deep
  link, services, permissions, and absence of external torrent filters;
- `clients/android/app/src/main/java/org/rstorrent/bootstrap/ProductEngineService.kt`:
  one sticky foreground-service/application owner, fixed `Online` startup
  policy, partial wake lock, and Stop-only notification;
- `clients/android/app/src/main/java/org/rstorrent/bootstrap/ui/ProductApp.kt`:
  current Notifications, Power, Advanced, and unavailable rows; and
- `clients/android/app/src/main/java/org/rstorrent/bootstrap/ui/ProductSettingsScreens.kt`:
  backed network settings plus disabled VPN, metered, and proxy rows.

The implemented/evidence baseline comes from Tacticals `117`, `165`, `191`,
and `194` plus the Android rows in `client-surfaces` and
`capability-readiness`.

### JSTorrent Android

The comparison inspected these paths at the revision recorded above:

- `android/app/build.gradle.kts` and `android/app/src/main/AndroidManifest.xml`:
  production ID/version shape, public components, magnet, MIME, and file
  intake;
- `android/app/src/main/java/com/jstorrent/app/LinkHandlerActivity.kt`:
  standalone/companion routing and external source handling;
- `android/app/src/main/java/com/jstorrent/app/settings/SettingsStore.kt`:
  Android-only network, background, power, add, completion, companion-idle,
  locale, and appearance preferences;
- `android/app/src/main/java/com/jstorrent/app/network/{NetworkMonitor,NetworkRestrictionEnforcer}.kt`:
  unmetered/VPN observations and whole-engine suspension behavior;
- `android/app/src/main/java/com/jstorrent/app/notification/{ForegroundNotificationManager,TorrentNotificationManager}.kt`:
  foreground status/actions plus completion and error attention;
- `android/app/src/main/java/com/jstorrent/app/service/{ServiceLifecycleManager,ForegroundNotificationService}.kt`:
  foreground/background/idle ownership, wake locks, and low-battery handling;
- `android/app/src/main/java/com/jstorrent/app/ui/screens/{NetworkSettingsScreen,PowerManagementSettingsScreen,AdvancedSettingsScreen,StorageSettingsScreen,SpeedConnectionLimitsSettingsScreen}.kt`:
  proxy, DHT/PEX, power, localization, reset/support, file-selection, seeding,
  and memory controls;
- `android/app/src/main/java/com/jstorrent/app/ui/screens/{SearchScreen,SearchPluginSettingsScreen}.kt`
  and `android/app/src/main/java/com/jstorrent/app/search/`: search/plugin
  product and sandbox boundaries; and
- `android/app/src/main/java/com/jstorrent/app/player/PlayerActivity.kt`:
  local/progressive Media3 playback, subtitles, and picture-in-picture.

These sources are behavior and edge-case references, not architecture
templates or source donors. The replacement retains RSTorrent's Rust engine,
one application owner, generated boundary, bounded resources, and ordinary
integrity rules.

## Validation Program

Each tactical defines proportional gates. The integrated replacement cohort
eventually includes:

- deterministic reducers for notification edges, network prerequisites,
  background reasons, migration commit points, and extension-version pairing;
- instrumented AVD cases for external intents, permission denial, metered and
  unmetered transitions, process death, task removal, reboot, migration crash,
  revoked SAF grants, and exact cleanup;
- physical current-API phone screen-off/Doze, Wi-Fi/cellular/metered-handoff,
  notification, completed/error, playback if selected, and battery-policy
  evidence;
- physical ChromeOS Compose and production-extension cold/warm/reconnect,
  app-first/extension-first update, retained-root repair, detached transfer,
  fixed-ARC reachability, and same-LAN refusal;
- signed AAB fresh installation and upgrade from a production-equivalent
  `com.jstorrent.app` fixture with the real signature/update path; and
- payload hashes, root/grant observations, profile inspection, process/task
  residue, descriptor/resource high-water marks, and network capture where a
  privacy claim is involved.

Live public swarms remain opt-in. Controlled fixtures and pinned-libtorrent
interoperability provide ordinary transfer evidence before any representative
live run.

## Recommended Next Work

1. Create the bounded `JAR-004` production handoff and legacy-state tactical.
   It fixes the candidate identity, migration/reset boundary, payload safety,
   signing inputs, and exact old-version fixture before code changes make
   accidental compatibility promises.
2. Execute ready Tactical
   [`197`](../tactical/197-android-external-torrent-intake.md) for `JAR-006`
   external Android torrent intake. It is small, user-visible, and independent
   of the larger policy work.
3. Implement `JAR-007` completion/failure notifications by adapting the
   already-proven desktop edge semantics to one Android-native owner.
4. Implement the unmetered portion of `JAR-008` without coupling it to a VPN
   privacy claim.
5. Select `JAR-009` background/seeding/idle policy, then reconcile low-battery
   and companion-idle controls against that owner.
6. Design `JAR-005` with the production extension before either store update
   is scheduled.
7. Decide VPN, proxy, DHT/PEX controls, search/plugins, playback,
   localization, reset/support, and the remaining table rows individually.
   Proxy and any engine/network privacy work follow the source-first engine
   campaign; search/plugin and playback remain separate security/lifecycle
   campaigns.
8. Run `JAR-010` only after the required gates and disposition ledger converge.
   Signing, store upload, staged rollout, production extension publication,
   and release promotion each remain explicitly authorized operations.
