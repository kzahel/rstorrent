# Tactical 165: Cross-Platform Active-Download Sleep Inhibition

Status: **Complete (2026-08-25).** Desktop signed packaging/updater Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) resumes as the sole
**Now** with its release outcome unchanged.

Topics: `beta-release-readiness`, `client-surfaces`, `client-persistence`,
`product-state-and-feedback`, `application-view-api`, `web-ui-design`

Dependencies: the maintained in-process application service and authoritative
TorrentList view, completed desktop lifecycle Tactical
[`162`](162-desktop-single-instance-and-tray-lifecycle.md), completed Android
product Tactical [`117`](117-jstorrent-shaped-android-product-ui.md), and
completed iOS lifecycle Tactical
[`149`](149-ios-lifecycle-recovery-and-distribution-readiness.md).

## Decision And Desired Outcome

Prevent automatic system sleep while RSTorrent is doing active download or
verification work, without keeping the display lit and without pretending the
application can veto an explicit user sleep, lid close, critical battery,
thermal action, Android restriction, or iOS suspension.

Fresh desktop and Android installations default **Prevent sleep during active
downloads and checks** on. The setting is platform-shell policy rather than a
torrent, profile, generated application-command, or browser preference. The
authoritative TorrentList `operational_state` controls the level-triggered
request:

- `Starting`, `Downloading`, and `Checking` require inhibition. This includes
  metadata discovery, stalled/no-peer active intent, storage work, and final
  publication while their application task remains active.
- `Queued`, `Paused`, `Seeding`, `Error`, and `Stopping` do not. Seeding alone
  does not prevent sleep in this MVP.
- disabling the preference, losing/resetting the authoritative view, or joined
  product shutdown releases the request immediately.

One or more qualifying torrents still produce only one platform request. The
first authoritative snapshot establishes the current level and may acquire an
inhibitor immediately; unlike Tactical `164` notifications, this policy is not
edge output and therefore must restore active protection after relaunch.

Desktop uses exact `keepawake` `0.6.1` on macOS and Windows with `idle=true`,
`display=false`, and `sleep=false`. Linux uses the GNOME SessionManager
suspend inhibitor when that session owner is present and the standard XDG
Desktop Portal `org.freedesktop.portal.Inhibit` suspend flag elsewhere. The
crate's Linux `idle` implementation is a systemd-logind idle inhibitor, while
GNOME's automatic suspend is session policy and does not consume that lock.
Using the crate's display inhibitor would also suppress normal display
blanking, and its explicit-sleep inhibitor would be broader than the product
promise.

Android retains its foreground service and uses one non-reference-counted
`PowerManager.PARTIAL_WAKE_LOCK`, now gated by the same authoritative
operational states and a durable default-on Android setting. It removes the
existing Wi-Fi performance lock: API 34 deprecated `WIFI_MODE_FULL_HIGH_PERF`
and automatically substitutes the low-latency mode, whose documented
foreground/screen-on restrictions do not provide the promised screen-off
background-download ownership. Active network traffic and the foreground
service remain ordinary Android networking behavior.

iOS adds no keep-awake setting or assertion. `UIApplication.isIdleTimerDisabled`
keeps the display awake and Apple limits it to foreground presentation uses;
it does not create general background execution. Tactical `149` remains the
truthful owner of finite UIKit and iOS 26 continued-processing opportunities,
expiration, checkpointing, suspension, and resume. This slice instead proves
that no misleading power control appears and that the finite lifecycle remains
intact.

## Scope And Stopping Condition

This tactical owns:

1. exact `keepawake = 0.6.1` dependency selection and license review,
   restricted to macOS and Windows;
2. one Linux session suspend-inhibition adapter over the selected D-Bus stack,
   using GNOME SessionManager when available and a race-safe XDG portal
   request elsewhere, with explicit release and connection-drop cleanup;
3. a pure desktop reducer over authoritative TorrentList snapshots, patches,
   removals, and reset boundaries;
4. one joined desktop owner for its application subscription, setting changes,
   platform worker, inhibitor acquisition/release, cancellation, and observed
   termination;
5. a version-3 `desktop-shell.json` migration preserving exact version-1
   background policy and version-2 notification policy while adding the
   default-on power preference;
6. a capability-gated Tauri-only **Power** Settings category with immediate
   durable replacement, rollback, and visible failure feedback;
7. an Android-only durable preference, Compose switch, pure eligibility
   policy, one partial wake lock, runtime toggling, and exact release on
   completion, pause, queueing, failure, disablement, and service shutdown;
8. an explicit iOS inapplicability record and absence test rather than a
   display-awake or indefinite-background implementation;
9. deterministic Rust, React, and Kotlin tests, package/static validators, and
   proportional credential-free platform gates; and
10. guest-resident installed macOS, Windows, and Linux evidence plus attached
    physical Android and iOS sanity evidence through `~/code/machine-control`.

The tactical stops when each supported desktop package acquires exactly the
intended system-idle/suspend inhibitor for one controlled active torrent,
retains it through a no-peer stall and hidden-window operation, releases it
for every nonqualifying state and Quit, persists and honors its setting across
restart, and never holds the display awake. Android must show the same setting
and partial-wake-lock lifecycle on the attached device without a Wi-Fi lock.
iOS must retain finite-background behavior without exposing or holding a
general keep-awake control. Every run must clean its application/process,
device artifacts, inhibitor, and machine-control claim.

## Non-Goals

- Preventing display sleep, dimming, screen lock, explicit sleep, shutdown,
  logout, lid-close sleep, low-battery/thermal action, or user force.
- Waking a sleeping machine, scheduling wake timers, starting at login, crash
  relaunch, or continuing after a process is terminated.
- Preventing sleep while only seeding, queued, paused, awaiting user repair,
  or displaying the application.
- Battery thresholds, AC-only policy, metered/VPN policy, adaptive behavior,
  per-torrent power controls, a sleep countdown, or power telemetry.
- A portable engine setting, application-contract command, browser wake-lock
  promise, remote-client power authority, or shared cross-device preference.
- General iOS background execution, entitlement misuse, display idle-timer
  disabling, or expansion of Tactical `149`.
- Publishing, tagging, signing-account changes, store/TestFlight activity, or
  production routing.

## Reference Inspection

### `keepawake` 0.6.1

The exact MIT-licensed crates.io source was inspected on 2026-08-25:

- `src/lib.rs` makes an owned guard from `Builder` and releases on drop;
- `src/sys/windows.rs` uses thread-scoped
  `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` for `idle` and
  restores the prior thread state on drop;
- `src/sys/macos.rs` creates and releases a
  `PreventUserIdleSystemSleep` IOKit assertion for `idle`; and
- `src/sys/linux.rs` maps `idle` to
  `org.freedesktop.login1.Manager.Inhibit("idle", ..., "block")`, `display`
  to `org.freedesktop.ScreenSaver.Inhibit`, and `sleep` to the broader logind
  sleep inhibitor.

The package's current CI and source support all three desktop families. Local
pre-tactical probes observed the intended Mac assertion and Windows SYSTEM
request, then observed both disappear when their guard was dropped. The
Windows API is per-thread, so acquisition and release must occur on the same
dedicated OS thread rather than an async task that may migrate.

### Linux desktop session

The XDG Desktop Portal Inhibit interface version 3 returns a Request handle,
defines flag `4` as Suspend and flag `8` as Idle, accepts a user-visible
reason, and releases inhibition through `org.freedesktop.portal.Request.Close`.
The standard frontend delegates to the desktop backend. GNOME's session
interface separately defines suspend flag `4` and identifies itself as the
private interface used by the portal.

Installed Ubuntu 24.04/GNOME 46 source and runtime inspection found automatic
suspend governed by the GNOME session inhibitor bitmask, not the keepawake
crate's logind `idle` file descriptor. The first installed probe also found
that the GNOME portal backend rejects an unsigned development bundle whose
desktop application identity is not installed in the backend's native app
registry. The final adapter therefore calls the exact GNOME SessionManager
interface when its session-bus owner exists and otherwise uses the portable
portal. The portal path subscribes to the exact request response before the
call, supplies a unique handle token, requires the expected returned handle
and success code within five seconds, and closes the request on failure or
drop. Missing services or request failure remain nonfatal and observable; they
never change torrent intent.

### Android and maintained JSTorrent

Android's official `PowerManager.PARTIAL_WAKE_LOCK` contract keeps the CPU
running while allowing the screen and keyboard backlights to turn off,
requires `WAKE_LOCK`, and requires prompt release because of battery cost.
Current Android guidance distinguishes the foreground service lifecycle from
the partial lock and recommends holding the latter only for the CPU-active
duration. API 34 deprecates `WIFI_MODE_FULL_HIGH_PERF` because of power cost
and substitutes low-latency behavior with narrower applicability.

RSTorrent already has an unconditional state-driven partial and Wi-Fi lock in
`clients/android/app/src/main/java/org/rstorrent/bootstrap/ProductEngineService.kt`.
The maintained JSTorrent checkout at revision
`9598770baecb1164a00ba5d41f7e7c11bfb78828` was also inspected:

- `android/app/src/main/java/com/jstorrent/app/service/ForegroundNotificationService.kt`
  gates CPU/Wi-Fi locks on downloading, metadata, or checking, releases for
  seeding, and supports immediate runtime toggling;
- `android/app/src/main/java/com/jstorrent/app/settings/SettingsStore.kt`
  persists an Android-only opt-in; and
- `android/app/src/main/java/com/jstorrent/app/ui/screens/PowerManagementSettingsScreen.kt`
  presents the switch under Power Management.

RSTorrent adopts the useful platform owner, active-only gating, runtime switch,
and seeding exclusion. It intentionally uses RSTorrent's authoritative typed
operational state, defaults on to preserve current product behavior, and does
not copy JSTorrent source or retain its deprecated Wi-Fi lock.

### Apple iOS

Apple documents `UIApplication.isIdleTimerDisabled` as a foreground display
facility for experiences such as games, maps, or content that must remain
visible, and says most applications should allow the display to turn off.
Apple separately states that applications are normally suspended shortly
after entering the background and must use applicable finite or special-
purpose background mechanisms. Tactical `149` already implements the relevant
finite UIKit and iOS 26 continued-processing boundaries. No iOS source,
fixture, entitlement, or sample is imported.

This is platform integration, not a BitTorrent engine or protocol feature, so
there is no libtorrent completeness-oracle requirement.

## Ownership, Task, And Dependency Direction

```text
ApplicationService authoritative TorrentList subscription
  -> pure desktop active-work reducer
  -> current persisted desktop power preference
  -> coalesced desired bool
  -> one dedicated platform inhibitor thread
       macOS/Windows: keepawake owned guard
       Linux: GNOME SessionManager cookie or XDG portal Request handle

Tauri Settings capability
  -> constrained read/replace desktop-power commands
  -> versioned atomic desktop-shell.json owner

AndroidPresentationRepository authoritative ProductState
  -> pure active-work predicate + Android preference
  -> ProductEngineService-owned partial WakeLock

IOSApplicationLifecycleOwner
  -> existing finite UIKit / continued-processing ownership only
```

Desktop shutdown first cancels and joins the view owner, closes its inhibitor
command channel, joins the dedicated thread after it releases the platform
guard, and only then shuts down the application service. The worker owns no
Tauri, React, application-service, filesystem, or torrent mutable state.

Android's foreground service is the sole lock owner. Preference replacement
must succeed before published UI/live state changes. The service calculates
eligibility from reduced typed torrent rows, performs idempotent acquire/release
on its owner, and releases again in shutdown even if the final state update was
lost.

## Resource Bounds And Failure Policy

| Resource | Bound and cleanup |
| --- | --- |
| Desktop power subscriptions | 1; closed and joined before service shutdown |
| Desktop inhibitor threads | 1; channel close, guard drop, joined Quit |
| Desktop native inhibitors | 1; replaced only on false/true transitions |
| Desktop shell record | 4 KiB maximum, atomic replacement, closed schema v3 |
| Android partial wake locks | 1 non-reference-counted service-owned lock |
| Android power preference | 1 private boolean, synchronous durable replace |
| iOS resources added | 0 |

Native acquisition failure is bounded, nonfatal, and observable without raw
torrent data. It does not pause a torrent, change the saved preference, claim
success, retry in a tight loop, or keep an unjoined task. A later false-to-true
transition may retry. Linux uninhibit/close and Android release failures are
logged and all owned references are still discarded during teardown.

Desktop v3 reads exact v2 notification/background settings and adds the power
default. It also reads exact v1 background state and supplies both notification
and power defaults. Unknown future versions, malformed/oversized records, and
write failures preserve the existing repair/rollback policy.

## Shape-Changing Cases

- active snapshot at process launch; empty snapshot; patch-only upserts and
  removals; reset before replacement snapshot; unexpected subscription close;
- multiple active torrents; one completes while another remains active; queue
  promotion/demotion; pause/resume; recheck; publication; terminal repair;
- stalled metadata or no-peer download; hidden/closed webview; UI reload; app
  restart; updater restart; normal Quit and shutdown failure;
- preference off before work, off while held, on while work is already active,
  failed persistence, and restart with either saved value;
- native acquire failure, Linux portal absence/restart, worker channel closure,
  Windows same-thread release, and forced process termination cleanup by OS;
- Android activity recreation without service recreation, service restart,
  screen off/on, background/foreground, force-stop, and low-power restriction;
  and
- iOS screen lock, finite background opportunity, expiration, suspension,
  foreground resume, and absence of a misleading persistent wake control.

## Implementation And Validation Stages

1. Record this contract, select Tactical `165` as the sole **Now**, and pause
   Tactical `158` without changing its signed-release stopping condition.
2. Implement and test the pure desktop eligibility reducer and exact
   version-1/version-2-to-version-3 shell migration.
3. Add the same-thread desktop worker, macOS/Windows keepawake guard, Linux
   portal request, setting channel, subscription, and joined shutdown.
4. Add the narrow Tauri controller and Tauri-only React Power category with
   persistence-failure and browser/demo omission coverage.
5. Replace Android's unconditional state check with a pure operational-state
   predicate, durable default-on preference, Compose switch, partial lock only,
   immediate toggling, and shutdown release tests.
6. Add iOS absence/truthfulness coverage and remove copy that could imply
   general indefinite power ownership.
7. Run proportional repository gates and build/package all desktop targets,
   both Android ABIs, and iOS simulator/device products.
8. Use `~/code/machine-control/bin/machine-control` and its platform guides.
   Run inventory and doctors first; use one exclusive claim per desktop;
   preserve inherited power state; and use only guest-resident `desktop`
   routes for UI, process, shell, and capture. Do not call Tart or drive a
   host-side VM window. Run attached Android and iOS devices through their
   common adapters and project-owned builds. Keep private inventory and device
   identifiers out of repository evidence.
9. Record exact state/inhibitor transitions, setting persistence, display
   behavior, shutdown cleanup, mobile sanity results, and omissions; reconcile
   topics/checklists; mark complete; and resume Tactical `158` as sole **Now**.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure policy | Every operational state; snapshot/patch/remove/reset; multiple torrents; queue/pause/recheck/publication; preference transitions |
| Persistence | Fresh defaults; exact desktop v1/v2 migration; v3 reopen; malformed/oversized/future repair; failed write rollback; Android restart |
| Desktop owner | Initial active acquisition; coalesced transitions; native failure; unexpected close; same-thread Windows lifetime; cancellation and joined release |
| React | Tauri-only Power category and accessible switch; immediate save; failure rollback/status; browser/demo omission; wide/compact/phone layouts |
| Android | Pure predicate; service/UI toggle; screen-off active transfer; pause/queue/complete/disable release; restart and shutdown; no Wi-Fi lock |
| iOS | No keep-awake control or idle-timer assertion; foreground transfer plus existing finite background/expiration/resume sanity |
| Package | Exact dependency/license; no arbitrary webview power authority; macOS/Windows/Linux builds; Android manifest/build; iOS archive metadata |
| Installed | macOS arm64, Windows arm64, Linux arm64 real inhibitor state; Windows x86_64 hosted build/package coverage; attached Android real WakeLock state; attached iOS lifecycle sanity; cleanup |

## Completed Implementation

Commits `035b185`, `0b4ed3a`, and `a71125a` implement the accepted slice:

- desktop shell schema version 3 preserves version-1 background and version-2
  notification preferences while adding one default-on power preference;
- one Tauri-owned subscription reduces authoritative torrent-list
  snapshot/patch/remove/reset state and drives one joined same-thread native
  worker. `Starting`, `Downloading`, and `Checking` acquire; every other state,
  preference disablement, reset, pause, and shutdown release;
- macOS and Windows use exact `keepawake` `0.6.1`; Linux uses one GNOME
  SessionManager suspend cookie or the bounded XDG portal request described
  above. No desktop path requests display or explicit-system-sleep ownership;
- the Tauri-only Power Settings category persists immediately and rolls back
  visibly on failure; browser/demo surfaces receive no power capability;
- Android persists the default-on setting, derives eligibility from the same
  typed operational-state set, owns one non-reference-counted partial wake
  lock in the foreground service, and removes the Wi-Fi lock and permission;
  and
- iOS exposes no keep-awake setting or idle-timer assertion and retains the
  truthful finite-background presentation owned by Tactical `149`.

The campaign also repaired two same-boundary state defects exposed by live
testing. Metadata discovery retries now remain `Starting` instead of briefly
projecting `Queued`, and a late active/checking runtime update can no longer
override persisted `desired_running = false` after Pause. These are operational
projection fixes, not new scheduler or persistence policy.

## Validation Evidence

Deterministic and build evidence passed for the Rust workspace, desktop shell,
React product, Android dual-ABI product, iOS simulator/archive, and desktop
release validator:

```text
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
node scripts/validate-desktop-release.mjs
node --test scripts/validate-desktop-release.test.mjs \
  scripts/validate-desktop-update.test.mjs
clients/android/build.sh
cd clients/android && ./gradlew lintDebug assembleDebugAndroidTest
clients/ios/scripts/test.sh ABSOLUTE_OUTPUT.xcresult
```

The web run passed 279 active tests with two skipped. The desktop release
validator passed all nine regression cases, including sleep-authority drift.
The final Android lint/test-package build passed; the current dual-ABI build
and physical APK came from the same implementation commits. The iOS simulator
run passed 25 unit tests and two UI tests. It retains the already-tracked Swift
6 concurrency warnings owned by `IOS-006`; those warnings do not concern or
weaken this power-policy slice.

`~/code/machine-control/bin/machine-control` supplied every native platform
route. No Tart command, host-surface pointer/keyboard route, host-focus change,
private inventory value, or device identifier is part of the evidence.

- **macOS arm64:** an ad-hoc signed exact-source app acquired one
  `PreventUserIdleSystemSleep` IOKit assertion and zero display/system-sleep
  assertions for a controlled stalled metadata torrent. It remained held for
  more than 30 seconds and while minimized. The default-on setting, immediate
  off/on release and reacquisition, Pause, paused restart, Start, and process
  termination all produced the exact expected assertion state.
- **Windows 11 arm64:** the exact-source native app produced one SYSTEM
  `powercfg /requests` entry and no DISPLAY request. It remained held for more
  than 35 seconds and while minimized. Default, off/on persistence, Pause,
  paused restart, Start, and termination passed. A Windows Firewall consent
  surface was handled through the guest-native controller; the host desktop
  remained untouched. Hosted Windows x86_64 builds cover the release target's
  compilation and package shape.
- **Ubuntu 24.04 GNOME 46 arm64:** one SessionManager suspend inhibitor was
  observable as `IsInhibited(4) = true` and `IsInhibited(8) = false`, with the
  exact RSTorrent identity and reason. It remained held for more than 30
  seconds and while minimized. Default, off/on persistence, Pause, paused
  restart, Start, termination, and the post-fix absence of a late discovery
  reacquisition passed.
- **Physical Android API 37:** the exact current dual-ABI debug APK displayed
  the Power Management setting, preserved an off/on preference across
  force-stop/relaunch, and held exactly one
  `org.rstorrent.bootstrap:download` partial wake lock through more than 59
  seconds and screen-off Dozing state. No Wi-Fi lock was present. The explicit
  service stop released the lock, after which app data and the controlled
  download root were cleared.
- **Physical iOS:** a signed current archive installed and launched through
  the common CoreDevice/XCTest adapter. Both Settings pages showed the existing
  finite-background explanation and no general prevent-sleep control; Home,
  background, relaunch, and clean termination remained healthy.

Every controlled process, inhibitor, guest artifact, device artifact, and
machine-control claim was cleaned. The three desktop VMs were returned to
their inherited stopped state; Android application data was reset; the iOS app
installation and its preexisting data were not destructively removed.

## Deliberate Limits And Next Step

The available Windows testbed is arm64, so native behavior evidence is arm64;
the supported x86_64 package retains hosted build/package coverage and should
repeat this assertion matrix as part of the next signed-candidate campaign.
The GNOME route is installed evidence while the non-GNOME portal route has
deterministic request construction, race, timeout, response, and cleanup
coverage but no second live desktop environment. Neither limitation changes
the platform contract or keeps Tactical `165` open.

Tactical [`158`](158-desktop-signed-packaging-and-updater.md) resumes as the
sole **Now** and must put this implementation, along with completed Tacticals
`160`--`164`, into the next signed cross-platform candidate.
