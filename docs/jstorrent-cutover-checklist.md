# JSTorrent In-Place Cutover Checklist

Owner: [product-surfaces-and-migration](topics/product-surfaces-and-migration.md).
Campaign: [231](tactical/231-jstorrent-migration-working-campaign.md).
Candidate mechanics: [250](tactical/250-jstorrent-production-identity-candidates.md).

This is the acceptance checklist for replacing the existing desktop, Android
and extension products. A source check, emulator with disposable signatures,
or unsigned guest installation cannot satisfy a production delivery row.
Record exact source SHA, old/new artifact hashes, package/store versions, public
certificate/key fingerprints, platform/build, observation and recovery outcome
for each installed cohort. Keep personal paths, credentials and private keys out
of the public evidence. No row authorizes publication by itself.

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
passes original-key signing with the unchanged password. Package validation is
in progress; this signing-input proof does not close a delivery row.
The released macOS 0.2.1 app successfully checked its unchanged production
endpoint in an owned guest, but no successor install is qualified. The prepared
successor server descriptor passes controlled routing checks and is not active.
Android's repaired CI candidate builds signed APK/AAB and passes JVM tests/lint,
but final staging rejects its incubation certificate. Original GitHub APK signing,
Play upload certificate and Play app-signing certificate are separate gates.
The macOS arm64 CI app passes original-team Developer ID/notarization checks;
its updater signature fails the original root. A manual CI-bundle guest attempt
stops at the live-old-host alert before import and does not close an installed row.
Fresh macOS notarization then fails because Apple reports a missing/expired team
agreement; the account holder must resolve it before a new candidate can pass.
Windows signed NSIS/MSI, installed publisher/helper signatures and activation
checks pass, as do both Linux package lanes. Ordinary CI is fully green. These
partial checkpoints leave authenticated installed updating and all rows open.

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
  manager lanes have their documented upgrade route.
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

## Product, Recovery And Release Decision

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
