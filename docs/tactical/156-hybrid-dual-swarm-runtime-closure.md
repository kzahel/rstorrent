# Tactical 156: Hybrid Dual-Swarm Runtime Closure

Status: **In progress.** Stage 0 reconfirmed the normative and source-oracle
pins and recorded green focused baselines. Implementation is proceeding from
the runtime-free hybrid content and integrity foundation. This tactical closes
the bounded BEP 52 hybrid consumption and seeding
subset through one torrent owner, two protocol identities, two swarm lanes,
mandatory SHA-1 plus SHA-256 verification, safe metadata-time reconciliation,
and proportional first-party evidence. Torrent creation remains separate.

Topics: `bittorrent-v2-and-hybrid`, `protocol-support`,
`download-correctness`, `client-persistence`, `peer-lifecycle`,
`incoming-reachability-and-seeding`, `application-view-api`,
`client-surfaces`, `code-organization-and-refactoring`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tacticals
[`143`](143-dual-identity-and-persistence-foundation.md),
[`146`](146-runtime-free-bep52-metainfo-geometry-merkle.md),
[`151`](151-complete-source-pure-v2-runtime-vertical.md), and
[`155`](155-v2-magnet-authenticated-hash-exchange.md), plus the existing v1
and pure-v2 metadata, discovery, peer, storage, integrity, persistence,
upload, generated-boundary, and Android owners.

## Decision And Desired Outcome

Add one strict hybrid runtime vertical. A valid hybrid torrent is one product
torrent with one opaque owner, one selection, one payload namespace, one
publication lifecycle, a full v1 SHA-1 identity, and a full v2 SHA-256
identity. That owner participates independently in the v1 and v2 tracker,
DHT, peer-handshake, and MSE namespaces while sharing verified bytes.

The product contract is:

- a hybrid appears once in every application surface;
- complete hybrid `.torrent` input and exact hybrid magnets are admitted;
- a magnet may initially name only `btih`, only full `btmh`, or both, but
  authenticated metadata must prove the resulting complete alias set;
- separately added v1 and v2 magnets may remain distinct provisional owners
  only until exact authenticated metadata proves that both aliases name the
  same hybrid info dictionary;
- reconciliation is a metadata-admission transaction, not a live payload or
  filesystem merge: every revealed alias is reserved before content,
  selection-derived scheduling, candidate have, or publication can begin;
- the first-created owner survives reconciliation, keeps its torrent ID,
  destination, selection, queue position, and other user intent, and may
  combine only bounded trackers, peer hints, and discovery observations from
  the later provisional owner;
- normal re-addition of an already known owner keeps the existing duplicate
  contract, including explicit selection promotion; cross-owner
  reconciliation does not silently import the losing owner's selection;
- canonical hybrid magnet export includes both exact topics in deterministic
  v1-then-v2 order, while a verbatim source is reusable only when it contains
  both matching identities;
- padding files and pure-v2 alignment gaps remain hidden implementation
  details, not selectable or publishable user files; and
- a piece becomes durable have, readable, publishable, or uploadable only
  after both its v1 and v2 integrity requirements pass.

RSTorrent does not downgrade an inconsistent hybrid to one format. Metadata
layout disagreement, identity collision, or a piece that passes only one hash
scheme stops the torrent with typed integrity evidence. Choosing a preferred
swarm would weaken the accepted product guarantee and is outside this
tactical.

## Scope And Stopping Condition

This tactical owns:

1. production admission of Tactical `146`'s strict complete hybrid outer
   metainfo and hybrid info-only metadata through the ordinary application;
2. one explicit hybrid content/integrity model over the existing aligned v2
   geometry, exact v1 piece hashes, file roots, and authenticated v2 catalog;
3. strict mixed `btih` plus `btmh` magnet parsing, canonicalization, source
   verification, persistence, duplicate handling, and export;
4. metadata-time identity expansion from either single identity, atomic
   reservation of both aliases, and deterministic reconciliation of two
   provisional owners without moving or deleting published payload;
5. one shared torrent supervisor with exactly two fixed protocol-version
   swarm lanes for tracker, DHT, peer observations, dial routing, and
   diagnostics;
6. the BEP 52 hybrid reserved-bit upgrade for initiated and accepted TCP,
   uTP, and carried MSE peer handshakes, including exact final connection
   protocol selection;
7. hash-first payload scheduling and mandatory SHA-1 plus SHA-256 checking,
   recheck, corruption handling, candidate recovery, and contributor truth;
8. BEP 47 internal padding service using deterministic zeros without
   materializing pad files, plus only the already accepted historical
   missing-final-tail-pad compatibility shape;
9. restart with one owner and both aliases, selection, source intent,
   candidate state, discovery version, and conservative sparse-hash refetch;
10. active and completed metadata, authenticated hash, padding, and verified
    payload service through both swarm identities;
11. ordinary add/export, files, selection, progress, pause/resume, recheck,
    open/stream, removal, diagnostics, and generated-client behavior; and
12. deterministic hostile tests, controlled pinned-libtorrent evidence in
    both engines and both swarms, bounded high-water marks, terminal cleanup,
    web/Tauri/iOS gates, both Android ABIs, and an owned API 34 AVD run.

The tactical stops only when all of the following are true:

- strict canonical and accepted historical-tail hybrid `.torrent` sources
  reach exact selected content through one ordinary application owner;
- `btih`-only, `btmh`-only, and dual-topic magnets authenticate the same
  exact hybrid info, install both aliases, and export both exact topics;
- two separately added single-identity magnets that race for metadata retain
  the first owner and terminate the other generation before either can
  schedule payload or install durable have;
- wrong identity pairs, pure metadata behind a dual-topic magnet, alias
  collisions, incompatible layouts, missing internal padding, and stale
  reconciliation results fail before storage authority changes;
- v1 and v2 tracker and DHT operations are independently observable and
  share existing session/torrent admission ceilings rather than doubling
  owner or task limits implicitly;
- v1-entry hybrid handshakes upgrade only when both peers signal the BEP 52
  capability and the responder returns the correct truncated v2 identity;
  otherwise they remain valid v1 connections or fail closed as appropriate;
- no new payload request is issued until both expected integrity values are
  available, and a completed piece passes SHA-1 and SHA-256 before have;
- corruption that passes only one scheme produces typed inconsistent-hybrid
  failure, no have, no upload, no format fallback, and bounded contributor
  evidence;
- internal and final padding is synthesized as zero bytes for v1 hashing and
  legacy upload requests, but is never requested, written, selected,
  published, or counted as product payload;
- restart, force recheck, selection promotion, active reads, publication,
  upload, seeding, pause, remove, and shutdown preserve the one-owner/two-
  alias and dual-integrity invariants;
- RSTorrent and pinned libtorrent each complete as leecher from the other
  through isolated v1 and v2 hybrid swarm entry paths with independent
  payload comparison, including initiated and accepted peer directions;
- reserved-bit upgrade, direct-v2 routing, tracker, DHT, TCP, default uTP,
  and forced-RC4 MSE receive the proportional matrix below without an
  unnecessary full cross-product;
- pure v1, complete-source pure-v2, and pure-v2 magnet regressions remain
  green;
- production browser and API 34 application runs each show one row, both
  identities, exact selection/progress, restart, seeding, and removal, while
  Tauri/iOS build gates and both Android ABIs pass; and
- the BEP ledger, owning topics, execution record, resource highs, and
  deliberate deferrals claim only the demonstrated hybrid subset.

## Normative And Source-Oracle Record

Implementation must reconfirm every managed revision before code changes.
This planning review used the pins in
[`reference/pins.toml`](../../reference/pins.toml).

### Normative specifications

The BEP checkout is pinned at
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`.

- `reference/bittorrent.org/beps/bep_0052.rst` defines hybrid torrents as one
  exact info dictionary containing v1 `pieces` and file fields plus v2 `meta
  version`, `file tree`, and roots. The two halves must describe identical
  names, order, and alignment; multifile hybrids use BEP 47 padding; clients
  joining both swarms verify both hash formats.
- BEP 52 permits an initiator using the v1 SHA-1 handshake hash to set
  `reserved[7] & 0x10`. A compatible responder may return the truncated v2
  hash, making that connection v2. The returned hash selects the connection
  protocol; the bit alone does not authenticate metadata or authorize an
  arbitrary alias.
- `reference/bittorrent.org/beps/bep_0004.rst` assigns `reserved[7] & 0x10`
  to the hybrid legacy-to-v2 upgrade. Existing extension, DHT, and fast bits
  remain independent.
- `reference/bittorrent.org/beps/bep_0009.rst` permits `btih` and `btmh`
  exact topics in one magnet only when they identify the same hybrid torrent.
  Metadata remains the exact info dictionary, so both known identities must
  be checked over those exact bytes before expansion or reconciliation.
- `reference/bittorrent.org/beps/bep_0047.rst` defines padding content as
  zeros, recommends `.pad/<length>`, permits an omitted path, says aware
  clients should avoid writing or requesting padding, and requires them to
  service compatible legacy padding requests. Padding misuse can make a
  torrent internally inconsistent and must not endanger existing data.
- `reference/bittorrent.org/beps/bep_0003.rst` continues to define the v1
  file list, `pieces` sequence, piece numbering, requests, and SHA-1
  verification composed by a hybrid.
- BEPs 5, 10, 11, 15, 23, and 29 continue to govern the already-supported
  DHT, extension, tracker, compact-peer, and uTP mechanisms. This tactical
  versions their torrent key; it does not redefine those protocols.

### Pinned libtorrent source

Rasterbar libtorrent `2.0.13` is pinned at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. The review inspected:

- `src/torrent_info.cpp::parse_info_section()` for exact dual hashing,
  v1/v2 extraction, compatible-file comparison, and hybrid rejection;
- `src/file_storage.cpp::{add_file_borrow,remove_tail_padding}` and
  `aux::files_compatible()` for generated alignment padding, historical
  final-tail omission, and name/size/offset comparison;
- `src/magnet_uri.cpp::{parse_magnet_uri,make_magnet_uri}` for mixed identity
  parsing and deterministic hybrid generation;
- `src/torrent.cpp::set_metadata()` for validation against one or both known
  hashes, identity expansion after metadata, session collision detection,
  and Merkle initialization;
- `src/session_impl.cpp::add_torrent_impl()` and the torrent-index update path
  for full alias lookup and duplicate admission. RSTorrent deliberately uses
  a deterministic pre-payload reconciliation transaction instead of
  libtorrent's two-owner conflict pause;
- `src/bt_peer_connection.cpp::write_handshake()` and its
  `state_t::read_info_hash` path for advertising upgrade support, responding
  with the v2 hash, final protocol selection, and invalid-hash rejection;
- `src/peer_connection.cpp::{attach_to_torrent,associated_info_hash,
  on_seed_mode_hashed}` for versioned association, per-connection identity,
  and dual verification while seeding;
- `src/torrent.cpp::{on_piece_hashed,handle_inconsistent_hashes,
  start_checking,on_blocks_hashed}` and the received-piece completion path
  for requiring compatible SHA-1 and Merkle outcomes and stopping on
  disagreement;
- `src/torrent.cpp` tracker iteration and `src/session_impl.cpp` torrent
  indices for independent announces/lookups using both identities; and
- `src/torrent.cpp`, `src/read_resume_data.cpp`, and
  `src/write_resume_data.cpp` for two-hash restart and seed restoration
  shapes. RSTorrent retains its own opaque-owner schema and volatile sparse
  hash policy.

### Pinned libtorrent tests

The edge-case inventory includes:

- `test/test_torrent_info.cpp::parse_torrents` entries `v2.torrent`,
  `v2_multipiece_file.torrent`, `v2_hybrid.torrent`, and the empty-file
  hybrids, plus `parse_invalid_torrents` entries
  `v2_mismatching_metadata.torrent`, `v2_bad_file_alignment.torrent`, and
  `v2_invalid_pad_file.torrent`;
- `test/test_create_torrent.cpp::{create_torrent_round_trip_hybrid,
  create_torrent_round_trip_hybrid_missing_tailpad,hybrid,
  hybrid_single_file,hybrid_single_file_with_directory,
  hybrid_no_tail_padding}` for creator-style and historical layouts. These
  are fixture-shape references, not authorization to implement creation;
- `test/test_file_storage.cpp::{files_compatible,
  files_compatible_num_files,files_compatible_size,files_compatible_name,
  files_compatible_pad,files_compatible_empty_file_order,
  files_compatible_piece_size,remove_tail_padding_not_last,
  remove_tail_padding_last,remove_tail_padding_no_op}` for exact comparison
  and compatibility boundaries;
- `test/test_magnet.cpp::{parse_hybrid_uri,make_magnet_uri_hybrid,
  hybrid_info_hashes,save_resume_data_magnet_hybrid}` for two-topic identity
  and restart forms;
- `test/test_read_resume.cpp::{read_resume_info_hash2,
  round_trip_info_hash,round_trip_have_pieces,round_trip_verified_pieces}`
  for restart cases rather than a format to copy;
- `simulation/transfer_sim.cpp` and `simulation/test_transfer.cpp` for the
  default hybrid transfer cells, padding shapes, corruption, magnet, TCP,
  uTP, encryption, and initiated/accepted roles; and
- `simulation/test_tracker.cpp` dual-announce cases, including its explicit
  one-announce-per-hash expectations for hybrid tracker tiers.

RSTorrent intentionally differs from or narrows the oracle in five places:

1. exact v1/v2 aliases are attached to one opaque owner before payload work;
   a collision does not leave two paused product rows indefinitely;
2. reconciliation is permitted only while the losing owner is provisional.
   It is cancelled and joined before the winner adopts discovery facts; no
   active payload, have bitmap, file handle, or published namespace is merged;
3. structural compatibility remains Tactical `146`'s stricter raw-path,
   empty-file, offset, and internal-pad comparison. Only the historical
   omitted final tail pad is accepted;
4. both hash schemes are mandatory. RSTorrent does not take BEP 52's optional
   fallback to a single format after inconsistency; and
5. sparse v2 hashes remain volatile. Incomplete restart refetches them and
   rechecks candidate bytes before restoring dual-verified have.

No libtorrent source or checked-in fixture is copied. The controlled harness
generates temporary hybrid inputs with the pinned oracle, records independent
SHA-1/SHA-256 payload evidence, and removes every source, profile, capture,
and process it owns.

### JSTorrent product history

The local JSTorrent checkout is at
`9895410beeed6aff554053769bd006a3fbd373ef`. It was already dirty with
unrelated untracked attachment, design, and investigation directories; this
review treated it as read-only.

- `packages/engine/src/core/peer-connection.ts` recognizes BEP 52
  `info_hash2` and disconnects because its v1-only piece/storage model cannot
  safely continue.
- `packages/engine/src/core/metadata-fetcher.ts` diagnoses the truncated-v2
  hybrid case but has no dual-swarm or dual-verification owner.
- `docs/archive/tasks/bep52-v2-implementation-plan.md` identifies hybrid
  identity and SHA-256 poisoning resistance as desired future behavior, but
  remains an archived plan rather than runtime evidence.

The useful product lesson is fail-closed upgrade handling within the ordinary
one-row add/download experience. RSTorrent does not adopt JSTorrent's
identity, daemon, filesystem, or optional-hash design.

## Extracted Shape-Changing Edge Cases

The implementation must front-load these cases before the happy path can
serve as architectural evidence:

- dual-topic magnets accept one exact v1 identity and one exact `1220` v2
  identity in either input order; equal duplicate topics are idempotent;
  conflicting duplicates, unsupported multihashes, or more identities fail;
- a dual-topic magnet's exact metadata must satisfy both hashes and parse as
  hybrid. Pure-v1, pure-v2, future-version, malformed, or structurally
  inconsistent metadata cannot select one supplied identity and continue;
- metadata from a single-topic magnet may reveal the second identity only
  after its originally known hash passes and the hybrid parser validates both
  halves. The derived hash is then an authenticated alias, not a peer hint;
- two provisional owners can race metadata completion, timeout, reconnect,
  pause, remove, and shutdown. The first-created owner wins regardless of
  which receives metadata first; loser results after cancellation are stale;
- an alias already held by a non-provisional owner, an alias shared by two
  different exact info dictionaries, or a 20-byte v1/truncated-v2 collision
  is a typed conflict, never reconciliation evidence;
- alias reservation, losing-generation cancellation/join, durable-row
  consolidation, and winner activation must recover deterministically across
  transaction failure or process restart. There is never a durable state
  where two owners claim one alias;
- reconciliation combines bounded trackers and peer observations with source
  provenance and normal deduplication, but not destination, selection, queue
  position, payload, candidate have, reputation, or task state;
- normal duplicate re-add after reconciliation resolves through either alias
  to the survivor; only explicit duplicate selection intent may promote files;
- single-file, one-block, one-piece, multi-piece, multifile, empty-file, and
  selected/skipped hybrid shapes share exact payload-file indices;
- internal padding must exactly fill every gap and a present final tail pad
  must have exact size and placement. Missing internal, extra, reordered,
  duplicated, mis-sized, unmarked, or payload-colliding padding fails;
- the accepted historical final-tail omission changes neither logical piece
  count nor payload mapping. Padding path spelling is advisory; padding flag,
  position, length, offset, and zero content are not;
- padding bytes count in v1 piece SHA-1 but not product wanted/stored bytes.
  Requests that cover only padding are answered with zeros; requests crossing
  payload and padding assemble bounded exact spans without a pad artifact;
- a hybrid piece has independent `v1_expected`, `v2_expected`, computed
  SHA-1, computed leaves, and a single dual result. Neither success can set
  have before the other succeeds;
- a magnet with missing v2 piece roots may obtain them through a v2-direct or
  upgraded peer. New payload waits; existing candidate payload remains
  unavailable until both checks pass;
- a piece may pass SHA-1/fail SHA-256, fail SHA-1/pass SHA-256, fail both, or
  pass both. The first two are inconsistent-hybrid terminal errors; the third
  uses bounded ordinary corruption recovery only when both failures agree on
  invalid data; the fourth alone authorizes have;
- a v2 leaf proof can retain exact good blocks after an agreeing dual
  corruption failure, but no retained block or contributor becomes hybrid
  have independently of the piece's v1 result;
- direct v1, direct truncated-v2, and v1-with-upgrade handshakes must route to
  the same owner while retaining distinct connection protocol. A wrong v2
  response, upgrade bit on non-hybrid metadata, or unsupported alias fails;
- the same endpoint learned in both swarms is one bounded peer observation
  with version capabilities, not unlimited duplicate dial work. At most one
  live connection per duplicate peer identity survives existing resolution;
- tracker and DHT success/failure/backoff for one version cannot overwrite
  the other version's state or suppress its progress;
- upload bitfields, have, piece service, hash service, metadata service, and
  padding service use the connection's selected protocol while reading the
  same dual-verified content;
- restart with raw hybrid info but no sparse v2 hashes restores both aliases
  yet advertises no candidate have until hash refetch/reconstruction and both
  checks pass; and
- cancellation during metadata, alias reservation, reconciliation, announce,
  dial, handshake upgrade, hash acquisition, dual check, padding service,
  sync, publication, or upload cannot mutate a replacement generation.

## Accepted Runtime And Module Shape

### Hybrid content and integrity

Extend the runtime-free content enum explicitly rather than representing a
hybrid as a v1 object with optional v2 fields or as two torrents:

```text
HybridContentDescriptor
  exact raw info
  InfoHashes { full v1, full v2 }
  exact v1 piece hashes
  one validated V2TorrentLayout
  one validated HybridPaddingMap
  file roots and outer source facts

HybridIntegrity
  V2HashCatalog
  per-piece dual verification state
```

`TorrentContent::Hybrid` is a third semantic variant. It exposes the same
logical payload files and aligned global piece indices as the v2 descriptor,
plus a zero-copy lookup of the corresponding v1 expected piece hash. The
padding map names synthetic zero spans in peer-piece space and never creates
user files or storage paths.

The runtime-free protocol layer owns format classification, exact identities,
layout compatibility, expected-hash lookup, padding-span mapping, and the
pure transition from two verification results to `Verified`, `Invalid`, or
`Inconsistent`. It must not depend on Tokio, sockets, filesystems, channels,
clocks, persistence, or platform adapters.

### Magnet admission, aliases, and reconciliation

Generalize the supported magnet identity from exactly one value to a bounded
`KnownInfoHashes` set with only these admitted shapes:

```text
PureV1    = one btih
PureV2    = one full btmh:1220
CandidateHybrid = one btih + one full btmh:1220
```

Metadata admission hashes the exact assembled info once with SHA-1 and once
with SHA-256 as required by the known set. A single known identity must match;
then successful hybrid parsing authenticates the other. A dual known set
requires both to match. Install content only after a store transaction can
reserve every authenticated alias for the same opaque owner.

For separate provisional owners, use this fixed reconciliation algorithm:

1. lock or serialize alias admission in the existing application/store owner;
2. identify the oldest durable owner by creation sequence; it is the winner;
3. fence both current runtime generations before either installs content;
4. cancel and join the loser metadata, discovery, peer, hash, storage, and
   publication work;
5. atomically attach both aliases and safe bounded discovery/source facts to
   the winner and retire the loser's durable provisional row;
6. activate one fresh winner generation from authenticated metadata; and
7. delete only exact loser artifacts in RSTorrent-owned managed staging after
   joined ownership proves they were never published.

The transaction never deletes a user-selected root or published payload. If
the loser somehow crossed the provisional fence, reconciliation aborts as a
typed conflict and preserves both payload namespaces for explicit recovery;
implementation must fix the fence rather than invent a live merge.

Canonical export emits:

```text
xt=urn:btih:<40 lowercase hex>
xt=urn:btmh:1220<64 lowercase hex>
```

in that order, followed by the existing bounded display name, tracker,
peer-hint, and select-only fields. Source verification accepts a verbatim
hybrid magnet only when both identities match. A single-topic source is
retained for provenance but canonicalized to both topics after metadata.

### Shared storage and padding

The existing format-aware v2 layout remains the canonical product and storage
geometry. V1 piece numbers refer to the same aligned global pieces. The
hybrid padding map supplies zero spans to hashing and legacy upload without
allocating, opening, writing, syncing, selecting, publishing, or deleting pad
files.

Storage jobs continue to stream fixed 16-KiB chunks. A dual check computes
SHA-1 over the full peer piece, including synthetic zero spans, while feeding
real file bytes into the existing SHA-256 leaf/Merkle path. One read plan and
one storage job return both results; do not read or buffer the piece twice.
Short real file tails hash at their actual v2 block length while the v1 hash
continues over the protocol piece including the validated zero alignment.

Legacy incoming requests that overlap padding are served by one bounded span
plan. Real spans use the ordinary verified read owner and synthetic spans
write zeros. Pure-padding requests require dual-verified authority for their
containing piece and consume ordinary upload grants and bytes; they do not
create a file handle or mark product bytes uploaded.

### Dual verification and corruption

For every wanted hybrid piece, the scheduler requires both the exact v1
piece hash and an authenticated v2 expected piece root before issuing new
payload. Complete outer input supplies both; magnet input may wait for BEP 52
hash exchange through a direct or upgraded v2 peer.

After payload completion, the storage owner returns both computed results in
one generation-fenced outcome:

- both pass: sync through the existing durability barrier, then set one have;
- both fail: reset or diagnose through the existing bounded corruption path;
- exactly one passes: stop with `InconsistentHybridHashes`, preserve unrelated
  verified content, advertise no affected have, and do not fall back; or
- either expected value is missing: retain candidate state and request hash
  knowledge rather than treating the other result as authority.

Leaf diagnosis may retain v2-proved good blocks only within the current piece
generation. The subsequent reconstructed piece must still pass the v1 hash
and v2 root together. Contributor reputation records exact agreeing failures
and typed scheme disagreement without claiming that SHA-1 or SHA-256 alone
identified a malicious peer.

Force recheck, initial checking, candidate restart, active writes, and seed
restoration use the same dual transition. There is no alternate fast seed
path that trusts a v1 have bitmap or v2 catalog independently.

### Dual swarm and handshake routing

One torrent supervisor owns a fixed two-entry lane table:

```text
V1 lane -> V1InfoHash -> tracker/DHT/peer observations
V2 lane -> V2Truncated -> tracker/DHT/peer observations
                         shared peer/session budgets
                         one content/storage/integrity owner
```

Tracker and DHT operations carry an explicit `ProtocolVersion` through
request, response, retry, diagnostics, and cancellation. Shared URLs may
announce both identities independently. Returned peers enter the ordinary
bounded registry with source protocol; endpoint deduplication retains useful
version capability without cloning the registry or connection ceiling.

Handshake behavior is exact:

- a direct v1 dial sends the v1 hash and advertises the upgrade bit only for
  a validated hybrid whose v2 alias is installed;
- a direct v2 dial sends the tagged truncated v2 key and is v2 from the
  outset;
- an accepted v1 handshake with the bit may receive a v2-hash response only
  after the incoming router resolves that v1 alias to the same validated
  hybrid owner;
- an outgoing v1 handshake becomes v2 only when the response contains that
  owner's exact truncated v2 key; a v1 response remains v1; and
- once selected, a connection's protocol version is immutable and controls
  bitfield geometry, metadata identity, hash messages, and diagnostics.

The same rules apply after MSE identifies the owner and carries the peer
handshake. The MSE SKEY lookup remains version-tagged and may resolve either
installed alias. No heuristic treats an untagged 20-byte collision as both.

### Persistence, restart, upload, and publication

Persist one owner with both full identities, exact retained source/raw info,
format, selection, and existing candidate/have facts. Sparse v2 hash knowledge
remains non-durable under Tactical `155`; incomplete restart refetches or
reconstructs it and dual-checks candidate payload before advertising have.

Schema 19 was designed for multiple aliases and may remain if it expresses
the transaction safely. Because RSTorrent is unreleased, implementation may
replace the exact application-owned database and managed staging format with
a new version and clean reset instead of writing a preservation migration.
It must resolve targets first, report the reset, leave user-selected roots
and published content untouched, and prove clean restart. No compatibility-
only dual reader is required.

Active and completed registrations contain one content capability associated
with both aliases. Incoming routing resolves either alias to that capability.
Metadata service returns the same exact hybrid info; v2 connections may use
authenticated hash service; both versions may receive only dual-verified
payload. V1 padding requests synthesize zeros under the shared upload owner.

Publication, open, stream, Finished, completion announces, and seeding state
derive from real selected files and dual-verified have. A version-specific
announce may report completion only after the common product state is
complete; one lane's tracker failure does not make the other incomplete.

## Owner, Task, Cancellation, And Dependency Map

```text
runtime-free protocol
  ParsedInfo::Hybrid -> HybridContentDescriptor -> dual transition
  identities + aligned geometry + padding map + expected hash lookups
             no task, socket, filesystem, clock, or database
                                  |
                                  v
application transaction / SessionStore alias index
  provisional owners -> reserve both aliases -> one durable winner
                                  |
                                  v
one torrent generation / existing supervisor
    fixed V1/V2 lanes       integrity coordinator       storage owner
 tracker/DHT/peer routes    v1 + v2 catalog/state       read/hash/sync/pad
          |                         |                         |
          v                         v                         v
  peer generations ---- final protocol version ---- bounded I/O jobs
          |                         |                         |
          +----------- cancel + join + generation fence ----+
                                  |
                                  v
active/completed registration
 both aliases -> one metadata/hash/payload/publication capability
```

No second torrent supervisor, peer registry, storage namespace, integrity
coordinator, persistence actor, publication task, or platform protocol owner
is authorized. The lane table is task-free state; existing tracker, DHT, dial,
peer, storage, and upload owners perform versioned operations under their
current shared limits.

Metadata-time reconciliation is owned by the application/store transaction,
not by peer tasks. Peer tasks report exact metadata and handshake facts but
cannot attach aliases, choose a winning owner, merge discovery, or mutate
shared integrity truth.

Pause, selection replacement, force recheck, remove, application shutdown,
or generation replacement stops new metadata/discovery/hash/payload work,
cancels both lanes and every child, drains admitted storage results through
generation fences, joins children, and only then exposes terminal state. A
late valid metadata result, proof, announce, handshake, or hash outcome from
either lane remains stale.

## Security, Integrity, And Resource Invariants

- Full v1 and full v2 hashes are authoritative aliases. The truncated v2 key
  is tagged routing data only; equality with a v1 hash proves nothing.
- Both aliases derive from the same exact authenticated info bytes. Filenames,
  sizes, selection, tracker claims, peer hints, and payload cannot establish
  hybrid equivalence.
- Hybrid structural validation and alias reservation complete before content
  activation or payload scheduling. Two owners never share an alias.
- Only dual-verified pieces authorize have, reads, publication, or upload.
  One scheme passing cannot repair, suppress, or override the other.
- Internal padding is validated deterministic zero content. No peer can name
  a padding path, length, or offset that changes storage authority.
- The current 16-KiB/128-parameter magnet, 32 peer-hint, 32 tracker, 4,096
  select-range, 30-MiB BEP 9, 64-MiB outer source, 2,097,152-piece, file,
  path, depth, token, and checked-arithmetic limits remain in force.
- A hybrid owns exactly two fixed swarm-lane records. They share existing
  eight tracker-operation, peer-registry, dial, established-connection,
  request, payload, metadata, hash, storage-job, descriptor, upload, and
  bandwidth ceilings. Support does not multiply any ceiling by two.
- At most one alias-admission/reconciliation transaction is active per
  involved owner. It retains two owners, two full identities, and bounded
  source/discovery sets, never payload or full peer histories.
- Tactical `155` hash limits remain hard: two attempts per peer, 16 per
  torrent, two duplicates per logical range, two inbound requests per peer,
  eight hash-service jobs per torrent, one leaf-diagnosis piece, and an
  80-MiB maximum catalog profile.
- Dual checking uses one storage job and one 16-KiB read buffer at a time. It
  adds fixed SHA-1 state and the existing bounded Merkle frontier, not a
  second piece buffer or per-file task.
- A padding upload response is bounded by the ordinary 16-KiB peer request
  maximum and existing upload queue; zero synthesis cannot allocate the
  requested torrent piece or gap as one buffer.
- Discovery lane, reconciliation, proof, payload, persistence, and upload
  results are owner- and generation-fenced. Checked arithmetic precedes every
  identity, index, layer, count, offset, span, conversion, allocation, and
  mutation.
- Diagnostics expose bounded identities and typed counts already intended for
  product display, not raw metadata, proof hashes, payload, private paths, or
  hostile unbounded strings.

Stage 0 must measure maximum-shape hybrid parsing, padding maps, dual expected-
hash lookup/checking, two-lane discovery state, reconciliation retention, and
zero-span service before runtime composition. If the combined state exceeds
existing Android/session budgets, tighten admitted retained state or return a
typed resource error. Do not raise a ceiling or clone an owner silently.

## Required Observability

Extend structured state with:

- full v1/full v2 identities, admitted magnet identity shape, authenticated
  metadata hash results, and canonical source/export form;
- provisional owner, alias reservation, reconciliation started/winner/loser,
  joined children, merged discovery counts, conflict reason, and rollback;
- format and hybrid layout result, internal/final pad spans, accepted
  historical-tail shape, and padding bytes synthesized;
- per-version tracker/DHT operations, observations, dials, handshakes,
  reserved-bit offers, accepted upgrades, retained-v1 responses, wrong-hash
  failures, and final connection protocol;
- pieces waiting for v2 expected hashes, dual checks started/passed/failed,
  exact disagreement direction, candidate pieces, and dual-verified have;
- current/high-water lane records, reconciliation transactions, tracker/DHT
  operations, peer records, connections, hash attempts/catalog bytes,
  storage jobs, check buffers, padding jobs, and upload grants;
- restart source and whether have was restored through complete layers, peer
  refetch, local reconstruction, or full dual recheck; and
- terminal zero provisional losers, reconciliation work, discovery
  operations, dials, peer generations, hash/check/padding/storage jobs, seed
  registrations, and run-owned artifacts.

Reuse existing snapshots, protocol identities, Files/progress, tracker/DHT,
peer, disk/checker, error, and history surfaces. Do not add a second torrent
row, a Merkle UI, or a user-facing progress total that includes padding.

## Application And Platform Contract

The existing add commands admit strict hybrid outer bytes and the three
magnet identity shapes. Every path returns the survivor's opaque torrent ID.
If reconciliation retires a provisional ID, a command already waiting on the
loser receives a typed `Reconciled { torrent_id }` result; subsequent lookup
by either protocol identity resolves to the survivor. No permanent alias ID
is exposed as a second torrent.

The first owner retains destination, selection, queue position, pause state,
and user-facing source intent. Safe tracker and peer-hint union preserves
normal bounds and provenance. A later ordinary duplicate may apply the
existing explicit select-only promotion rule only after it already resolves
to the survivor; a losing provisional owner's implicit or explicit selection
does not alter the winner during reconciliation.

Source-aware export returns a verified verbatim dual-topic hybrid magnet when
possible and otherwise canonicalizes or synthesizes both topics. Complete
hybrid outer input can synthesize the same magnet. This tactical does not
synthesize a complete `.torrent` from acquired metadata or sparse hashes.

Files, progress, bytes wanted/downloaded/uploaded, Finished, Open, stream,
pause/resume, recheck, remove, and archive/restore keep existing semantics.
Padding is absent from Files and progress. Both identities are shown through
the existing technical identity contract; no hybrid toggle or choice of
preferred swarm is added.

Regenerate TypeScript, Kotlin, and Swift contracts only if a crossing Rust
type changes. Required platform evidence is:

- authenticated headless production-browser add/export, separate-magnet
  reconciliation, selective transfer, dual identity, restart, seeding, and
  exact removal with one application row;
- Tauri adapter tests and build without launching a visible product window;
- iOS generated-boundary tests and unsigned build/archive proportional to the
  unchanged in-process engine behavior;
- Android x86_64 and arm64-v8a native builds; and
- one owned API 34 no-window AVD run through the real application and SAF
  owner, covering hybrid magnet add, exact selected files, both swarm entry
  paths or a deterministic injected lane, dual verification, restart,
  publication/upload, padding exclusion, and cleanup.

No physical device is required unless a provider/lifecycle behavior changes
or the AVD leaves a material ambiguity. Android engine parity cannot be
replaced by host-only Rust evidence.

## Implementation Stages And Commit Gates

### Stage 0: Activation, source reconfirmation, and resource baseline

- Reconfirm BEP, libtorrent, and read-only JSTorrent revisions and paths.
- Record focused v1, pure-v2 source, and pure-v2 magnet baselines.
- Add maximum hybrid descriptor, padding, reconciliation, lane, and dual-
  checker resource tests before runtime work.
- Reconcile any source discovery that changes this decision record.

Gate: the tactical remains decision-complete, baselines pass, and every new
maximum fails before unbounded allocation or owner mutation.

### Stage 1: Hybrid content, storage geometry, and dual integrity

- Add the explicit hybrid content/integrity variant from validated Tactical
  `146` models without changing pure v1 or pure v2 variants.
- Compose v1 expected hashes, v2 catalog queries, aligned storage, zero spans,
  and one streamed dual-check outcome.
- Add disagreement, agreeing corruption, recheck, candidate, and padding
  service transitions with generation fencing.

Gate: runtime-free and storage tests cover every structural, padding, dual-
result, candidate, maximum-resource, and cancellation shape.

### Stage 2: Hybrid input, aliases, and reconciliation

- Admit strict complete outer hybrids and hybrid info-only metadata.
- Add dual-topic magnet parse/export and single-identity expansion.
- Implement atomic alias reservation and first-owner provisional
  reconciliation before content activation.
- Persist/resume both aliases and reset unreleased formats only if necessary.

Gate: deterministic store/application races prove winner stability, intent
retention, safe discovery union, conflict rollback, restart, stale results,
and no two durable owners for one alias.

### Stage 3: Two swarm lanes and hybrid handshake upgrade

- Version tracker/DHT operations and peer observations through one fixed lane
  table under shared budgets.
- Implement direct-v1, direct-v2, and v1-upgrade initiated/accepted
  handshakes across TCP, uTP, and carried MSE.
- Route final-protocol metadata, hash, payload, diagnostics, duplicate peers,
  and cancellation through existing owners.

Gate: scripted peers prove both lanes, offer/accept/decline/wrong-hash
upgrade, collisions, duplicate endpoints, tracker/DHT independence, MSE, uTP,
limits, and terminal zero work.

### Stage 4: Download, restart, publication, and seeding

- Require both expected values before payload and both results before have.
- Compose sparse hash acquisition, leaf recovery, padding, checking,
  publication, active reads, metadata/hash upload, and payload upload.
- Restore incomplete/complete state conservatively with both aliases and run
  both completion announces through one product state.

Gate: selective transfer, promotion, candidate restart, dual disagreement,
recheck, open/stream, both-version active/completed seeding, pause/remove, and
resource cleanup pass.

### Stage 5: Application and first-party platforms

- Carry one-row hybrid behavior, both identities, canonical export,
  reconciliation results, Files/progress truth, and typed errors through the
  ordinary application.
- Regenerate boundaries only where semantics cross them.
- Run production browser, Tauri, iOS build/boundary, both Android ABIs, and
  the owned API 34 application/SAF evidence.

Gate: no platform forks identity, reconciliation, padding, or integrity
policy, and every owned artifact cleans exactly.

### Stage 6: Controlled interoperability and closure

- Run the independent pinned-libtorrent role/swarm matrix below.
- Run maximum-resource profiles, full repository gates, and v1/pure-v2
  regressions.
- Reconcile tactical execution, owning topics, readiness, oracle checkpoint,
  and BEP ledger to exact evidence.

Gate: both engines exchange exact selected content through both hybrid swarm
identities, all subprocesses/artifacts terminate, and claims stop at hybrid
consumption/seeding rather than creation or all of BEP 52.

## Validation Matrix

### Deterministic protocol and state

- complete canonical and historical-tail hybrid positive fixtures;
- wrong raw paths, names, order, lengths, offsets, empty-file order, piece
  length, internal/final pads, `pieces`, roots, and layers;
- `btih` only, `btmh` only, dual topic, duplicate equal, conflict, malformed,
  pure-metadata mismatch, canonical order, source fidelity, and export;
- alias expansion/reservation/reconciliation success, opposite metadata
  completion orders, duplicate arrival, transaction failure, restart, remove,
  shutdown, non-provisional collision, truncated-key collision, and stale
  generation;
- hybrid descriptor lookup for one-block, one-piece, multi-piece, multifile,
  selected/skipped, empty, short-tail, internal-pad, and maximum geometry;
- SHA-1/SHA-256 pass/pass, fail/fail, pass/fail, fail/pass, missing expected,
  leaf retention, candidate, and recheck transitions;
- padding-only and crossing read plans, exact zero synthesis, short writes,
  backpressure, cancellation, and no pad artifact;
- fixed two-lane tracker/DHT state and exact handshake encoding/decoding for
  direct v1, direct v2, upgraded, declined, wrong-hash, non-hybrid, MSE, uTP,
  fragments, and coalesced bytes; and
- maximum resource accounting and generation-fenced release.

### Scripted runtime, storage, and failure

- two provisional magnets race across peers, timeout, disconnect, cancellation,
  winner activation, loser join, durable consolidation, and cleanup;
- each swarm lane independently succeeds, fails, retries, stalls, and returns
  duplicate endpoints without exceeding shared operation or peer ceilings;
- v1-entry offer accepted to v2, offer declined to v1, direct v2, incoming
  upgrade, wrong response, peer-ID duplicate, reconnect, and late handshake;
- new payload waits for both expected hashes; candidate payload promotes only
  after both checks; complete files reconstruct v2 knowledge then dual-check;
- consistent corruption recovers through whole-piece/leaf paths while both
  one-scheme disagreement directions stop without fallback;
- selective initial files, same-session promotion, restart before and after
  hashes, force recheck, active read, publication handoff, and completion;
- v1/v2 metadata, hash, active payload, completed payload, and padding upload
  through initiated and accepted peers; and
- pause, remove, shutdown, and generation replacement at every alias,
  announce, handshake, hash, dual-check, padding, sync, and publication phase
  with terminal zero counters.

### Controlled pinned-libtorrent interoperability

Use a temporary aligned multifile hybrid fixture with at least one selected
multi-piece file, one selected one-piece file, one skipped multi-piece file,
empty files, internal padding, and a short final tail. Compare each real file
independently; do not include padding in the product digest.

The minimum independent matrix is:

1. RSTorrent leecher adds only `btih` plus one `x.pe` to a pinned-libtorrent
   seed with tracker/DHT disabled. Wire evidence shows v1 handshake offer,
   v2 response upgrade, SHA-1 and SHA-256 metadata admission, required hash
   exchange before payload, dual verification, exact selection, and no
   skipped-file payload/hash requests.
2. RSTorrent leecher adds only `btmh` plus `x.pe` to the same oracle and uses
   direct v2 routing. Promote the skipped file, then restart once and prove
   both aliases, selection, sparse-hash refetch, dual have, and no unnecessary
   peer traffic after complete restart.
3. Race separate `btih`-only and `btmh`-only adds against delayed metadata
   peers. Prove the first owner survives both metadata completion orders,
   safe tracker/peer union, one row/runtime/storage owner, exact canonical
   export, and terminal loser cleanup.
4. Pinned libtorrent leecher uses the v1 identity against an RSTorrent seed,
   enters through an accepted hybrid-upgrade handshake, receives metadata,
   applicable v2 hashes and v1-compatible payload/padding, and verifies exact
   files.
5. A fresh pinned libtorrent leecher uses the v2 identity against the same
   RSTorrent content, proving direct-v2 incoming routing, metadata, hash, and
   payload service from the shared owner.
6. One controlled tracker session observes distinct v1 and v2 announces and
   introduces useful peers through both keys. One DHT-only session performs
   both versioned lookups/announces and completes without tracker or peer-hint
   masking.
7. One default-uTP case and one forced-RC4 TCP case cover opposite entry lanes
   or roles, with at least one exercising the hybrid upgrade rather than both
   repeating direct v2.

At least one run starts from complete outer metainfo and one from info-only
magnet metadata. Scripted companions inject each one-scheme disagreement,
inconsistent layout, wrong upgrade hash, stalled lane, and duplicate endpoint;
these attacks need not rely on libtorrent producing invalid data. Record wire
identities, final connection protocols, announce/lookup counts, expected-hash
ordering, dual check outcomes, payload digests, resource high-water marks,
and terminal cleanup. A public swarm is optional and cannot replace this
controlled matrix.

### Repository and platforms

Run in proportion to touched code, at minimum:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run generate --prefix clients/web
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
```

Also run focused interoperability, production browser E2E, Tauri adapter and
build, both Android native targets, the owned API 34 application/SAF scenario,
and applicable iOS generated-boundary/build commands from `DEVELOPMENT.md`.
Report exact commands and do not imply an unrun transport, platform, public-
network, or physical-device gate passed.

## Non-Goals And Deliberate Deferrals

- V2 or hybrid torrent creation, creator file ordering, piece-size selection,
  source hashing workflow, or first-party `.torrent` generation.
- Falling back to v1-only or v2-only operation after hybrid metadata or hash
  disagreement, choosing a preferred swarm, or a user setting for doing so.
- Merging owners after either has installed content authority, requested
  payload, opened storage, set candidate/have bits, or published files.
- Moving, renaming, deduplicating, overwriting, or deleting published payload
  or user-selected root content during reconciliation or reset.
- A preservation migration for unreleased databases/artifacts, durable sparse
  Merkle trees, a hash-cache artifact, or a compatibility-only dual reader.
- Synthesis/export of a complete outer `.torrent` from acquired metadata or
  sparse knowledge.
- Web seeds, hole punching, local discovery, new NAT policy, IPv6/uTP breadth,
  MSE-over-uTP, transport racing, or a performance-parity campaign.
- New choking, tit-for-tat, parole, general peer scoring, or long-term
  cross-swarm reputation policy.
- A new engine dependency, daemon, native host, IPC service, duplicate
  filesystem owner, or platform implementation of protocol/integrity policy.
- A hybrid/padding/Merkle UI, presentation redesign, new priority scale, or
  product choice between swarms.
- Public-swarm, remote-machine, physical-device, release, publish, tag, push,
  or migration preservation as a completion requirement.
- A claim for torrent creation, every historical hybrid variant, every
  optional Merkle base, durable sparse resume, or all of BEP 52.

After this tactical, creation remains a separate later capability. Tactical
[`153`](153-wired-lan-utp-data-plane-scalability.md) remains an unrelated
deferred performance frontier.

## Escalation Gates

Routine decisions inside this document are authorized once implementation
begins. Stop for maintainer direction if implementation would:

- change the first-owner reconciliation policy, import losing user intent,
  or merge an owner after the provisional metadata fence;
- accept an identity or layout mismatch, downgrade a hybrid, or authorize
  content after only one integrity scheme passes;
- delete, move, overwrite, or repurpose published payload or user-selected
  root content;
- widen the historical padding exception beyond one omitted final tail pad;
- raise any parser, alias, lane, tracker/DHT, peer, connection, request,
  payload, hash, catalog, storage, padding, task, descriptor, or upload limit;
- add durable sparse state or a preservation migration instead of an exact
  reset of RSTorrent-owned unreleased state;
- add a crypto, runtime, storage, engine, or platform dependency with
  meaningful tradeoffs;
- create a second torrent, discovery, peer, integrity, filesystem,
  persistence, publication, or platform protocol owner;
- move protocol, peer, hash, payload, or storage work across Kotlin, Swift,
  JavaScript, IPC, or daemon boundaries;
- weaken Android first-party engine parity; or
- require external publication, a public network, remote machine, or physical
  device side effect not already authorized.

A fresh fail-closed reset of exact RSTorrent-owned application database and
managed staging state is authorized if the current format cannot express the
atomic alias/reconciliation contract. Resolve and report exact targets first,
leave external and published payload untouched, and prove clean restart.

## Commit And Evidence Plan

Use logical commits that leave the repository buildable and preserve v1 and
pure v2:

1. activate Tactical `156`, reconfirm sources, and add resource baselines;
2. explicit hybrid content, padding map, and dual integrity transitions;
3. hybrid source and dual-topic magnet admission/export;
4. atomic alias expansion and provisional-owner reconciliation;
5. versioned tracker/DHT lanes and hybrid handshake upgrade;
6. dual-verified download, restart, publication, upload, and seeding;
7. application/generated clients and first-party platform composition;
8. controlled pinned-libtorrent both-swarm/both-role evidence; and
9. full regression, resource record, topic/ledger closure, and cleanup.

Each nontrivial implementation commit records its validation and includes
`Topic: bittorrent-v2-and-hybrid`. Evidence belongs in this document as it
lands. Temporary oracle torrents, payloads, profiles, captures, logs,
packages, AVD/simulator state, and subprocesses are removed before closure
unless one bounded artifact is explicitly retained and linked.

## Execution Record

### 2026-08-14: Stage 0 activation and source reconfirmation

- Reconfirmed the BEP checkout at
  `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`, pinned libtorrent 2.0.13 at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d`, and the read-only JSTorrent
  history checkout at `9895410beeed6aff554053769bd006a3fbd373ef`.
- Re-read the normative BEP 52 hybrid upgrade, BEP 4 reserved-bit, BEP 9
  mixed-topic, and BEP 47 padding requirements and the exact libtorrent
  source and tests named above. The source review did not change the accepted
  runtime, integrity, reconciliation, or compatibility decisions.
- `cargo test -p rstorrent-protocol`: 257 passed, 4 ignored, including the
  existing strict hybrid structure and maximum metainfo work/allocation
  bounds.
- `cargo test -p rstorrent-engine`: 564 passed, 9 ignored.
- `cargo test -p rstorrent-session`: 244 passed, 2 ignored.
- Maximum runtime descriptor, padding-map, dual-check, reconciliation, and
  lane accounting cases land with their owning stages so each bound is tested
  at the mutation boundary it protects.

### 2026-08-14: Hybrid content and one-pass dual integrity foundation

- Added an explicit runtime-free `TorrentContent::Hybrid` descriptor with
  exact full aliases, raw info, validated v1 and v2 geometry, logical payload
  files, and a bounded synthetic-zero padding map derived only after strict
  BEP 52 structural validation.
- Added hybrid expected-piece lookup and the pure `Verified`, `Invalid`, or
  `Inconsistent` dual-result classifier. The storage pipeline now treats a
  one-scheme pass as a typed terminal `InconsistentHybridHashes` failure and
  never installs have for it.
- Added one storage read path that feeds real bytes to SHA-1 and the v2 Merkle
  accumulator together, then feeds validated padding zeros only to SHA-1.
  Padding is not represented as a product file or storage artifact.
- Focused validation: the hybrid descriptor/padding/expectation protocol test,
  the hybrid single-read dual-hash storage test, all 13 storage-pipeline tests,
  and compile checks for `rstorrent-protocol`, `rstorrent-engine`, and
  `rstorrent-session` passed.

### 2026-08-14: Hybrid source and dual-topic magnet admission

- Complete outer hybrid sources now enter the ordinary content projection and
  info-only hybrid metadata enters the magnet/restart path after every known
  exact identity is checked. A single known topic may authenticate the second
  alias; a dual-topic source must match both.
- Magnet parsing retains the bounded full identity set, accepts only the
  existing exact `btih` and full `btmh:1220` shapes, and canonicalizes dual
  topics in v1-then-v2 order. New dual-topic owners reserve both unique full
  aliases atomically and export only a matching dual-topic retained source.
- Durable hybrid content uses the v2 logical file catalog while retaining the
  validated v1 padding/hash geometry. Incoming v2 hash service and complete
  local v2 reconstruction accept the hybrid catalog without weakening pure-v2
  validation.
- Focused dual-topic parser, canonicalization, alias reservation, schema-19,
  and pure-v2 restart tests passed; `cargo check --workspace` passed across
  Rust engine, session, Android, iOS, gateway, media, and desktop crates.

### 2026-08-14: Atomic provisional-owner reconciliation

- Metadata admission now serializes all authenticated aliases, requires every
  colliding owner to remain strictly pre-content, chooses the oldest durable
  owner (with a stable torrent-ID tie break), deletes only the later
  provisional row, and reserves both aliases for the survivor in the same
  transaction that installs metadata and empty have state.
- The winner keeps its storage root, selection/default intent, queue identity,
  and torrent ID. Only bounded trackers and peer hints are combined; losing
  selection and payload authority are never imported.
- A later owner that authenticates first commits metadata to the winner and
  receives a typed reconciliation stop before storage creation. The session
  application drains the resulting owner event, cancels/joins any losing
  runtime generation, unregisters incoming service, and removes the stale
  runtime handle.
- Deterministic tests cover both metadata completion orders, winner selection,
  stale loser rejection, exact one-row/two-alias state, winner selection
  retention, bounded discovery union, and the runtime cleanup compile path.
  `cargo check --workspace` remained green.

### 2026-08-14: Negotiated outgoing hybrid handshakes

- A v1 hybrid entry now advertises the BEP 52 reserved bit and accepts only
  the exact v1 identity (decline) or exact truncated v2 identity (upgrade) in
  the response. The returned identity selects the connection protocol before
  peer-wire decoding; an unrelated response remains a typed handshake error.
- The same selection is carried through plain TCP, default uTP, and both MSE
  payload modes. MSE continues to identify the torrent by the initiating v1
  hash while the encrypted BitTorrent response may select v2.
- Magnet metadata and content dials retain their selected entry key and offer
  the upgrade only from v1 once authenticated hybrid metadata supplies the v2
  alias. Direct v2 connections continue through the existing v2 decoder.
- `cargo test -p rstorrent-protocol` passed with 259 tests and 4 ignored.
  `cargo test -p rstorrent-engine` passed with 568 tests and 9 ignored,
  including deterministic v2 accept and v1 decline over plain TCP, v2 accept
  inside MSE, v2 accept over uTP, and wrong-response rejection.
