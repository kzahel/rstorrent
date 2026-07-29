# Tactical 003: Android Storage Feasibility Probe

Status: ready; implementation has not started.

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
the `jstorrent-tablet` AVD and the physical Chromebook's ARCVM. Record
capabilities and allocation behavior instead of assuming that a successful
write implies sparse or crash-safe semantics.

This is a feasibility experiment, not an Android product client. A later run
on an ordinary physical Android phone or tablet remains an explicit validation
gate because ARCVM, the AVD, OEM filesystems, and third-party document
providers may differ.

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
- Chromebook ARCVM: API 33, x86_64 and arm64-v8a ABIs; and
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

The host runner must verify model and API before installing. A mismatch or
multiple ambiguous transports is an error.

The AVD and ARCVM use Android's Downloads document provider. This does not
claim compatibility with Google Drive, OEM cloud providers, removable media,
or an ordinary physical Android device.

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
- ARCVM and AVD evidence is not presented as physical Android-device support.

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
- claiming ordinary physical Android phone or tablet validation

## Implementation Sequence

1. Record this tactical and the exact environment matrix.
2. Build the minimal Android and Rust probe with deterministic host-side
   orchestration.
3. Validate native operations and result parsing locally.
4. Run three fresh cycles on `jstorrent-tablet`.
5. Run three fresh cycles on Chromebook ARCVM through the testbed.
6. Compare capabilities and allocation behavior, remove artifacts, run
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
python3 scripts/references.py status
git diff --check
```

The host runner may provide a narrower build command when the Rust native
library must be built before Gradle packaging. Record the exact commands that
actually ran.

## Stopping Condition

This tactical is complete when three fresh runs on both the
`jstorrent-tablet` AVD and Chromebook ARCVM:

- perform the 256 MiB sparse-offset operation through Rust for app-private
  and SAF-backed descriptors;
- verify bytes before close, after reopen, and after process relaunch;
- prove duplicated-descriptor ownership and cancellable task termination;
- exercise and classify staging-directory and materialization renames;
- report allocation, timing, memory, and descriptor evidence;
- leave no probe document, APK data, emulator process, build-only temporary
  file, or testbed capture behind; and
- record which conclusions transfer to tactical `002` and which require a
  later ordinary physical Android device.

## Execution Record

Not started.
