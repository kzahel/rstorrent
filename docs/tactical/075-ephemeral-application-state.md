# Tactical 075: Ephemeral Application State

Status: Accepted for implementation on 2026-08-04 after completion of
Tactical `074`. Implementation has not started.

Topics: `client-persistence`, `application-control`, `client-surfaces`,
`capability-readiness`

Dependency: completed Tactical
[`074`](074-context-specific-metainfo-limits.md) supplies the named durable
metainfo and have-state resource profiles reused by this slice.

## Decision And Motivation

Add an explicit application persistence mode whose session catalog, request
receipts, settings, verified metadata, have state, DHT snapshot, and speed
history exist only for one `ApplicationService` lifetime and create no profile
database or auxiliary profile file.

RSTorrent currently requires a profile root. Opening the application creates
`session.db` with WAL durability and separately opens `metrics.db`. Tests use
isolated temporary directories and remove them afterward, but they still
exercise disk-backed databases and therefore are disposable rather than
memory-only.

Ephemeral state is a useful product capability and a stronger test seam. It
also prevents future `.torrent` intake and source-retention work from assuming
that every accepted source has a persistent filesystem location.

Ephemeral application state is not the same as RAM-backed torrent content. An
explicit path or platform payload root remains an external capability and may
be written when the caller starts content. With no content start and no
platform client persistence, the application can run without persistent
application-state or payload writes. A fully memory-backed content-storage
engine is a separate tactical.

## Desired Outcome And Stopping Condition

The tactical stops when:

- `ApplicationConfig` selects durable or ephemeral persistence explicitly;
- durable mode retains the existing profile-root, WAL, `synchronous=FULL`,
  migration, restart, backup, and platform behavior unchanged;
- ephemeral mode uses private SQLite in-memory databases for session state and
  speed history, applies the same schema and transactional semantics, and
  creates no profile directory, `session.db`, `metrics.db`, WAL, shared-memory,
  journal, temporary SQL, DHT snapshot, or other application-state file;
- request idempotency, revisions, settings, metadata, have state, views,
  diagnostics, live DHT state and its snapshot write path, and speed history
  work for the lifetime of one ephemeral service and disappear only after the
  service shuts down, joins its owners, is dropped, and closes its private
  SQLite connections;
- two ephemeral services are isolated and a newly opened service restores no
  state from an earlier one;
- the session and metrics in-memory databases have explicit page/byte maxima
  and surface exhaustion as a bounded resource failure rather than aborting or
  silently falling back to disk;
- at least one supported headless entry point can select the mode without a
  fake profile path; and
- deterministic, application-lifecycle, no-file, persistent-regression, and
  workspace tests pass with actual resource high water recorded.

## Mode Contract And Initial Bounds

The conceptual configuration is:

```text
ApplicationPersistence =
  Durable { profile_root }
  | Ephemeral
```

Exact Rust names may follow existing conventions. `profile_id` remains a
bounded service-instance identity in both modes; it does not imply that an
ephemeral database can be reopened.

Initial bounds are:

- session in-memory database: 128 MiB maximum page space;
- metrics in-memory database: 32 MiB maximum page space;
- request receipts: existing maximum of 1,024;
- torrent metadata, piece state, diagnostics, view queues, DHT state, and
  speed retention: their existing owner-specific bounds; and
- SQLite temporary storage: memory only in ephemeral mode.

Implementation computes and verifies `max_page_count` from SQLite's actual
page size instead of assuming 4 KiB. It may tighten the database maxima from
measured evidence but may not raise them without direction. SQLite errors
caused by the cap must map to a typed application `resource_limit` failure
with no partially accepted command. The same typed classification applies to
SQLite `FULL` from a durable database; it improves failure reporting without
changing durable journal, synchronization, or fallback behavior.

The page maxima bound main-database page space, not total process memory.
SQLite rollback journals, temporary in-memory structures, allocator overhead,
Rust materialized state, and payload buffers retain their own policies and
measured high-water evidence. No process-global SQLite heap limit is added.

Ephemeral mode does not weaken hostile-input, network, queue, task, descriptor,
or payload-memory limits. It changes durability and filesystem effects only.

## Stable Scenarios And Edge Cases

- Opening and cleanly shutting down an empty ephemeral service creates no
  profile or database file.
- Adding a paused metadata-only magnet, accepting verified metadata, changing
  selection/settings, opening and updating views, recording speed, and saving
  a DHT snapshot all work within the service lifetime without a profile file.
- Replayed request IDs return the same receipt while the service lives;
  conflicting reuse still fails.
- Closing the last view set or transport connection does not clear ephemeral
  state while its `ApplicationService` remains alive.
- After shutdown and drop, a new ephemeral service with the same `profile_id`
  starts at revision zero with no torrent, settings, DHT, metric, or receipt
  state from the prior service.
- Two simultaneously open ephemeral services with the same profile ID do not
  share an SQLite cache or application state.
- SQL sorting, migration, and temporary indexes cannot spill to a temporary
  disk file.
- Reaching a page cap leaves the current transaction unapplied and the service
  responsive enough to report and shut down.
- A metadata-only ephemeral session using an already provisioned external
  payload root creates no output, staging, part, or new root artifact. Starting
  content against an explicitly configured filesystem or platform root is
  allowed to write payload there and is reported truthfully as outside the
  no-profile-file guarantee.
- Durable service restart, WAL verification, metrics restore, DHT warm state,
  and conservative torrent resume continue to pass unchanged.
- Failure to open durable persistence never falls back to ephemeral mode.
  Ephemerality is explicit, not a corruption or permission recovery policy.

## Scope

- Add one plain persistence-mode value to application configuration while
  preserving an ergonomic durable constructor for existing platform clients.
- Refactor `SessionStore` construction so a supplied SQLite connection shares
  schema migration, foreign keys, transactions, receipts, snapshots, and
  domain queries, while durable and memory connections apply different
  verified journal/synchronous policies.
- Add a private `Connection::open_in_memory` session path. Do not use an empty
  filename or SQLite temporary database because either may allocate a disk
  file.
- Refactor speed-history preparation similarly so ephemeral metrics use a
  separately bounded private in-memory SQLite connection and the same query
  and retention behavior.
- Set and verify `foreign_keys=ON`, `journal_mode=MEMORY`,
  `synchronous=OFF`, `temp_store=MEMORY`, actual page size, and maximum page
  count on each in-memory connection. `journal_mode=OFF` is forbidden because
  it disables atomic commit and rollback. Durable connection policy remains
  WAL plus `synchronous=FULL`.
- Add an application `resource_limit` error code for SQLite `FULL`, regenerate
  the checked TypeScript/schema/Kotlin semantic artifacts, and preserve full
  transaction rollback. No visible client behavior is added.
- Treat background metrics-cap exhaustion as degraded history persistence,
  emit one bounded diagnostic, and continue serving the retained live history;
  it is not attached to an unrelated command response. Do not add an
  `Ephemeral` speed-persistence presentation state.
- Keep DHT snapshot persistence, settings, request receipts, metadata, and
  have state in the in-memory session schema so runtime behavior remains the
  same until close.
- Make the headless session application and/or gateway select ephemeral mode
  explicitly with mutually exclusive configuration. The exact CLI or
  environment spelling may follow existing entry-point conventions.
- Add diagnostics that identify persistence mode and bounded exhaustion
  without including paths, database contents, tracker credentials, or source
  metadata.
- Update focused topics and readiness with the implemented mode and exact
  evidence.

## Non-Goals

- A memory-backed torrent payload, part file, staging tree, publication
  backend, filesystem emulator, tmpfs manager, or claim that an active content
  download performs no disk I/O.
- A visible desktop, web, or Android incognito/private-session setting,
  remembered mode selection, profile switcher, simultaneous durable profiles,
  or migration between ephemeral and durable sessions.
- Persisting, exporting, importing, or adopting ephemeral state when the
  service closes.
- Automatic fallback to ephemeral mode after a durable database, migration,
  permission, disk-full, or corruption failure.
- Changing the session schema, source-provenance model, magnet
  canonicalization, BEP 9 `raw_info` placement, original `.torrent` storage,
  resume metadata, or backup policy.
- A generic persistence trait hierarchy, alternate SQL engine, mock store,
  process-global singleton, shared-cache in-memory database, or multi-process
  access.
- `.torrent` transport, upload, parsing policy, chunking, outer-field
  retention, export, or product UI.

## Reference Dossier

### SQLite

- [SQLite in-memory databases](https://sqlite.org/inmemorydb.html) specifies
  that the exact `:memory:` filename opens no disk file, each ordinary
  connection is private, and the database ceases to exist when that connection
  closes. It distinguishes this from an empty filename, which creates a
  temporary disk file.
- [SQLite PRAGMA documentation](https://sqlite.org/pragma.html) defines
  `foreign_keys`, `journal_mode`, `synchronous`, `temp_store`, `page_size`,
  `max_page_count`, and the query forms needed to verify rather than assume
  the active connection policy. It also specifies that in-memory databases
  use `MEMORY` or `OFF` journals and that `OFF` disables atomic commit and
  rollback.

### Pinned libtorrent and JSTorrent context

- `reference/libtorrent/include/libtorrent/session_params.hpp` states that
  session parameters do not contain individual torrents; the embedding client
  restores them separately.
- `reference/libtorrent/include/libtorrent/torrent_handle.hpp::save_info_dict`
  makes metadata available to client-owned resume persistence but does not
  require it.
- `reference/libtorrent/examples/client_test.cpp::{resume_file,
  add_torrent}` and its startup resume-directory scan demonstrate one optional
  file-backed client policy, not engine-owned persistence.
- The local JSTorrent sibling has memory filesystem and in-memory swarm tests,
  but the inspected product persistence path does not establish a complete
  no-profile-file product mode to adopt.

RSTorrent keeps application persistence outside the engine as it does today.
It differs from the reference example by using the same typed transactional
SQLite schema in memory rather than inventing a second resume format.

## Ownership, Tasks, And Dependency Direction

```text
headless/platform configuration
  -> ApplicationConfig persistence mode
  -> ApplicationService
       -> SessionStore (durable SQLite | private in-memory SQLite)
       -> SpeedHistory (durable SQLite | private in-memory SQLite)
       -> existing engine, DHT, views, and task owners
```

`ApplicationService` owns both connections and closes them only after existing
engine, DHT, speed, view-reaper, and storage tasks have stopped and joined.
No task receives a profile path in ephemeral mode. Store schema and domain
transitions remain runtime independent; SQLite stays in `rstorrent-session`
and does not enter protocol or engine crates.

The speed owner retains its current joined flush/shutdown behavior even though
an ephemeral flush has no crash-durability meaning. A close or shutdown race
must not retain a connection, resurrect state, or write a fallback file.

## Implementation Sequence And Gates

1. Extract common connection initialization and add private in-memory
   `SessionStore` tests for schema, transactions, receipts, isolation, page
   caps, and close semantics. Preserve all durable store tests.
2. Add bounded in-memory metrics using the existing schema and prove identical
   live queries plus disappearance on close.
3. Introduce application persistence configuration and route session, metrics,
   DHT snapshot, views, and shutdown through it. Prove no profile path is
   touched.
4. Add one explicit headless selection path and end-to-end loopback-only
   metadata lifecycle evidence, plus a separate offline no-network lifecycle.
5. Run durable restart regressions, record memory/page high water, and update
   the tactical and owning topics.

No phase may silently disable a current feature merely because it is
ephemeral. If a state owner cannot operate without a path, make that dependency
explicit and resolve it at the same boundary or stop under the escalation
contract.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Store | Same migrations/schema, foreign keys, transactions, request replay/conflict, page-cap rollback, private connection isolation, disappearance on close. |
| Metrics | Same retention/query behavior, bounded page use, no file, disappearance on close. |
| Application lifecycle | Loopback-only metadata add through verified metadata, settings/selection/views/diagnostics/DHT/speed activity, last-client detachment, joined shutdown, and fresh empty reopen; a separate offline case proves no-network open and close. |
| Filesystem effects | An absent profile path remains absent, and a separately pre-provisioned external payload root remains empty of output/staging/part artifacts throughout metadata-only open, activity, and close. Verified in-memory policy prevents database/WAL/SHM/journal/SQL-temp files. |
| Durable regression | Existing WAL/FULL checks, migration, warm DHT, metrics restore, metadata restore, have state, and restart pass unchanged. |
| Resource evidence | Record session/metrics page count, configured page maxima, process memory high water, and bounded exhaustion response. |
| Entry point | Headless configuration selects ephemeral without a profile path; conflicting durable/ephemeral options fail before service open. |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and `git diff --check`. |

No network beyond loopback, public swarm, browser, visible desktop, Android
build, emulator, physical device, schema migration, or new dependency is
required. Checked generated semantic artifacts change only for the new typed
error code.

## Escalation And Next Boundary

Implementation may choose internal constructors and factor common SQLite
initialization without direction. Stop if the mode requires a schema change,
weakens durable pragmas, spills any application state to disk, needs a generic
backend abstraction, adds a dependency, exposes new product UX, or requires a
RAM payload backend to meet the bounded stopping condition.

The next boundary remains discussion of persistent resume/source metadata,
session SQLite shape, original `.torrent` storage, `.torrent` transport and
chunking, and whether existing magnet/BEP 9 metadata retention should change.
