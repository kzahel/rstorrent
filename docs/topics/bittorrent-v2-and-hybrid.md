# BitTorrent v2 And Hybrid Torrents

Topic: `bittorrent-v2-and-hybrid`

Status: Research and campaign direction accepted on 2026-08-12. Completed
Tactical
[`143`](../tactical/143-dual-identity-and-persistence-foundation.md) installs
the v1-preserving opaque-owner, dual-identity, schema-19, artifact, runtime,
and first-party client foundation. Completed Tactical
[`146`](../tactical/146-runtime-free-bep52-metainfo-geometry-merkle.md) adds
the runtime-free exact parser, aligned geometry, Merkle core, strict complete
piece layers, and hybrid structural validator. Completed Tactical
[`151`](../tactical/151-complete-source-pure-v2-runtime-vertical.md) carries
strict complete local pure-v2 `.torrent` content through product intake,
aligned storage, Merkle verification, restart/recheck, publication, standard
peer transfer, discovery, streaming, and seeding. Completed Tactical
[`155`](../tactical/155-v2-magnet-authenticated-hash-exchange.md) adds the
bounded pure-v2 magnet vertical: exact `btmh`, SHA-256 BEP 9 metadata,
authenticated hash messages 21--23, sparse hash scheduling, candidate
recovery, selective payload, corruption repair, restart, and hash/payload
service. Completed Tactical
[`156`](../tactical/156-hybrid-dual-swarm-runtime-closure.md) adds the strict
hybrid consumption/seeding vertical: one owner and payload namespace, two
full aliases and discovery lanes, negotiated/direct v2 entry, mandatory dual
integrity, provisional-owner reconciliation, restart/recheck, upload, and
first-party product evidence. The BEP 52 claim remains **Partial** because
creation, arbitrary Merkle base layers, durable incomplete sparse-tree state,
broader historical hybrid layouts, and public-swarm reliability remain absent.

## Scope And Owning Role

This topic owns the continuing design direction for BitTorrent v2 and hybrid
torrents. It covers the changes that must remain coherent across identity,
metainfo, file geometry, integrity, peer wire behavior, discovery, storage,
persistence, seeding, and first-party platform composition.

It is an umbrella plan rather than one implementation tactical. Each future
slice must still name fixed scope, non-goals, invariants, resource limits,
owners, cancellation, exact source findings, validation, and a falsifiable
stopping condition before code changes begin. The readiness queue separately
controls when this campaign becomes active.

This topic does not authorize:

- replacing the first-party Rust engine with libtorrent or another engine;
- copying reference source or fixtures without the provenance review in
  [`../references.md`](../references.md);
- claiming BEP 52 support from parsing, codecs, or unit tests alone;
- implementing torrent creation in the initial download campaign;
- silently accepting inconsistent hybrid content;
- deleting user-selected or published payload files as an incidental schema
  reset; or
- changing the current engine campaign queue.

## Current Truth

RSTorrent has deliberately bounded pure-v2 and hybrid product/wire subsets
alongside the existing v1 behavior:

- Product-facing [`Metainfo`](../../crates/rstorrent-protocol/src/metainfo.rs)
  remains the v1 projection. `TorrentContent` is the runtime-owned
  v1/pure-v2/hybrid sum; pure v2 and hybrid may enter from a strict complete
  outer source or authenticated info-only metadata. A hybrid retains its
  validated v1 geometry, v2 logical file catalog, and synthetic padding map.
- [`magnet.rs`](../../crates/rstorrent-protocol/src/magnet.rs) accepts bounded
  `btih`, exact hexadecimal `btmh:1220`, or a matching dual-topic hybrid,
  canonicalizes topics v1 then v2, and fails closed on malformed, conflicting,
  or metadata-inconsistent identity.
- [`TorrentLayout`](../../crates/rstorrent-protocol/src/storage_layout.rs)
  remains the contiguous v1 layout. `ContentLayout` projects either that shape
  or v2's aligned file-local logical space through one checked runtime
  boundary.
- `V2TorrentLayout` models aligned nonempty files, ordered empty files,
  non-payload gaps, and checked global/file-local mappings without allocating
  per-piece state. Gaps are never payload, selection entries, part bytes, or
  uploadable ranges.
- The task-free SHA-256 Merkle owner builds block, piece, and file roots with
  at most 36 retained hashes, validates exact proofs with at most 35 siblings,
  and reconstructs strict complete outer piece layers. A separate bounded
  sparse catalog retains only roots and proof nodes authenticated to the file
  roots named by exact info; sparse knowledge is volatile across restart.
- The engine integrity plan explicitly selects v1 SHA-1 or streamed v2
  SHA-256 Merkle verification. V2 uses fixed 16-KiB leaves and distinguishes
  one-piece file roots from multi-piece layer roots before durable have state.
- Peer handshakes, trackers, DHT, MSE routing, incoming registration, and peer
  picking explicitly select a version-tagged `SwarmKey`; their 20-byte codec
  shape is no longer the authoritative torrent owner. A hybrid owns two fixed
  lanes beneath the same global budgets and can enter directly through v2 or
  through the authenticated BEP 52 v1-to-v2 upgrade.
- The engine and application use a canonical opaque `TorrentId`; schema 19
  stores full protocol aliases separately. Exact v2/hybrid outer source,
  selection, durable have state, path/SAF artifacts, publication, and
  versioned incoming routes survive restart. Tactical `156` required no
  migration or reset because that format already expressed the final shape.
- Trackers, DHT, plaintext handshakes, MSE routing, TCP, and uTP use the tagged
  20-byte v2 truncation while the full 32-byte identity remains authoritative.
  Standard bitfield/have/request/piece/cancel exchange serves active verified
  pieces and completed content. Negotiated v2 peers additionally exchange
  bounded request/hashes/reject messages 21--23; payload scheduling waits for
  an authenticated expected root, and upload serves required piece or leaf
  proofs under existing storage budgets.
- The [protocol ledger](protocol-support.md) reports BEP 52 as **Partial** for
  the demonstrated complete-source, pure-v2 magnet, and strict hybrid subsets.
  It does not claim creation, durable incomplete sparse-hash state, arbitrary
  Merkle-base behavior, broader historical hybrid layouts, or public-swarm
  reliability.

This is not a SHA-1-to-SHA-256 substitution. BEP 52 changes the identity
cardinality, file-to-piece geometry, expected-hash source, verification unit,
metadata acquisition lifecycle, peer-wire messages, and hybrid discovery
model at the same time.

## Accepted Direction

### Stable ownership and multiple protocol identities

One torrent owner may have a v1 SHA-1 identity, a full v2 SHA-256 identity, or
both. Tactical `143` introduced explicit types for those facts instead of
overloading `[u8; 20]`:

- the full 32-byte v2 hash remains the authoritative v2 identity;
- the first 20 bytes of the v2 hash are a version-tagged tracker, DHT, peer-
  handshake, and MSE routing key, never an authoritative database identity;
- a hybrid torrent retains both full protocol identities and may participate
  in both swarms through one content and storage owner; and
- no zero hash, length inference, or untagged 20-byte value may stand in for a
  missing identity.

The accepted persistence shape is one stable internal torrent key plus a
unique protocol-identity alias set. This avoids re-keying a live torrent when
metadata obtained from a v1-only or v2-only magnet reveals that it is hybrid.
Completed Tactical
[`143`](../tactical/143-dual-identity-and-persistence-foundation.md) selects a
nonzero opaque 16-byte owner rendered as `t1-` plus 32 lowercase hexadecimal
digits, independent from truncated wire identity. No two live torrent owners
may silently share one protocol identity; an alias collision is an explicit
conflict that must fail closed or enter a separately designed reconciliation
path.

### Resettable persistence during incubation

RSTorrent's public `0.1.x` artifacts are unsupported incubation builds.
Compatibility with their databases, have-state files, part-file headers,
torrent rows, and cached source records is not an implementation constraint
for this campaign.

Future v2 tacticals may replace those formats together, bump their versions,
and use a deliberate clean-state reset instead of carrying dual readers,
one-off SQLite migrations, or compatibility fields whose only purpose is to
preserve current development torrents. The replacement format must still be
versioned, bounded, validated, and fail explicitly when incompatible data is
encountered.

An executing tactical must resolve and document the exact application-owned
database and managed staging targets before resetting them, report what was
discarded, and prove restart into the new format. This decision does not
implicitly authorize deletion of user-selected roots or already published
payload files. Any payload deletion requires separately stated scope and
exact target checks.

### Metainfo and hash material are separate facts

The v2 info dictionary contains the file tree and per-file Merkle roots. A
complete `.torrent` normally carries `piece layers` in the outer dictionary,
outside the exact info bytes covered by the SHA-256 info hash. BEP 9 metadata
exchange supplies only the info dictionary. A v2 magnet can therefore obtain
authenticated roots before it possesses enough piece hashes to verify every
payload piece.

The data model must keep these facts distinct:

- exact raw info bytes and their v1 and/or v2 identities;
- validated file-tree roots;
- outer piece layers supplied by a `.torrent` source;
- sparse piece or leaf hashes learned through authenticated proofs; and
- missing hash material that is still needed before present payload can
  become verified.

Missing hash material is a normal runtime state for magnet acquisition, not a
reason to mark payload verified or redownload known-present bytes immediately.
A later compatibility decision must distinguish strict complete `.torrent`
validation from intentionally accepting a layer-incomplete source as partial
metadata. Whichever policy is selected, unproven hashes never authorize have
state.

### Format-aware file and piece geometry

BEP 52 maps each non-empty file to a piece boundary. A file's final piece may
be shorter than the torrent piece length, leaving an alignment gap before the
next file. Its Merkle tree is per-file, built from 16-KiB leaves, and padded to
a power-of-two tree with zero hashes beyond the file.

The current flat layout must become format-aware rather than pretending pure
v2 alignment gaps are ordinary payload files. The deterministic protocol
layer should own:

- canonical file-tree traversal and file ordering;
- file offsets in the logical peer piece space;
- synthetic alignment-gap segments that are neither user files nor writable
  payload;
- mapping between global peer piece indices and per-file pieces and blocks;
- empty-file and single-block-file geometry; and
- hybrid comparison against the v1 file list and BEP 47 padding layout.

Storage, selection, streaming, checking, and upload should consume that one
validated geometry instead of each deriving offsets independently.

### Merkle and integrity state

Merkle arithmetic, root construction, padding hashes, proof shape, and proof
validation belong in runtime-free protocol code. They must remain independent
from Tokio, sockets, filesystems, channels, and task handles.

The runtime integrity owner must be able to distinguish at least:

- absent payload;
- present payload awaiting trusted hash material;
- a computed block or piece hash whose proof is still unknown;
- v2-verified blocks and logical pieces;
- v1-verified pieces; and
- hybrid pieces that have satisfied both required hash schemes.

A hybrid torrent does not become authoritative because either scheme passes.
RSTorrent's initial policy is to reject or stop on inconsistent v1/v2
metadata or verification instead of silently falling back to one swarm. A
later fallback policy would require its own explicit integrity design.

### Wire, discovery, and metadata behavior

The existing tracker, DHT, peer-stream, incoming, and MSE owners remain in
place, but their operations become protocol-version aware:

- tracker and DHT operations use the version-tagged 20-byte key appropriate
  to that swarm;
- a hybrid torrent schedules and observes v1 and v2 announces independently
  without creating two content owners;
- peer handshake routing accepts the truncated v2 key only in a v2 context;
- the BEP 52 hybrid-upgrade reserved bit and response hash select the final
  protocol version of the connection;
- BEP 9 verifies acquired raw info with SHA-1, SHA-256, or both as required by
  the known identity set;
- peer messages 21, 22, and 23 carry bounded hash requests, hashes, and hash
  rejections; and
- metadata upload, payload upload, MSE request-hash routing, PEX provenance,
  and incoming admission use the selected protocol identity without
  duplicating torrent ownership.

Hash requests are allowed independently of choke state by the protocol, so
their admission, rate policy, retry, correlation, cancellation, and memory
accounting must be explicit rather than hidden inside the ordinary piece
request window.

## Recommended Owner And Dependency Shape

| Layer or owner | Responsibilities | Must not own |
| --- | --- | --- |
| `rstorrent-protocol` identity and metainfo | Tagged v1/v2 identities, exact info hashing, file tree, piece layers, hybrid structural validation | Sockets, files, tasks, persistence |
| `rstorrent-protocol` geometry and Merkle core | File/piece/block mapping, alignment gaps, padding hashes, proof validation, messages 21--23 | Retry policy, peer reputation, disk I/O |
| Session identity registry | One stable torrent owner, unique identity aliases, conflict detection, lookup by versioned wire key | Payload verification or peer-message parsing |
| Torrent integrity coordinator | Sparse per-file hash knowledge, hash request selection, proof correlation, verification readiness, retries and peer attribution | Raw socket reads or filesystem implementation |
| Peer connection generation | Encode/decode hash messages, negotiate protocol version, report correlated results, terminate its own requests | Shared Merkle truth or durable have state |
| Storage/checking owner | Positional reads/writes, 16-KiB SHA-256 block calculation, v1 SHA-1 calculation where required, durability and publication | Accepting unauthenticated expected hashes |
| Tracker and DHT owners | Per-version announce/lookup scheduling and observations using selected 20-byte keys | Treating truncation as full identity |
| Application/session persistence | Versioned identity aliases, source fidelity, have state, sparse Merkle state or explicit refetch policy | Compatibility with discarded pre-release formats |
| Platform adapters | Existing storage, lifecycle, generated-boundary, and Android evidence | Reimplementing protocol or integrity policy |

Each background hash acquisition or verification activity needs one owner,
bounded work, cancellation on torrent generation replacement, and observable
termination. A stale peer, storage, or proof callback cannot mutate a newer
torrent generation.

## Required Security And Resource Invariants

- Treat file-tree names, depths, counts, lengths, roots, piece layers, proof
  fields, and peer messages as hostile input before allocation or state
  mutation.
- Check `meta version` before validations that could misclassify a future
  version as malformed v2.
- Hash the exact encoded info substring; never derive identity by casually
  decode-and-reencoding unvalidated input.
- Bound outer bytes, info bytes, decoded items, recursion, files, path
  components, logical pieces, piece-layer bytes, retained tree nodes, and
  arithmetic before allocation.
- Validate each supplied piece layer against its named file root, expected
  tree layer, exact unpadded file-piece count, and zero-padding rules.
- Bound outstanding hash requests per peer and torrent, hashes and proof
  layers per message, retry cadence, rejected-request history, and retained
  peer attribution.
- Never mark a v2 piece present merely because bytes exist on disk. Trusted
  expected hash material and successful verification are both required.
- Never mark a hybrid piece present until every required integrity scheme has
  passed for the same logical data and current storage generation.
- Never route a full v2 identity solely by its 20-byte truncation when more
  than one candidate could match.
- Keep synthetic v2 alignment gaps non-writable and exclude them from
  selected payload byte accounting while retaining their logical piece-space
  effect.
- Preserve conservative restart when persisted Merkle or have state is
  absent, malformed, from another identity, or disagrees with current
  geometry.

Exact numeric limits belong in the first tactical that introduces the
corresponding allocation. Libtorrent's limits are evidence, not automatic
RSTorrent defaults.

## Normative Reference Dossier

The managed BEP checkout is pinned by
[`reference/pins.toml`](../../reference/pins.toml) at
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`.

- [BEP 52](https://www.bittorrent.org/beps/bep_0052.html), mirrored locally at
  `reference/bittorrent.org/beps/bep_0052.rst`, is the normative v2 source. It
  defines the SHA-256 info hash, 20-byte wire truncation, file tree, 16-KiB
  per-file Merkle leaves, piece layers, aligned logical piece space, messages
  21--23, dual-swarm hybrid layout, dual verification, and handshake upgrade.
- [BEP 9](https://www.bittorrent.org/beps/bep_0009.html), mirrored locally at
  `reference/bittorrent.org/beps/bep_0009.rst`, defines the full multihash-
  formatted `btmh` magnet identity and permits `btih` and `btmh` together for
  one hybrid torrent. Its info-only metadata exchange is why outer piece
  layers cannot be assumed present after magnet acquisition.
- [BEP 4](https://www.bittorrent.org/beps/bep_0004.html), mirrored locally at
  `reference/bittorrent.org/beps/bep_0004.rst`, records reserved-byte bit
  `0x10` for the hybrid legacy-to-v2 upgrade.
- [BEP 47](https://www.bittorrent.org/beps/bep_0047.html), mirrored locally at
  `reference/bittorrent.org/beps/bep_0047.rst`, owns the v1 padding-file
  representation required to make hybrid file alignment describe the same
  data.
- Existing BEP 3, 5, 10, 15, and 23 behavior remains relevant to base peer,
  DHT, extension, and tracker operation. BEP 52 changes the identity and
  geometry supplied to those owners rather than replacing all of them.

BEP 52 is marked Draft but is the normative wire and metainfo contract for
this campaign. Its document is placed in the public domain. RSTorrent will
independently summarize and test behavior rather than copy prose or the
example creator.

## Pinned libtorrent Source And Test Dossier

Rasterbar libtorrent `2.0.13` is pinned at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. It is the required completeness,
edge-case, and executable interoperability oracle, not an architecture
template or source donor.

The initial design review inspected these exact source areas:

- `include/libtorrent/info_hash.hpp`: `info_hash_t`, `has_v1()`, `has_v2()`,
  versioned 20-byte `get()`, and full dual identity comparison;
- `src/torrent_info.cpp`: `parse_info_section()`, `extract_files2()`,
  `parse_piece_layers()`, exact SHA-1/SHA-256 info hashing, hybrid file
  comparison, missing-layer compatibility, and piece-layer lifetime;
- `src/file_storage.cpp`: `files_compatible()` and tail-padding handling;
- `src/merkle.cpp`, `src/merkle_tree.cpp`, and their headers: tree geometry,
  sparse state, loading piece layers, obtaining proofs, and adding verified
  hashes;
- `src/hash_picker.cpp`: `validate_hash_request()`, `pick_hashes()`,
  `add_hashes()`, `set_block_hash()`, and `hashes_rejected()`;
- `src/bt_peer_connection.cpp`: hybrid reserved-bit negotiation,
  `on_hash_request()`, `on_hashes()`, `on_hash_reject()`, the three message
  writers, and bounded outstanding request dispatch;
- `src/torrent.cpp`: `initialize_merkle_trees()`, `set_metadata()`, checking,
  block verification, inconsistent dual hashes, sparse hash acquisition, and
  persisted tree extraction;
- `src/session_impl.cpp` and the torrent-list implementation: lookup,
  insertion, metadata-time identity expansion, and duplicate identity
  conflicts;
- `src/magnet_uri.cpp`: pure-v2 and hybrid magnet parsing and generation;
- `src/create_torrent.cpp`: canonical v2 ordering, padding, piece layers,
  file roots, and v1-only, v2-only, and hybrid creation; and
- `src/read_resume_data.cpp`: full identity, Merkle tree, and verified-leaf
  resume shapes.

The matching test review extracted these required case families:

- `test/test_torrent_info.cpp`: v2-only, multi-piece, multi-file, hybrid,
  sanitized filenames, absent and incomplete layers, invalid file trees,
  deep recursion, non-power-of-two pieces, mismatched hybrid metadata, bad
  alignment, unknown or malformed layers, invalid roots, large offsets,
  empty names, and round-trip hash retention;
- `test/test_create_torrent.cpp`: v2 and hybrid round trips, missing historical
  tail padding, path conflicts, empty files, piece layers, file roots,
  canonical ordering, single-file forms, and no-tail-padding variants;
- `test/test_merkle.cpp` and `test/test_merkle_tree.cpp`: full and sparse tree
  modes, partial proofs, piece and leaf layers, padded and unpadded tails,
  invalid proofs, invalid hashes, and known or unknown block transitions;
- `test/test_hash_picker.cpp`: piece-layer selection and retry, rejected
  requests, leaf and piece hashes, padded and unpadded requests, bad proofs,
  block and piece failures, request bounds, and pad, empty, and single-block
  files;
- `test/test_magnet.cpp`: `btmh`, malformed multihashes, hybrid URI identity,
  generation, info-hash projection, and v1/v2/hybrid resume round trips;
- `test/test_checking.cpp`: v2 complete, corrupt, incomplete, read-only,
  single-file, and force-recheck behavior;
- `test/test_read_resume.cpp`: dual identities, Merkle trees, and verified leaf
  hashes; and
- `test/test_tracker.cpp` and `test/test_torrent_list.cpp`: separate v1/v2
  tracker announces and identity alias/list behavior.

Future tacticals must re-open the exact files relevant to their slice and
record adopted behavior and deliberate differences. This topic's broad review
does not replace the tactical source dossier.

## Important Reference Differences

BEP 52 and libtorrent together are sufficient references only when their
roles remain distinct:

- The BEP says a complete v2 `.torrent` without required piece layers is
  invalid. Libtorrent deliberately accepts absent or incomplete layers like
  magnet metadata, marks the hashes unverified, and attempts to obtain what is
  missing. RSTorrent must select strict-import or explicit partial-metadata
  compatibility rather than inherit this silently.
- Libtorrent accepts historical hybrid torrents without one form of tail
  padding and removes that difference before comparing layouts. Whether
  RSTorrent accepts the same compatibility shape must be stated and tested.
- Libtorrent's hash picker supplies concrete request batching, retry cadence,
  sparse-tree storage, and implementation limits that the BEP does not
  prescribe. Those are useful failure and resource cases, not normative
  defaults.
- Libtorrent represents payload that exists before expected hashes arrive as
  finished-but-not-verified work. RSTorrent needs the same semantic state but
  should express it through its existing explicit owner and generation model,
  not copy libtorrent's picker architecture.
- When metadata reveals an identity conflict, libtorrent preserves its
  one-info-hash-to-one-torrent invariant and fails the conflict. RSTorrent
  likewise must not perform an unsafe live merge; any more permissive
  reconciliation requires a bounded design.

The specification supplies the contract, libtorrent source and tests supply
deployed edge cases and lifecycle evidence, independently authored tests
prevent implementation mirroring, and controlled two-role interoperability
proves the result. No Ubuntu or Debian torrent is required as a canonical
fixture. Libtorrent's v2-only and hybrid example torrents and its test corpus
are useful reconnaissance, while permanent copied fixtures require explicit
license and provenance review. Prefer generating small controlled torrents
with the pinned external oracle and verifying exact content independently.

JSTorrent is product and failure-history evidence rather than a v2
implementation oracle. Its current engine recognizes a truncated-v2 hybrid
connection and disconnects because its v1 piece model cannot safely continue;
it does not supply the required Merkle, storage, hash-exchange, or hybrid
runtime design.

## Tactical Campaign

All five consumption/seeding stages are complete. Adjacent future stages may
be combined only when the resulting scope remains bounded and its stopping
condition becomes clearer.

### 1. [Identity and resettable persistence foundation](../tactical/143-dual-identity-and-persistence-foundation.md)

Completed on 2026-08-13: typed v1/v2 identity, one stable torrent owner with
protocol aliases, versioned wire-key lookup, and the replacement
session/have/part-file/source format rebuild cleanly instead of migrating
current development torrents.

Preserve all existing v1 behavior and evidence while removing untagged
identity assumptions from shared boundaries. This stage stops when v1
metainfo, magnet, restart, incoming/outgoing transfer, trackers, DHT, MSE,
seeding, removal, and generated client contracts pass with the new identity
shape. It adds no v2 parser or support claim.

### 2. [Runtime-free BEP 52 metainfo, geometry, and Merkle core](../tactical/146-runtime-free-bep52-metainfo-geometry-merkle.md)

Completed on 2026-08-13: exact v2 info hashing, `meta version`, file-tree
parsing, piece layers,
per-file roots, aligned logical geometry, pure Merkle primitives and proofs,
and structural hybrid validation pass while preserving the distinction
between complete outer metainfo and info-only metadata.

Independently authored vectors and the applicable
pinned-libtorrent positive and negative corpus agree on accepted content,
rejected hostile shapes, identities, file order, logical piece mapping, roots,
piece layers, and proofs. Both Android ABIs compile with no generated contract
change. The stage owns no socket or filesystem task and makes no product
support claim.

### 3. [Pure-v2 `.torrent` download, checking, and seeding](../tactical/151-complete-source-pure-v2-runtime-vertical.md)

Completed on 2026-08-13. Strict complete local pure-v2 `.torrent` input now
passes through format-aware storage, streamed Merkle verification, durable
have, checking, publication, streaming eligibility, active upload, completed
seeding, and versioned discovery/peer routing. Exact single-file and aligned
multi-file transfers pass against pinned libtorrent in both roles, including
selective files, recovery, restart, TCP, default uTP, forced RC4 MSE, tracker,
DHT, browser, platform storage, Android AVD, iOS archive, and bounded-resource
evidence. V1 remains green and the resulting BEP 52 claim is intentionally
Partial; at that checkpoint v2 magnet hash acquisition was still absent and
was subsequently supplied by Stage 4.

### 4. [V2 magnets and authenticated hash exchange](../tactical/155-v2-magnet-authenticated-hash-exchange.md)

Completed on 2026-08-14. Exact `btmh` intake/export, SHA-256 BEP 9 metadata
validation, explicit missing-hash state, volatile sparse Merkle knowledge,
messages 21--23, bounded hash scheduling, proof validation, rejection/retry,
leaf-level repair, upload service, and peer attribution now pass.

The stopping condition passed with v2 magnet-to-content transfer in both
oracle roles, data-present-before-hash recovery, malformed and unsolicited
messages, bad proofs, rejected and stalled requests, reconnect/restart,
request and tree high-water marks, terminal cancellation, production browser,
Tauri/iOS build, both Android ABIs, and API 34 SAF evidence.

### 5. [Hybrid dual-swarm runtime closure](../tactical/156-hybrid-dual-swarm-runtime-closure.md)

Completed on 2026-08-15. Strict complete and info-only hybrid metadata now
produce one owner with simultaneous v1/v2 aliases, per-version tracker and DHT
participation, authenticated handshake upgrade, direct-v2 entry, shared
peer/storage ownership, atomic provisional reconciliation, BEP 47 layout
comparison, and mandatory dual verification.

Pinned libtorrent passes in both roles through both swarm entry paths with
selection promotion, restart, active and complete upload, forced-RC4 MSE,
default uTP, exact dual tracker/DHT keys, canonical and accepted historical
final-tail layouts, inconsistent-hash rejection, resource bounds, browser,
desktop, Android API 34 SAF, and iOS-build evidence. The protocol ledger may
therefore describe this exact hybrid-supported subset, but not creation or
unproven broader BEP 52 behavior.

### Separate later creation capability

Creating v2 and hybrid `.torrent` files is not required to download, verify,
resume, publish, or seed them. The controlled oracle can generate temporary
fixtures during the download campaign. First-party creation, canonical file
ordering policy, piece-size selection, and source export should receive a
separate tactical after interoperable consumption is complete.

## Evidence And Claim Gates

Every implementation tactical applies the source-first campaign ladder:

1. deterministic identity, metainfo, geometry, Merkle, and hostile-input
   transitions without networking or storage where possible;
2. scripted runtime failures for missing hashes, proof rejection, corruption,
   cancellation, restart, and storage replacement;
3. controlled pinned-libtorrent interoperability in both roles with exact
   generated payload hashes and captured protocol-version facts;
4. ordinary application composition, durable restart, seeding, selective
   files, publication, and supported transport regression;
5. Android boundary/build and proportional AVD or physical storage evidence in
   the same tactical when the capability is applicable; and
6. representative live evidence only after the controlled gates pass.

Record maximum accepted metainfo, piece-layer, sparse-tree, proof-message,
outstanding-request, resident-hash, task, descriptor, and storage high-water
marks. Temporary torrents, payloads, captures, logs, and oracle processes must
be bounded and cleaned.

Parsing a v2 file is not a support claim. The complete-source, magnet, and
hybrid subsets graduate only the exact paths their evidence proves. Tactical
`156` demonstrates one hybrid content owner participating through both
identities while verifying both schemes. [`protocol-support.md`](protocol-support.md)
owns the final claim language and evidence links.

## Resolved Decisions For Implementing Tacticals

Tactical `156` resolves separate-magnet reconciliation. Authenticated
metadata reserves both aliases before payload authority. The first-created
provisional owner survives; the later owner is cancelled and joined; only
bounded discovery facts combine; and destination, selection, queue position,
payload, candidate/have, and published state never live-merge. A collision
after the provisional fence fails closed.

These choices refine the accepted architecture. A decision that introduces a
new engine dependency, weakens fail-closed integrity, deletes published user
payload, or changes product identity semantics beyond this topic remains a
human review gate.

Tactical `146` resolves two earlier Stage 2 questions: an explicit complete
v2/hybrid `.torrent` requires complete validated piece layers, while BEP 9
info-only metadata has a distinct layer-unavailable representation; and
hybrid comparison accepts only the pinned-libtorrent historical omission of
the final tail pad, not missing internal padding.

Tactical `155` resolves the Stage 4 questions. Sparse authenticated hashes are
volatile in the first magnet vertical: incomplete restart refetches them,
while a complete file may reconstruct its tree locally and must revalidate the
authenticated file root. Candidate payload is never advertised before that
authority returns. Hash requests use the BEP 52 power-of-two shape with at
most 512 base hashes and 35 proofs, two attempts per peer, 16 per torrent, one
bounded leaf-diagnosis piece, and shared upload/storage limits. Its selected
TCP, default-uTP, MSE, tracker, DHT, initiated/accepted, web, Android, and
platform matrix is the proportional Stage 4 gate rather than a transport
cross-product.

## Queue And Next Work

[`capability-readiness.md`](capability-readiness.md) records measurement-only
Tactical [`153`](../tactical/153-wired-lan-utp-data-plane-scalability.md) as the
active priority at that historical checkpoint. Completed Tacticals
[`151`](../tactical/151-complete-source-pure-v2-runtime-vertical.md),
[`155`](../tactical/155-v2-magnet-authenticated-hash-exchange.md), and
[`156`](../tactical/156-hybrid-dual-swarm-runtime-closure.md) own the exact
complete-source, pure-v2 magnet, and strict hybrid consumption/seeding subsets.
Torrent creation remains a separately planned capability rather than implied
follow-on scope.
