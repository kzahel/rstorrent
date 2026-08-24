# Beta Release Readiness

Topic: `beta-release-readiness`

Status: **Active as of 2026-08-24.** RSTorrent desktop `0.1.0` is the first
public signed incubation release and `0.1.1` is its first updater-validation
release; Android and iOS remain unreleased alpha lanes. Credential-free cross-
platform presubmit CI, a credentialed five-target signed desktop rehearsal,
two tagged publications, production updater metadata, one installed macOS
arm64 launch smoke, and exact macOS arm64 and Linux arm64
`0.1.0`-to-`0.1.1` update passes. Windows x86_64 replacement/relaunch also
passes under an automatic-loopback profile, but fresh default startup is
blocked in the public `0.1.0` and `0.1.1` builds by local-network address
selection. Completed Tactical `160` repairs that defect on `main` and adds a
passing native Windows regression; a newer signed package and exact installed
proof remain open. The current public packages and `main` also lack a native
Windows download-root picker; Tactical `161` is **Now** to close that blocker
and make the packaged Linux picker self-contained before publication resumes.
Linux x86_64 remains open and
installed Intel macOS testing is deliberately omitted. The public product
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
[`161`](../tactical/161-packaged-desktop-folder-picker.md) is **Now**;
desktop release/updater Tactical
[`158`](../tactical/158-desktop-signed-packaging-and-updater.md) is paused at
its next publication boundary.

## Scope And Release Definition

This topic answers whether a build is ready to be handed to people outside the
development team. It owns:

- the beta product boundary and deliberately deferred features;
- platform packaging, signing, installation, update, rollback, and uninstall
  readiness;
- presubmit, scheduled, release, and post-release validation gates;
- product identity, version, migration, privacy, support, licensing, and
  release-note gates; and
- the ordered backlog of bounded tacticals needed to reach beta.

[`capability-readiness.md`](capability-readiness.md) remains the detailed
engine and application capability scoreboard and owns exactly one **Now**.
[`protocol-support.md`](protocol-support.md) owns exact BitTorrent protocol
claims. Platform and UI truth remains in
[`client-surfaces.md`](client-surfaces.md). This topic classifies those facts
for release rather than duplicating their design.

**Beta** means a versioned, signed build intentionally offered to external
testers with a documented supported platform and upgrade path. A local debug
APK, unsigned Apple archive, `tauri dev` window, website build, or headless web
host is not a beta release.

Beta is a program with independent lanes. Desktop beta is not blocked on App
Store review, and mobile beta is not implied by a desktop tag:

| Lane | Intended beta channel | Current release state |
| --- | --- | --- |
| macOS desktop | signed/notarized DMG plus in-app updates | public Developer ID-signed, notarized, and stapled app/DMG packages pass for arm64 and x86_64; an installed arm64 launch smoke and exact `0.1.0`-to-`0.1.1` replacement/relaunch pass; Intel installed testing is deliberately omitted |
| Windows desktop | signed per-user NSIS plus in-app updates | public NSIS and MSI packages have valid expected-publisher Authenticode signatures; per-user NSIS replacement/relaunch passes under an automatic-loopback profile, but a fresh default profile cannot start because local-network address selection falls back to an invalid wildcard |
| Linux desktop | AppImage plus in-app updates; DEB/RPM remain package-manager channels | public AppImage, DEB, and RPM packages plus updater artifacts pass for x86_64 and arm64; exact arm64 AppImage replacement/relaunch passes, while x86_64 installed evidence remains absent |
| Android/ChromeOS | signed Android App Bundle through a closed testing channel | maintained Compose/in-process Rust/SAF app and hosted dual-ABI debug/test APK gates pass; release identity, signed AAB, emulator/store, and upgrade evidence absent |
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

Until the first external beta, development data may be reset rather than
migrated. The first published build freezes its application identifiers,
updater key, update route, and a supported persistence/schema baseline. Every
later build must either migrate that baseline transactionally or fail closed
without corrupting or silently reinterpreting user state.

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
  the accepted `com.jstorrent.rstorrent`, and release validation rejects drift.
  Android still uses the unreleased `org.rstorrent.bootstrap`, and iOS uses
  `org.rstorrent.ios.dev`. Incubation clients do not inherit an existing
  JSTorrent application/store identity; the shared gate remains open for those
  mobile lanes.
- [ ] **REL-003 — Establish one release version source and bump procedure.**
  Desktop web, Cargo, and Tauri metadata currently agree at `0.1.1`; Android
  and iOS use independent provisional values. Release validation must reject
  drift for the lane being shipped.
- [x] **REL-004 — Add a changelog and release-note policy.** `CHANGELOG.md` and
  the desktop release runbook require supported behavior, known limitations,
  data reset/migration, and security/privacy changes for each release.
- [ ] **REL-005 — Freeze the first supported persistence baseline.** Exercise
  upgrade from the oldest supported beta database and application-owned files,
  including crash during migration, corrupt/newer schema, root loss, and
  rollback policy.
- [ ] **REL-006 — Define support, privacy, and legal presentation.** Ship
  license/notices, a privacy statement for network behavior and any update
  installation ID, support/report instructions, and a safe diagnostics export
  with user-visible contents.

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
  profile fails startup. Linux x86_64, the common cohort, reboot/relaunch, and
  full uninstall policy remain open.
- [ ] **QA-004 — Establish a crash/support loop.** Users need an accessible
  version/build identity, copyable bounded diagnostics, known-issues link, and
  a report path. Automatic crash or analytics upload is not required for beta.
- [ ] **QA-005 — Review dependencies, notices, and release artifacts.** Verify
  license provenance, dependency advisories, archive contents, absence of
  secrets/development endpoints, and published checksums. All public `0.1.0`
  assets passed `SHA256SUMS`, manifest, target, signature, and immutable-URL
  checks; dependency/notices review and reviewed archive-content policy remain
  open.

## Desktop Beta Checklist

### Packaging and platform integration

- [ ] **DESK-001 — Make the Tauri bundle real.** Tactical `157` supplies
  provisional icon assets and local bundle configuration. Hosted native DMG,
  NSIS, MSI, AppImage, DEB, and RPM builds now pass across the intended matrix.
  The public arm64 DMG also passed one installed macOS launch and graceful quit,
  followed by exact `0.1.0`-to-`0.1.1` replacement/relaunch in isolated macOS
  and Linux arm64 appliances. Windows x86_64 replacement/relaunch also passes
  under an automatic-loopback profile, but fresh-default startup does not.
  Linux x86_64, uninstall, retained-state, and broader clean-machine evidence
  remain required; Intel Mac installed testing is deliberately omitted.
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
  repeated the complete matrix and finalizer successfully.
- [ ] **DESK-003 — Set least-privileged package ownership.** The default is a
  DMG-installed self-contained app on macOS, per-user NSIS on Windows, and a
  user-writable AppImage on Linux. MSI/DEB/RPM installs stay with their package
  managers and show a manual-update path.
- [ ] **DESK-004 — Finish lifecycle integration.** Decide single-instance,
  open-file/magnet handoff, platform close/quit/reopen behavior, crash restart,
  and whether tray/background operation is part of beta. File associations and
  tray are not automatically blockers if the limitation is explicit.
- [ ] **DESK-005 — Qualify native root pickers.** Tactical `161` is **Now**.
  It provides parented native Tauri behavior for Windows and packaged Linux,
  retains macOS native behavior, and must prove Windows choose, cancel,
  first-default, restart, and repair semantics. Installed Linux behavior
  remains separately explicit if only its hosted package gate is available.
- [ ] **DESK-006 — Fix fresh-default Windows listener startup.** The public
  `0.1.0` and `0.1.1` Windows builds select `127.0.0.1` from the multicast
  source-route probe, reject it for local-network listening, fall back to
  `0.0.0.0`, and then fail application validation. Tactical `160` repairs the
  selector and wildcard fallback on `main`; prove the first signed package
  carrying it on a clean installed profile before calling the Windows lane
  runnable.

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
- [ ] **UPD-005 — Prove a real cross-version update.** On each supported
  desktop testbed, install an exact older public signed build, check through
  the production route, download/install/relaunch, and verify new application
  version/build identity. macOS arm64 and Linux arm64 now pass from exact
  public `0.1.0` to `0.1.1`, including explicit approval, replacement,
  relaunch, current-version checking, and private installation-ID continuity.
  Windows x86_64 proves the same updater mechanics and Authenticode continuity
  under an automatic-loopback profile, but must repeat from a fresh default
  profile after `DESK-006`. Linux x86_64 remains open. Intel macOS installed
  testing is deliberately omitted by maintainer direction; source and metadata
  tests alone are the only x86_64 macOS claim.
- [ ] **UPD-006 — Document update privacy and recovery.** Explain the random
  resettable installation ID, private server logging, automatic schedule,
  manual retry/download path, rollback expectations, and behavior when an
  update or metadata service fails.

## Android/ChromeOS Beta Checklist

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
  on representative Android and ChromeOS devices, update from an older build,
  complete the cohort, recover foreground/background transitions, repair a
  revoked root, open content, and remove/uninstall cleanly.
- [ ] **AND-005 — Complete store and platform declarations.** Data safety,
  privacy, foreground service, notification, local-network/network behavior,
  content rating, listing text/screenshots, and support link must match actual
  behavior.
- [ ] **AND-006 — Retire Android platform deprecations.** The current build
  still warns on the legacy activity-result path plus Wi-Fi, notification, and
  system-bar APIs. Migrate or deliberately bound each before a target-SDK or
  toolchain update turns warning debt into a release failure.

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
- [ ] **IOS-005 — Prove TestFlight upgrade and lifecycle.** Install an older
  beta, update through TestFlight, validate retained state/roots and schema,
  run phone/iPad cohort cases, and record finite-background limitations.
- [ ] **IOS-006 — Make storage bridging Swift 6 concurrency-clean.** Current
  archives warn that asynchronous `NSLock` and `DispatchGroup.wait` calls in
  `PlatformStorageBridge` become errors in Swift 6 language mode. Replace them
  with an async-safe ownership design before enabling that mode or requiring a
  toolchain that does so.

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
- production remote access, extension control, relay, pairing, and migration
  from legacy JSTorrent;
- VPN-only, metered-network, proxy, interface-specific, and power policy;
- local service discovery (BEP 14), HTTP web seeds (BEP 17/19), tracker scrape
  (BEP 48), hole punching (BEP 55), PCP/NAT-PMP, and full IPv6/uTP breadth; and
- complete BEP 52 coverage. Pure-v2 and hybrid behavior may be labelled beta
  or experimental according to the public-swarm cohort actually passed.

Missing features become blockers if the product advertises them, if their
absence creates corruption/security risk, or if cohort evidence shows they
are necessary for ordinary advertised downloads. Public incomplete-swarm
reliability and ordinary performance remain evidence requirements even though
no single optional BEP is mandatory.

## Ordered Tactical Queue

1. **Complete — Tactical `157`: beta release foundation.** Established this
   ledger, graduated the Android module path, added provisional platform
   artwork/bundle metadata, corrected entry-point status, and preserved
   historical evidence.
2. **Complete — Tactical `159`: cross-platform presubmit CI.** Hosted Rust/web,
   native desktop, Android, iOS, deterministic E2E, and short locked
   loopback-interoperability jobs pass; the repaired performance workflow also
   retains a successful manual smoke artifact.
3. **Paused — Tactical `158`: desktop signed packaging and updater
   adoption.**
   The product-owned `desktop-update-v1` client, signed package workflow,
   release validation, per-app key, public configuration, production route,
   five-platform hosted rehearsal, two tagged publications, one installed
   macOS arm64 launch smoke, and exact macOS arm64 and Linux arm64
   `0.1.0`-to-`0.1.1` production-route updates are complete. Windows x86_64
   updater replacement also passes under an automatic-loopback profile.
   Completed Tactical `160` repairs fresh-default address selection on `main`;
   publish a newer signed package, repeat clean Windows from the default, and
   run Linux x86_64 after Tactical `161` closes first-root setup. Intel macOS
   installed testing is deliberately omitted.
4. **Complete — Tactical `160`: Windows local-network address selection.**
   Wildcard binding remains, only a concrete eligible address is reported,
   the bounded Windows best-route fallback and native CI regression pass, and
   signed installed proof returns to Tactical `158`.
5. **Now — Tactical `161`: packaged desktop folder picker.** Add the native
   parented Windows picker, make packaged Linux self-contained, preserve the
   stable root boundary, and prove installed Windows selection and restart.
6. **Next — application identity and upgrade baseline.** Freeze package IDs,
   persistence compatibility, changelog, privacy/support, diagnostics export,
   and a repeatable cohort before any public installer.
7. **Later — platform release campaigns.** Close desktop, Android closed-
   testing, and iOS TestFlight gates independently with real older-to-newer
   installed evidence.
8. **Later — Tactical `153`.** Wired-LAN uTP scalability remains valuable
   engine evidence but no longer displaces the explicit beta-readiness
   campaign.

Each implementation item requires its own bounded tactical. This ordering is
not authorization to tag, publish, alter production routing, create store
listings, or provision credentials without the required maintainer action.

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
