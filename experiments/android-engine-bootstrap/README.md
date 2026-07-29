# Android Engine Bootstrap

This is the Tactical 004 integration harness. It packages the real
`rstorrent-engine` behind generated UniFFI Kotlin bindings and gives one
foreground service sole ownership of the native session. It is not a product
UI and uses only app-private path-backed storage.

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
```

Available targets are `avd`, `motox4`, and `chromeos`. Every device command is
addressed through the exact verified target controller. The runner owns and
removes its reverse port, controlled seed, app-private run IDs, application,
and fresh AVD session.
