# JSTorrent In-Place Cutover Checklist

Desktop, Android/ChromeOS, the existing extension and website are in scope.
iOS is excluded. Linux is best effort: AppImage only for this desktop release,
maintained bundled libraries and CLI/headless fallback; no older-distro backport
campaign or production DEB/RPM qualification. See the
[accepted scope](topics/product-direction.md#linux-release-scope-accepted-2026-10-09).

This document keeps the current tasks and acceptance contracts. Dated execution,
failures and historical signed candidates live in
[Tactical259](tactical/259-jstorrent-finish-line-checks.md) and the ignored
per-slice finish-line reports. Earlier passes remain bound to their exact artifacts.
Use the shorter [update scenarios](jstorrent-update-scenarios.md) for recovery
copy and independent store timing.

## Current finish-line tasks, 2026-10-10

Tactical290 now corrects Windows installer artwork locally. Its actual native
presentation harness passes, but it changes Windows packaging inputs after
the frozen a7 build. Fresh original-signed Windows packaging and focused
installed/updater checks remain required; existing a7 qualification is preserved
as historical exact-byte evidence. All other package inputs stay unchanged.

- [x] Complete branding and recovery-copy corrections, local web/extension
  checks, actual controlled API35 upgrades and narrow-window/drawer captures.
- [x] Prepare AppImage-only website handoff in its owning repository at
  `c84fed4b`; descriptor stays disabled and the public website is unchanged.
- [x] Deliver the reviewed Android27 AAB only to existing internal testers.
  Play confirms availability; production23, testers and device counts are
  unchanged. The generated APK independently verifies managed signing,
  API28 minimum, both ABIs and16-KiB ZIP/ELF alignment.
- [x] Observe physical Play26-to27 update on one retained Chromebook: UID,
  first-install date, Play installer and URI grants remain unchanged. Its
  actual managed27 normal Shutdown removes its task/service and normal reopen
  returns Live after saving statistics opt-out. Exact grants remain unchanged;
  populated-library and reboot acceptance are separate.
- [x] Qualify actual managed27 controlled1MiB download/file confirmation and
  three populated paused Shutdown/reopen cycles with independent hashes and
  persisted folder grant retained. Exact owned row/file/marker cleanup passes;
  the selected empty test folder remains. Removed-detail navigation polish is
  recorded; background/hour/reboot and populated old-store migration stay open.
- [x] Verify actual extension inventories: B has none; A has unpacked1.1.1 and
  stale unpacked Beta0.4.0. Close tabs individually before windows on both
  Chromebooks; independent fresh launches restore only one new tab, then close
  that too. These unpacked installations do not qualify actual store updates.
- [x] Finish original-root signed desktop build and bounded identity/signature
  qualification at
  frozen `a7ee65ed`, [run37958746928](https://github.com/kzahel/rstorrent/actions/runs/37958746928).
  All five lanes and collector pass, as does main CI37958695272. Independently
  qualify15 core hashes, six original-root signatures/six wrong-root refusals,
  11 selectors, both Mac trust/notary/staples and native Windows publishers.
- [x] Repeat bounded signed Linux x64 and Mac ARM migration/restart with native
  screenshots. Both pass eight assertions, stable four-row restart and preserved
  source/payload; scoped restoration, staging removal, VM-off and claim-release
  verify. These are manual replacement checks, not automatic-update acceptance.
- [x] Qualify current signed MSI native manual-update guidance and tray
  Quit/reopen. Cabinet binaries/notices match actual installation; independent
  100-registry/nine-file restoration and off/release cleanup pass. Installer
  wizard, legacy MSI migration and full associations remain separate.
- [x] Repeat current signed Windows NSIS populated migration and tray Quit/reopen.
  Eight assertions/eight refusal routes and nine actual captures pass;50-registry/
  seven-file restoration, owned firewall/staging cleanup and off/release verify.
- [x] Qualify the unchanged live HTTPS server’s private A7 selection and clean
  disable; ordinary routes stay unchanged. Released Windows0.2.1 automatically
  authenticates/installs exact A7, then native Quit/restart retains four fixture
  records. Ten assertions/eight refusals and exact restoration/off/release pass.
- [x] Qualify current Mac ARM and Linux x64 automatic HTTPS updates and native
  Quit/reopen. Each passes ten assertions and scoped restoration/off/release;
  private trials are removed. Ordinary GUI and unrun architectures stay separate.
- [x] Correct website initial fragments,320px layout and conditional successor
  No Play Store guidance. Fresh builds/guards/responsive captures pass locally;
  real metadata stays disabled and the public website is unchanged.
- [x] Qualify Windows wrong-signature refusal and valid retry against exact A7.
  Eleven assertions, native Quit/reopen, preservation and exact restoration pass.
  Earlier runner failures stay recorded; interrupted/ordinary GUI repeat is separate.
- [x] Qualify Windows interrupted download and ordinary GUI valid retry against
  exact A7. Eleven assertions/eight refusal routes, four-record native Quit/reopen,
  privacy persistence and exact restoration/off/release/trial removal pass.
  Original failure and guarded recovery remain separately recorded.
- [x] Qualify exact A7 Mac ARM ordinary GUI Install & Restart, native Cmd-Q/
  reopen, ten assertions/nine refusal routes and independent privacy persistence.
  Exact scoped restoration, capture/server cleanup, VM-off/release and private
  trial removal pass. Intel and DMG drag/drop remain separate.
- [x] Qualify exact a7 Intel-app ordinary GUI updating under existing Rosetta:
  ten assertions/nine refusals, four records, native Quit/reopen, independent
  privacy persistence and exact restoration/off/release/trial removal pass.
  Physical Intel hardware and DMG installation remain separate.
- [x] Save/reload four conservative local-data categories in the unsubmitted
  store draft: resettable identifier, authentication, activity and content.
  Five other categories and existing three certifications remain unchanged.
  Final owner certification and accurate deployed policy are still open.
- [x] Save and reload-verify accurate draft Web Store single-purpose/permission
  explanations and description; public version and certifications,
  policy URL and graphic assets remain unchanged. Submission stays separate.
- [ ] Complete remaining selected native installed/updater/launcher cohorts;
  Exact current isolated updater trial and15-file checksum support are prepared;
  the maintainer has directed execution of the next slices, including the
  prepared isolated updater trial and its cleanup. Linux extracted-native launch
  probe remains partial/failed; its final scoped cleanup and VM-off verify.
  review native security, notices and corresponding-source/relink delivery.
  Both actual AppImages meet the reviewed Noble WebKit/OpenSSL build floors;
  this does not close complete redistribution or installed acceptance.
- [x] Upload the exact approved extension1.1.2 ZIP to the existing draft item.
  Actual package tables confirm draft1.1.2 and unchanged published1.1.1.
  Submission, certifications, publication and installed-store updates stay separate.
- [ ] Complete final managed/installed updates, mixed versions, Shutdown/reopen,
  selected physical recovery and supported desktop cohorts below. Existing
  inherited privacy choices and device policies require explicit disposition.
- [ ] Review selected platform gaps, soak/stop criteria, recovery owner and the
  exact shipment. Production store/feed/release/website publication is separate.

Machine Control is the preferred machine-testbed interface. Use repository
runners first, its common CLI/platform guides for native evidence, and shut down
VMs and release claims between uses. Close all physical Chromebook tabs and
application windows after every slice, then verify the empty inventory. Keep a
separate report per slice; screenshots and detailed notes remain ignored. None of these local tasks closes a broader delivery row by itself.

Owner: [product-surfaces-and-migration](topics/product-surfaces-and-migration.md).
Campaign: [231](tactical/231-jstorrent-migration-working-campaign.md).
Candidate mechanics: [250](tactical/250-jstorrent-production-identity-candidates.md).

For each installed cohort record exact source/artifact hashes, old/new versions,
public signing identity, platform/build, observation and recovery. Disposable
signatures, injected states and unsigned installations are explicitly scoped;
they cannot satisfy production delivery. Keep private values out of public docs.

## Candidate Identity And Delivery

- [ ] **D-01 Desktop identity:** JSTorrent name/icons, `com.jstorrent.desktop`,
  existing Tauri updater trust root and `updates.jstorrent.com` route. Candidate
  version exceeds every selected installed source. Beta route/key remain separate.
  Frozen a7 signed five-target0.3.0 metadata, actual Mac/Windows package names,
  both packaged Mac icons,2,040 display values/40 original assets, retained
  ID/key/route and selected legacy0.2.1 ordering qualify. Native installed,
  updater/launcher/pixel cohorts remain separate P gates.
- [ ] **D-02 Desktop signatures:** retained updater key verifies final signatures;
  wrong key fails. macOS Developer ID/team, notarization/stapling/Gatekeeper and
  Windows publisher/signature match the accepted production delivery lane.
  Exact a7 qualifies six retained-root signatures/six wrong-root refusals,
  both Mac strict/deep signatures, Accepted notary CDHash bindings, DMG staples
  and ARM-host Gatekeeper assessments. Native Windows x64 verifies exact
  NSIS/MSI hashes, accepted-publisher Authenticode and Microsoft timestamps.
  Five independently extracted formats reconcile binaries/notices; actual MSI
  read-only metadata qualifies identity. Failed inspection-script assumptions
  stay recorded; corrected checks reconcile the retained native observations.
  Native source/redistribution review and installed acceptance remain R-05/P.
  Reopened for tactical290’s new Windows packaging inputs; old a7 proofs stand.
- [x] **D-03 Android identity:** `com.jstorrent.app`; versionCode exceeds all Play
  tracks, including closed/internal/testing, and selected GitHub APKs. Record
  upload certificate separately from the existing Play app-signing certificate.
  Play-generated APK updates an installed Play build without uninstall/data clear.
  Code27 exceeds the fresh pre-upload maximum26 and selected GitHub code24.
  Exact original-upload-signed AAB, Play-generated APK and actual installed
  base APK independently verify their separate certificates. The retained
  Play26-to27 installation preserves UID, first-install time and installer.
  Library, privacy/lifecycle and broader physical acceptance remain A gates.
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
  and macOS arm64, plus the ordinary Windows/macOS UI. Mac x64 now also passes under existing Rosetta; physical Intel hardware,
  Linux ARM and the Linux old-app UI remain open; the complete row is not satisfied.
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

1. Finish preserved-install acceptance for Android27, now available to existing
   internal Play testers. Production remains23; availability alone is not an
   installed-library or lifecycle pass.
2. Qualify companion/browser-local writers and the actual production extension
   ID updated in place, then repeat mixed-store pairings on both physical cohorts.
3. Complete the two-tree/reboot/grant-loss and sleep/network-loss matrix; include
   retained bytes, actual upload serving and independently verified repair.
4. Requalify installed delivery against the fresh signed desktop bytes where
   source changes matter. Extend macOS x64 and representative current Linux
   systems under the accepted best-effort scope; previous signed-candidate
   evidence remains pinned to its own hashes.
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
