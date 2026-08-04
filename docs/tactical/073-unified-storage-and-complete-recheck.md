# Tactical 073: Unified Storage And Complete Recheck

Status: Draft for maintainer review. Implementation has not started.

Topics: `download-correctness`, `client-persistence`, `download-roots`,
`storage-throughput-architecture`, `android-saf-storage`,
`application-control`, `capability-readiness`, `oracle-driven-engine-campaign`

## Decision And Motivation

Treat BEP 3 single-file and multi-file metainfo as two input and publication
shapes, not as two download, storage, persistence, or recheck mechanisms.

The parser already makes the useful normalization: a single-file info
dictionary with `length` becomes one logical `MetainfoFile`, while a
multi-file info dictionary with `files` remains multi-file even when that list
contains exactly one entry. `TorrentLayout`, piece planning, selection,
positional I/O, SHA-1 verification, durability checkpoints, and session have
state do not need a metainfo-mode fork.

The current engine nevertheless forks at `run_content_download`:

- `MetainfoMode::SingleFile` uses `run_single_download` and `StagingFile`;
- `MetainfoMode::MultiFile` uses `run_selective_download` and
  `SelectiveStorage`; and
- resumable execution rejects the single-file branch entirely.

That split is now an accidental correctness boundary. `StagingFile` creates a
whole-file `.<name>.rstorrent-part`, has no `ResumeContext`, participates in no
durable have checkpoint, and syncs only immediately before rename. Extending
that branch would preserve the wrong architecture.

This tactical removes it. One torrent-storage owner will consume the normalized
logical layout for every v1 torrent. Metainfo mode remains visible only where
it is semantically required:

| BEP 3 info shape | Recognizable final artifact | Hidden staging artifact |
| --- | --- | --- |
| `length`, no `files` | `<root>/<name>` regular file | `<root>/.<info-hash>.rstorrent-staging` regular file |
| `files`, including one entry | `<root>/<name>/` directory tree | `<root>/.<info-hash>.rstorrent-staging/` directory tree |

The existing `.<info-hash>.rstorrent-parts` file is different from the
singular `.rstorrent-part` bring-up artifact. The plural hash-owned file stores
piece slots routed away from skipped files and remains part of the unified
selective-storage design.

The second outcome is a complete conservative recheck. Restart must hash all
physically readable wanted pieces, not only pieces already claimed by SQLite.
That is required to recover the valid post-sync/pre-checkpoint crash window,
to clear stale positive claims, and to make force recheck useful for completed
published torrents. Persisted have bits are input evidence and a progress
cache; SHA-1 over the current logical piece is the authority.

This is one coherent correctness slice because the current mode fork prevents
the common recheck and publication contract from being stated or tested once.
It is not a request to preserve the current class names or incrementally add
single-file exceptions.

## Desired Outcome And Stopping Condition

The tactical stops only when all of the following are true:

- fresh, resumed, rechecked, repaired, checkpointed, and published v1 content
  uses one storage owner and one content-supervisor pipeline for BEP 3
  `length`, one-entry `files`, and ordinary multi-file torrents;
- `StagingFile`, `run_single_download`, the `ContentStorage::Single` runtime
  variant, and all production generation of the singular `.rstorrent-part`
  suffix are removed;
- startup/restart performs a bounded full recheck of every engine-owned
  staging or published artifact when its torrent is admitted for running,
  before peer work, including durable torrents previously marked complete;
- an application-level force-recheck operation joins active work, rechecks the
  same managed artifacts, and either restores completion or resumes only the
  missing/corrupt wanted pieces according to durable run intent;
- valid unclaimed pieces are recovered, invalid claimed pieces are cleared,
  and neither process death nor storage mutation can produce a false durable
  have bit;
- missing files, short files, oversized files, corruption, cross-file pieces,
  skipped-file spans, padding, part storage, storage unavailability, and hard
  I/O/security failures have deterministic tested outcomes;
- path publication is durable across the sync/rename/namespace-sync/session-
  commit crash windows and never merges with or overwrites an unowned final
  destination;
- path and dynamic platform storage execute the same logical recheck contract,
  with their existing namespace/capability differences kept at the backing
  boundary; and
- a controlled local harness exercises matching fixtures and comparable
  checking failures with pinned libtorrent `2.0.13`, while independent
  RSTorrent tests prove exact requested-piece and crash behavior.

Passing a fresh single-file download alone does not satisfy this tactical.

## Stable Scenarios

These scenario names are local to this tactical. Completion updates the
durable `DL-C` ledger with the accepted cases and evidence.

| Scenario | Required outcome |
| --- | --- |
| T073-R01 metainfo shape | A `length` torrent and a `files` torrent containing one file retain different final artifact shapes but enter the same normalized storage/check/recheck pipeline. |
| T073-R02 fresh execution | Every shape writes positional spans, verifies pieces, checkpoints, and publishes through the common owner; no singular `.rstorrent-part` is created. |
| T073-R03 death before payload sync | Restart trusts no uncheckpointed claim. A partially written or torn piece is requested again and cannot publish until its current full hash passes. |
| T073-R04 death after sync before have commit | Full recheck discovers valid unclaimed pieces and prevents their redownload. A later durability or database failure still cannot make them authoritative. |
| T073-R05 death after have commit | Restart rehashes the claimed piece. Matching data is retained; mutation, truncation, or deletion clears exactly every affected whole v1 piece. |
| T073-R06 empty or stale resume bitmap | Existing valid engine-owned bytes are recovered even when SQLite claims none; false positive bits never suppress recheck. |
| T073-R07 file geometry | Missing and short sources make affected pieces absent; oversized files hash only declared logical bytes and are normalized only under an explicit post-check repair/publication fence. |
| T073-R08 cross-file piece | Every physical source touched by one logical piece participates. A missing second file invalidates the whole v1 piece even when the first span is intact. |
| T073-R09 selection and part storage | Wanted boundary pieces reconstruct selected files, skipped-file sources or part slots, and padding in logical order. A missing/corrupt part slot invalidates only affected wanted pieces. |
| T073-R10 padding and zero-length files | Padding hashes as specified zero bytes without storage I/O; zero-length wanted files are materialized only when publication requires them. |
| T073-R11 completed publication | Startup and explicit force recheck can inspect published content. Invalid pieces revoke complete state and are repaired without treating the recognizable destination as an unrelated collision. |
| T073-R12 publication death | Restart reconciles an engine-owned staging or final artifact after death before rename, after rename before namespace durability, and after namespace durability before the durable published callback. |
| T073-R13 collisions and types | An unowned final artifact, simultaneous staging and final artifacts, symlink, directory/file shape mismatch, or replaced identity fails closed without merge, traversal, overwrite, or deletion. |
| T073-R14 pause, cancellation, and replacement | Checking has bounded progress, closes admission on pause/removal/shutdown, joins every hash/open/sync job, and rejects late results from the old torrent or namespace generation. |
| T073-R15 storage unavailable | A missing platform grant or temporarily unavailable root starts no false check and creates no empty evidence. The torrent waits for the existing repair flow; old claims are not used meanwhile. |
| T073-R16 read-only and hard I/O | Read-only intact content can be checked. Permission needed for later repair, `EIO`, non-seekable handles, and unsupported durability operations are distinguished from ordinary missing data and become typed storage/repair outcomes. |

## Scope

### One normalized torrent storage owner

- Replace the specialized whole-file owner with one `TorrentStorage`-level
  owner. Renaming and refactoring `SelectiveStorage` is expected; the exact
  internal name is not an API promise.
- Retain `TorrentLayout`, `FileSelection`, immutable positional write/hash
  plans, `PartFile`, the session-wide `StorageFilePool`, generation fences,
  and the bounded checkpoint owner where they already express the correct
  invariant.
- Make artifact topology explicit. A storage plan contains final, staging,
  and optional part identities plus `File` or `Tree` publication shape. A
  logical payload route does not infer topology from file count.
- Route both metainfo modes through one content planner, storage coordinator,
  hash join, checkpoint path, and publication state machine.
- Preserve direct path, dynamic platform, and diagnostic descriptor backings
  as backing variants under that common logical owner. This tactical must not
  create a trait hierarchy merely to rename the current branch.
- Remove the singular suffix implementation and its tests. Old unowned
  bring-up artifacts are neither resumed nor automatically deleted; RSTorrent
  has no durable evidence that an arbitrary matching path remains engine-
  owned. Managed removal continues to delete only artifacts justified by its
  recorded ownership state.

### Complete managed-storage recheck

- Add one full-recheck operation that walks every wanted logical piece in
  piece order and executes the ordinary immutable hash plan. A piece entirely
  outside current selection is not claimed merely because bytes happen to
  exist; a boundary piece is checked in full.
- Check all wanted pieces, including persisted false bits. Do not add a fast-
  resume hash-skipping policy in this tactical.
- Inventory and recheck use open-existing/read operations only. They do not
  create, extend, truncate, rename, or materialize payload/part files; those
  mutations begin only after the checked result enters an owned repair or
  publication fence.
- Begin checking only after verified metadata, selected-root identity, managed
  artifact ownership, and required storage capability are available. Before
  the first hash job, atomically enter `checking` and remove the old bitmap
  from runtime authority. The old bitmap may be retained as bounded input to
  classify previously checkpointed versus newly recovered matches.
- Construct a new bitmap from current SHA-1 results. Matching formerly
  unclaimed staging data passes a durability barrier for its deduplicated
  physical targets before it may enter the replacement bitmap. At successful
  check completion, replace have state and leave `checking` in one session
  transaction.
- A crash before replacement leaves no optimistic authority; the next start
  repeats the full check. Persisting a partial recheck frontier is not needed
  for correctness and is deferred.
- Reuse the same hash-plan implementation for startup recheck, force recheck,
  selection materialization, and targeted runtime verification. No session or
  platform layer reimplements piece-to-file arithmetic.
- Publish checking progress and bounded counters through existing application
  state/activity mechanisms. Do not expose verified content while its torrent
  is still represented as `complete` during a force recheck.

### Lifecycle and force recheck

- Verified managed torrents with desired running intent enter recheck on
  service restart before any content peer is admitted, including torrents
  whose prior durable state was `complete`/`published`.
- Add one semantic application force-recheck operation. If content work is
  active, it closes admission, cancels and joins the torrent generation,
  completes or abandons its checkpoint according to the existing safe
  boundary, then starts a new check generation.
- Keep the operation behind the existing request envelope, expected-revision
  check, and durable request deduplication. Replaying one request cannot start
  another check generation; a stale rejected request changes neither durable
  nor runtime state.
- Preserve desired run intent across the operation. A successful complete
  check returns to complete. An incomplete result starts download only when
  durable intent is running; otherwise it remains paused with its newly
  checked have state.
- If all wanted pieces exist in staging, proceed through ordinary
  publication. If an engine-owned published artifact is incomplete, retain
  its published identity under the mutation fence and repair it in place when
  the backing supports that existing contract. If the complete published
  artifact is absent, recreate staging lazily and republish after repair.
- Generated Rust/TypeScript/Kotlin application contracts and transport tests
  are in scope if the semantic operation crosses those existing boundaries.
  Adding a visible UI action is not.

### Artifact reconciliation and durable publication

`managed_artifacts` remains ownership evidence; physical existence is not
ownership. Add a durable `publishing` phase before the path namespace mutation
so death after rename is distinguishable from an unrelated final-path
collision. Reconciliation follows this policy before recheck:

| Durable ownership | Physical primary artifacts | Action |
| --- | --- | --- |
| `none` | none | Create staging lazily on first payload write. |
| `none` | staging or final exists | Preserve it and report an unowned collision. |
| `staging` | staging only | Open and full recheck the owned staging artifact. |
| `staging` | final exists, with or without staging | Preserve both and fail closed. Without a durable publishing intent, the final path is not owned. |
| `publishing` | exactly one of staging/final exists | Open and full recheck that exact owned artifact. Staging means rename did not become visible; final means it did. |
| `published` | final only | Open and full recheck the owned published artifact. |
| `staging`, `publishing`, or `published` | neither exists | Treat payload bytes as missing; retain a valid owned part artifact if present and recreate staging lazily for repair. |
| `publishing` or `published` | both exist | Fail closed as ambiguous; never merge or choose by timestamp/length. |
| `published` | staging only | Fail closed. The ordered protocol cannot produce this observation without later external mutation. |
| legacy | any | Retain the existing explicit legacy removal/repair policy; do not guess a new artifact identity. |

The common path publication sequence is:

1. stop new storage admission and join every matching positional job;
2. complete the final payload/part durability checkpoint;
3. ensure every wanted piece in the publication generation is verified;
4. create required zero-length entries and normalize oversized managed files
   without following symlinks;
5. invalidate and release affected pooled handles;
6. prove the recognizable final destination is absent, then durably record
   the exact `publishing` intent for this torrent/storage generation;
7. rename the hidden staging file or tree to its recognizable final path with
   an atomic no-replace primitive; a check-then-overwriting-rename sequence is
   not sufficient;
8. make the containing namespace durable with the strongest supported local
   filesystem primitive; and
9. record durable `published`/`complete` state.

Restart must accept either side of step 7 after an abrupt process or system
failure and full recheck it. It must not infer publication from a same-name
unowned destination. Platform-capability publication retains the accepted
two-phase provider sequence from Tactical `067`; its fresh published handles
enter the same logical recheck before the durable complete transaction.

## Recheck Outcome And Failure Policy

The checker returns task-free per-piece outcomes to its supervisor. Storage
backings classify errors; they do not mutate picker/session state directly.

| Observation | Piece/session outcome |
| --- | --- |
| Complete logical piece and matching SHA-1 | Set the replacement bit after any required recovered-data durability barrier. |
| Complete logical piece and SHA-1 mismatch | Clear that whole v1 piece; schedule it only if wanted and running. |
| Missing file/part source, EOF, or declared span beyond current length | Clear every affected piece and continue checking later readable pieces. |
| Extra bytes beyond declared file length | Ignore the suffix while hashing; report it and normalize only under the later managed mutation fence. |
| Padding span | Feed specified zero bytes without opening a file. |
| Root/grant unavailable before checking starts | Wait in the existing storage repair/availability state; do not clear/create evidence merely because the capability is absent. |
| Ordinary read permission failure | Preserve a typed repair error. Do not convert it into hash mismatch or create replacement data. |
| Symlink, wrong artifact shape, identity replacement, non-seekable provider handle | Security/invariant failure; fail closed and require repair. |
| `EIO`, arithmetic/plan invariant failure, unexpected short positional read after validated geometry, or worker join failure | Hard storage failure; stop the check generation and surface exact context. |

Corruption never invalidates unrelated v1 pieces. Conversely, a piece that
crosses two files is one integrity unit: loss of either physical span clears
the whole piece.

## Persistence Contract

No new resume file is introduced. SQLite remains the only durable application
resume owner; payload and part artifacts remain storage-owned.

The intended transaction boundaries are:

```text
begin recheck:
    persisted metadata/layout/root/artifact ownership validated
    -> state = checking, runtime have = empty, old have retained only as input

complete recheck:
    all hash jobs joined
    -> recovered unclaimed targets made durable
    -> replace exact have bitmap + checked progress/state in one transaction

ordinary download checkpoint:
    piece hash pass
    -> checkpoint intent
    -> sync captured unique targets
    -> batch exact pieces into SQLite

publication:
    all wanted pieces durable
    -> namespace mutation durable
    -> published/complete transaction
```

The current checkpoint limits remain authoritative: at most 256 dirty pieces,
64 MiB of dirty payload, or two seconds per batch; an oversized single piece
may form its own batch; payload sync concurrency remains four. This tactical
may factor shared helpers but must not weaken those bounds or add a second
resume writer.

A bounded pre-release schema revision is required to add `publishing` to the
managed-artifact state (or an equivalently explicit single-owner publication
operation record). Verified metadata, exact have state, `checking`, desired
running intent, and the remaining storage ownership already exist. The new
phase must be written before rename, identify the current torrent/storage
generation, replace rather than shadow the current ownership owner, and have
fresh, version-6 migration, reopen, and every-boundary crash tests. It is not a
general filesystem journal.

## Owners, Tasks, Cancellation, And Dependency Direction

| State or work | Owner | Cancellation and termination |
| --- | --- | --- |
| Hostile metainfo parsing and normalized logical file list | `rstorrent-protocol` metainfo/layout code | Task-free and independent of paths, files, Tokio, SQLite, and platform handles. |
| Final/staging/part topology and logical file routes | Engine storage planner | Immutable after verified metadata/root/selection input; path shape is explicit and does not leak into protocol types. |
| Open payload/part sources, routing generations, verified bitmap, publication shape | One engine `TorrentStorage` generation | The torrent supervisor closes admission, cancels queued work, joins running work, then drops or replaces the generation. |
| Full-check scheduling and result reduction | Content/torrent supervisor | At most the configured hash limit is in flight. Pause/removal/shutdown stops admission and joins all blocking calls before returning. |
| One piece hash | Existing bounded hash executor | Owns one immutable plan and private 16 KiB buffer; returns an outcome and cannot mutate session/have state. |
| Recovered-data and ordinary download durability | Existing checkpoint owner | Retains exact captured storage references until sync and session callback complete; cancellation follows its current finish/abort contract. |
| Durable bitmap, lifecycle, selected root, artifact ownership, desired intent | `ApplicationService` plus `SessionStore` | One supervised torrent operation; task completion is observed before another generation, removal, repair, or shutdown mutates its namespace. |
| Path/SAF handles and descriptor cap | Session-wide `StorageFilePool` | Existing 40-handle bound, single-flight acquisition, generation validation, invalidation, and zero-handle shutdown remain authoritative. |
| Provider namespace mutation and grants | Existing platform adapter/service | Existing four-active/16-queued request bounds and two-phase operation IDs remain; late/cancelled responses are closed and rejected. |

No detached task, separate persistence actor, native host, socket proxy, or I/O
daemon is introduced. Runtime and platform layers depend inward on pure layout
and transition code; protocol state never depends outward on filesystem or
session infrastructure.

## Resource Bounds And Observability

- Full recheck reuses the existing storage hash executor. Desktop remains at
  the selected maximum four concurrent hashes and Android no higher than two
  unless a separate measured tactical changes those defaults.
- Each in-flight v1 hash owns one 16 KiB fixed buffer. Recheck never allocates
  a piece-sized buffer, one buffer per metainfo file, or one job per piece in
  an unbounded queue.
- The supervisor enqueues only enough work to fill bounded hash capacity and
  retains one bounded result record per in-flight piece. The replacement
  bitmap is bounded by the already validated metainfo piece-count ceiling.
- The 40 Rust-owned storage-handle service cap, platform four-active/16-queued
  acquisition cap, existing payload budgets, and checkpoint limits remain in
  force during repair and publication.
- Checking exposes bounded structured fields for generation, artifact source,
  pieces total/checked/matched/missing/mismatched, bytes hashed, recovered
  unclaimed pieces, cleared former claims, active/queued hash high water,
  oldest active age, storage error class, cancellation, and elapsed time.
- Logs may explain failures but are not the state or command transport.
  Presentation state reports `checking` and exact progress until the session
  transaction chooses paused/downloading/awaiting-publication/complete/repair.

## Normative And Reference Dossier

No reference source, test, fixture, resume format, or class graph is copied.

### Normative behavior

- BEP 3 at `reference/bittorrent.org/beps/bep_0003.rst` defines the mutually
  exclusive `length` and `files` info shapes, identifies `name` as the
  suggested file/directory name, and hashes pieces over the concatenation of
  logical file bytes. It is the authority for v1 integrity and cross-file
  piece behavior.
- BEP 47 at `reference/bittorrent.org/beps/bep_0047.rst` defines padding files.
  Padding participates as specified zero bytes and must not require stored
  payload or peer traffic.

### Pinned libtorrent oracle

The required checkout is `reference/libtorrent` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` (`v2.0.13`). Relevant owners and
cases inspected while drafting this tactical are:

- `src/torrent_info.cpp::{extract_single_file,parse_info_section}` and
  `include/libtorrent/file_storage.hpp` normalize both metainfo shapes into
  `file_storage`; single-file entries use the no-path representation rather
  than a separate disk/resume subsystem.
- `src/mmap_storage.cpp::{read,write,hash}` and `src/posix_storage.cpp` route
  both shapes through the same logical-file read/write helper. File priority
  and part-file behavior are orthogonal to metainfo mode.
- `include/libtorrent/disk_interface.hpp::async_check_files` and
  `src/storage_utils.cpp::verify_resume_data` require every physical file
  touched by a claimed piece to exist. The latter explicitly handles a piece
  spanning multiple files and checks seed/all-have geometry.
- `src/mmap_disk_io.cpp::do_check_fastresume` initializes storage and requests
  a full check when resume data is missing/incomplete but files exist, unless
  `settings_pack::no_recheck_incomplete_resume` explicitly disables it.
- `src/torrent.cpp::{force_recheck,on_force_recheck,start_checking,
  on_piece_hashed}` disconnects/fences active work, forgets optimistic have
  state, schedules bounded full-file hashing, treats missing/EOF/short files
  as recoverable absence, and treats other disk errors as fatal.
- `test/test_checking.cpp::test_checking` covers ordinary, corrupt, incomplete,
  extended, read-only, force-recheck, and single-file shapes through one
  checker harness. Its single-file cases are v2-specific in this pinned tree,
  so RSTorrent supplies independent v1 single-file coverage rather than
  overstating that exact reference test.
- `test/test_storage.cpp::{test_check_files,
  fastresume_spanning_piece_missing_file}` covers sparse/zero-priority/
  oversized storage and the cross-file missing-source resume case.
- `test/test_recheck.cpp::recheck`,
  `test/test_part_file.cpp::{part_file,posix_part_file,
  part_file_short_read}`, `test/test_resume.cpp::{resume_data_have_pieces,
  unfinished_pieces_*}`, and
  `test/test_read_resume.cpp::{round_trip_have_pieces,
  round_trip_verified_pieces}` provide force-recheck, part-source, have, and
  partial-progress completeness checks.

RSTorrent intentionally differs in several ways:

- SHA-1 full recheck is always authoritative in this tactical; timestamps,
  sizes, and persisted bits do not form a fast-resume trust shortcut.
- RSTorrent keeps recognizable publication separate from hidden hash-owned
  staging and keeps resume state in its application SQLite store.
- RSTorrent does not implement libtorrent's resume-data format, seed mode,
  unfinished-block map, memory mapping, mutable-torrent hard links, v2/hybrid
  checking, or general file remapping here.
- RSTorrent's part file, pool, checkpoint batches, and platform capability
  protocol retain their established Rust/application ownership.

### RSTorrent and JSTorrent inputs

- Current RSTorrent owners are
  `crates/rstorrent-engine/src/{storage.rs,selective_storage.rs,driver.rs}` and
  `crates/rstorrent-session/src/{application.rs,store.rs}`. The removal target
  and current partial claimed-piece loop are recorded in the motivation rather
  than treated as the desired architecture.
- Existing tacticals `052`, `053`, `054`, `062`, `063`, and `067` own the
  durability, positional-plan, concurrency, named publication, live
  selection, and dynamic platform boundaries this slice must preserve.
- Before implementation, inspect the then-current pinned JSTorrent revision
  for single/multi path normalization, resume/check behavior, part storage,
  and Android/ChromeOS failures. Record the exact revision, paths, and adopted
  or rejected lessons in this section before code lands. JSTorrent is product
  history, not a persistence-format or architecture donor.

## Implementation Sequence And Logical Commits

1. **Pure artifact topology and reconciliation.** Add explicit `File`/`Tree`
   publication shape, common final/staging/part plans, and a task-free
   reconciliation reducer. Cover both metainfo forms, one-entry multi-file,
   collisions, ownership states, symlinks/types, and every publication crash
   observation before changing runtime dispatch. Add the bounded schema
   revision and durable `publishing` transition before landing namespace work.
2. **Collapse the storage/content fork.** Generalize the normalized storage
   owner, route every v1 torrent through it, retain backing-specific open and
   publication operations, and delete `StagingFile`, `run_single_download`,
   `ContentStorage::Single`, and singular-suffix production. Re-run all
   positional, selection, padding, part-file, batching, and generation tests.
3. **Build the full-check reducer and scheduler.** Add bounded piece-order
   planning, complete outcome/error classification, current-bitmap creation,
   recovered-target durability, cancellation/join, progress, and exhaustive
   deterministic tests. Replace the claimed-bits-only restart loop.
4. **Join session lifecycle and force recheck.** Add atomic begin/finish check
   store transitions, check completed torrents at startup, preserve desired
   intent, expose the semantic operation through affected generated contracts,
   and prove active/paused/complete/removal/shutdown replacement behavior.
5. **Close publication and repair crash windows.** Add namespace durability,
   owned staging/final reconciliation, missing/short/oversized/corrupt repair,
   published-data recheck, and injected death at every ordered boundary.
6. **Prove path/platform parity.** Apply the common logical checker to dynamic
   platform handles, preserve acquisition and provider bounds, test grant
   loss/late response/publication acknowledgement, cross-build Rust Android
   targets, and run the controlled no-window AVD scenario if platform code or
   generated Kotlin behavior changes.
7. **Graduate with controlled interop and documentation.** Land the reusable
   local fault harness, execute the complete matrix, update the owning topics,
   capability/readiness/protocol claims, this tactical's completion record,
   and the oracle campaign restart checkpoint.

Each logical commit must leave the workspace tests passing. Temporary adapters
may exist within a commit sequence, but no landed intermediate commit may
create two durable resume writers or treat the singular suffix as a supported
new artifact.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure protocol/layout | BEP 3 `length`, one-entry `files`, multi-file cross-piece, final short piece, padding, zero-length file, hostile path, piece/file count bounds, and exact logical segment order. |
| Pure storage planning | File/tree final and staging shapes; full-hash internal names; final/staging/part non-aliasing; ownership/existence reconciliation table; symlink/wrong-type rejection; Windows/macOS/Linux path component behavior. |
| Storage unit/runtime | Fresh/staging/published reads and writes for all shapes; empty/true/false/malformed old bitmaps; valid unclaimed recovery; exact corruption/missing/truncation effects; oversized logical hashing; read-only check; part short read; cross-file missing span; fixed-buffer and concurrency high water; cancellation and stale-generation rejection. |
| Checkpoint/store | Death before sync, after sync before have replacement, after replacement, and during batch failure; atomic checking transitions; exact bitmap replacement; completed/paused/running intent; no false presentation of complete; schema-7 fresh/version-6 migration/reopen coverage for durable publishing intent. |
| Publication/path | File and tree atomic no-replace rename; namespace durability injection; death before publishing intent, after intent before rename, after rename, after namespace sync, and before/after published callback; restart with staging only/final only/neither/both; unowned final collision preservation; repair and managed removal. |
| Application/transport | Startup check before network, explicit force recheck of paused/active/complete torrents, missing/corrupt repair of exact piece set, cancellation/removal/shutdown joins, diagnostics/progress, generated Rust/TypeScript/Kotlin contract stability or regeneration. |
| Platform deterministic | Single and multi logical layouts over dynamic handles; OpenExisting-only recheck; missing/short/replaced/non-seekable handles; grant loss; four-active/16-queued and 40-owned-handle bounds; publication acknowledgement followed by fresh full check. |
| Controlled Android | If affected, API 34 no-window AVD: single-file and cross-file fixtures, interruption/restart, force recheck, one corruption repair, publication, exact hashes, descriptor/request high water, grant-loss recovery, cleanup. |
| Controlled libtorrent | Pinned local libtorrent seed plus paired client oracle on deterministic `length`, one-entry `files`, and cross-file fixtures; incomplete/corrupt/extended/force-recheck variants where comparable; exact final bytes and cleanup. |
| Workspace | Formatting, warning-denying clippy, all workspace tests, generated-contract checks, affected web gates, Android Rust cross-builds, and no ignored live test counted as passing evidence. |

### Controlled fault and interoperability harness

Add or extend one headless harness under `tests/interop/` rather than composing
manual shell steps. It must:

- create deterministic non-sparse payloads and independently encoded v1
  `length`, one-entry `files`, and cross-file torrents;
- use pinned libtorrent `2.0.13` as a loopback seed and, for comparable cases,
  as a client reference;
- inject observable RSTorrent gates immediately before payload sync, after
  sync/before session commit, after commit, after durable publishing intent
  before rename, after staging rename, after namespace durability, and before
  the published callback;
- terminate the RSTorrent process at those gates without graceful cleanup,
  then reopen the same application profile and storage root;
- run restart with the seed unavailable when current bytes should suffice,
  proving that valid unclaimed pieces are recovered rather than silently
  redownloaded;
- mutate, truncate, remove, and extend exact sources, then prove by the
  deterministic request/activity trace that only affected wanted whole pieces
  are requested. Libtorrent payload counters are supporting evidence, not the
  sole exact-piece oracle because protocol retries may duplicate bytes;
- require final logical file hashes and exact lengths, durable complete state,
  no singular `.rstorrent-part`, bounded high-water metrics, zero leaked tasks/
  handles, and exact owned-artifact cleanup; and
- emit one structured result containing implementation versions, fixture
  fingerprints, fault point, artifact state, old/new have maps, requested
  pieces, bytes, timings, high-water marks, terminal classification, and
  cleanup result.

This is controlled local evidence. No public swarm, visible product client, or
physical device is needed for the path-storage stopping condition.

## Baseline Commands

Run in proportion to affected surfaces, with exact commands and results added
to the completion record:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Also run the affected generated-contract, web, Android cross-build, AVD, and
controlled interop commands established during implementation. The tactical
does not predeclare a command name that does not exist yet; the completion
record must show the final reproducible invocations and retained result paths
or summaries.

## Non-Goals And Deliberate Deferrals

- BEP 52/v2/hybrid storage or Merkle verification.
- Fast-resume trust based on timestamps, file size, inode identity, or a
  persisted “verified recently” shortcut. A later optimization must preserve
  this full-check oracle and prove equivalent invalidation.
- Persisting unfinished block maps or a partial recheck frontier. Restarting a
  cancelled check from piece zero is correct, bounded behavior for this slice.
- Automatically adopting arbitrary existing user files, importing external
  downloads, hard-link reuse, mutable-torrent links, relocation, or a visible
  “use existing data” workflow. This tactical rechecks only artifacts already
  justified by RSTorrent's durable managed ownership.
- Migrating or deleting unowned singular `.rstorrent-part` development
  artifacts. Production code stops creating or recognizing them.
- A visible force-recheck button/menu, file-priority redesign, automatic
  suffixing, overwrite/merge policy, general backup/restore, disk-space
  reservation, multi-torrent scheduling, upload/seeding, or performance work
  beyond preventing a material regression.
- New platform architecture, eager descriptor manifests, a second file pool,
  a separate I/O daemon, or weakening the accepted two-phase provider
  publication contract.

## Escalation Contract

Within this accepted tactical, ordinary refactoring, file/module naming,
task-free reducers, bounded internal store transitions, generated-contract
updates for the semantic force-recheck operation, deterministic failpoints,
and same-boundary correctness fixes do not require separate maintainer
approval.

Stop for direction before:

- adopting, overwriting, merging, truncating, renaming, or deleting a path not
  justified by durable managed ownership;
- changing the recognizable publication policy or treating a `files` torrent
  with one entry as BEP 3 single-file form;
- accepting hash-skipping, same-length trust, or another weaker integrity
  authority;
- adding a dependency or unsafe platform shim for atomic no-replace rename or
  namespace durability with meaningful portability or maintenance tradeoffs;
- weakening the 40-handle, platform request, payload, hash, or checkpoint
  bounds;
- adding a new process, daemon, socket proxy, platform host, or visible product
  surface;
- requiring public-swarm, visible UI, physical-device, destructive external,
  or other evidence outside the controlled authorization above; or
- discovering that correct published-data repair requires a new user-data
  ownership policy rather than the engine-owned in-place/staging behavior
  defined here.

## Completion Documentation

When the stopping condition passes:

- update `download-correctness.md` for restart, stale claims, publication, and
  multi-piece single-file evidence;
- update `client-persistence.md` from claimed-piece-only conservative recheck
  to the accepted full managed-storage contract and exact crash outcomes;
- update `download-roots.md`, `storage-throughput-architecture.md`, and
  `android-saf-storage.md` with the final file/tree artifact and backing
  behavior;
- update `application-control.md` for the force-recheck semantic operation;
- update `capability-readiness.md`, `oracle-driven-engine-campaign.md`, and
  `protocol-support.md` only to the level justified by recorded tests and
  interoperability evidence; and
- append exact implementation commits, validation commands/results, resource
  high waters, intentional differences, and any still-open evidence limit to
  this document.
