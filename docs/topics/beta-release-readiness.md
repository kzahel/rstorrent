# Beta Release Readiness

Topic: `beta-release-readiness`

Status: **Active as of 2026-08-23.** RSTorrent is a functional unreleased
alpha with maintained desktop, Android, and iOS clients, but it does not yet
have supported distribution, upgrades, or release automation. Credential-free
cross-platform presubmit CI is proven on hosted runners. The public product
name is RSTorrent for the foreseeable release line; current work targets its
incubation beta. A later production
graduation is expected to retain JSTorrent's existing name, application
identity, and updater trust root, with best-effort legacy-state migration
scoped separately. It is not a beta requirement. This topic is the
authoritative beta gap ledger and release checklist. Tactical
[`157`](../tactical/157-beta-release-foundation.md) completed the first cleanup
slice, cross-platform presubmit Tactical
[`159`](../tactical/159-cross-platform-presubmit-ci.md) is complete, and
desktop release/updater Tactical
[`158`](../tactical/158-desktop-signed-packaging-and-updater.md) is **Now**.

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
| macOS desktop | signed/notarized DMG plus in-app updates | product and hosted unsigned arm64 app bundle pass; DMG, signing, install, updater, and x86_64 evidence absent |
| Windows desktop | signed per-user NSIS plus in-app updates | hosted unsigned x86_64 NSIS package passes; signing, clean install, and updater evidence absent |
| Linux desktop | AppImage plus in-app updates; DEB/RPM remain package-manager channels | hosted unsigned x86_64 AppImage passes; arm64, install, updater, and distro-package evidence absent |
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
  Desktop web, Cargo, and Tauri metadata currently agree at `0.1.0`; Android
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
  [`32569246987`](https://github.com/kzahel/rstorrent/actions/runs/32569246987)
  proves the credential-free runner and read-only permissions contract.
- [x] **CI-002 — Add shared-web end-to-end coverage.** Run the deterministic
  Playwright suite in CI, retain failure traces/screenshots, and keep public-
  swarm cases opt-in rather than required. The locked Chromium job passes 33
  deterministic tests with 12 live cases skipped locally and on the hosted
  `main` run; failures retain bounded traces/screenshots.
- [ ] **CI-003 — Add a desktop OS matrix.** Compile and package on macOS arm64
  and x86_64, Windows x86_64, and Linux x86_64; add Linux arm64 when a native
  runner or deliberate cross-build path exists. A compile-only matrix does not
  satisfy installer or update evidence. The hosted ordinary floor now passes
  an arm64 macOS app bundle, x86_64 Windows NSIS package, and x86_64 Linux
  AppImage. macOS x86_64, Linux arm64, signing, clean install/update, and
  release installer breadth remain open. The signed release workflow now
  defines both macOS architectures, both Linux architectures, Windows x86_64,
  and the full installer/update artifact set; its first hosted run remains the
  evidence gate.
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
- [ ] **CI-008 — Protect the release branch.** Require the release-readiness
  checks and review after their signal is stable. `main` was unprotected when
  audited on 2026-08-22.

### Product smoke and quality floor

- [x] **QA-001 — Core deterministic and local product suites pass.** On
  2026-08-22, Rust format/clippy/workspace tests, web typecheck and 248 passing
  unit tests with 2 skips, the production web build/CSP check, 33 deterministic
  Playwright tests with 12 live tests skipped, five Tauri desktop tests plus an
  unsigned arm64 macOS app, both Android ABIs plus Kotlin unit/lint/app and
  instrumentation APK builds, 25 iOS unit tests, 2 iOS UI tests, and an
  unsigned arm64 iOS archive passed locally.
- [ ] **QA-002 — Record a repeatable beta torrent cohort.** Cover small and
  large single/multifile v1 torrents, magnets and `.torrent` files, public and
  controlled discovery, selective files, pause/resume/restart/recheck,
  completion, opening, seeding, removal, low disk, corrupt data, and no-peer
  behavior without retaining payloads or sensitive logs.
- [ ] **QA-003 — Run installed release-candidate smokes.** Each claimed lane
  must install outside a source checkout, launch without development tools,
  complete the common cohort, survive relaunch/reboot where applicable, and
  uninstall with documented retained/removed state.
- [ ] **QA-004 — Establish a crash/support loop.** Users need an accessible
  version/build identity, copyable bounded diagnostics, known-issues link, and
  a report path. Automatic crash or analytics upload is not required for beta.
- [ ] **QA-005 — Review dependencies, notices, and release artifacts.** Verify
  license provenance, dependency advisories, archive contents, absence of
  secrets/development endpoints, and published checksums.

## Desktop Beta Checklist

### Packaging and platform integration

- [ ] **DESK-001 — Make the Tauri bundle real.** Tactical `157` supplies
  provisional icon assets and local bundle configuration. Native DMG/NSIS/
  AppImage builds, file metadata, clean-machine install, and uninstall remain
  required.
- [ ] **DESK-002 — Sign and notarize tagged builds.** Use the existing shared
  publisher Developer ID/notarization and Windows Azure signing setup. Missing
  credentials must fail a tagged build before publication; untagged CI must
  remain buildable without release credentials.
- [ ] **DESK-003 — Set least-privileged package ownership.** The default is a
  DMG-installed self-contained app on macOS, per-user NSIS on Windows, and a
  user-writable AppImage on Linux. MSI/DEB/RPM installs stay with their package
  managers and show a manual-update path.
- [ ] **DESK-004 — Finish lifecycle integration.** Decide single-instance,
  open-file/magnet handoff, platform close/quit/reopen behavior, crash restart,
  and whether tray/background operation is part of beta. File associations and
  tray are not automatically blockers if the limitation is explicit.
- [ ] **DESK-005 — Qualify native root pickers.** macOS works. Windows and
  Linux picker behavior, permissions, unavailable roots, restart, and repair
  remain open platform gates.

### Desktop updater contract

RSTorrent will adopt the proven
[`desktop-update-v1`](https://github.com/kzahel/desktop-release-kit/blob/main/contract/desktop-update-v1.md)
contract and shared multi-product update service. The local
`desktop-release-kit` canary is the operational reference; AtPiano's updater
is still pending and is not the compatibility oracle.

- [ ] **UPD-001 — Provision RSTorrent's update identity.** Generate one unique
  updater key, store only its public half in the app, add CI secrets for the
  private half/passphrase, and register an RSTorrent product route/config with
  the shared server. The distinct RSTorrent key pair was generated and all
  required updater/macOS/Windows repository secrets were confirmed present on
  2026-08-23. The public key, app endpoint, and product-owned server config are
  validated in source; production route registration remains open. Credential
  values never enter this repository. The JSTorrent updater private key is not
  part of the incubation beta workflow.
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
- [ ] **UPD-004 — Add release metadata validation.** Produce signed updater
  artifacts for `darwin-aarch64`, `darwin-x86_64`, `windows-x86_64`,
  `linux-x86_64`, and `linux-aarch64`; validate URLs, signatures, checksums,
  version agreement, immutable release assets, and draft-before-finalize. The
  source validators, negative tests, serialized five-leg workflow, and sole
  finalizer now pass local checks; real hosted assets remain required.
- [ ] **UPD-005 — Prove a real cross-version update.** On each supported
  desktop testbed, install an exact older public signed build, check through
  the production route, download/install/relaunch, and verify new application
  version/build identity. Source and metadata tests alone do not close this.
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
3. **Now — Tactical `158`: desktop signed packaging and updater adoption.**
   Implement the product-owned side of `desktop-update-v1`, release
   validation, draft artifacts, and platform testbed runbook. RSTorrent naming
   and the `com.jstorrent.rstorrent` desktop target are settled; configuration,
   public-key embedding, and production route provisioning remain open. The
   per-app key and required repository secrets are provisioned.
4. **Next — application identity and upgrade baseline.** Freeze package IDs,
   persistence compatibility, changelog, privacy/support, diagnostics export,
   and a repeatable cohort before any public installer.
5. **Later — platform release campaigns.** Close desktop, Android closed-
   testing, and iOS TestFlight gates independently with real older-to-newer
   installed evidence.
6. **Later — Tactical `153`.** Wired-LAN uTP scalability remains valuable
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
