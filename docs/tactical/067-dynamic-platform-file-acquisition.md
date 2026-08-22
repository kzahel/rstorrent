# Tactical 067: Dynamic Platform File Acquisition

Status: Implemented and validated on the API 34 AVD (2026-08-03). Physical
ChromeOS validation was authorized and attempted, but ARCVM ADB was
disconnected; this is a recorded evidence gap rather than an implementation
blocker.

Topics: `android-saf-storage`, `client-persistence`, `download-correctness`,
`download-roots`, `storage-throughput-architecture`, `capability-readiness`

## Review Decisions

The maintainer accepted the following decisions before implementation:

- replace Android's complete startup descriptor manifest with per-file,
  asynchronous capability acquisition on cache misses;
- use one session-wide Rust file pool for path and SAF storage, limited to 40
  actual Rust-owned storage descriptors rather than 40 logical cache entries;
- use one `Arc<std::fs::File>`-backed positioned-I/O handle per open file and
  run blocking open, sync, and close work on the existing storage blocking
  boundary;
- bound Android acquisition to four provider calls in flight and sixteen
  queued requests, with a 30-second request deadline and same-key
  single-flight before the platform boundary;
- keep all payload reads, writes, hashes, materialization, part-slot handling,
  and durability in Rust; Kotlin resolves provider documents and lends one
  descriptor only on a miss;
- keep the existing joined publication/removal state machine, but replace
  descriptor-bearing confirmation with namespace-only acknowledgement and
  dynamic Rust reopen/verification; and
- make AVD evidence the stopping condition. The maintainer later also
  authorized physical ChromeOS/Android validation when a device was available.

## Motivation

Android SAF storage is currently proven only through a fixed descriptor
manifest: after metadata arrives, Kotlin eagerly creates the part document and
every initially wanted payload document, opens all of them, and hands the
entire set to Rust. Rust then retains two descriptors per wanted file. This
scales with metainfo file count, creates artifacts before I/O needs them, and
cannot support live `Skip`/`Normal` changes when a previously skipped document
has no descriptor.

The product needs the ordinary filesystem model at the capability boundary:
opening a logical file should not create it; a read/recheck should open only an
existing document; a write should create the destination only when routing
actually needs it; and inactive files should consume neither a permanent
descriptor nor Kotlin payload work. A torrent with hundreds of files, and a
future session seeding hundreds of such torrents, must be bounded by its active
working set rather than total file count.

This slice replaces the manifest with dynamic acquisition while preserving one
shared Rust selective-storage implementation. It also establishes the file
pool that future seeding reads must reuse.

## Desired Outcome And Stopping Condition

A path-backed or SAF-backed storage operation resolves a stable logical file
key through one session-wide Rust pool. Cache hits perform no platform call.
On an SAF miss, Rust asynchronously asks the existing same-process Android
foreground service for one document capability, duplicates the returned
descriptor, then performs all payload I/O in Rust. Reads never create missing
documents; the first routed write creates the exact wanted payload or part
document.

The tactical stops when all of the following are true:

- product startup, resume, and live selection no longer build or require a
  descriptor manifest proportional to torrent file count;
- path and SAF storage share one 40-descriptor application-service pool whose
  permits remain charged until the last in-flight handle reference is dropped;
- SAF request delivery is asynchronous, bounded, cancellable, and independent
  of the `AndroidApplicationClient` service mutex;
- metadata-only and read-existing flows create no payload, staging, or part
  artifacts;
- live `Skip`/`Normal`, boundary-piece routing, lazy part creation,
  materialization, empty-part deletion, restart, publication, root repair,
  removal, and joined shutdown work through dynamic acquisition;
- deterministic tests prove LRU, single-flight, mode compatibility,
  generation invalidation, cancellation, provider failure, and exact
  descriptor accounting;
- controlled path and Android AVD multi-file/multi-torrent runs complete with
  exact hashes while recording pool hit/miss/eviction counts, provider latency,
  pending-request high water, Rust-owned descriptor high water, and process FD
  high water; and
- the Rust workspace, Android unit/instrumented build, and relevant controlled
  interoperability and performance gates pass with all temporary artifacts
  removed.

The tactical is not complete merely because Kotlin can open one file on
demand. The shared resource owner, cancellation/restart behavior, sparse file
selection, namespace transitions, and observed bounds are part of the slice.

## Scope

- Add a concrete session-wide storage-file pool and migrate path-backed
  selective storage to it before adding the SAF source.
- Replace `SelectiveStorage`'s retained per-file handle array with logical
  routes and acquired handles scoped to immutable positional jobs and
  durability epochs.
- Add runtime-independent platform storage request/result types and a bounded
  asynchronous broker client in Rust.
- Expose Android-only UniFFI methods to await a platform request and complete
  it with either a borrowed descriptor or a typed failure.
- Add one Kotlin broker loop owned by `ProductEngineService`, performing
  provider work on `Dispatchers.IO` and closing every `ParcelFileDescriptor`
  after synchronous Rust duplication.
- Open existing payload/part documents without creation for read, hash,
  recheck, resume, and publication verification.
- Create exact parent directories and the payload or part document only for an
  `OpenOrCreate` write request.
- Apply Tactical `063`'s `Normal`/`Skip`, retained-destination,
  materialization, and lazy-part semantics to SAF storage.
- Fence and invalidate cached handles for publication, root repair, managed
  removal, torrent-generation replacement, and shutdown.
- Extend bounded structured storage diagnostics and Disk-view inputs with file
  pool and platform acquisition behavior.
- Add deterministic fake-broker tests, Android provider tests, controlled
  AVD evidence, and a resource report.
- Update the owning topics, readiness matrix, and this execution record as
  implementation lands.

## Non-Goals

- Seeding/upload implementation. Future seeding is required to reuse this
  pool, but upload scheduling and peer-wire work are not introduced here.
- Higher/lower/sequential/streaming file priorities, an add-dialog file tree,
  or any new file-selection UI.
- A general virtual filesystem trait, POSIX-over-SAF API, Kotlin payload cache,
  Java/Kotlin read/write bridge, native host, companion process, socket proxy,
  or storage daemon.
- Cloud-backed document providers, removable-media claims, or providers that
  have not proven seekable positioned I/O and durability.
- Torrent relocation, cross-root moves, multiple simultaneous profiles, or a
  per-torrent descriptor reservation/fairness policy.
- A user-facing advanced file-pool setting. The initial value is an internal
  named setting and diagnostic only.
- Part-file format changes, compaction, or migration of an existing destination
  into part storage when a file becomes skipped.
- A portable application command containing paths, SAF URIs, document IDs,
  descriptor numbers, or provider operations.
- Physical Android or ChromeOS behavior beyond the authorized validation run
  and required testbed workflow.

## Reference Dossier

### Normative behavior

- `reference/bittorrent.org/beps/bep_0003.rst` defines a multi-file torrent as
  one concatenated byte space. Complete boundary pieces still include bytes
  from adjacent skipped files.
- `reference/bittorrent.org/beps/bep_0047.rst` defines padding bytes as
  synthetic zeroes that need not be requested or stored.

No BEP defines descriptor pooling, Android capability acquisition, part-file
layout, or client file-priority APIs. Those are bounded client storage policy.

### Pinned libtorrent oracle

The required oracle is libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`, pinned by
`reference/pins.toml`.

- `include/libtorrent/settings_pack.hpp::file_pool_size` defines the
  session-wide open-file limit; `src/settings_pack.cpp` defaults it to 40 and
  `src/session.cpp::{min_memory_usage,high_performance_seed}` selects 4 and
  500 for those explicit profiles.
- `include/libtorrent/aux_/file_view_pool.hpp` and
  `src/file_view_pool.cpp::open_file` implement the shared
  `(storage_index,file_index)` LRU, hit promotion, compatible-open
  single-flight, read/write upgrade, eviction, resize, and release. File
  destruction occurs after releasing the pool mutex and an in-flight
  `shared_ptr` keeps an evicted view alive.
- `src/mmap_disk_io.cpp` owns one file pool for all storage instances and
  reports hits, misses, stalls, races, and current size.
- `simulation/test_file_pool.cpp::file_pool_size` downloads a 144-file torrent
  with a configured limit of five and proves the observed retained-file high
  water does not exceed five.
- `src/mmap_storage.cpp::{need_partfile,set_file_priority}` and
  `src/part_file.cpp` make part ownership lazy, open its backing file per
  operation, create directories only for a write, export promoted data, and
  remove the artifact after the final slot is released.
- Tactical `063` records the relevant libtorrent priority and part-file tests
  for live selection and boundary pieces.

RSTorrent adopts the ownership, resource, and observable selection semantics.
It does not adopt libtorrent's C++ object layout, multi-index container,
memory-mapped backend, disk-job API, settings surface, or resume format.

### JSTorrent Android history

The local JSTorrent sibling was inspected at
`9895410beeed6aff554053769bd006a3fbd373ef`.

- `packages/engine/src/adapters/native/native-filesystem.ts` represents an
  open file by root key and logical path without eagerly creating a document.
- `packages/engine/src/adapters/native/native-file-handle.ts` defines the file
  operations used by the engine.
- `android/quickjs-engine/src/main/kotlin/com/jstorrent/quickjs/bindings/FileBindings.kt`
  resolves root keys and bridges actual file operations.
- `android/io-core/src/main/java/com/jstorrent/io/file/FileManagerImpl.kt`
  creates missing parents/documents on a write but not a read, defaults to a
  32-handle pool, uses `Os.pread`/`Os.pwrite` with a `FileChannel` fallback,
  and serializes per-path and per-directory provider operations.
- `packages/engine/src/core/parts-file.ts` checks existence without creation,
  creates the info-hash-named part artifact on its first nonempty flush, and
  deletes it when empty.
- `android/io-core/src/androidTest/java/com/jstorrent/io/file/FileManagerSafTest.kt`
  and `FileManagerConcurrencyTest.kt` cover nested creation, positioned round
  trips, deletion, and concurrent directory creation.

JSTorrent proves that dynamic SAF lookup and bounded descriptor reuse are
practical and records provider-specific traps. RSTorrent intentionally does
not adopt its per-read JavaScript/Kotlin payload bridge; only capability
acquisition crosses the platform boundary.

### Existing RSTorrent boundary

- `crates/rstorrent-engine/src/selective_storage.rs` stores
  `Vec<Option<RetainedFile>>`; each `RetainedFile` owns a Tokio control handle
  and a cloned positional handle. Descriptor create/resume requires a complete
  selected-file manifest and descriptor mode cannot lazily acquire a part or
  materialization destination.
- `crates/rstorrent-android/src/lib.rs::duplicate_descriptor` correctly uses
  `F_DUPFD_CLOEXEC`, but `AndroidApplicationClient::start_saf` receives the
  entire borrowed manifest while holding its application-service access path.
- `clients/android/.../ProductSafDocuments.kt::openStaging`
  eagerly creates every wanted document and two part descriptors.
- `ProductEngineService::advanceSaf` reacts to coarse storage/publication/
  removal phases and owns the persisted tree grant and provider lifecycle.
- `crates/rstorrent-session/src/store.rs` currently rejects live selection for
  non-path storage because a new file cannot be acquired.
- Tacticals `005`, `009`, `052`, `053`, `054`, `062`, and `063` already prove
  descriptor duplication, durable SAF phases, durability epochs, immutable
  positional plans, bounded storage work, publication, and path-backed live
  selection. This tactical changes the acquisition/lifetime boundary without
  weakening those contracts.

## Architecture And Dependency Direction

The concrete dependency direction is:

```text
protocol layout / FileSelection
               |
               v
rstorrent-engine selective routing + StorageFilePool
               |                    |
               |                    +--> PathFileSource
               |                    +--> PlatformFileSource (typed channel)
               v
rstorrent-session ApplicationService owns one pool and broker endpoint
               |
               v
rstorrent-android UniFFI request/completion bridge
               |
               v
ProductEngineService + ProductSafDocuments (SAF namespace only)
```

`rstorrent-engine` owns the pool, logical keys, open intent, acquired handle,
and platform request types because selective storage consumes them. These
types may depend on Tokio channels at the engine runtime boundary, but pure
torrent-coordinate mapping and `FileSelection` remain independent of Tokio,
files, channels, Android, and session state. The engine must not depend on
`rstorrent-session`, `rstorrent-android`, UniFFI, Kotlin, `Uri`, or
`ParcelFileDescriptor`.

`ApplicationService` constructs exactly one `StorageFilePool` for its profile
and supplies clones to path and platform torrent generations. The current
single-active-torrent product policy is unchanged; making the pool
session-wide now prevents a per-torrent ownership mistake before concurrent
execution or seeding exists.

The platform source is a narrow concrete capability source, not a speculative
filesystem abstraction. Namespace rename, delete, and provider observation
remain typed Android lifecycle operations; they do not become arbitrary
engine filesystem calls.

## Logical Identity And Open Contract

The pool key is conceptually:

```text
StorageFileKey {
  storage_instance_id,
  namespace_generation,
  role: Payload(file_index) | Part,
}
```

`storage_instance_id` is stable for one torrent's managed content and cannot
be a display name. `namespace_generation` changes when staging becomes
published, a root is repaired/rebound, managed content is replaced, or another
namespace transition can change what the key names. Generation is transient;
the durable root ID and existing storage/publication phase reconstruct it on
restart, when no old process handles survive.

The source receives validated path components and one explicit disposition:

- `OpenExisting`: read/hash/recheck/publication verification; missing is a
  typed absence and creates no parent or file;
- `OpenExistingReadWrite`: acquire an existing managed destination for future
  writes without creating it; and
- `OpenOrCreate`: create exact missing parents and the file only after a write
  or materialization plan actually needs it.

Access compatibility is part of the cached entry. A read-only entry satisfies
only `OpenExisting`; a read/write entry satisfies all non-creating access for
the same key. An upgrade removes the insufficient cache entry, waits for its
last incompatible in-flight reference if necessary, and single-flights one
replacement open. No caller may retain an old handle while synchronously
waiting for its own upgrade.

Path and SAF sources must return the same `Arc<std::fs::File>`-backed owned
handle. Positioned read/write, length validation, sync, and hash use that one
descriptor. Any operation requiring a blocking syscall runs on the existing
bounded blocking storage lane; a second cloned control descriptor is not
retained.

## File-Pool Ownership And Bounds

`ApplicationService` owns the pool and its shutdown. The initial internal
setting is `40` actual Rust-owned storage descriptors per profile/service,
matching libtorrent's ordinary default. It is not multiplied per torrent,
root, payload role, or auxiliary file.

The bound is enforced by permits attached to handles, not cache entries:

- acquiring one owned descriptor consumes one permit;
- a cache hit clones an `Arc` and consumes no additional permit;
- eviction drops the cache reference but does not release the permit while an
  I/O job or durability epoch retains the handle;
- the handle returns its permit only when its final `Arc` is dropped and its
  descriptor closes; and
- a miss waits cancellably for a permit after evicting eligible LRU entries.

This distinction is required to make “40” an actual Rust-owned descriptor
bound even when an evicted file is still in use. The process still needs a
separate reserve for sockets, SQLite, logs, and runtime internals. The four
temporary Kotlin `ParcelFileDescriptor` values are measured independently and
closed immediately after each response. Controlled evidence must report both
the pool-owned and whole-process descriptor high water.

Pool behavior is:

- one global LRU across path and platform torrents;
- every hit moves the entry to most recently used;
- compatible concurrent misses share one acquisition result;
- capacity pressure removes least-recently-used cache references until a
  permit can become available;
- closes and blocking destruction occur outside the pool-state mutex;
- cancellation of one waiter does not cancel an acquisition still needed by
  another waiter;
- an acquisition with no remaining waiters may cancel before insertion;
- one `EMFILE`/`ENFILE` result invalidates eligible idle entries and retries
  once after a permit/close fence; a repeated result fails observably; and
- shutdown refuses new acquisitions, cancels queued requests, joins open work,
  drops cache references, awaits outstanding handle permits, and proves the
  owned count returns to zero.

There is initially no per-torrent quota. Starvation, low hit rate, or root
latency evidence may justify a later policy tactical; total metainfo file
count does not justify pre-opening.

## Platform Request Contract

The Rust broker uses a bounded channel of 16 request envelopes and a semaphore
of four active provider calls. Same-key single-flight happens before enqueue,
so duplicate storage jobs do not consume platform queue capacity. Request
variants are limited to opening one logical file and deleting the exact owned
part artifact after its Rust handle fence. Whole-torrent publication and
managed removal retain their existing coarse lifecycle operations.

An open request contains only:

- a monotonic request ID;
- service/torrent generation and namespace generation;
- stable root ID and storage instance ID;
- trusted role (`Payload(file_index)` or `Part`);
- bounded, already validated relative path components;
- disposition/access intent; and
- a 30-second request deadline.

Results are a borrowed descriptor or a closed typed outcome such as missing,
grant unavailable, name collision/deduplication, provider refusal,
non-seekable, cancelled, deadline exceeded, or internal failure. Provider and
path details are bounded and redacted before diagnostic storage.

`AndroidApplicationClient` must keep request delivery separate from
`service: AsyncMutex<Option<ApplicationService>>`. The intended bridge has an
async `next_platform_storage_request`-style method and a completion method
keyed by request ID; exact generated names may follow UniFFI conventions. A
wait for the next request never holds the service mutex, because engine work
inside that service may be waiting for Kotlin to complete the request.

`ProductEngineService` owns one broker coroutine for the client lifetime. It
uses `Dispatchers.IO`, limits provider work to four calls, and resolves the
current persisted tree grant at execution time. Provider queries and opens use
an Android `CancellationSignal` where the API permits it. A provider that
cannot meet the deadline and cancellation contract is not promoted into the
supported local-provider set. For an open result:

1. Kotlin locates or creates the exact canonical document as requested.
2. Kotlin opens one `ParcelFileDescriptor` in a compatible seekable mode.
3. Kotlin completes the request with its borrowed numeric descriptor.
4. Rust synchronously duplicates it with close-on-exec before accepting the
   response.
5. Kotlin closes the original in `finally`.

Completion for an unknown, cancelled, expired, or replaced request duplicates
nothing and Kotlin closes the original. Rust inserts no entry until
duplication, seekability/mode validation, generation validation, and permit
ownership all succeed.

The 30-second deadline bounds a local-provider call without adding retries
that can duplicate namespace work. The adapter signals cancellation at the
deadline and awaits the bounded child termination. A provider operation that
nevertheless finishes late has its result discarded and closed and is
classified as unsupported/failed evidence rather than silently detached. Only
the resource-exhaustion retry described above is automatic. Any evidence that
an otherwise supported local provider regularly needs a longer deadline
requires review before changing the default.

## Owner, Task, And Cancellation Map

| State or work | Owner | Termination / cancellation |
| --- | --- | --- |
| Pool entries, LRU, permits, single-flight table, metrics | One `StorageFilePool` owned by `ApplicationService` | Service shutdown closes admission, cancels acquisitions, drops entries, joins opens, and awaits zero owned handles. |
| One path open | The single-flight acquisition future | Cancellable while queued; once blocking open starts, result is closed if no valid waiter/generation remains. |
| One platform request | `PlatformFileSource` request table entry | Torrent/service cancellation removes the waiter and sends cancellation; timeout or late completion cannot insert. |
| Kotlin request delivery | `ProductEngineService` broker coroutine | Service scope cancellation stops intake, cancels provider children, closes every PFD, and reports terminal completion to Rust. |
| Provider open/create | One `Dispatchers.IO` child, maximum four | Cancellation is best effort around provider calls; any late URI/PFD is discarded/closed. |
| Positional read/write/hash | Existing bounded Rust storage job owner | Job holds one `Arc`; generation cancellation joins it before namespace mutation. Eviction cannot close its handle. |
| Durability epoch | Existing checkpoint owner | Retains each dirty handle until payload sync and verified-state commit complete. |
| Per-file physical route and part slots | `SelectiveStorage` generation | Immutable positional plans carry routing generation; replacement joins all jobs before route or namespace changes. |
| Publication/repair/removal fence | `ApplicationService` torrent lifecycle | Stops admission, joins jobs/checkpoint, invalidates matching pool keys, waits for handles, then permits Kotlin namespace work. |

No detached task, additional Android service, process, socket, or daemon is
introduced. Every spawned task has a named owner, cancellation input, and
observable joined termination.

## Selection, Part File, And Conservative Recheck

The path behavior proven by Tactical `063` becomes backend-independent:

- `Normal` to `Skip` changes future demand and routing but does not delete,
  truncate, rename, or import an existing destination into part storage. Its
  handle may remain cached or age out normally.
- `Skip` to `Normal` does not eagerly create the destination. The first
  verified-span materialization or payload write issues `OpenOrCreate`, copies
  exact verified spans in Rust, syncs them under the ordinary durability
  epoch, then releases no-longer-needed part slots.
- A boundary piece remains wanted and is verified in full when any overlapping
  non-padding file is wanted.
- Construction, metadata-only state, resume, and recheck do not acquire or
  create a part document. The first actual part-routed write does.
- The part handle uses the same pool and 40-descriptor budget. Releasing the
  final slot fences dirty work, invalidates/drops the part handle, waits for
  its last reference, then asks Kotlin to delete the exact engine-owned part
  document. Absence is idempotent; an unexpected identity is not deleted.
- Recheck uses `OpenExisting`. Missing, short, corrupt, inaccessible, or
  replaced sources clear unsupported have claims and become ordinary
  redownload/wait-for-root state. They never create empty evidence.
- Zero-length wanted files may be represented in publication layout only when
  publication requires them; they do not consume a long-lived handle or cause
  eager parent-tree creation during metadata/recheck.

The existing sparse durable file selection remains the authority. Descriptor
manifest rows and numeric descriptor values are never added to SQLite.

## Publication, Restart, Repair, And Removal

The durable two-phase SAF lifecycle from Tactical `009` remains authoritative.
Dynamic acquisition changes its descriptor mechanics:

1. Rust stops new storage admission for the current namespace generation.
2. It joins all positional jobs and the required durability checkpoint.
3. It invalidates affected pool entries and waits for their handle permits.
4. Existing Kotlin code performs the exact staging-to-final provider rename.
5. Kotlin acknowledges only the namespace operation ID and outcome; it does
   not reopen and return a complete published descriptor manifest.
6. Rust advances the namespace generation and dynamically opens each wanted
   published file with `OpenExisting` for exact length/hash confirmation.
7. Only successful fresh verification permits the existing durable complete
   transaction.

Crash-before-rename, crash-after-rename, and retry behavior retain the durable
states already proven by Tactical `009`. A process restart begins with an
empty pool and reconstructs all capabilities dynamically.

Root repair preserves the stable root ID but invalidates all entries tied to
the former grant/locator before work resumes. Grant loss cancels outstanding
requests and leaves torrents waiting for repair; it never falls back to a path
root. Managed removal likewise waits for relevant handles to close before
Kotlin deletes staging, part, or published documents, then acknowledges the
existing operation ID.

## Required Invariants And Shape-Changing Cases

- Metainfo path and file-count bounds remain the existing parser bounds. Every
  platform component is revalidated at the Kotlin boundary and cannot contain
  separators, traversal, empty names, or provider-reserved aliases.
- Provider lookup must find zero or one exact canonical child. Silent
  deduplication to `name (1)` or another sibling is a collision, not success.
- Read-existing never calls provider create APIs, even after missing lookup.
- At most 40 Rust-owned storage descriptors exist for the service, including
  handles evicted from the LRU but retained by in-flight jobs.
- At most four Kotlin PFDs are in provider/open handoff work and at most 16
  request envelopes are queued. Queue saturation applies backpressure; it
  never allocates another queue.
- Descriptor duplication, seekability validation, generation validation, or
  mode validation failure leaks neither side's descriptor and inserts no
  cache entry.
- A cache mutex is never held across filesystem, provider, channel send,
  await, sync, close, or callback work.
- Same-key compatible callers observe one acquisition result. Read/write races
  either share a sufficient handle or serialize a single upgrade; they cannot
  insert two live entries for one generation/key.
- Eviction cannot cancel or close an active read, write, hash,
  materialization, or durability sync.
- A cancelled request and every late response are terminal for that request
  ID. A response from an older torrent or namespace generation cannot mutate
  replacement state.
- Provider revocation/external disappearance invalidates the affected entry or
  root and clears unsupported verified claims conservatively.
- Publication, repair, removal, torrent replacement, and shutdown wait for the
  exact affected handle generation before provider mutation.
- No path, URI, document ID, descriptor number, or payload byte enters
  portable commands, SQLite torrent records, browser views, or unredacted
  diagnostics.

## Persistence And Contract Impact

No SQLite schema migration is expected. Existing durable torrent ID, stable
root ID, file selection, verified pieces, storage phase, prepared publication
evidence, and removal operation ID are sufficient. Namespace generation,
request IDs, open handles, LRU state, and pool metrics are runtime-only.

No portable application command or web contract is added. Android UniFFI and
generated Kotlin bindings gain an internal request/result contract and the old
descriptor-bearing `startSaf`/publication confirmation path is retired from
the product flow. Diagnostic proof helpers may remain only if clearly named
and bounded; no supported product path may construct the old full manifest.

If implementation discovers that a durable identity or state transition is
actually missing, stop for review before adding a migration. Do not smuggle a
provider URI or descriptor manifest into the session database as a shortcut.

## Observability

Add bounded structured metrics at the shared storage owner for:

- configured limit, current, and high-water Rust-owned storage descriptors;
- cache hits, misses, evictions, permit waits, and wait duration;
- single-flight leaders/waiters, cancelled waiters, races, and mode upgrades;
- current/high-water queued and active platform requests;
- provider open-existing/open-create counts, latency distribution, typed
  outcomes, timeouts, and late responses per stable root ID;
- part acquisitions, cache hits, last-slot invalidations, and delete outcomes;
- namespace/root invalidations and handles awaited by each fence;
- `EMFILE`/`ENFILE` retry and terminal exhaustion; and
- shutdown request/handle drain counts.

Disk-view diagnostics may expose bounded aggregate values. They must not
expose paths, SAF URIs, document IDs, raw descriptors, or provider error
chains. Resource evidence records pool-owned descriptors separately from
whole-process descriptors so a temporary platform handoff cannot be hidden.

## Staged Implementation And Logical Commits

Implementation began after maintainer authorization. The planned slices landed
as independently reviewable commits and each left the workspace green:

1. **Pool core and one-descriptor path handles.** Add logical keys, handle
   permits, LRU, single-flight, cancellation, metrics, and deterministic tests;
   migrate path selective storage and durability epochs to the common handle.
2. **Platform request source.** Add bounded request/result plumbing, fake
   broker, generation/deadline/error behavior, and session ownership without
   Android code. Prove no service-lock reentrancy.
3. **Android dynamic acquisition.** Add UniFFI/Kotlin broker flow, exact
   provider lookup/create behavior, PFD ownership tests, and remove product
   startup manifests/eager part creation.
4. **Selection and namespace parity.** Enable SAF live `Skip`/`Normal`, lazy
   part creation/deletion, dynamic publication confirmation, restart, repair,
   and removal with crash/failure tests.
5. **Controlled evidence and resource tuning.** Run path and AVD multi-file/
   multi-torrent cases, measure provider concurrency and descriptor/process
   high waters, and change the proposed `4`/`16` bounds only if recorded
   evidence and maintainer review justify it.
6. **Execution record.** Update this tactical, `android-saf-storage`, storage
   throughput/readiness topics, exact validation output, remaining gaps, and
   logical commit hashes.

Implementation commits must use subjects of at most 65 characters and wrap
bodies at 72 columns. Commits that materially advance the continuing concern
carry `Topic: android-saf-storage`.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure pool | LRU hit promotion and eviction; 40 actual permits across at least 100 storage identities and 10,000 logical files; in-flight eviction retention; same-key compatible single-flight; mode upgrade; cancelled leader/waiter; generation invalidation; queue saturation; one resource retry; joined zero-handle shutdown. |
| Selective storage | Path and fake-platform all-normal/all-skipped/boundary/padding/final-short cases; retained skipped destinations; promotion from part spans; no artifact on construction/read/recheck; first-write payload/part creation; final-slot part deletion; zero-length files; conservative missing/corrupt source handling. |
| Scripted runtime | Delayed/refused/non-seekable/revoked fake provider; request timeout and late descriptor; torrent replacement during acquisition and during I/O; publication/repair/removal fence; process-style restart with an empty pool; no descriptor leak on every injected failure. |
| Android unit/provider | Exact child lookup and dedup collision; concurrent parent creation; open-existing absence; open-or-create; nested paths; PFD duplicate/close ownership; four-active/16-queued bounds; service-scope cancellation; generated Kotlin compilation. |
| Controlled path | A deterministic multi-file transfer and restart through the shared pool, plus the retained engine performance smoke. Exact payload hashes, publication, cleanup, pool counters, FD high water, and no material regression against the declared Tactical `057` floors. |
| Controlled AVD | Metadata-only no-artifact proof; at least three simultaneous logical storage identities with at least 100 files each and a working set exceeding 40; live Skip/Normal and boundary materialization; interruption/restart; publication; grant-loss/repair; managed removal; exact hashes; descriptor and pending-request high waters. Network execution may remain serialized if the current application scheduler is single-active, but the storage-pool harness must interleave all identities. |
| Repository | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, Android Rust target checks, Gradle unit/instrumented tests, generated-binding checks, and documentation link/diff checks. |
| Physical hardware | Authorized but not required for completion. The ChromeOS testbed doctor passed 9/9 checks; ARCVM ADB connection was refused, so no APK cycle ran. |

The AVD resource report must show that increasing torrent file count does not
increase startup descriptor count, owned storage descriptors never exceed 40,
provider calls never exceed four, queued requests never exceed 16, all counts
return to baseline after shutdown, and no payload buffer crosses Kotlin.

## Escalation Contract

Stop and request maintainer direction if implementation would require:

- a schema migration or new durable provider identity;
- a general filesystem trait or a second selective-storage implementation;
- an additional Android service/process, daemon, socket, or reentrant callback;
- Kotlin/Java payload reads, writes, hashing, caching, or part-slot logic;
- a public application/API or UI setting;
- cloud/removable provider support or a weakened seekability/durability claim;
- a descriptor limit above 40, provider concurrency above four, or queue above
  sixteen without controlled evidence;
- a physical-device action outside the authorization granted for this slice;
- a change to the accepted publication, file-selection, durability, or
  product-root semantics.

Ordinary internal type naming, module extraction, test-fixture construction,
and fixes necessary to satisfy the stated invariants do not require another
design pause once this tactical is authorized.

## Deliberate Deferrals

After this slice, the expected remaining work is seeding/upload reads through
the same pool, evidence-driven fairness if multiple active torrents expose
starvation, optional advanced setting exposure, additional provider classes,
and separately authorized physical Android/ChromeOS validation. None may add
another descriptor cache or bypass the session-wide owner.

## Implementation Record

The slice landed in four logical commits:

- `521b82e` records the accepted dynamic SAF design and oracle dossier;
- `bda612c` adds the bounded shared storage-file pool core;
- `3cebed2` migrates path and platform selective storage, the part file,
  durability, publication verification, selection, repair, removal, and
  shutdown to lazy pooled handles; and
- `9a86309` adds the UniFFI/Kotlin provider broker and product-path Android
  validation harness.

`ApplicationService` now owns one 40-permit pool for all path and platform
storage. A permit remains attached to the actual Rust-owned descriptor until
the last in-flight `Arc` drops, including after LRU eviction. The pool uses
stable storage/generation/role keys, compatible-open single-flight, mode
upgrade, generation invalidation, one resource-exhaustion retry, and joined
shutdown. `SelectiveStorage` and `PartFile` retain logical references and
acquire one handle only for an actual positioned read, write, hash, sync, or
delete operation.

The Android product no longer calls the legacy eager `startSaf` or
descriptor-bearing publication confirmation path. Four `Dispatchers.IO`
workers consume a bounded 16-envelope Rust channel, resolve the current
persisted tree, open or create exactly one canonical document, and lend one
`ParcelFileDescriptor`. Rust validates and duplicates it with close-on-exec;
all torrent-coordinate I/O remains in Rust. Read-existing never creates,
empty part storage is deleted idempotently, duplicate provider children are a
typed name collision, and publication crosses only the torrent name before a
fresh generation-scoped Rust verification.

Root replacement pauses the affected active torrent, invalidates all cached
capabilities, installs the new Kotlin locator, and resumes the former active
torrent. Publication, unavailable storage, and managed removal invalidate
the matching storage identity. An acquisition completing after invalidation
is discarded before cache insertion.

## Validation Evidence

Repository validation passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test -p rstorrent-engine --lib -q`: 185 passed, 3 ignored;
- `cargo test --workspace --exclude rstorrent-engine -q`, including 91
  `rstorrent-session` tests and 6 `rstorrent-android` tests;
- `clients/android/build.sh`: both locked Android Rust
  targets, UniFFI generation, debug APK assembly, and Kotlin unit tests; and
- Python bytecode validation for both Android runners.

Deterministic pool evidence opens 10,000 logical keys representing 100
torrent identities with 100 files each. The configured eight-handle test pool
reported an eight-descriptor high water, retained eight cache entries, and
evicted at least 9,992 entries. Separate tests cover compatible same-key
single-flight, read-to-write upgrade, in-flight eviction retention,
read-existing non-creation, stale completion after invalidation, late broker
completion, and zero-handle joined shutdown. The fake platform selective test
creates no provider artifact at construction, exercises payload and part
routing plus hashing, renames staging, and verifies every published file
through fresh dynamic opens.

Three fresh `product-dynamic-saf` cycles passed on the API 34 ARM64
`jstorrent-tablet` AVD. Each cycle acquired the persisted tree through the
system picker, obtained metadata from the controlled pinned-libtorrent peer,
downloaded through `AndroidApplicationClient` and the dynamic provider
workers, published the exact `fixture` directory, and SHA-1 verified every
non-padding file. Each rejected an info-hash output directory and proved the
staging namespace, padding file, and empty part artifact were absent after
publication. All test grants, files, APK state, reverse ports, seeds, and AVD
processes were removed.

The three pool resource records were identical: limit `40`, Rust-owned
descriptor high water `6`, and platform pending-request high water `3` of the
bounded `16`. Whole-process descriptor baselines were `106`, `107`, and `113`;
the observed high water and final count were `137` in each run. The largest
observed delta was `31`, within the 40 Rust-owned plus four temporary-provider
envelope.

Physical validation followed the required ChromeOS testbed workflow. The
testbed doctor reported nine passed checks and no failures, including SSH,
active user session, and writable rootfs. `chromeos adb-connect` then failed
with connection refused at `127.0.0.1:5555`; ARCVM ADB was unavailable, so the
physical APK cycle remains unclaimed.

## Deliberate Evidence Limits

The legacy fixed-descriptor APIs and Tactical 004/005 diagnostic harness
remain only as bounded historical proof infrastructure; the product service
does not call them. The new AVD profile proves the real all-Normal product
path, provider acquisition, publication, lazy empty-part behavior, exact
hashes, and resource bounds. Live Skip/Normal, interruption/restart,
grant-loss repair, and removal retain deterministic/session coverage and the
earlier durable SAF lifecycle evidence, but were not all repeated as separate
dynamic-provider AVD profiles in this slice. Per-provider latency histograms
and Disk-view exposure also remain diagnostic follow-up; the implemented
snapshot exposes pool hits, misses, evictions, failures, owned descriptors,
and pending-request high water without paths, URIs, IDs, or descriptors.
