# Tactical 143: Dual Identity And Persistence Foundation

Status: **Planned and inactive.** Maintainer direction on 2026-08-12 approves
this decision-complete plan, but does not activate implementation. Tactical
[`142`](142-wan-transport-performance-matrix.md) remains the sole authoritative
**Now**.

Topics: `bittorrent-v2-and-hybrid`, `client-persistence`,
`application-control`, `application-view-api`, `client-surfaces`,
`android-saf-storage`, `incoming-reachability-and-seeding`,
`download-correctness`, `protocol-support`, `capability-readiness`,
`oracle-driven-engine-campaign`

Dependencies: completed Tacticals
[`073`](073-unified-storage-and-complete-recheck.md),
[`075`](075-ephemeral-application-state.md),
[`081`](081-v1-torrent-byte-intake.md),
[`097`](097-live-client-settings-and-replaceable-session-generations.md),
[`116`](116-platform-storage-coherence-and-ios-feasibility.md),
[`120`](120-per-torrent-trusting-fast-resume.md),
[`124`](124-duplex-verified-piece-upload.md),
[`133`](133-utp-product-default-enablement.md), and
[`140`](140-incoming-utp-reachability.md).

## Decision And Desired Outcome

Replace the current assumption that one 20-byte SHA-1 value is simultaneously
the torrent owner, application ID, persistent primary key, storage namespace,
and peer-swarm key. Establish instead:

- one opaque stable `TorrentId` for the content/runtime owner;
- a nonempty set containing an optional full v1 identity, an optional full v2
  identity, or both;
- a protocol-version-tagged 20-byte `SwarmKey` used only where BitTorrent wire
  protocols require 20 bytes; and
- one explicit mapping from each full protocol identity to at most one torrent
  owner.

The implementation remains v1-only at every accepted product input and wire
path. It must preserve existing v1 behavior while making identity meaning
explicit enough that a later v2 parser, pure-v2 runtime, and hybrid alias
expansion do not have to re-key the application or storage owner.

Use a fresh persistence epoch. RSTorrent is unreleased, so recognized schema
versions `1..=18` are discarded rather than migrated. The reset clears the
application-private session catalog, not user-selected roots or payload. Old
path- or provider-backed partial artifacts are not adopted or automatically
deleted; new opaque storage names make them non-conflicting and a later
explicit cleanup action may remove them with the existing ownership checks.

## Scope And Stopping Condition

This tactical owns one cross-cutting v1-preserving foundation:

1. add the identity and owner types and remove untagged 20-byte torrent
   identity from shared protocol, engine, session, and storage boundaries;
2. replace the SQLite torrent primary key and dependent foreign keys with the
   opaque owner key plus a constrained protocol-identity alias table;
3. replace the v1-keyed durable have, part-file, retained-source, storage-name,
   and generated application contracts together;
4. introduce fail-closed full-identity and versioned-wire-key registries,
   including deterministic alias conflict and ambiguous truncated-v2 lookup
   behavior;
5. reset recognized pre-foundation durable profiles before any application
   background owner starts, while leaving every external storage root
   untouched; and
6. re-prove ordinary v1 intake, restart, transfer, discovery, encryption,
   incoming routing, checking, publication, seeding, removal, and first-party
   client composition.

The tactical stops only when all of the following are true:

- every production torrent has one stable `TorrentId` and exactly one v1
  alias, while pure type/store tests cover v2-only and hybrid identity sets;
- no database key, application route, storage instance, have-state header, or
  part-file header uses a protocol info hash as the torrent owner;
- every 20-byte swarm use at a tracker, DHT, peer handshake, MSE, incoming, or
  metadata boundary is explicitly typed or immediately converted from a
  typed `SwarmKey`;
- recognized schema-18 and representative older profiles restart into the new
  empty catalog with one observable reset report, and subsequent restarts do
  not reset again;
- reset evidence proves `session.db` state was discarded while published and
  partial bytes under path and Android SAF roots were neither removed nor
  adopted;
- fresh durable and ephemeral profiles, v1 magnet and `.torrent` add,
  duplicate add, metadata acquisition, exact source retention, receipt replay,
  archive, queue, settings, checking, fast resume, seeding, and both removal
  policies pass with opaque IDs;
- generated JSON Schema, TypeScript, Tauri, UniFFI, Kotlin, React, and Android
  consumers treat `torrent_id` as opaque and display/export the explicit v1
  info hash where they previously reused the ID;
- controlled pinned-libtorrent v1 downloads and uploads pass in both roles
  over TCP and default uTP, with focused tracker, DHT, incoming, and MSE
  regressions and exact payload hashes;
- proportional Android upgrade/reset, fresh-add, restart, and removal evidence
  passes, both Android ABIs build, and all owners terminate without residue;
  and
- the full repository validation baseline passes and the owning topics,
  readiness matrix, protocol ledger, and campaign checkpoint record that v2
  and hybrid support remain absent.

## Identity Contract

### Protocol identities

`rstorrent-protocol` owns runtime-free value types:

```text
ProtocolVersion = V1 | V2
V1InfoHash       = exactly 20 bytes
V2InfoHash       = exactly 32 bytes
InfoHashes       = { v1: Option<V1InfoHash>, v2: Option<V2InfoHash> }
SwarmKey         = V1(V1InfoHash) | V2Truncated([u8; 20])
```

`InfoHashes` construction rejects the neither-present case. Presence is
represented by `Option`; an all-zero byte string is never interpreted as
absence. `SwarmKey::V2Truncated` is derived only from the first 20 bytes of a
present full `V2InfoHash`. There is no `best`, implicit projection, or
length-based version inference.

Equality and persistence of a v2 identity always use all 32 bytes. Two v2
identities with the same first 20 bytes remain distinct full identities and
produce an ambiguous wire-key lookup rather than silently selecting an owner.
A numerically equal v1 key and truncated-v2 key are distinct because the
protocol version participates in equality and hashing.

The v1 metainfo and magnet parsers construct `InfoHashes { v1: Some(...),
v2: None }`. Their existing deterministic rejection of `meta version = 2`,
`btmh`, and mixed identities remains unchanged in this tactical.

### Stable torrent owner

`rstorrent-engine` owns `TorrentId([u8; 16])` as the engine/application owner
identity. It is not a protocol hash, content digest, peer ID, DHT node ID, or
security secret. The session store allocates it inside the add transaction
with SQLite `randomblob(16)`, rejects the all-zero value, relies on the primary
key for uniqueness, and retries at most four collisions before returning a
typed failure. This uses the existing SQLite dependency and adds no random or
UUID dependency.

The canonical application and filesystem representation is exactly:

```text
t1-<32 lowercase hexadecimal digits>
```

Parsing is strict and canonical: the prefix, length, lowercase alphabet, and
16 decoded bytes must match, and the all-zero ID is rejected. The representation
is safe as a URL path segment and portable filename. First-party clients may
compare and route it but must not derive protocol meaning from it.

Every add transaction allocates the owner row and all initially known aliases
atomically. A duplicate v1 add preserves the existing typed duplicate/no-op
semantics and returns the already-owned opaque ID; it does not allocate a
second owner. The internal alias-expansion transition is deterministic and
transactional for later metadata use, but no accepted product input invokes
v2 expansion in this tactical.

### Content fingerprint

`ContentFingerprint([u8; 32])` is SHA-256 over the exact retained raw info
bytes. It binds local have and part-file artifacts to one authenticated
metadata instance even for a v1-only torrent. It is an internal integrity
guard, not automatically a v2 protocol identity; only the later validated v2
metainfo path may establish that relationship.

No artifact requiring metadata is created before raw info is available. Exact
source SHA-256 remains a separate digest over the original outer `.torrent`
or magnet source and is not substituted for `ContentFingerprint`.

## Alias And Wire Lookup Semantics

One identity registry owns these operations without spawning a task:

```text
insert_owner(TorrentId, InfoHashes)
attach_aliases(TorrentId, discovered: InfoHashes)
remove_owner(TorrentId)
find_full(V1InfoHash | V2InfoHash) -> Missing | Unique(TorrentId)
find_wire(SwarmKey) -> Missing | Unique(TorrentId) | Ambiguous
```

Insertion and expansion preflight every full alias and derived wire key before
mutation. A full identity already owned by another torrent returns a typed
`IdentityConflict` and changes neither owner. This tactical never merges live
owners, moves storage, transfers have state, or picks a winner. A same-owner
alias replay is idempotent.

The wire index retains enough bounded membership to recover from removal; it
does not allocate during lookup. A truncated-v2 collision yields `Ambiguous`
until membership changes. Incoming plaintext and MSE routing must reject
missing or ambiguous resolution without exposing candidate IDs. Production
registrations remain v1-only, so current v1 behavior stays unique.

Tracker, DHT, peer-wire, MSE, metadata, incoming, and peer-picking APIs must
make the distinction visible at their first shared boundary. Byte codecs may
encode `[u8; 20]` after explicit selection, but that wire shape must not flow
back into authoritative owner or persistence state. The audit leaves genuine
20-byte values alone: v1 piece hashes, peer IDs, DHT node IDs, transaction
tokens, and other protocol-defined SHA-1 values are not torrent identities.

## Replacement Persistence And Artifact Format

### SQLite epoch

Schema version `19` is a fresh foundation, not a migration from schema 18.
`torrents` uses `torrent_id BLOB PRIMARY KEY` with an exact 16-byte, nonzero
check. Every torrent-owned table uses that key and `ON DELETE CASCADE` where
the current lifecycle requires it. This includes file selections, pending
ranges, prepared files, normalized trackers and hints, original sources,
removal jobs, queue state, and any torrent-keyed observation retained in the
main catalog.

The new alias authority is:

```text
torrent_identities(
    torrent_id  BLOB NOT NULL REFERENCES torrents(torrent_id)
                         ON DELETE CASCADE,
    protocol    TEXT NOT NULL CHECK (protocol IN ('v1', 'v2')),
    full_hash   BLOB NOT NULL,
    PRIMARY KEY (torrent_id, protocol),
    UNIQUE (protocol, full_hash),
    CHECK ((protocol = 'v1' AND length(full_hash) = 20) OR
           (protocol = 'v2' AND length(full_hash) = 32))
) WITHOUT ROWID
```

Store write transactions enforce one or two aliases and never commit an owner
with none. Store reads reject orphan owners, duplicate protocol aliases,
wrong-length hashes, and raw-info identities inconsistent with the currently
supported v1 parser. Full v2 rows are constructible only in focused store
tests until the next tactical adds a v2 parser.

Request receipts, snapshots, settings, and queue order use opaque ID strings
only at the serialized application boundary. Because the old receipt table is
discarded, no response containing a 40-character hash ID can replay into the
new epoch.

### Durable have state

`HaveState` version 2 replaces version 1 without a compatibility reader. Its
header contains, in order, magic, format version, 16-byte `TorrentId`, 32-byte
`ContentFingerprint`, and the bounded logical piece count, followed by the
same canonical high-bit-first bitmap and zero-padding rule. The exact fixed
header is 62 bytes and the maximum encoded state is 262,206 bytes for the
existing 2,097,152-piece limit.

Decode requires the expected owner, fingerprint, and piece count. Any
mismatch, unsupported version, malformed length, or nonzero padding rejects
the checkpoint and invokes the existing conservative verification path for
that torrent; it never aborts the healthy remainder of the profile.

The bitmap continues to mean fully verified logical pieces. In this tactical
that is exactly v1 SHA-1 verification. Later v2 and hybrid tacticals must not
set a bit until their stronger integrity contract is satisfied.

### Part file and storage namespace

Part-file version 2 has a 96-byte fixed identity header within the existing
1,024-byte-aligned header region: magic, version, declared header length,
`TorrentId`, `ContentFingerprint`, piece count, piece length, total length,
and zeroed reserved bytes. The existing bounded slot table and payload layout
remain unchanged. There is no version-1 reader.

Path-backed hidden artifacts become exact siblings of the final publication:

```text
.<t1-id>.rstorrent-staging
.<t1-id>.rstorrent-parts
```

`StorageFileKey::storage_id`, platform storage plans, namespace invalidation,
and SAF descriptor acquisition use the canonical opaque ID. Final published
files and trees retain the validated metainfo publication name; publication
does not rename user-visible content to the opaque ID.

The transient-artifact recognizer accepts only the new canonical ID grammar
and exact suffixes. Symlinks, wrong types, noncanonical IDs, unowned names,
and final publication paths retain the existing fail-closed behavior.

### Source and application projections

Original magnet and outer-metainfo bytes remain provenance; raw info,
normalized tracker/hint rows, and source digests remain operational authority
under the new foreign key. V1 magnet export reads the explicit v1 alias.

The generated application contract keeps the field name `torrent_id` but
changes its semantic validator to the exact opaque grammar. Torrent summaries
and detail expose a tagged protocol-identity record with optional lowercase
40-character `v1` and 64-character `v2` full hashes. The v2 field is absent in
all production snapshots in this tactical.

React and Compose routes, selection maps, media URLs, commands, events, and
diagnostics use the opaque ID. Places that label or export an info hash use
the explicit protocol-identity field. Public logs may include the bounded
opaque ID and protocol version but do not need to include full protocol
hashes.

## Clean Reset Contract

The durable reset target is exactly the application-private profile files:

```text
<profile_root>/session.db
<profile_root>/session.db-wal
<profile_root>/session.db-shm
```

Before any application task, socket, storage handle, or platform descriptor is
started, the store inspects `user_version`. A missing database creates schema
19. A regular database with recognized version `1..=18` is exclusively
locked, closed, and those three exact files are removed if present before a
fresh schema-19 store is created. Busy, symlinked, non-regular, malformed,
unversioned nonempty, or future-version databases fail with a typed startup
error rather than being deleted. Ephemeral mode simply creates schema 19 in
memory.

The reset discards the entire old session catalog, including torrents, have
state, sources, receipts, root registry, settings, DHT state, and pending
removals. The configured platform inputs may re-register roots in the fresh
store. The separate metrics database, installation-wide product identity, and
platform-owned grants are outside the reset.

No path or provider below a selected payload root is an automatic reset
target. In particular, the old exact shapes
`.<40-hex>.rstorrent-staging`, `.<40-hex>.rstorrent-parts`, legacy
`<root>/<40-hex>` artifacts, final publication names, Android SAF documents,
and materialization siblings are left untouched. They are reported as
external artifacts that may require explicit cleanup, never treated as
verified state, and never adopted by a new owner. A final-name collision on a
later add stays a typed storage conflict.

The newly created catalog retains one bounded `ProfileResetReport` containing
the previous schema version, the discarded catalog categories, the three
database basenames considered, and `external_payload_modified = false`.
Application startup publishes the report once through structured diagnostics
and makes it available to headless/platform startup evidence. It contains no
payload locator or public network data.

Crash tests cover interruption after legacy detection, after exclusive close,
after any subset of the three removals, and after new database creation. Every
recoverable state converges to one valid empty schema-19 catalog; an unsafe or
ambiguous file shape fails without touching external roots.

## Owner, Task, Cancellation, And Dependency Map

| Owner | Mutable state and work | Cancellation and termination |
| --- | --- | --- |
| Protocol identity values | validated full identities and explicit wire projection | pure values; no runtime or infrastructure types |
| Session store | owner allocation, alias uniqueness, reset report, catalog transactions | reset completes synchronously before tasks; transactions roll back atomically |
| Application torrent registry | `TorrentId` to runtime plus full/wire alias indexes | existing serialized application owner; generation removal unregisters all aliases |
| Incoming registry | bounded `SwarmKey` membership and missing/unique/ambiguous lookup | existing joined incoming generation; shutdown drains registrations before listener exit |
| Torrent runtime | one owner ID plus its protocol identities and existing generation state | current torrent cancellation fence; stale callbacks carry the owner generation |
| Storage/checkpoint owners | owner/fingerprint-bound have, part, path/SAF namespace and handles | existing storage cancellation and pool invalidation; no new task |
| Tracker/DHT/MSE/peer owners | selected v1 `SwarmKey` for current operations | existing per-operation/task cancellation; no v2 operation admitted |
| Application contract and clients | opaque routing ID plus explicit display identities | existing subscription, request, activity, and service lifecycles |

Dependency direction is:

```text
rstorrent-protocol identity values
        -> rstorrent-engine owner/runtime/storage boundaries
        -> rstorrent-session persistence and application ownership
        -> gateway, Tauri, UniFFI, Android, and React consumers
```

Protocol code never depends on `TorrentId`, SQLite, async runtimes, storage,
or client schemas. The engine never depends on session persistence or a
platform adapter. Platform code receives opaque IDs and descriptors but does
not reproduce alias or hash policy.

The concrete boundary improvement is to replace semantically unrelated raw
`[u8; 20]` and `String` parameters with the smallest owning type, rather than
adding a generic identity service or compatibility façade.

## Resource And Security Bounds

- `TorrentId` is 16 bytes internally and 35 ASCII bytes externally; allocation
  retries at most four database uniqueness conflicts.
- Each torrent has one or two full aliases, never zero and never more than one
  per protocol version. With the existing 1,024-torrent/runtime bound, the
  live alias registry holds at most 2,048 memberships.
- Full-identity and wire-key lookup allocate nothing. Ambiguity is retained
  explicitly and never resolved by insertion order.
- Existing metainfo, source, magnet, file, piece, tracker, hint, receipt,
  session-page, peer, and task limits do not increase. The have-state maximum
  changes only to the exact 262,206-byte version-2 bound.
- ID, identity, and fingerprint bytes are length-checked before copying or
  allocation. Arithmetic for headers and table sizes remains checked.
- No peer-controlled value can allocate a `TorrentId`, attach an alias, or
  change persistence until its existing source and identity authentication
  succeeds.
- Full v2 hashes are never truncated for equality, database uniqueness,
  artifact identity, client routing, or logs that claim authoritative
  identity.
- Reset never follows symlinks, globs, unresolved environment variables, or
  paths from the legacy database. Only the three fixed profile basenames may
  be removed automatically.
- Published and partial payload bytes provide no have authority after reset.
  A later add begins absent and must follow normal storage conflict,
  observation, checking, and verification policy.

Record alias membership, reset-file, task, descriptor, database page, and
storage-handle high-water marks where the relevant harness already exposes
them. This tactical adds no per-torrent task, background timer, socket,
descriptor, or peer-controlled queue.

## Source-First Record

No reference source, fixture, or test vector is copied.

### Normative specifications

Re-open before implementation the managed BEP checkout at exact commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`:

- `beps/bep_0052.rst`, especially **infohash**, trackers, DHT, peer protocol,
  and compatibility, requires exact SHA-256 info identity, 20-byte truncation
  only for specified wire uses, and both identities for hybrid operation;
- `beps/bep_0009.rst` defines full multihash `btmh` and permits exact `btih`
  and `btmh` topics for one hybrid torrent; and
- existing BEPs 3, 5, 6, 10, 15, 23, and 29 remain the normative v1 behavior
  whose typed inputs change but whose wire bytes do not.

Adopt full authoritative identities, explicit wire projection, and dual
identity cardinality. This tactical deliberately does not accept `btmh`, v2
metainfo, hybrid negotiation, or any new wire message.

### Pinned libtorrent

Re-inspected Rasterbar libtorrent `2.0.13` at exact commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/info_hash.hpp::info_hash_t` holds optional v1 and v2
  values, projects v2 to 20 bytes only with a protocol version, and iterates
  both identities;
- `include/libtorrent/aux_/torrent_list.hpp` plus
  `src/session_impl.cpp::{find_torrent,insert_torrent,
  update_torrent_info_hash,add_torrent_impl}` index one torrent through every
  known hash and update the index when metadata expands identity;
- `src/torrent.cpp::set_metadata` authenticates against the known identity,
  discovers hybrid aliases, checks every new alias for another live torrent,
  and fails both owners instead of violating one-hash/one-torrent ownership;
- `src/read_resume_data.cpp` and `src/write_resume_data.cpp` retain full v1/v2
  identity rather than only the wire truncation;
- `test/test_info_hash.cpp` covers empty, v1, v2, hybrid, ordering, presence,
  projection, and hashing behavior;
- `test/test_torrent_list.cpp` covers v1/v2/hybrid insertion, lookup, duplicate
  aliases, self-overlap, removal, and truncated-v2 lookup;
- `test/test_magnet.cpp` covers full v2 and hybrid identity parsing and resume
  round trips; and
- `test/test_read_resume.cpp` covers v1-only, v2-only, full identity round trip,
  and mismatched resume identities.

RSTorrent adopts optional full identities, alias expansion, duplicate
preflight, and explicit versioned wire projection. It deliberately differs by
using an opaque stable owner so metadata expansion never changes application
or storage identity; representing missing hashes with `Option` rather than a
zero sentinel; treating a truncated-v2 collision as ambiguous; and returning
a typed conflict without automatically failing both live owners in this
foundation. Later metadata tactics must choose the user-visible lifecycle
response without weakening the atomic conflict.

### JSTorrent product history

Re-inspected local sibling commit
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/engine/src/core/torrent-parser.ts` computes and uses a single v1
  SHA-1 info hash;
- `packages/engine/src/core/torrent.ts` uses that hash as product identity and
  fallback display text; and
- `packages/engine/src/core/peer-connection.ts::handleExtendedHandshake`
  detects a full `info_hash2` after a truncated-v2 hybrid connection and
  disconnects because the v1 piece model cannot continue safely.

Retain the useful fail-closed response and explicit product display fallback.
Do not inherit the single-hash owner, use a truncated v2 hash as identity, or
claim that detecting `info_hash2` constitutes v2 support.

## Existing RSTorrent Boundaries To Audit

The implementation inventory begins with these exact owners:

- `rstorrent-protocol`: `metainfo.rs`, `metainfo/direct.rs`, `magnet.rs`,
  `peer_wire.rs`, `metadata.rs`, `mse/handshake.rs`, `dht.rs`,
  `udp_tracker.rs`, and `storage_layout.rs`;
- `rstorrent-engine`: `driver.rs`, `peer_socket.rs`, `incoming.rs`, `dht.rs`,
  `http_tracker.rs`, `metadata_seed.rs`, `torrent_peer.rs`,
  `selective_storage.rs`, `part_file.rs`, `storage_file_pool.rs`,
  `seed_content.rs`, `active_seed_content.rs`, and `upload.rs`;
- `rstorrent-session`: `store.rs`, `store_schema.rs`, `have.rs`, `control.rs`,
  `application.rs`, `torrent_runtime.rs`, `incoming_seeding.rs`, `media.rs`,
  and the view contract/projections;
- generated and platform boundaries in `rstorrent-gateway`,
  `rstorrent-android`, `clients/desktop`, and `clients/web`; and
- fixtures, probes, and interop helpers that currently derive an application
  ID or storage namespace from SHA-1.

The audit classifies every `info_hash`, `torrent_id`, and `[u8; 20]` occurrence
by semantic role before replacement. It must not mechanically rename DHT node
IDs, peer IDs, v1 piece hashes, or unrelated SHA-1 fields.

## Implementation Stages And Gates

1. **Inventory and baseline:** classify identity uses, record exact focused
   reference findings, and run the current v1 parser, persistence, storage,
   incoming, DHT, tracker, MSE, generated-contract, and Android build gates.
2. **Pure identity values:** add the full identity, version, wire key, owner,
   fingerprint, strict codecs, and collision/ambiguity tests without runtime
   or persistence dependencies.
3. **Fresh catalog epoch:** create schema 19, alias authority, opaque ID
   allocation, recognized-profile reset, reset reporting, ephemeral behavior,
   and crash/fail-closed tests; remove obsolete schema-1-through-18 migration
   code and fixtures.
4. **Durable artifacts:** replace have-state, part-file, source foreign keys,
   storage namespace, transient-name recognition, path cleanup planning, and
   SAF plans as one format epoch.
5. **V1 runtime threading:** carry `InfoHashes`, `TorrentId`, and `SwarmKey`
   through metainfo, magnets, metadata, driver, incoming, MSE, peer handshake,
   tracker, DHT, seed/upload, diagnostics, and cancellation generations while
   admitting only v1.
6. **Application and clients:** change semantic validation and explicit
   protocol-identity projections, regenerate TypeScript/schema/UniFFI, and
   update React, Tauri, media, Android service, and Compose consumers.
7. **Layered evidence:** run deterministic and persistence suites, controlled
   reset/storage failure profiles, pinned-libtorrent v1 interop, Android
   upgrade/fresh/restart/removal evidence, resource high-water capture, and
   terminal cleanup audits.
8. **Closure:** reconcile this tactical, every owning topic, readiness,
   protocol claims, and campaign checkpoint without activating the BEP 52
   parser tactical implicitly.

Each stage lands only after its focused tests pass. Intermediate commits may
not claim a usable mixed old/new profile; the implementation branch remains a
single replacement epoch and the final baseline proves the integrated shape.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure Rust | construction/presence, exact encodings, zero-as-present hash behavior, missing-set rejection, full-v2 equality, version-separated equal 20-byte keys, truncated-v2 ambiguity, atomic alias conflict, owner-ID codec and bounded generation |
| Protocol v1 | exact info hashing, magnet round trip, deterministic v2/hybrid rejection, peer handshake, metadata SHA-1, MSE request hash, tracker encoding, and DHT query bytes unchanged |
| SQLite | fresh schema 19, exact constraints/FKs, one/two/no alias cases, duplicate/replay/rollback, opaque ID stability, durable and ephemeral reopen, page cap, malformed rows, and no old receipt replay |
| Reset | recognized versions including 18, future/unversioned/malformed/busy/symlink failures, sidecar subsets, crash points, one-shot report, settings/catalog reset, metrics survival, and zero payload-root mutations |
| Have and part | boundary piece counts, owner/fingerprint/geometry mismatch, bad version/length/padding/reserved/slot data, conservative fallback, new path grammar, no v1 reader, and no legacy artifact adoption |
| Storage lifecycle | path and SAF absent/staging/publishing/published transitions, selection, checking, trusting restart, publication-name collision, keep/delete-managed removal, grant loss/repair, and exact handle invalidation |
| Engine runtime | v1 magnet metadata acquisition and `.torrent` transfer, outgoing/incoming plaintext, TCP MSE, default uTP, Fast/PEX/metadata upload, archive/pause/resume, cancellation, and zero terminal owners |
| Discovery | HTTP/UDP tracker and IPv4/IPv6 DHT lookup/announce use tagged v1 keys and preserve existing schedules, routing, privacy suppression, and observations |
| Application/client | JSON serialization and validators, generated TypeScript/schema/UniFFI, React and Compose opaque routing, explicit info-hash display/export, Tauri/media URLs, replay, view replacement, and stale-ID rejection |
| Controlled oracle | pinned-libtorrent v1 seed and leecher roles over TCP/default uTP, focused MSE and incoming cases, exact payload/source hashes, one owner, selected transport proof, and cleanup |
| Android | both ABI builds plus API-34 schema-18 upgrade/reset with published/partial sentinel bytes, fresh v1 add, forced restart, trusting or conservative resume as applicable, keep/delete-managed removal, and no descriptor/task residue |
| Repository | formatting, warning-denying workspace Clippy, workspace tests, web typecheck/tests, generated-contract cleanliness, no temporary artifacts, and clean diff |

Representative live public-swarm evidence is not required. Controlled oracle
and first-party composition are stronger for this identity-only v1 regression.

## Non-Goals And Deliberate Deferrals

This tactical does not:

- parse or accept v2/hybrid `.torrent` files or `btmh` magnets;
- implement BEP 52 file trees, piece layers, Merkle roots/proofs, alignment
  gaps, messages 21--23, hybrid upgrade, or dual verification;
- announce, dial, accept, seed, or download through a v2 swarm;
- reconcile two already-live owners when later metadata proves they are one
  hybrid torrent;
- delete, import, reattach, or verify old published or partial payload;
- preserve any schema-1-through-18 catalog row, receipt, setting, or migration
  fixture;
- change final publication naming, add a product migration UI, or implement a
  general orphan-artifact cleaner;
- create v2 or hybrid torrent files;
- add a UUID/randomness/database/identity framework dependency; or
- change Tactical 142's active status or claim any BEP 52 support.

The next tactical remains runtime-free BEP 52 metainfo, geometry, and Merkle
work. It may use the v2 identity slots created here, but activation still
requires an explicit readiness decision.

## Escalation Contract

Stop for maintainer direction before implementation if evidence requires:

- changing the accepted opaque application-ID format or treating a protocol
  hash as the owner again;
- a new dependency with meaningful lifecycle, portability, or supply-chain
  tradeoffs;
- preserving selected legacy catalog data instead of the accepted full reset;
- deleting any path or provider object beneath a user-selected payload root;
- merging, moving, or discarding one live owner after an alias conflict;
- weakening full-v2 identity, ambiguous-wire-key, or artifact-integrity
  checks; or
- expanding this foundation into v2 parsing, wire operation, creation, a
  support claim, or another platform surface.

Routine type placement, table query shape, generated fixture churn, focused
refactors, test calibration within the stated bounds, and logical commits are
implementation choices once readiness activates this tactical.
