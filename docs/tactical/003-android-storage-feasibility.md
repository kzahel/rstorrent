# Tactical 003: Android Storage Feasibility Probe

Status: completed on the AVD, Chromebook ARCVM, physical Pixel 7a, and
physical Moto X4 internal and removable exFAT storage.

## Motivation And Outcome

Tactical `002` proved selective multi-file storage on a normal desktop
filesystem. Its versioned part file uses compact piece slots whose logical
offsets are separated by the torrent piece length. With the accepted maximum
256 MiB piece length, two occupied slots place the second slot at an offset
slightly above 256 MiB even when only a few 16 KiB ranges contain data.

That layout is intentionally provisional. Android product clients will
normally receive Storage Access Framework documents rather than ordinary
paths. Before a desktop filesystem assumption becomes a durable storage or
resume contract, measure whether Android document-provider file descriptors
support the operations and costs the engine needs.

Build one independently authored Android probe whose hot-path operations run
in Rust against a caller-owned raw file descriptor. Run the same assertions on
the `jstorrent-tablet` AVD, the physical Chromebook's ARCVM, and an ordinary
physical Android device when one becomes available. Record capabilities and
allocation behavior instead of assuming that a successful write implies
sparse or crash-safe semantics.

This is a feasibility experiment, not an Android product client. The initial
plan treated an ordinary physical Android phone or tablet as a later
validation gate because ARCVM, the AVD, OEM filesystems, and third-party
document providers may differ. A Pixel 7a became available during execution
and was added to the bounded matrix. A Moto X4 running Android 9 with a
removable exFAT SD card later became available, so the same checks were
extended to both its internal shared storage and exact removable volume.

## Dependencies And Environment

- [Tactical 002 execution record](002-selective-multi-file-storage.md)
- [Product and engine direction](../topics/product-direction.md)
- [Engineering principles](../engineering-principles.md)
- The repository ChromeOS instructions in [`../../AGENTS.md`](../../AGENTS.md)
- The authoritative testbed skill at
  `~/code/chromeos-testbed/skills/SKILL.md`

Observed starting environment on 2026-07-29:

- Android SDK platforms 34, 35, and 36;
- Android build tools 34.0.0, 35.0.0, and 36.1.0;
- Android NDK `27.0.12077973`;
- cached Android Gradle Plugin `8.7.3` and Gradle `8.11.1`;
- Rust `1.97.0` and cargo-ndk `4.1.2`;
- `jstorrent-tablet`: API 34, x86_64, 2 GiB RAM, 6 GiB data partition;
- Chromebook ARCVM: API 33, x86_64 and arm64-v8a ABIs;
- Pixel 7a: API 37 and arm64-v8a;
- Moto X4: API 28, arm64-v8a, with mounted removable volume `F69D-D340`
  backed by exFAT with a 128 KiB block size; and
- ChromeOS testbed doctor: nine checks passed, no failures.

The local USB-connected Quest device is not part of this tactical. Do not
install or run the probe on an unlisted target.

## Scope

### Reproducible probe

Add a self-contained project under `experiments/android-storage-probe/`:

- a minimal debuggable Android application;
- a small Rust `cdylib` with one auditable JNI boundary;
- a host runner that addresses an explicit ADB target and never chooses the
  first attached device implicitly; and
- maintainer instructions for building, deploying, granting a document tree,
  collecting structured results, and cleaning up.

The probe must not depend on JSTorrent source, its Android modules, or its
process topology. Existing local Gradle and Android caches are toolchain
inputs, not product dependencies.

### Backends

Run equivalent core file-descriptor checks against:

1. an app-private ordinary file as the platform baseline;
2. a document created beneath a user-granted Downloads tree through the
   Storage Access Framework; and
3. on the Moto X4, a document created beneath a user-granted child at the
   root of the exact removable SD volume.

Acquire the SAF tree through `ACTION_OPEN_DOCUMENT_TREE`, persist the URI
permission, and record provider authority and document capability flags.
Never manufacture a content URI or assume access without a returned grant.

### Rust file-descriptor operations

While the Java `ParcelFileDescriptor` remains open, pass its integer file
descriptor to Rust. Rust must duplicate it before taking ownership and must
never close the caller's descriptor.

Through the duplicated descriptor:

- truncate to a logical length of 256 MiB plus one 16 KiB block;
- write deterministic 16 KiB markers at offset zero and at 256 MiB;
- call `fsync`;
- read both markers back and validate them;
- report logical length, `st_blocks`-derived allocated bytes when available,
  and operation timings; and
- close every Rust-owned duplicate on all paths.

The probe records whether logical holes remain physically sparse. Sparse
allocation is an observation, not a pass condition. A provider that allocates
the full logical length may still be usable with a different part-file
layout.

### Reopen and persisted access

Close the Java descriptor and reopen the document by URI. Verify the logical
length and both markers again through Rust. Then force-stop and relaunch the
application without clearing its data, prove that the persisted tree grant
survives the process boundary, reopen the document, and repeat verification.

### Descriptor ownership and cancellation

Prove both sides of the ownership seam:

- a Rust-owned duplicate remains usable after Java closes its
  `ParcelFileDescriptor`; and
- the caller-owned descriptor is not closed by the completed Rust call.

Run a cancellable native writer that owns its duplicate, writes only fixed
16 KiB chunks, observes a cancellation flag, terminates observably, reports
the exact bytes written, and closes the duplicate. The test must not depend on
process death as its cancellation mechanism.

### Publication and materialization operations

Within the granted tree:

- create a hidden-style staging directory;
- create and verify a staged document;
- attempt to rename the complete directory to its published name;
- create a materialization temporary document inside the published directory;
- write, flush, close, reopen, and verify it;
- rename it to its final materialized name; and
- delete the probe tree at cleanup.

Record rename, delete, and directory support separately as `supported`,
`unsupported`, or `failed`. Unsupported optional provider operations must not
be silently treated as success. Data corruption, descriptor misuse, leaked
temporary documents, or an unexpected provider error fails the run.

### Resource evidence

Record for each backend and environment:

- Android API, build fingerprint, model, ABI, and provider authority;
- descriptor filesystem type and block size, plus the host-visible removable
  volume mount type;
- logical and allocated bytes before and after sparse writes;
- truncate, write, sync, read, reopen, and cancellation timings;
- application Java heap, native heap, and total proportional-set-size
  snapshots before and at the probe high-water;
- fixed Rust buffer and write-chunk sizes;
- descriptor counts before and after cleanup; and
- final cleanup state.

These measurements characterize this probe only. They are not exact engine
RSS or storage guarantees.

## Environment Matrix

Run three fresh application-data cycles on each target:

| Environment | Destination | Transport | Android | ABI |
| --- | --- | --- | ---: | --- |
| `jstorrent-tablet` AVD | internal shared | explicit emulator serial | 34 | x86_64 |
| Chromebook ARCVM | internal shared | testbed ADB path | 33 | x86_64 |
| Pixel 7a | internal shared | exact USB serial | 37 | arm64-v8a |
| Moto X4 | internal shared | exact USB serial | 28 | arm64-v8a |
| Moto X4 | removable exFAT | exact USB serial | 28 | arm64-v8a |

The host runner must verify model and API before installing. A mismatch or
multiple ambiguous transports is an error.

The first three targets use Android's external-storage Downloads document
provider. The Moto internal profile uses the Android 9 Downloads provider,
while its removable profile uses the external-storage provider with the exact
volume-scoped document ID. This does not claim compatibility with Google
Drive, OEM cloud providers, other SD cards, filesystems, devices, or Android
versions.

## Contracts And Invariants

- The host runner always addresses an explicit verified serial.
- No probe is installed on an unlisted attached device.
- SAF access begins with a user-visible system grant and persists only the
  returned permission.
- Rust duplicates a borrowed descriptor before owning it.
- Java and Rust each close only the descriptors they own.
- Native I/O uses fixed 16 KiB buffers independently of logical file length.
- Returned Moto document IDs must name the requested internal path or exact
  removable volume before any result is accepted.
- All offsets and lengths use checked 64-bit arithmetic.
- Every successful write is read back after `fsync` and again after reopen.
- Process relaunch verification uses the persisted URI, not an in-memory
  descriptor.
- Cancellation has a requested state and an observed terminated state.
- Capability absence is distinguished from data-integrity failure.
- The probe deletes its own documents and application state but never broad
  Downloads content.
- ChromeOS transport, prompts, screenshots, and recovery remain owned by the
  testbed repository; RSTorrent owns the APK, assertions, and results.
- Physical Pixel and Moto evidence is not broad provider, filesystem, or
  device support.

## Non-Goals

- an Android product UI or background service
- integrating the torrent engine into the APK
- selecting a permanent FFI framework
- declaring the tactical `002` part-file format production-ready
- crash-consistent resume or power-loss testing
- measuring third-party cloud document providers
- claiming removable-media behavior beyond the named Moto X4 card and exFAT
  volume
- exact device-wide memory or disk guarantees
- tracker, peer, torrent, or network testing
- modifying the JSTorrent Android application
- installing on the connected Quest device
- claiming broad physical Android or document-provider compatibility

## Implementation Sequence

1. Record this tactical and the exact environment matrix.
2. Build the minimal Android and Rust probe with deterministic host-side
   orchestration.
3. Validate native operations and result parsing locally.
4. Run three fresh cycles on `jstorrent-tablet`.
5. Run three fresh cycles on Chromebook ARCVM through the testbed.
6. Add and run three fresh cycles on the physical Pixel 7a made available
   during execution.
7. Add and run three fresh cycles on both the internal and exact removable
   Moto X4 destinations made available during execution.
8. Compare capabilities and allocation behavior, remove artifacts, run
   repository validation, and record exact evidence and limitations.

## Validation

Run and record:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
experiments/android-storage-probe/gradlew \
  -p experiments/android-storage-probe clean assembleDebug
python3 -m py_compile \
  experiments/android-storage-probe/run_probe.py
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
python3 scripts/references.py status
git diff --check
```

The host runner may provide a narrower build command when the Rust native
library must be built before Gradle packaging. Record the exact commands that
actually ran.

## Stopping Condition

This tactical is complete when three fresh runs on the `jstorrent-tablet` AVD,
Chromebook ARCVM, physical Pixel 7a, Moto X4 internal storage, and Moto X4
removable exFAT storage:

- perform the 256 MiB sparse-offset operation through Rust for app-private
  and SAF-backed descriptors;
- verify bytes before close, after reopen, and after process relaunch;
- prove duplicated-descriptor ownership and cancellable task termination;
- exercise and classify staging-directory and materialization renames;
- report allocation, timing, memory, and descriptor evidence;
- leave no probe document, APK data, emulator process, build-only temporary
  file, or testbed capture behind; and
- record which conclusions transfer to tactical `002` and which remain
  provider- or device-specific.

## Execution Record

Completed on 2026-07-29.

### What Landed

- `experiments/android-storage-probe/` is a self-contained Android project
  with a minimal Java activity, a Rust `cdylib`, a Gradle wrapper, and an
  explicit-target host runner. Generated APK, JNI, Gradle, native target, and
  Python cache outputs are ignored.
- Java obtains and persists an `ACTION_OPEN_DOCUMENT_TREE` grant for one exact
  probe-owned child under Downloads. The app creates all test documents
  beneath that grant and releases it after restart verification.
- Rust accepts borrowed integer file descriptors, duplicates them before
  ownership, and uses fixed 16 KiB arrays for truncate, positional write,
  `fsync`, positional read, materialization, and cancellable writes.
- The probe records separate truncate, marker-write, sync, immediate-read,
  reopen, and post-restart read timings. It also records logical and allocated
  bytes, descriptor filesystem type and block size, memory snapshots,
  descriptor counts, provider metadata, capability outcomes, and cleanup state
  as JSON.
- A staging directory, sparse document, cancellation document, and
  materialization temporary document exercise create, reopen, directory
  rename, file rename, and recursive delete. Optional provider capabilities
  remain distinct from integrity failures.
- The host runner accepts only the exact `jstorrent-tablet` AVD, Chromebook
  ARCVM identity, Pixel 7a, or Moto X4 serial and identity. It starts a wiped
  headless AVD itself, uses the ChromeOS testbed for hardware health and APK
  transport, never selects the first attached device, and rejects unexpected
  API, model, device, ABI, removable-volume, or mount values.
- Every cycle uses fresh application data, an exact empty grant directory,
  process force-stop and relaunch, persisted-URI verification, permission
  release, content deletion, application-data clearing, and APK uninstall.

The work landed in bounded commits:

- `8592a4e` planned the Android storage feasibility probe;
- `f332763` added the Android, Rust, and host-runner experiment;
- `316c0d0` hardened the process-restart handoff;
- `8d1f660` made ChromeOS identity and grant targeting deterministic;
- `45d9018` added per-operation timing and observable cancellation progress;
- `37e9155` removed a cold-AVD synchronous activity-launch race; and
- `bdfbd08` added dual-ABI packaging and the physical Pixel target;
- `9f09e0d` recorded the first three-target execution matrix; and
- `ff9852d` added Android 9 removable-storage probing and filesystem identity.

### Environment And Capability Evidence

The final evidence matrix was:

| Target | Identity | Provider | Result |
| --- | --- | --- | --- |
| `jstorrent-tablet` | API 34, `emu64xa`, x86_64 | `com.android.externalstorage.documents` | 3/3 pass |
| Chromebook ARCVM | API 33, model `nami`, device `nami_cheets`, x86_64 | `com.android.externalstorage.documents` | 3/3 pass |
| Pixel 7a | API 37, device `lynx`, arm64-v8a | `com.android.externalstorage.documents` | 3/3 pass |
| Moto X4 internal | API 28, `payton_sprout`, arm64-v8a | `com.android.providers.downloads.documents` | 3/3 pass |
| Moto X4 removable exFAT | API 28, `payton_sprout`, arm64-v8a | `com.android.externalstorage.documents` | 3/3 pass |

The AVD and ARCVM executed the packaged x86_64 Rust library; the Pixel and
Moto executed arm64-v8a. The Chromebook fingerprint was
`google/nami/nami_cheets:13/R150-16700.46.0/15802699:user/release-keys`;
the AVD fingerprint was
`google/sdk_gphone64_x86_64/emu64xa:14/UE1A.230829.050/12077443:userdebug/dev-keys`;
and the Pixel fingerprint was
`google/lynx/lynx:17/CP2A.260705.006/15641320:user/release-keys`.
The Moto fingerprint was
`motorola/payton_fi/payton_sprout:9/PPWS29.69-39-6-13/badd5:user/release-keys`.

The AVD and ARCVM runs observed provider flags `16716`; the Pixel runs
observed `82252`; both Moto profiles observed `332`. All fifteen runs
classified directory rename,
materialization-file rename, and probe-tree deletion as supported. Every
persisted permission count was one before process death and zero after
verification and release. The Downloads root itself could not be granted
because Android's picker disables that privacy-sensitive root; selecting a
dedicated child worked on all targets. Android 9 also permitted selecting the
dedicated child directly beneath the removable volume root.

### Sparse And Integrity Evidence

Every app-private and SAF run:

- truncated to exactly `268,451,840` bytes, which is 256 MiB plus 16 KiB;
- wrote and read 16 KiB markers at offsets zero and `268,435,456`;
- returned a zero native error mask before close, after reopen, through a
  Rust-owned duplicate after Java closed its descriptor, and after process
  force-stop and relaunch;
- left the Java caller's descriptor usable after each borrowed native call;
  and
- preserved sparse allocation in every app-private file and every internal
  shared-storage SAF file; and
- allocated `268,566,528` bytes from an initial zero in every removable-exFAT
  SAF run for the `268,451,840`-byte logical file.

Internal storage allocated `36,864` bytes from an initial `4,096` on the AVD,
ARCVM, and both Moto profiles' app-private baselines; the Pixel allocated
`40,960` from zero. The Moto internal SAF document also allocated `36,864`
from `4,096`. Its app-private descriptor reported ext4 magic `0xef53` and a
4 KiB block size; internal SAF reported sdcardfs magic `0x5dca2df5` and 4 KiB;
removable SAF reported exFAT magic `0x2011bab0` and 128 KiB, agreeing with the
host-visible exFAT mount.

The Android internal-storage providers preserved the sparse hole, but the
Moto's removable exFAT volume eagerly materialized essentially the full
logical range. Tactical `002`'s sparse compact-slot geometry is therefore not
a portable storage contract. Production storage must choose a dense or
otherwise non-sparse representation when the destination cannot preserve
holes, and must not infer that capability merely from successful random I/O.

Final SAF stage timings in milliseconds were:

| Target/run | Truncate | Write | Sync | Read | Total | Reopen |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| AVD 1 | 5.914 | 33.966 | 24.408 | 0.155 | 64.569 | 2.455 |
| AVD 2 | 0.221 | 37.793 | 1.959 | 1.812 | 41.885 | 1.280 |
| AVD 3 | 5.357 | 6.379 | 9.176 | 1.777 | 22.830 | 2.428 |
| ARCVM 1 | 0.389 | 0.407 | 2.578 | 0.280 | 3.684 | 1.191 |
| ARCVM 2 | 0.282 | 0.268 | 2.066 | 0.318 | 2.986 | 1.025 |
| ARCVM 3 | 0.259 | 0.276 | 2.308 | 0.183 | 3.042 | 1.167 |
| Pixel 1 | 0.708 | 0.137 | 0.694 | 0.136 | 1.685 | 0.825 |
| Pixel 2 | 0.231 | 0.166 | 0.471 | 0.225 | 1.105 | 0.863 |
| Pixel 3 | 0.219 | 0.140 | 1.427 | 0.200 | 1.993 | 1.863 |
| Moto internal 1 | 0.061 | 0.169 | 4.942 | 0.128 | 5.489 | 1.413 |
| Moto internal 2 | 0.075 | 0.173 | 4.537 | 0.127 | 5.198 | 1.669 |
| Moto internal 3 | 0.128 | 0.159 | 3.351 | 0.121 | 3.935 | 1.689 |
| Moto exFAT 1 | 8,900.963 | 0.222 | 2,070.638 | 0.260 | 10,972.737 | 6.314 |
| Moto exFAT 2 | 5,821.936 | 0.211 | 5,043.698 | 0.304 | 10,866.703 | 13.805 |
| Moto exFAT 3 | 5,291.646 | 0.179 | 3,302.314 | 0.110 | 8,594.559 | 3.477 |

Post-restart SAF verification took 0.918, 59.654, and 12.051 milliseconds on
the AVD; 2.117, 8.971, and 13.397 milliseconds on ARCVM; and 1.906, 2.364,
and 2.979 milliseconds on the Pixel. Moto internal verification took 2.255,
1.703, and 1.912 milliseconds; Moto removable verification took 1.609, 1.731,
and 1.833 milliseconds. These are diagnostic observations under variable
system load, not storage benchmarks or latency budgets. The repeated
multi-second exFAT truncate and sync behavior is nevertheless a material
feasibility result, not a one-run outlier.

### Bounds, Cancellation, And Resource Evidence

Rust used a 16 KiB marker, verification, materialization, and cancellable
write buffer regardless of the 256 MiB logical length. The cancellable worker
published atomic progress before the application requested cancellation. The
three AVD runs stopped after 131,072, 262,144, and 606,208 bytes; the three
ARCVM runs stopped after 409,600, 425,984, and 425,984 bytes; and all three
Pixel runs stopped after 458,752 bytes. Both Moto profiles stopped after
409,600, 409,600, and 425,984 bytes. All were below the 64 MiB maximum and
terminated with an observed join.

Initial-phase total PSS snapshots changed:

- on the AVD, from 13,053--19,128 KiB before work to
  20,148--23,693 KiB afterward; and
- on ARCVM, from 21,050--21,114 KiB before work to
  22,951--23,156 KiB afterward; and
- on the Pixel, from 35,704--52,325 KiB before work to
  52,898--53,868 KiB afterward;
- on Moto internal storage, from 13,513--13,601 KiB to
  23,471--23,630 KiB; and
- on Moto removable storage, from 13,432--13,478 KiB to
  23,486--23,636 KiB.

Post-restart completion snapshots were 24,603--25,592 KiB on the AVD and
22,610--22,681 KiB on ARCVM, 46,087--46,207 KiB on the Pixel,
23,141--23,178 KiB on Moto internal storage, and 17,302--17,312 KiB on Moto
removable storage. Native heap, Java heap, and descriptor snapshots were also
emitted per phase. Framework and provider activity makes those process-level
deltas noisy, but even full exFAT disk allocation did not produce a
corresponding resident allocation. This remains a component observation, not
an absolute future engine RSS guarantee.

Per-backend descriptor assertions passed in every run; broader activity-level
counts also include Android UI and provider descriptors. Successful cleanup
then terminated the process, deleted the probe tree and private file, released
the persisted permission, removed the exact empty grant directory, cleared
application data, and uninstalled the APK.

### Edge And Failure Evidence

Exploratory runs were discarded rather than counted in the final matrix. They
found and corrected several relevant Android edges:

- asynchronous `SharedPreferences.apply()` could race an immediate host
  force-stop, so restart-critical URI state now uses a checked synchronous
  commit;
- a retained document-picker task could obscure the restart activity, so
  each phase clears its task before launch;
- a cancellable worker could receive cancellation before cold-start
  scheduling gave it its first timeslice, so the app now waits up to two
  seconds for observable first-block progress;
- `am start -W` could exceed 30 seconds while a wiped AVD initialized
  background services, so launch is asynchronous and bounded semantic result
  waits own completion;
- Chromebook keyboard translation mangled text injected into the picker, so
  the runner creates one exact empty directory and selects it semantically;
- ChromeOS reports model `nami` and Android device `nami_cheets`, so both
  exact properties are checked before installation;
- Android 9's UI automation cannot dump into `/data/local/tmp`, so the runner
  uses one immediately deleted shared-storage XML file;
- Android 9 uses a `SELECT` action without a second confirmation and may
  restore a stale picker location despite an initial URI, so the runner first
  opens roots, selects the intended destination, enters the exact child, and
  only then selects it;
- the removable root is displayed by its user label rather than volume ID, so
  the runner separately requires mounted volume `F69D-D340`, verifies its
  exFAT mount, and rejects any returned document ID outside that volume; and
- the Android 9 external-storage provider threw a null-pointer exception when
  asked to enumerate a newly granted empty SD directory before creating its
  first child. The host already proves that its exact grant directory is empty
  and refuses to delete a nonempty directory, so the redundant provider-side
  pre-clean enumeration was removed.

Failure recovery attempts app-owned document deletion and permission release,
then removes the exact grant directory only if it is empty. It will not
recursively delete or overwrite unrelated internal or removable content.

### Validation And Audits

The required final device commands passed:

```bash
source ~/.profile
python3 experiments/android-storage-probe/run_probe.py \
  --target avd --avd jstorrent-tablet --runs 3 --no-build
python3 experiments/android-storage-probe/run_probe.py \
  --target chromeos --runs 3 --no-build
python3 experiments/android-storage-probe/run_probe.py \
  --target pixel7a --runs 3 --no-build
python3 experiments/android-storage-probe/run_probe.py \
  --target motox4 --storage internal --runs 3 --no-build
python3 experiments/android-storage-probe/run_probe.py \
  --target motox4 --storage sdcard --runs 3 --no-build
```

The APK used by those runs came from:

```bash
source ~/.profile
experiments/android-storage-probe/build_probe.sh
```

That script built x86_64 and arm64-v8a Rust release libraries with cargo-ndk
platform 28 and ran a clean Gradle debug APK build against Android platform 35
and NDK `27.0.12077973`. Native unit tests and strict clippy passed before the
build. These final validation commands also passed:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo fmt \
  --manifest-path experiments/android-storage-probe/native/Cargo.toml \
  --all -- --check
cargo clippy \
  --manifest-path experiments/android-storage-probe/native/Cargo.toml \
  --all-targets -- -D warnings
cargo test \
  --manifest-path experiments/android-storage-probe/native/Cargo.toml
python3 -m py_compile \
  experiments/android-storage-probe/run_probe.py
python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

The workspace suite passed 17 engine tests, two diagnostic argument tests,
31 protocol tests, one architecture test, and all doc tests. The native probe
unit test passed separately. Reference status was clean at:

- BitTorrent BEPs `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`;
- rqbit `4e5f94cbcf1d57ec500885c77cf1e24d70232d89`;
- libtorrent `7d7fc38fac61177fa5e02148f791b2f65250b09d`; and
- JSTorrent `main@0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`.

The workspace dependency tree was unchanged. The final audit found no probe
package or grant directory on the Pixel, Moto X4, or ARCVM and no running AVD.
It removed generated Gradle, APK, JNI, Rust target, Python bytecode, raw
result, log, and screenshot artifacts.

The testbed skill's required `chromeos doctor` passed before hardware work.
ARCVM ADB authorization initially required accepting the visible Android
debugging prompt on the Chromebook; later runner invocations repeated doctor
and `adb-connect`. The local USB-connected Quest was never addressed; local
ADB operations used only the exact AVD, Pixel, or Moto X4 serial.

### Deliberate Limits And Next Boundary

The stopping condition is satisfied for the named AVD, Chromebook ARCVM,
physical Pixel 7a, and both Moto X4 destinations. The evidence supports
retaining tactical `002`'s sparse compact-slot direction only behind an
observed storage capability. It must not be used unchanged on the tested
removable exFAT destination.

It does not establish power-loss durability, directory-entry syncing, a
permanent resume format, other removable filesystems, cloud-provider behavior,
other OEM behavior, or an exact product process-memory ceiling. It also does
not test torrent networking, Android foreground-service lifecycle, or the
actual engine inside an Android application.

The physical-device gate now covers one modern Google phone and one Android 9
Motorola phone with a specific removable exFAT card, not Android devices or
providers generally. Before Android engine integration adopts tactical
`002`'s provisional part file, a bounded storage-layout tactical should define
and prove a dense or extent-based fallback for destinations that do not
preserve sparse holes. Later integration should preserve the other proven
seams: user-granted child trees, synchronous restart metadata, duplicated
borrowed descriptors, fixed buffers, observable cancellation and termination,
explicit provider capabilities, and exact cleanup.
