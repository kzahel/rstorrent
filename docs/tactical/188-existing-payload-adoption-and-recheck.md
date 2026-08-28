# Tactical 188: Existing Payload Adoption And Recheck

Status: **Complete on 2026-08-28.** Existing path and platform-capability
payloads now enter the common full checker, adopted ownership is crash-fenced,
and managed removal preserves unrelated content. All declared Linux-hosted,
web, Android, controlled-oracle, package, and local-service gates pass.
Tactical `176` resumes as the sole **Now** with only its unchanged
macOS-hosted iOS compile gate.

Topics: `download-roots`, `client-persistence`, `application-control`,
`android-saf-storage`, `storage-throughput-architecture`,
`capability-readiness`, and `oracle-driven-engine-campaign`.

Dependencies: completed Tacticals
[`052`](052-batched-durability-checkpoints.md),
[`062`](062-user-visible-publication-layout.md),
[`067`](067-dynamic-platform-file-acquisition.md),
[`073`](073-unified-storage-and-complete-recheck.md),
[`105`](105-fact-based-persistence-and-recheck-containment.md),
[`108`](108-serialized-torrent-control-and-observable-checking.md),
[`116`](116-platform-storage-coherence-and-ios-feasibility.md),
[`120`](120-per-torrent-trusting-fast-resume.md), and
[`143`](143-dual-identity-and-persistence-foundation.md) own the artifact
layout, checker, durable verification generations, path/platform storage seam,
ordinary fast-resume trust, and opaque torrent identity this slice extends.

## Motivation And Decision

Re-adding a torrent after disposable application state is lost currently
fails with `output already exists` even when the recognizable destination
contains exactly the payload the user wants RSTorrent to recover. That is a
product failure, not a useful ownership defense. The metainfo hashes already
provide the correct authority: existing bytes are candidates, every readable
piece is checked, matching pieces are retained, and only absent or corrupt
pieces are downloaded.

Replace the fresh-row collision policy with automatic bounded discovery and
full recheck. Discovery never turns path, name, kind, length, or prior
RSTorrent-looking artifacts into verified evidence. Only the existing piece
checker may establish the new durable bitmap.

Automatic adoption also changes deletion safety. A pre-existing torrent tree
may contain unrelated user files before adoption or gain them later. Managed
removal must therefore delete only metainfo-listed payload files and exact
hash-owned auxiliary files, then prune only directories proven empty. It must
never recursively delete an adopted publication root.

No catalog schema change, compatibility reader, migration, profile reset,
thumbnail work, media-classification change, or new user setting is part of
this tactical.

## Completion Result

The implementation landed in five logical commits:

- `fb7783f` accepted this tactical, recorded the exact libtorrent and
  JSTorrent source-first result, and temporarily selected it as **Now**;
- `a735441` added bounded artifact discovery, atomically adopted discovered
  ownership with a pending verification generation, and routed it through the
  common full checker;
- `9f81b23` made path, Android SAF, and iOS platform-capability cleanup
  metainfo-exact while preserving unrelated descendants and oversized
  suffixes;
- `f08d7e7` completed the workspace cleanup lint exposed by the broader
  validation gate; and
- `905fda9` extended the controlled pinned-libtorrent harness to exercise the
  actual no-state re-add path for single-file, one-entry-tree, and cross-file
  piece layouts.

Fresh adds still use hidden staging and atomic publication without an
unnecessary hash pass. A fresh row that observes an exact final, staging, or
validated v1/hybrid part artifact instead commits its discovered ownership
and a pending verification generation in one SQLite transaction. The common
checker alone establishes piece authority; matching pieces survive and only
missing, short, or corrupt pieces become download work.

This matches libtorrent 2.0.13's default add-without-resume outcome: existing
metainfo-listed data triggers a full check, good pieces survive, and oversized
suffixes are not truncated. RSTorrent intentionally differs by exposing no
verification-bypass flag, rejecting expected-path symlinks and special
objects, treating simultaneous final and staging generations as ambiguous,
and assigning a new opaque catalog owner on re-add while retaining the same
protocol identity.

## Stopping Condition

This tactical is complete only when:

1. A torrent with verified metadata, no durable payload ownership, and an
   existing exact final destination automatically enters the common complete
   checker instead of `needs_repair`.
2. Matching pieces become durable have evidence; missing, short, or corrupt
   pieces remain absent and normal downloading requests only the remaining
   wanted work.
3. No existing byte is trusted from path, file kind, size, provider identity,
   or same-name layout alone. Network content activity does not begin before
   discovered-data checking completes.
4. Discovered namespace ownership and a pending verification generation are
   committed atomically. Death before that transaction leaves storage
   unowned and discoverable; death after it necessarily restarts through full
   checking; no crash window can convert unchecked data into ordinary
   fast-resume authority.
5. Fresh adds with no artifact retain the current hidden staging and atomic
   publication path without an unnecessary payload hash pass.
6. Exact hash-owned staging and v1/hybrid part artifacts may be recovered
   after state loss only after topology and part-header identity validation.
   Simultaneous final and staging namespaces, a v2 part artifact, wrong kinds,
   unsafe expected-path symlinks, or malformed part identity fail closed.
7. Oversized expected files are never truncated during discovery or checking.
   Only torrent-declared bytes participate in hashes; a bounded diagnostic
   records the structural difference.
8. Unrelated files and directories are ignored by recheck and preserved by
   `Keep` and `Delete managed data`. Removal deletes exact metainfo paths,
   prunes only empty parents, and removes exact hash-owned part state without
   following symlinks.
9. Path storage and supported local platform-capability storage share the
   discovery/check decision. Android SAF lands the applicable provider
   observation, exact removal, generated-boundary, build, cancellation, and
   bounded-resource evidence in this tactical. iOS retains the shared
   platform removal contract and Linux-available boundary checks; new
   physical iOS evidence is not required because no new iOS storage claim is
   made.
10. Deterministic engine/store/application tests, the Rust workspace baseline,
    web-contract gates when generated values change, Android dual-ABI and
    Gradle gates, and one local-service end-to-end recovery observation pass.
11. The owning topics, readiness queue, campaign checkpoint, and this
    execution record agree with the landed behavior and exact evidence.

## Stable Scenarios

1. **EPA-001 complete final:** no durable payload ownership plus a complete
   single- or multi-file final namespace enters Checking, hashes every
   candidate piece, records all pieces, and becomes Published/Complete without
   downloading payload.
2. **EPA-002 partial final:** readable matching pieces survive; missing and
   short files produce absent pieces; ordinary peers supply only remaining
   wanted data and final content verifies exactly.
3. **EPA-003 corrupt final:** a same-length mutation is mismatched rather than
   structurally trusted, is replaced through ordinary download, and cannot be
   served or presented as verified before repair.
4. **EPA-004 fresh namespace:** no final, staging, or part artifact follows the
   current fresh staged-download path and does not manufacture a checking
   generation.
5. **EPA-005 interrupted internal state:** exact hash-owned staging and a
   matching v1/hybrid part header re-enter full checking. Part-only recovery is
   allowed; output plus staging is ambiguous; malformed or foreign part state
   is repair-local.
6. **EPA-006 crash fence:** injected death immediately before and after the
   atomic adoption checkpoint proves respectively unowned rediscovery and
   durable pending full checking. A death during hashing restarts the same
   pending generation conservatively.
7. **EPA-007 unrelated content:** extra siblings and nested files do not
   contribute candidate bytes, do not block checking, and remain byte-exact
   after `Delete managed data`; only empty metainfo-derived directories are
   pruned.
8. **EPA-008 hostile filesystem:** symlinks or special objects at the final
   root, an expected file, or an expected parent fail without traversal or
   mutation. Unrelated entries outside every expected path are ignored.
9. **EPA-009 oversized file:** a file longer than metainfo remains the same
   physical length, its declared prefix may verify, and restart never treats
   the extra suffix as torrent content.
10. **EPA-010 platform parity:** a fake provider and Android SAF discover an
    exact published namespace, use open-existing for checking, retain the
    40-handle/16-request bounds, and remove only exact payload documents and
    empty parents while preserving unrelated documents.

## Source-First Record

### Pinned libtorrent oracle

The required oracle is libtorrent `2.0.13` at exact pinned commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`.

Inspected implementation paths:

- `src/posix_disk_io.cpp::async_check_files` and
  `src/mmap_disk_io.cpp::do_check_fastresume`: absent resume data calls
  `has_any_file` and returns `need_full_check` when torrent storage contains
  data; the default `no_recheck_incomplete_resume` value is false.
- `src/storage_utils.cpp::{has_any_file,initialize_storage}`: discovery scans
  metainfo-listed files, treats missing files as ordinary, reports oversized
  files, and deliberately does not truncate existing files.
- `src/torrent.cpp::{on_resume_data_checked,start_checking,on_piece_hashed}`:
  `need_full_check` enters `checking_files`, uses bounded parallel hash jobs,
  retains matching pieces, and treats missing, EOF, and short-file outcomes as
  absent rather than fatal.
- `src/storage_utils.cpp::delete_files`: managed deletion unlinks exact
  metainfo files and then attempts parent removal in reverse order, preserving
  directories containing unrelated entries.
- `include/libtorrent/torrent_flags.hpp::no_verify_files` and
  `include/libtorrent/settings_pack.hpp::no_recheck_incomplete_resume`: both
  stronger caller/policy bypasses exist, but neither is the default.

Inspected tests:

- `test/test_checking.cpp::test_checking` plus `checking`, `incomplete`,
  `corrupt`, `extended`, `force_recheck`, and their v2/single-file variants;
- `test/test_storage.cpp::test_check_files` plus mmap and POSIX oversized and
  sparse variants; and
- `test/test_resume.cpp` seed-mode missing-file and no-verify cases retained
  from Tactical `120`'s exact oracle run.

RSTorrent adopts the default no-resume discovery, complete hash authority,
missing/corrupt recovery, oversized preservation, bounded checking, and exact
file deletion behavior. No source, fixture, resume encoding, or test vector is
copied.

Intentional differences are explicit:

- RSTorrent exposes no `no_verify_files` or
  `no_recheck_incomplete_resume` product bypass. Discovered bytes always lack
  authority until the checker passes them.
- RSTorrent rejects symlinks and special objects at expected paths; it does
  not adopt libtorrent's optional symlink-metainfo behavior.
- RSTorrent retains hidden staging, part identity, publication generations,
  and SQLite verification generations. Simultaneous final and staging
  generations therefore have an ambiguity that ordinary libtorrent storage
  does not represent.
- An existing per-torrent final namespace may briefly enter checking even when
  it contains no nonzero expected file. Libtorrent's shared `save_path`
  triggers only when `has_any_file` finds a positive-size listed file; the
  eventual empty-have result is the same.

### JSTorrent product history

The local JSTorrent sibling was inspected at exact revision
`0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`:

- `packages/engine/src/core/torrent.ts::verifyResumeData` explicitly mirrors
  libtorrent and marks data checking necessary when files exist without resume
  evidence;
- `torrent.ts::_doCheckPieces` clears prior authority, closes stale handles,
  inventories existing files, checks part data, and hashes candidate pieces;
  and
- `packages/engine/test/core/resume-listtree.test.ts` covers absent resume data
  with and without existing payload.

RSTorrent adopts that user expectation while retaining its first-party Rust
checker, stronger crash ordering, bounded storage pool, part identity, and
platform capability seam.

`scripts/references.py status` confirms the exact specification, libtorrent,
and rqbit pins. Its aggregate status remains non-green because unrelated
optional libutp checkouts are absent and the JSTorrent remote differs only in
repository-name case; the exact local revision above was inspected directly.

## Ownership, Transactions, And Dependency Direction

`rstorrent-session` decides whether a durable row is an exact managed resume
or an unowned discovery attempt. It owns one new transaction that changes
`absent` to the discovered staging/published ownership state and advances
`verification_requested` together. It does not inspect payload bytes.

`rstorrent-engine` owns one plain discovery intent and result. Existing
`SelectiveStorage` path/platform observations determine `Created`, `Staging`,
or `Published`; the existing full checker remains the sole byte authority.
The download task owns cancellation and is already joined by pause, removal,
replacement, and shutdown. No new task, executor, checker, service, storage
trait, database table, or background owner is introduced.

Platform adapters continue to own namespace mutation. Rust supplies a bounded
validated removal manifest derived from verified metainfo; Android and iOS
delete exact leaves and only empty parents. Portable state never receives a
path, URI, bookmark, provider ID, or descriptor.

Dependency direction remains inward: protocol content supplies immutable safe
paths; engine storage consumes them; session persistence and platform adapters
coordinate durable and namespace effects. Protocol/domain code does not depend
on SQLite, Tokio tasks, filesystems, SAF, or client bindings.

## Resource And Failure Bounds

- Discovery performs at most one namespace/auxiliary observation per logical
  side plus one bounded scan of at most 374,998 non-padding metainfo files. It
  never performs a file-by-piece nested scan.
- Recheck uses the existing independent bounded hash concurrency, shared
  storage permits, 40-handle pool, Android four-provider-worker owner, and
  16-pending-request ceiling.
- Cleanup manifests contain at most one bounded component vector per
  non-padding file. Parent pruning deduplicates at most the bounded metainfo
  path set and never traverses unrelated descendants.
- Diagnostics remain one adoption/check summary plus existing bounded checker
  progress. No path strings, payload bytes, or per-file history are retained.
- Store/checkpoint failure before the atomic adoption transaction leaves the
  existing namespace untouched and unowned. Failure after it leaves a pending
  full-check generation. Hash, provider, permission, and root failures use the
  established torrent-local repair/availability paths.

## Implementation Stages

1. Add the closed discovery intent/result and atomic adoption-generation store
   transition with deterministic crash-order tests.
2. Replace the session collision preflight and route discovered path storage
   through the existing complete checker; prove complete, partial, corrupt,
   oversized, part, ambiguous, and fresh cases.
3. Replace recursive publication deletion with a metainfo-exact bounded
   cleanup plan and prove unrelated-content preservation.
4. Apply discovery and exact cleanup to the generic platform storage broker,
   regenerate bindings, and adapt Android SAF and iOS namespace owners.
5. Run deterministic, workspace, web-contract, Android, controlled oracle,
   and local-service recovery evidence; reconcile all owning documents.

Each stage lands as a coherent commit. No intermediate runtime commit may
claim discovered storage without also making full verification durably
pending.

## Validation Plan

- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- focused engine/session crash, discovery, cleanup, and fake-platform tests
- `npm run generate --prefix clients/web` when the shared boundary changes
- `npm run typecheck --prefix clients/web`
- `npm run test --prefix clients/web`
- Android generated-binding check, both maintained native ABI builds, and
  applicable Gradle unit/lint/APK gates from `DEVELOPMENT.md`
- Linux-available iOS boundary checks when the shared removal record changes
- controlled pinned-libtorrent existing-data comparison where the retained
  harness can express this exact add-without-resume case
- rebuilt/redeployed local headless service observation of the reported
  existing Big Buck Bunny destination entering Checking and converging without
  payload deletion or an `output already exists` repair

## Recorded Evidence

The completed slice passed:

- focused engine, session-store, application, fake-platform, cleanup, crash-
  ordering, oversized-file, ambiguity, and hostile-path tests;
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace`;
- `npm run typecheck --prefix clients/web` and
  `npm run test --prefix clients/web`;
- `clients/android/build.sh`, including regenerated Kotlin UniFFI, both
  maintained release ABIs, debug APK assembly, and unit tests, followed by
  `lintDebug`, `testDebugUnitTest`, and `assembleDebugAndroidTest`;
- `cargo test -p rstorrent-android`, `cargo test -p rstorrent-ios`, and
  `cargo check -p rstorrent-session -p rstorrent-android -p rstorrent-ios`.
  This Linux host cannot execute Swift/Xcode compilation, and this tactical
  makes no new iOS presentation or physical-device claim;
- the retained controlled pinned-libtorrent topology comparison for
  single-file and one-entry-tree layouts, each retaining two of three pieces
  and repairing one 32 KiB piece, plus a cross-file layout retaining one of
  three pieces and repairing two 32 KiB pieces. In both engines the 24-byte
  oversized suffix survived where applicable; and
- `scripts/build-headless-package.sh` plus independent package validation of
  20 files and 76,575,942 extracted bytes. The installed binary exactly
  matched the release build, the enabled service restarted cleanly, and both
  direct-LAN and Tailscale health endpoints passed.

The local-service acceptance used the user's preserved Big Buck Bunny tree.
The pre-fix quarantined catalog row was removed with `Keep data`; its locally
retained metainfo was uploaded without a public metadata fetch. The new opaque
owner adopted the existing final namespace, completed verification generation
1/1 across all 1,055 pieces, and reached `complete`/`published` with no error.
SHA-256 hashes of the subtitle, MP4, and poster were identical before and
after the re-add. No payload was deleted or restored for this observation.

Public swarm traffic, payload restoration beyond the user's re-add action,
release publication, schema migration, and physical-device mutation are not
implicitly authorized by this tactical.
