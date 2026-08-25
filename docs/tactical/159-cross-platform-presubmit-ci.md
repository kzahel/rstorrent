# Tactical 159: Cross-Platform Presubmit CI

Status: **Complete (2026-08-22).** Cheap deterministic build and test coverage
now passes on hosted runners across every maintained product platform.
Release/updater Tactical `158` retains the remaining signed-package work and
is again **Now** after desktop-notification Tactical `164` completed.

Topics: `beta-release-readiness`, `capability-readiness`, `client-surfaces`,
`product-direction`, `product-surfaces-and-migration`

Dependencies: completed beta foundation Tactical
[`157`](157-beta-release-foundation.md); decision-complete updater Tactical
[`158`](158-desktop-signed-packaging-and-updater.md); the maintained Rust
workspace, React/Tauri desktop, Android, and iOS build/test entry points; and
the locked loopback libtorrent environment under `tests/interop`.

## Decision And Desired Outcome

Make an ordinary pull request or `main` update answer five useful questions
without credentials, physical devices, public swarms, or release publication:

1. Does the shared Rust engine and application workspace format, lint, test,
   and interoperate on one deterministic loopback smoke?
2. Does the generated web contract remain checked in exactly, and does the
   shared React product typecheck, unit test, production build, and pass its
   deterministic browser end-to-end suite?
3. Does the Tauri desktop product compile, run its native tests, and create one
   unsigned canonical package on macOS, Windows, and Linux?
4. Does Android cross-build both supported Rust ABIs, generate the Kotlin
   boundary, lint, run JVM tests, and compile its instrumentation test APK?
5. Does iOS regenerate its boundary/project, run deterministic simulator unit
   and UI tests, and produce an unsigned device archive?

This is a smoke and regression floor, not release attestation. Signed
packages, clean-machine install/update, native architecture breadth, physical
mobile/ChromeOS evidence, and public-swarm reliability retain separate gates.

## Scope And Stopping Condition

This tactical owns:

- one credential-free presubmit workflow on pull requests, `main`, and manual
  dispatch with read-only repository permissions and per-ref cancellation;
- exact reviewed action revisions rather than moving major tags;
- a Linux Rust job running workspace format, warnings-denied clippy, workspace
  tests, and one small loopback v1 piece transfer against locked libtorrent;
- a Linux web job regenerating and diff-checking the application contract,
  then running typecheck, unit, production/CSP, and deterministic Playwright
  checks with failure traces retained;
- native Tauri smoke legs for current macOS arm64, Windows x86_64, and Linux
  x86_64 runners, each running desktop Rust tests and producing one unsigned
  platform package;
- an Android job using Java 17, the pinned NDK, Rust `x86_64` and `arm64`
  targets, cargo-ndk, the generated UniFFI boundary, Gradle lint/JVM tests,
  debug APK, and instrumentation APK assembly;
- an arm64 macOS iOS job using the repository's Xcode project generator,
  exact installed Xcode 26.6, simulator unit/UI suite, and unsigned
  generic-device archive;
- bounded failure-artifact retention for browser, Android, and Apple test
  reports rather than unconditional large package uploads;
- repair of the scheduled performance workflow's action reference and a
  syntax/static workflow validation pass; and
- documentation of exact required check names, typical cost, deliberate
  omissions, and the first observed GitHub run.

The implementation portion stops when all commands are represented by valid
workflow syntax, the affected scripts and deterministic suites pass locally
where the current machine can run them, and active docs agree on coverage. The
tactical itself closes only after one pull-request or `main` GitHub run proves
every job on its hosted operating system. Local success cannot prove runner
images, package repositories, virtualized simulators, or Actions permissions.

## Non-Goals

- signing, notarization, updater artifacts, tags, releases, deployment, store
  upload, or use of publisher credentials;
- public-swarm, WAN, comparative performance, long protocol matrices, or
  changing-network success thresholds in required presubmit;
- Android emulator or physical-device execution until its setup/runtime signal
  justifies the added cost; instrumentation compilation and JVM product logic
  are the initial Android floor;
- iOS physical-device, development-signing, TestFlight, selected-root, or
  indefinite-background claims;
- macOS x86_64, Linux arm64, or every installer format in ordinary smoke CI;
  those remain release-matrix requirements even though the three operating
  systems receive native package evidence here;
- automatic retry that hides a deterministic failure; or
- making the performance workflow a required low-latency presubmit.

## CI Contract

All jobs:

- use fixed runner generations and full action commit revisions with the
  reviewed release tag recorded in a comment;
- have explicit timeouts, least permissions, fail-fast behavior local to the
  relevant matrix, and concurrency cancellation by ref;
- install only declared toolchain inputs and use repository lockfiles;
- do not read release, store, updater, signing, deployment, or device secrets;
- leave live/public tests skipped by their existing explicit opt-in contract;
- preserve useful failure reports for a bounded number of days; and
- fail on warnings/drift rather than silently rewriting tracked generated
  files.

Required checks should be stable job names rather than matrix display strings
that change casually. Branch protection remains a maintainer action only after
the checks demonstrate useful signal.

## Platform Evidence Boundary

| Job | Required evidence | Explicitly not claimed |
| --- | --- | --- |
| Rust | fmt, workspace clippy/tests, one locked loopback v1 transfer and cleanup | broad interoperability, public reliability, performance |
| Web | generated drift, type/unit/production build, deterministic Chromium E2E | live gateway, public swarm, native Tauri lifecycle |
| Desktop macOS | desktop Rust tests, unsigned arm64 `.app` bundle | x86_64, signing, notarization, DMG install/update |
| Desktop Windows | desktop Rust tests, unsigned x86_64 NSIS package | signing, clean-machine install/update, MSI |
| Desktop Linux | desktop Rust tests, unsigned x86_64 AppImage | arm64, DEB/RPM, distro install/update |
| Android | both native ABIs, generated Kotlin, lint/JVM tests, app and test APKs | emulator/device behavior, signed AAB, Play/ChromeOS update |
| iOS | generated Swift/project, simulator unit/UI tests, unsigned device archive | signing, physical lifecycle, TestFlight update |

## Validation Matrix

- `actionlint` against every workflow and `git diff --check`;
- the repository Rust baseline;
- locked web dependency install, generated drift check, typecheck, unit,
  production build, and deterministic Playwright suite;
- one local unsigned Tauri package on the available host platform;
- `clients/android/build.sh`, Gradle lint/unit/instrumentation packaging;
- the iOS simulator test runner and unsigned archive; and
- one actual complete GitHub-hosted workflow run before closure.

Record exact skipped/inapplicable local platform legs. Do not mark a hosted
runner green from YAML presence or another operating system's local build.

## Escalation And Next Slice

Workflow/source edits, fixed action revisions, cache policy, bounded report
uploads, script portability, and deterministic test fixes are authorized.
Stop before enabling branch protection, using secrets, pushing solely to
trigger external CI, mutating a testbed, or weakening a failing product test
into a skip.

After hosted closure, return updater Tactical `158` to **Now**. Release-only
architecture/installer breadth, Android emulator execution, longer controlled
interop, and any flaky or expensive test get separately justified additions
rather than silently growing ordinary presubmit latency.

## Implementation Checkpoint

The 2026-08-22 implementation adds `.github/workflows/ci.yml` with Rust/web,
three-host desktop, Android, and iOS jobs. It also pins every action in the
existing website and performance workflows to a reviewed full revision,
repairs the performance workflow's invalid `setup-uv` reference, makes the
iOS scripts independent of a maintainer-specific shell profile, adds a
simulator-selection test entry point, selects Xcode 26.6 explicitly, and makes
Playwright use its locked Chromium in CI while retaining system Chrome for
local runs.

Local evidence on the available arm64 macOS host passes:

- all workflow files pass `actionlint` `v1.7.9`, YAML parsing, and
  `git diff --check`;
- workspace format, warnings-denied clippy, and tests pass; the locked
  libtorrent `first_verified_piece.py --runs 1` transfer verifies its exact
  payload and cleans up in under one second;
- generated web contract drift, typecheck, 248 unit tests with 2 skips,
  production/CSP build, and CI-mode Playwright pass with 33 deterministic
  tests and 12 explicitly live-only skips;
- five desktop native tests and an unsigned arm64 `RSTorrent.app` package
  pass;
- Android's x86_64 and arm64 Rust builds, generated Kotlin, JVM tests, debug
  APK, lint, and instrumentation-test APK assembly pass; and
- 25 iOS unit tests, 2 iOS UI navigation/accessibility tests, and the unsigned
  arm64 device archive pass.

The Android compiler still reports deprecated activity-result, Wi-Fi,
notification, and system-bar APIs. The Apple archive reports asynchronous
`NSLock` and `DispatchGroup.wait` calls that become errors under Swift 6 mode.
These are recorded release-hardening gaps, not silently filtered warnings.

Hosted execution then proved the portability and signal contract rather than
merely accepting the first YAML draft. Iteration exposed and corrected the
Android SDK-manager location, clean-checkout Tauri asset dependency, an E2E
timing boundary, Windows SQLite handle lifetime, and loopback tests that had
silently depended on a global IPv6 route. None of those failures was converted
to a skip or hidden by automatic retry.

Final `main` run
[`32569246987`](https://github.com/kzahel/rstorrent/actions/runs/32569246987)
passes all seven stable jobs:

- Rust format, warnings-denied workspace clippy/tests, workflow lint, and the
  locked exact first-piece libtorrent transfer;
- generated web-contract drift, typecheck, unit and production/CSP build, plus
  33 deterministic Chromium E2E tests with 12 explicit live-only skips;
- desktop native tests and one unsigned arm64 macOS app, x86_64 Windows NSIS,
  and x86_64 Linux AppImage package;
- both Android Rust ABIs, generated Kotlin, JVM tests, lint, debug app APK, and
  instrumentation test APK; and
- generated iOS boundary/project, 25 simulator unit tests, 2 simulator UI
  tests, and one unsigned device archive under exact Xcode 26.6.

The complete hosted run took 23 minutes 56 seconds. Individual job wall times
were 5:54 web, 6:49 Linux desktop, 7:14 macOS desktop, 8:35 Android, 13:13
iOS, 18:17 Rust/interop, and 23:47 Windows desktop. Windows compilation is the
ordinary critical path; every job remains below its explicit timeout.

Manual performance run
[`32568169955`](https://github.com/kzahel/rstorrent/actions/runs/32568169955)
also passes the repaired hosted smoke and retains JSON-only artifact
`performance-32568169955-1`. This proves workflow artifact production but does
not close the separate requirement for the first successful weekly scheduled
run.

The tactical stopping condition is satisfied. Remaining macOS x86_64/Linux
arm64 package breadth, signing, install/update, Android emulator/release AAB,
physical mobile evidence, broad application lifecycle interoperability,
public-swarm reliability, and branch protection remain explicit release gates
under the beta-readiness topic.

Post-completion run
[`32703372543`](https://github.com/kzahel/rstorrent/actions/runs/32703372543)
on 2026-08-24 passes all seven jobs after the Windows local-network repair.
The preceding run exposed one scheduler-dependent storage-pool test; commit
`7be2397` replaces assumed concurrent timing with the existing controlled
platform broker. The follow-up run's first iOS attempt timed out while Xcode
launched the application on the hosted simulator. One failed-job rerun passed
the unchanged simulator unit/UI tests and unsigned archive; no check was
suppressed or automatically retried.
