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
