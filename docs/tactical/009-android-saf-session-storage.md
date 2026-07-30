# Tactical 009: Durable Android SAF Session Storage

Status: planned on 2026-07-30; implementation in progress.

## Motivation And Outcome

Tactical `005` proved bounded selective storage through Android SAF
descriptors, including provider-side publication and restart verification.
Tactical `007` established the durable SQLite application service, and
Tactical `008` made that service the owner of the Android foreground product
client. The product path still downloads into an app-private directory,
however, while the proven SAF machinery remains a separate diagnostic
orchestrator with no durable torrent state.

Connect those two established paths for one controlled torrent and one
persisted SAF tree root. A magnet added through the normal typed application
command must retain its metadata, file selection, verified-piece state, SAF
document identity, and user running intent across activity recreation,
foreground-service restart, and forced process death. Android reopens
provider descriptors as coarse platform capabilities; peer, piece, hashing,
and file payload remain inside Rust.

The stopping condition is a controlled SAF magnet download that can be
interrupted, reopened, conservatively rechecked, published, verified through
fresh descriptors, and cleaned exactly on an AVD and the unlocked Pixel 7a.
This is not general multi-torrent policy or product UI completion.

## Dependencies And References

- [`../topics/client-persistence.md`](../topics/client-persistence.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../topics/product-direction.md`](../topics/product-direction.md)
- [`../engineering-principles.md`](../engineering-principles.md)
- [`005-saf-selective-storage.md`](005-saf-selective-storage.md)
- [`007-durable-session-control.md`](007-durable-session-control.md)
- [`008-reactive-multi-surface-control.md`](008-reactive-multi-surface-control.md)
- Android `DocumentsContract`, persisted URI permission, and foreground
  service behavior already exercised by the repository harnesses
- The controlled libtorrent BEP 9 seeder and session-resume fixture under
  `tests/interop/`

No provider implementation or reference-client source is copied.

## Scope

### Stable root identity and platform capability

Generalize the configured application-service root from only a path to one of:

- a concrete path owned directly by the Rust storage layer; or
- a platform capability whose live descriptors must be supplied by the
  platform adapter.

Both use the same bounded stable root ID in portable commands and SQLite.
Path strings, SAF tree URIs, provider document URIs, and descriptor numbers
do not enter `Command`, `TorrentSnapshot`, the browser contract, or the
engine's protocol state. Android persists the selected tree URI and
per-torrent document identity in app-private platform state and validates the
persisted grant before use.

The application service must not classify a temporarily absent platform
capability as corrupt torrent state. It exposes an explicit waiting state and
preserves verified metadata, have state, selection, and running intent so the
adapter can reopen or replace the capability. Malformed durable engine state
continues to fail closed as needing repair.

### Metadata-before-storage transition

A magnet does not reveal its file layout until BEP 9 metadata is verified.
For a platform root, the engine therefore performs a bounded metadata-only
step first. The service commits the exact verified info dictionary and an
empty have state before requesting storage preparation.

Once metadata is durable, Android requests a deterministic native storage
plan, creates or rediscovers the staging directory, wanted documents, and
part document beneath the selected tree, opens independent descriptors, and
supplies one bounded manifest through an Android-only binding method. This is
a coarse lifecycle call, not a foreign callback on the file or piece hot
path.

Interrupted document preparation is recoverable by deterministic names and
exact plan validation. Existing payload documents are never silently
truncated on a resumable path. A genuinely new `storage_state = none` attempt
may initialize empty artifacts; a staging resume requires exact payload and
part-file geometry.

### Resumable descriptor storage

Add descriptor-backed resume beside the existing path-backed resume:

- duplicate every borrowed descriptor synchronously before returning across
  UniFFI;
- require exactly one descriptor for each selected non-padding file and no
  unexpected file;
- require independent current and reopen descriptors for the part file;
- validate exact file geometry and the part-file identity header;
- rehash every database-claimed piece through the existing fixed 16 KiB
  verification buffer before trusting it;
- clear false claims and retain only claims supported by current descriptor
  content;
- sync verified payload and part data before committing a have bit; and
- keep cancellation out of storage initialization, sync/checkpoint, and
  provider-publication critical transitions.

A stale or closed borrowed descriptor is rejected before task ownership
changes. Provider loss, revoked grants, missing documents, wrong document
lengths, duplicate indices, wrong part identity, and incomplete manifests
must not establish a verified piece or complete state.

### Prepared and published state

SAF publication is owned by Android and cannot be part of a filesystem rename
inside Rust. Split completion explicitly:

1. Rust syncs all selected content and part data.
2. Rust hashes every prepared wanted file with the fixed buffer.
3. SQLite atomically stores the bounded prepared-file manifest and advances
   the torrent to `awaiting_publication`.
4. Android renames provider documents according to the native plan and
   persists its publication phase.
5. Android reopens the published documents through fresh descriptors.
6. Rust checks their indices, exact lengths, and hashes against the durable
   prepared manifest.
7. Only then may SQLite mark storage published and the torrent complete.

A crash at any boundary is conservative and idempotent. Before preparation
finishes, Android reopens staging descriptors and Rust rechecks claimed
pieces. After the provider rename but before the complete transaction,
Android reopens the published tree and Rust verifies it against the durable
manifest. A crash after the complete transaction may leave only harmless
platform bookkeeping cleanup.

### Android product integration

Replace the product proof's app-private payload root with one SAF tree selected
by the normal document-tree picker. Retain the SQLite profile in app-private
storage. The foreground service owns:

- the application-service instance;
- the selected stable root ID;
- platform preparation and reopen jobs;
- typed view subscriptions;
- notification and wake-lock policy while metadata, checking, or download is
  active; and
- explicit cancellation and joined shutdown.

Activity recreation, unbinding, or backgrounding does not close descriptors
owned by an active Rust task or stop the foreground service. The adapter
closes every borrowed `ParcelFileDescriptor` after native duplication and
does not retain numeric descriptor values.

The Compose surface only needs enough root state to select or replace the
tree and make the controlled download observable. Full file-selection,
storage-management, migration, and settings presentation are deferred.

## Durable State And Migration

Advance the SQLite schema transactionally. The migration adds explicit
states for waiting on a platform capability and waiting on publication, plus
a bounded normalized prepared-file manifest keyed by torrent and file index.
An older binary continues to refuse the newer schema.

The database remains the authority for:

- torrent identity and source magnet;
- stable storage-root ID;
- desired running or paused state;
- exact raw info bytes;
- selected files;
- verified-piece state;
- current storage phase; and
- prepared publication length/hash evidence.

Android app-private state remains the authority for:

- selected SAF tree URI;
- persisted-grant acquisition;
- staging, part, and published document URIs or reconstructible identities;
  and
- provider publication phase.

Descriptor numbers are always ephemeral. Provider display names and URIs do
not prove content identity.

## Required Failure And Edge Profiles

### Capability acquisition

- no selected tree;
- user cancels the picker;
- persisted permission absent or revoked;
- read-only or provider-refused document creation;
- provider query returns a missing or duplicate expected child;
- service starts while the device is unlocked but no activity is attached;
  and
- stale descriptors supplied after their `ParcelFileDescriptor` owners close.

### Preparation and resume

- process death after metadata commit but before document creation;
- process death during partial deterministic document creation;
- process death after one or more piece checkpoints;
- same-length corruption of a claimed piece;
- missing wanted file, missing part file, wrong wanted-file length, and wrong
  part-file identity;
- manifest index duplication, omission, unexpected padding/skipped file, and
  file-count bounds;
- pause during metadata, peer input, storage write, and recheck; and
- resume after activity recreation without restarting the foreground service.

### Publication

- provider rename refusal or final-name collision;
- process death after native preparation but before rename;
- process death after rename but before SQLite completion;
- reopened published descriptor with wrong length or bytes;
- repeated publication/confirmation calls; and
- no complete state before fresh-descriptor verification succeeds.

Tests preserve or clean artifacts according to the phase being asserted and
must never delete an unrelated document tree.

## Validation

Run, in proportion to the implementation:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Add focused Rust tests for root kinds, metadata-only transition, descriptor
creation/resume, claim recheck, migration, publication manifest bounds,
fail-closed confirmation, and restart phase restoration. Add Android unit
tests for deterministic SAF state reduction and exact descriptor ownership.

Run the controlled libtorrent magnet seeder against:

1. the `jstorrent-tablet` AVD; and
2. the attached, unlocked Pixel 7a.

For each target, record:

- persisted tree grant and stable root ID;
- verified metadata size and identity;
- at least one piece checkpoint before interruption;
- process termination and foreground-service restoration;
- claimed pieces before and after recheck;
- resumed network bytes;
- provider publication phase;
- fresh-descriptor final SHA-1;
- activity recreation/background survival;
- foreground notification presence while active;
- joined shutdown; and
- exact deletion of only the controlled profile and SAF test folder.

The physical-device harness must inspect lock state first, refuse a locked
device, never issue a power, sleep, wake, keyguard, or lock command, and
exclude unrelated attached devices from mutation.

## Non-Goals

- simultaneous or queued multi-torrent downloads;
- multiple active SAF roots, root migration, or completed-content moves;
- general settings UI or polished JSTorrent Android parity;
- seeding on Android;
- trackers, DHT, PEX, or magnets without a usable explicit peer hint;
- optimistic hash-skipping fast resume;
- block-level unfinished-piece persistence;
- provider-specific sparse-allocation optimization;
- remote pairing, authorization, relay, or wake-up;
- desktop lifecycle or storage changes;
- an HTTP playback server;
- arbitrary URI, path, SQL, or descriptor operations in the portable RPC;
  and
- removable-media and ancient-filesystem compatibility claims beyond the
  provider behavior actually tested.

## Stopping Condition

Stop when one controlled magnet added through the normal Android product
command uses a persisted SAF root, survives activity recreation and forced
process death with conservative descriptor-backed recheck, publishes through
the provider, verifies through fresh Rust-owned duplicate descriptors, reaches
durable complete state, and cleans only its controlled artifacts on both the
AVD and unlocked Pixel.

Record exact automated and physical-device evidence, any provider-specific
observations, implementation commits, and deliberate deferrals here before
marking the tactical complete.

## Execution Record

Implementation and validation are in progress.
