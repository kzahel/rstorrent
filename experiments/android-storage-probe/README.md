# Android Storage Probe

This is the tactical `003` experiment, not an Android product client. It
compares the same fixed-buffer Rust file-descriptor operations through an
app-private file and a user-granted child directory under Downloads.

The native probe duplicates every borrowed descriptor before owning it. It
truncates to 256 MiB plus 16 KiB, writes 16 KiB markers at offset zero and
256 MiB, flushes, reads them back, and reports logical and allocated bytes.
The application also tests reopen, process relaunch, persisted SAF access,
descriptor lifetime, cancellable termination, staging-directory rename,
materialization rename, descriptor filesystem type and block size, memory
snapshots, and cleanup.

## Build

The configured development machine uses Android platform 35, NDK
`27.0.12077973`, Gradle `8.11.1`, and the x86_64 and aarch64 Android Rust
targets:

```bash
source ~/.profile
rustup target add x86_64-linux-android aarch64-linux-android
experiments/android-storage-probe/build_probe.sh
```

The generated APK and native build output are ignored. The Gradle wrapper and
both lockfiles are tracked.

## Run

The runner always selects and verifies an explicit target. It will not install
on another attached Android device.

```bash
python3 experiments/android-storage-probe/run_probe.py \
  --target avd --avd jstorrent-tablet --runs 3

python3 experiments/android-storage-probe/run_probe.py \
  --target chromeos --runs 3

python3 experiments/android-storage-probe/run_probe.py \
  --target pixel7a --runs 3

python3 experiments/android-storage-probe/run_probe.py \
  --target motox4 --storage internal --runs 3

python3 experiments/android-storage-probe/run_probe.py \
  --target motox4 --storage sdcard --runs 3
```

Each fresh run clears application data, creates an exact empty
`RSTorrentStorageProbeGrant` directory under Downloads, opens the system tree
picker to grant that directory, executes the initial probe, force-stops the
process, relaunches for persisted-URI verification, deletes the probe tree,
releases the grant, removes the empty grant directory, and clears application
data again. Pre-creation avoids host keyboard-layout translation during UI
automation. Android prevents granting the Downloads root itself. The AVD
process is started and stopped by the runner. ChromeOS health, ARCVM
authorization, and APK transport use `~/code/machine-control`.

The Moto X4 removable profile selects the exact mounted `F69D-D340` volume
and verifies the returned document ID before writing. It creates its dedicated
grant directory at the SD-card root because Android 9 permits that selection;
the internal profile continues to use a child under Downloads.

Results are emitted as one JSON object per run followed by a summary. A
provider operation is reported as `supported`, `unsupported`, or `failed`;
only unsupported optional rename behavior is non-fatal.
