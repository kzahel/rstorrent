# Tactical 200: Android Product Background Lifecycle

Status: **Complete as of 2026-08-31.**
Maintainer direction selected JSTorrent-like standalone lifetime semantics
for the Android replacement and explicitly authorized end-to-end repository
implementation. VPN, proxy, low-battery thresholds, and configurable
companion-idle policy remain deferred. The initial close used a maintainer-
accepted composition of the completed API 28/API 35 AVD campaigns, Tactical
`194`'s physical ChromeOS transport/security evidence, and this tactical's
deterministic companion-lifetime evidence after the approved Chromebook was
unreachable and locked. A later explicitly authorized physical ChromeOS 150 /
Android API 33 campaign strengthened that close with the real Home, recovery,
completion, seeding, notification-eligibility, companion grace/reconnect,
explicit-Stop, relaunch, listener-refusal, and cleanup matrix. `JAR-009` is
closed without claiming indefinite Android background duration or OEM-wide
behavior.

Topics: `android-jstorrent-replacement`, `beta-release-readiness`,
`application-control`, `client-surfaces`, `capability-readiness`

Dependencies: completed Android product Tactical
[`117`](117-jstorrent-shaped-android-product-ui.md), completed active-work
sleep Tactical
[`165`](165-cross-platform-active-download-sleep-inhibition.md), completed
ChromeOS companion Tactical
[`194`](194-chromeos-android-extension-control.md), and completed external-
intake Tactical [`197`](197-android-external-torrent-intake.md). Runtime
integration also consumes notification-transparency Tactical
[`198`](198-android-completion-and-attention-notifications.md) and live
unmetered-network Tactical
[`199`](199-android-live-unmetered-network-enforcement.md). Their native
notification eligibility/timeout and fail-closed initial/live network seams
are implemented with deterministic, generated-boundary, dual-ABI, and owned-
AVD evidence. Tactical `198`'s exact physical completion/repair notification-
tap gate and Tactical `199`'s physical-phone network-handoff gate remain
independent open work; neither blocks this tactical's completed lifetime
contract.

## Decision And Desired Outcome

Replace the current always-sticky Android product with one explicit lifetime
owner matching the useful maintained JSTorrent standalone behavior:

- Android UI keeps the application available while it is visibly in use;
- **Continue downloads in background** is an explicit default-off setting;
- background operation requires visible foreground-service notification
  eligibility selected by Tactical `198`;
- only active download, metadata, checking, temporary unmetered-network
  waiting, separately opted-in seeding, an authenticated live companion, or a
  bounded platform interaction retains the owner after Android UI leaves;
- completion stops background operation by default;
- **Keep seeding in background** is a separate default-off choice;
- idle, paused, queued-only, fatal, or storage-repair state does not keep an
  unattended engine alive;
- ordinary task removal follows the selected background policy, while
  explicit **Stop**, Android foreground-service timeout, notification
  ineligibility, and user/system stop remain terminal for that service
  generation; and
- a later user-visible launch opens the same profile and lets durable torrent
  intent resume through ordinary application admission.

The foreground service and the Rust application remain one product owner.
RSTorrent does not copy JSTorrent's split `Application` engine plus separate
notification service or its legacy ChromeOS raw-I/O daemon. While one Android
activity is genuinely visible, the service may leave foreground status after
the binding/visibility handoff and remain as the bound application owner.
Before the last visible lease is released into eligible background work, the
same owner promotes to `dataSync` foreground status. If no background reason
is eligible, it cooperatively joins the application and stops.

This setting controls product lifetime, not torrent intent. Background
disablement, completion policy, timeout, idle shutdown, task removal, and
companion disconnect never submit Pause, Pause All, Stop Torrent, or a queue
mutation. They close the service/application generation. Desired-running,
paused, queue, selection, priority, limits, verified pieces, roots, and
storage state remain durable, so only still-desired-running work can return
on the next user launch.

Android 15 and newer do not permit an indefinite `dataSync` claim. Retain the
existing truthful service type and Tactical `198`'s prompt `Service.onTimeout`
shutdown. The product states that Android can limit a long background session
and that opening RSTorrent again is required to continue. Do not silently
restart into exhausted quota or present background operation as perpetual.

## Accepted Lifetime Matrix

The service reduces current facts into one of these product outcomes:

| Android/product facts | Outcome |
| --- | --- |
| At least one Android activity is started and visible | Retain the application as interactive. After the foreground-start handoff is satisfied, remove foreground status unless a bounded external Android workflow requires it. Background and seeding preferences do not limit visible use. |
| No activity, but a bounded permission/settings/SAF/companion-approval result workflow owns an interaction lease | Retain only through that workflow's existing deadline. Use foreground status when required; notification denial is allowed only under Tactical `198`'s visible-interaction rule. |
| No interaction lease and notification/app/Background activity channel eligibility is denied or blocked | Join and stop regardless of background, torrent, or companion preference. |
| No interaction lease, notifications eligible, background enabled, and at least one qualifying download/check/wait owner exists | Retain in `dataSync` foreground state. |
| No interaction lease, notifications eligible, background and keep-seeding enabled, and at least one desired-running complete torrent is seeding | Retain in `dataSync` foreground state. |
| No Android activity, notifications eligible, and at least one authenticated ChromeOS companion connection is active | Retain in `dataSync` foreground state even when background downloads are disabled; the extension is the active presentation. |
| The last companion disconnects and no other background reason exists | Retain for one fixed 60-second reconnect grace, then join and stop. A reconnect cancels the deadline. |
| No visible interaction, qualifying download/check/wait, opted-in seed, active companion, or reconnect grace | Join and stop after the bounded state-settle interval. |
| Explicit Stop, API-35 `dataSync` timeout, terminal initialization failure, or committed shutdown | Join and stop without sticky restart. A new visible user action is required to create another generation. |

Foreground notification delivery is a prerequisite, not a lifetime reason.
The existence of the ongoing notification never keeps an otherwise idle
owner alive. A configured listener, DHT routing table, peer catalog, mapping,
retained pairing, queued-only torrent, notification event, or enabled setting
also does not independently qualify.

### Qualifying torrent work

Use the authoritative existing torrent-list projection, not Kotlin inference
from rates, byte changes, names, notifications, or engine task handles:

- `TorrentOperationalState::{Starting,Downloading,Checking}` qualifies as
  background download work;
- after Tactical `199`, desired-running
  `ProgressReason::WaitingForUnmeteredNetwork` also qualifies. The quiescent
  service must remain able to observe eligibility and resume while Android's
  finite foreground allowance remains;
- `Seeding` qualifies only when **Keep seeding in background** is enabled;
- `Queued`, `Paused`, and `Error` do not qualify alone;
- awaiting storage, storage repair, fatal failure, removal, and archived state
  do not qualify; Tactical `198` may leave an actionable notification before
  this owner stops; and
- `Stopping` retains only the already-committed join. It cannot reverse the
  lifetime decision or become a new background reason.

Queue promotion and completion can cross in adjacent projection updates. Use
one two-second latest-state settle interval before idle shutdown so an
ordinary newly admitted queued torrent can become active without tearing down
the application. This is transition stabilization, not a user-configurable
idle timer. A reset or continuity loss retains the prior eligibility for at
most five seconds while the existing presentation owner requests a fresh
snapshot; failure to resynchronize then stops rather than guessing active
work.

## Scope And Stopping Condition

This tactical owns:

1. one task-free Kotlin `ProductLifetimePolicy` reducer over interaction,
   notification, preference, authoritative work, companion, startup, timeout,
   stop, and monotonic-deadline facts;
2. one service-owned serialized lifecycle coordinator with a nonzero
   generation, one nearest-deadline job, exact promotion/demotion, idempotent
   joined shutdown, and observable termination;
3. app-private default-off **Continue downloads in background** and **Keep
   seeding in background** preferences with failure-safe persistence and no
   portable Rust-profile command;
4. a Power Management presentation matching the accepted JSTorrent outcomes,
   notification-permission/settings routing, finite-background disclosure,
   current configured/effective truth, and no false indefinite guarantee;
5. exact Android activity and bounded platform-workflow interaction leases,
   including configuration change, permission/settings return, SAF picker,
   external intake, and companion approval/root handoff races;
6. foreground promotion before the last visible lease enters an eligible
   background state, foreground demotion after a genuine visible handoff, and
   `START_STICKY` only for an admitted background generation;
7. intent-preserving idle/completion/disable/Stop/timeout shutdown and clean
   foreground reopening through the incumbent profile/application owner;
8. one event-driven, count-only ChromeOS companion activity observation plus
   a fixed 60-second launch/disconnect reconnect grace;
9. fail-closed sticky-process recovery that checks preferences and Tactical
   `198` notification eligibility before application open, starts networking
   closed through Tactical `199`, and permits egress only after authoritative
   work qualification;
10. exact task-removal, process-death, Android Task Manager stop, force-stop,
    timeout, start/stop race, service-start failure, and no-reboot behavior;
11. integration with Tactical `165` so wake-lock policy remains independently
    level-triggered by active work and never by the background setting or
    seeding alone; and
12. deterministic Kotlin/Rust boundary coverage, dual Android ABI builds, API
    28 and API 35 installed AVD campaigns, controlled transfer/restart
    evidence, and bounded physical phone plus ChromeOS campaigns after exact
    target authorization.

The tactical stops only when:

- a fresh install has both lifecycle preferences off and leaving Android with
  active work joins the service without altering torrent intent;
- explicitly enabling background downloads through notification-eligible UI
  lets a genuine incomplete transfer survive Home, screen-off, and task
  removal, then stop when its last qualifying download completes;
- enabling background seeding separately retains a completed desired-running
  seed, proves one controlled upload, and disabling it joins the otherwise
  idle owner without pausing the torrent;
- metadata acquisition, checking, unmetered waiting, queue promotion, user
  pause, fatal/storage failure, all-complete, and empty-library cases follow
  the matrix without rate- or timing-based guesses;
- foreground return after policy shutdown opens the same profile, preserves
  exact verified state/intent, and resumes only ordinary eligible work;
- an authenticated companion keeps the one owner alive, disconnect/reconnect
  cancels the fixed grace correctly, and an idle disconnected listener cannot
  live indefinitely;
- a killed admitted-background process recovers through one sticky generation
  without an optimistic network leak, while explicit Stop, timeout, idle
  shutdown, Task Manager Stop, force-stop, and reboot create no automatic
  replacement;
- visible Compose, permission/settings/SAF workflows, foreground promotion,
  notification revocation, process recovery, and concurrent start/stop cannot
  create two services, two application owners, a profile-lock race, or stale
  foreground state;
- API-35 shortened quota handling reaches Tactical `198`'s joined terminal
  zero without ANR or restart, and the product makes no duration promise
  beyond Android's allowance; and
- deterministic, generated Android boundary, build, AVD, controlled runtime,
  physical device, resource high-water, and cleanup evidence is recorded here
  and in the owning topics.

Passing this tactical closes replacement gate `JAR-009`. It does not qualify
an indefinite Android background service, signed/store distribution, reboot
autostart, low-battery policy, configurable companion auto-close, playback,
or general seeding goals.

## Preferences And Product Presentation

Add one app-private `product_lifecycle` preference record:

- `background_downloads_enabled`, default `false`; and
- `background_completion_policy`, closed value
  `stop_when_downloads_complete` by default or `keep_seeding`.

The values belong to this Android installation, not the Rust profile, torrent
settings, generated application command schema, Chrome extension, web local
storage, or desktop/iOS shell. Production Tactical `JAR-004` later decides
whether and how current JSTorrent preferences migrate.

Enabling background downloads is one explicit user transaction:

1. inspect Tactical `198` notification permission plus app and Background
   activity channel eligibility;
2. if permission can be requested, explain that the same action permits
   visible background transfer status and request it;
3. if system notification settings must be repaired, route there and require
   a later explicit confirmation after return;
4. commit `background_downloads_enabled=true` synchronously only after current
   eligibility succeeds; and
5. publish configured/effective state and re-evaluate lifetime.

A general notification grant for completion alerts does not silently enable
background downloads. Grant resulting directly from the explained background
toggle may complete that pending user transaction, matching JSTorrent's
explicit permission-backed opt-in. Denial/cancel leaves the setting false.
Runtime permission revocation persistently disables the setting, matching
maintained JSTorrent. App or Background activity channel blocking makes the
setting ineffective and stops unattended work but retains configured intent
so the user can repair the channel deliberately. Never repeatedly prompt.

Disabling persists `false` before publishing or relaxing any UI state. If
Android is already absent, the lifecycle coordinator joins immediately after
the normal settle boundary unless a companion interaction independently
qualifies. Persistence failure retains the prior configured/effective value
and presents one bounded inline error.

**Keep seeding in background** is enabled only when background downloads are
configured. Turning it on requires one concise battery/data warning, matching
JSTorrent's product warning, then durably stores `keep_seeding`. Turning
background downloads off does not erase this secondary choice; it becomes
ineffective until background operation is enabled again. Turning keep-seeding
off while Android is absent joins an otherwise seed-only owner without
changing torrent desired state.

The Power Management page orders:

1. **Continue downloads in background**;
2. **Keep seeding in background**;
3. the implemented **Prevent sleep during active downloads and checks**; and
4. the still-unavailable **Low-battery shutdown** row.

Supporting text says that background operation needs visible notifications,
stops when no selected work remains, and can be limited by Android. It does
not promise an exact remaining quota, reboot recovery, continuous Doze
networking, or completion while the app is force-stopped. The ongoing
notification reports the highest-priority bounded reason: active download or
check, waiting for an unmetered network, seeding, connected ChromeOS client,
or finishing shutdown. It never exposes paths, endpoints, pairing identity,
or a raw error.

## Foreground, Interaction, And Service Contract

### Direct visible start

`MainActivity` and the exact user-visible external-intake/ChromeOS deep-link
routes remain the normal service entry points. `ProductEngineService` calls
`startForeground` immediately on creation to meet Android's foreground-start
deadline, then holds one maximum 30-second startup handoff while the activity
binds, registers its visibility lease, and the first authoritative product
snapshot arrives.

Once a real Android activity is started and no out-of-app result workflow
needs foreground retention, remove the ongoing foreground notification with
`STOP_FOREGROUND_REMOVE` while keeping the single bound/start-generation
application owner. Activity binding is not itself sufficient: `MainActivity`
reports started/stopped state explicitly so a stale binding, configuration
handoff, or delayed disconnect cannot fabricate visibility.

Use a two-second latest-state visibility settle boundary across configuration
changes and activity replacement. Bounded permission, notification-settings,
SAF, external-provider, and companion-approval/root workflows acquire named
interaction leases from their incumbent owners and release them on result,
cancel, deadline, recreation handoff, or shutdown. There is no general token
API for clients and no lease survives process death.

### Entering background

Before accepting the last visible-activity release, calculate the latest
lifetime decision. If download, seed, or companion work qualifies and
notifications are eligible, promote the current generation to foreground and
publish its ongoing status before the interaction settle boundary expires.
Record that start generation as sticky only after promotion succeeds. A
promotion failure keeps networking closed, enters joined shutdown, and reports
one bounded in-app failure if Android is still visible.

If no background reason qualifies, start the settle deadline. New visible,
torrent, preference, notification, or companion facts cancel or replace that
deadline. When it fires against the same revision, atomically commit
`Stopping`, close Tactical `199`'s network prerequisite, cancel platform
work, join the application/companion/presentation/SAF owners, remove
foreground status, and call the start-ID-safe service stop.

### Sticky process recovery

Return sticky behavior only from a successfully admitted background start
generation. A null-intent recreation therefore means Android is attempting to
restore a generation that previously had a qualifying reason; it is not a
generic launch instruction.

On that recreation:

1. load lifecycle preferences and current notification/app/channel
   eligibility before opening the application;
2. stop immediately and non-sticky if background is disabled, visibility is
   unavailable, quota timeout is terminal, or notifications are ineligible;
3. otherwise open the normal profile with Tactical `199`'s BitTorrent network
   prerequisite initially closed for lifecycle admission;
4. wait at most 30 seconds for the authoritative initial torrent snapshot and
   companion activity facts;
5. allow the normal network prerequisite only if download, opted-in seed, or
   companion work still qualifies; otherwise join and stop without BitTorrent
   egress; and
6. retain the same durable request-ID, queue, root, verification, and
   application recovery rules as an ordinary visible open.

There is no separate summary cache, lifecycle journal, persisted active flag,
or Android parser for the Rust database. Android's sticky-start fact is only a
bounded admission hint; authoritative application state decides continuation.
Reset/resync and late startup outcomes are generation-fenced so an old
snapshot cannot reopen a newer stopping owner.

### Stop, timeout, task removal, and reboot

- The ongoing notification's **Stop** and Compose's explicit shutdown commit
  terminal non-sticky shutdown. They do not pause torrents and do not allow a
  concurrent old callback to restart the service.
- Android 15 `dataSync` timeout uses Tactical `198`'s same atomic joined
  shutdown and calls `stopSelf` within the platform deadline. It never posts a
  delayed self-restart, changes foreground-service type, or falls back to
  hidden work.
- Swiping the task away is not Stop. If an admitted background reason remains,
  the foreground service continues; otherwise the ordinary last-visibility
  decision joins it.
- Android 13+ Task Manager Stop and force-stop provide no cooperative callback
  and kill the whole app. OS process/socket cleanup is authoritative; the next
  explicit visible launch may record only the bounded
  `ApplicationExitInfo.REASON_USER_REQUESTED` category before normal recovery.
- Do not declare `RECEIVE_BOOT_COMPLETED`, a boot/package-replaced receiver,
  alarm, worker, or job. Reboot leaves RSTorrent stopped until a user-visible
  activity, external intake, notification action, or ChromeOS handoff starts
  it. Android 15 also prohibits launching this `dataSync` service from
  `BOOT_COMPLETED`.
- Background-start restriction failures are terminal for that attempted
  generation and never spin. Normal starts originate from a visible activity
  or other already accepted user-visible platform route; sticky recreation is
  the platform-owned exception.

## ChromeOS Companion Contract

Tactical `194` retains one Android application/profile/engine and one
same-device ARC listener. A successfully authenticated companion WebSocket is
an active product interaction even if Compose is absent. It keeps the service
foreground while Tactical `198` notification eligibility is satisfied,
independently of the background-download preference.

Add a count-only, event-driven observation from the existing companion
gateway owner to `AndroidApplicationClient`. It publishes initial and changed
authenticated connection counts through one latest-value bounded watch. It
contains no origin, extension/installation ID, credential, address, request,
or frame fact. Kotlin owns one joined consumer job; there is no interval poll,
per-connection coroutine in the lifecycle layer, or portable application
view.

The first explicit ChromeOS launch and transition from one active connection
to zero starts one fixed 60-second grace when no torrent/background reason
otherwise qualifies. Reconnection cancels it. Expiry joins the entire service,
including the listener; a later extension action uses Tactical `194`'s normal
user-visible Android launch/reconnect path. An enabled listener, retained
pairing, or unauthenticated HTTP probe does not reset the grace.

This intentionally differs from maintained JSTorrent's separate legacy
`IoDaemonService`, whose **Auto-close when idle** preference defaults off and
offers 5--120 minutes. RSTorrent's companion is not an independent daemon: an
idle disconnected listener would retain the whole Rust engine/profile and
consume the same finite `dataSync` allowance. A configurable companion-idle
preference remains deferred until production extension evidence shows the
fixed reconnect behavior is insufficient.

Pending pairing and Android root workflows retain only Tactical `198`'s
bounded interaction leases. Notification denial while Android is absent
closes an authenticated companion regardless of its count. Companion commands
never change Android-only lifecycle preferences.

## Intent, Seeding, Network, And Power Invariants

- Lifetime shutdown calls the existing joined `ApplicationService::shutdown`
  and never a torrent Pause/Resume command.
- `stop_when_downloads_complete` controls only unattended owner retention.
  While Android or an authenticated companion is active, a desired-running
  complete torrent may seed normally. After lifecycle shutdown, its durable
  desired-running complete state can seed again on a later interactive open.
- `keep_seeding` means only that desired-running complete torrents qualify to
  retain the background owner. It adds no seed queue, active-seed count,
  share-ratio goal, time goal, upload quota, tracker policy, or automatic
  torrent stop.
- Background disable/idle does not archive, remove, clear errors, change file
  priorities, move queue positions, release SAF grants, delete payload, or
  reset the profile.
- Tactical `199` network eligibility and this product-lifetime eligibility are
  conjunctive. Unmetered waiting can retain an admitted background owner, but
  it cannot send network traffic until the live network prerequisite allows
  it. Product shutdown closes both.
- A configured background preference cannot broaden fixed Offline or
  LoopbackOnly policy and cannot bypass notification ineligibility.
- Tactical `165` remains the sole wake-lock policy. Starting, Downloading, and
  Checking may hold its default-on partial CPU lock; unmetered waiting,
  seeding, companion-only use, idle grace, and the background setting alone do
  not. No Wi-Fi lock returns.
- Doze, App Standby, battery saver, thermal action, OEM process management,
  user Task Manager Stop, and force-stop may interrupt work. The product
  preserves recoverable intent but does not claim to defeat the system.

## Ownership, Tasks, Cancellation, And Dependency Direction

```text
MainActivity started/stopped + bounded Android result workflows
  -> ProductInteractionLeaseRegistry

Tactical 198 NotificationEligibility
ProductLifecyclePreference
existing authoritative torrent-list snapshot/patch/reset
ChromeOS companion latest connection count
service start/stop/timeout facts + monotonic deadlines
  -> task-free ProductLifetimePolicy
     -> ProductLifetimeDecision + reason + next deadline
        -> ProductLifecycleCoordinator (one service-scope owner)
           -> foreground promote/demote
           -> Tactical 199 network admission on sticky recovery
           -> one nearest-deadline job
           -> existing joined ProductEngineService shutdown

ProductEngineService
  -> one AndroidApplicationClient
     -> one ApplicationService/profile/engine
     -> one optional ChromeOS companion server
  -> one AndroidPresentationRepository subscription
  -> notification, SAF, power, intake, and lifecycle coordinators
```

`ProductLifetimePolicy`, preference decoding, work classification, and
deadline replacement are plain Kotlin without `Context`, Compose, Binder,
coroutines, sockets, files, or Rust objects. Platform adapters depend inward
on them.

`ProductLifecycleCoordinator` owns all mutable lifetime state. It receives
small serialized facts on the existing service scope, advances one nonzero
revision, reduces synchronously, and keeps at most one job sleeping until the
nearest startup, visibility, resync, or companion deadline. An event burst
replaces that deadline; it does not launch a coroutine per state change.

The existing presentation repository remains the sole Android torrent-list
subscription. Extend its narrow service callback with snapshot/patch/reset
classification facts rather than opening a lifecycle subscription. It retains
no lifecycle policy. The existing application and gateway remain the torrent
and companion authorities; Kotlin owns only Android product lifetime.

The companion count watch is a narrow Android-platform boundary inside
`rstorrent-android`. It does not add a TypeScript/JSON view, portable command,
application event, daemon, socket proxy, or Android concept to the engine.
No libtorrent-facing module depends on product lifecycle.

Successful committed shutdown orders:

1. fence new interaction, preference, presentation, companion, and deadline
   facts for the retiring generation;
2. close Tactical `199`'s network prerequisite and unregister Android
   connectivity/notification observations;
3. cancel companion pairing/root/count work and close authenticated sessions
   plus the listener;
4. close the presentation subscription and cancel bounded SAF/intake workers;
5. join the application service, engine network/storage tasks, and platform
   requests;
6. release the partial wake lock and cancel owned notifications; and
7. remove foreground status, use the latest safe start ID to stop the service,
   and publish terminal diagnostics before scope cancellation where possible.

No step detaches a Rust task to satisfy an Android deadline. Tactical `198`'s
timeout implementation must already prove its shorter emergency path; this
tactical routes policy timeout through that same owner rather than creating
another shutdown sequence.

## Bounds And Race Contract

- One service, application client, application profile, lifecycle
  coordinator, torrent-list subscription, companion count watch, and nearest-
  deadline job exist at high water.
- Interaction leases are closed named slots for Activity, permission/settings,
  SAF, intake, companion approval, and companion root result. At most one of
  each incumbent workflow exists; there is no arbitrary token collection.
- The reducer retains counts/booleans plus one work classification, requested
  and effective state, nonzero revision, terminal reason, and up to four
  absolute monotonic deadlines. It retains no event history or torrent IDs.
- Startup handoff and authoritative initial state have a 30-second hard
  deadline. View reset recovery has five seconds. Visibility/work transition
  settle has two seconds. Companion reconnect grace is exactly 60 seconds.
- Ordinary joined lifecycle shutdown has a five-second controlled-test target.
  Timeout shutdown retains Tactical `198`'s stricter platform deadline. A
  missed target is a test failure and terminal degraded fact, never authority
  to detach a task or restart.
- Duplicate facts and repeated same-state preferences are no-ops. Revision
  overflow is a typed terminal shutdown rather than wraparound.
- A new visible start before shutdown commit cancels a replaceable settle
  deadline. After terminal shutdown commits, no callback can reopen that
  generation; the visible caller observes disconnect and starts one new
  service only after the old owner releases the profile.
- A newer Android start ID prevents an older settle result from calling
  `stopSelfResult` against it. It does not cancel an application shutdown that
  already committed; replacement waits for terminal release.
- Foreground promotion completes before an activity/workflow release can make
  background execution effective. Failure closes networking and stops.
- Foreground demotion occurs only after a current visible Activity lease, not
  merely a bind request or cached lifecycle flag.
- Completion racing queue promotion waits through the two-second latest-state
  boundary. Keep-seeding changes and user Pause/Resume are reduced from the
  latest authoritative projection; no earlier timer wins.
- Notification revocation/channel blocking racing companion or transfer work
  follows Tactical `198`: visible workflows may finish within their lease;
  absent visibility commits shutdown regardless of other reasons.
- Unmetered eligibility racing lifetime shutdown cannot reopen networking
  after the lifecycle fence. A sticky restart starts closed until both policies
  allow it.
- Process death loses all leases, deadlines, and connection counts. Only an
  OS sticky restart from a formerly admitted background generation may
  reconstruct; it must re-read every durable/platform fact.

## Current RSTorrent Findings

The implementation begins from a strong joined owner but no product lifetime
policy:

- `ProductEngineService.onCreate` immediately creates foreground status,
  opens the profile/application, starts presentation and optional ChromeOS
  owners, and never demotes while Compose is visible.
- `onStartCommand` returns `START_STICKY` for ordinary start, companion enable,
  external intake, and even the asynchronous Stop path. There is no admitted-
  background versus terminal generation distinction.
- `MainActivity.onStart/onStop` binds and unbinds the service but does not
  publish visible interaction or decide product lifetime. Every normal route
  calls `startForegroundService` first.
- `ProductEngineService.shutdown` already has one atomic guard and joins the
  presentation owner, companion jobs, SAF workers, Android client,
  application service, and wake lock. It is the correct terminal path to
  extend rather than replacing it with process kill or Pause All.
- `AndroidPresentationRepository` supplies one authoritative bounded torrent-
  list stream and `ProductState` contains generated `operational_state`,
  progress, desired state, storage state, and queue facts sufficient for a
  pure work classifier.
- Tactical `165` already owns the app-private default-on power setting and one
  non-reference-counted partial wake lock over Starting, Downloading, and
  Checking. It correctly excludes queued work and seeding.
- Tactical `194` persists only whether ChromeOS companion support was enabled.
  Once enabled on ChromeOS, each service open starts the ARC listener and
  polling jobs. No current Kotlin/UniFFI boundary exposes authenticated active
  connection count, although the gateway already owns bounded active
  connection metrics.
- the manifest has no boot/package-replaced receiver or
  `RECEIVE_BOOT_COMPLETED`, which is the accepted first-release behavior;
- the Power Management page exposes only sleep inhibition and an unavailable
  Battery policy row; and
- there is no background-download/seeding preference, activity/workflow lease
  registry, work classifier, lifecycle timer, foreground demotion, task-
  removal policy, or process-recovery qualification.

Tactical `198` now supplies notification eligibility, visible-interaction
transparency, generic ongoing status, and prompt API-35 timeout shutdown.
Tactical `199` now supplies the atomic initial/live network prerequisite and
joined generation convergence. This tactical consumes and completes those
seams; it does not create competing permission, notification, connectivity,
or shutdown owners.

The concrete boundary improvement is one explicit Android product-lifetime
reducer and coordinator around the incumbent service. Engine/application
state remains authoritative inward; Android lifecycle and notification facts
remain platform-owned outward.

## Reference Inspection

### Maintained JSTorrent Android product

The maintained sibling JSTorrent checkout was inspected at exact revision
`25e4b701433fd815398ba89526546f5e4f072e3f` on 2026-08-30:

- `android/app/src/main/java/com/jstorrent/app/settings/SettingsStore.kt`
  defaults `background_downloads_enabled` false, `when_downloads_complete` to
  `stop_and_close`, low-battery shutdown false/15%, and companion auto-close
  false/30 minutes;
- `android/app/src/main/java/com/jstorrent/app/service/ServiceLifecycleManager.kt`
  counts started activities, qualifies downloading/metadata/checking plus
  separately selected seeding, starts foreground work only after the app
  leaves, stops idle or disallowed background engines, restores on foreground
  return, and fences explicit user quit;
- `android/app/src/main/java/com/jstorrent/app/NativeStandaloneActivity.kt`
  reports start/stop, explains notification permission on first launch, and
  enables background downloads only when the user accepts that combined
  background/completion explanation;
- `android/app/src/main/java/com/jstorrent/app/viewmodel/SettingsViewModel.kt`
  rejects background enablement without notification permission and disables
  it when that permission is revoked;
- `android/app/src/main/java/com/jstorrent/app/ui/screens/PowerManagementSettingsScreen.kt`
  presents background download, CPU wake-lock, low-battery, and keep-seeding
  choices and warns before keep-seeding;
- `android/app/src/main/java/com/jstorrent/app/service/ForegroundNotificationService.kt`
  normally returns sticky, handles the start-foreground race, releases wake
  locks when only seeding, and implements low-battery shutdown by Pause All;
- `android/app/src/main/java/com/jstorrent/app/service/IoDaemonService.kt`
  is the separate legacy companion daemon with foreground mode and optional
  disconnected 5--120 minute auto-close; and
- `android/app/src/test/java/com/jstorrent/app/service/ServiceLifecycleManagerTest.kt`,
  `android/app/src/androidTest/java/com/jstorrent/app/service/ServiceLifecycleTest.kt`,
  and
  `android/app/src/androidTest/java/com/jstorrent/app/service/BackgroundServiceLazyEngineTest.kt`
  cover playback leases, default/persisted completion mode, activity facts,
  empty/disabled shutdown, active-work retention, and cache-led lazy recovery.

RSTorrent adopts the standalone background opt-in, notification prerequisite,
active-work classification, default completion shutdown, separate
keep-seeding choice, foreground recovery, sticky active-work recovery, and
explicit quit fence. It independently authors stronger deterministic race,
terminal-zero, and real-transfer evidence rather than treating JSTorrent's
several delay- or state-presence-only instrumentation cases as proof.

Intentional differences are required by accepted RSTorrent architecture and
earlier evidence:

- one service owns the Rust application/engine instead of coordinating a
  process-global JavaScript engine with a separate foreground wrapper;
- generic notification permission does not silently enable background work;
  only the explicit background action may complete that transaction;
- Tactical `165`'s default-on CPU sleep inhibition remains separate and no
  deprecated Wi-Fi lock returns;
- low-battery Pause All is not adopted because it rewrites torrent intent and
  the feature was explicitly deferred;
- native playback has no Android presentation and supplies no lifetime lease;
- the selected semantic ChromeOS companion is not the raw-I/O daemon and uses
  a fixed reconnect grace rather than a first-release configurable idle owner;
  and
- Android 15 timeout handling and no-boot behavior are explicit instead of an
  indefinite sticky claim.

No JSTorrent source, test, string, fixture, or asset is imported.

### Android platform contract

Official Android documentation was inspected on 2026-08-30:

- [Foreground service timeouts](https://developer.android.com/develop/background-work/services/fgs/timeout)
  limits target-35 `dataSync` foreground services to a cumulative six hours in
  the background per 24 hours, calls `Service.onTimeout`, requires `stopSelf`
  within a few seconds, rejects restart after exhausted quota, and documents
  shortened-timeout test controls;
- [Foreground-service background-start restrictions](https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start)
  prohibits ordinary background starts for target-31+ applications except
  bounded exemptions and permits user-visible transitions/actions;
- [`Service`](https://developer.android.com/reference/android/app/Service)
  defines `START_STICKY`, null-intent recreation, start IDs, foreground stop
  flags, and the distinction between started and bound service lifetime;
- [Notification runtime permission](https://developer.android.com/develop/ui/compose/notifications/notification-permission)
  confirms that foreground-service launch does not require
  `POST_NOTIFICATIONS`, but denied notices appear only in Android Task Manager
  rather than the notification drawer on Android 13+;
- [Handle user stopping foreground-service apps](https://developer.android.com/develop/background-work/services/fgs/handle-user-stopping)
  states that Android 13+ Task Manager Stop removes the entire process and
  activity stack without an app callback and identifies
  `ApplicationExitInfo.REASON_USER_REQUESTED` for later inspection; and
- [Stop a foreground service](https://developer.android.com/develop/background-work/services/fgs/stop-fgs)
  distinguishes stopping the service from removing foreground status while
  the service continues.

Android 15 also prohibits `dataSync` foreground-service launch from
`BOOT_COMPLETED`. The accepted product requires a user-visible start after
reboot, so it adds neither the receiver nor an alternative scheduler.

This is Android product/platform lifecycle integration, not BitTorrent wire,
discovery, scheduling, or storage behavior. No libtorrent inspection is
required, and no external source is used as an architecture template.

## Implementation Stages

1. After Tactical `198` fixes notification eligibility, add app-private
   lifecycle preference decoding/persistence, the task-free reducer, work
   classifier, deadline model, and exhaustive pure JVM tests. Do not change
   service lifetime from Compose callbacks directly.
2. Add the service-owned coordinator and closed interaction-lease registry.
   Feed it current notification facts and exact snapshot/patch/reset work
   classifications through the incumbent presentation owner.
3. Split direct startup, visible interactive, admitted background, stopping,
   and terminal start generations. Implement immediate foreground bootstrap,
   real-visibility demotion, promotion-before-release, start-ID fencing, one
   deadline job, and the existing joined stop path.
4. Add configured/effective Power Management controls, permission/settings
   transaction, keep-seeding warning, finite-background disclosure, and
   generic ongoing lifecycle reason. Keep Battery policy unavailable.
5. Add the count-only companion activity watch through `rstorrent-gateway` and
   `rstorrent-android`, one Kotlin consumer, launch/disconnect grace, and
   deterministic connection/reconnect/shutdown tests without a portable view.
6. After Tactical `199` supplies the live network prerequisite, implement
   sticky background recovery with initial network closure, authoritative
   qualification, generation fencing, and zero-egress idle refusal.
7. Prove Home, configuration, permission/SAF workflow, task removal, process
   death, explicit Stop, Task Manager Stop, force-stop, timeout, queue
   promotion, completion, seeding, notification loss, and concurrent relaunch
   cases with terminal resource assertions.
8. Build both Android ABIs and run installed API 28/API 35 AVD campaigns with
   controlled peer/tracker traffic, shortened quota, process/service
   inspection, exact preference/policy restoration, and no public swarm.
9. After explicit target authorization, run bounded current-phone screen-off/
   process-recovery evidence and the existing physical ChromeOS extension
   connection/disconnect/relaunch cohort. Do not alter or publish the
   production extension.
10. Reconcile this tactical and all owning topics. Close only `JAR-009`; leave
    low-battery, configurable companion idle, playback, reboot scheduling,
    production handoff, signing, and release publication open.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure lifetime reducer | Every matrix row; initial unknown; duplicate facts; preference persistence result; permission/channel change; activity/workflow lease replacement; snapshot reset/resync; completion/queue race; seed choice; companion count/grace; deadline revision; overflow; stop/timeout terminal precedence |
| Work classification | Starting, metadata, Downloading, Checking, unmetered wait, Queued, Paused, Error, repair, removal, archived, Seeding off/on, mixed torrents, all-complete, empty library; no rate/byte inference |
| Service transitions | Foreground bootstrap; visible demotion; promotion before activity release; Home and configuration change; bounded result workflow; idle join; current start ID; relaunch before/after commit; initialization failure; terminal zero |
| Persistence and recovery | Both defaults; enable grant/deny/settings return; revocation; channel block/repair; failed commits; visible reopen; sticky null intent; killed active download/check/seed; no idle egress; same profile/intent/verified state |
| Companion | Initial zero; authenticated one/multiple connections; count-only privacy; disconnect grace; reconnect cancellation; enabled idle listener; pending pairing/root workflow; notification loss; concurrent transfer; exact listener/session cleanup |
| Power and network | Background versus wake-lock independence; no seeding/companion/wait grace lock; unmetered wait retained with zero egress; eligible recovery; lifetime shutdown closes network gate; no Wi-Fi lock |
| Android platform | Task removal, `cmd activity stop-app`, force-stop, no callback assumption, next-launch exit reason, API-35 shortened `dataSync` timeout, quota-exhausted start rejection, no sticky restart, no boot/package receiver |
| Presentation | Power settings, configured/effective state, permission/settings routing, warning, generic ongoing reasons, finite disclosure, visible/offline behavior, accessibility and rotation |
| Repository | Kotlin format/compile/lint/unit/instrumentation, Rust boundary tests, generated UniFFI check, both Android ABIs, unchanged portable view/command schema, no new dependency/license/source import |

### Deterministic and controlled runtime cases

At minimum, automated cases prove:

- direct visible launch creates one service/application, satisfies foreground
  startup, then demotes only after the real Activity lease;
- Home with the default-off setting and one active controlled download closes
  peer/tracker/DHT/listener/SAF/application owners through joined shutdown while
  retaining exact desired intent and verified pieces;
- foreground reopen restores the same torrent and continues without a Resume
  command or duplicate engine;
- background enabled plus notification eligibility retains metadata,
  download, and checking states; queued-only, paused, error, repair, and empty
  state stop;
- one last completion either promotes the next queued torrent within the
  settle boundary or joins when none qualifies;
- keep-seeding off joins after completion, while keep-seeding on retains one
  desired-running seed and serves a controlled leecher; disabling it joins
  without pausing;
- unmetered waiting retains only an admitted finite background owner, produces
  no controlled endpoint packets while blocked, and resumes on eligibility;
- notification denial/revocation/channel block, Stop, timeout, and idle races
  select one terminal shutdown and cannot be defeated by a later stale torrent
  or companion fact;
- a system settings or SAF picker round trip retains one bounded workflow,
  configuration replacement does not stop, and abandoned workflows expire;
- an active companion with background downloads off retains the owner;
  disconnect starts 60 seconds, reconnect cancels it, and expiry stops only if
  no torrent or visible reason has appeared;
- sticky recovery of a killed admitted background generation starts egress
  closed, confirms authoritative work, then resumes exact progress;
- a sticky restart with now-ineligible notifications or no qualifying state
  performs zero BitTorrent endpoint traffic and stops;
- task removal preserves eligible work, whereas explicit Stop and shortened
  timeout leave no restart; and
- every path reaches zero service, application, listener, connection,
  subscription, callback, deadline, SAF worker, socket, mapping, wake-lock,
  notification, and queued-byte ownership where it is required to stop.

### Installed AVD campaign

Use explicitly owned API 28 and API 35 AVDs. Begin with command-driven package,
activity, notification, service, process, app-op, device-config, and network
inspection before any manual presentation check.

The campaign records:

- exact API/ABI, target SDK, application ID, APK digest, lifecycle preferences,
  notification permission/channel state, start ID/generation, PID, service
  foreground status, activity/task state, and redacted lifetime reason;
- default-off Home/return with a genuine tiny controlled transfer and exact
  retained-progress/payload hash;
- enabled Home, screen-off, task removal, process kill/sticky recovery,
  completion shutdown, keep-seeding controlled upload, and visible reopen;
- permission grant/deny/revoke, Background channel block/restore, settings and
  SAF result workflows, rotation/recreation, rapid Home/return, Stop, and
  concurrent external intake;
- Tactical `199` eligible/metered/wait transitions with controlled TCP/UDP
  endpoint counters and no optimistic sticky-restart traffic;
- shortened target-35 `dataSync` timeout and exhausted restart refusal using
  official device-config controls, including prompt stop and no ANR;
- task removal versus `adb shell cmd activity stop-app`, force-stop, package
  restart, reboot/no-autostart observation where the AVD supports it; and
- restoration of device-config, notification/app-op, network, process, task,
  package data, fixtures, captures, and AVD ownership with terminal zero.

No public swarm or uncontrolled payload is required. Do not infer background
duration from a short smoke or claim OEM behavior from AVD evidence.

### Physical phone and ChromeOS campaigns

Physical work requires explicit maintainer authorization and claimed targets.
Use a tiny controlled private fixture and retain the existing Machine Control
platform instructions.

On a current Android phone, prove default-off Home shutdown/reopen, opted-in
screen-off transfer, system sleep/Doze observation under Tactical `165`'s
setting, task removal, controlled process kill/recovery, notification Stop,
completion shutdown, optional keep-seeding upload, and exact cleanup. Record
observed behavior without claiming immunity to OEM power management.

On the existing ChromeOS product path, prove Android-visible pairing, one
authenticated connection with background downloads off, switch to Chrome,
disconnect/reconnect inside 60 seconds, disconnect expiry when idle, detached
transfer retention when background downloads are enabled, completion/seed
policy, extension relaunch after service stop, notification ineligibility,
explicit Stop, and terminal ARC-listener refusal. Preserve Tactical `194`'s
same-device and same-LAN security evidence; do not change extension identity,
package, permissions, store state, or publication.

Both campaigns remove controlled payload, package state where required,
notifications, tasks, processes, listeners, pairings created for the test,
captures, policy mutations, and machine claims.

The closing campaign used the explicitly accepted fallback because the
approved Chromebook was not safely reachable. It composes Tactical `194`'s
physical ARC-only bind, authenticated extension connection, same-LAN refusal,
detached transfer, reconnect, stop, and cleanup evidence with this tactical's
deterministic authenticated-count/reconnect-grace transitions and installed
API 28/API 35 Android lifecycle campaigns. It does not relabel emulator
evidence as physical evidence or add a fresh ChromeOS/OEM duration claim.

After that close, maintainer direction explicitly retried the now-available
Chromebook and authorized the physical strengthening campaign recorded below.
The accepted fallback remains the historical basis on which the tactical was
first closed; the later campaign adds direct device evidence rather than
rewriting that history.

### Build and repository baseline

Run from the repository root after sourcing the configured profile:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
(
  cd clients/android
  ./gradlew lintDebug testDebugUnitTest assembleDebug assembleDebugAndroidTest
)
./clients/android/build.sh
```

The portable application view/command schema should remain unchanged. The
focused companion count addition still regenerates and compiles the Android
UniFFI boundary and both native ABIs. If implementation instead requires a
portable view field, stop and justify that broader contract before changing
it.

## Documentation And Completion Updates

Before marking this tactical complete:

- record exact commits, JSTorrent paths/revision, Android source pages, tests,
  commands, AVD/device classes, API/ABI/package digests, preferences,
  notification/service/task/process transitions, controlled transfer/upload,
  timeout configuration, resource high waters, failures, and cleanup;
- mark `JAR-009` complete in
  [`android-jstorrent-replacement.md`](../topics/android-jstorrent-replacement.md);
- update granted-background and notification truth in
  [`beta-release-readiness.md`](../topics/beta-release-readiness.md);
- update platform-owned shutdown/intent semantics in
  [`application-control.md`](../topics/application-control.md);
- update Android and ChromeOS lifecycle truth in
  [`client-surfaces.md`](../topics/client-surfaces.md); and
- update the ready set and Android capability row in
  [`capability-readiness.md`](../topics/capability-readiness.md).

Leave production identity/migration `JAR-004`, extension rollout `JAR-005`,
signed Play qualification `JAR-010`, low-battery shutdown, configurable
companion idle, playback, seed goals/limits, VPN, proxy, search/plugins,
localization, and publication open under their existing owners.

## Escalation Contract

Implementation may add the pure Kotlin policy/preferences, closed interaction
leases, service coordinator, start-generation and deadline state, focused
companion count watch, Power Management controls, foreground promotion/
demotion, fail-closed sticky admission, deterministic fault/time hooks,
privacy-preserving diagnostics, and internal refactors required to separate
the current combined power/notification collector.

Stop for maintainer direction if evidence requires:

- changing either default, the qualifying-work set, 60-second companion
  grace, two-second settle interval, or explicit notification prerequisite;
- adding WorkManager, JobScheduler, user-initiated data-transfer jobs, alarms,
  another service/process/daemon, a different foreground-service type, boot or
  package-replaced startup, background-start exemption, battery-optimization
  request, or OEM-specific workaround;
- persisting a lifecycle-active/cache ledger, reading the Rust profile from
  Kotlin, rewriting torrent desired intent, or adding a portable lifecycle
  command that Chrome/web clients can use to bypass Android policy;
- implementing low-battery shutdown, native playback lifetime, ratio/time
  seed goals, active-seed admission, or configurable companion auto-close;
- retaining an invisible owner after Tactical `198` notification
  ineligibility or claiming duration beyond Android's documented allowance;
- changing production application/extension identity, migration, signing,
  Play declarations, store state, or publishing any artifact; or
- using a physical device, cellular/public swarm traffic, unapproved ChromeOS
  target, or external publication without the stated authorization.

An ordinary Kotlin, Rust boundary, Compose, service, notification, timeout,
AVD, controlled-transfer, process-recovery, build, or cleanup failure is not
an escalation. Diagnose it within the declared owner and bounds.

## Implementation And Evidence Record

The repository slice landed as these reviewable commits:

- `28af8f8` activated the bounded tactical and readiness entry;
- `78a8093` added the pure task-free lifetime reducer, authoritative work
  classifier, preference values, limits, and exhaustive JVM matrix;
- `7af2d2a` exposed only the current authenticated companion connection count
  through a latest-value, cancellation-aware Android UniFFI subscription;
- `7490be1` added one serialized service coordinator and replaceable monotonic
  deadline owner;
- `890164e` integrated activity/workflow leases, notification eligibility,
  authoritative torrent work, companion grace, foreground handoff, sticky
  admission, fail-closed network recovery, and joined terminal shutdown;
- `4b9ba5b` added the default-off Power Management controls, effective-state
  truth, notification routing, finite-duration disclosure, and keep-seeding
  warning;
- `19a8b6e` added the installed controlled-transfer campaign, connected
  visibility/terminal test, and terminal foreground-ordering repair;
- `7112331` added the persistent Android-15 exhausted-quota fence and the real
  shortened-timeout/restart-refusal campaign;
- `ed8282a` added exact API-35 recent-task removal while admitted background
  work remains owned;
- `3a69230` enabled the bounded physical ChromeOS strengthening campaign;
- `1f8cabd` selected Machine Control's ready ARCVM ADB route;
- `1e72aa2` made repeated foreground starts acknowledge the current start ID
  without an Android ANR;
- `62b757b` preserved quoted ADB shell arguments through the ChromeOS route;
- `c2b14e7` bridged the Chromebook-host upload forward to the controlling
  machine for the controlled seeding proof;
- `4414e7a` made an already-created Activity start a new service generation
  before binding after a joined shutdown; and
- `d1fdc17` made the source companion UI discard stale inspection state and
  expose an explicit retry after socket disconnect.

`ProductLifetimePolicy` is the closed deterministic reducer.
`ProductLifecycleCoordinator` is the only deadline/revision serializer.
`ProductEngineService` remains the sole application/profile/engine,
notification, wake-lock, network-prerequisite, SAF-worker, and companion
lifetime owner. `MainActivity` publishes genuine visible state and bounded
result-workflow leases; it does not issue torrent lifecycle commands.
`CompanionPairingOwner` publishes a count without identities or credentials,
and the Android adapter owns one closable latest-value subscription. No
service, daemon, worker scheduler, reboot receiver, portable lifecycle
command, generated application view field, new dependency, or source import
was added.

The API-35 quota campaign exposed a material edge beyond the initial direct
`onTimeout` test: a prohibited post-timeout `startForegroundService` could be
accepted before foreground promotion and leave a crashed service record.
`ProductDataSyncQuotaFence` now commits the exhausted edge before joined
shutdown, refuses non-visible recreation before foreground creation, and is
cleared only by a later visible Activity launch. The final installed campaign
observed the real platform callback at 1,000 ms, joined shutdown, an accepted
start request blocked by that fence, no live service, and no ongoing
notification. The original `device_config` value was `null` and was restored
to `null` in `finally`.

Deterministic evidence includes 16 `ProductLifetimePolicyTest` cases, three
coordinator/deadline cases, the existing notification/network/service tests,
gateway authenticated-count transitions, and the Android latest-value
subscription close test. The final connected suites passed 20/20 tests on API
28 and 22/22 on API 35. The API-35 suite includes direct timeout convergence;
the installed profile additionally exercises the real system timeout.

The final owned-AVD controlled campaigns used package
`org.rstorrent.bootstrap`, target SDK 35, arm64-v8a Google API images, one
private five-piece SAF fixture, and no public swarm:

- API 28 (`generic_arm64`, Android 9 fingerprint
  `Android/sdk_gphone_arm64/generic_arm64:9/PSR1.210301.009.B6/9767327:userdebug/dev-keys`)
  proved default-off Home shutdown, exact foreground reopen at 1/1 verified
  piece or better, opted-in background foreground-service ownership, killed-
  process sticky recovery, completion shutdown, a 133,304-byte controlled
  background upload, seeding-disable shutdown, exact payload hashes, and
  cleanup. Descriptor baseline/high/final was 75/75/75; SAF ownership peaked
  at 6/40 handles and 1/16 pending requests.
- API 35 (`emu64a`, Android 15 fingerprint
  `google/sdk_gphone64_arm64/emu64a:15/AE3A.240806.043/12960925:userdebug/dev-keys`)
  proved the same cohort at 1/1 retained verified piece, plus exact recent-
  task removal with `background_admitted=true`, real shortened quota and
  fenced restart refusal. It uploaded 133,304 controlled bytes, retained
  exact payload hashes, peaked at 6/40 SAF handles and 1/16 pending requests,
  and recorded descriptor baseline/high/final 148/148/118 before exact
  cleanup.

The final debug APK is 111,125,527 bytes with SHA-256
`64f462a895dd7bdcd3cbb9c66f91fb6369ff8a37aefd562ec04193917bb40209`.
It contains `librstorrent_android.so` for arm64-v8a (23,727,744 bytes) and
x86_64 (26,700,104 bytes). Both task-owned AVD definitions, package/process
state, SAF fixture tree, reverse transport, controlled payload, notification,
quota override, and host fixture were removed. An attached phone was left
untouched.

The final repository baseline passed:

```text
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web       # 365 passed, 2 skipped
./gradlew lintDebug testDebugUnitTest assembleDebug assembleDebugAndroidTest
./clients/android/build.sh               # x86_64 and arm64-v8a
python3 -m py_compile clients/android/run_bootstrap.py
```

The generated portable web contract remained unchanged. Android's existing
deprecated Activity-result/theme warnings and the existing Android-target
`rstorrent-platform` dead-code warnings remain pre-existing output; lint,
clippy, and every commanded gate passed.

On 2026-08-31, after ChromeOS access was explicitly authorized, the required
common read-only Machine Control doctor reported `ready=false`: SSH
administration was unreachable, the profile session was locked, resident and
semantic prerequisites were unavailable, and physical VT2 recovery was the
only outer route. The doctor deliberately did not probe ARCVM ADB, and no
device, profile, extension, package, power policy, or target state was
mutated. Maintainer direction then accepted the already-passing AVD campaign
as the Android fallback. Closure therefore uses three independently bounded
layers: Tactical `194`'s physical ChromeOS same-device/security campaign,
Tactical `200`'s deterministic companion count/grace/shutdown coverage, and
Tactical `200`'s installed API 28/API 35 lifecycle, transfer, recovery,
seeding, timeout, and cleanup evidence. This is compositional qualification,
not a claim that the locked Chromebook reran the Tactical `200` matrix.

### Later Physical ChromeOS Strengthening

The same day, after physical VT2 SSH recovery and profile unlock, the common
Machine Control doctor reported the testbed ready. The authorized target was
the physical x86_64 ChromeOS 150 ARCVM fingerprint
`google/nami/nami_cheets:13/R150-16700.62.0/16031715:user/release-keys`, Android
API 33. The installed final debug APK was 111,125,527 bytes with SHA-256
`4ea0245a4a00982502b839bfffeb97022a78b8489640ac32585d62173480f3c6`.
It retained package `org.rstorrent.bootstrap`, target SDK 35, and both packaged
x86_64 and arm64-v8a native libraries. API-35 quota callbacks are inapplicable
to the API-33 ARCVM and remain covered by the installed API-35 AVD campaign.

The physical product harness passed default-off Home shutdown and exact
foreground reopen, explicitly admitted background ownership, sticky process
recovery, completion shutdown, one 133,304-byte controlled background upload,
continued-seeding disable shutdown, exact payload hashes, and terminal
cleanup. SAF ownership peaked at 6/40 handles and 1/16 pending requests;
descriptor baseline/high/final was 130/130/142 before uninstall. The focused
two-generation instrumentation regression
`ProductBackgroundLifecycleTest#visibleStartCreatesANewGenerationAfterJoinedShutdown`
passed on ARCVM in 0.342 seconds.

The retained JSTorrent Beta extension stayed at version 0.4.0 and exact ID
`gcgoepclopkgijmclmlheafaglmbjlcc`; it was not redeployed, reloaded, granted a
new host permission, or published. A fresh extension launch reused the saved
pairing without another Android approval and showed Android/profile-default/
protocol-1 identity. With background downloads off, one isolated authenticated
WebSocket from that retained extension origin and credential produced
`retain_chromeos_companion`; closing it produced the fixed 60-second
`retain_chromeos_reconnect_grace`, reconnecting after two seconds canceled the
deadline, and a later idle disconnect expired into joined shutdown. This
narrow socket hold avoided duplicate retained companion tabs and the installed
pre-fix page's stale disconnected presentation while exercising the real
extension identity, persisted pairing, ARC endpoint, authentication, and
service count owner. The source-only `d1fdc17` fix passed web/extension gates
but was deliberately not deployed to the retained physical extension.

The denied-notification branch allowed only visible use and then selected
`stop_notification_ineligible`; the foreground service, process, notification,
and ARC listener terminated. The permission was then revoked and granted
through the real Compose explanation plus Android **Allow** dialog. With an
authenticated client in the background, ChromeOS displayed the ongoing
**RSTorrent — ChromeOS client connected** notification. Its real **Stop**
action selected `stop_explicit_stop`, completed the joined application-client
shutdown, removed the service and notification, and left the ARC endpoint
refusing connections. A subsequent extension launch created a new service
generation, reused the pairing without approval, and restored the same
identity, proving relaunch after service stop.

Campaign cleanup removed the extension credential created for the test and
its companion tab, uninstalled the app and test packages with their data,
removed notifications, tasks, services, listeners, device-side XML, controlled
payload, and local captures, and restored default ChromeOS idle/lid suspend
policy. The retained extension installation and unrelated ChromeOS state were
left unchanged. No phone, public swarm, extension/store identity, permission,
or publication state was touched.

## Restart Checkpoint And Next Action

This tactical and `JAR-009` are complete through the original compositional
close plus the later physical ChromeOS strengthening campaign above. Do not
reopen policy, defaults, the work classifier, persistence shape, foreground
owner, quota fence, or the generated application boundary absent new contrary
evidence. This remains a bounded ChromeOS/API-33 observation, not an
indefinite-duration or general OEM guarantee.

Next keep Tactical `198`'s exact physical completion/repair notification-tap
gate and Tactical `199`'s physical-phone handoff under their own owners, then
prioritize production handoff/reset-support work or the separately bounded
Android playback presentation. VPN and proxy remain explicit post-release
candidates rather than an implied continuation.
