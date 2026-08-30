# Tactical 198: Android Completion And Attention Notifications

Status: **Ready as of 2026-08-30.** Maintainer direction selected
JSTorrent-like notification transparency: Android notification permission is
not a technical prerequisite for starting a foreground service, but RSTorrent
will not retain an invisible long-running Android application or ChromeOS
companion owner after the visible interaction ends. No implementation or
release action has occurred yet.

Topics: `android-jstorrent-replacement`, `beta-release-readiness`,
`client-surfaces`, `capability-readiness`

Dependencies: completed Android product Tactical
[`117`](117-jstorrent-shaped-android-product-ui.md), completed desktop
notification Tactical
[`164`](164-desktop-completion-and-attention-notifications.md), completed
active-work sleep Tactical
[`165`](165-cross-platform-active-download-sleep-inhibition.md), completed
ChromeOS companion Tactical
[`194`](194-chromeos-android-extension-control.md), the maintained in-process
`ApplicationService`, and the existing Android torrent-list presentation
owner.

Tactical `164` supplies already-proven edge semantics and privacy vocabulary,
not a Tauri architecture to copy. Android retains an independent native
delivery, permission, channel, intent, and service-lifecycle owner. Ready
external-intake Tactical [`197`](197-android-external-torrent-intake.md) may
execute independently; neither tactical changes the other's exported-input or
notification contracts.

## Decision And Desired Outcome

Add native Android notifications for a download that genuinely completes and
a torrent that newly enters a fatal or storage-repair condition. The one
`ProductEngineService` owner derives edges from its authoritative torrent-list
stream and delivers them through Android. Compose and the ChromeOS extension
neither manufacture events nor receive a general notification command.

Use three Android notification categories:

1. **Background activity** is the existing low-importance, silent, ongoing
   foreground-service status. It is mandatory whenever the service is
   foreground and has no app toggle.
2. **Downloads completed** is a default-importance event category controlled
   by a default-on Android application preference and the corresponding system
   channel.
3. **Action required** is a high-importance category for newly fatal or
   storage-repair torrent edges and explicit pending Android workflows such as
   the existing companion root-picker fallback. A default-on Android
   application preference controls automatic torrent attention edges, not an
   explicit user-requested workflow already waiting for action.

Notification permission and service eligibility remain separate facts.
Android 13 and newer allow an app to launch a foreground service without
`POST_NOTIFICATIONS`, but hide that service notification from the notification
drawer when permission is denied. RSTorrent adopts JSTorrent-like product
transparency on top of that platform rule:

- visible Compose use and bounded app-owned activity-result handoffs remain
  available without notification permission;
- while permission or the Background activity channel is blocked, losing the
  last visible-interaction lease cooperatively shuts down the one Android
  application owner and its optional companion listener;
- a ChromeOS companion launch may pair and operate while Android remains
  visible, but switching back to Chrome without notification visibility closes
  the Android owner and the extension observes an ordinary disconnect;
- granting permission does not itself promise perpetual background execution;
  current granted behavior remains until background lifecycle Tactical
  `JAR-009` selects active, idle, seeding, task-removal, reboot, and restart
  policy; and
- Android 15's six-hour-per-24-hour `dataSync` foreground-service limit remains
  real. This tactical adds prompt timeout shutdown so the current target-35
  service cannot ANR, but makes no indefinite companion or download claim.

The Android Settings surface exposes **Download completed** and **Needs
attention** preferences plus permission state and **Manage system notification
settings**. There is no **Notify while visible** setting in this slice:
completion and attention edges remain eligible whether Compose or the Chrome
presentation is visible. There is also no JSTorrent extension-shaped
**background tab progress** setting; Android's ongoing foreground-service
notification owns native status independently of Chrome tab visibility.

The existing foreground action remains **Stop**. Pause All and Resume All are
useful possible follow-up actions, but adding their multi-torrent command,
partial-failure, companion, and intent-preservation semantics is not necessary
to close `JAR-007` or `AND-009`.

## Scope And Stopping Condition

This tactical owns:

1. a pure bounded Kotlin notification-edge reducer over authoritative
   torrent-list snapshots, patches, removals, and reset boundaries;
2. one service-lifetime Android notification coordinator that receives exact
   list snapshot/patch/reset facts from the existing presentation repository,
   owns policy state, preferences, channels, delivery, cancellation, and
   observable termination;
3. completion and fatal/storage-repair edge semantics matching Tactical
   `164`, including initial/restart/reset/recheck suppression, genuine download
   arming, duplicate avoidance, recovery rearming, removal, and cleanup;
4. installation-local default-on **Download completed** and **Needs
   attention** preferences, persisted before live state changes without
   altering the Rust application profile or generated contract;
5. separate low/default/high Android channels with truthful names,
   descriptions, importance, sounds, badge behavior, and system-settings
   routing;
6. bounded privacy-preserving notification titles, bodies, opaque identities,
   active counts, and diagnostics;
7. exact cold and warm notification tap routing to a torrent detail, exact
   storage repair, or the existing pending companion root workflow;
8. JSTorrent-like denied/revoked-permission transparency for standalone and
   ChromeOS companion use, including app/channel block observation, visible
   interaction leases, and joined service/listener shutdown;
9. generic ongoing status text derived from authoritative state without raw
   service errors, paths, source URIs, hashes, tracker/peer endpoints, pairing
   identifiers, or extension details;
10. prompt API-35 `dataSync` foreground-service timeout handling that cancels
    and joins the application and companion owners before stopping rather than
    crashing or immediately restarting into exhausted quota; and
11. deterministic Kotlin/Compose coverage, merged-manifest and channel
    inspection, both Android ABI builds, installed API 34 and API 35 AVD
    notification campaigns, and a bounded physical ChromeOS companion
    permission campaign.

The tactical stops when:

- one controlled incomplete-to-complete transfer produces exactly one Android
  completion notification and one controlled newly entered fatal or
  storage-repair state produces exactly one attention notification;
- initial complete/error/repair rows, process restart, subscription reset,
  settings enablement, repeated rows, and recheck do not replay either event;
- taps route cold and warm to the exact torrent or repair workflow without
  creating a second application owner;
- app preferences and Android channel blocking independently suppress only
  their owned automatic category without replay after re-enablement;
- denial or blocking of Background activity allows visible product use but
  closes the service and companion after the visible-interaction lease ends,
  while grant permits the current explicitly provisional background behavior;
- a shortened API-35 `dataSync` timeout reaches joined shutdown with no ANR,
  sticky restart, listener, wake lock, subscription, or coroutine residue;
- the physical Chromebook proves that permission denial keeps the Android
  activity visible for a usable companion session, returning to Chrome
  disconnects truthfully, and a later grant permits the current foreground
  notification plus reconnect; and
- force-stop/uninstall and testbed cleanup leave no RSTorrent notification,
  pending intent, permission mutation, process, listener, wake lock, or test
  artifact.

Passing this tactical closes notification gate `AND-009` and replacement gate
`JAR-007`. It does not close `JAR-009` or qualify indefinite Android background
operation.

## Non-Goals

- Selecting the complete granted-background policy for active downloads,
  checking, idle, seeding, playback, task removal, reboot, force-stop, battery
  thresholds, or companion auto-close. `JAR-009` owns that state machine.
- Claiming a way around Android 15's `dataSync` quota, selecting another
  foreground-service type, introducing WorkManager or user-initiated jobs, or
  making an indefinite companion availability promise.
- Progress bars, percentage/ETA updates, notification grouping summaries,
  badges beyond normal channel behavior, history, scheduled alerts, or
  per-torrent notification preferences.
- Start, pause, resume, queue, tracker, peer, DHT, duplicate-add, network,
  metered, VPN, proxy, update, low-battery, seeding-goal, or playback
  notifications.
- Pause All, Resume All, Open Folder, direct file-open, or inline repair action
  buttons. Completion opens the exact torrent, from which existing verified
  file actions remain available.
- Letting the Chrome extension or shared React presentation post Android
  notifications, suppress Android events based on Chrome tab visibility, or
  adding Chrome notification permission to the companion contract.
- A generic application-event bus, durable notification ledger, delivery
  acknowledgement, retry queue, analytics, crash reporting, or product
  counter.
- Changing engine, torrent, storage, duplicate, application command, profile,
  UniFFI, or generated web/Android contract semantics.
- Production JSTorrent branding, final notification-channel IDs, application
  ID migration, signing, Play foreground-service declarations, extension
  publication, or store rollout. `JAR-004`, `JAR-005`, and `JAR-010` retain
  those owners.
- iOS or desktop notification changes. Desktop Tactical `164` remains a
  complete independent platform record.

## Current RSTorrent Findings

The implementation begins with useful owners and several gaps:

- `clients/android/app/src/main/AndroidManifest.xml` already declares
  `POST_NOTIFICATIONS`, `FOREGROUND_SERVICE`, and
  `FOREGROUND_SERVICE_DATA_SYNC`; both product and diagnostic services are
  provisional `dataSync` foreground services.
- `clients/android/app/build.gradle.kts` targets API 35. Android 15 therefore
  applies the cumulative six-hour background `dataSync` limit.
- `ProductEngineService.onCreate` creates one low-importance
  `rstorrent-product` channel and calls `startForeground` immediately. Its
  `START_STICKY` lifetime ends only through explicit Stop or exceptional
  shutdown.
- `MainActivity` reads and requests notification permission, while Library and
  Settings expose a setup card and system-settings link. Permission state does
  not currently change service or companion eligibility.
- `ProductEngineService.observePowerAndNotification` collects the same
  `ProductState` used by Compose, owns the active-work partial wake lock, and
  updates ongoing status. Its error branch currently puts raw
  `ProductState.error` into the notification and must become generic.
- `AndroidPresentationRepository` owns the sole summary torrent-list
  subscription and already distinguishes snapshots, patches, explicit reset,
  continuity failure, and resync. The notification coordinator can receive
  those facts through a narrow callback without another view subscription.
- `ProductState` contains authoritative generated `TorrentView` rows,
  `TorrentState`, `StorageState`, received-byte text, verified-piece counts,
  and display-name precedence sufficient to apply the desktop edge semantics.
- the current companion root-picker fallback uses the foreground channel and
  notification ID `43`, then attempts direct activity launch. It needs the
  Action required channel, collision-safe identity, and explicit behavior
  when notifications are unavailable.
- notification taps currently open the general `MainActivity`; no exact
  torrent or storage-settings route is owned.
- there is no `Service.onTimeout` implementation for API 35 `dataSync` quota
  exhaustion.

No new Rust event or subscription is required. The concrete boundary
improvement is to separate the current combined power/status collector into a
pure notification policy plus one Android coordinator, while the existing
service remains the sole mutable owner.

## Reference Inspection

### Maintained JSTorrent Android product

The maintained sibling JSTorrent checkout at revision
`25e4b701433fd815398ba89526546f5e4f072e3f` was inspected on 2026-08-30:

- `android/app/src/main/java/com/jstorrent/app/JSTorrentApplication.kt`
  creates low/default/high service, completion, and error channels; observes
  torrent transitions outside the foreground service; suppresses initial
  terminal state; and removes tracking for deleted torrents;
- `android/app/src/main/java/com/jstorrent/app/notification/TorrentNotificationManager.kt`
  checks Android 13 notification permission, posts per-torrent completion and
  error notifications, routes taps to the exact info hash, and offers an Open
  Folder completion action;
- `android/app/src/main/java/com/jstorrent/app/notification/ForegroundNotificationManager.kt`
  reports single- or multi-torrent progress, aggregate speed, network
  restriction, Pause All/Resume All, and Quit on a silent ongoing
  notification;
- `android/app/src/main/java/com/jstorrent/app/service/ServiceLifecycleManager.kt`
  runs the standalone service only when the activity is absent, background
  downloads are enabled, and selected downloading/seeding or playback work is
  active;
- `android/app/src/main/java/com/jstorrent/app/settings/SettingsStore.kt` and
  `ui/screens/PowerManagementSettingsScreen.kt` default background downloads
  off and require notification permission to opt in;
- `android/app/src/main/java/com/jstorrent/app/ui/screens/NotificationsSettingsScreen.kt`
  exposes permission state plus a system notification-preferences link rather
  than native completion/error toggles;
- `android/app/src/main/java/com/jstorrent/app/MainActivity.kt` and
  `service/IoDaemonService.kt` give legacy ChromeOS companion mode a separate
  permission-gated **Run in background** setting plus optional 5--120 minute
  disconnected auto-close; without permission, the daemon drops foreground
  status and is expected to die after its activity closes; and
- the current target is API 36, while neither foreground service implements
  the Android 15 `dataSync` timeout. That behavior is a known reference gap,
  not something RSTorrent adopts.

The shared JSTorrent extension/desktop client has a different notification
policy:

- `packages/engine/src/config/config-schema.ts` defaults completion and error
  alerts on and extension-only background-tab progress off;
- `packages/client/src/components/SettingsOverlay.tsx` exposes all three
  switches; and
- `extension/src/lib/notifications.ts` owns Chrome notifications and shows
  persistent progress only while its connected UI tab is hidden.

Those switches are not the current native Android notification UI. The old
companion also keeps its torrent engine in Chrome, while RSTorrent keeps the
one engine and semantic owner in Android. RSTorrent adopts the useful default
completion/attention choice and explicit background transparency, not the
legacy raw I/O daemon, split engines, Chrome visibility policy, raw error
body, unbounded active notification set, or foreground-service timeout gap.

### Android platform contract

Official Android documentation was inspected on 2026-08-30:

- [Notification runtime permission](https://developer.android.com/develop/ui/compose/notifications/notification-permission)
  states that `POST_NOTIFICATIONS` is not required to launch a foreground
  service, that the service must still supply a notification, and that denied
  foreground-service notices appear in Task Manager rather than the
  notification drawer on Android 13 and newer.
- [Launch a foreground service](https://developer.android.com/develop/background-work/services/fgs/launch)
  defines start eligibility, prompt promotion, notification, and declared
  type requirements.
- [Foreground service types](https://developer.android.com/develop/background-work/services/fgs/service-types)
  identifies upload and download as `dataSync` work and lists its alternatives.
- [Android 15 behavior changes](https://developer.android.com/about/versions/15/behavior-changes-15)
  impose a cumulative six-hour-per-24-hour background timeout on target-35
  `dataSync` services, call `Service.onTimeout`, and require prompt
  `stopSelf` to avoid failure.
- [`NotificationManager`](https://developer.android.com/reference/android/app/NotificationManager)
  defines app and channel block-state broadcasts delivered to the owning app,
  channel importance, channel inspection, and tagged notification cleanup.

This is Android platform integration rather than a BitTorrent protocol or
engine feature. No libtorrent inspection is required, and no JSTorrent source,
fixture, string, icon, or notification asset is imported.

## Notification And Settings Contract

### Installation-local preferences

Add one app-private `product_notifications` preference owner with exactly:

- `notify_download_complete`, default `true`; and
- `notify_needs_attention`, default `true`.

These settings are provisional-installation Android policy. They do not enter
the Rust profile, application settings, generated UniFFI contract, shared web
storage, Chrome extension storage, future product metrics, or a migration
promise. `JAR-004` must explicitly map them if the production package update
retains them.

Persist the complete new boolean before changing `ProductState` and visible
Compose state. A failed synchronous persistence leaves the prior live policy
active and presents bounded inline failure. Enabling a setting does not scan
for existing terminal rows. A disabled setting still advances reducer state,
so later enabling cannot replay a suppressed edge.

The Notifications page presents:

1. permission/background-visibility state with **Enable** or **Manage**;
2. **Download completed** with concise default-on wording;
3. **Needs attention** with fatal/storage-repair wording; and
4. **Manage system notification settings**, always available.

App preferences and Android channels are conjunctive. An edge displays only
when its app preference is enabled, app-level notifications are allowed, and
its system channel is not blocked. Compose must not claim that changing an app
toggle can override system channel state.

### Channels

| Channel | Initial importance | Product behavior |
| --- | --- | --- |
| Background activity | Low, silent, no badge | One ongoing foreground-service notification; no app toggle; blocking removes background eligibility under this tactical's transparency rule. |
| Downloads completed | Default | One auto-cancel event per eligible torrent completion; subject to the completion app preference. |
| Action required | High | One auto-cancel event per newly fatal/repair edge plus explicit pending Android workflows; the attention app preference filters only automatic torrent edges. |

Channel IDs remain derived from the current provisional package namespace and
are not production identity promises. Recreating a deleted or blocked channel
must not reset user-selected importance or sound. Channel creation occurs
before foreground promotion or ordinary delivery.

### User-visible content

- Completion title: **Download complete**.
- Completion body: bounded display name plus **finished downloading**.
- Attention title: **Download needs attention**.
- Attention body: bounded display name plus **Open RSTorrent for details**.
- Missing/blank display names fall back to **Torrent**.
- Normalize whitespace and limit the visible name to 120 Unicode scalar
  values, replacing the last value with an ellipsis when truncated.
- Ongoing status may say **Opening profile**, **Ready**, **Downloading 1
  torrent**, **Downloading N torrents**, **Ready for Chrome**, or **RSTorrent
  needs attention** according to authoritative state. It never interpolates
  `ProductState.error` or a diagnostic detail.
- Notification content, ticker, subtext, category, diagnostics, and intent URI
  never contain a raw error, path, source URI, magnet, metainfo, hash, tracker,
  peer, endpoint, pairing credential, installation ID, or extension origin.

Android lock-screen redaction, notification history, sound, vibration, Do Not
Disturb, and user-modified importance remain operating-system policy.
RSTorrent stores no parallel history.

## Edge Semantics And Resource Bounds

The pure Kotlin reducer mirrors Tactical `164` over generated `TorrentView`
values. It owns at most one entry per current torrent and a FIFO of the 256
most recently removed torrent IDs. It emits only typed `DownloadComplete` or
`NeedsAttention` values containing the internal torrent ID and bounded display
name; it knows nothing about Android `Context`, notifications, preferences,
intents, Compose, coroutines, filesystems, or clocks.

### Baseline, reset, and removal

- The first torrent-list snapshot establishes policy without output. Existing
  Complete, Error, NeedsRepair, or unavailable-storage rows do not notify.
- Every explicit reset, continuity failure, resync snapshot, service restart,
  and profile reopen establishes a new baseline without reconstructing missed
  edges.
- A baseline incomplete row can notify only after subsequent ordinary
  download progress is observed in the new service lifetime.
- A removed torrent immediately drops its reducer entry and cancels any active
  RSTorrent completion and attention notifications for that torrent.
- The 256-ID removed FIFO suppresses a coalesced remove/re-add terminal row.
  Later ordinary download work or recovery may establish a new edge.

### Completion

- Arm completion only after received payload bytes increase, or verified-piece
  count increases while the prior or current state is Downloading.
- Merely observing Downloading, restoring desired-running intent, importing
  existing payload, startup verification, or entering Checking does not arm
  completion.
- Entering or leaving Checking clears completion eligibility. Recheck of
  already complete content therefore cannot notify.
- Emit exactly once when an armed row first reaches both
  `TorrentState.COMPLETE` and available storage. Evaluate new progress before
  terminal state so one coalesced final patch remains eligible.
- Emission consumes the edge. Pause/resume, repeated rows, seeding, archive,
  view replacement, and settings changes do not duplicate it.
- A genuinely later incomplete generation can arm and complete again only
  after newly observed ordinary download work.

### Fatal and repair attention

- Emit once when a previously non-attention row newly enters
  `TorrentState.ERROR`, `TorrentState.NEEDS_REPAIR`, or storage
  `NEEDS_REPAIR`.
- Raw error changes while remaining in the same attention class do not emit.
- Recovery to a non-error, non-repair row clears the latch. A later distinct
  transition may emit once more.
- Awaiting metadata/storage, queued, paused, checking, stalled/no-peer,
  tracker retry, metered/VPN waiting, and ordinary recoverable network failure
  are not attention edges.
- If one row update appears to satisfy completion and attention, attention
  wins and completion is consumed without a second event.

### Active notification bounds and identity

- Retain at most 32 active automatic completion notifications and 32 active
  automatic attention notifications. Before posting a 33rd, inspect only this
  application's tagged active notifications and cancel the oldest in that
  category.
- The ongoing service and one pending explicit workflow notification are
  outside those category caps and retain their existing single-instance
  bounds.
- Derive a stable opaque notification tag and `PendingIntent` identity from
  category plus canonical torrent ID without putting the raw ID in visible
  content or an intent URI. Keep the exact torrent ID only in an explicit
  package-private intent extra needed for routing.
- Process restart may leave an operating-system-retained notification, but
  stable tags permit later removal cleanup without a durable notification
  ledger. Restart never reposts the edge.
- Notification submission is best effort. Permission revocation, channel
  blocking, `SecurityException`, system rejection, or process death records
  only a bounded category/reason and never retries or changes torrent state.

## Permission And Background Transparency

Define a native `NotificationEligibility` value from:

- Android version and `POST_NOTIFICATIONS` grant;
- app-wide notification block state;
- Background activity channel block state; and
- whether at least one visible-interaction lease exists.

The service owns this state. `MainActivity` owns one visibility lease across
started/stopped transitions. An existing app-owned permission, notification
settings, SAF picker, or companion approval/result workflow may retain one
bounded interaction lease until result, cancel, activity recreation handoff,
or its existing workflow deadline. Chrome tabs and WebSocket connections are
not visibility leases.

| Notification visibility | Interaction lease | Required outcome |
| --- | --- | --- |
| Allowed and Background activity channel unblocked | Present or absent | This tactical preserves current service behavior; `JAR-009` later decides when granted background work should actually remain alive. |
| Denied or app/channel blocked | Present | Interactive Compose and companion work may continue. The UI explains that leaving Android stops background work. |
| Denied or app/channel blocked | Absent | Cancel and join the companion listener, presentation subscriptions, SAF workers, application service, and wake lock; remove foreground status; stop the service without sticky restart. |

Register only app-delivered notification app/channel block-state observations
while the service exists, and re-evaluate permission/channel state on service
start, activity start/stop, permission result, system-settings return, and
pending interaction completion. Unregister every receiver/listener during
shutdown. No permanent receiver or polling loop is required.

Denial does not trigger repeated permission prompts. A user action may request
permission while Android permits an inline request; otherwise it opens exact
application notification settings. Returning without a grant keeps the prior
preferences and visible-only behavior.

On ChromeOS:

- a companion deep link remains user-visible and may start the one service;
- Android owns the prompt and permission explanation, never the extension;
- with permission unavailable, approval and a connected companion can operate
  only while the Android interaction lease remains;
- leaving Android closes the listener and authenticated connections normally,
  so the extension presents disconnected/retry state rather than stale
  success; and
- after a later grant and explicit Android/extension action, the same existing
  pairing may reconnect. No edge or root request is replayed automatically.

## Tap And Action Routing

- Completion tap opens or reuses `MainActivity`, selects the exact torrent,
  and navigates to its General detail. A removed or unavailable torrent falls
  back to Library with bounded visible feedback.
- Fatal torrent attention opens the exact torrent. Storage repair attention
  opens the existing Storage repair route for the exact referenced root when
  available; otherwise it opens Storage settings without guessing another
  root.
- The pending companion root notification continues to open its exact
  request/repair flow and cancels when completed, canceled, expired, or the
  service shuts down. It is not suppressed by the automatic attention app
  preference.
- Every `PendingIntent` is explicit, package-owned, immutable, uniquely
  identified, and update-safe. Cold, warm `singleTop`, activity recreation,
  and stale/removed targets converge through the existing routing owner.
- Notification taps never submit an application command automatically, grant
  storage permission, approve pairing, start a torrent, or expose an Android
  URI to Chrome.
- The ongoing foreground notification retains Open and Stop. Stop invokes the
  existing joined shutdown path and closes companion connectivity. Pause All
  and Resume All remain deferred.

## Ownership, Tasks, And Dependency Direction

```text
ApplicationService TorrentList view
  -> AndroidPresentationRepository sole list subscription
     -> exact snapshot / patch / reset callback
        -> pure AndroidNotificationPolicy
           -> zero or one typed edge per torrent transition
              -> ProductNotificationPreference filter
              -> AndroidNotificationCoordinator
                 -> Android NotificationManager channels/tags/intents

MainActivity + owned activity-result workflows
  -> bounded visible-interaction leases
     -> service-owned NotificationEligibility
        -> keep current owner while visible or notifications are visible
        -> otherwise joined ProductEngineService shutdown

Android app/channel block broadcasts + permission results
  -> NotificationEligibility re-evaluation

API-35 Service.onTimeout(dataSync)
  -> same joined shutdown owner
  -> stopForeground + stopSelf

ProductEngineService shutdown
  -> cancel notification/eligibility owners and pending workflow notification
  -> cancel/join companion and presentation owners
  -> cancel/join SAF workers and ApplicationService
  -> release wake lock
  -> remove ongoing notification and stop
```

`AndroidNotificationPolicy` is a plain Kotlin module. Runtime and Android
adapters depend inward on it. `AndroidNotificationCoordinator` has no engine,
profile, SAF, companion-server, or Compose authority. It receives already
authoritative rows and constrained routing callbacks.

Use the existing presentation subscription rather than adding a second
TorrentList subscription. Extend its callback boundary so the notification
owner sees list snapshot, patch, removal, and reset identity before those
facts are collapsed into general Compose state. Power-lock state may continue
to consume `ProductState`; split native notification policy and delivery out
of the current combined collector.

The coordinator owns no free-running delivery coroutine. It performs bounded
state reduction and notification submission on the service scope. Any
registered receiver, visibility lease, pending workflow, or platform callback
has an explicit unregister/cancel path and is gone before service shutdown
returns.

## Failure And Race Cases

- Permission can be revoked between eligibility check and `notify`. Catch the
  platform failure, advance reducer state, re-evaluate background eligibility,
  and never replay the edge.
- App or Background activity channel blocking while Android is absent closes
  the owner through the same idempotent joined shutdown as Stop. Duplicate
  block broadcasts cannot create parallel shutdowns.
- Activity stop racing a permission grant observes one serialized eligibility
  result. Grant may retain the owner; denial may stop it. Neither starts a
  second service or companion listener.
- Permission and SAF/system-settings activities retain only their already
  bounded interaction result. Process death loses that ephemeral lease and a
  denied sticky restart shuts down instead of guessing visibility.
- An explicit companion root request racing permission loss either opens while
  Android is visible or remains canceled/failed through its existing request
  owner. It never waits indefinitely on a notification that cannot appear.
- A completion edge and torrent removal in one coalesced update leave no
  notification. A post racing later removal is canceled by stable tag.
- Reset or continuity failure clears notification policy before resync. The
  replacement snapshot establishes state without output.
- Channel creation is idempotent and never restores user-blocked importance.
- Active-notification inspection failure falls back to suppressing that new
  post rather than exceeding the 32-per-category bound.
- API-35 timeout racing Stop or permission loss shares one atomic shutdown
  guard. It never calls `startForegroundService`, reacquires the wake lock, or
  returns `START_STICKY` into exhausted quota.
- Raw product errors remain available only in existing in-app diagnostics;
  ongoing and event notifications use generic text.

## Implementation Stages

1. Add the pure Kotlin reducer, typed edges, name normalization, opaque
   identity helper, and exhaustive snapshot/patch/reset/removal tests.
2. Refactor the presentation repository's narrow callback so the sole list
   subscription supplies exact baseline/reset facts to one service-owned
   coordinator. Separate ongoing notification delivery from the power-lock
   collector.
3. Add the app-private preference owner and native `ProductState` fields;
   replace the Notifications placeholder with permission, two default-on
   toggles, system settings, and bounded persistence failure presentation.
4. Create the three channels, move the companion root fallback to Action
   required, make ongoing text generic, implement capped tagged event
   delivery, and add exact cold/warm tap routing.
5. Add visibility leases and app/channel block observation. Make denied or
   blocked no-visibility state enter the existing joined service shutdown,
   including normal companion disconnect and no sticky restart.
6. Implement target-35 `dataSync` `onTimeout` through the same atomic shutdown
   owner and prove a shortened timeout leaves no Android or Rust owner.
7. Run local Kotlin/Compose/instrumentation, merged-manifest, dual-ABI, full
   repository, and API 34/35 AVD campaigns. Record notification counts,
   channels, taps, permission matrices, resource bounds, and cleanup.
8. Use the repository-authorized physical ChromeOS testbed for denied/granted
   companion behavior, reconnect, and cleanup. Do not change the production
   extension, Android package identity, or Play state.
9. Reconcile this tactical and the owning topics with exact evidence. Close
   `JAR-007` and `AND-009` only after every stopping condition passes; leave
   `JAR-009` and the indefinite-background claim open.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure Kotlin policy | Initial terminal baseline; ordinary progress then completion; coalesced final progress; zero-byte/import/recheck suppression; fatal/repair edges; raw-error churn; recovery; removal/re-add; reset/resync; attention-over-completion; 256 removed IDs; 120-character names |
| Preference and eligibility | Defaults; durable toggles; failed persistence; no enable replay; API/version permission; app and service-channel block; visible lease acquire/release; duplicate events; process restart; idempotent joined stop |
| Android notification adapter | Three exact channels; user importance retention; generic status; completion/attention bodies; stable opaque tags; 32/32 eviction; explicit immutable unique intents; submission denial/failure; removal and shutdown cleanup |
| Compose/navigation | Permission explanation; Enable/Manage behavior; two toggles; system-channel truth; no false background claim; exact cold/warm torrent and storage routing; removed target fallback; companion root request |
| AVD runtime | API 34 and 35 grant/deny/revoke/channel-block matrices; visible-only service; genuine controlled completion and repair; notification-shade inspection; content-intent tap; restart/reset/recheck non-replay; shortened `dataSync` timeout; force-stop/uninstall cleanup |
| Physical ChromeOS | Android-visible denied companion; extension connection; switch-to-Chrome disconnect; grant and reconnect; foreground notification; pending root action; service Stop; process/listener/wake-lock/notification cleanup |
| Repository | Kotlin format/compile/lint/unit/instrumentation, both native ABIs, Rust workspace baseline, unchanged generated contract, no secret/source leakage |

### Deterministic and package cases

At minimum, automated tests cover:

- first snapshot containing Complete, Error, torrent NeedsRepair, and storage
  NeedsRepair rows emits zero events;
- Downloading with received-byte or verified-piece growth followed by Complete
  and available storage emits one completion;
- imported complete payload, startup Checking-to-Complete, Force recheck, and
  zero-work Complete emit none;
- identical/coalesced rows, service restart, explicit reset/resync, preference
  disable/enable, channel unblock, and permission grant do not replay;
- an automatic event suppressed by its app preference still consumes its
  edge;
- error and storage-repair transitions emit once, raw message changes do not,
  recovery rearms, and a later distinct edge emits once;
- removal cancels both possible event tags and a coalesced terminal re-add is
  suppressed;
- the 33rd active completion or attention event evicts only the oldest same-
  category RSTorrent event;
- status and event content omit sentinel errors, paths, URIs, magnets, hashes,
  endpoints, pairing values, and extension origins;
- each notification intent has exact package ownership, immutable flags,
  unique identity, and correct cold/warm/stale-target routing;
- permission denial and app/service-channel blocking with a live Activity do
  not stop interactive use, while release of the last interaction lease joins
  the service and companion;
- a blocked completion channel does not disable background eligibility, while
  a blocked Background activity channel does;
- a pending companion root notification bypasses only the automatic attention
  app preference, never Android permission or system channel policy; and
- timeout, Stop, permission loss, and concurrent Activity return share one
  idempotent shutdown and leave no restarted owner.

### Installed AVD campaign

Use explicitly owned API 34 and API 35 AVDs. Build and install the debug
product, then use `adb` package-manager, app-op, notification, activity, and
`dumpsys` interfaces before any notification-shade UI inspection.

The campaign must record:

- exact API, ABI, application ID, target SDK, APK digest, channel IDs,
  importance, permission and app/channel state;
- one genuine tiny controlled transfer from incomplete state through exact
  payload verification and one controlled storage-repair edge;
- active notification count, tag/category, redacted title/body, tap target,
  task/service/PID identity, and no duplicate after restart/reset/recheck;
- preference and system-channel suppression plus later non-replay;
- Home/background with permission granted versus denied and service-channel
  blocked, including bounded joined-stop timing;
- a documented shortened API-35 `dataSync` timeout with `onTimeout`, service
  stop, no ANR/sticky restart, no listener, no wake lock, and no coroutine or
  Rust-owner residue; and
- exact permission/channel restoration, app-data cleanup, AVD teardown, and
  zero retained test notifications or temporary artifacts.

### Physical ChromeOS campaign

Before physical work, read the Machine Control ChromeOS platform guide and run
the common read-only doctor. Use the current installed beta extension only as
the existing Tactical `194` companion presentation; no extension publication
or production-ID change is authorized.

On the claimed Chromebook:

1. deny Android notification permission, launch the companion through the
   exact extension flow, approve/connect while Android remains visible, and
   verify the UI explains visible-only operation;
2. return to Chrome and prove the Android owner joins, the exact ARC listener
   refuses, and the extension reports disconnected/retry rather than success;
3. grant permission through Android user action, reconnect the same pairing,
   return to Chrome, and verify the low-importance foreground notification and
   current provisional background behavior;
4. cause one controlled completion and one exact pending root or repair action
   while Compose is absent, inspect/tap each Android notification, and verify
   exact routing without a second owner; and
5. Stop, revoke, force-stop/uninstall as appropriate, restore inherited device
   policy, release the testbed claim, and verify zero process, ARC listener,
   notification, wake lock, pending request, permission mutation, or staged
   artifact.

This campaign proves transparency and native delivery, not long-duration
companion survival, suspend/reboot behavior, store policy, or Android 15 quota
avoidance.

### Build and repository baseline

Run from the repository root after sourcing the configured profile:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
(
  cd clients/android
  ./gradlew lintDebug testDebugUnitTest assembleDebug assembleDebugAndroidTest
)
./clients/android/build.sh
```

Run connected instrumentation only on explicitly owned AVDs and record exact
commands and cleanup. Web typecheck/test is required only if implementation
changes shared React code; the accepted design keeps Android settings and
notification delivery native. Regenerate no contract unless implementation
finds a missing authoritative field and stops under the escalation contract.

## Documentation And Completion Updates

Before marking this tactical complete:

- record exact commits, Android/JSTorrent source paths, tests, commands, AVDs,
  physical Chromebook identity class, notification/channel/permission
  outcomes, transfer/repair results, timeout behavior, resource high waters,
  failures, and cleanup here;
- mark `JAR-007` complete in
  [`android-jstorrent-replacement.md`](../topics/android-jstorrent-replacement.md);
- mark `AND-009` complete in
  [`beta-release-readiness.md`](../topics/beta-release-readiness.md);
- update Android notification, companion, and background truth in
  [`client-surfaces.md`](../topics/client-surfaces.md) and
  [`capability-readiness.md`](../topics/capability-readiness.md);
- leave `JAR-009` open with the target-35 timeout and granted-background
  alternatives as explicit inputs; and
- leave production channel IDs, strings, package identity, signing, Play
  declarations, production extension rollout, and publication to `JAR-004`,
  `JAR-005`, and `JAR-010`.

## Escalation Contract

Implementation may choose internal Kotlin class names, refactor the current
power/status collector and presentation callback, add app-private preferences,
add debug-only deterministic fault/timeout hooks, tighten active notification
bounds, and fix same-owner Android bugs without further direction.

Stop for maintainer direction if evidence requires:

- retaining a denied or service-channel-blocked background/companion owner
  after all visible interaction ends;
- changing the accepted default-on completion/attention preferences, adding a
  focused/Chrome-visible suppression rule, or making Chrome an Android
  notification owner;
- adding Pause All, Resume All, Open Folder, direct repair mutation, a general
  notification command, a durable notification ledger, or another service or
  process;
- selecting a new foreground-service type, WorkManager, user-initiated data
  transfer jobs, Companion Device Manager, exemption, or quota-avoidance
  mechanism;
- changing the Rust application service, UniFFI/generated contract, torrent
  state, storage state, completion meaning, or profile persistence;
- changing production package/channel identity, the production extension,
  signing/store state, or any public release artifact; or
- using a physical phone, an unapproved ChromeOS target, a public swarm, or
  external publication beyond the explicitly bounded physical ChromeOS
  campaign.

An ordinary Kotlin, Compose, notification, AVD, timeout, companion, build, or
test failure is not an escalation. Diagnose it within the declared owner and
bounds.

## Next Slice Boundary

After `JAR-007` closes, execute the unmetered portion of `JAR-008` or plan
`JAR-009` background lifecycle. `JAR-009` must use the transparency and
target-35 facts fixed here to select:

- whether background downloads remain opt-in and their fresh-install default;
- the exact active download, checking, seeding, playback, companion, and idle
  reasons to retain the owner;
- task-removal, process-recovery, reboot, low-battery, and companion auto-close
  behavior;
- a truthful Android 15+ mechanism or explicit finite-duration limitation; and
- cancellation, user-intent preservation, restart, and Play declaration
  evidence.

Notification implementation must not preselect those larger lifecycle answers
or turn current sticky behavior into a supported long-run promise.
