# Tactical 259: JSTorrent Finish-Line Checks

Status: Active, 2026-10-07. Maintainer-directed end-to-end release qualification.

Topics: `product-surfaces-and-migration`, `beta-release-readiness`,
`android-jstorrent-replacement`, `client-surfaces`.

## Scope and stopping condition

Finish the release qualification for the replacement desktop, Android/ChromeOS,
production extension and website. iOS is explicitly out of scope. Execute the
checklist below, fix concrete failures within this slice, and record current
source/artifact evidence. The [cutover checklist](../jstorrent-cutover-checklist.md)
owns acceptance criteria; this document owns the ordered execution and restart
checkpoint. A historical pass remains historical until repeated with the final
artifact where the change requires it. Never mark an unavailable cohort passed.

Stop only with all applicable acceptance gates qualified and a concrete shipment
capsule, or an explicit unresolved external dependency with every independent
check completed and an executable next action. Production publication remains
subject to the repository's explicit push/publish/tag/release instruction; do
all local preparation and qualification before requesting a final decision.

## Invariants and ownership

Preserve production IDs, original updater/upload/app-signing roots, existing
store items, minimum Android API 28, both Android ABIs and staggered-update
compatibility. Do not uninstall or clear inherited production data, overwrite
private signing material, reset a VM, use public swarms, or change Machine
Control's always-awake ChromeOS appliance policy. Normal migration must fence
old writers and preserve downloaded bytes and unrelated files. New protocol or
runtime fixes require their focused source/oracle review before implementation.

Machine Control is already the preferred machine-testbed interface in
`AGENTS.md` and `DEVELOPMENT.md`. Use its common CLI and platform guides for
doctor, exact target identity, claims, guest execution, native UI, captures and
cleanup. Use deterministic tests and existing qualification runners first.
Claims and task markers own exclusive access, not desktop permission. Record
initial VM power and restore it; release claims/markers and reap owned services
in finally cleanup. Boot each VM only for its ready check and shut it down
between uses; waiting for builds or approvals does not justify an idle VM.
Physical production profiles remain intact.

Detailed reports, screenshots, machine logs and receipts are retained locally
under ignored `docs/evidence/259-finish-line/`. Public summaries must omit
private targets, endpoints, accounts, paths, library contents and credentials.
Record source SHA, public package identity, artifact hashes, exact checks and
limitations in this document. No new engine/service architecture is implied.

## Ordered finish-line checklist

Checkboxes mean the complete stated check passes. Partial evidence is recorded
below without checking the whole item.

### 1. Source and candidate freeze (D-01–05, R-01, R-05)

- [ ] Audit current source, existing workflows and signing trust; confirm store
  maxima and candidate versions across all published tracks.
- [ ] Run final source CI, branding/assets/catalog/CSP guards, extension tests,
  website build and affected Rust/web/Android release checks.
- [ ] Produce exact-source, original-key signed desktop five-target packages
  and Android APK/AAB; independently verify hashes, signatures, identities,
  notarization/publisher, notices, resolved labels and ABI/platform inventory.
- [ ] Prepare exact-source production extension ZIP, staged store artwork and
  website; inspect Android 9 minimum/unsupported-device guidance and accepted
  feature dispositions without claiming unshipped parity.

### 2. Installed desktop and preview transitions (P-01–07, R-02–03)

- [ ] Windows x64, macOS arm64/x64 and Linux x64/arm64 normal installation,
  launch, file/magnet/toolbar/tray/single-owner and restart evidence.
- [ ] Actual old-to-final signed automatic and GUI update on available owned
  cohorts; original-root wrong-signature refusal and interrupted-download retry.
- [ ] Union of old profiles, paused/partial/complete/magnet/Normal/Skip records,
  multiple roots/settings, duplicate/corrupt/unavailable inputs and old-writer
  fencing; byte-checked recheck/repair and stable restart completion marker.
- [ ] Interrupted migration/install, locked store, permission denial and disk
  exhaustion preserve state; safe retry and rollback with all writers stopped.
- [ ] Qualify the renamed Linux preview package/resource transition separately
  before any preview publication; production retains its existing package name.

### 3. Production extension continuity (D-04, P-06–07, A-03)

- [ ] Inspect the existing Web Store item/version/permissions and actual
  existing-profile update; an unpacked installation is not store evidence.
- [ ] Exercise all old/new extension–desktop and extension–Android pairs,
  historical launch routes, cold/warm/file/magnet/toolbar intake, browser/view
  restart, pairing credential replacement and repeated repair.
- [ ] Confirm obsolete writers cannot modify the migrated library and both
  current views display the same backend state; support copy is truthful.

### 4. Android and both physical ChromeOS cohorts (D-03, A-01–08)

- [ ] Final Play-generated update on the existing package: managed signer,
  version progression, UID/install-time and standalone/companion library,
  settings, bytes and launcher preserved without uninstall/data clear.
- [ ] Two SAF trees, read/write persistence, picker cancellation, revoked and
  restored grants, removable/provider failure, force-stop and reboot/restart.
- [ ] Real Android onboarding and mixed-store recovery on both physical
  devices; distinguish actual unavailable/failed store cases from injection.
- [ ] Signed Linux setup/launcher/folder-sharing, stopped VM and browser
  recovery where supported; explicit alternate/unsupported route where not.
- [ ] Background/unmetered operation, detached views, notification/power,
  independently observed completed-file upload, network loss and sleep/wake.
- [ ] Three cold-launch/recovery repetitions and one 60-minute controlled
  observation per supported backend/device, with independent payload hashes.

### 5. Shipment, operational recovery and review (D-05, R-01–05)

- [ ] Inspect final populated/native/error/setup/store screens and branding;
  retain screenshots and resolve material usability/recovery defects.
- [ ] Bind exact final source, CI, artifacts, certificates, versions and
  qualified/unqualified cohorts in one reviewable shipment capsule.
- [ ] Prepare staged independent desktop/Play/Web Store/website cutover order,
  update-stop controls, rollback procedure, observation thresholds and owner.
- [ ] Restore owned machine/test state, stop listeners/seeders, remove scratch
  files and release claims/markers; retain the ignored report and screenshots.
- [ ] Obtain any still-required explicit production-publication instruction
  against the concrete reviewed capsule, then perform only that approved scope.

## Completed bounded checks

These completed checks show progress within the broader acceptance rows above;
they do not close those rows' remaining delivery, platform or recovery cases.

- [x] Candidate `a35934d2`: every in-scope source CI job and website CI pass.
- [x] Source `a35934d2` Android 1.0.26/code 26: signed APK/AAB identity,
  upload certificate, minimum API, dual ABI, alignment and notices pass. This
  capsule is superseded for final Android delivery by the Tactical 260 fix.
- [x] Existing Web Store authentication/item inspection and candidate 1.1.2
  production ZIP, permissions and CSP checks pass; no store update performed.
- [x] Current five-target desktop capsule: all 23 asset hashes, 15 updater
  selections and ten original-root payload signatures independently pass.
- [x] Current signed Linux ARM manual migration: eight assertions, six old-host
  refusal routes and inherited-state restoration pass; pixel review remains open.
- [x] Current signed macOS ARM package: original-root signature, Developer ID,
  notarization/stapling/inventory and eight manual installed migration checks pass.
- [x] Physical cohort A: three cold/recovery repetitions, 60-minute foreground
  transfer with independently verified bytes and seven SAF/recovery cases pass.
- [x] Local hosted-page query security, disclosure copy, responsive update guide,
  website builds and browser captures pass; public deployment remains pending.
- [x] Owned VMs stopped, idle claims released and artifact-transfer listener
  stopped; macOS fixture state restored and its owned scratch removed.

## Execution checkpoint

2026-10-07: production candidate source is `a35934d2`. Every in-scope job in
source CI `37656649017` passes; the excluded iOS job failed while downloading
the Rust toolchain. Website source CI passes. Nonpublishing Android candidate
`37657799144` passes independent upload-certificate, package/launcher, minimum
API 28, both-ABI and 16-KiB bundle checks: version 1.0.26/code 26, AAB SHA-256
`446b46f1d853ff9dc46dba9dc0f95fbc87c1590f37f870a3f913edeebffc0411`.
Authenticated Play shows production code 23 and internal code 25. The earlier code-26 internal review is superseded by Tactical 260: rebuild
and reverify the final Android APK/AAB after its physical qualification. No
code-26 upload/release is authorized. Production extension item remains published/draft 1.1.1; candidate
1.1.2 ZIP passes packaging/CSP with SHA-256
`979a0f136ce0a219bca8428b1b5afef2c11a1b98ac1a9fc75e5b61abc67cc4aa`.

Desktop nonpublishing run `37657795184` attempt 1 timed out in Linux dependency
installation before signing. Attempt 2 passes every packaging leg and the original-root identity proof.
The collector is canceled during `apt` installation of minisign, after its
assembly/descriptor checks pass. Independent local assembly from all five
same-attempt lanes passes the exact 23-asset inventory/hashes, 15 updater
selections and all ten original-root payload signatures. Do not describe the
canceled workflow as a complete CI pass. The current macOS ARM archive independently passes original-root
updater signature, Developer ID, notarization, stapling and file inventory
checks. Its installed old-to-new fixture rehearsal passes all eight checks. The
older `f5860c99` signed capsule is historical.
Current unsigned production-identity macOS arm64 old-to-new migration rehearsal
passes profile union, paused intent, valid/corrupt payload recheck, missing-root
refusal, nine old-host fencing routes, stable restart marker and source-byte
preservation. The signed-candidate manual replacement rehearsal now confirms the same
eight invariants; automatic updating and the broader installed matrix remain
unqualified.
Local release tooling (53 tests) and migration failure/recovery tests (29) pass.

Both physical ChromeOS production Play installations remain intact. Cohort A
passes three isolated current-source cold-launch/recovery repetitions and the
full 3,600-second foreground observation with independent final payload hash.
Cohort B passes three repetitions, then fails its hour attempt at approximately
571 seconds with a Connecting/Checking download folders screen. A diagnostic
600-second rerun passes transfer and final byte verification; this is bounded
evidence, not an hour pass. Cohort A additionally passes seven SAF/source recovery checks. Attempt 9
times out in repetition 3; its logs show notification-ineligible shutdown.
After repairing notification setup following every isolated reset, attempt
10 passes three repetitions but stalls again in its short observation. The
completion did not verify within the 300-second recovery budget. Its raw
receipt and explicit failed-observation adjudication are retained; cleanup
passes. The separate crash buffer proves an uncaught typed native shutdown
error for an uncertain router lease. Tactical 260 contains the adapter failure
without changing reachability policy; all 121 JVM tests and both-ABI build pass.
A bounded physical run of its isolated APK is active. The original shutdown
trigger remains unresolved; notification setup repair does not explain it. No completed-file upload or Play delivery is inferred.

All three VMs started for this session are verified stopped and their exact
claims released after the maintainer's idle-power correction. Only the Mac
was subsequently booted for its ready signed rehearsal, then its state was
restored, owned fixtures removed, VM stopped and claim released again. Linux
ARM then passes all eight current signed manual-replacement assertions, including
four imported records, six refusal routes and unchanged source/payload bytes.
Its fixture restoration passes, owned scratch is removed, product writers stop,
and the VM is verified stopped with its exact claim released. Native accessible
controls are populated but captured application and GTK-alert surfaces are
black. Pixel review and the cause remain open; accessible DOM is not a visual pass. Boot only the
cohort whose artifact/check is ready; shut it down between uses. Windows UI
unlock and native Windows x64/macOS x64 cohorts remain external availability
gaps. No push, tag, publication, store upload, or hosted-context enablement has
occurred. Local hosted-page security/copy preparation is independently checked;
public deployment remains unqualified.

Next action: finish physical shutdown/recovery diagnostics on the fixed Android
APK, investigate Linux pixel capture/presentation, and test available native
cohorts one at a time. Rebuild final signed Android artifacts after qualification. Resume internal Play delivery only on explicit authorization; retain all
unqualified gates and the exact failures in the ignored finish-line report.

### Physical runner repair scope

The second physical cohort's DocumentsUI confirmation is visually clipped by
the shelf and exposes zero UIAutomator bounds. Maximizing a previous picker
does not survive creation of the next picker. The runner must recover that
observed surface through Machine Control's named native Maximize action, then
rediscover geometry before accepting the owned tree. Add an optional picker
recovery hook to the shared test helper, owned by the ChromeOS runner; never
tap zero bounds, guess coordinates, change provider settings or grant an
inherited tree. Verify the guard and fresh-bounds behavior deterministically
and repeat on the physical cohort. This is harness work, not a storage/engine
policy change. Retain the failed attempts and successful cleanup separately.

### Hosted-copy and diagnostics repair scope

Prepare a branded update guide on the development website without offering
unqualified artifacts: explain independent store updates, Android 9 minimum,
best-effort imports and folder repair, separate Android/Linux libraries, and
accepted legacy-feature exclusions. Keep availability explicit and link only
existing support/privacy destinations. Verify built desktop/phone layouts and
branding; do not turn the guide into a release announcement or deployment.

Current authenticated Web Store package tables both report 1.1.1; candidate
1.1.2 reduces required permissions to nativeMessaging/storage. Existing privacy
justifications describe the old browser engine and four removed permissions.
Prepare exact replacement copy before submission; no certification or publication
is implied. The live jstorrent.com privacy page and current sibling source retain
absolute no-server-data claims inconsistent with the separate disclosed updater.
The 208 sibling commit named in historical documentation is unavailable in this
checkout. Reconcile that checkpoint, correct the hosted source, and independently
verify bounded query allowlists and text-only rendering before deployment. Keep
all three hosted-context transmission gates false. No new analytics or recipient
is authorized. Preserve current form destinations, styles and public routes.

Cohort A completes its current-source isolated foreground 60-minute observation
with independently verified bytes. Cohort B completes three repetitions, then
stalls during observation with Connecting/Checking download folders visible.
Capture bounded logs scoped to the owned package before cleanup, retain actual
failed elapsed/progress status, and run a bounded diagnostic reproduction.
Do not claim store delivery, detached uploads or a passed hour from these checks.

The next diagnostic attempt times out in repetition 3. Before-cleanup owned
logs prove a joined `lifecycle_notification_ineligible` shutdown, with no
process crash. The runner cleared its own profile between repetitions without
restoring notification permission; correct that setup after each reset and
record it in the report. Retain this failure and add package-filtered activity
state to failure diagnostics. A bounded rerun distinguishes test setup from a
possible activity-visibility defect before product changes or another hour.

### Linux arm64 installed-rehearsal preparation

The available Linux VM is native arm64. Extend only the existing pinned
AppImage rehearsal's architecture selection and evidence, retaining package
and helper guards before any inherited-state mutation. No fixture, migration
policy or engine changes. The public legacy `tauri-app-v0.2.1` release is source
`73427b7d3aef2eaf1c4ac1409922fbb52dff751d`; independently matched arm64 asset
`JSTorrent_0.2.1_aarch64.AppImage` SHA-256 is
`5c74f3f7f0f375a0eb17b3e5f2da171a07c8fdc45948b3b46489e9cb87d446ae`.
Only its released host and daemon are extracted locally for oracle comparison;
no reference code/assets are imported into the repository. The JSTorrent MIT
reference provenance and generated-fixture policy remain unchanged.
Helper SHA-256 values are
`7bfd7ee516933119d830f678db38bd66095eec9c8d848021751422d14b165bc8` and
`25b424417aeadd1b7d6b85696988953cdb7804263e1cc668174a7b9dec18f33d`.
Reject unsupported architecture instead of substituting x64 pins; test that
guard and run the native guest only after the current signed package is ready.

The short observation completion rule now also fails explicitly when final
bytes do not verify; the previous short label did not qualify that outcome.
Attempt 10 is preserved without rewriting its raw receipt. Thirteen runner
safety tests and Python compilation pass after the correction.

### Default-off physical lifetime diagnosis

Attempt 11 uses the Tactical 260 APK. Three cold-launch/offline-restart byte
checks pass (128.9, 61.39 and 61.65 seconds). Its observation stops advancing
and fails at 555 seconds; cleanup passes. Before cleanup, package-scoped
activity state proves Android stops the activity. The lifecycle enters
visibility settling, then `stop_idle`, and joins native/client cleanup with
`cleanup_failed=false`. Read-only Android power state records a timeout sleep;
Machine Control's existing lid/idle suspend policy remains intact. No device
power settings are changed and no passing hour is inferred.

Qualify the actual supported background policy separately: an optional runner
mode navigates the owned app's real Settings/Power Management UI, verifies the
default-off switch, enables/persists it and detaches the Android view. Name the
lifetime in the receipt. Preserve the failed foreground run; background evidence
does not rewrite it or imply Play delivery. No product background-policy change
or synthetic lifecycle intent is introduced. Thirteen existing harness safety
cases and Python compilation pass. Run a bounded background check before the
full-hour observation.

### Current signed Linux x64 and Android background checkpoints

The same-attempt `a35934d2` signed Linux x64 AppImage passes eight manual
installed migration assertions and six refusal routes. SHA-256 is
`1f74e148c5360d5c76e0a4e03188ec1b18288afe8392aad537459886d1f2af5c`.
Actual successor pixels render the four controlled rows, verified completion,
corrupt-file recheck and missing-root attention state. Restart renders the same
library. The released 0.2.1 Linux window remains blank. Inherited guest state is
restored, the owned root removed, product writers joined, VM verified off and
claim released. Automatic HTTPS delivery and the final 261 rebuild remain open.

The Tactical 260 Android APK passes cohort B's three byte/offline-restart
repetitions and a detached 600-second background observation. The real default-
off setting is enabled through the UI and persisted; foreground service evidence
is captured. After releasing the controlled seed limit, the complete 28-MiB
payload matches SHA-1 `1b90d0a98b5c16a7ced9cb42c13f5c61757d6482`; cleanup passes.
This is a bounded background pass, separate from the failed foreground hour.
The full background hour is now running with new receipt paths.

Normal original-upload-key Android release at `184dcaba157c4dbd335778b7a70fe1cbd3393932`
passes both ABI builds, generated Kotlin, 121 release JVM tests, release lint and
independent certificate/launcher/API/16-KiB/notices checks. APK SHA-256 is
`e99bb6f24a43ec54ad63b07a0ccb75e9f2573d554b074fe526076c97bb232758`;
AAB SHA-256 is
`121b2124916bc016126df51ce88478d7d5e6fb33706a4469b315e44530b5a963`.
This supersedes the earlier pre-260 Android candidate. Authenticated Play latest
releases/bundles reconfirms production 23, internal 25 and maximum uploaded 25.
The concrete existing-internal-track review is prepared; upload/release requires
explicit authorization, requested while independent checks continue.

### Observed-folder confirmation repair

Cohort B's background-hour attempt 13 fails during the third SAF setup,
before the hour begins; two verified repetitions and successful cleanup are
retained. Actual final picker XML shows the owned folder entry at Downloads,
with the parent confirmation disabled. The shared helper marked entry on an
attempted tap and kept that boolean without observing the resulting breadcrumb.
Repair only this harness state: derive folder entry from each fresh observed
owned breadcrumb, retry only the owned visible entry within the existing budget,
and never accept the parent or disabled confirmation. Add a focused transition
regression, run the existing safety cases, then repeat the physical setup/hour.
No folder grants, engine, platform policy or inherited data change is introduced.

The owned-folder transition regression and all 14 safety cases pass; affected
Python modules compile. A new physical B background-hour attempt uses the fix
and fresh paths. Cohort A's final-source run has passed all three repetitions
and entered its background observation independently.

### Native Windows x64 credential boundary

The alternate native x64 controller resolves exact target identity and a
ready, mode-0600 stored credential. Cold boot reaches the supported protected
resident and lock screen. One supported stored-password submission has no
independently established login effect; the subsequent documented credential
verification rejects that stored password. No alternate password is guessed,
credential changed, VM reset or inherited profile modified. The signed installer
and harness are prepared, but no product test begins on this cohort. The VM is
cleanly stopped and its claim released while the canonical credential handoff
is repaired. This is a machine authentication gap, not a product failure or pass.

Fresh local final-source validation passes format, branding (2,013 display values
and 40 original assets), localization, desktop release configuration, extension
unit/manifest checks and production ZIP/CSP. Shared-web typecheck and 470 unit
tests pass (two existing skipped tests); the complete configured browser suite
passes 46 cases with 14 existing fixture/live-service skips. CI=1 selects the
bundled headless Chromium rather than the primary installed browser; the browser
and owned dev server are reaped. Website check/build pass with three static pages.
No historical live-service skip is reported as a current pass.

### Completed-file background upload and joined restart scope

Extend only the controlled physical qualification harness with an optional
completed-file upload check. Use actual Power Management and Incoming connections
UI controls on the isolated profile; enable/persist the already supported seeding
policy, observe the real listener port, detach the view and verify an independent
libtorrent leecher's complete payload hashes. Reuse the existing bootstrap upload
helper and ChromeOS SSH/ADB forwarding cleanup, adding an optional actual device
port rather than assuming 6881. No synthetic policy intent, product gateway,
public swarm, inherited installation, mapping lease or network policy change.

After upload, disable seeding through its real switch, observe joined native
shutdown and absent owned service, reopen with the controlled source paused,
and verify unchanged payload bytes and retained SAF registry. Existing time and
allocation budgets remain bounded; forwards, leecher state and owned app/fixture
cleanup remain in finally. Preserve diagnostics before cleanup and report this
flow separately from the earlier default-completion background hour.

The full local Rust baseline now passes `cargo clippy --workspace -- -D warnings`
and `cargo test --workspace`: 1,574 passing cases, zero failures and 18 explicitly
ignored cases across the recorded result lines. Eighteen release configuration/
input/nightly selection tests also pass. The optional upload harness compiles,
keeps all 14 safety tests passing, and rejects incompatible modes before any
machine operation. Its actual physical upload/restart evidence is pending while
the two background observations own those devices.


### Background intake checkpoint, 2026-10-08

Cohort B attempt 14 passes all three cold verified-byte/offline restart
repetitions (130.86, 79.23 and 127.59 seconds), then fails intake before its
background observation. Scoped logs show accepted metadata intake followed by
normal default-off visibility settling and joined idle shutdown; the hour is
unrun. Cleanup succeeds. The runner now selects/persists the real background
setting before observation intake, so an ARC view detach during metadata
acquisition can exercise that policy. It detaches again after confirmation.
Device power policy remains unchanged. Fourteen qualification safety cases and
Python compilation pass; attempt 15 is active. The listener observation also
accepts Tactical 262's human-readable address/port label, retaining the earlier
APK label for exact historical test artifacts. All Machine Control VMs are off.
