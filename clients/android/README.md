# RSTorrent for Android

This directory owns the maintained first-party Android and ChromeOS client. It
packages the real `rstorrent-engine` behind generated UniFFI Kotlin bindings
and gives one foreground service sole ownership of the native product session.
It also retains the original Tactical 004/005 bounded diagnostic harness.
The fixed-descriptor SAF proof is retained behind the engine's
`descriptor-storage-diagnostics` compatibility feature for that diagnostic
service only. The product application has one storage architecture: bounded
dynamic acquisition through an explicitly granted Android Storage Access
Framework tree.

Tactical `157` graduated the complete module from its historical
`experiments/android-engine-bootstrap` path without splitting package,
manifest, foreground-service lifecycle, generated UniFFI, two-ABI packaging,
SAF, Compose, or test ownership. Product sources are isolated under
`Product*`, `AndroidPresentationRepository`, and `ui/`. The older diagnostic
activity, service, and `run_bootstrap.py` profiles remain explicitly named
evidence infrastructure, not an alternate product UI. Historical tacticals
retain their original titles while current commands use this directory.

Build both locked Android ABIs and the debug APK:

```bash
source ~/.profile
clients/android/build.sh
```

The build uses Android Gradle Plugin `8.7.3`, Gradle `8.11.1`, Kotlin
`2.0.21`, JNA Android AAR `5.17.0`, UniFFI `0.31.0`, NDK
`27.0.12077973`, and API 28 Rust targets. It generates Kotlin from the host
native library and packages independently cross-built `x86_64` and
`arm64-v8a` libraries. Generated bindings, native libraries, reports, and APKs
remain under ignored build directories.

Run product lint, unit, and instrumentation packaging after the full build:

```bash
cd clients/android
./gradlew lintDebug testDebugUnitTest assembleDebugAndroidTest
ANDROID_SERIAL=emulator-5554 ./gradlew connectedDebugAndroidTest
```

The instrumentation command requires an explicitly selected owned emulator;
never substitute the first device returned by ADB.

Device execution is owned by `run_bootstrap.py`. Do not install or start this
harness by selecting the first ADB device; the runner verifies an explicit
listed target before mutation.

The runner enters the pinned libtorrent environment automatically. Profiles
are explicit and repeatable:

```bash
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet --runs 3 --profile success
python3 clients/android/run_bootstrap.py \
  --target avd --profile slow-storage --profile cancellation \
  --profile peer-failure --profile duplicate-start \
  --profile activity-recreation --profile preexisting-artifacts
python3 clients/android/run_bootstrap.py \
  --target chromeos --storage saf-internal --runs 3 --profile success
python3 clients/android/run_bootstrap.py \
  --target avd --storage saf-internal --runs 3 \
  --profile product-dynamic-saf
python3 clients/android/run_bootstrap.py \
  --target pixel7a --storage saf-internal \
  --profile product-saf-grant-repair
python3 clients/android/run_bootstrap.py \
  --target avd --storage saf-internal \
  --profile product-https-tracker
python3 clients/android/run_bootstrap.py \
  --target pixel7a --runs 1 --profile product-mse
python3 clients/android/run_bootstrap.py \
  --target avd --runs 1 --profile product-ipv6-policy
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet --storage saf-internal --runs 1 \
  --profile product-incomplete-duplex --no-build
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet --storage saf-internal --runs 1 \
  --profile product-hybrid-saf --no-build
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet --storage saf-internal --runs 1 \
  --profile product-notifications --no-build
python3 clients/android/run_bootstrap.py \
  --target avd --avd rstorrent-task-api35 --avd-api 35 \
  --storage saf-internal --runs 1 --profile product-notifications --no-build
python3 clients/android/run_bootstrap.py \
  --target avd --avd rstorrent-task-api35 --avd-api 35 \
  --storage saf-internal --runs 1 --profile product-background-lifecycle --no-build
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet --storage saf-internal --runs 1 \
  --profile product-external-intake --no-build
python3 clients/android/run_bootstrap.py \
  --target motox4 --storage saf-sdcard --runs 3 --profile success
```

Available targets are `avd`, `motox4`, `chromeos`, and the optional `pixel7a`.
Storage modes are `private`, `saf-internal`, and the Moto-only `saf-sdcard`.
The SAF success profile obtains an exact persisted grant through the system
picker, creates the Rust-generated direct-document plan, force-stops the
process, reopens every final document in a fresh process, verifies exact
length and SHA-1 in Rust, then removes the payload and part documents and
releases the grant.

The `product-dynamic-saf` profile exercises the real application service. It
grants a tree, adds a controlled loopback magnet, serves provider requests
through the four Kotlin workers, performs direct final-document payload I/O in
Rust, and verifies every non-padding file. It
then force-stops and restores the product, proves conservative verification
reconstruction, completes an explicit Force recheck, serves the exact torrent
back to pinned libtorrent through the Android listener, and removes exact
torrent files through the application command. It rejects info-hash output
directories, eager/empty part artifacts, staging survivors, and inexact
removal. It then re-adds the torrent with one skipped file and proves exact
selective direct storage, followed by an in-flight product Pause and exact
removal. The profile records the 40-handle native pool, 16-request
channel, and whole-process descriptor high water.

The `product-saf-grant-repair` profile exercises platform-root health rather
than payload transfer. It proves an initially healthy persisted grant, retains
the stable root identity after debug-only grant revocation, observes the root
as unavailable after process restart, repairs it through the system picker,
and observes it as healthy across another restart.

The `product-notifications` profile performs a genuine controlled SAF
download, inspects and taps its completion notification, and proves that
restart and Force recheck do not replay it. It then replaces one verified
payload file with the wrong object kind to drive the authoritative torrent
and storage states to `NEEDS_REPAIR`, inspects and taps the exact Storage-route
attention notification, restores the fixture bytes for safe removal, and
verifies zero retained automatic notifications. Notification tags are checked
as opaque and the profile owns its package, grant, peer, transport, payload,
and AVD cleanup.
API 35 evidence uses an explicitly created task-owned AVD whose name starts
with `rstorrent-`, selects `--avd-api 35`, and removes that AVD after the run.
The established API 34 path remains restricted to `jstorrent-tablet`.

The `product-incomplete-duplex` profile stores exactly two verified pieces
through a capped seed, revokes the SAF grant, force-stops and restarts the
product, observes unavailable storage, and repairs the same stable root. A
complementary pinned-libtorrent peer then exchanges Fast Piece frames with
Android in both directions before completion, including cross-file and
part-backed second blocks. The profile verifies exact wanted and oracle hashes,
absent skipped/padding documents, handle/provider/descriptor high waters, and
exact data, reverse-transport, application, and fresh-AVD cleanup.

The `product-hybrid-saf` profile adds a controlled aligned six-file hybrid to
the ordinary product service. It downloads an exact selected subset, promotes
another file, excludes and synthesizes BEP 47 padding, serves equal verified
payload through direct-v2 and legacy-upgraded libtorrent connections,
force-stops and restores complete content without peer payload or hash
requests, Force rechecks both integrity lanes, and removes the SAF namespace
exactly. It records storage-handle, pending-operation, and process-descriptor
high-water marks under the existing bounds.

The `product-external-intake` profile installs the androidTest fixture package
on a fresh API 34 AVD and exercises implicit cold/warm `magnet:` delivery plus
warm cross-package `content://` delivery under temporary grants. It covers
confirmation, start-disabled and start-enabled adds, exact duplicate
coalescing, AlreadyPresent, empty/oversized/invalid input, provider denial and
failure, explicit retry, cancellation, timeout, directory rejection, generic
MIME fallback, and an exact controlled transfer. A throttled unknown-length
64 MiB stream records source-buffer, Java/native/process RSS, descriptor, and
SAF-handle high waters. The profile scans product diagnostics and app-private
files for source leakage, proves settled descriptors and temporary-grant
revocation, and removes every app, provider, transport, grant, and payload
artifact.

The `product-unmetered-network` profile runs only on a fresh task-owned API 28
or API 35 AVD. It enables the persisted default-off cost policy, crosses a
real controlled SAF download from unmetered to emulator-marked metered Wi-Fi,
proves flat payload traffic and zero native transport endpoints after bounded
convergence, restarts the process while still blocked, and then resumes to
exact hashes when Wi-Fi becomes unmetered. A separately user-paused torrent
remains paused throughout recovery. The runner restores the emulator metered
override, preference, package data, reverse transport, payloads, and SAF
tree.

The `product-background-lifecycle` profile runs only on a fresh task-owned API
28 or API 35 AVD. It proves the lifecycle preferences default off, a genuine
partially verified transfer joins after Home without changing durable intent,
and foreground reopening preserves verified progress. It then enables
notification-backed background work, verifies foreground admission, crashes
and observes one closed-network sticky recovery, waits for completion-driven
shutdown, and performs a controlled upload from an opted-in background seed.
On API 35 it also removes the exact recent-task card while work is admitted,
then applies and exactly restores a one-second `dataSync` quota override,
requires the real timeout callback, and proves an exhausted restart is fenced.
Disabling keep-seeding joins the otherwise idle owner. The profile removes the
package owner, reverse, controlled payloads, SAF tree, and host fixture; it
uses no public swarm.

The `product-concurrent-downloads` profile also carries the installed seed-
admission matrix. After three controlled downloads complete, it applies an
active-seed limit of one through the real typed settings path and requires one
active plus two queued seeds with the exact default priority goals. It repeats
that view after default-off Home shutdown and reopen, then proves the same
queue under opted-in notification-backed background seeding and joined
shutdown when keep-seeding is disabled.

Every device command is addressed through the exact verified target
controller. The runner owns and removes its reverse port, controlled seed,
grant child, app-private run IDs, application, and fresh AVD session.

The `product-https-tracker` profile uses the same dynamic product storage path
but omits an explicit peer hint. It reaches a host-owned HTTPS tracker through
an owned reverse transport, accepts its deliberately untrusted wrong-host
certificate under the tracker-only unauthenticated TLS policy, consumes the
returned libtorrent seed, and verifies the direct files.

The `product-mse` profile selects an internal SAF tree, applies the live
`required` peer-obfuscation policy, and downloads from five controlled host
seeds forced to RC4. It verifies every direct file hash, observes all five
oracle connections as RC4, checks the session-wide four-job DH ceiling and
complete drain, and removes device and host artifacts. The deterministic Rust
owner test proves exact `4 active + 1 waiting` saturation; the device profile
asserts the scheduler-independent `1..=4` production high-water bound.

The `product-ipv6-policy` profile uses the ordinary product settings owner. It
checks the fresh default, applies disable, force-stops and restarts the
process, verifies disabled persistence, and re-enables IPv6. A device without
an eligible global-unicast address must report typed `Degraded` state with
effective IPv6 disabled while IPv4 remains usable; that is an expected
environment outcome rather than an application error.
