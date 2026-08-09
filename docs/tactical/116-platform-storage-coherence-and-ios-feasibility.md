# Tactical 116: Platform Storage Coherence And iOS Feasibility

Status: Authorized and in progress after completed Tactical
[`114`](114-session-wide-concurrent-torrent-admission.md). Maintainer
discussion on 2026-08-09 accepted Android as a non-deferrable engine parity
gate and iOS as an eventual first-party native product worth testing on a
physical device before more storage-facing engine features harden the current
seams; implementation authorization followed the same day.

Topics: `product-direction`, `capability-readiness`, `android-saf-storage`,
`client-persistence`, `download-roots`, `storage-throughput-architecture`,
`incoming-reachability-and-seeding`, `client-surfaces`,
`product-surfaces-and-migration`, `code-organization-and-refactoring`,
`oracle-driven-engine-campaign`

Dependencies: completed Tacticals
[`003`](003-android-storage-feasibility.md),
[`005`](005-saf-selective-storage.md),
[`009`](009-android-saf-session-storage.md),
[`052`](052-batched-durability-checkpoints.md),
[`054`](054-bounded-independent-storage-execution.md),
[`061`](061-user-selected-download-roots.md),
[`067`](067-dynamic-platform-file-acquisition.md),
[`073`](073-unified-storage-and-complete-recheck.md),
[`078`](078-local-single-peer-tcp-seeding.md),
[`082`](082-bounded-multi-peer-upload-ownership.md),
[`105`](105-fact-based-persistence-and-recheck-containment.md), and
[`108`](108-serialized-torrent-control-and-observable-checking.md) establish
the storage, persistence, Android, upload, and checker behavior this slice
reconciles. Tactical `114` first moves storage and checking admission to the
session owner so this slice does not shore up an owner already scheduled for
replacement.

## Decision And Motivation

Shore up the capability-backed storage boundary before adding another engine
feature. RSTorrent already has one strong Rust payload implementation beneath
path and Android SAF acquisition, but the surrounding lifecycle is not yet
coherent enough to be a durable multi-platform foundation:

- download, selective placement, hashing, checkpointing, and complete recheck
  share `SelectiveStorage`, `StorageFilePool`, and dynamic logical file
  references;
- complete-torrent upload constructs a separate path-backed `SeedContent` and
  explicitly rejects platform roots;
- path restart can inspect file type, length, symlink status, and publication
  topology, while the platform broker can currently return only an open file
  or deletion success;
- Android queries stable provider document IDs and display names during
  lookup but does not return type, length, modification, or identity
  observations to Rust;
- root availability can be advertised before a persisted SAF grant has been
  exercised, so failure is learned only when an operation requests a file;
- path and platform publication/removal use the same durable product states
  but still have different outer choreography; and
- legacy fixed descriptor-manifest backing remains a core storage variant
  even though the product moved to bounded dynamic acquisition.

The present supported SAF path is conservative rather than known corrupt: it
rehashes before it restores verified state. The risk is architectural drift
and missing product capability, not evidence of false verification. Leaving
the split in place would make fast resume, seeding, relocation, file
priorities, cache work, and another native platform each choose between
duplicating path behavior or widening an already fragmented core.

This tactical establishes the smallest storage-capability vocabulary earned
by three real cases: local paths, Android SAF, and a physical iOS probe. It
does not design a generic virtual filesystem and does not import libtorrent or
JSTorrent architecture.

## Non-Negotiable Platform Policy

### Android cannot be left behind

Android/ChromeOS Android is a first-party RSTorrent engine product, not a
follow-up compatibility port. From this tactical onward:

1. An engine or application capability cannot be marked complete while its
   applicable Android engine semantics are missing. The implementation,
   generated binding adaptation, Android build, and proportional AVD or
   physical evidence land in the same tactical.
2. A missing Android adapter or SAF operation blocks completion; it cannot be
   moved to an unspecified later task. A behavior may be marked inapplicable
   only when the tactical explains why no Android product path can exercise
   it.
3. Shared correctness and product semantics include restart, cancellation,
   resource bounds, diagnostics, and repair—not merely successful
   cross-compilation.
4. Presentation parity remains a separate product decision. Android need not
   mirror dense desktop tabs or every advanced control, but the engine and
   application behavior beneath an applicable command may not silently
   diverge.
5. No Kotlin payload I/O, Kotlin checker, platform-only piece cache, second
   scheduler, second storage runtime, or Android-only torrent state is an
   acceptable way to obtain parity.

This policy is a completion gate, not a requirement that every commit build an
APK. Intermediate commits may be locally incomplete; the tactical and feature
cannot close that way.

### iOS is front-loaded without pretending it is shipped

iOS is an accepted eventual first-party in-process product. This tactical
must obtain early physical evidence because security-scoped storage, file
coordination, process suspension, and background execution can change the
correct capability and lifecycle boundaries. The probe is not a complete iOS
client, App Store or alternative-distribution effort, UI parity commitment,
or iOS support claim.

Future applicable engine tacticals must preserve the proven platform-neutral
seam and record an iOS applicability decision. Until a separate iOS product
tactical exists, Android's same-slice completion rule does not automatically
make every feature ship on iOS.

## Stopping Condition

This tactical is complete only when all of the following hold:

1. Download writes, positional reads, piece hashing, complete recheck,
   durability, and verified upload reads consume one logical-file and
   file-pool ownership model. `SeedContent` no longer reconstructs a
   path-specific payload filesystem or rejects a complete torrent solely
   because its root is platform capability-backed.
2. A small backend-neutral observation value can report the exact facts this
   product currently needs—existence, expected object kind, length, and a
   backend-specific opaque change/identity token when safely available—without
   exposing paths, SAF URIs, Apple URLs, provider document IDs, or descriptors
   to portable state.
3. Path and supported local Android SAF adapters implement that observation
   contract with typed unavailable, missing, permission, wrong-kind,
   collision, stale-generation, and provider-failure outcomes. Unsupported or
   absent observation is represented explicitly and never fabricated.
4. Root restore and repair exercise the capability sufficiently to expose a
   lost grant or unusable root before torrent work is admitted, while still
   treating later provider failure as an ordinary availability transition.
5. Complete published SAF torrents register with the existing incoming and
   outgoing upload owner, serve only pieces justified by current verified and
   readable state, use the same 40-handle session pool and ten-read admission,
   and unregister before publication, repair, removal, pause, replacement, or
   shutdown changes storage authority.
6. Publication, repair, deletion, and root loss share explicit task-free
   namespace-transition inputs and outcomes even where the path and platform
   adapters execute different operating-system calls. No adapter independently
   invents verified, published, complete, or removed state.
7. The fixed descriptor-manifest backing is removed from the production core
   or isolated behind a diagnostic/test-only compatibility boundary. Dynamic
   product construction is the only Android storage architecture.
8. The Android parity matrix below passes for download, selection, forced
   process restart, complete recheck, publication, upload, removal, grant
   loss/repair, cancellation, and exact cleanup, with bounded descriptor and
   request high-water marks.
9. A minimal harness using the actual Rust engine/storage code builds and runs
   on the paired physical iPhone. It records the iOS storage and lifecycle
   matrix below, including negative findings, without treating simulator-only
   behavior as physical evidence.
10. The owner/cancellation map, current topics, queue, platform policy,
    readiness rows, and execution record agree with the landed code and exact
    evidence. No later fast-resume policy is smuggled into this slice.

## Scope

### Shared logical storage seam

Extract immutable artifact geometry and safe logical identities from the
write-side coordinator as already recommended by
`code-organization-and-refactoring.md`. Keep `SelectiveStorage` as the owner
of selection routes, part-file state, verified state, materialization, and
publication transitions. Keep `StorageFilePool` as the session owner of
actual path and platform handles.

The common vocabulary is limited to operations already required by product
behavior:

- open an existing logical file for bounded positional reads;
- open or create a wanted engine-owned logical file for writes;
- observe an exact logical artifact without creating it;
- publish the exact fenced staging namespace without replacement;
- remove an exact engine-owned artifact after handle release; and
- synchronize the durability targets retained by a checkpoint generation.

These may be represented by plain enums, structs, and concrete adapters. A
trait is justified only if implementation shows it makes ownership or tests
clearer. Do not add ambient directory traversal, arbitrary path operations, a
POSIX emulation API, or a framework whose surface is not exercised by path,
Android, and the iOS probe.

`StorageObservation` is restart evidence, not content proof. Length, kind,
timestamps, allocation, and opaque identities can disqualify a trusting path;
they cannot establish piece validity by themselves. Unsupported facts remain
`None`/unknown rather than optimistic defaults.

### Android SAF closure

Extend the existing bounded `ProductSafDocuments` broker and
`PlatformStorageClient`; do not create another service or callback payload
path. The Android namespace owner may return safe bounded observation fields
derived from `DocumentsContract` and the current grant. Raw document IDs stay
inside Kotlin even when used to derive a process-safe opaque token.

The supported initial provider claim remains narrow: the local provider used
by the existing product/AVD and explicitly tested physical device roots.
Cloud, offloaded, and third-party providers are not silently treated as
equivalent. A provider that cannot supply stable observations or safe seekable
descriptors remains conservative/unavailable for the affected optimization;
ordinary full checking may still operate when the established SAF contract is
otherwise satisfied.

SAF upload reads reuse `StorageFileReference` and the shared file pool. They
must use open-existing, never create a missing published file, and must update
verified/readable availability after a failed open or observation instead of
serving stale have bits.

### iOS physical feasibility track

Create the smallest repository-owned iOS harness needed to exercise the real
Rust capability—normally under `experiments/ios-storage-probe` with a narrow
Swift bridge. A full SwiftUI product, generated application contract, or
second engine implementation is out of scope.

The harness first proves the app-owned Documents directory, then a directory
the user explicitly selects from the local **On My iPhone** provider. It must:

- cross-compile the relevant Rust crates for `aarch64-apple-ios` and link them
  into a signed development build;
- select a directory with `UIDocumentPickerViewController`, persist a
  bookmark, terminate, relaunch, resolve it, and balance every successful
  `startAccessingSecurityScopedResource()` call;
- use `NSFileCoordinator`/the applicable Apple coordination contract around
  external-root access. A Rust descriptor lease may operate only inside a
  proven coordination window; a successful uncoordinated POSIX call is not
  sufficient evidence;
- create nested files, perform bounded `pread`/`pwrite` at offsets, truncate,
  sync, close/reopen, rename without replacement, observe, and delete through
  the proposed capability seam;
- verify exact SHA-1 through Rust and record sparse/allocation behavior as an
  observation rather than a requirement;
- prove that payload buffers do not cross Swift and that descriptors,
  security scopes, coordinator work, Rust tasks, and files all return to
  baseline after cancellation and shutdown;
- exercise direct Rust TCP and UDP against a controlled local endpoint so the
  target is not mistaken for a storage-only library port; and
- record foreground, ordinary background, expiration, suspension/resume, and
  force-close/relaunch behavior. On iOS 26, separately probe whether a
  user-initiated finite download/check operation is compatible with
  `BGContinuedProcessingTask`; do not infer indefinite seeding or support on
  older iOS releases.

If coordinated external-provider descriptor I/O is not viable, the accepted
result is not to tunnel payload through Swift. Record the negative result,
retain the opaque root/capability shape, and limit the first eventual iOS
storage claim to the app-owned Documents directory until a later design has
evidence.

The planning preflight on 2026-08-09 found Xcode 26.6 (build `17F113`), iOS
26.2/26.3/26.5 simulator runtimes, and an available paired iPhone SE (3rd
generation). Rust currently has no iOS target installed. These are toolchain
and device-availability observations only; they are not build, signing,
storage, networking, or lifecycle evidence.

## Persisted And Runtime Facts

No raw platform locator enters portable torrent rows. The durable split is:

- profile storage owns a stable opaque root ID, selected-root relationship,
  payload/publication state, verification evidence, and any later persisted
  observation envelope;
- the platform adapter owns the Android tree grant or Apple bookmark and its
  repair/revocation lifecycle;
- the engine owns safe relative artifact identity, expected geometry,
  verification, and current storage generation; and
- open handles, coordinator leases, security-scope counts, provider
  observations, and root health are generation-fenced runtime facts.

This tactical may version a platform-neutral observation envelope if needed
for deterministic comparison, but it does not authorize a trusting resume
decision or skip piece hashing. Existing restart remains conservative and
Force recheck remains a full validation pass.

Crash state is based on interrupted durable work, not on a blanket process-
crash bit. A crash does not make an inactive torrent suspect merely because
the process ended. Later fast resume will decide trust per torrent from its
own last durable epoch, storage generation, root identity, and observation
evidence; this tactical only makes those facts coherent across backends.

## Owner, Task, Cancellation, And Dependency Map

```text
ApplicationService generation
  -> root registry + durable torrent/storage state
  -> session storage resources (from Tactical 114)
       -> StorageFilePool (all path/platform handles)
       -> write/hash/check admission and durability generations
  -> per-torrent SelectiveStorage state (task-free transitions)
  -> per-torrent peer owner
       -> common published logical content
       -> existing session upload read admission
  -> platform namespace adapter
       -> desktop path operations
       -> Android SAF broker workers
       -> iOS probe security-scope/coordinator owner
```

- Pure artifact layout, observations, transition decisions, and generation
  checks remain independent of Tokio, Kotlin, Swift, URLs, descriptors, and
  operating-system calls.
- The application generation owns root health, publication/removal sequencing,
  and platform broker lifetime. It does not perform payload I/O.
- `StorageFilePool` owns Rust handles. In-flight jobs retain leases; eviction
  removes only the pool reference; close occurs outside the pool lock.
- Android retains at most the existing four broker workers and 16 pending
  requests unless evidence justifies tightening those values. Late responses
  to cancelled or replaced generations are closed and rejected.
- The iOS probe owns every security-scope and coordination lease in Swift, but
  Rust owns the bounded operation and payload. Expiration requests
  cancellation; the Rust generation joins before the platform lease ends when
  the operating system permits. Force-close is treated as unjoined process
  death and recovered from durable facts.
- Publication, root repair, removal, pause/replacement, and application
  shutdown first stop admission, then join/fence storage and upload work,
  release handles, perform the namespace operation, advance the generation,
  and only then advertise the new state.

Dependency direction remains:

```text
pure layout/observation/transition values
                 ^
engine storage and upload owners
                 ^
session persistence and lifecycle
                 ^
desktop / Android / iOS platform adapters
```

No Apple or Android SDK type may point inward across that boundary.

## Resource Bounds

Retain the established limits unless implementation evidence requires a
smaller value:

| Resource | Initial bound |
| --- | ---: |
| Session storage handles, path and platform combined | 40 |
| Concurrent platform broker workers | 4 |
| Pending platform storage requests | 16 |
| Concurrent upload reads | 10 |
| Observation fields encoded per logical artifact | constant, at most 5 |
| Opaque observation token | 256 bytes |
| Platform failure detail | 1,024 bytes |
| iOS probe files | at most 64 |
| iOS probe live Rust file leases | at most 8 |
| iOS probe TCP connections / UDP sockets | 2 / 2 |

The iOS limits are feasibility limits, not product defaults. Record Rust-owned
handle, process descriptor, pending request, security-scope, coordinator,
memory, task, and socket high-water marks. Tightening a limit from evidence is
in scope; materially increasing an established product limit requires the
tactical record to justify it.

## Shape-Changing Edge Cases

These land with the common path rather than becoming later patches:

- missing file versus provider refusal versus revoked root permission;
- wrong file/directory kind, symlink on path storage, duplicate provider name,
  case/canonicalization collision, and no-replace publication conflict;
- same-length content with a changed identity/token, changed length with a
  stable name, unavailable timestamp/token, and timestamp granularity;
- stale namespace generation and a late open/observe/delete/publish response;
- read-existing never creating a file and failed observation never truncating
  or materializing content;
- file-pool eviction during upload/read/hash/checkpoint and root repair while
  a lease is retained;
- pause, removal, repair, publication, and shutdown racing an upload read;
- process death before/after payload sync, verification commit, provider
  rename, observation capture, and complete registration;
- Android grant revoke/restore and provider process death;
- iOS stale bookmark, failed security-scope acquisition, provider
  coordination error, app background expiration, suspension, memory
  termination, and force-close without an expiration callback; and
- partial support: a backend may lack a safe opaque identity token without
  losing conservative full-check functionality or fabricating evidence.

## Reference Dossier

### Pinned libtorrent 2.0.13

Implementation must revalidate the checkout at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` before code changes. The starting
paths already inspected by Tacticals `067` and `073` are:

- `include/libtorrent/aux_/file_view_pool.hpp` and
  `src/file_view_pool.cpp::open_file` for session-wide compatible-open
  single-flight, LRU, upgrade, deferred close, and generation release;
- `src/mmap_disk_io.cpp` for one pool across storage instances and pool
  observability;
- `src/mmap_storage.cpp::{read,write,hash,need_partfile,set_file_priority}` and
  `src/posix_storage.cpp` for one storage interface across payload operations;
- `src/part_file.cpp` for lazy auxiliary ownership;
- `simulation/test_file_pool.cpp::file_pool_size` for bounded multi-file
  completion; and
- `test/test_part_file.cpp` and `test/test_resume.cpp` cases recorded in
  Tactical `073` for short read, priority, resume, missing/changed file, and
  checking edge cases.

Adopt completeness questions and observable behavior, not libtorrent's C++
class graph, mmap policy, resume format, timestamp trust policy, or storage
interface.

### JSTorrent product history

The sibling was inspected at
`9895410beeed6aff554053769bd006a3fbd373ef` on 2026-08-09. Relevant paths are:

- `android/io-core/.../FileManagerImpl.kt` and its SAF/concurrency tests for
  local-provider positioned I/O, handle validation, and provider races;
- `ios/JSTorrent/App/AppSettings.swift` and `SettingsScreen.swift` for
  directory selection, persisted bookmarks, security-scope lifetime, and
  root restoration;
- `ios/JSTorrentKit/.../Bindings/FileBindings.swift` for pooled descriptors
  and `pread`/`pwrite` in the current iOS client;
- `ios/JSTorrentKit/.../Bindings/SocketBindings.swift` for direct native TCP
  and UDP rather than a companion daemon;
- `ios/JSTorrent/App/ContentView.swift` and
  `Runtime/EngineController.swift` for shutdown-on-background behavior; and
- `docs/archive/reports/android-standalone-vs-ios-runtime-gap-report.md` for
  the documented split: disruptive iOS root/controller replacement, unknown
  root fallback, weaker persistence/checking, lifecycle gaps, and Android/iOS
  file-runtime divergence.

RSTorrent adopts the product lessons and failure cases. It does not embed the
TypeScript engine, copy Swift/Kotlin source, carry payload through a language
bridge, silently fall back unknown roots into a sandbox, or maintain separate
Android and iOS torrent runtimes.

### Apple platform contract

The primary platform references are Apple's
[Providing access to directories](https://developer.apple.com/documentation/uikit/providing-access-to-directories),
[`UIDocumentPickerViewController`](https://developer.apple.com/documentation/uikit/uidocumentpickerviewcontroller),
[`NSFileCoordinator`](https://developer.apple.com/documentation/foundation/nsfilecoordinator),
[Extending your app's background execution time](https://developer.apple.com/documentation/uikit/extending-your-app-s-background-execution-time),
and
[Performing long-running tasks on iOS and iPadOS](https://developer.apple.com/documentation/backgroundtasks/performing-long-running-tasks-on-ios-and-ipados).
They establish user-selected recursive directory access, bookmark and
security-scope requirements, coordinated external-document I/O, brief and
expiring ordinary background time, and the iOS 26 user-initiated continued-
processing model. Platform API reading is not a feasibility or support claim;
the physical matrix is required.

No source, sample, fixture, project asset, or entitlement is imported from any
reference by this planning change.

## Execution Record

### Stage 1: frozen starting behavior

Revalidation on 2026-08-09 found the pinned libtorrent checkout clean at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` and the local JSTorrent sibling at
the recorded `9895410beeed6aff554053769bd006a3fbd373ef`. All dossier paths
still exist. The exact starting behaviors are:

- `file_view_pool::open_file`, `mmap_storage::open_file`, and
  `posix_storage::open_file` retain one bounded compatible-open vocabulary
  across read, write, hash, priority, and part-file cases. The
  `file_pool_size` simulation and seed-mode missing-file cases remain the
  relevant completeness checks; no C++ ownership shape or resume trust rule
  is adopted.
- JSTorrent still balances bookmark/security-scope restoration in
  `AppSettings.swift`, performs positioned descriptor I/O in
  `FileBindings.swift`, owns direct TCP/UDP in `SocketBindings.swift`, and
  shuts its current runtime down on background entry in `ContentView.swift`.
  These are product-history inputs, not RSTorrent evidence.
- RSTorrent path and fake-platform writes, hashing, recheck, publication
  inventory, pool eviction, generation fencing, and namespace mismatch cases
  were already covered. Path upload already masks missing, skipped, truncated,
  cross-file, and padding sources. A new broker-cancellation fixture freezes
  read-existing request shape, typed cancellation, and zero handle/cache
  leakage; a task-free session fixture freezes the current path-only upload
  eligibility before it is deliberately widened.
- The platform broker has only open and delete operations at this checkpoint.
  It cannot observe a logical artifact without opening it, and
  `SeedContent::open_published_with_pool` still performs path metadata and
  symlink checks directly. This absence is the observation baseline, not a
  capability claim.

Validation for this stage is recorded with its commit. No runtime semantics
were changed except making the pre-existing path-only seed-root decision an
explicit testable helper.

### Stage 2: front-loaded physical iOS feasibility

The repository-owned harness under `experiments/ios-storage-probe` now links
the real `rstorrent-engine` `StorageFilePool` into a small SwiftUI shell. Rust
owns every payload byte, positioned read/write, truncate, sync, SHA-1,
no-replace rename, observation, deletion, TCP, and UDP operation. Swift passes
only a coordinated root path and receives bounded JSON evidence; no payload
callback crosses the bridge.

On a physical iPhone SE (3rd generation) running iOS 26.6:

- both `aarch64-apple-ios` and `aarch64-apple-ios-sim` Rust builds pass, the
  simulator Xcode build links, and an existing development identity signs,
  installs, and launches the physical build without changing account state;
- five app-owned Documents runs reproduce SHA-1
  `48b6fdf2fd3b77c14cc54f54891dc6aed1eeec3a` for the 65,536-byte sparse
  fixture, reject a no-replace collision, reopen and truncate to 40,960 bytes,
  remove the exact workspace, peak at one of eight allowed Rust file leases,
  and finish with zero cached or owned handles;
- a bookmark/`NSFileCoordinator` run against the app's exported local
  `PickerRoot` passes. Initial access needs no security-scope acquisition;
  after forced termination, bookmark restoration is non-stale,
  `startAccessingSecurityScopedResource()` succeeds, the balanced coordinated
  Rust run passes again, and the workspace is absent afterward;
- exact 30-byte TCP and UDP echoes pass with direct Rust sockets against a
  controlled in-process loopback endpoint. A same-LAN Mac endpoint returned
  `No route to host` before local-network consent could be automated, so this
  stage proves direct socket ownership and loopback semantics, not local-LAN
  or public-network reachability;
- ordinary foreground-to-background delivery is observed, the deliberately
  retained UIKit background assertion reaches its expiration handler, and an
  uncatchable process kill followed by launch detects the armed durable fact
  and reruns app-owned plus restored-bookmark storage cleanly; and
- `BGContinuedProcessingTask` registration succeeds and one harness-submitted
  finite ten-second Rust storage/check run continues after backgrounding and
  completes with progress. Submission from an actual UI tap is not claimed.

Two physical XCTest attempts installed and launched the UI-test runner but
timed out while enabling Apple's automation mode. Therefore the real
`UIDocumentPickerViewController` selection of a separate **On My iPhone**
folder, first-access scope behavior for that selection, user-tap provenance
for the continued task, and system-UI cancellation remain unproven. The
bookmarked fixture result is intentionally labeled app-owned and does not
stand in for external File Provider support.

The architectural result is still decisive: current iOS can run the real Rust
pool, payload I/O, hashing, durability calls, namespace operation, and direct
sockets in-process, and coordinated bookmark restoration can enclose a Rust
operation without Swift payload callbacks. The common seam should retain an
opaque root capability and bounded coordination lifetime; it must not claim
external-provider support until the picker case is physically exercised.

### Stage 3: backend-neutral observation contract

`StorageFileReference` now observes an exact path or platform artifact without
opening or creating it. The four-field value reports existence, object kind,
file length, and an optional opaque token; constructors and broker completion
reject internally inconsistent values and tokens above 256 bytes. Path
observations use no-follow metadata, classify symlinks and special objects as
wrong-kind candidates, and derive a bounded modification token only when the
host supplies one. These facts remain disqualifying evidence rather than
content proof.

The platform request is now an explicit open/observe/delete operation instead
of an open request plus a deletion flag. UniFFI carries typed SAF observation
values and the expanded failure vocabulary. Kotlin resolves and queries the
exact document without eager creation, hashes provider document identity and
available metadata into a bounded opaque token, and never exports a document
ID, URI, path, or descriptor as observation state. The real two-ABI Android
build, generated bindings, APK assembly, and JVM tests pass with this contract.
Engine fixtures prove missing path observation creates nothing, platform
observation consumes no pool handle, and cancellation/open limits remain
bounded. Root admission and common published reads intentionally follow in the
next stages; no fast-resume decision consumes the new value.

## Staged Implementation And Intermediate Gates

1. **Freeze current behavior.** Add task-free comparison fixtures for path and
   fake-platform layout, opening, observation absence, namespace transitions,
   upload eligibility, and cancellation. Re-run the pinned and JSTorrent
   source survey and record any changed paths.
2. **Front-load the physical iOS boundary spike.** Install the pinned Rust iOS
   targets, build the minimal harness around current Rust file-pool/positional
   operations, and run app-owned plus selected-local-provider bookmark,
   coordination, direct TCP/UDP, and background/force-close probes on the
   paired iPhone. Record negative results before fixing the common seam so they
   can change it. The simulator may shorten iteration but does not gate this
   stage.
3. **Extract the narrow seam.** Using path, SAF, and the physical iOS findings,
   move immutable artifact geometry and logical observations out of path-
   specific/write-side ownership. Preserve the existing path download,
   selection, checkpoint, publication, removal, and seeding suite before
   changing Android behavior.
4. **Close Android observations and root health.** Extend the existing broker,
   add typed observation/failure responses, prove no eager materialization,
   and pass deterministic plus AVD grant/provider failure profiles.
5. **Unify published reads.** Make seeding consume common logical published
   content, remove the platform-root rejection, and prove simultaneous
   download/check/upload pool accounting and lifecycle fencing.
6. **Converge namespace transitions.** Reuse common task-free transition
   inputs/outcomes for path and SAF publication, repair, and removal; isolate
   or remove descriptor-manifest production backing.
7. **Run the Android product matrix.** Cross-build both ABIs, regenerate and
   compile bindings when changed, run JVM/instrumented tests, then AVD and
   explicitly authorized physical storage/lifecycle profiles.
8. **Re-run physical iOS through the final seam.** Development-sign and install
   the reconciled harness on the paired iPhone, repeat internal and selected
   local-provider roots plus TCP/UDP and lifecycle cases, remove probe files,
   and record which early findings were preserved or resolved without device
   identifiers.
9. **Reconcile truth.** Update the tactical execution record, owning topics,
   queue, support claims, limits, and next boundary. Remove temporary builds,
   logs, captures, test payloads, and installed probe data when practical.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Artifact geometry, safe logical identity, observation comparison, unsupported fields, namespace generations, publication/removal decisions, stale completion, upload availability |
| Engine runtime | Path and fake-platform reads/writes/hash/recheck/checkpoint/upload, pool eviction, wrong kind/length, missing content, pause/repair/removal races, exact task/handle cleanup |
| Session persistence | Root availability, publication/checking/complete transitions, restart at every durability boundary, no false have or seeding eligibility, quarantined torrent containment |
| Controlled interop | Exact multi-file payload seeded from path and platform storage to pinned libtorrent and independently SHA-1 verified; shared pool and upload accounting retained |
| Android build | Both established Rust targets, UniFFI/Kotlin generation or byte-identical check, `assembleDebug`, JVM/unit tests |
| Android runtime | API 34+ no-window AVD full matrix plus authorized physical local-SAF rerun; exact provider/request/descriptor high water and cleanup |
| iOS build | Host tests, both required simulator/device Rust targets, simulator harness, development-signed physical build |
| iOS physical | App-owned and selected On My iPhone roots, bookmark relaunch, coordinated I/O, SHA-1, namespace operations, direct TCP/UDP, foreground/background/expiration/force-close, exact cleanup |
| Repository | Rust baseline, generated client checks affected by contract changes, Android gates, documentation links, and `git diff --check` |

The implementation record lists exact commands, device classes and OS
versions without stable identifiers, repetitions, transferred bytes, hashes,
high-water marks, failures, and deliberate omissions. A simulator does not
substitute for the physical iOS row. A physical success with an app-owned
directory does not establish external File Provider support.

## Escalation And Device Authorization

Ordinary private refactoring, adding exact adversarial tests, choosing names,
tightening bounds, adding the minimal iOS experiment, installing Rust target
components, and fixing same-boundary defects are within the tactical once its
implementation is authorized.

That implementation authorization also permits build/install/launch/log
capture and non-destructive temporary file/network probes on the paired test
iPhone, Android AVD, and separately authorized owned Android hardware required
by this matrix. Do not change signing accounts, distribution records, cloud
provider data, or unrelated device content. Probe files remain within an
explicitly selected empty test directory and are removed after evidence is
captured.

Stop for direction if evidence requires payload callbacks through Swift or
Kotlin, a generic filesystem/VFS dependency, a second process or daemon,
cloud/third-party provider support, a product minimum-iOS/version or
distribution decision, an entitlement with meaningful release consequences,
destructive user-data handling, or a persistence contract beyond the bounded
observation envelope.

An unavailable device, signing failure, revoked grant, provider refusal,
ordinary test failure, or negative background result is not permission to
weaken the gate or claim support. Diagnose and record it; escalate only when
progress needs authority outside this tactical.

## Deliberate Non-Goals And Next Boundary

This tactical does not implement:

- evidence-based fast resume or any payload-hash skip policy;
- a full iOS product, shared web/SwiftUI presentation, settings UI,
  notification design, packaging, notarization, AltStore, or release;
- indefinite iOS background downloading or seeding, older-iOS compatibility,
  or a claim that `BGContinuedProcessingTask` is suitable for an entire
  torrent session;
- iCloud Drive, offloaded files, third-party File Provider, general removable
  Android storage, or Android multi-root UI support;
- relocation, import/adoption of arbitrary existing payload, streaming
  priorities, file serving, v2/hybrid hashing, or new protocol breadth;
- a general VFS, filesystem trait inventory, path abstraction for its own
  sake, native host, companion service, REST/WebSocket file proxy, or separate
  I/O daemon; or
- identical desktop, Compose, and eventual iOS presentation.

After this tactical closes, the next storage-facing slice may implement
evidence-based fast resume using the now-coherent clean epoch, storage
generation, root identity, and file observations. Clean eligible torrents may
skip payload hashing; every mismatch or unsupported observation falls back to
the common full checker; Force recheck always hashes. Android uses the same
decision from its supported observations rather than being deferred, while a
future iOS product adopts it only after its persistence and lifecycle owner is
implemented and validated.
