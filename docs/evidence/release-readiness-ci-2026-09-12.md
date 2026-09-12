# Release Readiness Hosted Qualification, 2026-09-12

## Source And Execution

Manual CI run
[`34682523287`](https://github.com/kzahel/rstorrent/actions/runs/34682523287)
qualifies commit `1599a0e67960c14562d08a0c6454b70024e546f6` on
`release-readiness-ci`. The branch avoids the public website deployment
triggered by pushing `main`. No tag, release, production route or support
declaration is part of this run. The initial partial run `34682455970` was
canceled before redispatching the combined qualification; it is not a pass.

Manual CI calls the same storage-recovery and advisory workflows used by
their schedules, at the caller's exact source revision. This permits
qualification before those new workflow files reach the default branch.
Ordinary push/PR coverage, timeouts and evidence-retention bounds are unchanged.

## Completed Evidence

Web contract regeneration has no drift. Type checking, production build and
the 12-bundle CSP check pass. Vitest reports **386 passed / 2 skipped**;
Playwright's owned Chromium reports **43 passed / 14 skipped**, including
the four support-report phone/wide light/dark cases. Opt-in interop/live
cases retain their existing skips. Extension tests and companion ZIP
packaging pass independently, as do workflow/release-tool checks.

The extended application lifecycle passes all four cases: ordinary length
and cross-file layouts, plus oversized length and one-entry-files layouts.
Every case repairs exactly 32,768 bytes, seeds two complete copies to the
independent client, joins all owners and reports successful cleanup. Storage
ownership peaks at three handles; incoming state peaks at one connection,
one pending/read operation, four queued requests, 63,355 queued bytes and
16,397 writer-buffer bytes. The gateway binary SHA-256 is
`3f752f14a0601d861105559dd2c3cc73dc4e4bdb56c38b2a32ca982740770dc4`.

All three storage topology cases and all three checkpoint crash boundaries
pass against libtorrent 2.0.13.0, reference revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. Before the checkpoint commit,
both crash windows retain zero durable pieces despite 256 valid payload
pieces; after commit, all 256 are durable. Every restart needs zero payload
upload and every case cleans up. The session binary SHA-256 is
`0afc8793c34aa2d593ae5011d8b28c0a6703acdbcdc2a9a3dda8e094220f0481`.

Hosted advisory review matches RustSec revision
`b50980aad8b8f14f77e25a97b32dd94bf008b0af`: zero Cargo/npm vulnerability
entries, six unmaintained warnings, and GLib 0.18.5 unsoundness. The uploaded
report explicitly retains **release_ready: false**. This successful review
checks that the inventory is unchanged and fresh; it does not clear GLib.
Cargo.lock SHA-256 is
`2454174c7f86670db2903f576cc49abadd01e30f668d19d783cc51448dd1a4ed`;
the web lockfile SHA-256 is
`38564d98f51f27ae72450410f4de2e63f582b80721097c00564cd4e3b246a6a4`.

Android passes dual-ABI build/JVM tests, lint, instrumentation compilation and
the actual API 35 x86_64 SAF lifecycle on system-image revision 9. The report
records zero preconfirmation upload, absent skipped files, force recheck,
joined local-torrent cancellation, exact removal, a 3/40 storage-handle peak,
successful cleanup and removal of the owned AVD. APK SHA-256:
`311970e6123b818877773bb8c37fb49ae54079eec5687e36e218f21ba6eea8a0`.
iOS passes simulator tests and the unsigned device archive.

Both Linux architectures pass 44 desktop and 346 session tests, with two
session tests ignored. The actual unsigned AppImages pass native-host,
activation metadata and content/notice inspection. x86_64 has 262 inventory
entries / 324,977,819 bytes and notices for 529 Rust plus 28 npm packages;
ARM64 has 359 entries / 338,646,246 bytes and 527 Rust plus 28 npm packages.
Both contain one notice bundle matching the reviewed lockfiles. These checks
do not establish native-library license clearance or a signed release.

macOS ARM64 passes 44 desktop and 346 session tests (two ignored), unsigned
application packaging, native-host smoke, activation metadata and notice
inspection. Its six inventory entries total 59,105,464 bytes; the notice
bundle covers 459 Rust plus 28 npm packages and matches the same lockfiles.

## Windows Failure And Controlled Repair

Windows passes all 43 desktop tests and 344 session tests, but
`durable_complete_torrent_applies_slots_live_and_fences_lifecycle` times out
waiting for seed registration after recheck. The session total is
344 passed / 1 failed / 2 ignored; packaging is not reached. This run must
remain a failure regardless of successful jobs elsewhere.

Forcing checker termination before application maintenance reproduces the
same five-second, zero-registration timeout locally. The directly owned test
does not run the product maintenance owner. Its old Complete-state poll could
finish before the checker task terminated, leaving no later command to join
the checker and restore seed admission. The repair forces this ordering,
asserts Complete with zero registrations, then drives the existing application
Snapshot path. All original lifecycle/ownership assertions remain. Production
code and generated platform contracts are unchanged.

The forced ordering passes 20/20 repetitions across four concurrent processes;
the complete local session suite passes 346 tests with two ignored. Session
tests clippy with warnings denied and workspace formatting pass. Corrected
hosted Windows/package qualification is pending.

## Remaining Run Evidence

Workspace test results are still pending. Update this record from actual
completed jobs and uploaded evidence; do not claim
complete hosted qualification until the repaired source passes.
