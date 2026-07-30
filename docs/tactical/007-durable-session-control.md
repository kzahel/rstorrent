# Tactical 007: Durable Session Control And Resume

Status: completed on 2026-07-30.

## Motivation And Outcome

Magnet metadata, user intent, file selection, and verified-piece progress
currently disappear with the process. CLI, Android, and tests also orchestrate
the engine through different ad hoc entry points. Adding persistence directly
to those surfaces would create a third owner without establishing the
application boundary needed by desktop, Android, and eventual remote control.

Introduce one concrete `rstorrent-session` application-service crate. It owns
one profile-local SQLite database, a transport-neutral command dispatcher, and
engine task lifecycle. Starting from an explicit-peer v1 magnet, the service
stores source intent and exact verified info bytes, durably checkpoints
selected-root and verified-piece state after payload sync, survives forced
process death, conservatively rechecks claimed pieces, and resumes to exact
publication.

This is the first durable restart thread and the first shared application
contract. It is not optimistic fast resume, a complete torrent client, or a
remote server.

## Dependencies And References

- [Client persistence direction](../topics/client-persistence.md)
- [Application control direction](../topics/application-control.md)
- [Magnet metadata execution record](006-magnet-metadata-peer-hint.md)
- [Selective storage execution record](002-selective-multi-file-storage.md)
- [Engineering principles](../engineering-principles.md)
- [BEP 9](https://www.bittorrent.org/beps/bep_0009.html)
- [SQLite transaction, WAL, and backup documentation](https://sqlite.org/docs.html)
- Rasterbar libtorrent fast-resume behavior and resume-data ownership
- JSTorrent, qBittorrent, Transmission, Deluge, rTorrent, and rqbit
  persistence behavior recorded in the client-persistence topic

No reference database schema, source, or fixture is copied. The implementation
uses public behavior and independently authored tests.

## Scope

### Application service and control contract

Add one instance-owned application service above `rstorrent-engine`. Its first
semantic API contains:

- `RequestEnvelope { version, request_id, expected_revision, command }`;
- commands to add an explicit-peer magnet, query a snapshot, request pause or
  resume, and shut down the instance;
- correlated typed responses containing the resulting service revision;
- structured stable error codes plus bounded diagnostic messages;
- profile, torrent, lifecycle, progress, selection, and storage-root snapshot
  values; and
- persisted request receipts for mutation deduplication across caller retries
  and process restart.

The in-process Rust API is authoritative. A newline-delimited JSON diagnostic
adapts standard input/output to the same dispatcher so process-death evidence
does not invent another orchestration surface. That encoding is versioned but
is not a stable remote protocol.

One service instance owns one profile and at most one running torrent task in
this tactical. Its catalog may contain more durable records, but queue policy
and simultaneous downloads are deferred.

### Profile and SQLite ownership

The service receives an explicit profile root and a map of configured
path-backed storage roots. It creates `session.db` beneath the profile root.
Payload roots remain outside the profile and are referenced by stable bounded
root identifiers.

Use one bundled SQLite implementation and one identifiable writer owner with:

- foreign keys enabled and verified;
- WAL mode enabled and verified;
- `synchronous=FULL`;
- a bounded busy timeout;
- explicit transactions for all state transitions;
- transactional numbered schema migration using SQLite's user version;
- refusal of a newer unsupported schema; and
- no database access from engine worker tasks except through service-owned
  checkpoint operations.

The initial schema stores torrent identity, canonical bounded source magnet,
selected root and sparse file-selection overrides, lifecycle and storage
state, exact raw info bytes, versioned have state, last bounded error,
revision, timestamps, and request receipts. Payload blocks, logs, peer caches,
and transfer history remain outside SQLite.

### Durable metadata and selection

Adding a magnet commits source intent before networking starts. After BEP 9
assembly, the engine exposes the exact raw info dictionary only after its
SHA-1 matches the magnet identity and bounded metainfo parsing succeeds. The
service commits those bytes and metadata-derived selection geometry in the
same transition.

On restart, raw info bytes are rehashed against the torrent identity and
reparsed under current protocol limits before file names, lengths, piece
hashes, or persisted have state are trusted. Missing, corrupt, mismatched, or
newer unsupported metadata moves the torrent to an observable repair/error
state without touching payload storage or connecting to a peer.

The first source form is the existing bounded v1 magnet with at least one
explicit `x.pe` hint. Tracker, DHT, PEX, and peer-cache persistence remain out
of scope.

### Versioned verified-piece state

Store have state as an explicit encoding with a format version, info hash,
piece count, MSB-first bit order, exact byte length, and zero padding.
Malformed length, count, identity, version, or padding cannot establish any
verified piece.

Before setting a have bit, the storage owner:

1. writes every block through existing bounded storage placement;
2. hashes the complete piece through the existing 16 KiB buffer;
3. synchronizes the files and part payload touched by that piece; and
4. asks the service to transactionally commit the bit and new revision.

A crash may lose a just-verified bit and cause redundant work. It cannot leave
a durable have bit ahead of durable payload.

This tactical checkpoints each completed piece rather than introducing a
timer or batching policy. The simpler write frequency makes forced-death
ordering directly testable; later scheduling evidence may justify batching.

### Storage reopen and bounded recheck

Extend path-backed selective storage with an explicit reopen mode:

- incomplete storage reopens the staging tree and identity-checked part file;
- completed storage reopens the published tree and identity-checked part
  file;
- every expected wanted file has its exact bounded length checked;
- unexpected absence, truncation, extension, invalid part header, or
  incompatible layout produces a typed repair result without destructive
  cleanup; and
- new-download create mode retains its existing refusal to overwrite
  pre-existing artifacts.

At service startup or explicit resume, hash every persisted claimed piece
through fixed 16 KiB buffers. Matching pieces remain set. Failed hashes are
cleared transactionally and become ordinary missing work. Malformed durable
state clears all claims. A structurally unopenable storage set enters
`needs_repair` instead of guessing or deleting user data.

The content runner skips revalidated pieces, downloads only missing wanted
pieces, checkpoints each one, and idempotently publishes when selection is
complete. Restart from a fully published selection is successful without a
peer connection after recheck.

### Interruption and lifecycle

The resumable session path retains owned staging and part-file artifacts on
pause, process death, peer failure after progress, or controlled shutdown.
It must not weaken the current diagnostic APIs, whose failure cleanup remains
part of their contract.

Pause expresses desired durable state, cancels and joins the task, and records
an observable paused terminal state. Resume revalidates durable metadata and
storage before starting network work. Shutdown cancels and joins all owned
tasks, closes the database, and emits no success for work still running.

Process death is tested with an external kill; it cannot run cleanup and is
not modeled as graceful pause.

## Contracts And Invariants

- Dependency direction is platform/diagnostic to session to engine to
  protocol; SQL and serialized envelopes never enter protocol code.
- One profile database, connection owner, dispatcher, and task registry belong
  to one non-global service instance.
- All peer-controlled and durable lengths, counts, strings, blobs, selections,
  revisions, and request receipts are bounded before allocation or mutation.
- Exact verified metadata bytes, not reconstructed bencode, authorize restart.
- A have bit is only evidence to recheck in this tactical, never proof by
  itself.
- Recheck and verification memory are independent of piece length.
- Durable intent precedes side effects; durable verified progress follows
  synchronized payload.
- Filesystem and SQLite transitions have explicit intermediate states and
  idempotent restart behavior because they cannot share one transaction.
- Stale expected revisions and duplicate request identities cannot partially
  apply commands.
- Product state, command responses, and structured engine observations remain
  distinct channels.

## Nasty Cases Required Up Front

Tests or controlled interoperability evidence cover at least:

- a missing database, an existing current database, each supported migration,
  a transaction-interrupted migration, and a newer unsupported user version;
- failure to enable required pragmas, lock contention bounded by timeout, and
  database corruption surfaced without recreating or overwriting it;
- duplicate add with the same request and payload, request-ID reuse with a
  different payload, stale expected revision, unknown command/version, and
  retry after process restart;
- oversized root IDs, torrent source, error text, receipt payload, selection,
  metadata BLOB, have BLOB, and piece count rejected before large allocation;
- raw metadata absence, bit flip, wrong identity, valid hash with invalid
  metainfo, and no storage/network side effect after rejection;
- have encoding wrong version, wrong identity/count/length, nonzero padding,
  and all boundary bit counts;
- crash before payload sync, after payload sync but before the DB checkpoint,
  and after the checkpoint;
- restart with no verified pieces, some verified pieces, all verified pieces,
  a same-length corrupt piece, missing/truncated/extended wanted file, missing
  or corrupt part file, and already-published output;
- pause before metadata, during content, after progress, and racing completion;
- peer failure followed by a successful retry without discarding verified
  progress; and
- bounded request receipts, snapshots, errors, task joins, and retained
  artifacts under repeated commands.

## Non-Goals

- stable public or remote wire compatibility
- TCP, HTTP, WebSocket, native-messaging, relay, discovery, authentication, or
  authorization
- UI implementation, event subscriptions, notifications, or background wakeup
- tracker, DHT, PEX, peer-cache persistence, or general peer replacement
- multiple active downloads, simultaneous profiles, profile switching UI, or
  installation-wide profile registry
- SAF capability restoration, descriptor-backed resume, removable-storage
  relocation, or Android database placement policy
- BitTorrent v2/hybrid support, arbitrary `.torrent` imports, multi-piece
  single-file expansion, unfinished-block resume, or upload/seeding resume
- optimistic hash-skipping fast resume, file-observation heuristics, backup,
  export, import, deletion, relocation, or legacy JSTorrent migration
- settings catalog, transfer history, ratios, priorities, queueing, bandwidth
  policy, or a generic persistence-backend abstraction

## Implementation Direction

Create `rstorrent-session` as a concrete inward-dependent crate rather than
adding SQL to the engine or inventing a generic client facade. Keep plain
semantic control types separate from SQLite rows and from task supervision
where doing so exposes deterministic tests.

Add a narrow engine checkpoint callback or channel carrying verified metadata,
storage-ready, piece-durable, and published transitions. It must not expose SQL
or payload buffers. Add a resumable engine entry point and explicit storage
open mode while preserving existing diagnostic entry points and cleanup.

Use the v1 info hash as the profile-local torrent identity in this slice. Use
canonical lowercase hexadecimal in control and database keys and retain raw
20-byte identity where integrity encoding needs it.

The JSON diagnostic should remain a thin serializer/dispatcher loop. Its
process lifetime owns the service; EOF or shutdown joins it. Human-readable
CLI output remains diagnostic and need not be retrofitted in this tactical.

## Implementation Sequence

1. Record the application-control topic and this tactical.
2. Add exact control values, bounded validation, snapshots, errors, and
   deterministic request/revision/idempotency tests.
3. Add the bundled SQLite store, schema migration, integrity codecs, and
   hostile durable-state tests.
4. Add path-storage reopen/sync/recheck and resumable engine checkpoints while
   preserving diagnostic cleanup behavior.
5. Supervise the resumable magnet task from the service and expose the JSON
   diagnostic adapter.
6. Add forced-process-death libtorrent evidence, run the full baseline, remove
   temporary artifacts, and record exact results and limits.

## Validation

The execution record must list only commands actually run. Expected baseline:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv run --project tests/interop \
  python tests/interop/session_resume.py --runs 3
uv run --project tests/interop \
  python tests/interop/magnet_metadata.py --runs 3
python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

Build or cross-compile the session crate for the supported Android Rust targets
because bundled SQLite is now shared product code. Physical Android database
and SAF resume evidence is explicitly deferred.

## Stopping Condition

This tactical is complete when one controlled multi-piece libtorrent seed can
be added by magnet through the semantic request envelope, killed after at
least one durable piece checkpoint, reopened from the same profile, rechecked
with bounded memory, resumed without reacquiring already stored metadata or
redownloading valid claimed pieces, and published with exact expected bytes in
three fresh runs.

Unit and integration evidence must also prove schema compatibility behavior,
metadata and have-state fail-closed handling, same-length payload corruption
fallback, structural-storage repair state, request deduplication and stale
revision rejection, graceful pause/join, and unchanged cleanup for the older
diagnostic APIs. Any unavailable platform row or deliberately deferred nasty
case is recorded rather than implied.

## Implementation Outcome

The tactical landed in six implementation checkpoints after the topic and
tactical commit:

- `rstorrent-session` now depends inward on the engine and protocol crates. It
  owns one `session.db`, configured storage roots, the semantic dispatcher,
  one supervised task, and pause/shutdown joins without a process-global
  singleton or persistence-backend trait.
- Control version `1` has typed add-magnet, snapshot, pause, resume, and
  shutdown commands; caller request IDs; optional expected revisions; typed
  snapshots and errors; and bounded persistent mutation receipts. A request-ID
  replay returns the original response after restart, reuse with different
  content conflicts, and a stale expected revision changes nothing.
- Schema version `1` uses typed profile, root, torrent, sparse selection, and
  receipt tables. Exact verified info bytes and a self-identifying,
  versioned, MSB-first have encoding are BLOBs. Database open verifies WAL,
  foreign keys, `synchronous=FULL`, and a two-second busy timeout; a newer
  schema and corrupt database are refused without recreation.
- The bundled dependency is `rusqlite 0.40.1` with
  `libsqlite3-sys 0.38.1`, whose bundled source reports SQLite `3.53.2`.
- The engine's new resumable entry point retains raw BEP 9 bytes, opens new,
  staged, or already-published path storage, validates exact regular-file
  geometry and part identity, rechecks claimed pieces through a 16 KiB
  buffer, skips valid pieces, and defers peer resolution until missing work is
  known.
- Each newly verified piece synchronizes every touched wanted file and the
  part payload when applicable before its checkpoint callback can commit a
  have bit. Publication is a separate idempotent checkpoint. The older
  one-shot APIs still remove their owned artifacts after failure or
  cancellation.
- The application service translates coarse metadata, storage, recheck,
  piece, and publication callbacks into SQLite transitions. Malformed have
  state clears to an empty encoding after raw metadata revalidation. Corrupt
  metadata or structurally inconsistent storage enters `needs_repair` without
  storage access, overwrite, or deletion.
- The JSON-lines process diagnostic is only a serializer around the same Rust
  dispatcher. It supplied the external process boundary needed for real
  `SIGKILL` evidence without adding a product listener or daemon.

## Execution Evidence

The final repository baseline passed:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

The workspace run covered 100 unit, binary, architecture, and integration
tests plus all doc-test targets. New evidence includes schema creation/reopen
and newer-version refusal, corrupt-database preservation, have encoding
boundaries and padding, durable request replay/conflict/stale revision,
metadata corruption before storage, malformed-have conservative clearing,
pause/join across restart, path-storage staged/published reopen and exact
length checks, and application-level incomplete-artifact repair without
overwrite.

Three fresh forced-death runs passed:

```bash
uv run --project tests/interop \
  python tests/interop/session_resume.py --runs 3
```

Each run used libtorrent `2.0.13`, exact 26,686-byte two-block metadata, and a
three-piece 40,000-byte payload. The diagnostic was killed after two durable
piece bits. One same-length staged piece was then corrupted. Restart retained
the exact metadata, recheck changed two claimed pieces to one, and libtorrent
uploaded 23,616 payload bytes rather than the complete 40,000 bytes. The
published payload SHA-1 was
`576143b2992ecf25c780ff41c79552f3bb50941b`. A further profile restart with
the libtorrent seed removed advanced through the published-storage and
complete checkpoints, proving the fully complete path did not resolve or
connect to the peer hint. All three run directories were removed.

The existing magnet and storage baselines also passed:

```bash
uv run --project tests/interop \
  python tests/interop/magnet_metadata.py --runs 3
uv run --project tests/interop \
  python tests/interop/first_verified_piece.py --runs 1
uv run --project tests/interop \
  python tests/interop/first_verified_piece.py --large-piece --runs 1
uv run --project tests/interop \
  python tests/interop/first_verified_piece.py --selective-files --runs 1
```

The magnet baseline retained its 26,686-byte, two-block, bidirectional BEP 9
evidence in all three runs. The small 40,000-byte download, 32 MiB one-piece
download at a 256 KiB payload high-water, and five-piece selective fixture at
a 32 KiB high-water all passed with exact output and cleanup.

Bundled SQLite cross-compiled for both established Android Rust targets at the
existing API 28 floor:

```bash
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"
cargo ndk -t x86_64 -t arm64-v8a -P 28 \
  build --release -p rstorrent-session --lib
```

Reference and dependency checks passed:

```bash
python3 scripts/references.py status
cargo tree --workspace --locked
rg -n '^#define SQLITE_VERSION ' \
  "$HOME/.cargo/registry/src"/*/libsqlite3-sys-0.38.1/sqlite3/sqlite3.c
```

The BEP, rqbit, libtorrent, and JSTorrent pins were present, the dependency
tree showed the intended session-to-engine/protocol direction, and the
bundled source reported SQLite `3.53.2`.

## Recorded Limits And Deferrals

- This is path-backed multi-file v1 resume with explicit `x.pe`. SAF
  capability restoration, descriptor resume, physical Android database
  execution, removable storage, single-file multi-piece torrents, trackers,
  DHT, and peer replacement remain unclaimed.
- SQLite calls are serialized through the one service-owned connection and
  coarse checkpoint seam. Per-piece `synchronous=FULL` transactions favor
  evidence over throughput; batching remains a measured later decision.
- The service supports one active task. It has no general queue, event
  subscription, removal, settings catalog, backup/export, profile registry,
  or simultaneous-profile policy.
- The tests cover transaction migration from version `0` to `1`, a newer
  version, and corrupt-database preservation. They do not inject an operating
  system failure at every SQL statement, time lock contention, or prove power
  loss and parent-directory fsync behavior.
- Actual `SIGKILL` covers process death after committed piece progress.
  Deterministic ordering and unit tests cover the storage-sync-before-have
  seam, but crashes immediately before and after every individual filesystem
  and SQLite barrier were not separately injected.
- The JSON-lines encoding is repository diagnostic infrastructure, not a
  stable public or authenticated remote protocol.

These limits preserve the tactical's stopping boundary. The implemented
evidence satisfies the stopping condition without claiming optimistic fast
resume or platform persistence that was not run.
