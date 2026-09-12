# Release Readiness Hosted Qualification, 2026-09-12

The execution record is chronological. The GLib backport qualification at
the end supersedes earlier pending-source-decision and dependency-blocker
statements; other signed-release and publication limits remain in force.

## Source And Execution

Initial manual CI run
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
tests clippy with warnings denied and workspace formatting pass. The corrected
hosted Windows/package qualification below subsequently passes.

## Rust Gate And Build Measurements

The first run finishes **11 successful jobs / 1 failed Windows job**. Rust
formatting, workspace/default-WebRTC/feature-disabled clippy, feature-specific
tests, all workspace tests, deterministic libtorrent transfer and the ordinary
two-case application lifecycle pass. Workspace results total **1,504 passed /
18 ignored**, including binary/integration/doc test result lines. The separate
feature-specific tests add 16 passes and one ignored test.

| Rust job step | Hosted wall time |
| --- | ---: |
| Workspace clippy | 189 s |
| Default direct-file graph lint/tests | 466 s |
| Explicit feature-disabled product checks | 118 s |
| Compile workspace tests | 1,176 s |
| Execute workspace tests, including doctests | 187 s |
| Deterministic libtorrent smoke, including its build | 89 s |
| Build application diagnostic | 164 s |
| Application lifecycle and corruption repair | 15 s |

The Rust cache step reports no Rust cache found. Four Cargo timing HTML files
are uploaded, including the latest-report alias. Workspace compilation is
dominated by application/runtime units: desktop library 407.08 s, desktop
tests 283.90 s, headless tests 283.51 s, gateway tests 258.20 s, remote-host
end-to-end test 257.60 s, session tests 224.15 s and engine tests 166.86 s.
These units overlap; their times are not additive or a controlled profile
comparison. Compilation remains the main latency target. Retain `opt-level=2`
and current cache policy until a bounded comparison justifies changing them;
test retries or shorter test coverage would not solve this measured cost.

## Corrected Revision

Commit `8f31f98a48313c57c3e3ba16e5b8907b02d63135` contains the forced-order
repair and first-run evidence. Manual run
[`34684328524`](https://github.com/kzahel/rstorrent/actions/runs/34684328524)
passes **all 12 jobs** at that exact revision. The first run remains failed;
the corrected source earns its own complete hosted result.

The corrected revision passes web, extension, release tools, advisory
review, extended storage recovery, Android runtime and all macOS/Linux package
lanes. Its extended gateway SHA-256 is
`272493c70e3250f39e55cbb55c28f19be1f9250b1bba2e08ebd1820791eccdd8`;
session SHA-256 is
`f7900f0de9f22176ed6e43ad360b0fa79f1de3942001d61d435e0dca2da49ec4`.
The second API 35 x86_64 runtime retains the same image revision 9 and 3/40
handle high water, with all assertions and owned cleanup passing. APK SHA-256:
`0dd0a4b755e0ed1e4fec92cc3da08ccb2c7555b9ab1a9e127c190433f940cc8a`.
The corrected Linux x86_64 package has 262 entries / 324,981,915 bytes;
ARM64 remains 359 / 338,646,246 and macOS 6 / 59,105,464. All three notice
inventories match the reviewed lockfiles. iOS archive and the full Rust job
also pass. Windows passes 43 desktop tests, 345 session tests (two ignored),
the native local-address test, unsigned NSIS packaging, silent installation,
activation registry validation, native-host smoke and notice inspection.

The installed Windows inventory contains five files / 56,880,162 bytes and
notices for 469 Rust plus 28 npm packages. Its checked-out lockfiles use CRLF;
their hashes exactly match the repository files after only LF-to-CRLF
conversion: Cargo `f1bef12e13b287c8ff53a4c0e696d218bbde96add90a166ff9ef6a3a51c089e8`,
npm `4245715975c319223338f92b6293029a2e8c8fdf2174c995e2c1f574a349c623`.
These are input-byte digests, not different dependency resolutions. The exact
upstream license files remain protected from checkout newline conversion.

The unsigned `RSTorrent_0.1.3_x64-setup.exe` is 14,125,162 bytes, SHA-256
`de8e8abee0430392661625e317c34fbcb4793da4e1bf166968ebe577c41520cd`.
Installed desktop executable SHA-256:
`a24a6f7d77619e09b9bf516d952d551a1012eb40c23dee36358c8aaadd0ea5b6`.
This is an unsigned CI artifact, not a replacement for the separate signed
older-to-newer update qualification. Windows native desktop/session/address
steps, including compilation, take 1,083/430/267 s; installing the notice tool
takes 195 s and packaging 1,032 s. The installed registry/inspection step
takes four seconds. These measurements preserve the existing 60-minute bound.

The corrected Rust job again passes 1,504 workspace tests with 18 ignored and
both ordinary application lifecycle cases with cleanup. It restores roughly
1,492 MB of Rust cache; workspace clippy takes 46 s, workspace-test compilation
940 s, test execution including doctests 184 s, and the application cohort
16 s. The changed source and cache state prevent treating this as a controlled
compiler-profile experiment. No profile, cache policy or timeout was changed.

## Release Limits And Evidence Retention

CI-004 and CI-006 now have hosted evidence. The GLib source-maintenance choice,
native-library notice clearance, repaired signed Windows update, public
Tactical 208 disclosure qualification and explicit supported-release version
remain open. No release-ready claim follows from the green CI result.
Sanitized JSON and timing artifacts are retained seven days; unsigned package
artifacts follow their existing three-day retention. Local downloaded logs,
timings, packages and negative-control fixtures are removed after recording
these results. Subsequent evidence-document edits do not change tested code.


## Native Attribution Follow-through

Run [`34687692788`](https://github.com/kzahel/rstorrent/actions/runs/34687692788)
passes all 12 jobs at `a83de080d27adb80932a03c972db96d026c91520`. This adds
checked distro copyright/common-license attribution before AppImage creation
and signing. Both final extracted product packages pass the new manifest gate:

| Native package | Selected components | Distro packages | Notice files | Entries / uncompressed bytes |
| --- | ---: | ---: | ---: | ---: |
| Linux x86_64 | 174 | 113 | 128 | 391 / 327,392,358 |
| Linux ARM64 | 173 | 112 | 127 | 487 / 340,990,858 |

The manifest hashes and source/probe details live in Tactical 214. Launcher
and outer runtime provenance, per-package corresponding-source obligations
and GLib source maintenance remain explicit release review items. Tactical
217 separately adds Android attribution; this run predates that slice.

## Android Attribution Follow-through

At `f7e50a753e03896ffbce56648543ec04c1af90be`, run
[`34689662485`](https://github.com/kzahel/rstorrent/actions/runs/34689662485)
passes the Android dual-ABI/JVM/lint, notice-integrity and owned API 35 runtime
job. Its verified APK contains 83 Maven and 205 Rust attributions, accounting
for all six native libraries. The original notice assets exported from that
APK are retained with `android-notices-34689662485-1`. Their manifest is
byte-identical to the local record in Tactical 217. The runtime report records
APK SHA-256 `8057196c711d9c289ce90a68de3abfee60bb8afd96a90008a774c3eb5e804049`,
3/40 storage handles, no preconfirmation upload, exact recheck/removal, joined
cancellation, successful cleanup and removal of the owned AVD.

The complete workflow finishes **successfully at 2026-09-12 11:19:29 UTC**.
All **12 jobs pass** at that exact implementation revision, including four
unsigned desktop package lanes, Rust/loopback interop, web, iOS, extension,
workflow tools, extended storage recovery and dependency review. The Android
attribution slice is complete. The advisory inventory still explicitly
retains GLib as a release blocker; this run does not qualify a repaired signed
Windows update, publish the disclosure pages, or declare a supported release.

## GLib Backport Qualification

Tactical [218](../tactical/218-glib-variant-iterator-backport.md) adopts the
explicitly approved two-line `glib 0.18.5` repair. Implementation commit
`aa056ff6e4ac1ada4ffe31d1872a2f8699efbe2b` preserves all 121 published source
files and original MIT grants with exact patch/manifest verification. Cargo
selection, audit provenance and packaged notices independently enforce it.
The original registry warning remains visible through a checked audit-only
lock projection, including detection of future GLib advisories.

First run
[`34691933241`](https://github.com/kzahel/rstorrent/actions/runs/34691933241)
has ten successful jobs and two failures. Windows rejects Git's CRLF
conversion of the checksum-bound provenance JSON before build. Commit
`e535a5ffeb6cc9885a5e8f47196890d386960e35` adds the explicit LF rule and a real
Git checkout fixture with `core.autocrlf=true`; ten backport tests pass.
The independent iOS failure is an unchanged mirrored-layout UI test waiting
for the Add sheet. Its xcresult records 33 passed / one failed, with the
failure hierarchy still on Library. No iOS assertion or product code changes.
That first full workflow remains failed.

Corrected-source full run
[`34693467376`](https://github.com/kzahel/rstorrent/actions/runs/34693467376)
is in progress at `e535a5ffeb6cc9885a5e8f47196890d386960e35`. The accidental
immediate redispatch `34693425198` resolved the previous branch head and was
canceled; it is not qualification evidence. The corrected run's iOS job now
passes all 30 unit / four UI tests and the unsigned archive, including the
previous failure, without retrying individual tests or weakening assertions.

Both first-run Linux jobs pass 44 desktop tests, three independent GLib
iterator cases in both dev and release profiles, and 346 session tests
(two ignored). Their actual unsigned AppImages pass source, native-host,
activation, embedded backport notice and native-library inventory gates.
The native manifests inventory x86_64 174 components / 113 distro packages /
128 notices, ARM64 173 / 112 / 127. Exact inventory and source hashes live
in Tactical 218. Both corrected-run Linux jobs repeat those passes. Their
entire extracted inventories are identical to the first run, including all
file hashes, notice manifests and native components. The first-run ARM64
package exercised below therefore has the same packaged product bytes as
the corrected-source build.

The first-run ARM64 AppImage is independently hashed after transfer and
exercised in an owned disposable Ubuntu 24.04 ARM64 GNOME 46 Wayland VM:

- Native GTK picker Cancel leaves zero roots; selection adds the exact sole
  default root and renders its path in Downloads settings.
- The GNOME indicator is visible. With background mode checked, native close
  hides the window, and the exported native tray Show handler restores it.
- Native tray Quit releases the single-instance bus name and exits status 0.
  Relaunch and a second native Quit also pass; read-only catalog inspection
  confirms the same default root and the payload sentinel hash is unchanged.
- Both launch units end inactive with successful result. Machine Control
  shutdown/release discards the overlay; subsequent checks report zero
  temporary workspaces, an available claim and the baseline powered off.

This is current-source unsigned ARM64 package evidence. It does not substitute
for a repaired signed Windows update or change the older signed x86_64 tray
record. No host input, installed primary browser or public swarm is used.

Local checks pass: workspace fmt/clippy/tests (1,501 passed, 18 ignored on
macOS), ten backport integrity/audit/notice tests, 16 distribution review
tests, 23 desktop release-tool tests, 22 Android release/notice-tool tests,
changed-workflow actionlint and Linux/macOS notice generation. Fresh Cargo
and npm reports pass `review-dependency-audit.py --require-release-ready`
against RustSec `b50980aad8b8f14f77e25a97b32dd94bf008b0af`. All seven original
warnings and the October 12 expiry remain. Only the qualified GLib blocker
is removed; mandatory exact-source verification remains. Dependency readiness
does not close native notice/source-delivery review, the repaired signed
Windows update, public disclosure qualification or support declaration.
