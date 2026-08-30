# Tactical 005: SAF Selective Storage

Status: closed on 2026-07-30 with unavailable Moto and provider-failure
validation explicitly deferred.

## Motivation And Outcome

Tactical `003` proved that Android Storage Access Framework documents expose
usable random-access file descriptors, persisted grants, directory and file
renames, fixed-buffer Rust I/O, and observable cancellation on the required
Android environments. Tactical `004` separately proved that the real torrent
engine can run in-process behind UniFFI, remain owned by a foreground service,
open peer TCP directly, preserve its 32 KiB payload bound, and terminate
cleanly through app-private path-backed storage.

The product seam is still incomplete: the real engine cannot download into a
user-granted document tree. Implement that seam without moving piece payloads
through Kotlin and without weakening verified publication.

Run the existing edge-rich selective fixture through descriptors for wanted
files, the compact skipped-file part file, and materialization output. Kotlin
owns document-provider capabilities and names. Rust duplicates borrowed
descriptors synchronously, owns all torrent data movement, closes its copies on
every terminal path, and reports a prepared result only after piece
verification, part-file reopen, and materialization. Kotlin may then perform
the coarse provider renames. Application success exists only after those
renames and a durable reopen verification.

## Dependencies And References

- [Selective multi-file storage execution record](002-selective-multi-file-storage.md)
- [Android storage feasibility execution record](003-android-storage-feasibility.md)
- [Android engine bootstrap execution record](004-android-engine-bootstrap.md)
- [Product and engine direction](../topics/product-direction.md)
- [Engineering principles](../engineering-principles.md)
- The repository ChromeOS instructions in [`../../AGENTS.md`](../../AGENTS.md)
- The Machine Control ChromeOS skill at
  `~/code/machine-control/platforms/chromeos/skills/SKILL.md`
- The pinned libtorrent part-file and disk-storage implementations
- The controlled libtorrent `2.0.13` peer and Tactical `002` fixture under
  `tests/interop/`

The local Quest is excluded. Every runner operation must resolve and verify an
explicit listed target before installation or mutation.

## Architecture Boundary

### Engine descriptor storage

Add a concrete descriptor-backed mode to the existing selective storage
implementation. This is the second real storage capability and justifies a
small shared file-handle boundary; it does not justify a general virtual
filesystem or speculative async storage framework.

The engine accepts owned `std::fs::File` values for:

- every initially wanted non-padding file, including zero-length files;
- one new empty compact-slot part file;
- one independently opened handle used to reopen and validate that part file;
  and
- one new empty materialization staging file for each requested
  materialization.

Validate the descriptor manifest against the parsed metainfo and selection
before connecting to the peer. Missing, duplicate, unexpected, nonempty, or
wrong-index handles are typed storage failures. Checked lengths and indices
remain bounded by the protocol limits.

Reuse the same cross-file mapping, part-file header, compact slots, streamed
piece verification, selection state, and materialization logic as path-backed
storage. Do not fork a second copy of torrent placement policy.

Regular descriptor I/O may use Tokio's blocking-file adapter. Provider-backed
growth, seek, read, write, sync, and close work must not run on the Android
main thread or the Tokio network reactor. No unbounded storage queue may be
introduced. A received block remains charged to the existing payload budget
until descriptor storage accepts it.

### Borrowed descriptor handoff

Extend the locked UniFFI control plane with a bounded SAF storage plan and one
descriptor-backed start operation.

Kotlin opens caller-owned `ParcelFileDescriptor` values and passes their
integer descriptors in one bounded record. Rust duplicates every descriptor
synchronously before the start call returns. Kotlin closes only its originals;
the engine worker owns and closes only the duplicates.

Partial duplication failure closes all Rust-owned copies and starts no worker,
socket, or storage mutation. Start rejection must not silently take ownership
of caller descriptors. Raw descriptors, paths, and coarse results may cross
UniFFI; piece blocks and storage buffers may not.

Expose a deterministic storage-plan function that parses the bounded metainfo
in Rust and returns only the file indices, safe path components, lengths,
selection roles, part-file identity, and materialization requirements Kotlin
needs to create documents. The worker reparses and revalidates the metainfo
against the supplied descriptor manifest before networking.

### Two-phase verified publication

SAF directory rename is a platform capability, not a Rust path operation.
Avoid a foreign callback on the network or storage path by using an explicit
two-phase completion:

1. Rust downloads, stores, verifies, syncs, independently reopens the part
   file, materializes verified skipped content into temporary descriptors,
   syncs again, closes all descriptor duplicates, and terminates as
   `prepared`.
2. Kotlin verifies that the native task is joined, renames materialization
   temporaries to their final names, then renames the hidden staging directory
   to the final output name.
3. The application reports success only after a fresh process reopens every
   published document through the persisted grant and a Rust helper verifies
   exact length and SHA-1 using a fixed 16 KiB buffer.

Native `prepared` is not product success and must remain distinguishable in
snapshots and results. Provider rename, reopen, hash, metadata, or permission
failure after native preparation is a typed platform-storage failure. No final
output name may contain unverified or partially materialized data.

Keep the part document as a hidden sibling of staging and final output, as in
the path-backed layout. Kotlin persists exact returned document URIs with a
checked synchronous commit before any restart boundary.

### Android document ownership

Extend `clients/android/` rather than creating a second
engine application. A visible activity obtains an
`ACTION_OPEN_DOCUMENT_TREE` grant for one exact host-created empty child and
persists the returned permission before forwarding work to the existing
foreground service.

The service:

- refuses any existing final output, staging directory, part document, or
  app-private result;
- creates only the exact bounded directories and documents in the Rust storage
  plan;
- retains URI metadata but not payload buffers;
- closes every `ParcelFileDescriptor` immediately after synchronous Rust
  duplication;
- remains the sole owner of preparation, native task observation,
  publication, failure cleanup, and notification state;
- preserves pre-existing documents and deletes only documents it created;
- keeps successful published documents until restart verification completes;
  and
- releases the grant and removes the exact test tree during final cleanup.

Provider capability absence is explicit. Directory or materialization rename
unsupported by a provider is not silently treated as successful publication.

## Edge-First Required Scenarios

### Selective success and durable reopen

Retain the exact Tactical `002` fixture:

- five pieces including boundary, skipped-only, padding, and final-short
  shapes;
- wanted, skipped, padding, nested, and zero-length files;
- 97,232 requested real bytes in seven requests;
- 3,304 synthesized padding bytes;
- a 32 KiB payload allowance;
- two compact boundary-piece slots;
- independent part-file reopen;
- one 7,000-byte materialization; and
- exact final file hashes and absent skipped/padding documents.

Finish the activity during transfer, publish through provider renames, force
stop the process, reopen through the persisted grant, verify all wanted and
materialized documents in Rust, then delete only the test-owned tree and
release the grant.

### Cancellation and peer failure

Pass cancellation before storage acceptance and after at least one complete
stored block. Both paths must join, close the peer and all descriptor
duplicates, release the payload reservation, delete owned staging and part
documents, preserve the grant root, and leave no final output.

Disconnect after an actual request and before piece verification. Require a
typed peer failure and the same exact cleanup. A raced received block may be
reported but cannot survive as published content.

### Manifest, reopen, and publication failures

Test the shape-changing failures before treating success as supported:

- missing, duplicate, unexpected, invalid, and nonempty descriptor manifests
  fail before peer connection;
- truncated or corrupt part-file reopen fails after downloaded bytes are
  synced but before platform publication;
- a late final-output collision is preserved and prevents staging rename;
- materialization-final collision is preserved and prevents publication;
- injected provider rename failure leaves no final output and removes only
  owned staging and part documents; and
- revoked or absent permission before restart verification produces a typed
  platform-storage failure without broad deletion.

Repeated cancellation, cleanup, and verification commands must be
deterministic. A second start while active returns `BUSY` without creating a
second provider tree, descriptor set, worker, peer, or output.

### Slow and allocating destinations

Inject the existing bounded storage-acceptance delay and prove that descriptor
storage holds request refill at 32 KiB. Record requested, received, stored,
current payload, and payload high-water values.

Run the normal fixture on the Moto X4 removable exFAT destination. Its small
32 KiB piece geometry should remain correct whether or not the filesystem
preserves holes. Record logical and allocated bytes for the part document, but
do not introduce an allocation-policy branch from this single filesystem.

## Environment Matrix

The required successful matrix is:

| Environment | Destination | Android | ABI | Runs |
| --- | --- | ---: | --- | ---: |
| `jstorrent-tablet` AVD | SAF internal | 34 | x86_64 | 3 |
| Chromebook ARCVM | SAF internal | 33 | x86_64 | 3 |
| Moto X4 | SAF internal | 28 | arm64-v8a | 3 |
| Moto X4 | SAF removable exFAT | 28 | arm64-v8a | 3 |

The AVD and Moto internal destination run every adverse scenario at least
once. The Moto removable destination also runs cancellation once to prove
provider cleanup after an active descriptor transfer. The Chromebook supplies
physical ChromeOS packaging, grant, networking, publication, restart, and
success evidence; repeating every injected failure there is unnecessary
unless behavior diverges.

The Pixel 7a may add evidence if it is attached, but it is not required.

## Contracts And Invariants

- Protocol and deterministic placement remain independent of Android, UniFFI,
  document providers, descriptors, and platform task types.
- Rust owns every peer socket and every piece payload buffer.
- Kotlin creates provider documents but never reads or writes torrent payload.
- Rust duplicates borrowed descriptors before asynchronous ownership.
- Every Rust duplicate and every Kotlin original has one observable close
  path on success, failure, cancellation, and rejected start.
- Descriptor manifests are exact and validated before peer connection.
- The 32 KiB reservation-before-request bound covers received data until
  descriptor storage accepts it.
- Part-file reopen validates the durable header and logical slot extent before
  materialization.
- `prepared` is distinct from published application success.
- Only verified, synced, completely materialized content receives final names.
- Existing documents are preserved and cleanup is exact and ownership-based.
- Restart-critical grant and URI state is synchronously committed.
- Logs remain separate from commands, snapshots, events, and results.
- Target identity, volume, provider authority, and returned document IDs are
  verified before evidence is accepted.

## Non-Goals

- a product destination picker or Compose UI
- generic cloud-provider or OEM-provider support
- permanent unfinished-download resume or power-loss recovery
- an abstract filesystem exposed through per-read or per-write Kotlin calls
- a foreign callback on the piece or storage hot path
- moving payload bytes through UniFFI, JNI, Binder, or a socket proxy
- multiple simultaneous torrents or a general session API
- automatic allocation-mode selection or a new part-file format
- background scheduling, reboot restart, or notification policy
- trackers, magnets, DHT, PEX, upload, or seeding
- a desktop client or stable public application API

## Implementation Sequence

1. Record this tactical and the descriptor, publication, adverse-case, and
   target contracts.
2. Refactor the existing selective storage around shared owned file handles
   and add exact descriptor-manifest host tests.
3. Add prepared completion, synchronous descriptor duplication, storage-plan
   generation, and host-side Android control tests.
4. Extend the bootstrap activity and service with persisted tree grants,
   document preparation, coarse publication, restart verification, and exact
   cleanup.
5. Extend the explicit-target runner with SAF grants, success, cancellation,
   peer failure, descriptor, reopen, collision, rename, restart, and removable
   profiles.
6. Pass the AVD matrix and correct lifecycle, provider, and cleanup races.
7. Pass Moto internal and removable matrices, then the Chromebook matrix.
8. Remove generated artifacts, audit every target, run repository validation,
   and record exact evidence, limitations, and the next boundary.

## Validation

The completed execution record must list every command that actually ran. The
expected baseline is:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
clients/android/build.sh
ANDROID_HOME=/home/kgraehl/Android/Sdk \
  clients/android/gradlew \
  -p clients/android testDebugUnitTest lintDebug
python3 -m py_compile \
  clients/android/run_bootstrap.py
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet --storage saf-internal --runs 3
python3 clients/android/run_bootstrap.py \
  --target chromeos --storage saf-internal --runs 3
python3 clients/android/run_bootstrap.py \
  --target motox4 --storage saf-internal --runs 3
python3 clients/android/run_bootstrap.py \
  --target motox4 --storage saf-sdcard --runs 3
python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

## Stopping Condition

This tactical is complete when:

- the real selective engine uses duplicated SAF descriptors for every wanted,
  part, reopen, and materialization file with no payload crossing Kotlin;
- exact manifest validation and all required pre-network failures pass;
- native preparation, platform publication, restart reopen verification, and
  application success remain distinct and observable;
- three fresh successful cycles pass on all four required
  environment/destination rows;
- the AVD and Moto internal destinations pass the full adverse matrix and the
  Moto removable destination passes active cancellation cleanup;
- slow descriptor storage preserves the 32 KiB payload high-water;
- success, failure, cancellation, rejected start, and restart verification
  close all descriptors, terminate all tasks and peers, and clean only owned
  documents;
- final audit finds no installed test package, persisted grant, grant child,
  app-private run root, peer, reverse port, emulator, generated binding, APK,
  log, or capture artifact; and
- the execution record states what transfers to a product storage service and
  which provider, resume, allocation, and application contracts remain open.

## Execution Record

Implementation landed in incremental commits:

- `459fe4a` opened compact part files from owned, preopened descriptors and
  validated independent reopen without a path.
- `026cb32` added exact wanted, part, reopen, and materialization descriptor
  manifests while retaining one placement, hashing, and slot-release
  implementation for path and descriptor storage.
- `666da42` added bounded native storage-plan generation, synchronous
  close-on-exec descriptor duplication, distinct `PREPARED` completion,
  native per-file hashes, and the fixed 16 KiB restart verifier.
- `878c845` added the persisted tree grant, planned document creation,
  provider publication, force-stop/restart verification, exact cleanup, and
  explicit SAF runner modes.
- `195795c` hardened exact-target picker and failure-injection timing,
  including the ChromeOS `My files` hierarchy.
- `18aed43` made physical-device picker automation start at the external
  storage `Download` document, ignore covered accessibility nodes, and wake
  without unlocking the display.

Host validation currently passes all engine and Android native tests. It
covers duplicate and out-of-range descriptor indices, nonempty part
descriptors, descriptor ownership after caller close, exact selected mapping,
compact-slot reopen, materialization, per-file hashes, invalid raw
descriptors, plan rejection, and fixed-buffer reopen verification.

Recorded device evidence:

- API 34 `jstorrent-tablet` AVD: three fresh normal SAF internal cycles
  passed publication, force-stop, restart verification, and cleanup. Separate
  runs passed slow descriptor acceptance, peer disconnect after a request,
  duplicate start, activity recreation, and both cancellation phases. The
  slow run observed one received 16 KiB block waiting for storage while the
  payload high-water remained exactly 32 KiB.
- Physical Chromebook ARCVM, API 33, `nami_cheets`: three normal SAF internal
  cycles passed. Each final document was reopened through document IDs under
  the persisted grant, verified in Rust, and deleted with the part document.
- Physical Pixel 7a, API 37, `lynx`: three normal SAF internal cycles passed.
  Each cycle transferred 97,232 bytes with an exact 32 KiB payload high-water,
  reopened and hash-verified all five selected documents in Rust after
  force-stop, and deleted the published and compact-part trees.
- The required Moto X4 serial `ZY224JN8D2` was not present in `adb devices`.
  Internal and removable exFAT rows, including removable cancellation and
  allocation observations, remain unrun.

The main vertical thread is implemented. The maintainer closed this tactical
without access to the Moto X4. The original stopping condition was not fully
satisfied: the Moto rows and device-level injected manifest, corrupt-reopen,
collision, rename, and revoked-permission cases remain unrun. They are
explicitly deferred evidence, not claimed provider compatibility. Existing
successful output is never reported as application success before restart
verification.

Validation run for this implementation:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
clients/android/build.sh
ANDROID_HOME=/home/kgraehl/Android/Sdk \
  clients/android/gradlew \
  -p clients/android testDebugUnitTest lintDebug
ANDROID_HOME=/home/kgraehl/Android/Sdk \
  experiments/android-storage-probe/gradlew \
  -p experiments/android-storage-probe testDebugUnitTest lintDebug
python3 -m py_compile \
  clients/android/run_bootstrap.py \
  experiments/android-storage-probe/run_probe.py
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet \
  --storage saf-internal --runs 3 --profile success --no-build
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet \
  --storage saf-internal --profile slow-storage --no-build
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet \
  --storage saf-internal --profile peer-failure --no-build
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet \
  --storage saf-internal --profile duplicate-start --no-build
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet \
  --storage saf-internal --profile activity-recreation --no-build
python3 clients/android/run_bootstrap.py \
  --target avd --avd jstorrent-tablet \
  --storage saf-internal --profile cancellation --no-build
python3 clients/android/run_bootstrap.py \
  --target chromeos --storage saf-internal \
  --runs 3 --profile success --no-build
python3 clients/android/run_bootstrap.py \
  --target pixel7a --storage saf-internal \
  --runs 3 --profile success
scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

The final audit found no AVD, bootstrap package on the Pixel or Chromebook,
Pixel or ChromeOS grant child, ChromeOS reverse port, or generated APK/binding
tree. The Quest serial `2G0YC1ZF93041Z` remained explicitly excluded and
untouched.
