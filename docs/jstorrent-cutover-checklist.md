# JSTorrent In-Place Cutover Checklist

Owner: [product-surfaces-and-migration](topics/product-surfaces-and-migration.md).
Campaign: [231](tactical/231-jstorrent-migration-working-campaign.md).
Candidate mechanics: [250](tactical/250-jstorrent-production-identity-candidates.md).
Finish-line execution: [259](tactical/259-jstorrent-finish-line-checks.md)
(desktop, Android/ChromeOS, extension and website; iOS excluded).

This is the acceptance checklist for replacing the existing desktop, Android
and extension products. A source check, emulator with disposable signatures,
or unsigned guest installation cannot satisfy a production delivery row.
Record exact source SHA, old/new artifact hashes, package/store versions, public
certificate/key fingerprints, platform/build, observation and recovery outcome
for each installed cohort. Keep personal paths, credentials and private keys out
of the public evidence. No row authorizes publication by itself.

## Finish-Line Qualification Checkpoint, 2026-10-07

Tactical [259](tactical/259-jstorrent-finish-line-checks.md) owns current
execution. Source `a35934d2` passes every in-scope CI job; iOS is excluded.
Original-key Android 1.0.26/code 26 at `37673687f15235840f3ac47ab044f2cd3e2f2445`
passes normal release, 121 JVM tests, lint and independent package checks after
Tactical 260 and Tactical 262's Network-status branding correction. Actual
old/new screens, original-signed APK capture and all 24 API-35 navigation
cases pass. Production extension 1.1.2 also passes package checks. Authenticated
Play reconfirms production 23, internal 25 and uploaded maximum 25; the existing
Web Store item is published/draft 1.1.1. Neither candidate is uploaded.
All five fresh
signed desktop packaging legs pass. The collector is canceled during minisign
installation; independent local assembly validates all 23 asset hashes, 15
updater selections and ten original-root signatures from the same attempt.
The current signed macOS arm64 package passes independent signing/notarization
checks and eight manual installed migration checks. Automatic updating and the
broader installed matrix remain open. Signed Linux ARM and x64 manual migration pass eight assertions and six
refusal routes, with restored guest state. Current Linux x64 product pixels
render; ARM black captures and the old x64 blank window retain visual gaps.
Current ordinary and companion writer upgrades pass API 28/35 with disposable
signatures; actual old writers, two SAF roots, partial verification, restart and
both mixed-version companion pairs pass. Ordinary mode also passes emulator
reboot. Final Android application source 37673687 passes all seven isolated
physical SAF/source recovery cases on both cohorts, with independent hashes and
successful cleanup. Shared-identity background attempts fail after reaching
Complete and normal joined shutdown; they are not hour passes. Corrected runs
use unique infohashes and bounded 40-MiB hourly fixtures, followed by actual
completed-file upload and joined Live-library reopen. These runs are active.
Tactical 263's fresh web/website audit, strict review, affected builds/tests,
46 configured E2E cases (14 existing skips), byte-identical extension packages
and desktop notices pass; its residual upstream cache behavior and static Astro
call-path limits remain explicit. Sibling hosted graph checks continue under
Tactical 264. Managed Play and broader physical delivery gates remain open.
All owned VMs are stopped between uses and their idle claims released.
The tactical and ignored local report bind exact results and next actions.

## Historical Qualification Checkpoint, 2026-10-04

Tactical [257](tactical/257-play-upgrade-and-fresh-signed-candidates.md) passes a
bounded physical Play 1.0.23-to-1.0.25 standalone upgrade: managed signer,
installation identity, paused completed/partial rows, unchanged pre-update bytes,
unmetered setting, completed-file offline recheck, partial-file verified
completion and force-stop/relaunch. This supplies a subset of D-03/A-01/A-02/A-04;
full rows remain open for companion writers, other trees/providers, reboot,
staggered production extension updates and broader historical cohorts.

The observed internal code-25 Canary label is corrected in final Android
1.0.26/code 26. Exact-source f5860c99 passes all ten executed CI jobs and
Website. Non-publishing signed desktop 0.3.0 passes all five targets,
macOS notarization/stapling/Gatekeeper and Windows publisher/activation gates;
independent verification passes its exact 23-asset inventory/hashes and all ten
original-root updater payload signatures. Android APK/AAB independently pass
original upload signing, JSTorrent labels, launcher, both ABIs, alignment and
notices. See the [desktop receipt](evidence/jstorrent-fresh-desktop-257.json)
and [Android receipt](evidence/android-fresh-candidate-257.json). These are
package gates, not current-source installed update qualification. Code 26 is
not uploaded to Play; internal/production tracks, production feeds and Web Store
remain unchanged. Earlier installed evidence retains its own source/hashes.

## Internal Android Delivery Checkpoint, 2026-10-04

Tactical [255](tactical/255-android-play-internal-replacement.md) verifies the
original upload key against Play, corrects the existing CI signing inputs, and
publishes validated 1.0.25/code 25 to the existing app's internal track. The
separate managed app-signing certificate is recorded in its evidence. Production
remains 1.0.23/code 23. This supersedes the signing-input/Play-inspection blockers
in the historical checkpoint below. D-03 and installed migration rows retain the unqualified cohorts listed above.

## Source Preparation Checkpoint, 2026-10-01

Tactical 250 prepares desktop 0.3.0, Android 1.0.25/code 25 and extension
1.1.2 against pinned production identities/public trust. Candidate source
negative tests, original-extension desktop admission, generated Android
release launcher/package metadata, isolated debug assembly and both extension
ZIP lanes pass. An unsigned macOS arm64 app bundle has the production metadata,
native helper and original branding notices; it was not launched.

The original desktop updater key is now provisioned and its CI nonce proof
passes with the unchanged password. Original Android inputs remain unprovisioned.
Play/Web Store maximum versions and app-signing certificate remain unverified. No signed
installed or store row below is complete. Next: confirm those delivery inputs,
then qualify exact signed candidates in owned installed cohorts, including the same production extension ID and physical Chromebook.
Minimum API 28 (Android 9) is accepted on 2026-10-01; API 26/27 are outside
the replacement cohort and unsupported-device guidance still needs review.
Keep historical `jstorrent:` launch/pairing and existing website integration
explicitly in the launch-route review; the candidate currently retains magnet
and torrent intake rather than claiming every legacy route.

Tactical [251](tactical/251-jstorrent-ci-candidates-and-installed-update.md)
started signed CI candidates using existing inputs. Its first CI signature proves
those desktop inputs match the incubation key, **not** JSTorrent's retained root.
The maintainer-supplied original key replaces that secret; fresh run
[36831643488](https://github.com/kzahel/rstorrent/actions/runs/36831643488)
passes original-key signing with the unchanged password. Windows/Linux package
lanes complete; independent checks pass eight original-key payload signatures
and all 16 receipt hashes. Attempt 1 fails its Apple agreement gate; this
partial matrix does not close a delivery row.
The initial released macOS 0.2.1 check used its unchanged production
endpoint in an owned guest; Tactical 251 did not install the successor. The prepared
successor server descriptor passes controlled routing checks and is not active.
Android's repaired CI candidate builds signed APK/AAB and passes JVM tests/lint,
but final staging rejects its incubation certificate. Original GitHub APK signing,
Play upload certificate and Play app-signing certificate are separate gates.
The first macOS arm64 CI app passes original-team Developer ID/notarization checks;
its updater signature fails the original root. A manual CI-bundle guest attempt
stops at the live-old-host alert before import and does not close an installed row.
Initial macOS notarization fails Apple's missing/expired agreement gate; its
fresh recheck below now passes.
Windows signed NSIS/MSI, installed publisher/helper signatures and activation
checks pass, as do both Linux package lanes. Ordinary CI is fully green. These
partial checkpoints leave authenticated installed updating and all rows open.
Fresh attempt 2 of run 36831643488 now passes both macOS jobs: Apple accepts
both apps and outer DMGs, with no DMG issues; signing, stapling and Gatekeeper
checks pass. The agreement error is no longer blocking CI. The collector refuses
the failed-only retry because it has only two current-attempt legs. Fresh complete
non-publishing run [36845370571](https://github.com/kzahel/rstorrent/actions/runs/36845370571) now passes at the same
source SHA: all five lanes, exact release collection and all ten original-root
updater payload signatures. See [Apple recheck evidence](evidence/jstorrent-ci-candidate-251-apple-recheck.json). Complete candidate collection passes;
installed migration remains open. No production feed/store change is made.

Accepted [252](tactical/252-jstorrent-opt-in-upgrade-rehearsal.md) records an
opt-in test cohort at the normal HTTPS endpoint, one pinned candidate and a
real owned-guest installed migration. The reviewed server route is deployed;
its three-ID test configuration is disabled after the bounded qualification.

Server commit `f45885c` implements the opt-in route and passes 86 tests plus
lint/typecheck/build. It is pushed and deployed with explicit maintainer
authorization; 27 live HTTPS isolation checks retain byte-identical ordinary
responses. The trial is disabled and owned candidate assets removed afterward. Exact signed CI packages
pass manual installed replacement on Linux x64, Windows x64 and macOS arm64:
four records from two profiles, supported settings, rechecking, startup fences,
registered legacy refusal, stable restart and source/payload preservation. See
[manual evidence](evidence/jstorrent-upgrade-trial-252-manual-migration.json).
Original released HTTPS automatic updating passes on Linux x64 and macOS
arm64, and the normal macOS Check for Updates / Install & Restart path passes.
Linux signature refusal and interrupted-download retries pass; macOS signature
refusal and retry also pass, with the original executable hash preserved. See
[installed evidence](evidence/jstorrent-upgrade-trial-252-installed.json).
Windows automatic and ordinary GUI updates, signature refusal/retry and
interrupted-download/retry now pass the same migration/restart assertions.
Installed publisher/uninstaller continuity and file/registry/firewall restoration
pass; the isolated trial is disabled and normal routing reverified.
The old Linux release renders
blank in the selected VM under default, DMA-BUF-disabled and software/X11
settings, leaving its GUI path open. Broader installed cohorts, controlled
repair and real extension journeys remain open. Native catalog branding is corrected after
the candidate source and needs a fresh signed build.

## Candidate Identity And Delivery

- [ ] **D-01 Desktop identity:** JSTorrent name/icons, `com.jstorrent.desktop`,
  existing Tauri updater trust root and `updates.jstorrent.com` route. Candidate
  version exceeds every selected installed source. Beta route/key remain separate.
- [ ] **D-02 Desktop signatures:** retained updater key verifies final signatures;
  wrong key fails. macOS Developer ID/team, notarization/stapling/Gatekeeper and
  Windows publisher/signature match the accepted production delivery lane.
  Reconcile Linux hashes/signatures and exact packaged native host/notices.
- [ ] **D-03 Android identity:** `com.jstorrent.app`; versionCode exceeds all Play
  tracks, including closed/internal/testing, and selected GitHub APKs. Record
  upload certificate separately from the existing Play app-signing certificate.
  Play-generated APK updates an installed Play build without uninstall/data clear.
- [ ] **D-04 Extension identity:** existing Web Store item
  `dbokmlpefliilbjldladbimlcfgbolhk`, public manifest key derives that ID, and
  version exceeds every published track. Review permission/CSP/launch-route changes
  and store disclosure. Existing browser profile receives an actual extension
  update; a second unpacked ID is insufficient.
- [ ] **D-05 Update services:** inspect candidate response/signature, OS/arch
  selection, version ordering and retained channel behavior. Check rollback/stop
  route before offering the candidate. Changes to GitHub asset ownership or the
  update server's product descriptor are explicit, reviewed operations.

## Desktop Installed Matrix

Run on supported macOS arm64/x64, Windows x64 and Linux x64/arm64 package lanes;
select representative historical sources and browsers. Keep each unrun lane open.

- [ ] **P-01 Installed update:** real installed JSTorrent checks its normal route,
  verifies the candidate, replaces itself, relaunches and retains its OS identity.
  NSIS replacement, AppImage path and macOS bundle path are exercised. Package
  manager lanes have their documented upgrade route. Bounded 252 evidence passes
  original 0.2.1-to-0.3.0 HTTPS automatic delivery on Linux x64, Windows x64
  and macOS arm64, plus the ordinary Windows/macOS UI. Other architectures
  and the Linux old-app UI remain open; the complete row is not satisfied.
- [ ] **P-02 Legacy shutdown:** running desktop/old native hosts, open legacy
  extension pages and idle pre-handshake helpers cannot remain payload writers.
  Managed old registrations refuse the legacy protocol. Failure to fence or prove
  shutdown stops import visibly; a retry preserves source and payload.
- [ ] **P-03 Import:** union multiple browser profiles; paused, active/partial,
  completed, pending magnets, cached metadata, Normal/Skip selections and multiple
  roots. Mapped settings carry best effort. Fresh and latest-format destinations,
  duplicate torrents, malformed records, unknown/missing roots and unavailable
  source stores have explicit outcomes. Foreign verified bits never bypass checking.
- [ ] **P-04 Durability:** crash before/inside/after the atomic catalog transaction,
  retry, process restart and OS restart. Exactly one completion marker; no duplicate
  records, source resurrection after clear or partial catalog publication.
- [ ] **P-05 Real bytes:** independently hash valid/corrupt/partial payload before
  and after; valid retained bytes recheck, corrupt bytes repair, interrupted content
  resumes. Missing roots are not recreated and repair restores the original binding.
- [ ] **P-06 Launch continuity:** existing launcher/pinned shortcut, tray Open,
  `.torrent` file and magnet links, browser toolbar and native window reach one
  owner/library. Include spaces/non-ASCII paths, cold/warm launches, simultaneous
  launch, explicit Quit, passive reconnect and browser restart.
- [ ] **P-07 Mixed desktop versions:** new extension + old desktop gives an explicit
  desktop update requirement; old extension + new desktop cannot write through the
  retired protocol. Both new presentations show the migrated library. Credentials
  rotate on replacement and repeated pairing/registration repair preserves state.

## Android And Chromebook Installed Matrix

Controlled API 28/35 writer upgrades already pass in [247](tactical/247-android-ordinary-writer-upgrade.md)
and [248](tactical/248-chromeos-staggered-upgrade.md). Complete production/physical
rows independently; those tests use disposable signing and owned emulators.

- [ ] **A-01 Ordinary Android writers:** existing standalone and ChromeOS companion
  write real supported settings and paused/partial/completed torrents before the
  installed upgrade. Actual production package/signature, library and private
  downloads persist. Retained launcher component/pinned shortcut opens the new app.
- [ ] **A-02 SAF:** two different user-selected trees, exact per-torrent binding,
  real retained read/write grants, process restart, reboot, revoked/regranted tree
  and unavailable/removable provider. Preserve unrelated bytes; copied URI text
  never establishes permission. Include physical Chromebook ARCVM evidence.
- [ ] **A-03 Staggered stores:** old/old remains ordinary; new extension/old Android
  requests an app update without old pairing/I/O; old extension/new Android leaves
  standalone Android usable; new/new approves fresh pairing and controls the same
  migrated catalog. Exercise an existing production extension ID updated in place.
- [ ] **A-04 Android lifecycle:** foreground/background setting, unmetered network
  policy, leaving native/browser views, detached transfer completion, picker cancel,
  force-stop/relaunch and reboot. Device notification/power preferences behave as
  documented. No second engine/profile or legacy credential authority appears.
- [ ] **A-05 Historical cohorts:** qualify selected Play versions, companion remote
  KV and browser-local fallback/older routing explicitly. Crostini automatic
  migration stays out of scope and its library remains separate.
- [ ] **A-06 ChromeOS onboarding:** on both physical Chromebooks, qualify Play
  not enabled/setup unfinished, real app installation, unavailable/failed
  installation and mixed-version recovery. Record actual versus injected
  states, exact artifacts and every unrun condition; sideload/emulator evidence
  cannot close the Play journey. Tactical 253 passes six injected packaged-browser
  journeys and bounded cohort-A sideload/extension recovery, including real
  rejection, declared-window expiry and terminal manual cancellation. Real Play and
  cohort-B acceptance stay open.
- [ ] **A-07 Troubleshooting and Linux fallback:** before backend connection,
  explain the observed failed stage and manual next action; offer explicit
  Crostini setup where permitted. Qualify Linux-not-enabled, package absent,
  stopped VM and unshared folder. Explain separate libraries, preserve Android
  state and show an unsupported outcome if neither backend is available.
  Cohort-A offline recovery and exact Linux package runtime pass. Normal Terminal
  startup restores registration; current isolated browser file-picker intake,
  stopped-VM manual Retry and durable-profile row/byte retention pass. Current
  signed installation/candidate Launcher and the remaining setup/share/store
  cohorts keep full fallback acceptance open.
- [ ] **A-08 Physical repetition:** Tactical
  [253](tactical/253-chromeos-onboarding-recovery-and-physical-qualification.md)'s
  bounded cold-launch, verified transfer, view detach, interrupted connection,
  storage repair, restart and lifecycle cohort passes on both devices. Every
  unqualified route remains explicit before ChromeOS rollout. Cohort A passes
  three independent byte-verified recovery repetitions per backend and both
  controlled 60-minute observations. Android detached completion and actual
  Remove/keep/restart pass. Linux hour observations and three repetitions remain
  runtime only; the follow-up adds bounded real browser/file-picker and
  source-offline VM recovery evidence. Current storage repair,
  sleep/reboot/device network loss and remaining cohort-B acceptance stay open.
  Owned test state, pairing storage and the stopped VM baseline are restored.

## Product, Recovery And Release Decision

After the current source/package gates, prioritize the remaining work in this
order. These are qualification and product decisions, not publication authority:

1. Review corrected Android code 26 for a separately authorized internal Play
   release; code 25's visible Canary label cannot be repaired in place.
2. Qualify companion/browser-local writers and the actual production extension
   ID updated in place, then repeat mixed-store pairings on both physical cohorts.
3. Complete the two-tree/reboot/grant-loss and sleep/network-loss matrix; include
   retained bytes, actual upload serving and independently verified repair.
4. Requalify installed delivery against the fresh signed desktop bytes where
   source changes matter. Extend macOS x64/Linux arm64 and the old Linux GUI
   cohorts; previous signed-candidate evidence remains pinned to its own hashes.
5. Select the supported persistence/platform scope, feature dispositions, soak
   window and recovery responsibility, then review the exact shipment capsule.
   Keep unrun rows unchecked or explicitly exclude their unsupported scope.

- [ ] **R-01 Feature disposition:** review recorded unsupported VPN, battery/plugin
  and other gaps. Migrate only implemented matching semantics.
  Minimum API 28 (Android 9) is accepted; API 26/27 remain on the old app and
  outside the replacement cohort. Validate clear OS-update/unsupported-device
  guidance, including staggered extension updates with no available Play update.
- [ ] **R-02 User outcomes:** report imported/already-present/skipped/failed counts,
  actionable missing-folder repair and retry/recovery. Explain supported setting
  changes and dropped features. Verify voluntary support/export and privacy wording;
  do not include paths/hashes/credentials in automatic telemetry.
- [ ] **R-03 Recovery:** interrupt updater/install/migration, disk full, busy/locked
  store and denied registration/grant. Failure preserves source/payload and has an
  actionable retry. Old-engine rollback must not run concurrently with successor
  writers or pretend its retained verified bits are current.
- [ ] **R-04 Candidate soak:** real transfers, seeding, restart/sleep/wake and
  browser/native lifecycle over an agreed observation window. Record known gaps,
  selected cohorts, stop thresholds and rollback responsibility.
- [ ] **R-05 Shipment approval:** exact source and artifacts, all required CI,
  store version/certificate review, final checklist disposition, staged cohort and
  independent update order reviewed. Publishing/tagging/store uploads require an
  explicit instruction; this checklist prepares that review.
