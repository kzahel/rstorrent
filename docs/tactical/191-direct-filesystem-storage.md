# Tactical 191: Direct Filesystem Storage

Status: **Ready (accepted 2026-08-29).** This tactical removes the managed
publication architecture and makes direct libtorrent-shaped final-path
storage the only ordinary payload model. No implementation has started.

Topics: `direct-filesystem-storage`, `download-roots`, `client-persistence`,
`storage-throughput-architecture`, `android-saf-storage`,
`application-control`, `application-view-api`, `client-surfaces`,
`web-ui-design`, `libtorrent-policy-alignment`, `capability-readiness`, and
`oracle-driven-engine-campaign`.

Dependencies: completed Tacticals
[`052`](052-batched-durability-checkpoints.md),
[`053`](053-immutable-positional-storage-plans.md),
[`054`](054-bounded-independent-storage-execution.md),
[`063`](063-live-file-selection.md),
[`067`](067-dynamic-platform-file-acquisition.md),
[`073`](073-unified-storage-and-complete-recheck.md),
[`105`](105-fact-based-persistence-and-recheck-containment.md),
[`108`](108-serialized-torrent-control-and-observable-checking.md),
[`116`](116-platform-storage-coherence-and-ios-feasibility.md),
[`120`](120-per-torrent-trusting-fast-resume.md),
[`124`](124-duplex-verified-piece-upload.md),
[`138`](138-verified-http-file-serving.md),
[`139`](139-incomplete-file-streaming-demand.md),
[`143`](143-dual-identity-and-persistence-foundation.md),
[`151`](151-complete-source-pure-v2-runtime-vertical.md),
[`155`](155-v2-magnet-authenticated-hash-exchange.md),
[`156`](156-hybrid-dual-swarm-runtime-closure.md),
[`179`](179-disposable-incubation-state-epoch.md), and
[`188`](188-existing-payload-adoption-and-recheck.md) own the durability,
positional I/O, selection, checker, resume, platform, upload/streaming,
identity, v2/hybrid, disposable-state, and existing-data behavior this slice
must preserve while deleting the publication layer.

## Motivation And Accepted Outcome

RSTorrent currently downloads into a hidden owner-keyed staging file or tree,
tracks prepared/publishing/published lifecycle facts, and atomically renames a
whole selected namespace only after every wanted piece is verified and
durable. That policy is not a libtorrent storage mode and no current product
requirement justifies its complexity.

It also produces undesirable user behavior. In a selected TV-series download,
one complete episode may remain hidden until every other currently selected
file is complete. The implementation model leaks into first-party surfaces as
`published storage`, `Not published`, `managed data`, and publication progress
states. Persistence, restart, platform adapters, media availability, deletion,
and generated contracts all carry machinery for a completion-time namespace
transition that the normal direct model does not need.

Replace that model completely. Fresh and resumed payload writes go directly
to final metainfo paths beneath the selected root. Existing content at those
paths is checked and reused. The part file remains only for selected/unselected
piece-boundary bytes. Completion is derived from verified wanted content and
seeding from full protocol availability; neither has a publication gate.

Do not leave the old implementation behind an option, compatibility alias,
generic storage strategy, dead enum variant, feature flag, or UI wording. A
future packed backend, temporary file suffix, or move-on-completion feature
must establish its own use case and contract instead of preserving this
architecture speculatively.

## Stopping Condition

This tactical is complete only when all of the following are true:

1. Fresh single-file, multi-file, v2, and hybrid downloads write wanted bytes
   directly to their final safe metainfo paths beneath the selected root.
2. No runtime or persistence path creates, discovers, reconciles, renames, or
   deletes a `.rstorrent-staging` artifact. Publication preparation,
   confirmation, and namespace-transition code is removed rather than
   bypassed.
3. Existing direct files with no trusted resume evidence enter the common full
   checker. Matching pieces survive; missing, short, and corrupt spans remain
   download work; oversized suffixes and unrelated siblings are preserved.
4. Ordinary synchronized resume retains Tactical `120`'s accepted structural
   trust, while Force recheck and structural disagreement use the same common
   checker against direct paths.
5. Selective downloads leave skipped files absent, write wanted files
   directly, and use a lazy validated part artifact only for bytes falling in
   skipped files. A completed wanted file is externally usable without waiting
   for other wanted files.
6. Wanted completion and whole-torrent seeding are distinct derived facts.
   There is no awaiting-publication runtime phase, completion percentage gate,
   media gate, upload gate, or notification gate.
7. Path storage, Android SAF, and qualified iOS roots implement the same direct
   logical layout. Platform publication plans, confirmation calls, and
   completion-time provider renames are absent.
8. Keep-data and delete-data removal remain restartable and exact. Deletion
   removes only metainfo files plus a validated part artifact and prunes only
   empty expected directories; unrelated descendants survive.
9. Publication-specific durable columns, values, jobs, state transitions, and
   compatibility readers are absent from the fresh application schema.
   Recognized schema-21 profiles reset through the existing bounded
   application-private mechanism without touching any external payload,
   legacy staging, or part artifact.
10. Generated Rust/TypeScript/UniFFI contracts and every first-party client
    remove publication/managed-storage states and wording. Replacement facts
    name the actual remaining condition: checking, incomplete, missing, root
    unavailable, or repair required.
11. Current path/platform resource limits, cancellation and join, durability
    ordering, selection-generation fencing, and session/root fairness remain
    bounded and evidenced.
12. Deterministic, crash, controlled pinned-libtorrent, browser/Tauri, Android,
    and maintained iOS gates pass, and all owning topics/readiness claims are
    reconciled with exact landed evidence.

## Stable Scenarios

### DFS-001: Fresh Direct Single File

A fresh verified BEP 3 `length` torrent creates `<root>/<safe name>` lazily,
sets the expected logical length on first writable open where the backend
supports sparse files, and writes received pieces at final offsets. The path
is visible during transfer. No hidden full-payload file, rename intent, or
publication state exists. A zero-length wanted file is materialized directly.

### DFS-002: Fresh Direct File Tree

A multi-file torrent creates only needed final parents and wanted files under
`<root>/<safe torrent name>/...`. When one selected file becomes fully
verified and readable, external file access and eligible first-party media
handoff work even while another selected file is incomplete. Finishing all
wanted files changes selected completion without performing a tree rename.

### DFS-003: Existing Complete, Partial, And Corrupt Data

With no durable state, complete direct files check to all pieces without
payload download. Partial files retain matching pieces and request only absent
wanted work. A same-length mutation fails its affected piece hashes and is
repaired through ordinary downloading. Path, size, timestamp, or provider
identity alone grants no have evidence.

### DFS-004: Resume And Forced Verification

Matching synchronized committed direct-storage evidence admits the existing
task-free fast-resume result. Pre-sync or post-sync/pre-commit death cannot
create committed have bits. Force recheck, pending verification, missing
expected files, or structural disagreement enters the common checker and
cannot serve unchecked bytes.

### DFS-005: Selective Boundary Piece

A piece spanning wanted file A and skipped file B writes A's bytes to A's
final path and only B's bytes to a lazy validated part artifact. B is not
materialized. Promoting B reconstructs any verified boundary spans and writes
subsequent bytes to B's final path. Lowering A's priority does not delete A.
Wanted completion and full seeding remain truthful in every priority state.

### DFS-006: Exact Paths And Hostile Filesystems

Unsafe metainfo components, expected-path symlinks, special objects, a file
where an expected directory belongs, a directory where an expected file
belongs, or a concurrent active writer fail closed without traversal,
suffixing, or blind replacement. Unrelated siblings are ignored. Checking an
oversized expected file hashes only its declared prefix and preserves the
suffix and physical length.

### DFS-007: Crash And Durability Boundaries

Injected death before payload sync cannot commit a false-positive have bit.
Death after payload sync but before the SQLite checkpoint may cause checking
or re-download, never unverified completion. Death after the atomic checkpoint
restores its synchronized direct-path evidence. There is no prepare/rename/
confirm crash window to reconcile.

### DFS-008: Keep And Delete Data

Keep removes only catalog/runtime authority. Delete data is a restartable
explicit job that unlinks exact metainfo files and the exact validated part
artifact, then prunes only empty metainfo-derived parents. A sentinel sibling,
nested unrelated file, neighboring torrent, and unknown legacy hidden staging
artifact remain byte-exact. Grant loss or an unsafe path fails the job closed
and exposes repair/retry rather than broadening cleanup.

### DFS-009: Android SAF And iOS Capability Roots

The fake provider, Android API 34 SAF product profile, and maintained iOS root
adapter find-or-create final documents idempotently and use exact coordinated
descriptors through existing bounds. Concurrent parent creation cannot create
duplicate suffixed directories. Grant/bookmark loss, provider ambiguity, and
unsupported random access produce typed unavailable/repair outcomes. No
staging document or completion rename is requested.

### DFS-010: Application Contract And Presentation

Rust snapshots, generated JSON/TypeScript, UniFFI, React, Tauri, Compose, and
SwiftUI compile without storage-publication states. A file that cannot be
opened reports incomplete, checking, missing, root unavailable, or repair as
appropriate. Completion notifications describe a completed download, not
publication. Removal describes keeping or deleting downloaded files.

### DFS-011: Disposable Schema Reset

Opening a recognized schema-21 profile produces the fresh direct-storage
schema through the bounded disposable reset. Only the fixed SQLite database
and its sidecars are eligible for removal. Sentinels in final content, legacy
hidden staging, part files, and unrelated root entries remain byte-exact and
unclaimed. Re-adding metainfo recovers final content through DFS-003; legacy
staging is neither adopted nor silently deleted.

### DFS-012: Controlled Oracle Comparison

For fresh sparse storage, complete/partial/corrupt no-resume data, oversized
files, one-entry and cross-file layouts, and selective boundary pieces,
RSTorrent and pinned libtorrent operate on the same observable final paths and
retain the same verified piece set. Intentional path-safety and platform
differences are recorded, not hidden as failed parity.

## Source-First Record

### Pinned libtorrent

The required oracle is libtorrent `2.0.13` at exact pinned commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`.

Implementation paths inspected while accepting this tactical:

- `include/libtorrent/add_torrent_params.hpp` documents `save_path` as the
  path where torrent content is or will be stored.
- `include/libtorrent/storage_defs.hpp` defines `storage_mode_sparse` and
  `storage_mode_allocate`. Both store logical files at final positions; they
  differ in allocation policy.
- `src/file_storage.cpp::file_path` derives the metainfo-relative content path
  beneath the save path.
- `src/mmap_storage.cpp` and `src/posix_storage.cpp` open/read/write those
  final paths directly. Writable mmap files are resized to expected logical
  length; sparse mode does not imply up-front physical allocation.
- `src/mmap_disk_io.cpp::do_check_fastresume` and
  `src/posix_disk_io.cpp::async_check_files` use
  `src/storage_utils.cpp::{has_any_file,initialize_storage}` to request a full
  check when resume data is absent/incomplete and listed content exists. The
  default `no_recheck_incomplete_resume` policy is false.
- `src/torrent.cpp::{on_resume_data_checked,start_checking,on_piece_hashed}`
  retains matching pieces and leaves missing, EOF, short, or corrupt spans
  absent.
- `src/mmap_storage.cpp::{need_partfile,set_file_priority}` and the POSIX
  counterparts use the part file for priority-zero spans, including export on
  promotion.
- `src/storage_utils.cpp::delete_files` removes exact listed files and attempts
  empty-parent cleanup rather than recursively owning the save path.
- `torrent_status::{finished,seeding}` and their producing torrent predicates
  distinguish all wanted pieces from every piece.
- `torrent_handle::move_storage` is an explicit caller operation. It is not an
  automatic completion publication mode.

Tests that must be re-opened during implementation and represented by
independently authored RSTorrent cases:

- `test/test_checking.cpp::test_checking` and `checking`, `incomplete`,
  `corrupt`, `extended`, `force_recheck`, v2/single-file variants,
  `discrete_checking`, and `preserve_file_priorities`;
- `test/test_storage.cpp::test_check_files`, sparse/allocate mmap and POSIX
  variants, oversized and priority-zero cases, remove cases, and explicit
  `move_storage` cases;
- `test/test_part_file.cpp::{part_file,posix_part_file}`;
- `test/test_priority.cpp::{export_file_while_seed,
  file_priority_stress_test}`; and
- `test/test_file_storage.cpp` safe path/root-name/renamed-file cases.

RSTorrent adopts direct final paths, sparse ordinary allocation, default
no-resume checking, matching-piece retention, priority-zero part semantics,
wanted/full completion distinction, and exact deletion. It intentionally
retains stricter expected-path symlink/special-object rejection, opaque
application torrent IDs, SQLite durability generations, explicit task
ownership, session-wide resource pools, and native platform-capability
adapters. It exposes no verification-bypass product setting.

No source, fixture, resume encoding, or test vector is copied.

### JSTorrent History

The local sibling was inspected at exact revision
`0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`:

- `packages/engine/src/core/torrent.ts::verifyResumeData` requires checking
  when files exist without resume authority;
- `torrent.ts::_doCheckPieces` inventories direct content, clears stale
  authority, checks candidate pieces, and accounts for part data; and
- `packages/engine/test/core/resume-listtree.test.ts` covers absent resume
  state with and without existing payload.

This supports the product expectation that re-adding the same content resumes
by checking final files. RSTorrent does not adopt JSTorrent's JavaScript
engine, extension/daemon topology, storage-root implementation, or source.

### Current RSTorrent Baseline To Remove

Implementation must inspect the then-current exact owners before editing. The
acceptance audit found these publication-era concepts:

- engine storage: `PublicationShape`, `PublishedArtifactLayout`, hidden
  staging and plural part paths, `NamespaceState`, `NamespaceAction`,
  `PreparePublication`, `ConfirmPublication`, and `PathPublicationStage`;
- session/persistence: `publication_name`, `payload_state`, publication intent
  and confirmation transactions, managed ownership, and removal jobs with a
  `delete_managed` policy;
- platform seams: publication plans/confirmation and Android/iOS provider
  rename operations;
- application contract: `TorrentState::AwaitingPublication`, staging/
  prepared/published `StorageState` variants, publication progress phase and
  reason, `MediaFileAvailability::NotPublished`, and
  `RemovalDataPolicy::DeleteManaged`; and
- first-party copy such as `not available in published storage`, `Not
  published`, `is published`, and `RSTorrent-managed payload, staging, and
  part data`.

This list is a starting map, not permission to delete by string. The tactical
must trace each remaining invariant and replace only facts still needed by
direct storage.

## Accepted Persistence And Contract Transition

Use the public-incubation disposable-state policy instead of migrating
publication-era rows.

- Advance the product catalog to fresh schema `22`.
- Treat every recognized schema `1..=21` as resettable application-private
  state under the existing bounded pre-task reset.
- The reset allowlist remains only the fixed database path and exact SQLite
  sidecars. It does not traverse selected roots.
- Remove publication-specific columns and constraints rather than leaving
  nullable tombstones. The fresh torrent model retains raw verified metainfo,
  selected root, selection/priorities, run intent, verification generations,
  synchronized have evidence, and other non-publication authorities.
- Derive the safe content name/layout from verified metainfo (or store a
  neutrally named validated content-layout value only if profiling proves
  reparsing unsuitable). Do not retain `publication_name` under an alias that
  still represents a publication transition.
- Replace removal-job `keep`/`delete_managed` with `keep`/`delete_data` (exact
  internal spelling may follow repository convention) and update all
  first-party generated contracts in the same change. No legacy value reader
  or command alias remains.
- Remove durable payload/publication states. Persist only facts required for
  verification, repair, exact removal progress, and restart. A filesystem
  cannot share an atomic transaction with SQLite, but direct writes need no
  namespace-intent transaction; synchronized have commits remain the
  durability fence.

External data is deliberately not migrated. Final files are recovered through
the normal no-state checker when re-added. Legacy `.rstorrent-staging` and
part artifacts survive reset untouched and untrusted. Tactical `191` does not
add a broad scanner, importer, or cleanup action for them.

Generated enum/field removals are accepted breaking changes during `0.1.x`.
Do not retain deprecated variants in JSON schema, TypeScript, Kotlin, or Swift
solely to ease an unpublished compatibility transition.

## Ownership, Tasks, And Dependency Direction

### Runtime-Independent Core

Safe metainfo path geometry, logical file spans, piece-to-file ranges,
selection decisions, part-slot mapping, verification state transitions, and
completion/seeding derivation remain plain deterministic engine data. They do
not depend on Tokio, filesystems, SQLite, platform handles, or application
presentation.

Replace publication-shaped layout with one immutable direct content layout:
safe final relative paths, expected lengths, logical offsets, padding facts,
and optional validated part identity. Do not introduce a general storage-mode
trait unless a concrete remaining backend difference requires it.

### Storage Owner

The existing content-storage task remains the sole mutable payload owner for a
torrent generation. It creates/opens direct final files, executes immutable
positional reads/writes, manages the selective part artifact, and produces
sync completion before have evidence may commit. The independent bounded hash
executor, write/hash generation join, and session-wide storage admission stay
in place.

The storage task must no longer plan or execute namespace preparation,
completion rename, or publication confirmation. Pause, replacement, root
repair, removal, and shutdown continue to cancel and join the owner before a
conflicting mutation.

### Session And Persistence Owner

The serialized torrent controller remains the authority for durable run,
selection, verification, and removal intent. It chooses trusted structural
resume versus full check, fences priority changes, derives wanted completion,
and coordinates exact delete-data jobs. It stores no platform capability and
performs no payload I/O in SQLite transactions.

One active catalog owner must prevent simultaneous RSTorrent writers from
claiming the same expected direct paths. Cross-profile/process overlapping
root or same-torrent coordination remains unsupported; detect conflicts the
current architecture can prove and fail closed rather than inventing an
unbounded global lock protocol in this tactical.

### Platform Capability Owner

Path I/O remains native Rust. Android and iOS retain locators, grants,
bookmarks/security scopes, provider qualification, and exact document handle
acquisition. Their boundary exposes observe/open-or-create/resize/read/write/
sync/delete operations on final logical paths plus the part artifact. Remove
provider publication plan, rename, and confirmation operations.

Android directory find-or-create must remain serialized/idempotent where SAF
providers otherwise create `name (1)` duplicates. The Rust session-wide
40-handle and 16-pending-request ceilings remain authoritative. iOS retains
exact-file coordination and qualified on-device-root policy.

### Application And Presentation Owners

The application service derives storage/file availability from root health,
verification, selection, readable ranges, and repair facts. Generated
contracts carry those semantics. React/Tauri, Compose, and SwiftUI translate
them into platform-appropriate copy; no client infers correctness from a
filesystem listing or reconstructs engine lifecycle.

No new background task, daemon, executor, queue, database, native companion,
or transport is introduced.

## Resource And Safety Contract

The slice changes namespace policy, not resource breadth. Retain unless
measurement requires a smaller value:

- one content-storage task per active torrent generation;
- session-wide 40 open storage handles across path/platform files;
- session-wide ten active storage reads and existing write/hash concurrency;
- platform maximum 16 pending descriptor requests;
- current desktop/mobile resident and storage-intake watermarks;
- current bounded full-check parallelism and buffers; and
- existing 4,096-entry sparse selection limits.

Direct sparse file creation may increase visible logical file sizes but must
not increase resident memory or physical allocation to torrent total size.
Record physical allocated bytes separately from logical length in a
representative sparse case. Unsupported sparse/provider behavior may fall back
to ordinary incremental allocation within the same logical contract; adopting
full preallocation or a new disk-space reservation policy requires direction.

All metainfo and external filesystem/provider observations remain hostile.
Never follow expected-path links, recursively delete a selected root, use a
name prefix as ownership, trust existing bytes without hashes/resume evidence,
or overwrite an incompatible object to make a test pass.

## Implementation Stages And Gates

### Stage 1: Direct Layout And Pure State

- Introduce the direct content-layout terminology and deterministic mapping.
- Remove publication-only runtime-free enums/actions and make wanted
  completion versus full seeding explicit.
- Extend pure path/selection/part-range tests for DFS-001, DFS-002, DFS-005,
  and DFS-006.

Gate: runtime-free crates compile and their deterministic/adversarial tests
pass without publication actions or a second full-payload path.

### Stage 2: Path Storage And Recheck

- Route fresh path-backed reads/writes/sync directly to final files.
- Reuse Tactical `188` discovery and the common checker against those paths.
- Preserve Tactical `120` trusted restart and the existing crash ordering.
- Remove hidden staging creation, recovery, rename, and publication code.

Gate: DFS-001 through DFS-007 pass on path storage, including exact sparse
allocation observations and injected durability deaths.

### Stage 3: Persistence And Serialized Lifecycle

- Land fresh schema 22 and the bounded schema-21 reset.
- Remove publication columns/states/transactions and rename delete policy to
  plain delete data.
- Derive completion, repair, media, upload, and checking from direct facts.
- Preserve exact asynchronous removal-job restart and failure behavior.

Gate: store/application transition tests cover clean state, hostile/unknown
schema, reset sentinels, restart, recheck, selection, keep, and delete data.

### Stage 4: Platform-Capability Storage

- Replace Android/iOS staging and provider rename with idempotent final
  document acquisition and direct I/O.
- Retain exact coordination, grant repair, root qualification, and shared
  pools.
- Prove concurrent nested-parent creation cannot suffix/duplicate directories.

Gate: fake-provider and generated-boundary tests pass before Android/iOS
product validation; DFS-005, DFS-008, and DFS-009 have platform cases.

### Stage 5: Generated Contracts And First-Party Clients

- Remove publication-specific commands, facts, enums, fields, and aliases.
- Regenerate TypeScript/schema and UniFFI Kotlin/Swift boundaries.
- Update React/Tauri, Compose, SwiftUI, notifications, Library/file
  availability, progress, diagnostics, and removal copy.
- Add reducer/presentation cases for every replacement unavailable reason.

Gate: repository searches find none of the forbidden storage-publication
symbols or user copy, while legitimate uses such as software release
publication and unrelated protocol publication remain intact.

### Stage 6: Controlled Oracle And Cross-Platform Closure

- Compare direct path/check/selection outcomes with pinned libtorrent for
  DFS-012.
- Run complete workspace/web/package/platform gates below.
- Record exact resource high waters and reconcile every owning topic,
  readiness row, campaign checkpoint, and the tactical completion result.

Gate: all stopping-condition evidence passes; no documentation describes the
old model as current except historical completed tactical records.

Implementation may reorder tightly coupled edits so intermediate commits
compile. Do not expose an intermediate release where generated clients and the
service disagree about removed variants.

## Validation Matrix

### Deterministic And Store

- safe single/tree/v2/hybrid direct layouts, zero-length files, padding, and
  hostile components;
- wanted/skipped cross-file piece planning, part creation/export, priority
  changes, wanted completion, and full seeding;
- complete/partial/corrupt/missing/oversized existing files;
- structural fast resume, Force recheck, synchronized checkpoints, and
  verification generation coalescing;
- schema 22 clean creation, every recognized pre-22 reset, unknown/hostile
  state, receipt replay, and external sentinel preservation;
- exact keep/delete jobs with injected cancellation, restart, grant loss,
  unrelated descendants, unsafe links, and legacy-hidden-artifact sentinels;
  and
- application snapshots/diffs/reducers for checking, incomplete, missing,
  unavailable, repair, complete, and seeding.

### Scripted Runtime And Crash

- death before payload sync, after sync/before have commit, after commit, and
  during full checking;
- pause, remove, root repair, selection change, replacement, and shutdown
  while writes/checking/deletion are active, with exact terminal task/permit/
  handle counts;
- concurrent torrents on one root and separate roots under existing fair
  admission;
- nested final-directory acquisition races on path and fake provider; and
- completed-file external read/media/open while another wanted file remains
  active.

### Controlled Interoperability

Use independently generated temporary roots and pinned libtorrent `2.0.13`
for:

- fresh sparse single-file and multi-file transfer;
- no-resume complete, partial, corrupt, missing, and oversized files;
- one-entry `files` topology and a piece spanning files;
- selected/skipped boundary piece plus later promotion; and
- restart, Force recheck, wanted-finished, full-seeding, and exact cleanup
  observations.

Compare observable final relative paths, verified piece sets, requested
remaining work, skipped-file absence, part semantics, and final hashes. Record
RSTorrent's stricter unsafe-path behavior as intentional. No public swarm is
required for this tactical.

### Repository And Web

Run the proportional baseline, including:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
```

Run the deterministic Playwright browser/Tauri-representative suite and the
relevant desktop package/build checks from `DEVELOPMENT.md`. Add a production
React/WebSocket local-service scenario proving a direct file is visible and
openable before another selected file completes, plus exact removal copy and
behavior.

### Android

- build both maintained Rust Android ABIs and run the Gradle unit/build gates;
- run the API 34 no-window SAF product profile for fresh direct download,
  partial restart/recheck, boundary selection, grant loss/repair, completed
  file open, exact deletion, duplicate-parent race, and descriptor/request
  high waters; and
- retain the applicable generated-contract and cancellation assertions in the
  same tactical. Physical Android is required only if emulator/provider
  evidence exposes a device/provider behavior the accepted AVD cannot settle.

### iOS

Because the slice removes an implemented iOS storage lifecycle and changes its
generated contract and UI, run the maintained simulator tests/build and
unsigned device archive on a macOS host. Exercise qualified-root direct
single/tree storage, partial restart/recheck, complete-file Quick Look while
another wanted file is incomplete, bookmark repair, exact deletion, and
coordination cleanup. A physical transfer is required if simulator/fake-root
evidence cannot prove the provider/coordination behavior changed here.

### Resource Evidence

Record at minimum:

- storage/hash task and permit high waters;
- path and platform open handles plus pending descriptor requests;
- resident bytes and storage-intake high water;
- checker concurrency and buffers;
- logical length versus allocated blocks for a representative sparse file;
- part-file bytes for a boundary-piece case; and
- terminal zero tasks, handles, permits, requests, and temporary test
  artifacts after cancellation and cleanup.

## Explicit Non-Goals

- a stateless foreground single-shot CLI or headless-root UX;
- packed single-blob/archive storage;
- per-file temporary suffixes;
- move-on-completion, category relocation, cross-volume copy, or automatic
  root migration;
- full preallocation or new disk-space reservation policy;
- support for cloud/offloaded/identified third-party iOS providers;
- progressive incomplete-file presentation changes beyond removing the
  publication gate;
- simultaneous independent profiles/processes writing overlapping content;
- importing or deleting legacy hidden staging after the catalog reset;
- changing protocol support, scheduling priority, network breadth, peer
  policy, or public release state; and
- preserving pre-schema-22 API or persistence compatibility.

## Escalation And Autonomous Authority

Within this accepted tactical, ordinary implementation authority includes
refactoring modules around the one direct layout, deleting dead publication
code, renaming remaining direct-storage facts, adding adversarial cases,
advancing the disposable schema, regenerating all first-party boundaries,
fixing same-owner bugs exposed by the change, using emulator/simulator and
controlled local interoperability, and updating the tactical/topics with
actual evidence.

Do not stop merely because the old abstraction is widely referenced, an
intermediate branch does not compile, the exact direct-layout type name
changes, or additional tests are needed to preserve an existing bound.

Stop for maintainer direction if evidence requires:

- deleting, importing, or moving legacy external staging/content;
- retaining publication as an optional product mode;
- a new external dependency or native helper;
- weakening path/provider safety or trusting unverified bytes;
- a migration promise beyond disposable `0.1.x` state;
- a new packed/suffixed/relocating storage representation;
- increasing established resource ceilings materially; or
- changing supported-provider, public-release, or product completion policy
  beyond the direct-storage decision recorded here.

## Completion Record

When the slice lands, replace the Ready status with the exact commit range,
schema and contract result, source/test deltas discovered during
implementation, commands and platform environments actually run, controlled
oracle outcomes, resource high waters, deliberate deferrals, and the next
recommended slice. Do not claim implementation from this accepted document
alone.
