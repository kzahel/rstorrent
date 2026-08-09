# Android SAF Storage

Topic: `android-saf-storage`

Status: Tactical
[`067`](../tactical/067-dynamic-platform-file-acquisition.md) implements
dynamic capability acquisition, one 40-descriptor session pool shared by path
and SAF storage, live Normal/Skip platform support, lazy payload/part
artifacts, and fresh dynamic publication verification. Three product-path API
34 AVD cycles pass with exact hashes and bounded Rust/process descriptor
evidence. The fixed startup manifest remains only in legacy diagnostic proof
APIs and is not an acceptable or used product architecture. A newly
authorized physical run was attempted, but ChromeOS ARCVM ADB was unavailable.
Tactical `073` now makes dynamic publication confirmation a durable
`published`/`checking` handoff; fresh published handles run through the common
piece checker before `complete` can commit.
Tactical `081` extends the shared catalog/storage boundary to
libtorrent-scale v1 metainfo: Android consumes compact selection and paged
file catalogs, derives the same deterministic safe operational paths, and
keeps provider requests, documents, and descriptors lazy. It does not add an
Android `.torrent` picker or document-intent intake.
Planned Tactical
[`116`](../tactical/116-platform-storage-coherence-and-ios-feasibility.md)
is now the prerequisite shoring slice: it adds platform observations and early
root health, reuses the shared logical published-content owner for SAF upload,
converges namespace outcomes, and makes applicable Android behavior a
non-deferrable completion gate for future engine tacticals.

## Scope

This topic owns the continuing Android Storage Access Framework boundary:

- persisted tree-grant and provider-document ownership;
- dynamic acquisition of seekable file descriptors from safe relative torrent
  identities;
- descriptor lifetime and session-wide resource bounds;
- the division between Android namespace work and Rust payload I/O;
- lazy payload, part-file, restart, and publication behavior beneath a SAF
  root; and
- provider failure, cancellation, observability, and validation requirements.

It complements:

- [`download-roots.md`](download-roots.md), which owns user-visible root
  selection, stable root identity, repair, and publication layout;
- [`client-persistence.md`](client-persistence.md), which owns the durable
  catalog, selected root, file intent, verified state, and restart authority;
- [`storage-throughput-architecture.md`](storage-throughput-architecture.md),
  which owns shared positional I/O, durability epochs, and session/root
  scheduling; and
- [`application-control.md`](application-control.md), which keeps platform
  capabilities and descriptor values out of portable presentation commands.

This topic does not implement general Android multi-root UI, cloud document
providers, torrent relocation, a general virtual filesystem, or a second
Android storage engine. SAF-backed seeding and bounded file observations are
planned specifically in Tactical `116` rather than implied by this topic
alone.

## Current State

Tacticals
[`003`](../tactical/003-android-storage-feasibility.md),
[`005`](../tactical/005-saf-selective-storage.md), and
[`009`](../tactical/009-android-saf-session-storage.md) established important
facts that remain in force:

- Android persists a user-selected SAF tree grant and the platform adapter
  owns its URI and provider operations.
- Supported local providers expose `ParcelFileDescriptor` values usable for
  fixed-buffer positional I/O in Rust.
- Rust synchronously duplicates a borrowed descriptor before Kotlin closes
  its `ParcelFileDescriptor`; payload buffers never cross the Kotlin boundary.
- Rust owns torrent-coordinate mapping, verification, selective part storage,
  durability, and prepared state.
- Kotlin owns provider document creation, deletion, and rename. Publication is
  not successful until the renamed documents are reopened and validated.
- Grant loss and provider refusal are availability failures, not evidence that
  metadata or verified client state is corrupt.

The product path now restores only durable root identity, selection, storage
phase, metadata, and conservative verified state. `ApplicationService` owns
one pool whose permits count actual Rust-owned handles, including evicted
handles still retained by immutable jobs. Kotlin resolves one exact document
per cache miss using four bounded provider workers and closes the borrowed PFD
after synchronous Rust duplication. `SelectiveStorage` and `PartFile` keep
logical references rather than retained manifests, so construction and
metadata-only work create no payload or part artifacts.

Tactical [`063`](../tactical/063-live-file-selection.md)'s `Normal`/`Skip`,
boundary routing, lazy-part, retained-destination, materialization, and
restart semantics now apply to platform storage through the same Rust
implementation. Higher and lower priorities remain deliberately absent from
the UI and this boundary.

## Accepted Architecture

### One shared Rust storage implementation

SAF is not a second implementation of selective torrent storage. The following
remain shared Rust behavior for path and Android destinations:

- safe metainfo-relative layout and torrent-coordinate mapping;
- piece picking and file selection;
- immutable positional read, write, and hash plans;
- part-file slot allocation and verified-span materialization;
- routing-generation fences;
- durability checkpoints and conservative recheck; and
- publication preparation and seeding reads when seeding is implemented.

The platform-specific difference is intentionally narrow: how a safe logical
file identity becomes one owned, seekable file descriptor, and how namespace
operations such as provider rename or deletion are performed.

Do not introduce an Android filesystem that reimplements piece placement, a
Kotlin payload cache, a native-host socket proxy, or a general-purpose
filesystem trait with operations not required by the proven storage lifecycle.

### Android owns namespace capability; Rust owns open handles

`ProductEngineService` is the current same-process lifecycle owner for the
persisted grant and SAF work. Dynamic acquisition extends that owner; it does
not require another Android service, process, or I/O daemon.

The Android adapter must provide a small concrete broker with these semantic
operations:

- open an existing document without creating it;
- open or create a document and its exact parent directories;
- remove an engine-owned document after Rust releases its handle;
- perform the existing fenced staging-to-publication rename; and
- resolve bounded provider observations needed for restart or repair.

The broker receives a stable root ID, torrent/storage identity, trusted role,
validated relative path components, access mode, and request generation. It
does not receive piece payload. Raw SAF URIs and descriptor numbers remain
inside the platform adapter and UniFFI handoff rather than portable commands,
SQLite torrent records, browser views, or remote transports.

On an open request, Kotlin resolves or creates the document through
`DocumentsContract`, opens a `ParcelFileDescriptor` off the Android main
thread, and supplies its borrowed descriptor with the request ID. Rust
duplicates it with close-on-exec before accepting the response. Kotlin then
closes its original; Rust owns and closes the duplicate.

The handoff is asynchronous and cancellation-aware. Exact UniFFI records and
delivery mechanics belong to the implementation tactical, but the boundary
must not invoke a reentrant Kotlin callback while holding an engine, session,
storage, or cache lock. A torrent-generation owner awaits the request, and a
late response to a cancelled or replaced generation is rejected and closed.

### Dynamic acquisition replaces startup manifests

Startup restores only durable root identity, verified metadata, selection,
storage phase, and conservative resume evidence. It never constructs a
descriptor manifest proportional to torrent file count.

An ordinary storage operation follows this sequence:

1. Rust maps the torrent range to a logical payload or auxiliary file.
2. The shared file pool looks up the stable key.
3. A hit returns a shared Rust handle without crossing into Kotlin.
4. A miss is single-flighted per key and access mode.
5. Path storage opens locally; SAF storage asks the Android broker for one
   descriptor.
6. Rust duplicates and inserts the handle, then performs positional I/O.
7. Eviction removes the cache reference, while in-flight jobs retain their
   shared handle until completion.

Reads, recheck, and verification use **open existing** and must never create an
empty destination. Writes use **open or create** only after the routing and
selection plan says the destination is wanted. This distinction prevents
startup or conservative recheck from eagerly materializing the torrent tree.

## Session-Wide File Pool

The file pool is owned by the application service's shared storage/disk owner,
not by one torrent. Its initial scope is the one active profile/service
instance. If simultaneous profiles are later authorized, a process-level owner
must bound their aggregate descriptors rather than multiplying this limit per
profile.

Keys identify both storage ownership and file role. Payload keys include a
torrent/storage identity, namespace generation, and file index. Auxiliary keys
include at least the selective part file. Publication or root repair advances
the namespace generation and invalidates entries that can no longer name the
same provider documents.

The pool follows the pinned libtorrent behavior:

- one global least-recently-used order across torrents;
- a hit moves the entry to most recently used;
- a miss at capacity evicts the least recently used entry;
- concurrent compatible opens of one key share one acquisition;
- a write request upgrades or replaces an insufficient read-only entry;
- an in-flight job retains an `Arc` even after cache eviction; and
- close and other potentially blocking destruction occurs outside the pool
  lock.

The initial design target is 40 open storage files per service, matching
libtorrent's ordinary default. It is an advanced runtime setting, not initial
UI. The implementation must count actual operating-system descriptors rather
than logical entries, retain a reserve for peer sockets and other process
resources, and report the observed descriptor high-water. The current
two-descriptor `RetainedFile` shape should be reduced to one underlying owned
descriptor per cached file when compatible with positional I/O and durability;
otherwise the limit must charge both descriptors explicitly.

The platform broker also needs a much smaller bound on simultaneous pending
opens so a burst of misses cannot transiently exceed the steady-state pool.
The implementation tactical must justify that value with controlled Android
evidence rather than treating every metainfo file as independent concurrency.

An entry participating in an uncommitted durability epoch cannot be made
unsafe by eviction. The checkpoint owner retains its shared handle until the
required payload sync completes and the corresponding verified-state commit is
allowed. Removing the LRU reference never cancels an in-flight read, write,
hash, materialization, or sync.

No per-torrent reserved quota or fairness layer is initially required. A
global LRU makes the working set, rather than torrent file count, consume the
budget. Hit/miss and provider-latency evidence can justify later root-aware or
fairness policy.

## File Selection And Part Storage

The SAF backend adopts the live selection semantics already proven for path
storage:

- `Normal` to `Skip` changes routing and future piece demand. It does not
  delete, truncate, rename, or move an existing destination into the part
  file. Its cached handle may age out normally.
- `Skip` to `Normal` makes the destination eligible. The first materialization
  or payload write dynamically opens or creates it, and Rust exports verified
  part spans through the shared storage implementation.
- A boundary piece remains requested and verified when any non-padding span
  is wanted.
- A skipped destination absent from the provider remains absent until a real
  wanted write needs it.

The selective part artifact is also dynamic. Merely constructing or restoring
storage must not create it. The first write routed to a part slot requests its
document, writes through the returned descriptor, and places that descriptor
under the same session resource budget. When the final slot is released, Rust
fences and releases the handle before asking Kotlin to delete the empty owned
document.

Part payload and metadata remain Rust-owned. Kotlin neither interprets part
slots nor copies bytes between the part document and payload documents.

## Publication, Restart, And Repair

Provider publication remains an explicit joined namespace transition:

1. stop admitting writes for the storage generation;
2. join positional work and complete the required durability checkpoint;
3. remove affected handles from the pool and wait for in-flight references;
4. let Android rename the exact owned staging document;
5. advance the namespace generation; and
6. reopen published documents dynamically with a new namespace generation;
7. atomically enter published checking and replace have state from the common
   bounded all-wanted piece checker; and
8. commit complete only after that recheck finishes successfully.

A crash on either side of the provider rename retains the durable two-phase
state established by Tactical `009`. Dynamic acquisition changes how files are
reopened, not what constitutes published success.

The final Tactical 073 API 34 no-window AVD run force-stopped during download,
rechecked and repaired one corrupted piece, killed the process after provider
rename, and restarted through `checking/published`, `recheck_started`, and
`have_rechecked` before `complete`. The 16-piece/256 KiB payload matched its
SHA-1, the run recovered the persisted grant-loss path, provider request high
water was one, Rust-owned handle high water remained at the configured 40,
and joined shutdown plus owned cleanup passed.

Repair preserves the stable root ID while replacing or refreshing its platform
locator. It invalidates every pool entry under the old capability before work
resumes. Grant loss leaves torrents waiting for repair. It never silently
falls back to application-private storage or another root.

## Failure And Security Invariants

- All provider paths derive from bounded, validated metainfo components and a
  trusted engine-owned role. No presentation string becomes a provider path.
- Per-path creation is serialized. Providers that silently deduplicate a name
  to `name (1)` or another sibling must be detected; a mismatched document is
  not accepted as the requested canonical destination.
- Cloud-backed providers remain unsupported until they prove seekable,
  durable positioned I/O and the required restart behavior. A tree grant alone
  is not that evidence.
- Descriptor duplication failure, an invalid descriptor, mode mismatch, or
  provider refusal inserts no cache entry and leaks neither the Kotlin nor Rust
  handle.
- `EMFILE` or equivalent resource pressure evicts eligible LRU entries and may
  retry acquisition once. Continued failure becomes an observable bounded
  storage error rather than unbounded eviction or a busy loop.
- No cache, broker, publication, repair, cancellation, or shutdown path closes
  a descriptor still used by an in-flight operation.
- Cancellation removes pending requests for the generation. Late descriptor
  responses are closed without mutating replacement state.
- An externally missing document or revoked grant invalidates the relevant
  entry/root and invokes ordinary conservative storage recovery. It cannot
  establish or preserve a false verified claim.

## Observability

The shared storage owner must expose bounded structured counters for:

- configured file-pool limit and current/high-water actual descriptors;
- hits, misses, evictions, mode upgrades, and open failures;
- same-key single-flight waits and read/write acquisition races;
- pending and high-water platform requests;
- provider open/create latency and result class by storage root;
- part-file acquisitions and deletions;
- invalidations caused by publication, repair, provider errors, or external
  disappearance; and
- resource-limit retries and terminal exhaustion.

These are diagnostics and Disk-view inputs, not a second application command
stream. Paths, SAF URIs, document IDs, and raw descriptors must not be logged.

## Reference Findings

### Pinned libtorrent

The required oracle is libtorrent at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`, pinned in
[`../../reference/pins.toml`](../../reference/pins.toml).

- `include/libtorrent/settings_pack.hpp::file_pool_size` defines a session
  upper bound on retained open files.
- `src/settings_pack.cpp` defaults the pool to 40;
  `src/session.cpp::{min_memory_usage,high_performance_seed}` uses 4 and 500.
- `include/libtorrent/aux_/file_view_pool.hpp` and
  `src/file_view_pool.cpp::open_file` implement the session-wide
  `(storage_index, file_index)` LRU, compatible-open single-flight,
  read/write upgrade, deferred close, resize, and release behavior.
- `src/mmap_disk_io.cpp` owns one pool for all torrent storage instances and
  records hits, misses, stalls, races, and current size.
- `simulation/test_file_pool.cpp::file_pool_size` constructs a 144-file
  torrent, configures five entries, and proves the observed high-water remains
  within five while the transfer completes.
- `src/mmap_storage.cpp::need_partfile` constructs part storage only when
  selection requires it; `src/part_file.cpp` opens the artifact per operation,
  creates its directory only for a write, and retains no eager payload handle.

RSTorrent adopts the resource and ownership semantics, not libtorrent's C++
multi-index container, mmap policy, public API, or architecture.

### JSTorrent Android

The local sibling was inspected at
`9895410beeed6aff554053769bd006a3fbd373ef` on 2026-08-03.

- `packages/engine/src/adapters/native/native-filesystem.ts` represents an
  open handle as `(root key, relative path)` without creating a document.
- `android/quickjs-engine/.../FileBindings.kt` resolves root keys and sends
  only actual reads and writes to `FileManager`.
- `android/io-core/.../FileManagerImpl.kt` creates missing parents and files on
  the first write, never on a missing read, opens `ParcelFileDescriptor` in
  `rw` mode, and uses `Os.pread`/`Os.pwrite` with a `FileChannel` fallback.
- Its Android SAF handle pool defaults to 32 handles and its per-path and
  per-directory locks defend against concurrent provider deduplication.
- `packages/engine/src/core/parts-file.ts` checks existence without creation,
  creates the info-hash-named auxiliary file on its first nonempty flush, and
  deletes it when empty.
- `FileManagerSafTest.kt` and `FileManagerConcurrencyTest.kt` cover nested
  on-demand creation, positioned round trips, deletion, and concurrent SAF
  directory creation.

JSTorrent demonstrates the platform capability and records valuable provider
failure history. RSTorrent does not adopt its per-read JavaScript/Kotlin data
bridge; after acquisition, payload I/O remains in Rust.

## Current Evidence And Known Gaps

- Deterministic pool coverage proves 10,000 logical file keys across 100
  torrent identities, compatible single-flight, access upgrade, stale
  completion fencing, late responses, and exact handle accounting.
- Fake-platform selective storage proves lazy construction, payload and part
  routing, positioned Rust hashing, namespace rename, and fresh dynamic
  published verification.
- Three fresh product-path AVD cycles report a native descriptor high water of
  6/40 and pending-request high water of 3/16. Whole-process baselines were
  106/107/113 descriptors and observed high water was 137. Every published
  non-padding file matched SHA-1; no info-hash directory, staging namespace,
  or empty part artifact survived.
- The current resource snapshot exposes pool and request counters, but
  provider latency histograms, typed result counts, and Disk-view presentation
  remain follow-up observability work.
- The AVD product profile covers the all-Normal transfer and lazy empty-part
  case. Dynamic-provider live Skip/Normal, interruption/restart, grant repair,
  and removal retain deterministic/session and earlier durable SAF evidence
  but were not each repeated in a dedicated new AVD profile.
- ChromeOS hardware was reachable and passed its nine-check doctor, but ARCVM
  ADB refused connection, so no new physical dynamic-provider claim is made.
- Tactical `081` passes both target-architecture Rust builds, generated UniFFI
  Kotlin compilation, APK assembly, and JVM tests. Its high-cardinality test
  represents 374,998 wanted files as one range while consuming one 1,024-row
  catalog page; no eager descriptor manifest is reconstructed.
- Tactical `078` seeds path-backed storage only. Future SAF upload reads must
  use this same shared pool rather than adding a separate seeding descriptor
  cache.
- The broker response currently carries only an opened file or deletion
  success. `ProductSafDocuments` looks up document ID and display name but
  does not return kind, length, modification, or opaque identity observations
  to Rust; root availability is therefore learned late at operation time.

## Recommended Next Work

Do not extend the legacy fixed-manifest proof APIs. Execute planned Tactical
`116` after Tactical `114`: add typed observations/root health, make
SAF-backed seeding use this pool, converge lifecycle outcomes, isolate or
remove descriptor-manifest production backing, and repeat the complete
dynamic AVD/physical matrix. General root management, cloud/removable provider
support, and an exposed advanced file-pool setting still require their own
product decisions.
