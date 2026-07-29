# Tactical 004: Android Engine Bootstrap

Status: planned.

## Motivation And Outcome

Tacticals `000` through `002` established a bounded Rust BitTorrent engine
thread on desktop, including hostile metainfo limits, direct peer TCP,
block-granular payload accounting, selective multi-file placement, compact
skipped-file part slots, streamed verification, publication, reopen, and
materialization. Tactical `003` separately proved that Android can provide
usable file descriptors, persisted Storage Access Framework grants, fixed
buffer I/O, publication operations, cancellation, and cleanup across an AVD,
ChromeOS ARCVM, and physical phones.

The engine itself has not run inside an Android application. Its current
diagnostic entry point owns Tokio, peer TCP, and path-backed storage from a
desktop CLI. Before designing a broad application API or a product UI, prove
that the real engine can be packaged into Android, controlled through the
accepted UniFFI boundary, owned by a foreground service, and terminated
observably under both successful and adverse conditions.

Build one minimal Android bootstrap harness that runs the existing
edge-rich selective download against the controlled libtorrent peer. The
payload path remains entirely in Rust. Kotlin owns only Android lifecycle,
foreground-service integration, and coarse control and observation.

This tactical deliberately uses app-private ordinary files for engine
storage. That isolates engine packaging, runtime ownership, direct networking,
UniFFI, service lifecycle, cancellation, and backpressure from the separate
SAF storage-adapter design. Tactical `003` remains the evidence for the
descriptor and document-provider seam; a later bounded tactical may connect
that seam to the real engine.

## Dependencies And References

- [First verified piece execution record](000-first-verified-piece.md)
- [Bounded large-piece execution record](001-bounded-large-piece.md)
- [Selective multi-file storage execution record](002-selective-multi-file-storage.md)
- [Android storage feasibility execution record](003-android-storage-feasibility.md)
- [Product and engine direction](../topics/product-direction.md)
- [Engineering principles](../engineering-principles.md)
- The repository ChromeOS instructions in [`../../AGENTS.md`](../../AGENTS.md)
- The authoritative testbed skill at
  `~/code/chromeos-testbed/skills/SKILL.md`
- The pinned libtorrent reference and the existing controlled interoperability
  harness under `tests/interop/`
- The official [UniFFI guide](https://mozilla.github.io/uniffi-rs/latest/) and
  [changelog](https://github.com/mozilla/uniffi-rs/blob/main/CHANGELOG.md) for
  the exact version selected during implementation

The implementation must lock the selected UniFFI, Kotlin, JNA, coroutine,
Gradle, Android plugin, and Rust dependency versions. Current upstream
behavior is an input to validate, not a floating build dependency.

The local USB-connected Quest device is not part of this tactical. No runner
may select the first ADB device implicitly or install on an unlisted target.

## Scope

### Android-facing Rust library

Add a small Android-facing Rust `cdylib` crate that depends inward on
`rstorrent-engine`. It may adapt engine configuration, runtime ownership, and
typed results, but it must invoke the actual engine implementation rather than
reimplement peer or storage behavior.

Build at least `x86_64` and `arm64-v8a` libraries with the Android NDK. Keep
Android, UniFFI, JNA, Gradle, and Kotlin types out of
`rstorrent-protocol`. Platform-specific dependencies must not leak into pure
protocol or deterministic state transitions.

The library owns any Tokio runtime, worker thread, active engine task,
cancellation signal, and termination result that it creates. Ownership must be
visible in the exported object model and testable without relying on garbage
collection or process death.

### UniFFI control plane

Use UniFFI as the generated Rust/Kotlin binding. Prefer its proc-macro
interface unless implementation evidence shows that a UDL file materially
simplifies the bounded API.

Expose only the control-plane concepts required by this tactical:

- one opaque engine-session object;
- a bounded download configuration;
- an explicit start result that rejects a second active download;
- structured snapshots with lifecycle state and resource high-water values;
- a terminal success or typed failure result;
- an explicit cancel-and-join operation; and
- coarse event draining only if snapshots alone cannot make termination and
  state transitions observable.

Starting a download must return control promptly. Do not represent the entire
download as one foreign coroutine whose cancellation is assumed to stop the
engine. Cancellation is an explicit engine command followed by an observable
join.

Generated Kotlin and native scaffolding must come from the same locked Rust
interface. Build-time or startup checks must fail on mismatched generated
bindings and native libraries. Generated code is not hand-edited.

JNA or binding overhead is accepted for this low-frequency control plane.
Peer messages, piece blocks, hashes, storage buffers, and socket reads and
writes never cross UniFFI. Handwritten JNI is out of scope unless a concrete
blocker is recorded and the smallest possible escape hatch is added to this
tactical before implementation continues.

### Foreground-service ownership

Add a minimal Android application under
`experiments/android-engine-bootstrap/`. It is an integration harness, not the
first product UI.

A visible activity may start the work, but an Android foreground service owns
the UniFFI engine-session object and its explicit termination. The service:

- creates its notification channel and enters the foreground within the
  platform deadline;
- declares the required foreground-service type and version-appropriate
  permissions;
- permits the activity to finish or be recreated without cancelling the
  engine;
- rejects a second active start deterministically;
- maps explicit stop to cancel-and-join;
- records terminal state only after the Rust task has terminated; and
- tears down the notification and service after terminal cleanup.

The service must not rely on finalizers, Kotlin object collection, application
process death, or notification dismissal to stop Rust work.

### Controlled direct networking

Reuse the deterministic selective multi-file fixture and controlled libtorrent
peer from `tests/interop/`. The host runner may use explicit `adb reverse`
transport so the existing loopback-only diagnostic restriction remains in
force inside Android.

Rust opens, reads, writes, and closes the peer TCP socket directly. Kotlin
does not acquire a socket, proxy bytes, or receive per-peer callbacks.

The successful profile retains tactical `002`'s edge shape:

- five pieces including boundary, skipped-only, and final-short pieces;
- wanted, skipped, padding, and zero-length files;
- 97,232 requested real bytes in seven requests;
- 3,304 synthesized padding bytes;
- a 32 KiB engine-owned payload allowance;
- compact boundary-piece part slots;
- streamed mixed-source verification;
- verified publication and durable reopen; and
- materialization with correct slot retention and release.

If the fixture evolves during implementation, the tactical must record the
new exact shape and why the change preserves or strengthens these conditions.

### App-private storage baseline

Place each run beneath one exact app-private session root. Continue to use the
engine's normal path-backed staging, part-file, verification, publication,
reopen, and materialization behavior.

The runner and application may delete only paths they created and identified
for the current test. Pre-existing output, staging, part, or result paths are
errors and must be preserved. Failure and cancellation remove unverified
owned artifacts; successful output is verified before the runner removes the
whole exact test root.

SAF trees, `ParcelFileDescriptor`, document-provider callbacks, removable
storage, and user-visible destination selection are not engine storage inputs
in this tactical. Their exclusion is a bounded sequencing decision, not a
change to the accepted Android product direction.

### Backpressure and resource evidence

Preserve reservation-before-request accounting and the 32 KiB payload
allowance. A received block remains charged until storage accepts it. Slow
storage must stop request refilling rather than accumulate unbounded blocks.

Any new queue between network receipt and storage must have:

- an explicit byte limit;
- reservation before enqueue;
- release on success, failure, and cancellation;
- a reported current value and high-water mark; and
- a deterministic test that stalls storage and proves the limit.

Blocking file growth, sync, or native work must not run on the Android main
thread or a Tokio network reactor thread. The exact storage execution owner
may follow existing Tokio blocking-file behavior or use a dedicated worker,
but ownership, cancellation limits, and termination must be explicit.

Record per run:

- target identity, Android API, ABI, and build fingerprint;
- selected UniFFI and native library identity;
- service and engine lifecycle transitions;
- requested, received, stored, verified, selected, skipped, padding,
  materialized, and cleaned byte counts;
- payload and any storage-queue current and high-water bytes;
- task and descriptor termination assertions;
- bounded timing for start, cancellation, join, and total completion; and
- coarse Java heap, native heap, and total PSS snapshots.

These are component observations, not an exact future product RSS guarantee.

## Required Scenarios

### Successful foreground download

Start the service from the visible activity, begin the controlled selective
download, finish the activity while the transfer is active, and prove that the
service and Rust task continue. Require verified output, reopen,
materialization, a terminal success snapshot, explicit task termination, and
exact cleanup.

Run three fresh application-data cycles on every required target.

### Explicit cancellation

Use a controlled peer or storage gate that keeps the transfer active after at
least one block is accepted. Issue the service stop command, observe
`running -> cancelling -> terminated`, join within a recorded bound, and prove
that the socket, storage task, reserved bytes, and unverified files are
released.

Cancellation before the first block and repeated cancellation after the
terminal state must also be deterministic and non-leaking.

### Peer failure

Disconnect the controlled peer after at least one request but before
verification. Require a typed peer failure, reservation release, terminal task
observation, no published unverified content, and exact owned-artifact cleanup.
Do not report this as a generic timeout or successful cancellation.

### Duplicate start and activity recreation

Attempt a second start while one engine task is active and require a typed
busy result without creating another runtime, task, socket, or output root.
Destroy and recreate or finish the activity during the active transfer and
prove that the service remains the sole session owner.

### Pre-existing artifacts

Place sentinel content at every protected output, staging, part, and result
collision checked by the bootstrap. Require refusal, byte-for-byte
preservation, no peer connection, and no broad cleanup.

## Environment Matrix

The required successful matrix is:

| Environment | Android | ABI | Runs |
| --- | ---: | --- | ---: |
| `jstorrent-tablet` AVD | 34 | x86_64 | 3 |
| Chromebook ARCVM | 33 | x86_64 | 3 |
| Moto X4 | 28 | arm64-v8a | 3 |

The AVD and Moto X4 must each run cancellation, peer-failure, duplicate-start,
activity-recreation, and pre-existing-artifact scenarios at least once after
the three successful fresh cycles. The Chromebook supplies physical ChromeOS
packaging, service, networking, and success evidence; repeating every injected
failure there is not required unless its behavior diverges.

The host runner verifies exact model, device, API, ABI, serial, and transport
before installation. ChromeOS health, ARCVM authorization, APK transport, UI
automation, screenshots, and recovery use `~/code/chromeos-testbed`.

An attached Pixel may provide additional evidence but is not required by the
stopping condition unless the tactical is amended before those runs.

## Contracts And Invariants

- `rstorrent-protocol` remains independent of async runtimes, Android, UniFFI,
  JNA, JNI, Gradle, Kotlin, sockets, and filesystems.
- Android and binding crates depend inward on the engine; the engine does not
  depend on the Android application.
- Rust owns ordinary peer sockets and all piece payload movement.
- UniFFI carries bounded owned values and opaque handles, never borrowed Rust
  references or payload buffers.
- The foreground service is the identifiable owner of one engine session.
- Every background task has an explicit cancellation and observable join path.
- A second active start cannot create hidden duplicate work.
- Storage queueing is byte-bounded before accepting more peer payload.
- Verified publication remains the only success path.
- Pre-existing artifacts are preserved; cleanup is exact and ownership-based.
- Logs remain separate from commands, snapshots, events, and final results.
- Target selection is explicit and verified before installation.
- Generated bindings and native libraries cannot silently use different
  interface versions.
- Successful completion, failure, and cancellation all end with zero reserved
  payload bytes and no live engine task.

## Non-Goals

- a product Android UI or Compose architecture
- SAF-backed engine storage or removable-media downloads
- unfinished-download resume after process death
- automatic service restart, reboot recovery, or background scheduling
- trackers, magnets, DHT, PEX, LSD, multiple peers, upload, or seeding
- general Android network selection, VPN binding, proxy, or metered policy
- a stable public application API
- a desktop binding or first desktop client
- iOS bindings or Kotlin Multiplatform
- a native host, daemon, REST API, WebSocket proxy, or socket proxy
- per-block callbacks, byte arrays, or payloads across UniFFI or JNI
- exact process-memory or completion-latency guarantees
- power-loss durability or a permanent resume format
- broad Android, OEM, JNA-version, or document-provider compatibility claims

## Implementation Sequence

1. Record this tactical, the accepted boundary, target matrix, adverse
   scenarios, and exact stopping condition.
2. Add the Android-facing Rust library and host-side tests for its UniFFI
   object lifecycle, start rejection, snapshots, cancellation, join, and
   terminal-state behavior.
3. Package locked x86_64 and arm64-v8a libraries and generated Kotlin bindings
   into the minimal Android application.
4. Implement foreground-service ownership, notification behavior, structured
   results, and exact app-private test-root cleanup.
5. Extend the controlled interop harness and explicit-target runner with
   `adb reverse`, success, stalled transfer, peer failure, collision, activity,
   and service controls.
6. Pass the AVD success and adverse matrix, correcting cold-start and lifecycle
   races before counting final runs.
7. Run the physical Moto X4 matrix and required Chromebook success matrix.
8. Remove generated artifacts, audit all targets, run repository validation,
   and record exact evidence, limitations, and the next boundary.

## Validation

The implementation tactical should provide exact build and runner commands.
The expected baseline is:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
experiments/android-engine-bootstrap/build.sh
experiments/android-engine-bootstrap/gradlew \
  -p experiments/android-engine-bootstrap testDebugUnitTest lintDebug
python3 -m py_compile \
  experiments/android-engine-bootstrap/run_bootstrap.py
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target avd --avd jstorrent-tablet --runs 3
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target chromeos --runs 3
python3 experiments/android-engine-bootstrap/run_bootstrap.py \
  --target motox4 --runs 3
python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

The runner must expose named adverse profiles rather than hiding them inside a
single success command. The completed execution record must list every command
that actually ran, including narrower native, Gradle, and device audits.

## Stopping Condition

This tactical is complete when:

- the actual `rstorrent-engine` is linked into and runs inside the Android
  application on both packaged ABIs;
- Kotlin controls it through locked generated UniFFI bindings with no payload
  crossing;
- an Android foreground service visibly owns one Rust runtime and engine task;
- three fresh selective-profile downloads pass on the AVD, Chromebook ARCVM,
  and Moto X4;
- the AVD and Moto each pass explicit cancellation, peer failure,
  duplicate-start, activity-recreation, and pre-existing-artifact scenarios;
- the existing 32 KiB payload bound and any new storage-queue bound hold under
  an injected storage stall;
- success, failure, and cancellation all terminate observably and clean only
  owned artifacts;
- the final audit finds no live task, installed test package, app-private test
  root, host peer, reverse port, emulator, generated binding, APK, log, or
  capture artifact; and
- the execution record states what the bootstrap proves, what remains specific
  to app-private storage, and whether the next bounded slice should integrate
  SAF storage or define a broader application service.
