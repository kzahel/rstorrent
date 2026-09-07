# Tactical 210: Android Play Canary Setup

Status: **Release automation complete, verified 2026-09-07.** The separate
Play listing exists, and the maintainer
reports completing its remaining forms. A dedicated upload key is backed up
locally and configured in GitHub. Signed APK/AAB checks pass locally, in
the manual hosted rehearsal, and in the published `android-v0.1.0` release.
No Play upload or rollout was performed here.

Topic: `beta-release-readiness`

## Scope And Stopping Condition

Prepare an independent RSTorrent canary listing and an uploadable Android
release candidate. Reuse the maintainer's JSTorrent artwork and applicable
settings, while making the experimental product identity explicit. Stop when
the listing/declarations and release candidate are ready for the selected Play
track, or record concrete external blockers without claiming publication.

This does not replace `com.jstorrent.app`, migrate its state, publish an
extension, change engine architecture, or imply supported-beta graduation.
The subsequent maintainer-approved release-automation follow-up authorizes
implementation, CI setup, and the initial GitHub canary release. Play rollout
remains outside this follow-up.

## Dependencies, Invariants, And Validation

The owning topic is `docs/topics/beta-release-readiness.md`, especially
AND-002 through AND-005. Also read `android-jstorrent-replacement.md`,
`client-surfaces.md`, `product-surfaces-and-migration.md`, and the applicable
client topics before implementation. Tactical 208 and
`product-state-and-feedback.md` own current feedback/data disclosure behavior.

- Keep JSTorrent's existing listing and production release unchanged.
- Created listing name: **RSTorrent Canary**. Registered application ID:
  `com.jstorrent.rstorrent`. Release builds now use this ID; debug builds retain
  `org.rstorrent.bootstrap` for the existing test harnesses.
- Keep all signing secrets outside version control and public documentation.
- Store declarations must describe the actual release. A copied answer is a
  comparison baseline, not evidence of RSTorrent's behavior.
- Preserve the first-party in-process engine and existing task owners; this
  slice adds no new runtime owner or networking layer.
- Validate the final manifest, ID, version, target SDK, release signing,
  packaged ABIs, and generated boundary before upload. Run proportional
  Android build/tests and installed coexistence evidence after changes.
- Verify saved console settings and report draft/review/live status exactly.

## Read-Only Play Console Baseline

Observed from the logged-in JSTorrent console on 2026-09-06:

| Setting | JSTorrent value |
| --- | --- |
| Package | `com.jstorrent.app` |
| Default language | English (United States), `en-US` |
| Type / category | App / Tools |
| Price | Free |
| Tags | None shown; Tools suggested |
| Support email | graehlarts@gmail.com |
| Website | https://jstorrent.com/ |
| Phone | Empty |
| External marketing | Disabled |
| Privacy policy | http://graehlarts.com/privacy |
| Ads / advertising ID | No / No |
| Government / health / financial features | No / None / None |
| Target audience | 13–15, 16–17, 18 and over |
| Content rating | Everyone / PEGI 3 / corresponding regional all-ages ratings |
| Data safety | Does not collect/share data; data is not encrypted |
| Sign-in details | Restricted; instructions to install JSTorrent Chrome extension |
| Foreground service | `FOREGROUND_SERVICE_DATA_SYNC` |
| Full-screen intent | `USE_FULL_SCREEN_INTENT` declared |

The source listing has a 512×512 icon, a 1024×500 feature graphic, seven phone
screenshots, five Chromebook screenshots, no tablet screenshots, and no
YouTube video. Asset reuse was explicitly authorized by the maintainer. The icon and seven
phone screenshots have been copied into `clients/android/play/` and uploaded
to the separate listing. Its README records provenance and deviations. The
console's inline thumbnails are reduced resolution, so use the original
asset-library files rather than those thumbnails for upload.

Source short description:

> Fast, lightweight torrent client for Android and ChromeOS.

Source full description describes phone/tablet downloading, magnet and
`.torrent` intake, background notifications, a simple interface, and companion
operation with the existing JSTorrent Chrome extension. Adapt the last claim
to the canary's actual extension compatibility before saving it.

## Saved Console State

Verified on 2026-09-06 after accepting the creation certifications with the
maintainer's explicit confirmation:

- Separate RSTorrent Canary app created as Free, App, English (US).
- Tools category, no tags, support email, HTTPS website, empty phone, and
  disabled external marketing copied from JSTorrent.
- Privacy URL copied exactly; ads and government answers are No; health and
  financial features are None; target ages are 13–15, 16–17, and 18+.
- Sign-in details are No because the standalone canary requires no login.
  JSTorrent's old extension-installation restriction was not copied.
- Adapted canary text, duplicated icon, seven placeholder JSTorrent phone
  screenshots, and a new 1024×500 canary banner are saved. The banner reuses
  the source icon and is individually labeled as created/edited with AI.
  The console reports the default listing **Ready to send for review**.
- Dashboard verifies **9 of 11 complete**. No bundle uploaded, review sent,
  production rollout started, or JSTorrent production settings changed.
- Chromebook screenshots have not been copied: the source console only
  exposed reduced thumbnails and original local assets were not found.
  Automatic protection, countries, signing, and track settings remain to be
  compared when preparing the release.

## Initial Console-Setup Checkpoint (Superseded By Follow-up)

- Content rating: contact and All Other App Types category are prepared.
  The separate IARC Terms of Use acceptance question remains pending; do not
  treat the earlier app-creation certification approval as its answer.
- Data safety: no answers saved. `AndroidFeedback.kt` sends platform, app
  version, Android OS, and device make/model in an optional external-browser
  feedback URL. Tactical 208 keeps additional installation/counter fields
  release-disabled pending hosted disclosure verification. Reconcile actual
  release behavior, hosted privacy text, and the declaration before saving.
  The copied console privacy URL is `http://graehlarts.com/privacy`; the app
  links to `https://jstorrent.com/privacy.html`. Do not infer equivalent text.
- `clients/android/app/build.gradle.kts` still uses
  `org.rstorrent.bootstrap`, version code 1, version name `0.1`, compile/target
  SDK 35, and no release signing configuration. No release artifact exists.
- Google's current [target API requirement](https://support.google.com/googleplay/android-developer/answer/11926878?hl=en)
  requires API 36 for new ordinary Android apps after August 31, 2026.
  Prepare and validate the SDK/toolchain change before upload.
- The manifest declares foreground data sync but no full-screen intent.
  `CommandReceiver` is exported and forwards all received extras to the
  diagnostic engine service without a debug-only guard. Restrict diagnostic
  reachability in release builds and validate the merged release manifest.
- Configure durable signing outside version control; validate the final ID,
  version, ABIs, generated boundary, signing, and installed coexistence.
  No application/build code was modified during the console setup.

Validation so far: console saved-state checks, visual inspection of the
1024×500 banner, dimensions of copied assets, and `git diff --check`. No
runtime tests or Android builds ran because only docs/store assets changed.

Next actions: resolve the pending IARC agreement and questionnaire, reconcile
Data safety and hosted disclosure, complete original Chromebook artwork and
release-setting comparison, then prepare and validate the signed AAB. The
listing itself is saved; the content-rating browser form remains open for
handoff. This is preparation progress, not publication or beta graduation.

## Authorized Android Release Automation Follow-up

The maintainer reports completing the remaining console forms and on
2026-09-06 approved a tag-driven Android release workflow, version/changelog
release helper, downloadable signed APK/AAB, and a dedicated upload key with
a durable local backup. This follow-up implements those release mechanics.

Scope: independent Android versions, `android-vX.Y.Z` tags, manual CI build
rehearsals, pinned toolchains/actions, dual ABI native packaging, signed
artifact inspection, and an Android release runbook. No engine/API change,
Play rollout, JSTorrent signing-key reuse, or legacy state migration.

JSTorrent reference: `scripts/release-android.sh`,
`.github/workflows/android-ci.yml`, `android/app/build.gradle.kts`. Adopt its
Gradle versionName/versionCode and tag/changelog flow; improve strict version
validation, dry run, atomic push, tag/config agreement, mandatory signing,
artifact integrity checks, and prerelease publication without changing the
desktop latest release. Source inspected for behavior only, not copied.

Prepare API 36 with AGP 8.10.1 (existing Gradle 8.11.1 is compatible) and
NDK 28.2.13676358 for 16 KiB alignment. Preserve the bootstrap debug package
for existing test harnesses; use the registered canary ID for release.
Move diagnostic receiver registration into the debug manifest.

Required evidence: helper failure-path tests, shell/workflow checks, dual ABI
build, JVM tests/lint, merged release manifest, APK/AAB signature checks,
packaged ABI/ELF and APK zip alignment. Keep signing material outside Git,
with directory mode 0700 and files 0600. CI keys are temporary and cleaned
on failure. Stop at usable release automation and signed artifacts, reporting
any unexecuted hosted or installed-device evidence explicitly.

### Release Follow-up Evidence

Local signed `0.1.0` / versionCode `1` APK and AAB built successfully with
both ABIs and the dedicated upload key. The local backup is outside the
repository with restrictive permissions; all four Android signing secrets
are configured and their names verified in GitHub. Only the public certificate
is versioned. The maintainer reports completing the console forms; this
follow-up did not resubmit them or upload to Play.

Validation completed:

- `clients/android/build.sh release`: dual native ABI compilation, both
  UniFFI generations, signed APK/AAB, 113 release JVM tests, and release lint.
- Direct Gradle `assembleDebug testDebugUnitTest lintDebug
  assembleDebugAndroidTest`: 113 debug JVM tests, lint, and instrumentation
  compilation. No device instrumentation execution is claimed.
- `scripts/validate-android-release.py`: both upload signatures, expected
  certificate, package/version/SDK manifest fields, diagnostic exclusion,
  identical native payloads across APK/AAB, dual ABIs, ELF and ZIP 16 KiB
  alignment, and bundletool validation.
- Missing-signing rejection verified both through the build helper and direct
  Gradle release invocation; signed tasks revalidated after tightening it.
- Seven release-tool tests including an actual commit/tag/atomic push to a
  temporary local bare repository, invalid versions/tags/changelogs, manifest
  rejection, and native alignment failures.
- Shell syntax, new workflow actionlint, and `git diff --check` pass. The old
  actionlint version flags the existing unrelated `macos-26` labels when
  scanning the full CI file; it finds no issue in the new Android workflow.

No engine logic or shared application contract changed. Minification remains
disabled deliberately for the canary. The hosted rehearsal and tagged release
both passed; Play installation and 16 KiB runtime qualification remain
separate platform evidence.

### Hosted Release Completion

Verified on 2026-09-07:

- [Manual rehearsal 34037979177](https://github.com/kzahel/rstorrent/actions/runs/34037979177)
  completed successfully and uploaded the signed APK/AAB Actions artifact.
- [Tagged run 34039237324](https://github.com/kzahel/rstorrent/actions/runs/34039237324)
  passed both the signed-build job and publication job for `android-v0.1.0`.
- [Android 0.1.0](https://github.com/kzahel/rstorrent/releases/tag/android-v0.1.0)
  is a published GitHub prerelease with APK, AAB, and SHA256SUMS. Downloaded
  release artifacts match their published checksums. This is GitHub
  publication, not a Google Play rollout.
- The portable local signing-backup ZIP was checked against all four source
  files. The key directory is 0700; files and the ZIP are 0600. Private key
  material and passwords remain outside version control.

The approved release-automation stopping condition is met. Future Android
versions use the release helper and changelog described in the runbook.
Store installation, upgrade/coexistence qualification, and a Play rollout
remain explicit subsequent release actions.
