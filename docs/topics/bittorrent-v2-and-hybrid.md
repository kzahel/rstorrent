# BitTorrent v2 And Hybrid Torrents

Topic: `bittorrent-v2-and-hybrid`

Status: Research and campaign direction accepted on 2026-08-12. Completed
Tactical
[`143`](../tactical/143-dual-identity-and-persistence-foundation.md) installs
the v1-preserving opaque-owner, dual-identity, schema-19, artifact, runtime,
and first-party client foundation. Completed Tactical
[`146`](../tactical/146-runtime-free-bep52-metainfo-geometry-merkle.md) adds
the runtime-free exact parser, aligned geometry, Merkle core, strict complete
piece layers, and hybrid structural validator. RSTorrent still rejects v2 and
hybrid metainfo and magnets at every product boundary. Tactical `145` remains
paused at its congestion-policy review gate.

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

RSTorrent's accepted product and wire model remains intentionally v1-specific:

- Product-facing [`Metainfo`](../../crates/rstorrent-protocol/src/metainfo.rs)
  still projects an explicit `V1InfoHash` and one flat vector of 20-byte SHA-1
  piece hashes, and explicitly rejects v2 and hybrid info dictionaries. The
  separate runtime-free `ParsedInfo`/`ParsedOuterMetainfo` APIs validate exact
  v1, v2, and hybrid info bytes without entering application admission.
- [`magnet.rs`](../../crates/rstorrent-protocol/src/magnet.rs) accepts bounded
  `btih` identity and explicitly rejects `btmh` or mixed v1/v2 identity.
- [`TorrentLayout`](../../crates/rstorrent-protocol/src/storage_layout.rs)
  assumes one contiguous v1 byte space where `piece_start` is the piece index
  multiplied by one torrent-wide piece length. Multi-file pieces may cross
  ordinary files and explicit BEP 47 padding files.
- `V2TorrentLayout` separately models aligned nonempty files, ordered empty
  files, non-payload gaps, and checked global/file-local mappings without
  allocating per-piece state. It is not accepted by the v1 runtime layout.
- The task-free SHA-256 Merkle owner builds block, piece, and file roots with
  at most 36 retained hashes, validates exact proofs with at most 35 siblings,
  and reconstructs strict complete outer piece layers.
- [`piece.rs`](../../crates/rstorrent-protocol/src/piece.rs) and the engine
  download driver verify a complete piece against one 20-byte expected hash.
  A failed generation resets the whole v1 piece.
- Peer handshakes, trackers, DHT, MSE routing, incoming registration, and peer
  picking explicitly select a version-tagged `SwarmKey`; their 20-byte codec
  shape is no longer the authoritative torrent owner.
- The engine and application use a canonical opaque `TorrentId`; schema 19
  stores full protocol aliases separately. `HaveState` version 2, part-file
  version 2, retained sources, path/SAF artifacts, routes, and generated
  clients bind to that opaque owner plus integrity context where required.
- The [protocol ledger](protocol-support.md) therefore reports BEP 52 as
  **Unsupported**. Safe rejection is useful evidence but is not partial v2
  support.

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

### Resettable persistence before release

RSTorrent is unreleased. Compatibility with existing development databases,
have-state files, part-file headers, torrent rows, and cached source records is
not an implementation constraint for this campaign.

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

The first two stages are complete. Readiness now selects planning the bounded
Stage 3 pure-v2 vertical tactical; later numbers remain unassigned until that
document is ready. Adjacent stages may be combined only when the resulting
scope remains bounded and its stopping condition becomes clearer.

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

### 3. Pure-v2 `.torrent` download, checking, and seeding

Thread v2 geometry and integrity through storage planning, block hashing,
piece completion, selective files, part storage, durable have, recheck,
publication, streaming eligibility, upload, and protocol-version-aware peer
routing. Begin with a complete local `.torrent` whose piece layers are already
available so missing-hash acquisition does not obscure the storage slice.

The stopping condition is exact controlled transfer against pinned libtorrent
in both seed and leecher roles, including multi-file alignment, selective
files, corruption, restart, incomplete storage, publication, bounded
resources, terminal owner cleanup, generated boundaries, and proportional
Android evidence. Tracker/DHT and supported peer transports receive
proportional regression evidence. The BEP 52 claim remains scoped because v2
magnet hash acquisition is still absent.

### 4. V2 magnets and authenticated hash exchange

Add full `btmh` intake/export, SHA-256 BEP 9 metadata validation, missing-hash
state, sparse Merkle persistence or explicit refetch policy, messages 21--23,
hash request scheduling, proof validation, rejection/retry behavior, upload
service, and peer attribution.

The stopping condition includes v2 magnet-to-content transfer in both oracle
roles, data-present-before-hash recovery, malformed and unsolicited messages,
bad proofs, rejected and stalled requests, reconnect/restart, request and tree
high-water marks, terminal cancellation, and proportional first-party client
and Android evidence.

### 5. Hybrid dual-swarm closure

Add simultaneous v1/v2 identity aliases, per-version tracker and DHT
participation, hybrid handshake upgrade, shared peer/storage ownership,
duplicate-add and metadata-time collision behavior, BEP 47 layout comparison,
and mandatory dual verification.

The stopping condition requires controlled pinned-libtorrent transfers through
both the v1 and v2 swarm identities, both initiated and accepted roles,
consistent and inconsistent hybrid metadata, historical padding compatibility
if selected, corruption that disagrees between schemes, restart, seeding,
MSE and default uTP routing where applicable, exact publication, resource
bounds, generated clients, and Android evidence. Only then may the protocol
ledger describe a hybrid-supported subset.

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

Parsing a v2 file is not a support claim. A pure-v2 `.torrent` transfer may
graduate only the exact local-source subset it proves. V2 magnet support
requires authenticated hash acquisition, and hybrid support requires one
content owner to participate through both identities while verifying both
schemes. [`protocol-support.md`](protocol-support.md) owns the final claim
language and evidence links.

## Open Decisions For Later Implementing Tacticals

- Select sparse Merkle persistence granularity versus bounded refetch after
  restart.
- Calibrate sparse-tree, hash-message, request, retry, and attribution limits
  against independent maximum profiles; Tactical `146` now owns the parser,
  complete-layer, geometry, scratch, and proof ceilings.
- Define exact duplicate-add behavior when separate v1 and v2 magnets later
  prove to be one hybrid torrent, without unsafe live-owner merging.
- Select the minimal proportional TCP, default-uTP, MSE, tracker, DHT,
  incoming, and Android matrix for each vertical slice.

These choices refine the accepted architecture. A decision that introduces a
new engine dependency, weakens fail-closed integrity, deletes published user
payload, or changes product identity semantics beyond this topic remains a
human review gate.

Tactical `146` resolves two earlier Stage 2 questions: an explicit complete
v2/hybrid `.torrent` requires complete validated piece layers, while BEP 9
info-only metadata has a distinct layer-unavailable representation; and
hybrid comparison accepts only the pinned-libtorrent historical omission of
the final tail pad, not missing internal padding.

## Queue And Next Work

[`capability-readiness.md`](capability-readiness.md) records planning the
bounded Stage 3 pure-v2 `.torrent` vertical slice as the sole **Now** after
completed Tactical
[`146`](../tactical/146-runtime-free-bep52-metainfo-geometry-merkle.md).
Tactical `145` remains paused at its existing review gate. Product input and
wire support remain v1-only and the protocol ledger remains **Unsupported**
for BEP 52.
