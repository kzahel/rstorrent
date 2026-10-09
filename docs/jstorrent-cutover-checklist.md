# JSTorrent In-Place Cutover Checklist

Tactical [283](tactical/283-companion-update-recovery-guidance.md) adds
component-specific recovery links and plain connection guidance in the
packaged extension, preserving attach-only retries and existing discovery/
pairing authority. The local source/render checkpoint remains separate from
actual store delivery and installed mixed-version acceptance under259. The
new production payload supersedes the prior source13dde build and source03e
extension upload proposals for final qualification; historical evidence stays
bound to its own bytes. No new build/store publication occurs.

## Remaining release work, in brief

Use the short [update scenario checklist](jstorrent-update-scenarios.md) for
store timing, mixed versions and user-facing recovery messages.

Linux best-effort scope is accepted on 2026-10-09; no older-distro WebKit
backport project is required. AppImage is the primary Linux desktop offering;
Tactical285 locally retires production DEB/RPM and moves the Linux builds to
Ubuntu24.04 with a reviewed WebKit floor. Fresh signed inventories, maintained
bundled libraries and actual minimum-system checks remain to qualify. The native CLI/headless route remains a
fallback, with public availability and instructions to be verified. See the
[accepted product scope](topics/product-direction.md#linux-release-scope-accepted-2026-10-09).

- Build and qualify fresh signed desktop candidates with the latest fixes.
- Deliver Android code 27 and extension 1.1.2 to the authorized test/draft
  channels, then verify real preserved-install and mixed-version updates.
- Finish physical reboot, sleep/network and storage recovery checks, plus
  final-candidate migration, updater and failure/recovery checks on selected
  supported platforms. iOS is outside this cutover.
- Finish native license/source delivery review and agree the soak window,
  rollout/stop criteria and recovery responsibility.
- Review exact artifacts and publish the website, store listings and update
  routes only under explicit publication instructions.

Earlier passes remain evidence for their exact bytes. The detailed historical
matrix below is not a promise to qualify every older Linux distribution.

The frozen production-code build proposal is `13dde7a1`; shared web/extension
validation below was executed at source `03e63443`. New AppRun notice inputs
are locally qualified; revised hosted/signed qualification remains pending.

Runtime redistribution review has advanced under259 without product changes.
Seven source archives and ten original license files are inspected; pinned
libfuse/squashfuse archive hashes and all four exact Alpine recipe/archive
checksums plus16 auxiliary input hashes verify. Twelve source patches apply
sequentially in owned temporary extractions, which are removed.

A native x64 owned Alpine chroot builds runtime8f39b89 with patched libfuse3.15.0
and squashfuse0.5.2, using verified fixed zlib1.3.2-r1 packages. A separate
application-object/static-library relink runs version/help successfully.
Independent19-entry export hashes, seven libraries, ELF architecture and pinned
package/version facts verify. Both preliminary shell-path failures remain
failed; all three attempts remove their owned files/mounts, verify VM off and
release claims. No product profile, installed candidate or visible UI is used.
This prototype is not the original signed runtime, ARM validation or a delivered
source/relink offer. Complete native attribution/redistribution remains open;
the upstream runtime notice list also omits its statically linked mimalloc.

Original8c runtimes use Alpine zlib1.3.2-r0, while current1.3.2-r1 backports
[CVE-2026-85091](https://github.com/madler/zlib/commit/df84af25dc1942490e1d1c899a07619152a46148)
in nonblocking gzip writes. Both exact upstream debug assets verify release
SHA-256, original signed runtime GNU debuglink CRC and build-id bytes. Their
2,056/1,881 defined function tables contain no gzip-write functions or gzwrite.c;
109 C/header files across runtime/libfuse/squashfuse also have no such calls.
This supports a scoped absence inference, not complete security/exploitability
clearance. Maintained WebKit/accepted ABI, OpenSSL rebuild and R-05 remain open.

Tactical [282](tactical/282-apprun-license-attribution.md) locally fixes
missing AppRun MIT copyright attribution in the exact signed8c AppImage notice
inspection. Both reviewed mirror launchers independently match upstream bytes;
the collector packages the original Simon Peter/RazZziel notice with exact
hash/source revision/origin/architecture checks. All26 distribution cases and
both actual launcher controlled-notice round trips pass. No implementation code
is imported. Fresh signed inputs change again after03e; earlier pending build
proposals are superseded. The upstream legacy release is marked obsolete and
archived build metadata is unavailable; reproducible build, all native source/
relink/security obligations and maintained WebKit/accepted ABI remain open.

The following shared-web/extension checks used source03e63443. The full default bundled
Chromium suite now passes62 cases with14 existing live/opt-in skips; focused
managed-package captures pass separately. Current branding2037 display values/
40 original assets and all maintained localization catalogs pass. Fresh exact-
lock Rust/npm review passes the unchanged seven-warning/backport policy and
2026-10-12 expiry; native WebKit/OpenSSL/source/relink holds remain separate.
The rebuilt production extension1.1.2 is400105 bytes/SHA-256
`38be77fd4237f8f09218dc69a1618b0cfdc8aa6143bbfe36346809edc2c93bfb`,
retains the existing item ID, and passes54 extension cases, exact19-entry ZIP/
13-bundle CSP validation and17 injected packaged onboarding/cache scenarios.
No device/store acceptance is inferred from injected cases. Authenticated
read-only store refresh remains Web Store published/draft1.1.1 with three
legacy screenshots/privacy text, and Play internal26/production23. No new
upload/draft save/certification/submission occurs. Frozen13dde push/nonpublishing
signed-build review supersedes pending681/b0b/932/03e proposals; explicit action
instruction remains pending. D-01/D-02 reopen for changed final package inputs;
immutable source8c signature/identity evidence remains valid for its own bytes.

Tactical [281](tactical/281-managed-package-update-status.md) locally
repairs the actual MSI About-screen automatic-update contradiction. MSI/DEB/RPM
and unknown packages start and stay in existing manual package guidance, own
no check timers, and show no in-app check/install or automatic-check privacy
copy. Backend policy, trust/routes and preference behavior remain unchanged.
Full web typecheck/478 tests (two existing skips),20 focused bundled-Chromium
render/accessibility cases and16 inspected managed/theme/width captures pass.
Fresh exact-source signed native MSI evidence remains open under259; renderer
fixtures are labeled separately from unchanged source8c before-fix captures.

The exact signed source8c Windows x64 MSI now passes quiet per-machine
fresh installation, both installed native publisher/timestamp checks,
five-entry distribution/notice inspection, native empty-library launch and
Quit/restart with a synthetic loopback/mapping-off/DHT-off/PEX-off fixture.
All four new native screenshots are visually inspected. Its owned
version1/statistics-off fixture and clean-shutdown marker persist. Normal
uninstall and independent100 HKCU/HKLM scopes/nine file-scope comparisons
pass; no owned process, firewall rule, candidate/related MSI or uninstall
key remains. All26 staging files are hash-bound and removed; VM off and
exact claim release pass. Installer wizard, legacy MSI migration and full
OS association dispatch remain unqualified. MSI uses its advertised
`JSTorrent.torrent` class and quoted short magnet path, recorded separately
from the private NSIS class; no NSIS-validator pass is inferred.

The real MSI About screen initially claims automatic updates are enabled.
Manual Check correctly changes to package-channel guidance, but automatic
schedule/privacy copy and the check button remain. Tactical281 repairs this
presentation/controller initialization issue; the native backend already
refuses managed-package checks and replacement. Source8c screenshots are
unchanged before-fix evidence. The empty detail panel's implementation-oriented
copy is an additional design note, rather than an unstyled page.

Tactical [280](tactical/280-desktop-executable-display-brand.md) locally
corrects the desktop output to `jstorrent-client`, preventing the observed
advanced-firewall filename from retaining the old display brand. The distinct
name preserves released old-writer process detection. All41 Node/23 native
distribution/85 Rust cases, clippy/fmt/workflow lint and the actual unsigned
Mac bundle/inventory pass. Identifiers, host, profiles, networking and updater
trust/routes remain intact. Fresh signed Windows system-label, migration and
updater evidence remains required under259; source8c receipts are unchanged.

The final signed source8c Windows x64 NSIS candidate now passes actual
per-user legacy0.2.1-to-successor0.3.0 installation: eight assertions, eight
registered refusal routes, four imported records, valid100%/corrupt0% recheck,
retained unavailable root, stable native Quit/restart and source/payload
preservation. All eleven new native captures are visually inspected. The
owned disclosure saves statistics off and remains version1/off after restart.
Independent checks compare48 HKCU scopes and seven restored file scopes;
exactly four newly created program-bound firewall Block rules are removed,
unrelated OS rules are preserved, no owned process remains, and35 hash-bound
staging files are removed. The VM is verified off and its claim released.
The separate MSI installation checkpoint above now passes its bounded scope.
Windows automatic/GUI updating and full launch/association cohorts remain open.
The advanced firewall inventory exposes `rstorrent-desktop.exe`; Tactical280
corrects the output name locally, with fresh signed system-label evidence still
required. This finding is separate from the branded product screenshots. The raw native-host
registration error also confirms the existing actionable-copy design gap.

The authorized native Windows x64 appliance rebuild is handed off with verified
AMD64 Windows11, real Python3.13 sqlite3/winreg, native semantic/capture/input
readiness and a real cold locked-to-unlocked stored-password login. Credential
bytes stay in a private0600 file/0700 parent; the registry stores its locator.
Native login delivery is confirmed while its effect field says no_effect;
independent unlocked state is recorded separately. Seed/staging cleanup and
restored inventory pass; VM off/claim available at handoff. One timed-out
factory-status journal intent lacks a result, so ready-close remains incomplete
without history alteration. Parent signed product qualification now resumes;
no product acceptance row is closed by rebuilding a machine.

Both exact signed source8c AppImages additionally bundle WebKitGTK
`2.50.4-0ubuntu0.22.04.1`. [Upstream WSA-2026-0003](https://webkitgtk.org/security/WSA-2026-0003.html)
lists affected versions before2.52.4, including CSP enforcement. Ubuntu's
[CVE-2026-43742 status](https://ubuntu.com/security/CVE-2026-43742) marks Jammy
Ignored and says current WebKit cannot be built on Jammy and earlier; Noble
has a fixed2.52.6 package. R-05 holds Linux shipment pending maintained runtime
and compatibility disposition; no product exploitability is asserted. T279's
OpenSSL floor does not resolve this separate WebKit gap or authorize a silent
minimum-OS change. Both bundled GLib versions meet the specific USN-8794-1
floor; this selected review is not a full native vulnerability inventory.

Tactical [279](tactical/279-appimage-openssl-security-floor.md) records a
new native security shipment blocker: the exact signed source8c ARM AppImage
bundles OpenSSL `3.0.2-0ubuntu1.29`, below Ubuntu USN-8847-1's Jammy fixed
revision `3.0.2-0ubuntu1.30`. The x64 candidate contains the fixed revision.
A local packaging/extraction guard now refuses the ARM manifest, and Linux
builders explicitly refresh OpenSSL development/runtime packages. All 21
focused distribution cases and workflow lint pass locally. Rebuilding and
qualifying new signed bytes remain required; this is one advisory floor, not
complete native security clearance, and product exploitability is unestablished.
Original trust/integrity and functional receipts remain scoped to their bytes.


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

## Current Executable Finish-Line Progress

These local tasks and delivery dependencies are a quick execution view. The 25
full acceptance rows below retain their exact signed/store/cohort requirements;
a checked local task does not close a delivery row.

- [x] Branding guard: 2,037 display values and 40 original JSTorrent assets pass;
  actual native and responsive website before/after evidence is retained locally.
- [x] Compatible dependency/security corrections, web typecheck/470 tests,
  affected 46 E2E/54 extension cases and workspace 1,575 tests/fmt/clippy pass.
- [x] Delivered Android source `aeaa72f9`, original-key code-26 APK/AAB, 122 JVM tests,
  lint and independent package/labels/API/ABI/alignment/notices checks pass.
- [x] Bounded native incoming observer 269 diagnoses actual pre-admission probe
  ordering without changing engine, listener, background or retry policy.
- [x] Both original large physical fixtures independently upload/hash all 28 MiB,
  retain diagnostic filters, join/reopen the actual Live library and preserve SAF
  registry/payload bytes. Owned cleanup passes; targeted runs do not prove an hour.
- [x] Manual-only production publisher 271 passes 58 focused cases/workflow lint
  and the historical actual 23-asset/15-selection/10-signature local rehearsal.
- [x] Native Linux toolkit name correction is verified on x64. Historical signed
  ARM first-use and extracted Debian populated/restarted pixels render; eight
  migration assertions/six refusal routes and restored-state cleanup pass.
  Final source8c signed extracted Debian also passes eight assertions/six refusal
  routes with readable populated/restarted/error pixels. Actual package-manager,
  AppImage and updater qualification remain separate.
- [x] Concrete shipment review independently rehashes the three Android artifacts,
  exact production extension ZIP and 23 historical desktop assets. Final signed
  source, installed cohorts, store delivery and owner approval remain open.
- [x] First physical cohort: current-source three cold repetitions, detached hour,
  full 40-MiB independent download/upload hashes, retained diagnostic filters,
  joined actual Live reopen, unchanged SAF state and cleanup all pass. All seven
  current-APK storage/recovery cases also pass with independent hashes/cleanup.
- [x] Both physical cohorts: all seven current-APK storage/recovery cases pass,
  including actual grants/picker/relocation, independent hashes and cleanup.
- [x] Proportional upload 272 passes boundary/refusal/forwarding tests and actual
  B full 40-MiB independent upload within the 172-second bound.
- [x] Second physical cohort: fresh native-wake default repeat passes all three
  cold repetitions, the detached hour, full 40-MiB independent download/upload,
  retained filters, joined actual Live/owned-row reopen, SAF continuity and
  cleanup. The first whole-run reopen failure remains recorded unchanged.
- [x] Every completed Linux, Windows and macOS session verifies VM off and claim
  release; only prepared active checks use a running VM. Task evidence stays
  ignored and hash-bound exported guest screenshot copies are removed.
- [x] Approved exact source8c push to existing main and nonpublishing desktop
  dispatch37735081504 record the same SHA and publish=false. Fresh strict
  three-lockfile review passes; the existing expiry remains unchanged.
- [x] Exact source8c desktop run37735081504 completes all five original-signed
  targets and collector;23 hashes/sizes,15 selectors and10 original-root
  signatures and ten wrong-incubation-root refusals independently pass. Both Mac
  signatures/notarization/DMG stapling and local ARM Gatekeeper assessment pass.
  Installed acceptance remains separate.
- [x] Final signed Mac ARM manual replacement: eight migration assertions/nine
  refusal routes, readable settled/restarted JSTorrent library, source/payload
  preservation, independent inherited-tree restoration and exact owned cleanup;
  transfer server reaped, VM off and claim released. Separate actual final DMG
  read-only mount/signature/bundle installation passes the same eight/nine
  checks and native restart, with its mount detached. Automatic updating and
  GUI drag/drop qualification remain separate.
- [x] Final ARM Debian actual package-manager installation and installed binary:
  eight migration assertions/six refusal routes, readable settled/restart pixels,
  retained source/payload and owned disclosure choice. Owned package/files are
  purged, inherited directory/cache bytes restored, recent metadata removed and
  VM off/claim released. Launcher associations and automatic updating stay open.
- [x] Final ARM AppImage extract-and-run functional migration: eight assertions,
  six refusal routes, native owned disclosure acknowledgement, preserved bytes
  and normal scopes, exact owned cleanup, VM off and claim released. Actual
  foreground pixels remain unavailable through native captures/view changes;
  outer UI is prohibited by the testbed. FUSE/associations/updater stay open.
- [x] Final signed Linux x64 AppImage native migration: eight assertions/six
  refusal routes, readable settled/restarted JSTorrent, independently persisted
  owned disclosure choice and preserved source/payload. Six exported guest
  captures and exact guest/controller roots removed; VM off and claim released.
  Old-app blank pixels, raw recovery copy and unowned system crash dialog remain
  recorded; FUSE/normal association/updater acceptance stays separate.
- [x] Final signed Linux x64 Debian actual installation/installed binary:
  eight migration assertions/six refusal routes, readable native restart,
  preserved bytes and owned disclosure persistence. Exact package/files purged,
  inherited cache/directory/profile state restored, guest/controller staging and
  six native captures removed; VM off/claim released. Association/updater gates
  remain separate; only exact-owned crash metadata is checked.
- [x] Final signed Linux ARM controlled native launch: twelve post-Quit Retry
  responses, busy-owner retry/timeout, native/background/magnet/file and both
  owned OS associations, spaces/Unicode, twelve-way one-owner concurrency,
  warm activation and 35-second no-resurrection. Exact owned cleanup/off/release
  pass; inherited pins/browser/store/content-intake scope stays separate.
- [x] Final signed Linux x64 controlled native launch: twelve scoped driver
  cases, post-Quit Retry, bounded busy-owner retry/refusal, four launch intents,
  queried owned OS associations with spaces/Unicode, twelve-way one-owner/warm
  activation and 35-second no-resurrection; exact owned cleanup/off/release.
  Inherited pins/content/browser/store requirements remain separate.
- [x] 736 selected production input Git objects/modes match source8c-to681;
  Android Rust runtime prefix is identical and its readiness fix is test-only.
  Signed8c and local Android27 identities remain distinct; hosted CI unchanged.
- [x] Both final AppImage outer runtimes match pinned upstream8f39b89 bytes
  except their named16-byte payload digest; ELF/SquashFS boundaries and upstream
  license/build/dependency inputs are inspected read-only. Complete static
  dependency/relink/source and per-package redistribution review stays under R-05.
- [x] Final signed package identity/signing/notice integrity qualifies D-01/D-02;
  Windows native accepted publisher/timestamps and both Mac Gatekeeper policy
  assessments pass. Fresh exact-lock strict dependency audit passes unchanged.
- [x] Source8c hosted CI exposes an Android listener-test readiness race;277
  fixes the test only. Revised local source passes fmt/clippy and1,575 Rust
  cases with18 existing ignores. Revised-source hosted CI remains unperformed.
- [x] Actual Play-managed25-to26 update preserves both installation identities;
  one populated cohort preserves two rows, complete/partial hashes, paused50%,
  complete100%, settings and durable folder grant through engine restart.
  The second cohort retains its unacknowledged disclosure unchanged.
- [x] Android27 fixes the actual managed26 stale Live after Shutdown. Original-key
  APK/AAB,122 release JVM cases/lint and independent package checks pass;
  isolated physical normal Shutdown/task/service removal and fresh Live/100%
  reopen preserve payload/SAF state. Exact owned cleanup passes.
- [ ] Explicit revised-source push/CI and code27 internal delivery instruction;
  these newer bytes are not covered by the exact8c/code26 delivery approval.
- [ ] Final signed installed desktop updates and recovery matrix; native Windows
  x64 appliance rebuild is delegated with verified stored-password cold login
  and private registry handoff required. Intel Mac availability remains open.
- [x] Approved exact sourceaeaa original-key AAB uploads and publishes code26
  to the existing internal track/testers. Play reports Available to internal
  testers; production23 and supported devices are unchanged from internal25.
- [ ] Preserved managed ordinary/companion upgrades to code26; store availability
  alone is not installed acceptance. Production promotion is a separate decision.
- [ ] Approved same-item Web Store upload/submission/certifications and actual
  preserved-profile update/mixed-version acceptance. Native inspection
  completes read-only; published/draft1.1.1 and obsolete
  permission/privacy explanations are recorded without modifying the item.
- [ ] Remaining real physical provider/reboot/network/sleep and Linux integration
  outcomes using supported Machine Control routes; no injected-state substitute.
  Reboot qualification needs a canonical profile-unlock credential/handoff;
  neither physical target currently has one configured. Preserve the appliance's
  always-awake baseline unless a sleep-policy change is explicitly requested.
- [ ] Owner-reviewed final shipment capsule, independent rollout/halt/rollback
  policy and explicit delivery instructions; tags/releases/feeds remain intact.
- [ ] Actual qualified public inventory, enabled website descriptor and explicitly
  approved production website deployment/public verification.
- [x] Isolated physical cleanup: both owned staging/markers and payload/UI XML
  are absent; inherited production metadata was preserved before approved Play
  delivery. After managed25-to26, remove only owned fixture rows through normal
  Keep data actions, then independently hash/remove their four owned files.
  Managed26 UID/install/update/version/installer and inherited folder grant stay
  unchanged. The isolated27 test profile and its owned folder are cleared.
  Reports stay ignored.

## Native Windows x64 NSIS Checkpoint, 2026-10-08

- [x] Final signed Windows x64 NSIS populated legacy replacement and eight
  migration assertions/eight refusal routes; actual native branded pixels,
  saved owned statistics-off choice and stable Quit/restart.
- [x] Independent48 registry/seven file-scope restoration, exact four owned
  firewall-rule removal, no owned process,35 hash-bound staging-file cleanup,
  VM off and exact claim release.
- [x] Exact source8c Windows x64 MSI quiet per-machine fresh installation,
  both installed publisher/timestamp checks, five-entry notice inventory,
  native empty-library Quit/restart and synthetic preference persistence.
  Normal uninstall restores100 registry/nine file scopes; no owned process
  or firewall rule remains. All26 staging files removed, VM off/claim released.
- [ ] Windows MSI wizard/legacy migration, ordinary/automatic updater and full
  launch/store/cohort matrix; new executable/system-label and managed update
  status fixes require a fresh specifically approved signed candidate.

## Approved Delivery Checkpoint, 2026-10-08

The maintainer subsequently approves ONLY frozen source8c push/nonpublishing
build and exact sourceaeaa code26 existing internal-track/tester delivery.
Those actions execute: desktop run37735081504 records exact8c/publish=false;
Play reports internal26 available and production23 unchanged. The original-key
AAB independently rehashes to19f9c358 before upload. Existing testers and
supported devices are unchanged from internal25. English notes are condensed
to447 characters for the store limit. Its sole missing-deobfuscation warning
is recorded; exact release minification is disabled. Store availability does
not qualify managed installations, original-signed desktop collection, public
release/feed, production website or Web Store publication.

Fresh strict dependency review passes for the unchanged three source8c
lockfiles: zero vulnerabilities, seven existing reviewed Cargo warnings and
source-verified GLib backport. The review expiry stays2026-10-12.
Native Web Store inspection resumes after the unrelated Save modal clears,
without agent cancellation/submission: published/draft1.1.1 and obsolete
permission/privacy explanations remain. No Web Store upload/edit occurs.

The approved preview deploy fails its direct-hop checker HTTP403 before
activation. Tactical276 qualifies only the checker Host correction against
the actual source8c gateway: wrong Host403, missing credentials401, correct
static/health/WebSocket pass. The corrected preview activation succeeds; the private listener and HTTPS
route both pass exact-source static/health/WebSocket checks. Final signed
candidate packaging completes all five lanes and collector, with independent
23 hashes/sizes,15 selectors,10 original-root signatures and10 wrong-root
refusals. Physical managed baselines precede actual Play25-to26 updates on both
retained installations; no production sideload, uninstall or data clear is used.
One populated cohort preserves complete/partial bytes, settings and grant; the
second retains its inherited disclosure. Managed fixture cleanup subsequently
removes only owned rows/files and preserves installation metadata and grant.
Local27 corrects the observed stale Shutdown task; its internal delivery and
revised-source hosted CI require a new exact-artifact instruction.

## Local Production Publisher Checkpoint, 2026-10-08

Tactical271 prepares an explicitly manual Stable-tag production publication
mode. Branch candidates and automatic stable tag pushes cannot publish.
Production keeps all23 core assets/ten signature sidecars, uses the JSTorrent
release title and validates the complete private draft with its additional
SHA256SUMS asset. Source tag drift refuses before draft/publication. Local
refusal/shell tests, workflow lint and historical original-signed capsule
rehearsal pass; final CI, installed cohorts and explicitly approved publication,
updater-route activation and public website delivery remain open. No tag, push,
release, feed or store change is performed.

## Latest Android And Physical Checkpoint, 2026-10-08

Android application source `aeaa72f97b816c7f7fbad71dfb7aee5ea46499ca` supersedes
source0aa, source2ad and source376 candidates. Tactical 269 adds a bounded
read-only incoming-peer observer for native rejection evidence; it does not
change engine, listener or seeding policy. Normal original-upload-signed
release, 122 JVM tests, lint and independent APK/AAB package/signature/labels/
API/ABI/alignment/notices checks pass. AAB SHA-256:
`19f9c3585ea2ac8d596bdd19bc4e1ab7ffcb5bf73e96b22b0c3c1f595baa9644`.
Both physical devices have the exact isolated observer APK. Tactical269
completes locally with actual current-port/rejection samples. Tactical270 also
completes locally: both original large fixtures independently upload/hash all
28 MiB after fresh native registry readiness and pass retained diagnostic
filters, joined actual Live reopen, unchanged SAF registry/payload and cleanup.
Both current-APK default three-repetition/hour runs now pass, including full
40-MiB download/upload hashes, retained filters, joined actual Live/owned-row
reopen, SAF continuity and cleanup. B uses the bounded native display wake;
its first whole-run failure remains unchanged. All seven current-APK recovery
cases pass both devices. Code26 subsequently reaches the approved internal
track; managed installed acceptance remains separate and open.

Both source0aa physical small-fixture runs independently receive/hash all
524289 bytes in foreground, background and reopened foreground. Both joined
shutdowns, actual Live-library returns, unchanged SAF registries/payloads and
owned cleanups pass. Tactical 260's bounded adapter shutdown fix and Tactical
266's retained native diagnostic recovery are complete locally. These passes
exclude the large fixture's 120 empty files and do not qualify Play or an hour.
The large two-positive-file 15-second diagnostic receives about4.3–4.7 MiB
and one peer in each state; it remains incomplete, with successful joined
restart and cleanup. Measured movement justifies a120-second full-hash retry.

Both source376 physical detached hour components complete3,600 seconds and
independently hash all40 MiB. Their subsequent Android upload stages fail;
overall receipts remain failed. Large-metadata source0aa trials likewise
restore retained diagnostic history/actual Live but receive zero peers/bytes.
EOF is not proof of missing registration; an actual native peer record reports
completed-file registration accepted. Tactical267 retains these failures and
requires actual settled diagnostic rows, fresh UI captures and byte-checked
upload/reopen. Whole final physical qualification remains open. All VMs remain
stopped while these physical trials proceed.

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
render. Fresh isolated ARM first-use captures now render the same signed
package, while earlier black migrated captures and the old x64 blank window
retain their exact-run visual gaps; populated/final signed ARM acceptance is
still open.
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
call-path limits remain explicit. Sibling Tactical 264 completes locally at `56dc2299`: frozen current graph,
112 client cases/12 hosted route cases and 18 responsive captures pass; actual
logo/phone-header defects are fixed. Tactical 265 prepares the production
website handoff at `0e935e75`, with 22 inventory guards and actual/controlled
phone/wide checks. It remains disabled/null. Official JSTorrent publication requires the separately prepared manual271
mode, final CI/installed qualification and explicit instruction; public
website delivery remains an explicit technical/release gate. Managed Play and
broader physical delivery gates remain open.
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
  Exact source8c signed five-target0.3.0 metadata, both packaged Mac icons,
  2,037 display values/40 original assets, retained ID/key/route and selected
  legacy0.2.1 ordering qualify. Native/updater/launcher/pixel cohorts remain
  separate installed gates. This source8c pass remains historical;279/280/281
  change final package inputs, so final-candidate identity must be repeated.
- [ ] **D-02 Desktop signatures:** retained updater key verifies final signatures;
  wrong key fails. macOS Developer ID/team, notarization/stapling/Gatekeeper and
  Windows publisher/signature match the accepted production delivery lane.
  Exact final source8c qualifies ten retained-root signatures/ten wrong-root
  refusals, both Mac signatures/notary/staples and local ARM-host Gatekeeper
  assessments. Native Windows ARM verifies NSIS/MSI hashes, valid accepted-
  publisher Authenticode and Microsoft timestamps without install/login. Nine
  independently extracted signed formats reconcile binaries and notice integrity;
  AppImage source/redistribution review and installed/native acceptance retain
  separate R-05/P requirements. Fresh signatures for the279/280/281 candidate
  are required; do not carry source8c whole-row closure onto changed bytes.
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

1. Deliver locally qualified Android code 27 through a separately authorized
   internal Play release; code 26 remains the last delivered internal version.
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
