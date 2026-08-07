# Tactical 105: Fact-Based Persistence And Recheck Containment

Status: Accepted (2026-08-07); implementation not started.

Topics: `application-control`, `capability-readiness`, `client-persistence`,
`download-correctness`, `storage-throughput-architecture`

## Motivation And Outcome

An ordinary `./scripts/webui` profile failed to open with:

```text
Configuration("stored payload ownership and storage state are inconsistent")
```

The schema-13 SQLite database passed `PRAGMA integrity_check`, but one running
torrent retained `storage_state = staging` and
`managed_artifacts = published`. Its final owned payload existed, its staging
artifact did not, and its part file existed. The command history crossed file
selection, force recheck, and resume before new verified pieces became
durable. `SessionStore::record_pieces` then unconditionally rewrote
`storage_state` to `staging` without changing `managed_artifacts`. The live
process did not revalidate the cross-product; the next process did, and one
torrent-specific contradiction aborted the whole application service.

This is not SQLite corruption and is not only an old-profile migration gap. A
fresh profile can reach the same row because a generic piece checkpoint is
allowed to mutate a publication/workflow field. The persisted model contains
overlapping descriptions of runtime lifecycle and artifact ownership, while
the read and write paths do not share one enforced transition authority.

Replace that shape with fact-based durable torrent state:

- persist user intent, source, selection, root identity, one payload ownership
  fact, verification evidence, and explicit restartable operation intent;
- derive checking, downloading, publishing, complete, and seeding as runtime
  application state rather than independently writable database facts;
- make force recheck an exclusive, restartable validation request that admits
  no peer, discovery, upload, download, publication, or competing storage work;
- make piece checkpoints update only verified-piece evidence;
- recover the known schema-13 contradiction conservatively without moving or
  deleting payload data; and
- quarantine a malformed torrent while opening the rest of the profile.

Complete this tactical when the known profile shape migrates without payload
mutation, force recheck survives every declared crash boundary, no ordinary
piece or selection operation can construct an invalid ownership state, and a
malformed torrent cannot prevent healthy torrents and settings from opening.

## Scope

- Advance the session schema from version 13 to version 14.
- Remove durable torrent lifecycle as an independently mutable authority.
- Replace `storage_state` plus `managed_artifacts` with one closed payload
  record that combines RSTorrent ownership and the only restart-relevant
  namespace/publication intent.
- Persist a bounded verification request/completion generation and atomically
  replace its associated have state.
- Derive the existing application-facing `TorrentState`, storage presentation,
  progress, action availability, incoming-seed eligibility, and startup work
  from durable facts plus current runtime owners.
- Fence force recheck across active download, peer, discovery, incoming seed,
  file-pool, hash, write, checkpoint, and publication owners before the
  validation request becomes successful.
- Reuse the existing bounded all-wanted full checker; do not introduce a
  second hashing implementation or optimistic fast-resume path.
- Migrate every supported historical schema through version 14, including
  legacy-owned artifacts and the exact newly observed contradiction.
- Change application startup so a torrent-local metadata, bitmap, root, or
  artifact problem produces a bounded quarantined torrent record and
  diagnostic rather than aborting the profile.
- Add deterministic schema, state-reducer, runtime, restart, controlled path,
  and platform-capability evidence proportional to the changed boundary.
- Repair the maintainer profile only after the implementation and migration
  pass against an isolated copy. Preserve a recoverable backup and never
  modify its payload roots as part of migration.

## Non-Goals

- Optimistic fast resume, timestamp/inode trust, partial recheck frontiers, or
  persisting a checking cursor.
- Changing SHA-1 authority, piece geometry, selection semantics, publication
  names, part-file layout, or path/SAF payload I/O.
- A new torrent queue, concurrent multi-torrent checking policy, or background
  repair campaign.
- Relocation, adoption of arbitrary files, merge/overwrite behavior, automatic
  suffixing, or deletion of data whose ownership is ambiguous.
- A new visible repair UI, modal, settings page, or protocol command. Existing
  snapshots and actions may change internally while retaining their semantic
  product contract.
- A new database abstraction, persistence trait hierarchy, event-sourcing
  framework, actor system, daemon, or external service.
- Public-swarm, visible-client, or physical-device evidence. Controlled local
  path and platform-capability cases are sufficient unless implementation
  changes platform code beyond the existing boundary.
- Preserving provisional schema-13 column compatibility after its one-way
  transactional migration. RSTorrent remains unreleased.

## Durable Facts And Derived State

### Durable torrent facts

The schema retains only values that remain meaningful after the process and
all of its tasks cease:

- torrent identity and exact source provenance;
- verified raw info and normalized discovery inputs;
- storage-root identity and publication name;
- user run intent (`running` or `paused`) and archive/removal intent;
- file selection;
- one payload record;
- the verified-piece bitmap plus its verification generation;
- a requested verification generation; and
- a bounded quarantine reason or last operation failure when needed for
  recovery and presentation.

Internal names may change, but the payload record has one closed semantic
value equivalent to:

```text
absent
legacy_owned
work_owned
publication_pending
final_owned
```

`publication_pending` is not a claim that an old process is still publishing.
It is durable operation intent spanning the non-atomic SQLite/filesystem or
SQLite/provider boundary. On restart, the publication owner inspects the
exact work and final locations, rejects symlinks/wrong types/both-side
ambiguity, rechecks the one justified side, and completes or quarantines the
operation. No second ownership column may restate this value.

The stored bitmap is authoritative only when its completed verification
generation equals the requested generation. A force-recheck request advances
the requested generation in the same transaction as its idempotent command
receipt. Successful checking atomically replaces the entire bitmap and sets
the completed generation to the requested generation. Checked overflow of the
bounded nonnegative generation fails the request without mutation.

The existing conservative startup policy may request another full check. If a
request is already incomplete it reuses that generation; otherwise it
advances the request once before starting. A crash therefore leaves an
unambiguous pending validation rather than a durable claim that a task is
currently checking.

### Runtime-derived state

The application derives, but does not independently persist, states such as:

- awaiting metadata;
- awaiting storage capability;
- queued for check or checking;
- downloading or paused;
- awaiting publication or publishing;
- complete;
- seeding; and
- quarantined/needs repair.

The derivation consumes durable facts and observable current owners. A process
cannot restore an old network, hash, publication, or checking task merely
because a row once named that activity. Existing application DTOs may retain
their current state vocabulary, but SQLite is no longer a second runtime state
machine underneath them.

### Mutation authority

- Source intake alone creates metadata/source facts.
- Selection commands alone change selection intent.
- Force recheck and invalidating operations alone request a new verification
  generation.
- The full-check completion transaction alone replaces the complete bitmap
  and satisfies a requested verification generation.
- Piece durability may set current-generation have bits only after the
  existing payload-sync fence; it may not change the payload record,
  publication intent, desired state, or quarantine state.
- The storage/publication owner alone advances `absent -> work_owned ->
  publication_pending -> final_owned`.
- Removal alone clears ownership after its existing exact cleanup contract.
- Recovery may map an old or interrupted record only from declared database
  and exact filesystem/provider observations. It never guesses by content
  name or adopts an unowned path.

These authorities must be expressed in small store functions or a pure
transition reducer so an SQL statement cannot silently update an unrelated
domain.

## Force-Recheck Contract

Force recheck means exclusive integrity validation, not a sensitive variant
of ordinary download startup.

1. Acquire the torrent lifecycle fence so no replacement generation can be
   admitted.
2. Stop discovery and tracker/DHT advertisement, unregister incoming seeding,
   disconnect peers, cancel requested blocks, close download admission, and
   join outstanding writes, hashes, durability checkpoints, and publication
   work for that torrent.
3. In one transaction, advance the requested verification generation and
   commit the successful request receipt. Preserve the separate user run
   intent and file selection.
4. Release retained file handles and open current managed artifacts afresh.
5. Run the existing bounded all-wanted piece checker with no peer, upload,
   download, discovery, or publication activity admitted concurrently.
6. Atomically replace the exact bitmap and satisfy the request generation only
   after all checking and required recovered-target synchronization joins.
7. Derive the next state. Paused intent remains paused. Running intent repairs
   only missing wanted pieces, publishes when required, or becomes complete
   and seed-eligible if all required content is valid.

If cancellation or process death occurs before step 3, no successful command
was committed. If it occurs after step 3 and before step 6, the incomplete
generation forces the next process to repeat the check. If it occurs after
step 6 but before runtime restart, the exact bitmap is valid and startup
derives the next action without replaying an old task. Command replay never
requests another generation.

Checking progress, queue membership, task handles, cancellation state, and
the previous process's active/inactive status remain runtime-only.

## Per-Torrent Failure Containment

Profile-wide failure remains correct for an unsupported schema, failed schema
transaction, SQLite corruption, profile identity mismatch, or inability to
establish the database's configured durability policy. Those failures make
the profile authority itself unavailable.

After the schema opens, failures scoped to one torrent do not abort the
application service. Invalid raw info, have encoding, selection, missing root,
impossible legacy mapping, ambiguous artifact existence, symlink/wrong type,
or an interrupted operation with no safely identifiable side must:

- retain the catalog row and any known ownership fact;
- store or derive one bounded quarantine reason;
- admit no download, upload, discovery, checking, publication, or removal that
  could mutate payload data automatically;
- remain inspectable and removable with keep-data semantics;
- expose Force recheck only when exact managed sources are safely identifiable;
  and
- allow healthy torrents, settings, views, authentication, and application
  shutdown to operate normally.

Startup processes all catalog rows and may queue bounded recovery work only
after the complete profile snapshot is available. One torrent error must not
short-circuit the loop or escape `ApplicationService::open`.

## Schema-13 Migration And Recovery

Migration is one SQLite transaction preceded by bounded, read-only inspection
of at most the exact final, staging, and part paths derived from verified
metadata and recorded roots. It performs no payload rename, write, truncate,
or deletion and no content hashing. Hash validation occurs through the normal
post-open checker.

| Schema-13 facts | Physical observation | Version-14 result |
| --- | --- | --- |
| `none + none` | No owned artifact | `absent`; preserve bitmap shape but request validation when metadata/artifacts require it. |
| `staging + staging` | Exact work side only | `work_owned`; request validation. |
| `prepared + staging/published` | Exact work or final side only | `publication_pending`; restart reconciles and checks that side. |
| `published + published` | Exact final side only | `final_owned`; request validation under the conservative restart policy. |
| `published + legacy` | Exact historical hash-owned artifact | `legacy_owned`; retain supported read/check/keep/delete semantics without renaming it. |
| `staging + published` | Exact final side only | Recover the known piece-checkpoint defect as `final_owned`; request validation. |
| `staging + published` | Exact work side only | Recover as `work_owned` only when the path is already justified by the recorded torrent identity; request validation. |
| Any mapping | Both sides, wrong type, symlink, missing required identity, or otherwise ambiguous | Preserve paths and catalog; quarantine the torrent; open the profile. |
| Existing `needs_repair` or invalid bitmap | Any | Preserve the safely mapped payload fact, request no automatic mutation, and quarantine with a bounded reason. |

Migration tests must begin from independently constructed schema-1, schema-6,
schema-8, schema-12, ordinary schema-13, legacy schema-13, and exact defective
schema-13 fixtures. They must verify related sources, trackers, selections,
settings, receipts, prepared manifests, removal jobs, and revision semantics.
No migration test may use the maintainer's live database as its fixture.

Before repairing the maintainer profile, use SQLite backup or a controlled
closed/checkpointed copy, run the version-14 migration and startup tests on the
copy, confirm that payload files are byte-for-byte untouched, and retain a
recoverable pre-migration backup. The final handoff must state exactly what was
backed up, migrated, and verified; implementation authority does not include
deleting that backup.

## Stable Scenarios

| ID | Scenario | Required result |
| --- | --- | --- |
| T105-S01 | Published content is force-rechecked, some selected pieces are missing, and new pieces become durable in the final namespace. | Piece checkpoints update only have evidence; payload remains `final_owned`; restart succeeds. |
| T105-S02 | Process death occurs after a recheck request commits but before any hash, during hashing, or after bitmap commit before network restart. | The first two reopen with the same pending generation and repeat the check; the last derives the next action from the completed generation. |
| T105-S03 | Force recheck is requested while downloading or seeding. | Every peer/discovery/upload/download/storage owner joins before checking; no payload or protocol activity overlaps the checker. |
| T105-S04 | Force recheck is requested while paused. | User intent remains paused before, during, and after the check; invalid pieces do not trigger peer repair. |
| T105-S05 | File selection expands on final-owned partial content. | Current artifacts are rechecked, only newly required missing pieces download, and durable piece batches never relabel payload ownership. |
| T105-S06 | One catalog row has malformed metadata, have state, root identity, or ambiguous artifacts while another row is healthy. | Profile open succeeds; the bad torrent is quarantined and inactive; the healthy torrent and settings operate. |
| T105-S07 | Schema 13 contains the exact `staging + published`, final-only incident shape. | Migration produces `final_owned`, requests validation, changes no payload byte, and launches successfully. |
| T105-S08 | Schema 13 contains legacy completed rows. | Migration retains explicit legacy ownership and never turns compatibility history into an application-wide launch error. |
| T105-S09 | Publication dies before intent, after intent, after rename, and after namespace/provider durability. | The single payload record identifies the valid recovery rule without a second ownership column; exact final bytes and no-replace behavior remain unchanged. |
| T105-S10 | A successful force-recheck request is replayed or races an equivalent pending request. | At most one generation is requested and one check owner is admitted; the replay returns the original semantic result. |
| T105-S11 | Application shutdown or removal arrives during checking. | New hash work closes, admitted work joins, no stale completion commits, and keep/delete policy sees one exact ownership fact. |
| T105-S12 | A per-torrent runtime failure occurs after profile open. | It becomes bounded diagnostic/quarantine or retry state for that torrent and cannot poison the store mutex or profile lifecycle. |

## Owner, Task, Cancellation, And Dependency Map

```text
semantic command
  -> per-torrent lifecycle fence
       -> join discovery / incoming seed / peers / download / publication
       -> SessionStore verification request transaction
       -> one supervised full-check task
            -> existing bounded storage/hash owners
            -> atomic have + completed-generation transaction
       -> application runtime-state derivation
       -> optional running-intent repair/seeding generation

SessionStore
  -> durable facts and pure transition validation
  -X-> task handles, channels, sockets, file handles, checking progress

publication owner
  -> sole payload-record mutation authority

piece checkpoint owner
  -> have evidence only
  -X-> payload ownership or lifecycle state
```

The application service owns the lifecycle fence and supervised task. The
engine owns deterministic layout, hashing, storage I/O, and payload buffers.
The store owns schema, transactions, idempotent requests, and fact validation.
Platform adapters retain only their existing capability and namespace roles.
Dependency direction remains `platform/application -> session -> engine ->
protocol`; SQLite and runtime task types do not move inward.

The existing one-active-content-generation and bounded hash/storage limits
remain initial ceilings. This tactical adds no unbounded collection, queue, or
payload buffer. Quarantine detail remains within the existing 1,024-byte error
bound. Migration inspects a constant three derived paths per torrent and does
not hash during the schema transaction. Verification generation uses a
checked, nonnegative SQLite integer and cannot wrap.

## Reference Dossier

No BEP defines application persistence, force-recheck UI semantics, or local
publication layout. BEP 3 piece hashes remain the integrity authority; this
tactical changes only application/session ownership and crash recovery.

Pinned libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected at:

- `src/torrent.cpp`: `torrent::force_recheck`,
  `torrent::on_force_recheck`, and `torrent::start_checking` disconnect peers,
  stop announcing, clear have authority, release file handles, enter the
  checking queue, and resume normal behavior only after checking.
- `include/libtorrent/torrent_handle.hpp`: the `force_recheck` contract states
  that peers disconnect, tracker announcement stops, resume assumptions are
  discarded, all files are checked, and peer connections resume afterward.
- `test/test_recheck.cpp`: `TORRENT_TEST(recheck)` repeats force recheck on a
  seed and requires completion both times.
- `test/test_priority.cpp`: the force-recheck case retains piece priorities
  across checking and returns to finished selective state.

RSTorrent adopts the exclusive network/check boundary, fresh file observation,
selection preservation, and re-entry behavior. It does not adopt libtorrent's
class graph, queue implementation, resume-data encoding, or runtime status as
SQLite schema.

JSTorrent sibling revision
`9895410beeed6aff554053769bd006a3fbd373ef` was inspected at:

- `packages/engine/src/core/torrent.ts`: `_doCheckPieces` resets the runtime
  bitfield, closes handles, checks current files, and persists the rebuilt
  evidence; `recheckData` stops network activity and resumes it afterward.
- `packages/engine/src/core/torrent-queue-manager.ts`: check queue membership
  and active checking are runtime collections, and normal networking resumes
  from separate user state after completion.
- `packages/engine/src/core/torrent-initializer.ts`: file observations set a
  `needsDataCheck` fact instead of restoring a previous checking task.
- `packages/engine/src/core/session-persistence.ts`: restore catches failure
  per torrent and continues the catalog rather than failing the whole session.
- `packages/engine/test/core/recheck-manifest.test.ts` and the recheck cases in
  `packages/engine/test/core/bt-engine.test.ts`: recheck replaces verified
  evidence and remains part of file mutation recovery.

RSTorrent adopts the separation of user intent, runtime checking ownership,
and per-torrent restore containment. It intentionally retains its stronger
full managed-piece check, typed SQLite authority, crash-safe publication, and
bounded native storage execution.

SQLite transaction, WAL, backup, foreign-key, and durability requirements
remain those accepted in `client-persistence.md`. Filesystem and provider
namespace changes cannot be made atomic with SQLite; the single
`publication_pending` fact remains the explicit recovery bridge.

## Staged Implementation

1. **Freeze the incident as evidence.** Add schema-13 fixtures and a runtime
   regression that reaches final-owned partial content, force recheck,
   durable new pieces, process death, and reopen. Prove the current candidate
   fails for the recorded reason before changing the model.
2. **Install pure durable-fact transitions.** Define the single payload record,
   verification generations, quarantine classification, and runtime-state
   derivation independent of SQLite, Tokio, paths, and platform handles. Cover
   every legal and rejected transition exhaustively.
3. **Advance schema 13 to 14.** Transactionally rebuild the torrent table and
   related constraints, migrate historical and defective rows from bounded
   exact observations, retain revision/receipt semantics, and quarantine
   ambiguity without payload mutation.
4. **Narrow mutation owners.** Make piece checkpoints have-only, route payload
   record changes exclusively through storage/publication/removal, and replace
   store methods that write compound runtime status with fact-specific
   transactions.
5. **Rebuild application state derivation.** Remove persisted checking,
   downloading, publication, complete, and repair values as runtime authority;
   derive existing views/actions from facts and current owners. Make startup
   enumerate and contain torrent-local errors.
6. **Fence force recheck.** Join all declared owners, commit one idempotent
   verification request, run the existing full checker exclusively, atomically
   replace evidence, and resume only from preserved user intent.
7. **Close crash and platform cases.** Re-run publication gates, path and
   dynamic-provider recheck, selection expansion, removal/shutdown, and every
   generation boundary. Add no second checker or platform persistence model.
8. **Validate an isolated maintainer-profile copy.** Back up through SQLite's
   supported mechanism, migrate the copy, prove exact payload non-mutation and
   successful launch, then stop for explicit approval before touching the
   actual profile.
9. **Graduate documentation and repair.** Run workspace and controlled gates,
   update owning topics/readiness claims and this completion record, then—with
   explicit approval—migrate the closed maintainer profile and verify launch.

Every intermediate commit must pass focused store/session tests and must not
leave two production persistence writers or a schema that current startup can
open partially.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Exhaustive payload-record transitions; verification request/complete/replay/overflow; derived states for metadata/root/intent/selection/have/runtime-owner combinations; quarantine classification. |
| Schema | Fresh v14; every supported historical migration; exact ordinary, legacy, defective, ambiguous, and needs-repair v13 fixtures; constraints reject invalid values; related rows, revisions, receipts, and page bounds retained. |
| Store | Piece batches cannot mutate payload facts; only publication/removal transitions can; atomic bitmap-generation replacement; failed/replayed/stale requests leave exact state. |
| Scripted runtime | Active download, completed seed, paused torrent, selection expansion, recheck re-entry, cancellation, removal, shutdown, store error, and one-bad/one-good startup containment. Assert zero peer/discovery/upload/download/publication overlap during checking. |
| Crash/restart | Death before request commit, after request commit, during hash admission, after hashes before bitmap commit, after bitmap commit before runtime resume, and every retained publication gate. |
| Path storage | File/tree, work/final/part existence matrix, symlink/wrong type/both/neither, published partial repair, exact hashes, no payload mutation during migration, and exact keep/delete cleanup. |
| Platform capability | Dynamic handles reopen fresh, provider rename acknowledgement, grant loss, pending recheck restart, existing handle/request bounds, and no stale callback satisfying a newer generation. |
| Controlled interoperability | Pinned libtorrent seed for force recheck of complete, partial, corrupt, and selection-expanded path torrents; seed unavailable on restart when existing bytes suffice; exact requested pieces and final hashes. |
| Product headless | `./scripts/webui --no-open` against an isolated migrated incident fixture opens, exposes the quarantined/healthy catalog as applicable, and shuts down joined; no visible browser. |
| Workspace | Formatting, warning-denying clippy, workspace tests, generated-contract drift checks, affected web tests, Android Rust cross-build if shared session/platform code changes, and `git diff --check`. |

The controlled harness must report old/new schema, payload-record mapping,
verification generations, old/new have counts, artifact observations,
requested pieces, payload hashes, owner high-water marks, terminal
classification, and cleanup. It must redact source URLs, paths outside its
temporary root, peer addresses, and payload names.

Baseline commands, with exact results recorded on completion:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Escalation Contract

Within this tactical, ordinary module extraction, internal naming, schema-14
table/constraint details, pure reducers, store API replacement, deterministic
fault injection, generated-contract regeneration required to preserve current
semantics, and fixes at the same ownership boundary do not require additional
approval.

Stop for direction before:

- deleting, renaming, truncating, merging, adopting, or overwriting any
  existing payload or profile backup;
- touching the maintainer's actual profile rather than an isolated copy;
- weakening full piece-hash authority or trusting a migration observation as
  verified content;
- retaining two durable fields that independently describe payload ownership
  or runtime lifecycle;
- changing visible force-recheck, pause, selection, removal, or publication
  product semantics beyond the contract above;
- adding a dependency, process, service, database authority, public API,
  visible UI, emulator/device run, or public-network requirement outside the
  stated scope; or
- discovering a case that cannot be recovered or quarantined without a new
  user-data ownership policy.

## Completion Documentation

When the stopping condition passes:

- update `client-persistence.md` with the final fact schema, migration table,
  and per-torrent quarantine boundary;
- update `application-control.md` with the exclusive restartable force-recheck
  lifecycle;
- add the passing incident and containment scenarios to
  `download-correctness.md`;
- update `storage-throughput-architecture.md` so checkpoint mutation authority
  cannot drift again;
- restore the readiness claim in `capability-readiness.md` only after the
  schema, restart, controlled path, and headless product gates pass; and
- append implementation commits, exact validation, resource high waters,
  isolated-profile backup/migration evidence, actual-profile disposition, and
  deliberate deferrals here.

## Completion Record

Implementation, validation, and maintainer-profile repair have not started.
