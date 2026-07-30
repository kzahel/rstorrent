# Client Persistence And Session Boundary

Topic: `client-persistence`

Status: the SQLite persistence direction is accepted. A logical
application/engine boundary and instance-scoped profile isolation are the
current recommendations; the exact crate name and first physical extraction
remain decisions for Tactical `007`, where persistence provides the first
concrete caller.

## Scope

This topic owns durable client state, resume and restart correctness, database
portability, storage-root identity, and the boundary between the reusable
torrent engine and the first-party application service shared by platform
clients.

It does not define the final product API, select a desktop or Android UI
architecture, prescribe every future schema column, or promise that the engine
will become a separately supported general-purpose library.

Payload and part-file layout remain storage concerns. This topic owns the
durable records that identify those artifacts and determine whether previously
verified content may be restored or must be checked again.

## Terms And Ownership

RSTorrent uses these terms to keep "client" from collapsing several different
responsibilities:

- The **torrent engine** owns peer protocols, metadata and piece verification,
  scheduling, torrent-coordinate storage behavior, and the state transitions
  that determine when content is verified.
- The **application service** owns the durable torrent catalog, settings,
  storage-root registry, queue and user intent, multi-torrent lifecycle,
  restart orchestration, and the application-facing command, snapshot, and
  event model.
- A **platform adapter** owns operating-system capabilities such as the
  application-data directory, Android persisted SAF grants and descriptors,
  foreground-service lifetime, notifications, and desktop integration.
- A **product client** owns presentation and user interaction. Android and
  desktop clients consume the same application service in-process.

The application service is native Rust product code. Placing SQLite there does
not move persistence into Kotlin, a desktop UI runtime, or an external daemon.

## Reference Direction

[Rasterbar libtorrent](https://libtorrent.org/manual-ref.html#fast-resume)
produces engine resume snapshots and session state but leaves their durable
storage, filenames, catalog, and migration policy to its caller. That
separation is useful, but RSTorrent does not need to reproduce libtorrent's
public C++ library surface or place orchestration in Python.

JSTorrent uses SQLite as a cross-platform key/value store containing JSON and
encoded binary state. That proves the practical desktop and Android direction,
but RSTorrent should use typed tables, transactions, constraints, and native
BLOBs rather than retain a JavaScript-shaped persistence format. JSTorrent
Desktop also isolates stable profile identities into separate directories and
databases; its additional discovery, liveness, takeover, daemon, and port
coordination shows that storage isolation is much cheaper than simultaneous
profile execution.

Current qBittorrent source provides the closest database precedent: typed
client columns coexist with opaque libtorrent metadata and resume BLOBs.
Transmission, Deluge, rTorrent, and rqbit demonstrate viable file and
whole-session snapshot designs, but also show file proliferation,
cross-file-consistency windows, or whole-catalog rewrite and recovery costs.

These references inform the design; their persistence formats are not
compatibility requirements. See [`../references.md`](../references.md) for the
reference-use policy.

## Accepted Persistence Direction

### One local SQLite authority per profile

Use one versioned SQLite database per application-service instance as the
authoritative client and session store on desktop and Android. A normal
single-profile product opens one instance and one database. The database lives
in application-private local storage. SAF locations, removable filesystems,
network shares, and cloud-synchronized payload roots are represented by
records in the database; they do not contain the live database.

Bundle one sufficiently current SQLite version with the Rust application
service so supported platforms share behavior and fixes. At the time this
topic was written, WAL-enabled builds must contain SQLite's
[2026 WAL-reset fix](https://sqlite.org/wal.html#the_wal_reset_bug). Do not
allow Android platform SQLite and bundled Rust SQLite to open the same
database independently. The initial connection policy should prefer one
identifiable Rust writer owner, bounded transactions, a busy timeout,
foreign-key enforcement, WAL with its activation verified, and
`synchronous=FULL`.

[WAL files](https://sqlite.org/wal.html#the_wal_file) are part of a live
database's persistent state. Backup, export, and migration must use
[SQLite's backup facilities](https://sqlite.org/backup.html) or a controlled
checkpoint and close; copying only the main file while the application runs is
not a valid backup.

### Verified metadata is durable

[BEP 9](https://www.bittorrent.org/beps/bep_0009.html) supplies only the exact
bencoded info dictionary, not the complete outer `.torrent` dictionary. After
its hash matches the requested v1 identity and it passes the normal bounded
metainfo parser, store those exact raw bytes as a BLOB in the same transaction
that advances the torrent from awaiting metadata to ready.

Store explicit source intent separately. This includes the torrent identity
and bounded user-supplied magnet fields needed to reconstruct intent, such as
tracker URLs, display name, and explicit peer hints. A user-supplied peer hint
is distinct from a reconstructible dynamic peer cache.

The raw info bytes are authoritative. Parsed columns may support queries and
presentation, but restart re-hashes and re-parses the raw bytes under current
limits before metadata-derived state becomes trusted. A later export command
may synthesize a `.torrent` outer dictionary around those authentic info bytes;
that export is not the original metainfo file unless the original file was
independently retained as source input.

Separate per-torrent metadata files are not a second authority. They may
eventually exist as explicit exports or reconstructible caches.

### Verified-piece state is essential resume state

Persist the verified-piece bitfield as a bounded, explicitly versioned BLOB.
Its encoding defines the associated info hash, piece count, bit order, length,
and zero-padding rules. SQLite does not interpret BLOB representation, so raw
Rust struct layouts are never a persistence format.

Percent complete is derived from verified pieces, exact piece lengths, and
current selection. It is not authoritative durable state. A cached display
value may exist later, but the engine may not resume from it.

The initial durable-resume slice does not need unfinished-block maps, peer
caches, transfer history, or every libtorrent fast-resume field. Those may be
added when their owner, update frequency, bounds, and restart semantics are
defined.

### Resume is evidence, not verification

A persisted have-bit does not prove that current payload bytes are correct.
The initial restart path is deliberately conservative:

1. validate database and encoding versions and torrent identity;
2. hash and parse the stored raw info bytes;
3. validate the bitfield shape and storage-root resolution;
4. validate payload and part-file geometry;
5. hash every piece claimed by the persisted bitfield through the existing
   fixed-buffer storage mapping; and
6. restore only pieces that pass, clearing failed or unavailable pieces so
   they can be downloaded again.

This is durable resume with bounded recheck, not optimistic fast resume. A
future fast-resume policy may skip payload hashes when clean shutdown,
write-ordering, storage identity, file observations, and part-file generation
provide sufficient continuity evidence. Any disagreement falls back to
hashing.

The critical crash-ordering invariant is one-sided: durable storage and
verification occur before the database may commit a have-bit. A crash between
verification and that database commit can create a false negative that costs
another check or download. It must not create a false positive that presents
unverified content as complete.

## Logical Architecture

The intended dependency direction is:

```text
Android UI / desktop UI / CLI
              |
      platform adapter
              |
    application service
      SQLite + lifecycle
              |
       torrent engine
              |
     protocol and domain
```

The current crates do not yet express every box. `rstorrent-protocol` owns pure
protocol and deterministic state, `rstorrent-engine` owns Tokio networking and
storage execution, and `rstorrent-android` currently adapts the diagnostic
engine directly.

Persistence supplies a concrete reason to introduce the application-service
boundary:

- SQLite and migrations can be tested without peer networking.
- Peer and storage behavior can be tested without a database.
- Product settings and multi-torrent queue policy do not enter peer-engine
  modules.
- Android and desktop consume one native restart policy.
- The engine does not acquire platform paths, SAF URIs, SQL rows, or UI
  lifecycle types.

The likely physical shape is a new crate with a working role such as
`rstorrent-session` or `rstorrent-application`, depending inward on
`rstorrent-engine`. It owns the concrete SQLite implementation and supervises
engine work. Platform adapters depend on it.

The name is less important than the direction. Do not create a generic
`Client` facade, persistence-backend trait hierarchy, plugin system, remote
service contract, or public compatibility promise merely to resemble
libtorrent. Start with one concrete SQLite store and plain application-service
types. A trait or further crate split must solve a demonstrated second backend,
test seam, ownership problem, or reuse requirement.

The engine should expose bounded checkpoint values and coarse verified-piece
transitions without depending on SQL. The application service may batch those
transitions into database transactions. Piece blocks and payload buffers do
not cross the application or platform boundary.

## Profile-Ready Isolation

Multiple profiles are a potentially useful application feature, but they
should not make every torrent query and invariant multi-tenant from the start.
Preserve the option through instance and directory isolation:

```text
application data/
  profiles/
    <stable-profile-id>/
      session.db
```

Each application-service instance receives an explicit stable profile identity
and profile root at construction, owns exactly one database beneath that root,
and owns all engine tasks restored from it. Profile display names are mutable
metadata and never directory identities. Torrent identifiers need only be
unique inside a profile; a future cross-profile surface identifies a torrent
with both profile and torrent identity.

Do not add `profile_id` to every session table. A database is already the
isolation boundary, which makes backup, deletion, migration, corruption
recovery, and tests independently scoped. It also prevents one missing SQL
predicate from exposing or mutating another profile.

The first product may expose only one automatically created profile. Preserve
future switching with these low-cost constraints:

- no process-global application service, database connection, torrent
  registry, or mutable settings singleton; a runtime may be shared only
  through an explicit higher-level owner rather than a hidden global;
- all background tasks, events, caches, and temporary state have an
  application-service instance owner;
- profile-relative state remains beneath the profile root, while payload roots
  remain explicit external capabilities;
- truly installation-wide bootstrap state, such as the profile registry and
  last-selected profile, is kept outside any one profile database; and
- switching an exclusive active profile quiesces and joins its engine work,
  commits or conservatively abandons pending checkpoints, closes its database
  and platform capabilities, and only then opens the next instance.

Changing which profile the UI displays while previous profiles continue
downloading is simultaneous multi-profile operation, not merely quick
switching. It can use multiple application-service instances without changing
the database schema, but it creates real additional policy:

- one process-wide resource budget must bound the sum of profile activity;
- incoming listen ports, DHT identity, NAT mappings, bandwidth policy, and
  other process or network resources need explicit sharing or isolation;
- overlapping payload roots and the same torrent in multiple profiles can
  create cross-profile storage races;
- Android foreground-service, notification, and background-lifecycle
  ownership must represent every active instance; and
- deletion, takeover, and crash recovery need an authority above the
  individual profile.

Do not claim or implement simultaneous profiles in Tactical `007`. Designing
the service as an instance rather than a singleton keeps that later choice
open without paying these policy costs now.

## Schema Direction

Tactical `007` should define the first exact schema and migrations. The
continuing direction is:

- schema and migration history;
- global settings, using typed columns or narrowly scoped typed key/value
  records where flexibility is real;
- stable storage-root identities with platform-specific opaque locators and
  capability status;
- torrent identity, lifecycle, source intent, timestamps, queue order, and
  selected root;
- exact verified metainfo/info bytes;
- bounded versioned verified-piece state; and
- sparse file-selection overrides rather than an unconditional row for every
  file.

Payload files, part payload, logs, large reconstructible caches, and temporary
network data stay outside SQLite. Tracker credentials and private magnet
material are sensitive application data and must not appear in logs or
unredacted diagnostics.

Add, metadata-accepted, selection/root change, pause intent, and logical
removal transitions should be transactional inside the database. Filesystem
publication and deletion cannot share a transaction with SQLite; represent
their intermediate state explicitly and make restart cleanup idempotent.

## Cross-Platform Invariants

- The database file format is portable; absolute paths and capabilities are
  not. Use storage-root identities instead of treating a path string as a
  portable key.
- The live database remains on a local filesystem with SQLite-compatible lock
  and shared-memory behavior.
- Android SAF URIs and persisted grants are platform locators, never SQLite
  file locations and never open descriptor numbers.
- File sizes, timestamps, sparse allocation, case sensitivity, and identity
  tokens are platform-specific restart evidence, not universal content proof.
- Newer applications migrate older schemas transactionally. An older
  application refuses a newer unsupported schema rather than guessing or
  attempting an automatic downgrade.
- Backup and restore remap unresolved storage roots explicitly and never
  silently reinterpret a locator from another operating system.
- Corrupt or unsupported durable state cannot establish verified metadata,
  have-pieces, storage publication, or seeding eligibility.

## Known Gaps And Open Decisions

- The exact application-service crate name and first public Rust types.
- The first schema, migration mechanism, database filename, and backup policy.
- The installation-level profile registry format and whether the first product
  exposes more than its automatically created profile.
- Whether Android places the database in backed-up or explicitly no-backup
  app-private storage.
- The batching interval and checkpoint policy for verified-piece updates.
- The exact clean-shutdown, storage-generation, and file-observation evidence
  required before a later fast-resume path may skip hashing.
- How completed payload moved outside the application is deliberately
  relocated or rediscovered.
- How a future JSTorrent migration imports existing settings, metadata, and
  progress without treating unverified legacy state as verified content.

## Recommended Next Work

Draft Tactical `007` for the smallest complete persistence and restart slice:
one concrete SQLite implementation, an explicit application/engine seam,
versioned schema migration, durable verified magnet metadata and source
intent, storage-root and selection persistence, verified-piece batching,
forced-process-death recovery, and fixed-buffer recheck.

The tactical should decide the physical crate boundary from those real
dependencies before implementation. It should not broaden into a general
multi-torrent scheduler, stable product API, UI settings catalog, remote
control surface, profile-management UI, simultaneous profiles,
unfinished-block resume, or hash-skipping fast-resume policy.
