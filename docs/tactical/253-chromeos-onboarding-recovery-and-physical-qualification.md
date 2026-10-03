# Tactical 253: ChromeOS Onboarding, Recovery And Physical Qualification

Status: **Active, 2026-10-02.** Work started from clean, current main `223d5126`.
Portable implementation and bounded cohort-A physical slices have landed.
The Linux follow-up restores ordinary registration and adds offline connection
recovery plus current browser/file-picker and source-offline VM recovery evidence.
Full two-device/store/fallback acceptance remains open.

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

Next executable action: finish the controlled current-candidate observations,
qualify physical extension recovery and Linux launcher handoff where accessible,
and repeat the gap matrix on cohort B once its private target is reachable.
Store, OS setup and installed migration rows remain open.

### Runner And Candidate Gap Audit, 2026-10-01

| Existing facility | Credit / safe reuse | Actual gap |
| --- | --- | --- |
| `clients/extension/scripts/popup.test.mjs`, `service-worker.test.mjs` | Platform chooser, launch intent, optional permission and singleton handoff | No pre-Play failure selection or launch-confirmation diagnosis; popup launch request is not proof an app opened. |
| `clients/web/src/android-companion-upgrade.test.ts` | Strict old/current/incompatible discovery; current service takes precedence; no legacy authority | Unreachable service loops indefinitely; no bounded terminal help/manual recovery or support context. |
| `clients/android/scripts/run-legacy-upgrade.py --source companion` | 248's API 28/35 ordinary-writer four-pair regression | Owns rooted emulators; cannot qualify physical/store installation or preserve an inherited physical package by substitution. |
| `tests/interop/android_saf_session.py` and Android runtime/product runners | Existing deterministic fixtures, seeder and independent byte checks | SAF runner explicitly clears the fixed package and removes a fixed folder. Do not run unchanged against inherited physical state. |
| `scripts/test-crostini-installer.sh`, `run-crostini-bootstrap-fixture.sh`, `validate-crostini-package.sh` | Installer ownership, authenticated bootstrap and package allowlist | Fixture success is not physical OS setup; source-host build must first meet Ubuntu 22.04 ABI baseline. |
| 167/169/178 retained physical records | First device's install/VM recovery/storage passes, with exact historical hashes | Native package and web-only 178 evidence predate this source; no current package, full reboot or second-device product coverage. |
| Machine Control common doctor | First device passes ten checks; unlocked, no pending update | Health is not Play/product qualification. ChromeOS adapter reports `unsupported_claim_interface`; use explicit selector and an exclusive temporary ownership record before mutation. This session uses a target-side marker. |

No retained APK/ZIP/native hash is the candidate built from this checkpoint.
Record freshly built hashes separately and preserve original installed state.
The second target is declared in private inventory for another controller;
qualify local reachability through the public explicit host selector without
altering its private declaration or another device's selector.

Implementation owner map: the popup owns only permission/launch handoff; the
packaged Android page owns one bounded attach/pair attempt, cancellation,
connection and mounted React view. Page departure cancels and joins owned work;
retry is manual and attach-only. Static troubleshooting is available before
connection and from the connected header. Support preview uses only closed
stage/category/backend values and validated build versions, never raw errors,
credentials, account values, torrent state or filesystem paths. No engine or
application DTO changes are planned.

Reference audit: JSTorrent's
`extension/src/lib/daemon-bridge/chromeos/ws-connect.ts` uses a bounded 10-second
handshake and closes on timeout; retain bounded ownership rather than its wire
implementation. Existing 194/248 tests own current protocol compatibility;
this presentation slice does not change peer/engine semantics.

### Portable Recovery Checkpoint

The packaged Android page now owns a 20-second discovery deadline and a
150-second complete connection/pairing deadline, followed by manual Retry.
Cancel and page departure terminate the attempt; a frozen/restored page offers
manual recovery. Retry never sends OS launch intent. Disconnect and failed UI
mount release the mounted view and authenticated client before recovery.
The popup distinguishes a rejected launch request from unconfirmed startup.
Optional browser permission denial contacts no service. Local Network Access
and Play/app/policy state are not inferred from failed requests.

Static offline troubleshooting is reachable from the popup, connection screen
and connected Android header. It covers user-confirmed Play setup, unavailable
installation, opening/confirming launch, permission/pairing, incompatible
updates and folder repair. **Use Linux instead** opens explicit setup,
authenticated published installation and separate-library/unsupported guidance.
The volunteered support preview contains closed stage/category/backend and
permission/version facts with unknown Play/app/policy values. No raw error,
paths, accounts, credential, automatic copy or upload enters that report.

Validation: `npm run typecheck --prefix clients/web`; full
`npm run test --prefix clients/web` (449 passed, two skipped);
`npm test --prefix clients/extension` (45 passed); all four catalogs via
`node scripts/check-localization.mjs`; `bash scripts/test-crostini-installer.sh`;
beta and production-identity extension packaging and CSP/allowlist checks.
One intervening full web run exposed the existing external-intake test's
command-versus-notification timing race during concurrent native builds;
the unchanged test passes on the subsequent complete run. No product change
was made for that unrelated failure.

`node scripts/verify-chromeos-onboarding.mjs <extension.zip>` drives the actual
packaged extension in owned Playwright Chromium, preserving exact extension
origin checks. Six explicitly injected scenarios (denied optional permission,
unreachable service with cancel/retry/deadline, old app, incompatible protocol,
rejected and expired pairing) pass support preview and offline Linux help,
including 320/1440-pixel overflow checks. All profiles/browser processes are
removed/joined. This is presentation evidence, not physical or Play acceptance.

Hardware preflight: cohort A is x86_64 ChromeOS 150 `16700.65.0`, ARC Android
13/API 33. Inherited production Android 1.0.23/code 23, debug 0.1/code 1 and
Play Store are present. Preserve all three. Cohort B's explicit private target
fails common doctor and network preflight with LAN `No route to host`, without
VPN interception. It has not been mutated; its product rows remain unrun.
The attempted Crostini inventory reports a concierge/message-bus disconnect;
do not infer Linux is disabled from that infrastructure failure.

### Host Build Checkpoint

`clients/android/build.sh` passes on this Linux host after installing the
pinned NDK 28.2.13676358. Both native ABIs, generated Kotlin, debug APK/JVM tests
and packaged notices pass (83 Maven/205 Rust packages, six native libraries).
The debug-only `chromeosQualificationTestPackage` accepts exactly
`org.rstorrent.qualification253`, rejects combining with the legacy production
upgrade override, and leaves release identity unchanged. Its separate APK and
instrumentation APK build/JVM tests pass; actual package metadata is checked.
This permits controlled physical testing without replacing either inherited
Android app. It cannot qualify Play or same-package installed migration.

The host-side `scripts/build-crostini-baseline.sh` builds with native x86_64
Ubuntu 22.04, Rust 1.97.0 and checksum-checked Node 22.22.0. Its source mount is
read-only; build/cache state stays in the owned target directory. A private
empty Docker configuration avoids relying on the host's missing desktop
credential helper. Source refresh removes only the container's owned source
copy; an exclusive build lock prevents competing output owners. The finished
20-file/40,151,130-byte package passes allowlist validation, ELF/ldd inspection
and both executable version smokes. Maximum GLIBC is 2.34; dependencies are
libc, libm, libgcc_s and the standard ELF loader. Exact target dependency and
launcher/storage evidence remain required. Native ARM64 retains the qualified
release lane, with no local emulation claim.

Exact artifact SHA-256 at this checkpoint:

- Crostini x86_64: `95e6f05b35fcb22d4640f81175039759d78d1f2400f403b2e71626d7cccbd2a2`.
- Production-identity extension 1.1.2: `95d604a8215e2606b72f2fca72e00424b656aa4f62175743e53ae64603dbcfdc`.
- Beta extension 0.4.0: `54dc1562501e8c9eac2c9dc29fea6f1c1a027799fedb1bb6279308cbdd1c4e41`.
- Isolated debug APK: `062def17bcf2993ae1cce935bc1ec5bbf315876e18e9a463a5c83c036a5e658e`.
- Isolated instrumentation APK: `55cb94f130368f31b8b875958bcd8cc69e390498a2cc1d126cae419e2ab2c967`.

All are local non-published artifacts. The Crostini candidate has no production
release signature; runtime installation must be labeled controlled local
candidate, independently from the retained authenticated public-bootstrap pass.
No signed/store gate is closed by these builds.

### Isolated Physical Runner Checkpoint, 2026-10-02

`tests/interop/chromeos_android_qualification.py` uses the common CLI and the
exclusive qualification APK, a new owned SAF folder and controlled host seeder.
It preserves inherited packages, grants and logs. Its actual ACTION_VIEW intake
handles both external-input confirmation and subsequent enabled file selection;
it does not use the debug magnet shortcut as cold-start intake. The real picker
commits the current binary retained-root registry. Independent Android `sha1sum`
checks downloaded bytes. Three separate runs use distinct torrent identities;
clearing between them applies only to the owned qualification package. This is
not evidence for the user's Remove flow or installed-library migration.

`tests/interop/chromeos_crostini_qualification.py` exercises the exact candidate
through the existing VM's LXC runtime, an owned temporary profile, ordinary
application commands and unique Linux Downloads payload names. It checks the
actual gateway owner before termination, source-offline restart and
remove/keep/restart byte retention. Runtime results do not qualify ChromeOS OS
setup, installation, registered launcher, browser or extension journeys.

Six portable runner safety checks pass with
`PYTHONDONTWRITEBYTECODE=1 uv run --project tests/interop --locked python -m
unittest discover -s tests/interop -p test_chromeos_qualification.py -v`.
They cover actual nested-shell metacharacter preservation, log-clear refusal,
owned UI capture paths and separate external-input/metadata-selection stages.

Cohort A: the exact transferred Ubuntu 22.04 package SHA-256 matches the host
candidate. Both executables run; `ldd` resolves all native dependencies in the
inherited Debian 12 container. Gateway health reports Crostini build 0.1.0,
launch protocol 1. The inherited VM was initially stopped and the container
STOPPED. Starting it does not restore ChromeOS/Cicerone container registration:
normal `vsh --target_container=penguin` reports a missing container, while the
existing LXC container is reachable and runs. No VM reset or OS repair was used.
Normal Linux launcher/setup remains an observed blocked journey.

Calibration failures are kept separate from acceptance: first-use disclosure,
the old plaintext-grant assertion, shell quoting, debug cold-start intake and
two separate confirmation screens required runner corrections. A random seed
port was blocked by the controller firewall; narrowly owned temporary rules
now allow only this cohort's two explicit seed ports. Concurrent UI inspections
collided; the Android runner is now the sole UI inspector. No failed five-minute
attempt was counted as a pass. Libtorrent's default LAN rate-limit exemption
made the initial observation complete early; that observation cannot close
endurance. The corrected seeder explicitly disables the exemption. Each
observation must retain actual payload movement through the bounded hour and
finish with an independent byte check. Final results and cleanup follow below.

Real Play observation: the existing production listing opens and offers Open;
production 1.0.23 and Play Store remain installed. No installation or update was
performed. Preserving that inherited app prevents a fresh-install cohort; the
unsigned qualification sideload cannot close Play installation or store update.
Cohort B still fails transport doctor; all its product rows are unrun.

### Physical Recovery And Presentation, 2026-10-02

The candidate extension assets are staged in a new exclusively owned
subdirectory of the inherited unpacked beta extension. Its registered popup,
worker, original files and identity are preserved. The candidate companion and
offline guide are opened directly under that known beta origin; this qualifies
those exact page assets, not a fresh extension installation or the production
store pair. The single inherited Android-pairing storage key is privately
backed up before connection and must be restored after page departure.

Real rejection of the qualification app's Android approval returns the candidate
page to manual Retry. Retry and explicit approval connect to the same controlled
Compose library. The connected header identifies **JSTorrent Android** and
retains troubleshooting. Native cold restart returns the browser to disconnected
help; manual attach-only Retry restores the active torrent without a new launch
loop. Real guide rendering in ChromeOS Chrome passes at the 1600-pixel viewport
without horizontal overflow. Physical display capture reports no active CRTC;
these are semantic/browser-layout observations, not physical-panel screenshots.

The real support preview exposed Chrome's reduced user agent: it reports
ChromeOS `14541.0.0` despite the device's separately observed `16700.65.0`.
Support fields now explicitly name `chrome_user_agent_version` and
`chromeos_user_agent_version`; actual browser/OS build fields remain unknown.
Manifest and authenticated product versions retain their separate provenance.
An added regression case and the full web suite pass (450 passed, two skipped),
with typecheck. The connected Android label, displaced during help extraction,
is restored and covered. Test-specific wording is removed from user help.
Both final packaged extensions pass all six injected browser scenarios again.

Extension SHA-256 at this recovery checkpoint:

- Production identity 1.1.2: `9853fd75a8902e00b8f6dca045580880e8724cd5d98cb1644ccedc7285d2d766`.
- Beta 0.4.0: `4b42233bf20a9ea2548368663cb7b054e0cfd57bbe28aad3ddc4d25f732097e1`.

APK and Crostini hashes above are unchanged: these fixes affect only extension
companion/help entry points, not the native app or normal Crostini web bundle.

A private controlled HTTP tracker/seed feeds independently authored private
metainfo through the actual extension file input. A 64-KiB payload independently
verifies on Android and, through byte-upload API intake, the separate Linux
profile: SHA-1 `9d18c16c51954b29cf228754f795ccdd5fd1aecf`. A real ten-second
controlled source pause/reconnect is distinguished from unrun device network
loss. This file-input run does not qualify the OS file picker. The first view
close established retained bytes but not completion timing; a separate slower
payload checks that distinction. Actual extension Remove/keep completes, then
native cold restart preserves the bytes while the removed row stays absent and
the active observation torrent returns through manual Retry. An earlier owner
kill before Remove acknowledgement retained the row and bytes; that interrupted
attempt was not treated as completed removal. The hour observation includes
these two explicit native owner restarts and their recovery, rather than claiming
an uninterrupted session.

At this administrative-runtime checkpoint, Crostini browser navigation returned
`net::ERR_NAME_NOT_RESOLVED` for the expected `penguin.linux.test` authority.
Normal container-targeted vsh reported a missing container despite healthy candidate
runtime through LXC. Existing guest agents were observed, not repaired. This
blocked the browser journey at that checkpoint; the Linux follow-up below proves
ordinary Terminal startup restores registration and records current browser
acceptance separately. No alternate proxy or policy bypass turns the earlier
runtime pass into fallback acceptance. Reboot remains unrun
without a declared post-boot login source; sleep/wake is unrun under the inherited
always-awake appliance policy. Neither prerequisite is silently changed.

### Approval Window And Cancellation Ownership

Real rejection is now a closed `pairing_rejected` result, distinct from explicit
server expiry and malformed/unknown responses. The first physical expiry trial
returned a generic pairing API failure, not the injected fixture's expired JSON.
Source inspection of `crates/rstorrent-gateway/src/chromeos_companion.rs`
(`pending`, `prune_runtime`, `poll`, `request_pairing`, `pending_snapshot` and
`companion_pairing_poll`) explains the race: the Android observer can prune the
request before browser poll observes expiry, yielding NotFound. No HTTP NotFound
is treated as proof of app absence, policy or expiry.

The client validates declared approval duration as an integer in 1..120 seconds
and owns a monotonic deadline for that approval window. An explicit expired poll
or elapsed local window becomes `pairing_expired`; malformed outcomes and early
API errors remain unknown connection failures. Invalid durations issue no polls
and create no stored authority. Native protocol/server behavior is unchanged.
Local Cancel stops polling and joins work; the separately owned single Android
request remains bounded by its expiry. Pairing-stage cancellation now explains
waiting up to two minutes before manual Retry, since rejecting a canceled
request alone does not free that server slot. Static offline help says the same.

Cancellation during asynchronous UI startup closes the authenticated client
immediately and retains one shared close promise until termination. Retry stays
disabled while that release is pending. A blocked-mount test verifies this join,
in addition to declared-window/invalid-duration and rejected/expired/unknown
poll cases. Typecheck, all localization catalogs (1327 English web messages),
and the full web suite pass (462 passed, two skipped). The final artifact and
physical window repetition records follow after validation/cleanup.

### Remaining Journey Gap Matrix

These rows preserve historical passes above and describe the new bounded
candidate cohort independently. Cohort B has no new product evidence because
its explicit target is unreachable; appliance history does not close any row.

| Journey | Cohort A current candidate / retained limit | Cohort B |
| --- | --- | --- |
| Play setup unfinished/not enabled | Injected portable presentation and physical offline guide; real state unrun, inherited Play preserved. | Unrun: SSH/network unavailable. |
| Play available, app absent | Real listing opens, existing production 1.0.23 offers Open. Fresh install unrun: requires a disposable app-absent cohort and store successor. Sideload does not qualify it. | Unrun: SSH/network unavailable. |
| Installation failure/unavailable/managed | Offline help/manual next actions pass; no real installation failure or policy-disable transition. Play/app/policy remain unknown from connection failure. | Unrun: SSH/network unavailable. |
| Installed Android stopped / pairing recovery | Isolated current APK cold launches and real candidate-page approval/rejection, disconnect/manual retry pass; inherited production/debug apps unchanged. Optional permission denial is injected only. | Unrun: SSH/network unavailable. |
| Old/new app and extension combinations | Retain 248's emulator four-pair contract. Current isolated APK/candidate page assets pass a bounded physical slice; inherited popup/worker and production store pair are not replaced. Other physical combinations remain unrun. | Unrun: SSH/network unavailable. |
| Linux not enabled / package absent | Preserve inherited VM/container/package. Exact unsigned candidate runtime passes in isolated temporary profile; new OS setup and signed current installation unrun. Retain bounded 167/169 signed historical installation. | Unrun: SSH/network unavailable. |
| Linux stopped / browser restart | Administrative LXC startup failed normal registration; follow-up normal Terminal startup restores hostname and vsh. Current isolated connection page, stopped-VM manual Retry and durable-profile completed-row/byte retention pass. Current installed candidate Launcher remains unrun. Three earlier gateway repetitions remain runtime only. | Unrun: SSH/network unavailable. |
| Android grant / Linux share absent or revoked | Current owned SAF picker commits a root and transfers verified bytes. Retain bounded 178/194 repair/share passes; current revoked/share-repair repetitions are unrun. | Unrun: SSH/network unavailable. |
| Both backends / separate libraries | Independent native and Linux profiles verify controlled bytes; guide/header explain explicit backend/library choice. Follow-up normal-hostname Linux browser connects to its separate owned library. Full current worker/store-pair switching remains unrun. | Unrun: SSH/network unavailable. |
| Magnet / local torrent / detached view | Native magnet and actual extension file input pass. 1-MiB payload absent before view detach later verifies SHA-1 `27b016b8f330f063a2776225cd2242821cbb8b40`. Native OS file picker is unrun. Linux magnet/byte-upload runtime passes; follow-up real ChromeOS file picker and connected Linux browser independently verify 64 KiB. | Unrun: SSH/network unavailable. |
| Interrupted transfer / remove / source offline | Controlled ten-second seed pause is injected source interruption, not real device network loss. Native actual Remove/keep then owner restart retains bytes/removes row; interrupted pre-ack Remove retains row/bytes. Linux runtime remove/keep/restart passes. Follow-up Linux browser manual recovery after VM stop/start retains its completed row and exact bytes with the source offline. | Unrun: SSH/network unavailable. |
| Sleep/wake / real network loss / reboot | Unrun: preserve always-awake availability policy and remote transport; no declared post-reboot unlock source. No destructive Play/policy changes or generic OS repair. | Unrun: SSH/network unavailable. |
| Three repetitions / 60-minute observation | Three independent native repetitions and three Linux-runtime repetitions pass; both controlled 60-minute observations finish with independently verified bytes. One route is not the other route's GUI acceptance. | Unrun: SSH/network unavailable. |

Next cohort prerequisites: reachable cohort B with captured initial state;
app-absent disposable Play setup for real installation; original-certificate
store candidate; current signed Linux installation/registered Launcher; declared
post-reboot unlock path and an applicable sleep/network-loss policy. No signed
branding candidate, desktop GUI/store-pair, controlled-repair or broader-cohort
gate is closed by this ChromeOS work.

### Final Candidate And Observation Results, 2026-10-02

The final packaged extensions pass typecheck, the complete web suite (462
passed, two skipped), all four localization catalogs and both sets of six
injected packaged-browser scenarios. Their SHA-256 values are:

- Production identity 1.1.2: `856a6cd7281573e8f75810fb0891300b53857b7f962e344daf120479946c66af`.
- Beta 0.4.0: `6085ca136c276a03f2d35c70833a18278fd9fa0441b771d44838ced4b193310a`.

All 16 staged beta files match the final archive independently on cohort A.
The APK and Crostini hashes at the build checkpoint are unchanged. Native
instrumentation builds, but its APK is not installed; these are physical runner
results, not an instrumentation or Play-install claim. Native ARM64 remains a
separate build lane, not physical ARM64 evidence from this x86_64 device.

Real final-page pairing cancellation remains terminal through the remote
request's bounded lifetime. Manual Retry without approval returns the typed
`pairing_expired` result after 121.4 seconds; real rejection returns
`pairing_rejected`. Neither diagnostic infers Play, app presence or policy.
After the runner clears the owned app, browser control needs explicit setup
again; an attempted approval in that reset state is not a pairing pass.
After owned-app removal, foreground discovery reaches the observed 20-second
unreachable result with unknown Play/app/policy facts. Manual Retry/Cancel
returns to terminal Retry, and the actual Use Linux instead link opens the
offline guide with separate-library and unsupported-outcome guidance.

The controlled native repetitions each verify 262,144 bytes in 50.74, 51.30
and 51.59 seconds, including source-offline native cold restart. The separate
Linux-runtime repetitions verify the same size in 44.23, 43.19 and 105.75
seconds, including gateway restart and remove/keep/restart. All six independent
payload SHA-1 checks equal `363a09c4940de553b7f1f874bdb948aedd69f0f9`.
The longer third Linux repetition passes its budget; it is not discarded.

Each corrected observation lasts 3,600 seconds and retains 120 actual payload
samples at 30-second intervals, from 0 through 3,570 seconds. Independently
checking those logs finds progress in every two-minute window. Android's last
sample is 27,344,896 uploaded payload bytes; Linux's is 28,131,328. Both then
finish the 29,360,128-byte controlled torrent within the additional five-minute
verification budget, independently hashing actual downloaded bytes to
`1b90d0a98b5c16a7ced9cb42c13f5c61757d6482`. Linux's complete transfer,
source-offline restart and remove/keep/restart record takes 3,738.79 seconds.
Android includes the two explicit native restarts documented above; neither
observation qualifies sleep, reboot, device network loss or Linux browser UI.

The reports use `chromeos-android-qualification/v1` and
`chromeos-crostini-runtime/v1`; local temporary reports/logs are summarized here
and removed after restoration. Retained evidence consists of these bounded
facts, exact artifact hashes and reproducible committed runners, not recordings
or private inventory, credentials, personal paths or an archived device image.

### Restoration And Restart Checkpoint

The Android runner removes only its owned SAF payload tree/UI captures and
clears only the isolated qualification package. That APK is then uninstalled;
the instrumentation package remains absent. Inherited production 1.0.23 and
debug 0.1 remain installed with their original stopped flags (true and false,
respectively). No inherited app, grant, library or payload is reset or upgraded.

After leaving the candidate page, the original extension pairing key is restored
and equality checked without exporting its value. Only candidate subdirectory
assets, owned guide/companion tabs and the Play listing task opened by this run
are closed/removed; no browser page tabs remain, matching initial state.
Original popup, worker, profiles and extension files are retained.

The owned Linux gateway terminates after checking its command/profile and UID.
Its temporary package/profile and extra intake payload are removed, as are the
runner's four unique payload roots and temporary LXC configuration. The inherited
VM is stopped again and its disk is retained. No installed Linux package, normal
launcher, inherited profile or payload is replaced or repaired. Owned host
seeders terminate, their three narrowly scoped temporary firewall rules are
removed, and seed ports no longer listen. Temporary local reports, logs, fixture
downloads, APK copies and private target selection are removed after summarizing
evidence. The exclusive ownership marker and private pairing backup are removed.

The final common doctor passes all ten checks on cohort A; its unlocked session
and inherited closed-lid/idle availability overrides remain intact. This doctor
advertises configured capture prerequisites, not a successful physical capture:
the actual no-active-CRTC failure above remains the capture evidence limit.
Cohort B's final read-only network preflight still reports No route to host;
it is never mutated. No push, tag, store upload or production update is made.

Next executable work is the gap matrix, starting with reachable cohort B and
initial-state/ownership capture. A current installed Linux Launcher cohort,
a disposable app-absent real Play cohort and declared reboot unlock/lifecycle
prerequisites are needed for their respective rows. Tactical 253 remains active,
M-08 remains partial and cutover A-06/A-07/A-08 stay unchecked. Tactical 252's
signed desktop updater passes are retained; Linux legacy GUI rendering, actual
extension/store pairs, controlled repair, broader cohorts and a fresh signed
branding candidate remain independent open desktop gates.

### Linux Reachability And Recovery Follow-Up Scope, 2026-10-02

Maintainer direction: investigate when Crostini DNS/browser access fails,
understand the actual behavior and user experience, and address the failure.
Compare stopped Linux, ordinary Terminal startup, registered-container startup,
runtime-ready/browser-unreachable and manually retried recovery before treating
the last administrative runtime session as a general DNS regression.

This slice owns an offline extension recovery surface, truthful requested versus
connected outcomes, bounded exact-authority health checks and manual Retry/Cancel.
If browser access requires an optional host grant, request only the existing
`penguin.linux.test` authority through explicit user intent; do not add broad
network permissions, alternate IP routing, a proxy or automatic backend switching.
Failed browser fetches do not prove DNS, package absence, Linux availability or
policy. Native health validates the existing product/build/launch-protocol facts.
No engine, wire/API, root migration, production installer or store release changes.

The page owns at most one bounded health request and joins cancellation before
Retry. Successful reachability may navigate to the existing Linux product UI;
failure retains packaged offline help. The worker owns singleton handoff/tab
selection and must not call tab creation proof of a connected application.
Validate unavailable/denied/timeout/cancel/retry, wrong identity/protocol,
malformed/oversized responses and successful exact-authority handoff. Use real
ChromeOS normal-start and DNS observations where accessible, label injected
states and retain artifact limits. Generic OS diagnosis/control belongs in
Machine Control. Restore initial VM/window state and remove only owned fixtures.
Stop when bounded portable recovery and proportional current physical behavior
are recorded, with inaccessible scenarios still open in the existing gap matrix.

### Linux Portable Recovery Checkpoint

The extension now opens packaged `crostini/connect.html` instead of a bare
possibly unavailable Linux URL. Popup copy says connection is unconfirmed.
The page checks existing optional access, requests only the exact Linux host
through explicit Retry, and validates the unchanged health product/build/launch
protocol before navigating. Its request deadline is ten seconds and its body
bound is 4 KiB, including responses without Content-Length. Redirects fail closed;
oversized, malformed and failed HTTP responses cancel their body. Retry is manual,
Cancel joins transport work, and departure/bfcache restoration cannot resurrect
an attempt. No broad permission or alternate endpoint is admitted. Static help
explains ordinary Terminal startup, the confirmed Chrome hostname error and
preservation-safe shutdown without deleting Linux or its library.

Initial real browser navigation exposed an older HTML shell referencing an
absent old asset; the candidate on disk and its no-store HTTP response instead
reference the current asset. Fetching current HTML renders the connected React
library. Fresh explicit connection/backend handoff now uses a non-authoritative
random document query to bypass retained old cache entries without clearing
personal browser caches. A real HTTP-cache fixture independently proves ordinary
navigation still retrieves the old shell, while the new connection retrieves the
current document. Existing warm connected-tab handoff does not reload that tab.

`npm test --prefix clients/extension` passes 54 tests and source validation.
Both beta and production packages pass CSP/inventory validation and
`node scripts/verify-chromeos-onboarding.mjs <archive>`: six injected Android,
ten injected Linux and one HTTP-cache replacement case per archive. All four
localization catalogs pass; the shared React/Rust/application contract is
unchanged. Source/fixture checks do not qualify a store installation or real DNS
diagnosis. Native APK/Crostini package hashes retain their previous limits.

Physical preliminary results: normal Terminal startup restores hostname and
ordinary container-targeted vsh, including a stopped-VM repetition. A separate
unpacked test identity with exact candidate page assets permits real optional
permission denial/grant without changing either inherited extension. The current
package serves connected React through the normal hostname and a real ChromeOS
file picker feeds the controlled 64-KiB torrent; independently checked bytes hash
to `9d18c16c51954b29cf228754f795ccdd5fd1aecf`. Stopped Linux returns to offline
unknown-state guidance and manual Retry. Durable-profile VM recovery and final
restoration are recorded at the follow-up completion checkpoint below.

### Linux Follow-Up Completion And Restoration, 2026-10-02

Ordinary Terminal startup, selecting **penguin** and waiting for the command
prompt, restores `penguin.linux.test` in the host's registered hostname map and
ordinary container-targeted vsh. Stopping the VM removes that hostname; repeated
normal startup restores it. The earlier administrative VM/LXC session did not
complete that registration. Its observed browser DNS failure remains a bounded
failed administrative-path result; it is not evidence that ordinary Linux
startup generally fails. Waiting for actual registration matters: vsh immediately
after the start gesture can still report an unavailable VM.

For reference provenance, ChromiumOS platform2 revision
`4e7c7027ff788542bb225c09733c3a83d4760f72`,
[`vm_tools/cicerone/service.cc`](https://chromium.googlesource.com/chromiumos/platform2/+/4e7c7027ff788542bb225c09733c3a83d4760f72/vm_tools/cicerone/service.cc),
has `Service::ContainerStartupCompleted` register the container and primary
owner's Linux hostname before reporting startup. `ContainerShutdown` unregisters
it; `RegisterHostname` calls crosdns and `OnCrosDnsNameOwnerChanged` restores the
known map when that service returns. This supports the registration explanation;
it does not prove that exact revision is installed or diagnose every fetch error.
No generic OS repair or Machine Control code change was needed.

The final artifacts supersede the earlier extension hashes for this follow-up:

- Beta 0.4.0 SHA-256: `91bd1f361147eec19197c1d5843f0db231e15af564136f6348681d9afaa4850a`.
- Production identity 1.1.2 SHA-256: `15f5bca8430885141b388f187a7d7e98ceae182966b250b2bf70952ff4f9fd9b`.
- Unchanged Ubuntu 22.04 x86_64 package SHA-256: `95e6f05b35fcb22d4640f81175039759d78d1f2400f403b2e71626d7cccbd2a2`.

All four physical connection/help assets independently match the final beta
archive. The separate unpacked qualification identity omits the production
worker, action, external admission and Android permissions. It qualifies these
exact page assets and real optional Linux permission denial/grant, not installation
of either complete extension/store pair. Neither inherited extension is changed.
Both full archives pass their 17 portable cases; the health failures there are
injected, while the retained-old-document replacement uses a real HTTP cache.

On cohort A, the final page connects through the normal hostname to the unchanged
package and its React library. The real ChromeOS file picker supplies the owned
local `.torrent`, the Linux root chooser commits Linux Downloads, and the controlled
private transfer independently verifies 65,536 bytes with SHA-1
`9d18c16c51954b29cf228754f795ccdd5fd1aecf`. Stopped Linux produces the offline
unreachable result with setup/package/policy unknown. Normal Terminal startup,
relaunching the owned runtime and manual **Retry** return to connected React.
The actual torrent row remains 100% complete and the independently hashed payload
is unchanged, with the source offline during the recovery check.

Fixture calibration is separate from acceptance: a first profile under container
`/tmp` disappeared during normal VM startup and cannot qualify catalog retention.
The corrected recovery uses an owned durable user-data directory; it confirms
the row and bytes after VM stop/start. Its second intake reuses the controlled
download and is not a second independent transfer. A delivered file-picker typing
command did not enter text, so observed CDP input supplied the owned filename;
delivered input alone is not recorded as an OS-input pass. Extension removal also
required the actual Chrome confirmation and was checked against the resulting
extension inventory. No inherited ADB approval was granted.

The owned gateway is terminated and joined after checking its UID, executable and
profile. Its durable profile/package, owned downloaded payload, unpacked extension
and staged files are removed. Original beta/production extensions remain present;
all owned browser/Terminal tabs are closed, restoring the initial absence of page
tabs. The VM is stopped and its disk retained. Host fixture sources terminate and
their owned firewall rule and listeners are removed; the unrelated installed host
service remains intact. Local temporary logs/fixtures are removed after this
record. Common claims are unsupported on this adapter, so the exclusive target
marker acquired before mutations is released. Final doctor passes all ten checks;
inherited idle/lid availability policy is unchanged. Capture was not exercised.

This closes the bounded reachability/UX follow-up. Tactical 253 and M-08 remain
active/partial: current signed installation and candidate Launcher, full worker
and actual store pairs, real fresh Play installation, storage revocation/repair,
sleep/reboot/device network loss and the unreachable second cohort remain open.
Earlier hour observations and three-per-backend repetitions keep their original
Android/Linux-runtime limits. No production update, push or store publication occurs.

### Second Physical Cohort UX Review, 2026-10-03

The maintainer authorizes the now-reachable second cohort's Android/Play,
extension and Crostini validation, bounded bug fixes, incremental commits and a
local HTML review report with actual screenshots. Continue this tactical's
matrix and stopping conditions rather than infer product coverage from doctor.
Keep the requested report and selected captures as private local deliverables;
commit only machine-neutral evidence and reusable product changes.

Initial common doctor passes all ten checks on x86_64 ChromeOS 150
`16700.65.0`. Target-native EGL capture is exercised successfully. The common
claim interface remains unsupported; an exclusive target-side ownership marker
is acquired before interaction and must be released during cleanup. Initial ARC
ADB connection reports offline, so Android product readiness is still unknown.

Work order: record actual store/app/Linux state, exercise the ordinary available
store baseline, then isolated current candidates, explicit pairing/backend
choice, folder acquisition, controlled verified intake, view detachment and
recovery. Preserve inherited packages, libraries and grants. Existing host-built
artifacts may be reused only after source/version/hash reconciliation. No public
swarm, publication, account removal or destructive OS setup is implied.

Each report row records delivery route, observed result, screenshot where useful,
and remaining limits. Product fixes need focused regression checks and a repeat
of the affected physical flow. Inaccessible store/signing/lifecycle states stay
explicitly open. Cleanup joins owned processes, removes test state and retains
the requested review artifact; final doctor verifies appliance availability.
