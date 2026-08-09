# Android Engine Bootstrap

This is the Tactical 004/005 integration harness. It packages the real
`rstorrent-engine` behind generated UniFFI Kotlin bindings and gives one
foreground service sole ownership of the native session. It contains the
current Compose product surface plus the original bounded diagnostic harness.
It supports app-private path-backed diagnostics and both legacy proof and
dynamic product storage through an explicitly granted Android Storage Access
Framework tree.

Build both locked Android ABIs and the debug APK:

```bash
source ~/.profile
experiments/android-engine-bootstrap/build.sh
```

The build uses Android Gradle Plugin `8.7.3`, Gradle `8.11.1`, Kotlin
`2.0.21`, JNA Android AAR `5.17.0`, UniFFI `0.31.0`, NDK
`27.0.12077973`, and API 28 Rust targets. It generates Kotlin from the host
native library and packages independently cross-built `x86_64` and
`arm64-v8a` libraries. Generated bindings, native libraries, reports, and APKs
remain under ignored build directories.

Device execution is owned by `run_bootstrap.py`. Do not install or start this
harness by selecting the first ADB device; the runner verifies an explicit
listed target before mutation.

The runner enters the pinned libtorrent environment automatically. Profiles
are explicit and repeatable:

```bash
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target avd --avd jstorrent-tablet --runs 3 --profile success
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target avd --profile slow-storage --profile cancellation \
  --profile peer-failure --profile duplicate-start \
  --profile activity-recreation --profile preexisting-artifacts
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target chromeos --storage saf-internal --runs 3 --profile success
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target avd --storage saf-internal --runs 3 \
  --profile product-dynamic-saf
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target pixel7a --storage saf-internal \
  --profile product-saf-grant-repair
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target avd --storage saf-internal \
  --profile product-https-tracker
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target pixel7a --runs 1 --profile product-mse
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target avd --runs 1 --profile product-ipv6-policy
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target motox4 --storage saf-sdcard --runs 3 --profile success
```

Available targets are `avd`, `motox4`, `chromeos`, and the optional `pixel7a`.
Storage modes are `private`, `saf-internal`, and the Moto-only `saf-sdcard`.
The SAF success profile obtains an exact persisted grant through the system
picker, creates the Rust-generated document plan, waits for native
`PREPARED`, publishes by provider rename, force-stops the process, reopens
every final document in a fresh process, verifies exact length and SHA-1 in
Rust, then removes the published and part documents and releases the grant.

The `product-dynamic-saf` profile exercises the real application service. It
grants a tree, adds a controlled loopback magnet, serves provider requests
through the four Kotlin workers, performs payload I/O in Rust, publishes by
name-only namespace acknowledgement, and verifies every non-padding file. It
rejects info-hash output directories, eager/empty part artifacts, and staging
survivors while recording the 40-handle native pool, 16-request channel, and
whole-process descriptor high water.

The `product-saf-grant-repair` profile exercises platform-root health rather
than payload transfer. It proves an initially healthy persisted grant, retains
the stable root identity after debug-only grant revocation, observes the root
as unavailable after process restart, repairs it through the system picker,
and observes it as healthy across another restart.

Every device command is addressed through the exact verified target
controller. The runner owns and removes its reverse port, controlled seed,
grant child, app-private run IDs, application, and fresh AVD session.

The `product-https-tracker` profile uses the same dynamic product storage path
but omits an explicit peer hint. It reaches a host-owned HTTPS tracker through
an owned reverse transport, accepts its deliberately untrusted wrong-host
certificate under the tracker-only unauthenticated TLS policy, consumes the
returned libtorrent seed, and verifies the published files.

The `product-mse` profile selects an internal SAF tree, applies the live
`required` peer-obfuscation policy, and downloads from five controlled host
seeds forced to RC4. It verifies every published file hash, observes all five
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
