# Tactical 003: Android Storage Feasibility Probe

Status: completed on the AVD, Chromebook ARCVM, and physical Pixel 7a.

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
and was added to the bounded matrix.

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
- Pixel 7a: API 37 and arm64-v8a; and
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

1. an app-private ordinary file as the platform baseline; and
2. a document created beneath a user-granted Downloads tree through the
   Storage Access Framework.

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

| Environment | Transport | Android | ABI |
| --- | --- | ---: | --- |
| `jstorrent-tablet` AVD | explicit local emulator serial | 34 | x86_64 |
| Chromebook ARCVM | `~/code/chromeos-testbed` ADB path | 33 | x86_64 |
| Pixel 7a | exact local USB serial | 37 | arm64-v8a |

The host runner must verify model and API before installing. A mismatch or
multiple ambiguous transports is an error.

All three targets use Android's external-storage Downloads document provider.
This does not claim compatibility with Google Drive, OEM cloud providers,
removable media, or other physical devices and Android versions.

## Contracts And Invariants

- The host runner always addresses an explicit verified serial.
- No probe is installed on an unlisted attached device.
- SAF access begins with a user-visible system grant and persists only the
  returned permission.
- Rust duplicates a borrowed descriptor before owning it.
- Java and Rust each close only the descriptors they own.
- Native I/O uses fixed 16 KiB buffers independently of logical file length.
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
- Pixel evidence is one physical-device result, not broad provider or device
  support.

## Non-Goals

- an Android product UI or background service
- integrating the torrent engine into the APK
- selecting a permanent FFI framework
- declaring the tactical `002` part-file format production-ready
- crash-consistent resume or power-loss testing
- measuring third-party cloud document providers
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
7. Compare capabilities and allocation behavior, remove artifacts, run
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
python3 scripts/references.py status
git diff --check
```

The host runner may provide a narrower build command when the Rust native
library must be built before Gradle packaging. Record the exact commands that
actually ran.

## Stopping Condition

This tactical is complete when three fresh runs on the `jstorrent-tablet` AVD,
Chromebook ARCVM, and physical Pixel 7a:

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
  bytes, memory snapshots, descriptor counts, provider metadata, capability
  outcomes, and cleanup state as JSON.
- A staging directory, sparse document, cancellation document, and
  materialization temporary document exercise create, reopen, directory
  rename, file rename, and recursive delete. Optional provider capabilities
  remain distinct from integrity failures.
- The host runner accepts only the exact `jstorrent-tablet` AVD, Chromebook
  ARCVM identity, or Pixel 7a serial and identity. It starts a wiped headless
  AVD itself, uses the ChromeOS testbed for hardware health and APK transport,
  never selects the first attached device, and rejects unexpected API, model,
  device, or ABI values.
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
- `bdfbd08` added dual-ABI packaging and the physical Pixel target.

### Environment And Capability Evidence

The final evidence matrix was:

| Target | Identity | Provider | Result |
| --- | --- | --- | --- |
| `jstorrent-tablet` | API 34, `emu64xa`, x86_64 | `com.android.externalstorage.documents` | 3/3 pass |
| Chromebook ARCVM | API 33, model `nami`, device `nami_cheets`, x86_64 | `com.android.externalstorage.documents` | 3/3 pass |
| Pixel 7a | API 37, device `lynx`, arm64-v8a | `com.android.externalstorage.documents` | 3/3 pass |

The AVD and ARCVM executed the packaged x86_64 Rust library; the Pixel
executed arm64-v8a. The Chromebook fingerprint was
`google/nami/nami_cheets:13/R150-16700.46.0/15802699:user/release-keys`;
the AVD fingerprint was
`google/sdk_gphone64_x86_64/emu64xa:14/UE1A.230829.050/12077443:userdebug/dev-keys`;
and the Pixel fingerprint was
`google/lynx/lynx:17/CP2A.260705.006/15641320:user/release-keys`.

The AVD and ARCVM runs observed provider flags `16716`; the Pixel runs
observed `82252`. All nine runs classified directory rename,
materialization-file rename, and probe-tree deletion as supported. Every
persisted permission count was one before process death and zero after
verification and release. The Downloads root itself could not be granted
because Android's picker disables that privacy-sensitive root; selecting a
dedicated child worked on all three targets.

### Sparse And Integrity Evidence

Every app-private and SAF run:

- truncated to exactly `268,451,840` bytes, which is 256 MiB plus 16 KiB;
- wrote and read 16 KiB markers at offsets zero and `268,435,456`;
- returned a zero native error mask before close, after reopen, through a
  Rust-owned duplicate after Java closed its descriptor, and after process
  force-stop and relaunch;
- left the Java caller's descriptor usable after each borrowed native call;
  and
- reported the same allocation for app-private and SAF files on each target:
  `36,864` bytes from an initial `4,096` on the AVD and ARCVM, and `40,960`
  bytes from an initial zero on the Pixel.

The Android external-storage provider therefore preserved a sparse hole at
the 256 MiB slot offset on all three tested environments. The current tactical
`002` compact-slot geometry is feasible for this provider and these devices;
the experiment does not make sparse allocation a cross-provider contract.

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

Post-restart SAF verification took 0.918, 59.654, and 12.051 milliseconds on
the AVD; 2.117, 8.971, and 13.397 milliseconds on ARCVM; and 1.906, 2.364,
and 2.979 milliseconds on the Pixel. These are diagnostic observations under
variable system load, not storage benchmarks or latency budgets.

### Bounds, Cancellation, And Resource Evidence

Rust used a 16 KiB marker, verification, materialization, and cancellable
write buffer regardless of the 256 MiB logical length. The cancellable worker
published atomic progress before the application requested cancellation. The
three AVD runs stopped after 131,072, 262,144, and 606,208 bytes; the three
ARCVM runs stopped after 409,600, 425,984, and 425,984 bytes; and all three
Pixel runs stopped after 458,752 bytes. All were below the 64 MiB maximum and
terminated with an observed join.

Initial-phase total PSS snapshots changed:

- on the AVD, from 13,053--19,128 KiB before work to
  20,148--23,693 KiB afterward; and
- on ARCVM, from 21,050--21,114 KiB before work to
  22,951--23,156 KiB afterward; and
- on the Pixel, from 35,704--52,325 KiB before work to
  52,898--53,868 KiB afterward.

Post-restart completion snapshots were 24,603--25,592 KiB on the AVD and
22,610--22,681 KiB on ARCVM and 46,087--46,207 KiB on the Pixel. Native heap,
Java heap, and descriptor snapshots were also emitted per phase. Framework
and provider activity makes those process-level deltas noisy, but neither
logical file length nor sparse offset caused a corresponding resident
allocation. This remains a component observation, not an absolute future
engine RSS guarantee.

Per-backend descriptor assertions passed in every run. The SAF backend ended
with the same descriptor count it began with on ARCVM and the Pixel and with a
zero- or one-descriptor delta on the AVD; broader activity-level counts also
include Android UI and provider descriptors. Successful cleanup then
terminated the process, deleted the probe tree and private file, released the
persisted permission, removed the exact empty grant directory, cleared
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
  and
- ChromeOS reports model `nami` and Android device `nami_cheets`, so both
  exact properties are checked before installation.

Failure recovery attempts app-owned document deletion and permission release,
then removes the exact grant directory only if it is empty. It will not
recursively delete or overwrite unrelated Downloads content.

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
package or grant directory on the Pixel or ARCVM and no running AVD. It
removed generated Gradle, APK, JNI, Rust target, Python bytecode, raw result,
log, and screenshot artifacts.

The testbed skill's required `chromeos doctor` passed before hardware work.
ARCVM ADB authorization initially required accepting the visible Android
debugging prompt on the Chromebook; later runner invocations repeated doctor
and `adb-connect`. The local USB-connected Quest was never addressed; local
ADB operations used only the exact AVD or Pixel serial.

### Deliberate Limits And Next Boundary

The stopping condition is satisfied for the named AVD, Chromebook ARCVM, and
physical Pixel 7a. The evidence supports retaining the tactical `002` sparse
compact-slot direction for the Android external-storage provider while keeping
provider capabilities explicit and native buffers independent of piece size.

It does not establish power-loss durability, directory-entry syncing, a
permanent resume format, removable-media behavior, cloud-provider behavior,
OEM document-provider behavior, or an exact product process-memory ceiling.
It also does not test torrent networking, Android foreground-service
lifecycle, or the actual engine inside an Android application.

The physical-device gate is satisfied for one Pixel 7a running Google's API 37
external-storage provider, not for Android devices or providers generally. A
subsequent Android engine integration tactical should preserve the proven
seams: user-granted child trees, synchronous restart metadata, duplicated
borrowed descriptors, fixed buffers, observable cancellation and termination,
explicit provider capabilities, and exact cleanup.
