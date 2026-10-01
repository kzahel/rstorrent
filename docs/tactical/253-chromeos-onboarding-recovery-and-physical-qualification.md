# Tactical 253: ChromeOS Onboarding, Recovery And Physical Qualification

Status: **Ready; priority accepted 2026-10-01.** Gap-focused acceptance plan
recorded. Substantial prior physical evidence is credited below; this slice's
remaining implementation and new physical evidence have not started.

## Motivation And Outcome

Maintainer experience identifies both application unreliability and ChromeOS
Android installation/availability as migration risks. A working Rust engine
or already-configured development appliance does not qualify the user journey.
Prioritize repeatable end-to-end qualification on both available physical
Chromebooks, starting before Google Play or the application is ready.

The stopping condition is a recorded install-to-verified-download and recovery
matrix on both devices, plus product guidance that gets an unavailable Android
user to a usable supported route or an honest unsupported outcome. This gates
ChromeOS rollout independently of the accepted desktop rollout ordering.

## Owners And Dependencies

Read product-surfaces-and-migration, client-surfaces, web-ui-design,
application-view-api, android-saf-storage, download-roots, client-persistence,
localization and beta-release-readiness before implementation. Reuse Tacticals
167/169/178 for Crostini installation/storage, 194 for Android extension
control, 198/199 for lifecycle/network policy, 216 for local diagnostics,
248 for staggered upgrades and 250/251 for candidate delivery.
The full jstorrent-cutover-checklist retains production/store authority.

Machine Control owns generic device transport, desktop interaction, ARCVM,
Crostini and appliance recovery. This repository owns product fixtures,
assertions, troubleshooting presentation and evidence. Concrete device
selectors, accounts and access details stay in the private inventory.

## Build Host And Work Order

Maintainer direction on 2026-10-01 selects an x86_64 Linux development host
for this campaign's builds and orchestration. The old physical Chromebooks
are installed-runtime targets, not build hosts. Do not require a Rust, Node,
Gradle or Android SDK/NDK toolchain in their Crostini containers.

Build Android APKs on the development host using `clients/android/build.sh`;
its NDK pipeline produces both x86_64 and ARM64 native libraries. Build the
extension/web assets there and transfer finished packages through Machine
Control. Crostini's existing `scripts/build-crostini-package.sh` requires
Linux and builds for its host architecture. Use the release workflow's
Ubuntu 22.04 baseline in a host-side container or equivalent qualified
environment, rather than assuming a newer Linux host's dynamically linked
binary will run in the target container. Inspect native dependencies and
qualify the exact package on the target without compiling there. Native ARM64
package builds retain their separate qualified build lane if needed.

Portable troubleshooting UI, connection-state tests, installer failure tests,
localization and controlled integration fixtures can be developed and tested
on the current workstation before the Linux package or hardware session is
ready. Android-only physical cases do not depend on enabling Crostini: deploy
the host-built APK/extension to ARC and test there. Mocks and emulators cover
deterministic failure presentation; actual Play setup, installation, ChromeOS
permissions/lifecycle and Linux launch/storage still require physical evidence.

Order the work as portable recovery presentation/tests, host-built artifacts,
physical Android/Play journeys, then physical Linux fallback journeys.
Independent source work need not wait for the complete hardware matrix.

## Existing Evidence To Retain

This is not a restart of ChromeOS qualification. The first physical x86_64
ChromeOS 150 appliance already has these recorded passes:

| Record | Existing physical evidence | Remaining distinction |
| --- | --- | --- |
| [194](194-chromeos-android-extension-control.md#execution-checkpoint), 2026-08-30 | APK/deep-link launch, Android approval/pairing, shared Compose/extension library, two SAF roots, grant loss/repair, local torrent intake, controlled downloads, detached completion, restart persistence and ARC-only/LAN-refusal boundary | Debug APK and beta extension, not actual Play installation or the current production-identity candidate. |
| [167](167-chromeos-crostini-bundled-web-launcher.md#completion-record) / [169](169-hosted-crostini-bootstrap-and-release.md#post-completion-public-release-acceptance) | Linux package install/repair, actual public signed bootstrap, launcher/singleton, browser detach, twice-stopped-VM recovery, retained profile, preservation-safe uninstall/reinstall and bounded tamper rejection | Existing Linux environment; full ChromeOS reboot, suspend and newer exact candidate remain unqualified. |
| [168](168-platform-aware-extension-launcher.md), 2026-08-26 | Rendered Android/Linux chooser, correct Play listing with install UI, Linux handoff and separate-library wording | No Android app was installed through Play; no pre-Play or installation-failure journey was exercised. |
| [178](178-crostini-storage-guidance.md), 2026-08-27 | Before/after Share with Linux storage boundary, five paired hash-checked storage trials and physical Add/Settings help presentation | UI run used new web assets on retained public native binaries; newer host-built package hit GLIBC 2.39 incompatibility. |
| [198](198-android-completion-and-attention-notifications.md#physical-chromeos-completion) / [200](200-android-product-background-lifecycle.md#later-physical-chromeos-strengthening), 2026-08-31 | Verified completion/repair notifications and real taps, non-replay, permission denial/grant, foreground Stop, background transfer/seeding policy, sticky recovery and reconnect-grace/cleanup | Bounded API-33 ARCVM/debug observation, not broader endurance or store delivery. |
| [248](248-chromeos-staggered-upgrade.md#executed-evidence-and-reproduction), 2026-09-30 | API 28/35 emulator installed migration and four-pair update/re-pairing contract | Controlled emulator evidence, not physical ARC or production store identity replacement. |

The private inventory records the second Chromebook's controller/bootstrap
smoke, but not equivalent product Android/Crostini acceptance. Check for newer
evidence before execution; do not infer product coverage from appliance health.

Reuse these tests and retain their exact artifact/platform limits. Concentrate
new work on pre-Play/failed installation, troubleshooting gaps, the second
device, current production candidate regression/installed migration, and
explicitly unrun reboot/suspend/endurance cases. Do not rebuild passing root,
pairing, lifecycle or storage features merely to satisfy this new checklist.

## Invariants And Non-Goals

- Android retains one in-process Rust engine/profile/SAF owner. Crostini has
  its separate Linux owner/profile. Extension views remain detachable.
- Switching to Linux is an explicit backend/library change, not automatic
  migration. Never imply an empty Linux library lost Android's torrents or
  allow both engines to write the same payload concurrently.
- An unreachable service does not establish Play availability, app presence,
  administrative policy, or the cause of an installation failure. Distinguish
  observed facts from user-confirmed conditions and unknown state.
- No public deployment/store publication, powerwash, account removal,
  developer-mode transition, device-policy bypass, personal app/data clear,
  Play-disable operation that deletes inherited apps, or generic OS repair.
  Destructive setup transitions need a separately reviewed disposable cohort.
- No Crostini automatic legacy import, silent APK sideloading, new raw-I/O
  fallback, general remote discovery or platform performance claim.
- Record inaccessible states as unrun with the concrete reason; mocks and
  sideloaded APKs cannot close real Play installation rows.

## Product Troubleshooting Contract

Troubleshooting must be available from the extension's pre-connection screen
as well as the connected application. A user with no installed Android app
cannot reach an Android-only help screen.

Explain the stage and one next action: enabling/finishing Play setup,
installation unavailable/failed, opening the installed app, platform launch
confirmation, pairing approval, denied browser permission, incompatible app
version, connection loss, or unavailable download folder. Include manual
retry/cancel and retain user choice without indefinite automatic launch loops.
When the platform cannot identify why installation failed, say that clearly;
do not promise the app installed because the store link opened.

Offer **Use Linux instead** from Android setup/update/connection failure
guidance. Explain enabling Linux when allowed, installing the authenticated
package, starting the registered launcher, choosing Linux Downloads or
explicitly sharing a ChromeOS folder, and reaching the correct library.
If both Android and Linux are unavailable or policy-blocked, explain the
unsupported environment without cycling through installation links.
Keep the Android-alone route usable where extension control is unavailable.

Support context should name the failed stage, selected backend, build/OS
versions, known permission/version status and a closed error category. Reuse
previewed voluntary local diagnostics; automatic private log upload, raw
paths, account details and credentials are outside this slice. Existing
diagnostics are not assumed to cover these stages; qualify the new fields.

## Physical Acceptance Matrix

Record initial Play/ARC/Linux state, OS/build/architecture, candidate hashes,
extension identity, test-profile isolation and whether setup is real or
injected. Exercise both physical devices rather than treating one as a
representative pass for the other.
For every row, distinguish retained historical pass, current-candidate pass,
and genuinely unrun state. Existing evidence above closes its bounded original
cohort; repeat only the proportionate regression needed for changed artifacts.

| Cohort | Required user-visible result |
| --- | --- |
| Play not enabled / setup unfinished | Truthful Android setup steps; user can choose Linux; no claim of installed app or endless connection spinner. |
| Play available, app absent | Actual store installation, launch/confirmation, pairing, folder selection and independently hash-verified download. |
| Installation fails / app unavailable | Actionable help and retry or explicit Linux choice; preserve inherited apps/data. Record real failure separately from injected presentation coverage. |
| Play disabled/unavailable/managed | No assumption it can be enabled. Linux route works where permitted; neither route available produces honest unsupported guidance. |
| Android installed, stopped / permission denied / stale pairing | Explicit launch or approval/repair leads to the same library; cancellation terminates the pending attempt. |
| Old/new app and extension combinations | Tactical 248's four-pair contract and update help work on physical hardware, including no available Play update. |
| Linux not enabled / enabled but package absent | Real OS Linux setup where authorized, signed package installation, launcher handoff and verified download; identify blocked/unsupported Linux distinctly. |
| Linux VM stopped / browser restarted | Explicit launch restores one owner/library; retry and passive browser activity obey existing lifecycle rules. |
| Storage permission/share absent or revoked | Explain Android grant versus Share with Linux accurately; repair original root binding and preserve unrelated bytes. |
| Both backends installed | Clear selected backend/library; explicit switch; independent profiles and no duplicate payload writer. |

For each working route, exercise magnet and local torrent intake, native/
extension view changes, closing the view, interrupted transfer/reconnect,
verified completion, remove-and-restart, and source-offline retained state.
Run sleep/wake, network loss/recovery and reboot under the applicable device
policy, recording login and recovery prerequisites. Preserve Machine Control's
appliance availability policy rather than changing it as routine cleanup.

For the remaining qualification cohort, start with three independent
cold-launch/recovery repetitions per supported route per device and one
60-minute controlled transfer observation per route per device. This adds
current-candidate repeatability, not a demand to rerun every historical case.
Start with a 30-minute OS/store setup budget and a five-minute
connection/recovery budget; retain exact failure stage/timing. A timeout is a
recorded failure requiring diagnosis, not an implicit retry pass.
These are an initial qualification budget, not a reliability-rate claim or
permission for unbounded soaking. Use independently generated controlled
payloads/seeders; public swarms remain opt-in.

## Execution And Evidence

1. Reconcile the retained evidence above with current artifacts and existing
   product runners. Build a gap matrix before scheduling repeats. Add
   focused failure-presentation tests and a scripted product journey before
   manual hardware work; do not implement generic device control here.
2. Read Machine Control's ChromeOS guide, run the common read-only doctor for
   each explicitly selected private target, acquire applicable ownership, and
   record baseline plus setup/reset prerequisites before mutations.
3. Use isolated owned profiles/cohorts. Test real UI installation and permissions;
   root SSH/ADB is observation/control infrastructure, not proof ordinary users
   can install from Play. If isolation cannot preserve apps/grants, stop that
   transition and record its required disposable setup.
4. Implement bounded troubleshooting/fallback gaps, then execute the matrix
   and repetitions. Diagnose failures by stage; an engine/protocol change gets
   its own tactical/source-oracle review rather than expanding this slice.
5. Record per-device/state pass/fail/unrun, time to usable download, retry
   count, verified bytes, retained-state and cleanup outcomes. Public evidence
   uses machine-neutral cohort labels; keep raw support artifacts private.
6. Join owned browsers, seeders and application tasks, remove only test state,
   restore approved non-destructive test settings, and reconcile topics and
   checklist. Any failed required route remains a ChromeOS shipment blocker.

Next executable action: map the existing runners and retained passes onto the
current candidate, identify missing troubleshooting/state coverage, and qualify
the host-side Linux package build plus isolated pre-Play/second-device cohorts.
No physical acceptance row is closed by this planning checkpoint.
