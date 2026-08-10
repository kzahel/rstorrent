# Tactical 120: Per-Torrent Trusting Fast Resume

Status: Planned on 2026-08-10. The trust policy and bounded implementation
shape are accepted; implementation has not started. This planned slice does
not supersede the uTP Stage 1 human-review checkpoint currently recorded as
the authoritative **Now**.

Topics: `download-correctness`, `client-persistence`,
`code-organization-and-refactoring`, `android-saf-storage`,
`storage-throughput-architecture`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tacticals
[`052`](052-batched-durability-checkpoints.md),
[`073`](073-unified-storage-and-complete-recheck.md),
[`078`](078-local-single-peer-tcp-seeding.md),
[`105`](105-fact-based-persistence-and-recheck-containment.md),
[`108`](108-serialized-torrent-control-and-observable-checking.md),
[`114`](114-session-wide-concurrent-torrent-admission.md), and
[`116`](116-platform-storage-coherence-and-ios-feasibility.md) own the durable
checkpoint ordering, unified managed storage, completed-seed restoration,
fact-based verification generations, full-check controller, session resource
authority, and common path/SAF observation seam this tactical consumes.

## Decision And Desired Outcome

Make lightweight fast-resume validation the normal path for each eligible
durable torrent. A process crash is not a session-wide reason to distrust
unrelated torrents, and clean shutdown is not a prerequisite for trust.

RSTorrent follows pinned libtorrent's ordinary policy shape rather than its
stronger skip-all-validation flags:

1. validate durable torrent identity, have geometry, ownership state, root
   health, namespace shape, exact logical artifacts, and part-file structure;
2. when those facts are coherent, admit the already committed have bitmap
   without reading or hashing payload bytes;
3. when cheap validation rejects otherwise readable managed storage, run the
   existing complete checker for that torrent only;
4. when storage is unavailable, ambiguous, unowned, or malformed, retain the
   existing awaiting-storage, repair, or quarantine outcome rather than
   creating, deleting, adopting, or globally checking content; and
5. make explicit Force recheck, including a Force recheck interrupted by
   process death, always use the complete hash pass.

This is an intentional speed/trust tradeoff. The trusted bitmap proves that
RSTorrent hash-verified and synchronized those pieces before committing them;
cheap restart observations do not prove that an external actor has not since
changed same-length bytes. Ordinary fast resume may therefore miss a
same-length external mutation. Force recheck remains the explicit way to
re-establish present-byte integrity.

No user setting is introduced. Eligible fast resume is the default. The
stronger libtorrent `no_verify_files` behavior, a global clean-shutdown bit,
and a global crash-invalidates-catalog policy are explicitly rejected.

## Stopping Condition

The tactical is complete only when:

1. one task-free policy returns an exact per-torrent outcome equivalent to
   `Accepted`, `NeedsFullCheck`, `AwaitingStorage`, or `NeedsRepair`, with the
   reason represented by a closed typed value rather than diagnostic text;
2. ordinary eligible staging and published resumes accept only previously
   committed bits after bounded structural validation and admit zero payload
   hash jobs and zero payload bytes read;
3. completed, published, desired-running torrents remain immediately
   seed-eligible after an unrelated process death without entering checking;
4. missing, short, oversized, or truncated-slot content evidence invokes the
   existing complete checker for only the affected torrent, while wrong-kind,
   malformed-header, root-loss, and ambiguous-ownership cases retain their
   established non-checking recovery states;
5. Force recheck and every pending durable verification generation perform a
   complete selection-independent check, including after process death;
6. pre-sync, post-sync/pre-commit, and post-commit crash fixtures prove that
   only committed bits can be trusted, while uncommitted but physically
   present bytes remain safe false negatives and may be downloaded again;
7. path storage and the supported local Android SAF provider pass the same
   decision matrix, cancellation, resource, and exact-cleanup gates;
8. the full Rust, web-contract, Android build/runtime, and controlled restart
   gates below pass; and
9. owning topics, readiness truth, scenario evidence, and the execution record
   distinguish historical verification from a fresh full hash pass.

## Exact Trust And Fallback Contract

### Facts required before storage validation

The application service remains the durable authority. Before asking storage
to validate a resume, it requires:

- hash-authorized `raw_info` whose v1 identity and publication name match the
  torrent row;
- an exactly encoded have bitmap with the metainfo piece count;
- coherent `payload_state`, verification generations, file selection, root
  identity, and namespace generation;
- no removal, publication, repair, or quarantine transition which already
  owns recovery; and
- a healthy resolved path root or a currently usable supported platform
  capability.

Malformed durable facts remain fail-closed and torrent-local. Fast resume must
not turn them into a filesystem scan or use storage observations to repair a
database identity contradiction.

### Lightweight storage validation

The engine validates only the managed artifact side named by durable
ownership. It does not traverse ambient root content and does not adopt an
existing destination.

For a staging or published bitmap containing committed pieces:

- the expected namespace side exists, has the exact file/tree kind, is not a
  symlink or other unsupported object, and the opposite managed side is not
  simultaneously present;
- every logical non-padding payload artifact needed by at least one set have
  bit exists with file kind and the expected managed length;
- skipped-file segments needed by a set bit have either an exact retained
  logical source or a matching part-file slot;
- the part file, when required, has the expected magic, version, info hash,
  piece count, piece length, total length, unique in-range slot table, and
  sufficient file length for every trusted slot; and
- current route, selection, publication, and namespace generations agree.

An all-false but otherwise valid owned resume is coherent. Physical bytes
written before a failed checkpoint may be downloaded again instead of being
recovered by startup hashing. A future explicit adoption/import feature may
choose a full checker for unowned existing data, but this tactical does not
weaken current collision or managed-ownership rules.

Path and supported local SAF storage use the same logical decision. Exact
existence, kind, and length are sufficient for this slice. The optional
`StorageObservation.opaque_token` is neither persisted nor required; its
absence alone must not trigger checking. This tactical adds no timestamp,
inode, document-ID, provider-token, or per-artifact observation snapshot to
SQLite. Unsupported/offloaded/cloud provider behavior is not promoted to the
supported fast path.

Validation completes before the bitmap becomes runtime authority, peer
requests begin, or incoming seeding registers. Cancellation or failure before
the final decision makes no partial bitmap, verification-generation, route,
or registration change.

### Outcome matrix

| Condition | Required outcome |
| --- | --- |
| Coherent durable facts and all required managed observations match | Accept the committed bitmap; zero payload hashes and reads |
| Valid owned resume has a missing, short, oversized, or truncated-slot source which the ordinary checker can inventory | Start or resume one full-check generation for that torrent only |
| Force recheck is pending or explicitly requested | Full check regardless of matching observations |
| Root/grant is unavailable, permission is lost, or provider work cannot currently open content | `AwaitingStorage`; no global or speculative check |
| Durable identity, have encoding, payload ownership, namespace sides, file kind, symlink state, or part header/slot table are malformed or ambiguous | Existing repair/quarantine transition; no mutation or adoption |
| Complete published torrent is paused | Remain idle; validate when resumed or explicitly rechecked |
| Complete published torrent is desired-running and validation accepts | Register seeding without checking |
| Optional opaque observation token is absent | No effect when required exact facts are present |
| Bytes changed without changing any required structural fact | Fast resume may accept; Force recheck must detect the hash mismatch |

Fallback reuses Tactical `108`'s full checker and Tactical `105`'s durable
verification generations. It is not a partial-hash frontier and does not
clear or replace durable have state until the existing generation-matched
complete-check commit.

## Crash And Integrity Argument

Tactical `052` established the one-sided ordering used here:

```text
piece hash matches
  -> every touched payload/part destination syncs
  -> one SQLite have-bitmap transaction commits
  -> the committed bit may be trusted on restart
```

Death before the SQLite commit can leave bytes physically ahead of the
bitmap, never a committed bit ahead of synchronized verified bytes. Fast
resume deliberately preserves that asymmetry: it trusts committed positives
after structural validation and treats false bits as missing work. It does not
claim that filesystem metadata is cryptographic evidence or that storage
hardware can never violate its synchronization contract.

No clean-shutdown marker improves this per-torrent fact ordering. A crash in
torrent B cannot invalidate already committed torrent A, while a pending
verification generation already identifies the exact torrent whose full
checker must continue after restart.

## Resource And Work Bounds

- Durable have state remains at most 2,097,152 bits, or 262,144 bytes.
- Metainfo remains at most 374,998 files. Validation performs at most one
  bounded bitmap scan, one bounded logical-file scan, and one observation per
  relevant logical artifact; it does not perform a file-by-piece nested scan.
- Relevant file indices use a compact bounded set no larger than the existing
  metainfo/file-selection geometry. No per-block or unfinished-request resume
  map is added.
- Opening a part file may read its existing aligned slot header, at most
  8,389,632 bytes at maximum piece geometry. It reads no part payload.
- Android observation requests reuse the existing four provider workers,
  16-request broker bound, and 40-handle shared pool. Validation adds no new
  executor, unbounded queue, retained descriptor manifest, or handle class.
- At most the current active-download limit admits validation generations.
  Completed seed reconciliation remains bounded by the existing registration
  and storage-pool owners rather than spawning one task per catalog row.
- One accepted/rejected structured diagnostic is emitted per admission
  generation. No per-file diagnostic history or observation snapshot is
  retained.

The implementation records validation wall time, artifact observations,
part-header bytes, payload bytes read, and hash-job counts for the controlled
and scale fixtures. The accepted path's hard payload bounds are exactly zero
bytes and zero hash jobs; elapsed-time comparisons are observations, not a
support claim.

## Owner, Cancellation, And Dependency Direction

The feature introduces no second checker, scheduler, or service:

- `rstorrent-session` derives validation intent from durable facts. A pending
  verification generation means `Full`; an ordinary coherent resume means
  `FastEligible`. It owns the final lifecycle transition and torrent-local
  containment.
- A task-free engine policy combines intent, resumed namespace state, bitmap
  shape, and the storage result into the closed admission outcome. It has no
  SQLite, socket, platform, clock, or task dependency.
- Shared immutable artifact geometry and `StorageFileReference` observations
  identify exact path or platform artifacts. `SelectiveStorage` retains route,
  part-file, selection, materialization, and publication state transitions.
- The existing content/check task performs cancellable structural validation
  before either accepting the bitmap or entering Tactical `108`'s checker.
  The existing application lifecycle owner joins it on pause, removal,
  replacement, and shutdown.
- Completed published seeding consumes the same validation result before
  `SeedContent` registration. Upload remains a read-only owner and does not
  depend on write-side mutable storage.

The concrete refactor is the removal of the unconditional resume recheck from
`driver::run_selective_download` behind one explicit validation-policy seam.
Do not create a generic storage trait, resume framework, repository layer,
global recovery coordinator, or new crate.

## Source-First Record

### Pinned libtorrent oracle

The required reference is libtorrent `2.0.13` at exact pinned commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. This tactical inspected:

- `src/mmap_disk_io.cpp::do_check_fastresume` and
  `src/posix_disk_io.cpp::async_check_files`, which initialize storage,
  validate present resume data, request a full check when validation rejects,
  and check existing files when resume data is absent;
- `src/storage_utils.cpp::{verify_resume_data,has_any_file}`, where a seed
  checks relevant file presence and minimum length, while a partial resume
  requires files touched by completed pieces to exist without hashing them;
- `src/torrent.cpp::on_resume_data_checked` and its `no_verify_files` branch,
  which either install accepted resume pieces or enter full checking;
- `include/libtorrent/torrent_flags.hpp::no_verify_files`, the stronger
  caller-asserted skip-all-file-validation option which RSTorrent does not
  adopt as its default; and
- `include/libtorrent/settings_pack.hpp::no_recheck_incomplete_resume` plus
  its default-false definition in `src/settings_pack.cpp`, retained only to
  distinguish missing-data redownload policy from ordinary fast-resume
  validation.

Exact tests inspected include
`test/test_storage.cpp::{fastresume,fastresume_spanning_piece_missing_file,
check_files_oversized_mmap,check_files_oversized_posix}` and
`test/test_resume.cpp::{seed_mode_missing_files,
seed_mode_missing_files_with_pieces,seed_mode_no_verify_files}`. An isolated
out-of-tree Release build ran the complete `test_resume` and `test_storage`
binaries successfully on 2026-08-10. No source, fixture, resume encoding, or
test vector is copied.

Intentional differences are explicit:

- RSTorrent keeps its exact managed kind/length and namespace-ownership rules
  rather than accepting oversized final files merely because declared bytes
  are readable.
- RSTorrent's synchronized SQLite bitmap is the resume authority; it does not
  adopt libtorrent's bencoded resume format or public library flags.
- Existing unowned data remains a collision/explicit future adoption case,
  not an ambient `has_any_file` scan.
- Optional platform tokens are not persisted or compared in this slice.

### JSTorrent product history

The sibling JSTorrent checkout was inspected at commit
`9895410beeed6aff554053769bd006a3fbd373ef` without touching its existing
untracked documentation/attachment directories.
`packages/engine/src/core/torrent.ts::verifyResumeData` deliberately mirrors
libtorrent: complete torrents trust a bitfield after file presence/minimum-
size checks, partial torrents trust it when relevant storage exists, and
existing data without resume evidence requests a check.
`packages/engine/test/core/resume-listtree.test.ts` covers complete trust,
missing/short files, absent resume data, and bounded tree inventory.
`recheckData` remains the explicit hash path.

RSTorrent adopts this product expectation but retains its stronger managed
artifact, part-file, durability, and per-torrent containment contracts.

## Implementation Stages

1. **Freeze the current completed path.** Add direct assertions that a stable
   completed published torrent and a 500-torrent completed catalog perform no
   checker admission, hash job, or payload read after restart. Separate normal
   startup from the existing Force-recheck half of the fixture.
2. **Add the pure admission seam.** Represent ordinary fast eligibility,
   pending/explicit full verification, structural acceptance, fallback, root
   unavailability, and repair as closed task-free values. Unit-test the full
   outcome matrix before changing runtime behavior.
3. **Validate managed sources without payload reads.** Reuse artifact geometry
   and observations, deduplicate relevant logical files, validate part identity
   and trusted-slot extents, and preserve exact namespace and route fencing.
4. **Replace unconditional partial checking.** Accept committed bits on a
   matching ordinary resume; otherwise enter the unchanged full checker. Add
   typed diagnostics and exact validation counters without changing client
   contracts unless implementation exposes a genuinely user-relevant state.
5. **Converge completed seeding fallback.** Make desired-running completed
   torrents use the same decision; a structural rejection checks only that
   torrent, while paused complete torrents remain idle.
6. **Prove crash boundaries and trust risk.** Run the three checkpoint-death
   positions beside unrelated stable torrents, verify safe false-negative
   redownload, and prove a same-length mutation is accepted normally but
   detected by Force recheck.
7. **Close Android parity.** Run fake-platform, API 34 AVD, and, when explicitly
   authorized for this tactical, the supported-local-provider physical matrix
   with exact broker, descriptor, hash, payload-read, cancellation, repair,
   and cleanup observations. Do not infer cloud or external File Provider
   support.
8. **Reconcile repository truth.** Update the scenario ledger, persistence and
   SAF policy, refactoring topic, readiness queue, campaign checkpoint, and
   execution evidence.

Each implementation stage may land as a coherent commit. The first runtime
behavior commit must include both acceptance and forced-fallback tests; do not
leave an intermediate default which trusts without structural rejection.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure policy | Every outcome-matrix row; malformed bitmap/identity/generation; no partial mutation; deterministic reason values |
| Engine storage | Single-/multi-file, selected/skipped boundary pieces, padding, absent/short/wrong-kind/oversized files, missing/duplicate/truncated part slots, staging/published namespace sides, accepted zero payload reads/hashes, fallback full check |
| Session persistence | Ordinary versus pending Force generation, complete/partial/paused states, unrelated-torrent containment, malformed-row quarantine, atomic full-check replacement, no schema drift unless separately justified |
| Crash runtime | Pre-sync, post-sync/pre-commit, post-commit, crash during Force recheck, stable completed neighbor, safe false-negative redownload, exact final hashes and cleanup |
| Scale/performance | 500 stable complete rows plus a bounded mixed partial cohort; exact observation/header/payload/hash counts, wall time, handle/request high water, zero task/resource leakage |
| Controlled oracle | Pinned libtorrent accepts matching resume and rejects missing/truncated required files; compare behavior and exact final payload without public networking |
| Android | Both Rust ABIs, generated bindings check, Gradle compile/unit gates, API 34+ no-window AVD and, when explicitly authorized, the supported-local-provider physical path; grant loss/repair, Force recheck, exact cleanup |
| Repository | Rust baseline, affected web generation/typecheck/tests, owning documentation, clean reference status, `git diff --check` |

Run at minimum:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
```

Regeneration must be byte-identical unless a Rust application-boundary type
actually changes. Public-swarm and visible product-client runs are not
required; controlled process-death, libtorrent, and Android evidence exercise
the feature more directly.

## Required Scenarios

- Stable complete torrent A plus crashing torrent B at each checkpoint
  boundary: A never hashes; B trusts only committed bits.
- Partial staging restart with committed wanted, skipped, padding, and
  cross-file boundary pieces: matching sources accept without payload reads.
- Physically valid bytes with false bitmap entries remain missing work and may
  be downloaded again; they never become trusted merely because they exist.
- Missing, short, oversized, or replaced required logical sources disqualify
  only their torrent and run the complete checker when it can inventory them;
  wrong-kind or symlink artifacts enter repair without mutation.
- Missing or truncated part payload disqualifies trust and uses the complete
  checker; malformed identity, header, or duplicate/out-of-range slot tables
  enter repair before peer work begins.
- Complete published path and SAF torrents register for upload without hash;
  a disqualified completed torrent enters only its own checker.
- Root loss, grant revocation, provider refusal, repair, and late platform
  observation preserve stable root identity and generation fencing.
- Pause or shutdown during validation joins cleanly; paused completed torrents
  do no startup work.
- Explicit Force recheck and crash during Force recheck hash all physically
  readable logical pieces and settle the requested generation once.
- Same-length external corruption is deliberately accepted by ordinary fast
  resume, cannot be labeled freshly checked, and is detected and cleared by
  Force recheck.

## Observability And Product Truth

Emit bounded structured facts sufficient to distinguish:

- fast resume accepted, with committed piece count and observed artifact
  count;
- fast resume rejected, with one typed reason and full-check generation;
- validation deferred for storage availability or repair;
- explicit/pending Force recheck bypassing trust; and
- validation high-water counts for observations, part-header bytes, payload
  bytes, and hash jobs.

Do not present an accepted fast resume as a fresh recheck. Existing `Complete`
and verified-piece semantics remain historical durable verification facts.
Checker progress appears only when the complete checker actually runs. No new
setting, warning dialog, or recurring user prompt is required.

## Non-Goals

- No persisted clean-shutdown bit, session-wide invalidation, per-artifact
  observation table, timestamp/inode heuristic, or background periodic hash.
- No `no_verify_files`-style unconditional trust switch and no conservative-
  by-default product preference.
- No unfinished-block, peer-cache, transfer-history, partial-check-cursor, or
  libtorrent-compatible resume format.
- No adoption, merge, overwrite, relocation, or deletion of arbitrary
  existing user content; no change to publication collision policy.
- No concurrent peer transfer during a fallback full check and no second hash
  implementation.
- No generic filesystem/VFS trait, new crate, daemon, native host, socket
  proxy, platform callback payload path, or unbounded manifest.
- No cloud/offloaded/third-party Android provider, external iOS File Provider,
  Windows root, v2/hybrid torrent, or new protocol support claim.
- No claim that file metadata proves content integrity or that fast resume
  detects same-length external mutation.

## Escalation Contract

Once implementation is authorized, ordinary type/module naming, private
refactor placement, test-fixture construction, exact diagnostic fields, and
same-boundary bug fixes proceed autonomously. Existing AVD validation is in
scope. Physical Android use still requires explicit authorization and current
attachment for this tactical under the established testbed rules.

Stop for direction before adding a user setting, schema column, dependency,
new platform/provider support claim, background verification policy, payload
ownership/adoption behavior, concurrent checking and peer transfer, general
storage abstraction, destructive profile/payload mutation, or broader product
surface.

## Execution Record

Planned. Record implementation commits, exact validation commands, accepted
and fallback counts, payload/hash zeros, crash outcomes, high-water marks,
controlled oracle results, Android evidence, and deliberate deferrals here as
the bounded stages land.
