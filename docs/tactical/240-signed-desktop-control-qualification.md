# Tactical 240: Signed Desktop Control Qualification

Status: **Active, 2026-09-29.** Campaign 231; follows 232–239.
Topics: `beta-release-readiness`, `desktop-jstorrent-replacement`,
`client-surfaces`, `capability-readiness`.

## Outcome And Scope

Publish one current-source signed Latest prerelease through the existing CI
workflow, then qualify its installed desktop-control path in claimed macOS
arm64, Windows x86_64 and Linux x86_64 guests. The user explicitly requested
this release and installed verification. Keep Stable, production identities,
updater keys and routes unchanged. No legacy importer or personal migration.

First correct stale CI harness expectations exposed by the current source:
the package hello includes `desktop_control_v1`, and pending-add cancellation
removes the torrent before Android's diagnostic snapshot is projected. A
removed row cannot be required to retain its ID in that projection. Preserve
the strict capability list, cancellation marker, joined removal and independent
no-payload assertions. No engine or Android product behavior changes are
planned.

## Contracts, Dependencies And Limits

Read the release runbook, 230/232/234/237/238/239 and owning topics. Use the
existing verified-main selector and five-target signing/notarization pipeline;
record its exact selected source, run, version and published artifact hashes.
Successful older-source CI cannot qualify current-source delivery. Builder
checks precede installed checks. macOS initial installation uses the DMG;
updating uses the signed `.app.tar.gz`. The native messaging host is bundled,
then registered/copied per user by the installed application. Verify both
bundled and registered helper trust, not only the outer installer.

Use Machine Control discovery, read-only doctor, exclusive claims and supported
readiness. Preserve inherited apps, profiles, registrations and power state.
Use separately identified test browsers and a fresh native-selected library.
Retain 232/234's one-runtime, explicit-intent, attach-only reconnect, picker
ownership/cancellation and joined-Quit contracts. Use the existing bounded
32-MiB controlled fixture and independent hash oracle, never a public swarm.
Own and terminate browsers, fixtures and helper processes; remove task artifacts
and release claims. No new persistent task or resource owner is introduced.

## Validation And Stopping Condition

1. Correct the stale harnesses, run relevant local checks, commit and push
   normally, and require exact-source main CI success.
2. Dispatch the existing Latest workflow; verify all five signed package jobs,
   collector, immutable assets and the selected source. Record failures without
   bypassing source, signature or package checks.
3. Install the exact published candidates in guests. Verify platform trust,
   first-run registration and bounded repair, cold background extension launch,
   repeated/warm attach, native launch, singleton requests, shared library,
   valid/invalid/stale credentials, pause/resume, exact controlled bytes,
   picker success/cancel/disconnect/Quit, passive no-resurrection and explicit
   relaunch. Exercise the existing signed update path with a controlled older
   cohort and record catalog compatibility separately from external bytes.
4. Record exact commands/evidence and clean inherited state. Update the campaign,
   living topics and release runbook; commit bounded completed slices.

Stop when that bounded signed installed matrix passes or an evidenced external
blocker prevents further progress. Native sleep/wake, interrupted replacement,
broader browser endurance, production graduation and importer work remain
separate gaps; this checkpoint makes none of those claims.

## Initial CI Findings

Prior main CI `36609998712` fails the exact package hello assertion on all four
desktop checks: actual capabilities are `launch_desktop, desktop_control_v1`.
Its Android SAF scenario records `pending_add_cancelled` followed by
`torrent_removal_completed` with `torrent=none`; the removed local row is no
longer projectable. The scenario clears logcat after the preceding fixture's
joined cleanup, admits one pending local fixture and independently checks its
payload namespace. Requiring the cancelled row's ID in the final diagnostic
snapshot is the stale predicate. These are harness failures, not yet evidence
of an installed signed product failure.

The later `0aa7a56e` CI run's Android job passes without this fix, confirming
the observation depends on projection timing; the package predicate remains
strictly stale. Local validation for the bounded harness correction:

```sh
source ~/.profile
cargo build -p rstorrent-native-host
node scripts/smoke-native-host.mjs target/debug/rstorrent-native-host
python3 -m unittest discover -s scripts -p test_android_runtime_smoke.py
python3 -m py_compile clients/android/run_bootstrap.py
node --test scripts/*release*.test.mjs scripts/*desktop*test.mjs
python3 -m unittest discover -s scripts -p test_distribution_review.py
git diff --check
```

All pass: exact native hello, five Android runner controls, 25 release/package
tests and 18 distribution-review tests. Installed Android cancellation remains
the owned API-35 CI gate; these local checks do not claim that runtime evidence.
Official [Tauri signing](https://v2.tauri.app/distribute/sign/macos/),
[Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
and [Chrome native messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
guidance was reviewed for the existing signing and registered-helper contract.

The release workflow now also requires valid expected-publisher Authenticode
signatures on both installed Windows executables, including the native host.
Installer signatures alone do not establish this. macOS already performs deep
strict bundle verification; guest checks additionally inspect the copied host.

## Installed Matrix Preparation (Not Current-Source Qualification)

The source fixes are `2f246efd`; installed Windows signature enforcement is
`99bbe08d`. Normal pushes start exact-source CI `36620048563`. The superseded
`0aa7a56e` run required force-cancellation after ordinary cancellation left its
iOS job stuck in `Complete job`; no failed gate was waived. The corrected
Android owned runtime, web, extension, workflow and iOS jobs pass; remaining
jobs are still running at this checkpoint. No new release has been dispatched.

Common Machine Control doctor/claim/ensure-ready prepares the Apple-hosted
macOS 26.6.2 arm64 guest (inherited suspended), Windows 11 build 26200 x86_64
guest (off), and recreated Ubuntu 24.04.5 x86_64 guest (off, glibc 2.39).
Windows has an inherited installation/profile: move aside three paths, record
315 file hashes and seven affected registry key export/absence receipts before
installing. No source overlay is needed. macOS LaunchServices and Linux MIME
state are preserved. Cleanup and claim release are still pending.

Stage signed 0.2.501 as the controlled update baseline, never as current-control
evidence. Published/downloaded hashes:

| Artifact | SHA-256 |
| --- | --- |
| macOS arm64 DMG | `dd4beece26907d22c23aeab96798cd31f0c0a5264e7ba9dc36297ff5aaa39970` |
| Windows x64 NSIS | `17c43294a3ea6a100c743e8761ffb8a0021c2d153acc24c93c85963c08f5f9e9` |
| Linux x64 AppImage | `b6ce090248313e9752b7e505a976c6d58bc6496f63c92d6dc1102f173942670f` |

Builder inspection proves macOS arm64/minimum 13.0 and Linux x86_64/maximum
required GLIBC 2.34 before guest transfer. macOS deep strict verification and
Gatekeeper report Notarized Developer ID. Windows installer and both installed
executables have valid Kyle Graehl Authenticode signatures. The macOS registered
host preserves its bundled bytes, SHA-256
`61eee779dd26215c047d01f20efd9ebc7703cce896af68e2c7862a66b7e192fe`,
Developer ID team `VD7BYQ6ABM` and hardened-runtime flag. A standalone host
`spctl --assess --type execute` reports valid code that does not seem to be an
app; do not equate that app-oriented assessment with broken code signing or
claim standalone Gatekeeper acceptance. Browser execution is a separate gate.

Native Downloads/Add folder selects fresh `root-one` on each guest; native
About/updates selects Latest. External sentinel SHA-256 is
`f869fbf0f9f7176e8baa3b1a2bf4657367aaeddc8d586724a5be609d45d2e5c0`.
Both baseline and current source use catalog schema 26. Chrome for Testing
151.0.7922.34 is separately installed under task-only roots with CDP 9222 and
the packaged beta extension; no normal host browser runs. The beta extension
ZIP hash is `a21fb6e52ee720a83987d0b5094a60e1a338014c024cdaa977cbcadd47538533`.

The Windows skill's low-level login example incorrectly treated inventory ID
`winvm` as an SSH destination on the Linux controller. That failed before
credential delivery. Common claimed `testbed -- login` resolves the actual
transport and stored credential and confirms the Default desktop. Machine
Control commit `71a0865` corrects the guide; it is committed locally, not pushed.

## macOS Standard Browser Registration Correction

Before publication, source review finds the macOS Chrome for Testing support
root misspelled as `Google/ChromeForTesting`. Official Chromium
[`chrome_paths_mac.mm` at 151.0.7922.34](https://raw.githubusercontent.com/chromium/chromium/151.0.7922.34/chrome/common/chrome_paths_mac.mm)
uses `Google/Chrome for Testing` when no `CrProductDirName` override exists;
the exact installed 151.0.7922.34 bundle has no override. Linux's documented
`google-chrome-for-testing` root already matches. Correct the macOS root and
exercise initial manifest creation and deletion repair in the real spaced
directory, without creating an absent ordinary Chrome root.

The 232/237 default test-root repair evidence used the misspelled directory;
its custom-profile browser tests remain valid, but that evidence did not prove
actual default-profile discovery. This checkpoint replaces that registration
claim with an installed run using the test browser's standard support root.
Re-run exact-source CI after this correction before dispatching the release.

Local correction gates pass: `cargo fmt --all -- --check`,
`cargo test -p rstorrent-desktop --lib` (54 tests),
`cargo clippy -p rstorrent-desktop --all-targets -- -D warnings` and
`git diff --check`. This is a desktop registration adapter change; no shared
application boundary or Android behavior changes.

## Newly Reported Web Test Dependency Advisories

The next normal push reports six new open Undici alerts, including a high
TLS-validation finding. `npm ls undici --prefix clients/web` identifies the
locked `jsdom@29.1.1 -> undici@7.29.0` test dependency. Review upstream
[7.29.1 security fixes](https://github.com/nodejs/undici/releases/tag/v7.29.1)
and [7.30.0 changes](https://github.com/nodejs/undici/releases/tag/v7.30.0),
then update only that compatible transitive lock entry. Keep the existing
three-lockfile release audit fail-closed, including the independently reviewed
GLib backport; do not dismiss alerts or merge unrelated Dependabot changes.
Web type, unit and production/CSP build checks and fresh audit review precede
the next exact-source CI. This does not add a shipped Node runtime dependency.

`npm update undici --prefix clients/web --ignore-scripts` changes only the
Undici lock entry to 7.30.0 (version, URL and integrity); direct dependencies
remain unchanged. `npm run typecheck --prefix clients/web`,
`npm test --prefix clients/web` and `npm run build --prefix clients/web` all
pass, including the production CSP check. Fresh Cargo, web npm and website npm
collection followed by `scripts/review-dependency-audit.py
--require-release-ready` reports `release_ready=True`. The separately exposed
GitHub GLib version alert remains visible; its exact source backport review is
unchanged. No audit exception was added.

## Exact-Source CI And Release Dispatch

Main CI [`36623804397`](https://github.com/kzahel/rstorrent/actions/runs/36623804397)
passes all ten required jobs at
`ef5e2f31279b899aa8cee9b48e6ce5b85f67fd67`: Rust/workspace/controlled interop,
web browser tests, extension packaging, workflow tools, Android owned storage
lifecycle, iOS simulator/archive, and all four desktop package checks. The two
manual-only jobs are skipped by design. Windows installer compilation is the
last gate; no gate is bypassed or accepted from an older source.

```sh
gh run view 36623804397 --json status,conclusion,headSha,jobs
gh workflow run nightly-desktop.yml --ref main -f force=true
gh api repos/kzahel/rstorrent/actions/jobs/109617116848/logs
npm run package --prefix clients/extension
shasum -a 256 target/extension/jstorrent-beta-0.4.0.zip
```

Forced Nightly run
[`36630173322`](https://github.com/kzahel/rstorrent/actions/runs/36630173322),
dispatched 2026-09-29 at 20:59:06 UTC, selects that exact source and
`desktop-latest-v0.2.701`. The independently rebuilt beta extension remains
byte-identical to the staged ZIP above. Signing, publication and installed
qualification are still pending at this checkpoint; selection alone is not
release evidence.

## Published 701 And Outermost DMG Correction

Run 36630173322 publishes `desktop-latest-v0.2.701` at 2026-09-29
21:41:08 UTC from `ef5e2f31`. All five package legs and collector pass.
Independent public download verification checks all 13 SHA256SUMS entries,
GitHub asset digests, the immutable tag source, and all ten unique updater
payload signatures using the checked-in public key and `minisign -V`.
The manifest has 15 platform aliases. Native About/updates installs 701 over
signed 501 on all three guests; Mac/Linux sentinel and native-host bundle/copy
hashes match, correct default Chrome for Testing manifests appear, and host
directories become mode 0700. Windows installed app and both helper copies
have valid Kyle Graehl Authenticode signatures. Full lifecycle qualification
remains in progress.

An independent outer-container check catches a release pipeline omission:
`xcrun stapler validate RSTorrent_0.2.701_aarch64.dmg` has no ticket, and
`spctl --assess --type open --context context:primary-signature --verbose=4`
rejects it as `Unnotarized Developer ID`. The app inside passes deep/strict
code signing, execute assessment and stapled-ticket validation. The identical
app in the signed updater archive is also notarized. Do not confuse an app's
ticket with a DMG's ticket, or replace published 701 assets in place.

Apple's [packaging guidance](https://developer.apple.com/documentation/xcode/packaging-mac-software-for-distribution)
and [custom workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)
require submission of the distributed outer container; [TN2206](https://developer.apple.com/library/archive/technotes/tn2206/)
specifies the disk-image assessment context. Inspect pinned Tauri CLI 2.11.4
`crates/tauri-bundler/src/bundle/macos/app.rs` (notarizes/staples app) and
`macos/dmg/mod.rs` (signs DMG without notarizing). Add explicit bounded
20-minute DMG submission using existing App Store Connect credentials, require
Accepted with no reported issues, retain submission/log JSON for 14 days,
staple/validate, and require the correct Gatekeeper assessment. This occurs
before final hashes/staging; the updater archive and its signature are
unchanged. Publish a new immutable Latest only after exact-source CI passes.

Correction checks: `actionlint .github/workflows/desktop-release.yml`,
`node --test scripts/validate-desktop-release.test.mjs
.github/scripts/desktop-release-artifacts.test.mjs` (17 tests), and
`git diff --check` pass. Local `xcrun notarytool submit --help` confirms the
team-key arguments, JSON result and bounded wait syntax. Actual notarization
and disk-image trust remain signed CI/public-artifact gates, not mocked claims.
