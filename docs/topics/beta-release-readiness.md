# Beta Release Readiness

Topic: `beta-release-readiness`

Work-selection policy: release tacticals may be active alongside unrelated
engine, client, platform, or product tacticals. **Active**, **Ready**, and
**Later** are planning states rather than a lock or required sequence. The
historical yield/resume/**Now** narrative below records past prioritization
only; it does not require future work to displace Tactical `158` or any other
active slice.

Status: **Active as of 2026-08-27.** RSTorrent desktop `0.1.0` is the first
public signed incubation release and `0.1.1` is its first updater-validation
release. Public `0.1.2` is the first signed candidate carrying the completed
desktop repairs and native bootstrap. Public ChromeOS Linux preview
`crostini-v0.1.0` now passes native x86_64/ARM64 builds, independent signed
asset validation, and exact website install/Launcher/relaunch evidence on the
physical x86_64 Chromebook; Android and iOS remain unreleased alpha lanes.
Credential-free eight-job cross-platform presubmit CI, a credentialed
five-target signed
desktop rehearsal, three tagged publications, production updater metadata, one
installed macOS arm64 launch smoke, and exact macOS arm64 and Linux arm64
`0.1.0`-to-`0.1.1` updates pass. Windows x86_64 replacement/relaunch also
passes under an automatic-loopback profile. Fresh default startup is blocked
in public `0.1.0` and `0.1.1` by local-network address selection; completed
Tactical `160` repairs that defect, and public `0.1.2` now contains the repair.
Completed Tactical `161` adds the native parented picker, makes the packaged
Linux picker self-contained, and
passes an installed unsigned Windows fresh-profile choose/cancel/repair/
restart campaign. The clean installed `0.1.1`-to-`0.1.2` update and Linux
x86_64 installed evidence remain open. First launch of the unsigned Windows
listener also exposed a Windows Security consent prompt that the signed
`0.1.2` installed campaign must characterize and document. Completed Tactical
`162` adds single-instance, default-on close-to-tray, persisted background
policy, visible manual updating, joined Quit/restart, native Linux arm64
packaging, and installed Windows x86_64/Linux arm64 lifecycle evidence.
Completed Tactical `164` adds desktop completion and attention notifications with
installed macOS arm64, Windows x86_64, and Linux arm64 evidence. Completed
Tactical `165` adds default-on desktop/Android active-work sleep inhibition,
removes Android's Wi-Fi lock, preserves truthful iOS finite-background policy,
and passes guest-native installed macOS arm64, Windows arm64, Linux arm64,
physical Android API 37, and physical iOS evidence. Explicit maintainer
direction temporarily yielded Tactical `158` to bounded desktop-bootstrap
Tactical `166`. That slice is complete after its exact-ID installed Chrome
`hello` and cold-launch smoke, and Tactical `158` remains active. Explicit
maintainer direction later on 2026-08-26 temporarily yields
that work to bounded ChromeOS Linux Tactical
[`167`](../tactical/167-chromeos-crostini-bundled-web-launcher.md). This is a
source-package and physical-device incubation slice, not a new public beta
lane or signed release. That slice is complete after the available physical
Chromebook lifecycle, detachable-transfer, preservation, and purge matrix;
the conditional full reboot was unavailable because the testbed has no
approved profile-login credential. Tactical `158` remains active.
Installed Intel macOS testing is deliberately omitted. The public product
name is RSTorrent for the foreseeable release line. A later production
graduation is expected to retain JSTorrent's existing name, application
identity, and updater trust root, with best-effort legacy-state migration
scoped separately. It is not a beta requirement. This topic is the
authoritative beta gap ledger and release checklist. Tactical
[`157`](../tactical/157-beta-release-foundation.md) completed the first cleanup
slice, cross-platform presubmit Tactical
[`159`](../tactical/159-cross-platform-presubmit-ci.md) is complete, and
bounded Windows listener repair Tactical
[`160`](../tactical/160-windows-local-network-address-selection.md) is
complete. Packaged desktop picker Tactical
[`161`](../tactical/161-packaged-desktop-folder-picker.md) is complete;
desktop lifecycle Tactical
[`162`](../tactical/162-desktop-single-instance-and-tray-lifecycle.md) is
complete; desktop-bootstrap Tactical
[`166`](../tactical/166-desktop-native-bootstrap-and-extension-scaffold.md) is
complete; signed release Tactical
[`158`](../tactical/158-desktop-signed-packaging-and-updater.md) retains its
open gates and remains active; Crostini Tactical `167` and
configured Linux headless-service Tactical `170` are complete.
Explicit maintainer direction temporarily yields Tactical `158` once more to
bounded platform-aware extension popup Tactical
[`168`](../tactical/168-platform-aware-extension-launcher.md). This polish adds
no release lane, permission, Android detection, or application-control scope;
its deterministic package and physical ChromeOS chooser/link/handoff spot
check pass. Tactical `158` remains active.
Explicit maintainer direction temporarily yields Tactical `158` to hosted
Crostini bootstrap/release Tactical
[`169`](../tactical/169-hosted-crostini-bootstrap-and-release.md). This is
non-publishing source plumbing and physical fixture validation; it does not
create a public ChromeOS Linux release or expand the beta lane. That slice is
complete after its deterministic and physical signed-fixture real-package
matrix. A subsequent explicitly authorized operation published non-latest
`crostini-v0.1.0`, deployed the website bootstrap, and passed exact public
x86_64 acceptance; Tactical `158` remains active.
Explicit maintainer direction on 2026-08-26 temporarily yielded Tactical `158`
to configured Linux headless-service Tactical
[`170`](../tactical/170-configured-linux-headless-service.md). Tactical `170`
is complete with deterministic source/package gates, isolated HTTPS/WSS proxy
evidence, and a real x86_64 Linux service/transfer/preservation campaign. It
creates no public release lane and does not absorb the owner remote-
authentication or relay campaign. Tactical `158` remains active with
its remaining signed Windows and Linux x86_64 acceptance gates unchanged.
Explicit maintainer direction later on 2026-08-26 temporarily yielded Tactical
`158` to signed headless release and trusted-LAN service Tactical
[`171`](../tactical/171-signed-headless-release-and-lan-service.md). It owns
the completed source-only signed distribution/update plumbing, one exact RFC 1918
unauthenticated full-owner mode, truthful browser presentation, and an
enabled healthy current-host service campaign. Public release/channel
deployment, unattended updating, system-wide ownership, firewall changes, and
Raspberry Pi mutation remain outside the slice. Tactical `158` remains active
with its open gates unchanged.
Maintainer direction on 2026-08-24 promotes OS-level `magnet:` and local
`.torrent` activation from a post-beta deferral to a beta usability gap;
completed Tactical
[`163`](../tactical/163-desktop-external-torrent-intake.md) closes that gap.
Installed Linux arm64, Windows x86_64-application, macOS arm64, and exact
hosted eight-job acceptance pass; the Windows package ran under Windows 11
arm64 x64 emulation, and the macOS campaign preserved JSTorrent's inherited
default handler. Maintainer direction on 2026-08-25 makes basic native desktop
completion and fatal/repair notifications a beta usability requirement;
completed Tactical
[`164`](../tactical/164-desktop-completion-and-attention-notifications.md)
closes that gap. Explicit maintainer direction on the same date makes active-
download sleep inhibition the next beta usability slice; Tactical `165` owns
desktop and Android settings/behavior plus iOS inapplicability before
returning signed-package ownership to Tactical `158`. That slice is complete.

Explicit maintainer direction on 2026-08-27 declares all `0.1.x` packages and
current platform previews unsupported incubation builds with disposable
application-owned state and contracts. They remain valid package, signing,
updater, and product-behavior fixtures, but they are not a persistence or
rollback baseline. A future version must be explicitly declared the first
supported beta or release before compatibility obligations begin; `0.2.0` is
only a possible version, not a selected one.
Explicit user direction immediately activated disposable-incubation state
Tactical [`179`](../tactical/179-disposable-incubation-state-epoch.md). It is
complete with a fresh schema-21 catalog and removal of compatibility-only
state readers while retaining bounded reset and external-payload safety.
Tactical `176` is also complete after its Xcode 26.6 simulator and unsigned
device-archive gates passed on 2026-08-29. Signed release Tactical `158`
remains active.
Explicit maintainer direction on 2026-08-30 activates focused ChromeOS Android
extension-control Tactical
[`194`](../tactical/194-chromeos-android-extension-control.md) alongside
Tactical `158`. It is the first migration-critical JSTorrent Android parity
slice: Android retains the only Rust engine/profile owner and the extension is
a detachable shared React presentation. It includes Android's
extension-triggered SAF picker and retained-root semantics so changing the
current root for new downloads does not discard grants used by older torrents.
It does not authorize store publication or turn other missing Android features
into implicit scope.
Completed Tactical
[`197`](../tactical/197-android-external-torrent-intake.md) now closes the
independent Android beta usability gap for external `magnet:` and local
`.torrent` activation. Exact manifest/runtime filters, generic confirmation,
the shared root/start/add flow, hostile-provider bounds, connected API 34
instrumentation, controlled transfer, privacy, temporary-grant revocation,
and cleanup pass. This does not select the production JSTorrent identity,
publish a store artifact, or close notification/network/lifecycle gates.

Explicit user direction on 2026-09-01 accepts JSTorrent-shaped installation
metrics, feedback context, and extension-uninstall survey behavior in ready
Tactical
[`208`](../tactical/208-installation-metrics-and-feedback-parity.md). The
capability is not retroactively a blocker for an incubation package, but any
build that enables its richer transmission must first satisfy the exact
disclosure, privacy, disable/reset, and hosted-recipient gates below.

## Scope And Release Definition

This topic answers whether a build is ready to be handed to people outside the
development team. It owns:

- the beta product boundary and deliberately deferred features;
- platform packaging, signing, installation, update, rollback, and uninstall
  readiness;
- presubmit, scheduled, release, and post-release validation gates;
- product identity, version, migration, privacy, support, licensing, and
  release-note gates; and
- the statused backlog of bounded tacticals needed to reach beta.

[`capability-readiness.md`](capability-readiness.md) remains the detailed
engine and application capability scoreboard and owns non-exclusive active and
ready work sets.
[`protocol-support.md`](protocol-support.md) owns exact BitTorrent protocol
claims. Platform and UI truth remains in
[`client-surfaces.md`](client-surfaces.md). This topic classifies those facts
for release rather than duplicating their design.

**Supported beta** means a versioned, signed build explicitly declared for
external testers with a documented supported platform and compatibility
boundary. Public availability, a local debug APK, unsigned Apple archive,
`tauri dev` window, website build, or headless web host does not make an
incubation build a supported beta.

Beta is a program with independent lanes. Desktop beta is not blocked on App
Store review, and mobile beta is not implied by a desktop tag:

| Lane | Intended beta channel | Current release state |
| --- | --- | --- |
| macOS desktop | signed/notarized DMG plus in-app updates | public `0.1.2` Developer ID-signed, notarized, and stapled app/DMG packages pass for arm64 and x86_64; its exact public arm64 DMG launches and repairs native-host registration in a bounded spot check; an exact `0.1.0`-to-`0.1.1` replacement/relaunch also passes; Intel installed testing is deliberately omitted |
| Windows desktop | signed per-user NSIS plus in-app updates | public `0.1.2` NSIS and MSI packages contain the completed desktop repairs and have valid expected-publisher Authenticode signatures plus installed activation-registry validation; public per-user NSIS replacement/relaunch passes only for `0.1.0`-to-`0.1.1` under an automatic-loopback profile, while the repaired clean-profile update remains open |
| Linux desktop | AppImage plus in-app updates; DEB/RPM remain package-manager channels | public `0.1.2` AppImage, DEB, and RPM packages plus updater artifacts pass for x86_64 and arm64, including extracted activation metadata; exact arm64 AppImage `0.1.0`-to-`0.1.1` replacement/relaunch and the current installed lifecycle/icon campaign pass, while x86_64 installed evidence remains absent |
| Linux headless | signed non-latest `headless-v*` GitHub Release plus pinned website bootstrap and explicit CLI apply | strict native x86_64/ARM64 workflow, signed manifest/bootstrap/check/apply source gates, and an exact enabled x86_64 trusted-LAN install pass; no public candidate or stable manifest is promoted, and native ARM64 install/update evidence is absent |
| Android, including ChromeOS | signed Android App Bundle through a closed testing channel | maintained Compose/in-process Rust/SAF app and hosted dual-ABI debug/test APK gates pass; release identity, signed AAB, emulator/store, and upgrade evidence absent |
| ChromeOS Linux | signed non-latest `crostini-v*` GitHub Release selected by the pinned website bootstrap | public `crostini-v0.1.0`, the deployed pinned bootstrap, production-key manifest, native x86_64/ARM64 packages, independent exact-asset validation, and physical x86_64 website install/Launcher/relaunch pass; physical native ARM64, full reboot, suspend, and installed update/rollback evidence remain absent |
| iOS/iPadOS | signed TestFlight build | maintained SwiftUI app plus hosted simulator tests and unsigned device archive pass; distribution identity, signing, TestFlight, and upgrade evidence absent |

The first external lane may ship when its own blockers and the shared product
blockers pass. Unsupported lanes must remain labelled development preview; the
project must not imply simultaneous availability.

## Release Policy

Checklist meanings are exact:

- `[x]` means repository evidence currently satisfies the stated gate.
- `[ ]` means open. Text after the item records whether it blocks every beta,
  one platform lane, or only a broader promise.
- A gate closes from recorded commands, artifacts, or testbed evidence, not
  from the presence of configuration or a code path.

A release candidate is cut from one reviewed commit. Versions are stable
three-component semantic versions. The application, package metadata,
changelog, tag, installer metadata, and updater response must agree. A tagged
workflow may create a draft release, but only a final validation job may make
it public. Tags, publication, store submission, and production-route changes
remain explicit maintainer actions.

Every `0.1.x` package and current platform preview may reset or replace
application-owned state rather than migrate it. Current application
identifiers, updater keys, and routes are operational values, not a promise
that an older incubation installation remains supported. Recognized obsolete
state may use a bounded documented reset; malformed, ambiguous, busy, or future
state fails closed. Reset never includes user-selected payload roots or
published content and never converts old records into verified authority.

The first version explicitly declared the supported beta or release freezes
its fresh application identifiers, updater trust, route, and persistence/API
baseline from that point forward. It has no migration obligation to any
earlier incubation build. Later supported versions must follow the
compatibility and rollback policy declared with that baseline.

## Shared Beta Blockers

### Product identity and release contract

- [x] **REL-001 — Freeze the beta name and publisher identity.** Maintainer
  direction on 2026-08-22 selects RSTorrent as the public product identity for
  the foreseeable release line, beginning with an incubation beta. RSTorrent
  remains independent from current JSTorrent installations. Maintainer
  direction on 2026-08-23 records
  the general later goal: graduate the proven implementation through a normal
  JSTorrent update retaining JSTorrent branding, `com.jstorrent.desktop`, and
  its existing updater trust root. Exact timing and best-effort state migration
  are later scope; released RSTorrent routes and clients do not silently change
  identity.
- [ ] **REL-002 — Freeze application identifiers.** Desktop currently uses
  the accepted operational `com.jstorrent.rstorrent`, and release validation
  rejects accidental drift.
  Android still uses the unreleased `org.rstorrent.bootstrap`, and iOS uses
  `org.rstorrent.ios.dev`. Incubation clients do not inherit an existing
  JSTorrent application/store identity, and no `0.1.x` value constrains the
  future supported release. The shared gate remains open for those mobile
  lanes and for explicit reconfirmation at each lane's support boundary.
- [ ] **REL-003 — Establish one release version source and bump procedure.**
  Desktop web, Cargo, and Tauri metadata currently agree at `0.1.2`; Android
  and iOS use independent provisional values. Release validation must reject
  drift for the lane being shipped.
- [x] **REL-004 — Add a changelog and release-note policy.** `CHANGELOG.md` and
  the desktop release runbook require supported behavior, known limitations,
  data reset/migration, and security/privacy changes for each release.
- [ ] **REL-005 — Declare the first supported persistence baseline.** Choose
  the first explicitly supported version and freeze only its fresh database
  and application-owned formats. No `0.1.x` migration is required. Exercise
  fresh creation, recognized-incubation reset, interrupted reset,
  corrupt/ambiguous/busy/future state, root loss, payload preservation, and the
  forward/rollback policy that begins with that release.
- [ ] **REL-006 — Define support, privacy, and legal presentation.** Ship
  license/notices, a privacy statement for network behavior and any update
  installation ID, support/report instructions, and a safe diagnostics export
  with user-visible contents. If Tactical `208` is enabled, the statement and
  in-product disclosure must additionally name the pseudonymous identifier,
  exact coarse statistics and recipients, query-string exposure, default-on
  choice, one-report override, durable disable, and reset behavior. The hosted
  feedback/uninstall pages must no longer claim that no usage data is
  collected.

### CI and repository health

- [x] **CI-001 — Add ordinary presubmit CI.** Required checks run Rust
  formatting, workspace clippy with warnings denied, workspace tests,
  generated-contract drift, web typecheck/unit/build, and workflow/config
  checks. Hosted `main` run
  [`32627431920`](https://github.com/kzahel/rstorrent/actions/runs/32627431920)
  proves the credential-free runner and read-only permissions contract across
  all seven Rust/interop, web/E2E, desktop, Android, and iOS jobs.
- [x] **CI-002 — Add shared-web end-to-end coverage.** Run the deterministic
  Playwright suite in CI, retain failure traces/screenshots, and keep public-
  swarm cases opt-in rather than required. The locked Chromium job passes 33
  deterministic tests with 12 live cases skipped locally and on the hosted
  `main` run; failures retain bounded traces/screenshots.
- [x] **CI-003 — Add a desktop OS matrix.** Compile and package on macOS arm64
  and x86_64, Windows x86_64, and Linux x86_64; add Linux arm64 when a native
  runner or deliberate cross-build path exists. A compile-only matrix does not
  satisfy installer or update evidence. Hosted ordinary run
  [`32627431920`](https://github.com/kzahel/rstorrent/actions/runs/32627431920)
  passes the credential-free package floor. Signed rehearsal
  [`32627436936`](https://github.com/kzahel/rstorrent/actions/runs/32627436936)
  passes both macOS architectures, both native Linux architectures, and
  Windows x86_64 with the complete intended package matrix. Clean install and
  installed update evidence remain separately open under `QA-003` and
  `UPD-005`.
- [ ] **CI-004 — Add Android gates.** Build both Rust ABIs, lint, unit test,
  assemble a release bundle, and run a bounded owned-emulator product smoke.
  Physical-device and ChromeOS evidence remains a release-candidate campaign,
  not an unattended presubmit mutation. Hosted presubmit now passes both Rust
  ABIs, generated Kotlin, lint/JVM tests, debug app APK, and instrumentation
  APK compilation; a signed release AAB and emulator run remain open.
- [x] **CI-005 — Add iOS gates.** Generate bindings/project, build the device
  Rust library, run simulator unit/UI tests, and create an unsigned release
  archive on a pinned macOS/Xcode runner. Signed TestFlight work remains a
  protected release job. The exact Xcode 26.6 hosted Apple leg passes generated
  drift, 25 unit tests, 2 UI tests, and the unsigned device archive.
- [ ] **CI-006 — Add a bounded controlled interoperability smoke.** Choose a
  short v1 magnet/torrent intake, transfer, publication, restart, and seeding
  path against pinned libtorrent. Keep the long matrix and public catalog out
  of ordinary PR latency. The hosted presubmit passes Tactical `159`'s locked
  exact first-piece transfer and cleanup as the initial floor; intake,
  publication, restart, and seeding in one broader application lifecycle
  remain open.
- [ ] **CI-007 — Repair scheduled performance CI.** The 2026-08-10 and
  2026-08-17 runs failed before tests because `astral-sh/setup-uv@v8` could not
  be resolved. The workflow now pins reviewed `setup-uv` `v8.3.2`; manual
  hosted run
  [`32568169955`](https://github.com/kzahel/rstorrent/actions/runs/32568169955)
  passes both smoke profiles and retains JSON artifact
  `performance-32568169955-1`. The first successful weekly scheduled run is
  still required before closing this gate.
- [x] **CI-008 — Decide release branch protection policy.** Maintainer
  direction on 2026-08-23 deliberately keeps direct `main` work available and
  does not make branch protection an incubation-beta gate. Tagged publication
  remains fail-closed behind source checks, all release legs, and the sole
  finalizer.

### Product smoke and quality floor

- [x] **QA-001 — Core deterministic and local product suites pass.** On
  2026-08-23, Rust format/clippy/workspace tests, web typecheck and 262 passing
  unit tests with 2 skips, the production web build/CSP check, 33 deterministic
  Playwright tests with 12 live tests skipped, release-tool tests, and an
  optimized macOS app passed locally. Hosted run
  [`32627431920`](https://github.com/kzahel/rstorrent/actions/runs/32627431920)
  additionally passes the full seven-job Rust/interop, web/E2E, desktop,
  Android dual-ABI/lint/test, and iOS simulator/archive matrix.
- [ ] **QA-002 — Record a repeatable beta torrent cohort.** Cover small and
  large single/multifile v1 torrents, magnets and `.torrent` files, public and
  controlled discovery, selective files, pause/resume/restart/recheck,
  completion, opening, seeding, removal, low disk, corrupt data, and no-peer
  behavior without retaining payloads or sensitive logs.
- [ ] **QA-003 — Run installed release-candidate smokes.** Each claimed lane
  must install outside a source checkout, launch without development tools,
  complete the common cohort, survive relaunch/reboot where applicable, and
  uninstall with documented retained/removed state. The public `0.1.0` arm64
  DMG passed a bounded `/Applications` install, notarized launch, updater-ID
  initialization, and graceful-quit smoke. An isolated macOS arm64 appliance
  also installed that exact public build and updated through production to
  `0.1.1` with replacement, relaunch, and retained private updater identity.
  Linux arm64 now passes the same exact public AppImage replacement/relaunch
  and updater-ID continuity check. Windows x86_64 passes replacement/relaunch
  only after selecting automatic-loopback because its fresh default listener
  profile fails startup in `0.1.0` and `0.1.1`. Public `0.1.2` now contains the
  completed repair and passes a bounded exact-DMG macOS arm64 launch/native-
  host spot check, but it has not run the signed Windows older-to-newer
  campaign. Linux x86_64, the common cohort, reboot/relaunch, and full
  uninstall policy remain open. A separate
  unsigned x86_64 NSIS campaign under Windows 11 arm64 x64 emulation now passes
  installed cold/visible/hidden magnet and file activation, cancellation,
  bounded failures, duplicate handling, tray Quit, uninstall, and exact
  inherited-state restoration.
  Incubation update acceptance does not require retaining `0.1.x` torrents,
  settings, roots, selection, verification state, or updater identity; it must
  prove signed replacement, clean launch or bounded reset, and preservation of
  user-selected payload content.
- [ ] **QA-004 — Establish a crash/support loop.** Users need an accessible
  version/build identity, copyable bounded diagnostics, known-issues link, and
  a report path. Automatic crash or analytics upload is not required for beta.
  Tactical `208`'s optional previewed coarse context may improve the report
  path, but it does not replace a user-visible diagnostics export or authorize
  automatic submission.
- [ ] **QA-005 — Review dependencies, notices, and release artifacts.** Verify
  license provenance, dependency advisories, archive contents, absence of
  secrets/development endpoints, and published checksums. All public `0.1.0`
  assets passed `SHA256SUMS`, manifest, target, signature, and immutable-URL
  checks. Public `0.1.2` passed the complete signed workflow and finalizer; its
  exact arm64 DMG also independently matched `SHA256SUMS` before the installed
  spot check. Dependency/notices review and reviewed archive-content policy
  remain open.

## Desktop Beta Checklist

### Packaging and platform integration

- [ ] **DESK-001 — Make the Tauri bundle real.** Tactical `157` supplies
  provisional icon assets and local bundle configuration. Hosted native DMG,
  NSIS, MSI, AppImage, DEB, and RPM builds now pass across the intended matrix.
  The public arm64 DMG also passed one installed macOS launch and graceful quit,
  followed by exact `0.1.0`-to-`0.1.1` replacement/relaunch in isolated macOS
  and Linux arm64 appliances. Windows x86_64 replacement/relaunch also passes
  under an automatic-loopback profile. Public `0.1.2` now contains the
  fresh-default and native root setup/repair changes and passes signed package
  validation, but its clean Windows installed update does not yet. Linux x86_64,
  uninstall, reset safety, and broader clean-machine evidence remain
  required; Intel Mac installed testing is deliberately omitted.
- [x] **DESK-002 — Sign and notarize tagged builds.** Use the existing shared
  publisher Developer ID/notarization and Windows Azure signing setup. Missing
  credentials must fail a tagged build before publication; untagged CI must
  remain buildable without release credentials. Credentialed rehearsal
  [`32627436936`](https://github.com/kzahel/rstorrent/actions/runs/32627436936)
  proves Developer ID signing, Apple notarization/stapling, and expected-
  publisher Authenticode signing. Tagged run
  [`32656926123`](https://github.com/kzahel/rstorrent/actions/runs/32656926123)
  repeated those checks and published only after every leg and the sole
  finalizer passed. Tagged `0.1.1` run
  [`32661616090`](https://github.com/kzahel/rstorrent/actions/runs/32661616090)
  repeated the complete matrix and finalizer successfully. Tagged `0.1.2` run
  [`32959820514`](https://github.com/kzahel/rstorrent/actions/runs/32959820514)
  passed the same five signed legs and finalizer, including extracted Linux
  activation metadata and installed Windows activation-registry validation.
- [ ] **DESK-003 — Set least-privileged package ownership.** The default is a
  DMG-installed self-contained app on macOS, per-user NSIS on Windows, and a
  user-writable AppImage on Linux. MSI/DEB/RPM installs stay with their package
  managers and show a manual-update path.
- [x] **DESK-004 — Finish beta lifecycle integration.** Tactical `162`
  implements one desktop owner, second-launch restoration, default-on
  tray/background operation, persisted close policy, visible manual updating,
  and joined close/Quit/restart shutdown. The credential-free eight-job matrix
  and installed Windows x86_64/Linux arm64 campaigns pass, including branded
  shell icons and zero-process Quit. Autostart, crash restart, and identity
  migration remain post-beta decisions; external file/magnet handoff is now
  owned separately by DESK-008 and Tactical `163`.
- [ ] **DESK-005 — Qualify native root pickers.** Tactical `161` is complete.
  Its parented native Tauri picker passes installed Windows choose, cancel,
  first-default, unavailable-root repair, and controlled process-restart
  semantics; hosted Linux x86_64 AppImage testing proves the packaged picker
  dependency and build. Installed Linux desktop/portal behavior remains open,
  so the cross-platform gate is deliberately not checked.
- [ ] **DESK-006 — Fix fresh-default Windows listener startup.** The public
  `0.1.0` and `0.1.1` Windows builds select `127.0.0.1` from the multicast
  source-route probe, reject it for local-network listening, fall back to
  `0.0.0.0`, and then fail application validation. Tactical `160` repairs the
  selector and wildcard fallback; public `0.1.2` contains that repair. Prove
  the signed package on a clean installed profile before calling the Windows
  lane runnable.
- [ ] **DESK-007 — Qualify Windows firewall consent.** First launch of the
  unsigned fresh-profile listener build displayed Windows Security Allow/
  Cancel consent. Choosing Cancel granted no broader firewall access and left
  the app plus root picker usable. The signed `0.1.2` installed campaign must
  record whether the prompt appears, define the supported private/public-
  network choice, and document incoming-reachability consequences. Automation
  must not silently grant a firewall rule.
- [x] **DESK-008 — Handle external magnets and torrent files.** Tactical
  `163` now registers the current RSTorrent packages for `magnet:` and
  local `.torrent` activation, forwards cold and warm input through the
  existing single desktop owner, reuses root/start options, and requires
  installed macOS arm64, Windows x86_64, and Linux arm64 proof. Deterministic
  and package gates plus all three installed campaigns pass. Windows exercised
  the real x86_64 NSIS/PE under Windows 11 arm64 x64 emulation. macOS proved
  targeted LaunchServices cold/visible/hidden delivery, cancellation, bounded
  failures, duplicate handling, tray Quit, cleanup, and retention of JSTorrent
  as the inherited default. Exact hosted run
  [`32775002484`](https://github.com/kzahel/rstorrent/actions/runs/32775002484)
  passed all eight platform jobs. This work does not adopt JSTorrent identity,
  extension routing, or remote `.torrent` URLs.
- [x] **DESK-009 — Notify on completion and fatal/repair state.** Completed
  Tactical `164` adds one native Rust notification owner. The exact standard
  Tauri package owns macOS/Windows delivery; Linux retains the same underlying
  exact native handle directly because the wrapper dropped it before GNOME
  could display or activate it. Completion and attention are edge-triggered
  and non-replaying; fresh settings enable both and allow users to suppress
  notifications only while RSTorrent is focused. The webview receives typed
  settings but no arbitrary OS-notification authority. Installed macOS arm64,
  Windows x86_64, and Linux arm64 evidence covers focused policy, hidden
  delivery, restart, settings, click behavior, joined Quit, and cleanup.
  Linux click restoration passes; macOS/Windows retain the tray fallback after
  measured standard-package click limits. Progress, aggregation, and mobile
  notification work remain outside the slice.
- [x] **DESK-010 — Prevent idle sleep during active downloads.** Completed
  Tactical `165` adds one default-on Tauri-only preference, authoritative
  `Starting`/`Downloading`/`Checking` policy, system-idle inhibition without
  display inhibition, and joined cleanup. Exact installed macOS arm64,
  Windows arm64, and Linux arm64 tests cover stalled work, minimized windows,
  preference changes, Pause, restart, Start, Quit, and cleanup. The exact-head
  unsigned Windows x86_64 package now repeats that native matrix on the native
  x86_64 appliance: SYSTEM-only request, more than 35 seconds held and
  minimized, off/on, Pause, joined Quit, paused process restart, Start, and
  final cleanup all pass. The next signed-candidate campaign still repeats the
  integrated package-trust/update case. Explicit sleep, lid close, shutdown,
  low-power policy, and seeding remain under ordinary OS policy.
- [x] **DESK-011 — Establish the beta extension bootstrap identity.** Tactical
  `166` adds the distinct bounded native-messaging host, target-triple Tauri
  sidecar packaging, per-user first-launch registration repair, NSIS cleanup,
  and a self-contained Manifest V3 seed whose only permission is
  `nativeMessaging`. Deterministic host/extension tests and an actual unsigned
  macOS app-bundle host `hello` pass. Chrome Web Store item
  `gcgoepclopkgijmclmlheafaglmbjlcc`, its public key, independently derived
  unpacked identity, and its exact native-host origin are pinned. The rebuilt
  app repairs a manifest containing only the production and beta origins.
  Chrome 151 on the installed unsigned macOS arm64 app proves the exact
  unpacked ID, native `hello` with the app stopped, and cold launch through
  **Open RSTorrent**. Full extension control, Crostini, hosted Windows/Linux
  package breadth, and production JSTorrent migration are not part of this
  beta-bootstrap gate.

### Desktop updater contract

RSTorrent will adopt the proven
[`desktop-update-v1`](https://github.com/kzahel/desktop-release-kit/blob/main/contract/desktop-update-v1.md)
contract and shared multi-product update service. The local
`desktop-release-kit` canary is the operational reference; AtPiano's updater
is still pending and is not the compatibility oracle.

- [x] **UPD-001 — Provision RSTorrent's update identity.** Generate one unique
  updater key, store only its public half in the app, add CI secrets for the
  private half/passphrase, and register an RSTorrent product route/config with
  the shared server. The distinct RSTorrent key pair was generated and all
  required updater/macOS/Windows repository secrets were confirmed present on
  2026-08-23. The public key, app endpoint, and product-owned server config are
  validated in source. The `/rstorrent` product descriptor was deployed to the
  shared production service and its health/product registration were verified
  on 2026-08-23. Credential values never enter this repository. The JSTorrent
  updater private key is not part of the incubation beta workflow.
- [x] **UPD-002 — Implement client behavior.** Add the Tauri updater/process
  plugins, stable random installation ID in the platform config directory,
  `X-CFU-Id`, exact check reason, a silent startup check after five seconds, a
  silent 24-hour check, bounded timeout/deduplication, manual check, visible
  release notes/progress/errors, explicit install, and relaunch only after
  successful installation. Native, controller, and component tests plus the
  full web regression suite pass locally.
- [x] **UPD-003 — Enforce package policy.** In-app replacement is allowed only
  for macOS app, Windows per-user NSIS, and user-writable Linux AppImage.
  MSI/DEB/RPM installations must use a visible manual/package-channel path.
- [x] **UPD-004 — Add release metadata validation.** Produce signed updater
  artifacts for `darwin-aarch64`, `darwin-x86_64`, `windows-x86_64`,
  `linux-x86_64`, and `linux-aarch64`; validate URLs, signatures, checksums,
  version agreement, immutable release assets, and draft-before-finalize. The
  source validators, negative tests, serialized five-leg workflow, and sole
  finalizer pass local checks. Hosted rehearsal
  [`32627436936`](https://github.com/kzahel/rstorrent/actions/runs/32627436936)
  produced all five private signed package sets. Tagged run
  [`32656926123`](https://github.com/kzahel/rstorrent/actions/runs/32656926123)
  then exercised the real draft, all required and package-specific
  `latest.json` entries, immutable GitHub release URLs, `SHA256SUMS`,
  complete-draft validation, and publication finalization. Independent public
  downloads and all five production routes passed after publication; see
  [`desktop-v0.1.0`](../evidence/desktop-v0.1.0.md). Tagged `0.1.1` run
  [`32661616090`](https://github.com/kzahel/rstorrent/actions/runs/32661616090)
  repeated the complete release matrix; all public checksums, the 15-key
  manifest, and current/older production-route probes passed. See
  [`desktop-v0.1.0-to-v0.1.1`](../evidence/desktop-v0.1.0-to-v0.1.1.md).
  Tagged `0.1.2` run
  [`32959820514`](https://github.com/kzahel/rstorrent/actions/runs/32959820514)
  passed the complete signed matrix and finalizer at exact commit `788e953`;
  its bounded public-DMG evidence is in
  [`desktop-v0.1.2`](../evidence/desktop-v0.1.2.md). Production-route and
  installed cross-version validation remain part of `UPD-005` rather than
  being inferred from publication.
- [ ] **UPD-005 — Prove a real cross-version package update.** On each
  supported desktop testbed, use an exact older public signed incubation build
  to check through the production route, explicitly approve replacement,
  relaunch, and verify the exact newer version/build and package trust. Old
  `0.1.x` torrents, settings, roots, selection, verification state, updater
  identity, and rollback are not retained-state requirements. If the newer
  build rejects its application-private state, it must take the declared
  bounded reset or fail-closed path while leaving user-selected payload
  content untouched and launching cleanly afterward. macOS arm64 and Linux
  arm64 now pass updater mechanics from exact
  public `0.1.0` to `0.1.1`, including explicit approval, replacement,
  relaunch, current-version checking, and private installation-ID continuity.
  Windows x86_64 proves the same updater mechanics and Authenticode continuity
  under an automatic-loopback profile, but must repeat from a fresh default
  profile after `DESK-006`. Linux x86_64 remains open. Intel macOS installed
  testing is deliberately omitted by maintainer direction; source and metadata
  tests alone are the only x86_64 macOS claim.
- [ ] **UPD-006 — Document update privacy and recovery.** Explain the random
  resettable installation ID, private server logging, automatic schedule,
  manual retry/download path, the absence of incubation rollback/state
  compatibility, statistics disable/reset behavior, anonymous checks when the
  disclosed preference is off, and behavior when an update, reset, or metadata
  service fails.

## Android/ChromeOS Beta Checklist

This checklist owns the independent RSTorrent Android beta lane. Updating the
installed `com.jstorrent.app` product is a stronger and separately authorized
operation; [`android-jstorrent-replacement.md`](android-jstorrent-replacement.md)
owns its package/state handoff, production-extension rollout, and explicit
disposition of current JSTorrent Android features.

- [x] **AND-001 — Maintain a real first-party product client.** Compose,
  in-process Rust, foreground-service lifecycle, SAF storage, generated UniFFI
  contract, dual ABI packaging, AVD, ChromeOS, and physical Android evidence
  exist. Tactical `157` graduates the complete module from `experiments/` to
  `clients/android` without splitting these owners.
- [ ] **AND-002 — Freeze application identity and upgrade semantics.** The beta
  is a distinct RSTorrent listing rather than an update/replacement for
  `com.jstorrent.app`; select its durable package ID and exercise data/state
  coexistence without silently claiming the existing identity.
- [ ] **AND-003 — Create a signed release App Bundle.** Configure release
  signing through protected CI/store credentials, version code/name checks,
  minification/resource rules, mapping retention, and artifact inspection.
- [ ] **AND-004 — Qualify the closed-testing channel.** Install from the store
  on representative Android and ChromeOS devices, prove store replacement from
  a disposable older fixture without requiring state retention, complete the
  cohort from fresh current state, recover foreground/background transitions,
  repair a revoked root, open content, and remove/uninstall cleanly.
- [ ] **AND-005 — Complete store and platform declarations.** Data safety,
  privacy, foreground service, notification, local-network/network behavior,
  content rating, listing text/screenshots, and support link must match actual
  behavior.
- [ ] **AND-006 — Retire Android platform deprecations.** The current build
  still warns on the legacy activity-result path plus notification and
  system-bar APIs. Migrate or deliberately bound each before a target-SDK or
  toolchain update turns warning debt into a release failure.
- [x] **AND-007 — Make active-download wake ownership explicit.** Completed
  Tactical `165` turns the former unconditional CPU/Wi-Fi lock path into a
  durable default-on preference over authoritative active operational states
  and retains only one service-owned partial CPU wake lock. A physical API 37
  device proves persistence, acquisition through screen-off Dozing, absence of
  a Wi-Fi lock, service-stop release, and exact data/root cleanup.
- [x] **AND-008 — Qualify ChromeOS Android extension control.** Tactical `194`
  carries the release-built shared React presentation through explicit
  Android-approved pairing to the one foreground-service application/engine/
  profile owner. Physical ChromeOS 150 now passes pairing, identity, packaged
  React control, Compose convergence, extension-triggered retained SAF roots,
  current/default future binding, referenced-root rejection, independent grant
  loss/repair, restart persistence, local `.torrent` intake, two controlled
  root-bound transfers, a detached 4 MiB transfer, reconnect, and exact
  cleanup without exposing a URI or descriptor. The product listener now binds
  only to ARC's fixed guest address. The exact extension connects from ChromeOS,
  while the Chromebook Wi-Fi address refuses raw TCP and the formerly
  successful spoofed-Host/Origin request from another LAN device. Shutdown,
  credential removal, uninstall, and post-cleanup refusal pass. Android-through-
  Compose remains a separate supported choice.
- [x] **AND-009 — Add background completion and actionable failure
  notifications.** Android now has the bounded service-native completion and
  fatal/storage-repair owner, default-on preferences, three truthful channels,
  initial/reset/recheck suppression, exact taps, denied/blocked visible-only
  shutdown, and prompt target-35 timeout handling. Deterministic, dual-ABI,
  API 34/35 connected, genuine controlled transfer/repair, resource, and
  cleanup gates pass. Completed Tactical
  [`198`](../tactical/198-android-completion-and-attention-notifications.md)
  also passes exact physical completion-to-torrent and attention-to-Storage
  taps, notification removal, zero restart/recheck replay, genuine repair,
  malformed-path restoration, and terminal cleanup on ChromeOS 150/API-33.
  Composed evidence covers denied-visible-only behavior, Compose-explained
  permission grant, companion disconnect/reconnect, the real ongoing-
  notification **Stop** action, listener refusal, and exact package/
  credential/power cleanup. This slice does not by itself claim a complete
  granted-background lifecycle.
  Tactical
  [`200`](../tactical/200-android-product-background-lifecycle.md) now
  implements that contract: explicit default-off continuation, qualifying
  active work, default stop on completion, optional continued seeding,
  activity handoff, intent-preserving joined shutdown, finite duration,
  Android-15 exhausted-quota fencing, and a bounded authenticated-companion
  reconnect grace. Connected API 28/35, controlled Home/reopen, task removal,
  process recovery, completion, seeding upload, shortened timeout, dual-ABI,
  and repository gates pass. Tactical `200` and `JAR-009` initially closed
  through maintainer-approved evidence composition; a later physical ChromeOS
  150/API-33 campaign directly adds Home/reopen, sticky recovery, completion,
  controlled background upload, seeding disable, authenticated companion
  retain/grace/reconnect/expiry, notification eligibility and Stop, relaunch,
  listener refusal, and exact cleanup. Reboot autostart,
  low-battery shutdown, VPN, and proxy remain outside both slices.
- [ ] **AND-010 — Add metered-network safety and bound VPN policy.** JSTorrent
  Android enforces unmetered/Wi-Fi-only and VPN-only prerequisites. Tactical
  [`199`](../tactical/199-android-live-unmetered-network-enforcement.md) now
  implements RSTorrent's required default-off unmetered cost policy with
  fail-closed startup, joined live BitTorrent egress convergence, automatic
  intent-preserving recovery, truthful generated/Compose state, and exact
  cleanup. Deterministic, dual-ABI, and installed API 28/35 AVD evidence
  passes. The unmetered gate remains open only for an explicitly authorized
  physical current-API phone handoff campaign; no physical device was used.
  VPN-only may remain a disclosed later feature unless a dedicated tactical
  proves Android network binding, handover races, socket leakage, tracker/DHT/
  peer coverage, and fail-closed recovery.
- [x] **AND-011 — Handle external magnets and torrent files.** Completed
  Tactical `197` makes the installed provisional Android product a narrow
  `ACTION_VIEW` handler for `magnet:`, exact BitTorrent-MIME `content://`, and
  supported `.torrent` content paths. Cold/warm delivery converges through one
  `singleTop` activity and service owner with generic root/start confirmation,
  one bounded ephemeral queue/read job, duplicate and retry-once semantics,
  temporary grants, privacy-preserving diagnostics, and no generated-contract
  change. JVM, lint/package, connected API 34, hostile-provider, exact-transfer,
  resource, revocation, and cleanup gates pass. Signed package/store handler
  declarations remain owned by `AND-003` through `AND-005`.

## iOS/iPadOS Beta Checklist

- [x] **IOS-001 — Maintain a real first-party product client.** SwiftUI,
  in-process Rust/UniFFI, app Documents and qualified selected roots,
  simulator/physical lifecycle evidence, system preview, and reproducible
  unsigned/development archives exist.
- [ ] **IOS-002 — Freeze the distribution bundle identity.** Replace the
  `.dev` identifiers with a durable RSTorrent namespace while preserving the
  explicit independent-product and later-migration posture.
- [ ] **IOS-003 — Add complete app artwork and metadata.** Tactical `157`
  supplies provisional buildable artwork; final App Store icon, display copy,
  screenshots, privacy manifest review, support/privacy links, and localization
  review remain open.
- [ ] **IOS-004 — Automate a signed archive/export.** Use protected Apple
  credentials/profiles outside the repository, validate archive contents, and
  upload the exact reviewed build to TestFlight.
- [ ] **IOS-005 — Prove TestFlight replacement and lifecycle.** Install a
  disposable older fixture, replace it through TestFlight without requiring
  state retention, validate clean current state or bounded reset plus payload
  safety, run phone/iPad cohort cases, and record finite-background
  limitations. Compatibility begins only with the explicitly supported build.
- [ ] **IOS-006 — Make storage bridging Swift 6 concurrency-clean.** Current
  archives warn that asynchronous `NSLock` and `DispatchGroup.wait` calls in
  `PlatformStorageBridge` become errors in Swift 6 language mode. Replace them
  with an async-safe ownership design before enabling that mode or requiring a
  toolchain that does so.
- [x] **IOS-007 — Keep power behavior truthful.** Completed Tactical `165`
  adds no idle-timer assertion or general keep-awake setting. A current signed
  physical-device archive exposes the existing finite-background explanation
  across both Settings pages and resumes cleanly after Home/background, while
  Tactical `149` remains the lifecycle authority.

## Beta Feature Boundary

The beta MVP is an honest ordinary downloader/seeder, not feature parity with
every mature BitTorrent client. The following are required product behavior
for a lane unless the platform-specific checklist narrows them:

- magnet and local `.torrent` intake with clear errors and duplicates;
- path/root choice, selective files, pause/resume, queueing, recheck, remove
  with safe keep/delete choices, restart, completion, open/share through the
  platform's supported path, and basic speed/connection/seeding settings;
- truthful progress, no-peer/retry/storage/checking explanations, bounded logs,
  and version/support information; and
- repeatable common public v1 behavior plus the controlled correctness cohort.

These useful gaps are **not automatic beta blockers** when disclosed:

- embedded/progressive media playback and a media catalog;
- search, plugins, torrent creation, tracker mutation, rich numeric file
  priorities, ratio/time seed goals, and durable transfer totals;
- production remote access, relay, third-party pairing, and migration from
  legacy JSTorrent. Extension control remains required only for the explicitly
  advertised ChromeOS Android companion-presentation lane under `AND-008`;
- VPN-only, proxy, interface-specific, and broader power policy. Android
  metered-network safety is separately promoted to `AND-010`;
- local service discovery (BEP 14), HTTP web seeds (BEP 17/19), tracker scrape
  (BEP 48), hole punching (BEP 55), PCP/NAT-PMP, and full IPv6/uTP breadth; and
- complete BEP 52 coverage. Pure-v2 and hybrid behavior may be labelled beta
  or experimental according to the public-swarm cohort actually passed.

Missing features become blockers if the product advertises them, if their
absence creates corruption/security risk, or if cohort evidence shows they
are necessary for ordinary advertised downloads. Public incomplete-swarm
reliability and ordinary performance remain evidence requirements even though
no single optional BEP is mandatory.

## Tactical Status And Release Backlog

List order preserves the campaign record and rough release context; it is not
a required execution sequence. Independent items may become active
concurrently when directed, without demoting other active work.

1. **Complete — Tactical `157`: beta release foundation.** Established this
   ledger, graduated the Android module path, added provisional platform
   artwork/bundle metadata, corrected entry-point status, and preserved
   historical evidence.
2. **Complete — Tactical `159`: cross-platform presubmit CI.** Hosted Rust/web,
   native desktop, Android, iOS, deterministic E2E, and short locked
   loopback-interoperability jobs pass; the repaired performance workflow also
   retains a successful manual smoke artifact.
3. **Complete — Tactical `162`: desktop single-instance and tray lifecycle.**
   One application owner, default-on persisted background policy,
   close-to-tray, visible manual updater action, joined Quit/restart, native
   Linux arm64 packaging, release-only Windows GUI launch, and installed
   Windows x86_64/Linux arm64 proof pass. File/magnet handoff remains outside
   that completed slice and is now owned by Tactical `163`; autostart stays
   deferred.
4. **Active — Tactical `158`: desktop signed packaging and updater adoption.**
   The product-owned `desktop-update-v1` client, signed package workflow,
   release validation, per-app key, public configuration, production route,
   five-platform hosted rehearsal, three tagged publications, one installed
   macOS arm64 launch smoke, and exact macOS arm64 and Linux arm64
   `0.1.0`-to-`0.1.1` production-route updates are complete. Windows x86_64
   updater replacement also passes under an automatic-loopback profile.
   Completed Tactical `160` repairs fresh-default address selection on `main`;
   Completed Tactical `161` now proves unsigned installed Windows first-root
   setup; completed Tacticals `163`--`165` add external intake, notifications,
   and active-work sleep inhibition. Public `0.1.2` now carries those repairs,
   and its signed package matrix plus bounded macOS arm64 launch/native-host
   spot check pass. Repeat clean Windows from the default under the revised
   disposable-state `UPD-005` contract, characterize firewall consent, and
   run Linux x86_64. Intel
   macOS installed testing is deliberately omitted. Explicit maintainer
    direction temporarily yielded this item to now-complete Tacticals `169`,
    `170`, `171`, and now `179`. Its open gates remain unchanged.
5. **Complete — Tactical `163`: desktop external torrent intake.** The
   bounded shell/UI implementation, package gates, and installed Linux arm64,
   Windows x86_64-application, and macOS arm64 cold/visible/tray-hidden/
   cancel/failure/duplicate/Quit campaigns pass. Exact hosted run
   `32775002484` passed all eight platform jobs.
6. **Complete — Tactical `164`: desktop completion and attention
   notifications.** One Rust-owned, edge-triggered, non-replaying owner,
   versioned typed desktop preferences, deterministic/package gates, and
   installed macOS arm64, Windows x86_64, and Linux arm64 evidence pass.
7. **Complete — Tactical `165`: cross-platform active-download sleep
   inhibition.** One default-on desktop/Android preference, level-triggered
   system-idle ownership, iOS inapplicability, and real cleanup pass through
   machine-control on every available desktop/mobile target. Tactical `158`
   remains active.
8. **Complete — Tactical `160`: Windows local-network address selection.**
   Wildcard binding remains, only a concrete eligible address is reported,
   the bounded Windows best-route fallback and native CI regression pass, and
   signed installed proof returns to Tactical `158`.
9. **Complete — Tactical `161`: packaged desktop folder picker.** The native
   parented Windows picker, self-contained packaged Linux picker, stable root
   boundary, and installed Windows cancel/select/repair/restart evidence pass.
10. **Complete — Tactical `179`: disposable incubation state epoch.** The
   fresh schema-21 catalog resets every recognized schema 1 through 20 before
   startup. DHT-v1, desktop-settings-v1/v2, and browser-appearance-v1/v2
   readers are gone; current-format validation, crash convergence, fail-closed
   hostile/future handling, and external payload remain exact.
11. **Complete — Tactical `176`: durable High file priority.**
   High/Normal/Skip persistence, weighted ordinary scheduling, streaming
   composition, first-party presentation, Linux/web/Android gates, 28 iOS
   simulator tests, and the unsigned arm64 device archive pass.
12. **Ready — supported-release boundary.** Keep all `0.1.x` and current
   previews explicitly disposable, then choose and declare the first supported
   version with a fresh persistence/API baseline. No migration from incubation
   state is required. Complete its changelog, privacy/support, diagnostics
   export, and repeatable cohort before making that support claim.
13. **Later — platform release campaigns.** Close desktop, Android closed-
   testing, and iOS TestFlight gates independently with real signed
   replacement evidence under the disposable-incubation policy.
14. **Later — Tactical `153`.** Wired-LAN uTP scalability remains valuable
   engine evidence but no longer displaces the explicit beta-readiness
   campaign.
15. **Complete — Tactical `167`: ChromeOS Crostini bundled web launcher.** The
    bundled Rust backend and React UI, static user service, registered Linux
    Launcher, and exact beta-extension handoff pass source gates and the
    available warm, twice-stopped-VM, detachable-transfer, preservation, and
    purge matrix on the physical Chromebook. The conditional full reboot was
    unavailable because no approved profile-login credential exists; signed
    public packages remain later breadth. Tactical `158` remains active.
16. **Complete — Tactical `168`: platform-aware extension launcher.** ChromeOS
    omits the irrelevant desktop-native flow and presents the exact published
    JSTorrent Android listing beside ChromeOS Linux; desktop platforms retain
    only desktop behavior. The deterministic reviewed `0.3.0` package and
    physical ChromeOS chooser, exact Play destination, and warm Crostini
    handoff pass without new permissions or availability claims.
17. **Complete — Tactical `169`: hosted Crostini bootstrap and release.** The
    pinned updater-key one-command installer, strict signed manifest, native
    x86_64/ARM64 `crostini-v*` workflow, release runbook, deterministic failure
    corpus, and exact non-public x86_64 physical package repair/failure matrix
    pass. A later explicitly authorized operation published non-latest
    `crostini-v0.1.0`, deployed the website bootstrap, independently verified
    every signed public artifact, and passed the exact website install,
    Launcher, and stop/relaunch flow on physical x86_64. Physical native
    ARM64, full reboot, suspend, and installed update/rollback remain open.
18. **Complete — Tactical `170`: configured Linux headless service.** One
    ordinary-user Rust application owner, exact React assets, strict durable
    root/listener/origin/Basic secret-file configuration, and a disabled-by-
    default systemd user unit pass deterministic and real x86_64 Linux gates.
    HTTPS/WSS proxy control, zero-view transfer, completed re-seeding, idle
    reachability, joined restart, rollback-safe repair, uninstall preservation,
    and exact cleanup pass. x86_64/ARM64 packages construct byte-identically;
    native ARM64 systemd and public distribution remain unclaimed.
19. **Complete — Tactical `171`: signed headless release and trusted-LAN
    service.** A strict signed two-architecture `headless-v*` lane, verified
    bootstrap, operator-approved CLI/browser update discovery and apply,
    exact RFC 1918 `lan-none` admission with truthful full-control UI, and an
    enabled healthy current-host x86_64 service campaign pass. No public
    publication, unattended update, system-wide service, firewall change, or
    Raspberry Pi mutation occurred. Tactical `158` remains active.
20. **Complete — Tactical `194`: ChromeOS Android extension control.** Preserve
    the JSTorrent companion user journey without its extension-owned engine or
    raw IO daemon: package the shared React UI in the beta extension, pair it
    explicitly over the same-device ARC boundary, and attach it to the one
    Android foreground application/profile owner. Provide the shared root UI
    through Android's SAF picker and retain older grants for their bound
    torrents when a new root becomes current. Physical cold launch, pairing,
    coexistence, two-root lifecycle, repair, detached transfer, reconnect, and
    cleanup pass. The companion is fixed to ARC's guest address; ChromeOS
    connects through ARC while the Chromebook Wi-Fi address refuses the same
    port and the formerly successful spoofed-Host/Origin request. Store
    publication, production JSTorrent extension changes, state import, media,
    notifications, and dynamic network policy remain outside the slice.

Each implementation item requires its own bounded tactical. These status
classifications are not authorization to tag, publish, alter production
routing, create store listings, or provision credentials without the required
maintainer action.

## Maintenance Contract

Every release-readiness tactical updates this checklist with actual evidence,
newly discovered blockers, and deliberate deferrals. Keep item IDs stable.
Do not check a platform gate from another platform's build, infer update
success from generated artifacts, or turn a changing public swarm into a
required presubmit.

Before handing a build to external testers, copy the applicable open items
into a version-specific release candidate record with the exact commit,
versions, artifacts, checksums, CI runs, installed/update evidence, known
issues, and final publication decision. This living topic remains the backlog;
it is not itself a release attestation.
