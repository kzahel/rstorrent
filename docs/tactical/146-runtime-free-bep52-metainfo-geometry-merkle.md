# Tactical 146: Runtime-Free BEP 52 Metainfo, Geometry, And Merkle Core

Status: **Planned and decision-complete; queued Next after Tactical
[`145`](145-sustained-utp-reliability-and-throughput-near-parity.md).** This
tactical is not active and does not displace Tactical `145` as the sole
authoritative **Now**. BEP 52 remains **Unsupported** before and after this
runtime-free slice.

Topics: `bittorrent-v2-and-hybrid`, `protocol-support`,
`capability-readiness`, `oracle-driven-engine-campaign`,
`code-organization-and-refactoring`

Dependencies: completed Tacticals
[`002`](002-selective-multi-file-storage.md),
[`074`](074-context-specific-metainfo-limits.md),
[`081`](081-v1-torrent-byte-intake.md), and
[`143`](143-dual-identity-and-persistence-foundation.md). Queue execution is
sequenced after active Tactical `145`, but there is no code dependency on the
uTP repair.

## Decision And Desired Outcome

Build the deterministic foundation shared by later pure-v2 and hybrid
runtime work without admitting v2 content into the product. The protocol
crate will be able to:

- hash the exact raw info dictionary as SHA-256 and retain the full v2
  identity alongside the existing exact SHA-1 identity where appropriate;
- parse and validate v2 and hybrid info dictionaries, file trees, roots, and
  complete outer piece layers under explicit hostile-input bounds;
- represent info-only metadata separately from a complete outer `.torrent`;
- derive format-aware v2 file and piece geometry without representing
  alignment gaps as payload files;
- construct and validate BEP 52 Merkle roots and proofs with bounded scratch
  state; and
- reject inconsistent hybrids rather than silently using either their v1 or
  v2 interpretation.

The current application, engine, session, magnet, storage, checking, and peer
paths remain v1-only. Existing product admission methods continue to reject
v2 and hybrid metainfo. A new explicitly named runtime-free parsing surface
is available to deterministic tests, the controlled oracle, and the next
tactical; its existence is not a support claim.

This is a format-model and integrity-primitive slice, not a SHA-1-to-SHA-256
substitution. It deliberately establishes the file alignment, hash-material,
and hybrid-consistency invariants before filesystem or peer tasks can depend
on them.

## Scope And Stopping Condition

This tactical owns:

1. a bounded two-phase direct-bencode scan that discovers and checks
   `meta version` before version-specific semantic interpretation while
   retaining exact raw info bytes;
2. runtime-free parsed-info and complete-metainfo models for v1, pure v2, and
   hybrid content, including explicit full `InfoHashes`;
3. canonical v2 file-tree traversal and projection through the existing
   hostile-path policy;
4. a format-aware, allocation-bounded v2 geometry model for per-file aligned
   logical pieces and non-payload gaps;
5. strict complete-source piece-layer validation while preserving a typed
   layer-unavailable state for authenticated info-only metadata;
6. streaming Merkle construction, zero-padding, shape arithmetic, and exact
   proof validation;
7. structural validation that the v1 and v2 halves of a hybrid describe one
   payload layout, including one tightly bounded historical tail-pad
   exception; and
8. independently authored deterministic vectors plus normalized comparison
   with the exact pinned libtorrent implementation.

The tactical stops only when all of the following are true:

- exact raw-byte v1, v2, and hybrid info identities agree with independent
  computation, including non-ASCII keys and values that cannot survive a
  decode/re-encode shortcut;
- version-first error precedence, canonical bencode rejection, file-tree
  traversal, file order, roots, piece layers, and hostile shapes pass fixed
  positive and negative tests;
- deterministic geometry covers empty, single-block, exact-piece,
  partial-piece, multi-file, large sparse logical-offset, and overflow
  boundaries without allocating per-piece state;
- independently authored Merkle vectors cover non-power-of-two leaf counts,
  short final blocks, every padding level, piece layers, and valid and invalid
  proofs;
- strict complete `.torrent` and layer-unavailable info-only policies cannot
  be confused at a call site;
- canonical hybrid and historical missing-final-tail-pad fixtures pass while
  every other file, order, length, offset, padding, root, or piece-length
  inconsistency fails closed;
- a bounded controlled comparer agrees with pinned libtorrent on normalized
  identities, files, roots, piece layers, and accepted compatibility shapes,
  with every intentional policy difference asserted explicitly;
- existing v1 parser, layout, product rejection, and repository tests remain
  green; both Android native ABIs compile against the unchanged generated
  boundary; and
- the tactical evidence, v2 topic, readiness queue, campaign checkpoint, and
  protocol ledger record that no runtime or product BEP 52 support has been
  added.

## Normative And Oracle Record

The design review used these exact sources before implementation:

- BEP repository commit
  `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`:
  `beps/bep_0052.rst` is normative for v2 metainfo, file trees, alignment,
  piece layers, Merkle hashing, hybrid consistency, and hash messages;
  `beps/bep_0003.rst` supplies the v1 info dictionary and exact info-hash
  context; `beps/bep_0047.rst` supplies v1 padding-file semantics; and
  `beps/bep_0009.rst` establishes that metadata exchange carries the info
  dictionary rather than outer piece layers.
- libtorrent commit
  `7d7fc38fac61177fa5e02148f791b2f65250b09d`:
  `src/torrent_info.cpp` functions `extract_single_file2`, `extract_files2`,
  `torrent_info::parse_info_section`, `torrent_info::parse_piece_layers`, and
  `parse_torrent_file`; `src/file_storage.cpp::files_compatible`;
  `include/libtorrent/aux_/merkle.hpp`; `src/merkle.cpp` functions including
  `merkle_num_leafs`, `merkle_num_nodes`, `merkle_num_layers`,
  `merkle_root`, `merkle_root_scratch`, `merkle_pad`, and proof validation;
  `src/merkle_tree.cpp`; and `src/create_torrent.cpp`.
- the same libtorrent pin's `test/test_torrent_info.cpp`,
  `test/test_create_torrent.cpp`, `test/test_merkle.cpp`, and
  `test/test_merkle_tree.cpp`, including the checked-in empty-file and hybrid
  missing-tail-pad cases.
- the local JSTorrent reference at commit
  `9895410beeed6aff554053769bd006a3fbd373ef`:
  `packages/engine/src/core/torrent-parser.ts` remains a flat v1 parser, and
  `packages/engine/src/core/peer-connection.ts` detects a 32-byte
  `info_hash2` extension value and disconnects rather than mixing it with v1
  piece semantics. The useful product lesson is to fail closed until the
  geometry and integrity owners are coherent, not to copy its model.

The local JSTorrent checkout had unrelated pre-existing documentation and
attachment changes during review. No conclusions depend on those files, and
this tactical neither modifies nor imports them. `scripts/references.py
status` should continue to report exact pins; its nonzero dirty-checkout
warning is recorded rather than hidden.

No reference source, fixture, or test data is copied. Tests are independently
authored from the public protocol behavior. Temporary torrents generated for
comparison are disposable local evidence and are removed after each run.

## Extracted Edge-Case Checklist

The specification and pinned implementation review front-loads these cases:

- `meta version` must be selected before interpreting version-specific
  fields; unknown future versions must not fall through to v1;
- v2 piece length is a power of two no smaller than 16 KiB;
- a file-tree leaf is the empty-string key beneath its path and cannot also
  be a branch; the root itself cannot be a file;
- nonempty files require an exact 32-byte pieces root, empty files have no
  payload tree, and an all-zero 32-byte digest is still a present digest;
- dictionary byte order defines deterministic file traversal; nested empty
  files, branch/leaf conflicts, excessive depth, excessive component count,
  and hostile local-path projection need explicit coverage;
- every nonempty v2 file begins at a logical piece boundary, its final piece
  may be short, and the gap to the next file is not payload;
- a complete outer metainfo needs the right piece-layer hash count for every
  file larger than one piece, no entry for an unknown root, and roots that
  reconstruct the file root;
- final 16-KiB data blocks hash their actual short byte length, while missing
  power-of-two leaves use recursively derived zero padding hashes;
- Merkle proofs must reject wrong indices, wrong tree levels, short or extra
  sibling paths, invalid padding claims, and a recomputed root mismatch
  without partially mutating trusted state;
- hybrid v1 files and v2 traversal must match raw paths, lengths, order, and
  piece-aligned offsets after accounting for BEP 47 padding;
- libtorrent accepts a historical hybrid whose sole difference is omission
  of the final tail pad, and accepts absent or incomplete outer piece layers;
  these behaviors require explicit local policy rather than accidental
  inheritance; and
- libtorrent treats an all-zero pieces root as missing through a sentinel
  convention, which is not suitable for RSTorrent's explicit optional-value
  model.

Mutable sparse-tree loading and merging cases in `test_merkle_tree.cpp` are
recorded for the later hash-exchange tactical. This tactical uses their proof
and arithmetic cases but does not introduce sparse runtime knowledge.

## Metainfo Parsing And Version Contract

### Two-phase bounded direct parsing

Refactor the current direct metainfo parser rather than adding a second
bencode implementation or decoding an unbounded generic tree:

1. perform one bounded lexical/structural scan of the exact info dictionary,
   enforcing canonical dictionary order, integer form, nesting, byte, token,
   and item limits while recording only the exact span and the scalar
   `meta version` fact;
2. choose `V1`, `V2`, or `HybridCandidate` from that fact and the presence of
   version-specific top-level keys; then run the corresponding bounded
   semantic projection over the same raw bytes; and
3. hash the original byte span directly. Never re-encode a parsed value to
   obtain either identity.

Lexical and canonical-bencode failures necessarily precede semantic version
selection because an untrusted dictionary first has to be delimited safely.
After that scan, an unsupported `meta version` takes precedence over
version-specific missing or malformed fields. Exactly integer `2` enables
v2 semantics. Missing `meta version` with v2-only fields is invalid; missing
it with a valid v1 shape remains v1. Negative, boolean-like, duplicate, or
noncanonical encodings are invalid, and a future integer version is a typed
unsupported-version result.

The parser retains current context-specific limits and source distinctions:

- peer BEP 9 info: at most 30 MiB, 2,500,000 decoded tokens, and depth 200;
- explicit/durable outer metainfo: at most 64 MiB and 3,000,000 decoded
  tokens; explicit direct parsing retains depth 100;
- at most 374,998 explicit files or 312,498 peer-metadata files;
- at most 3,000,000 path components in total, 240 bytes per projected
  component, and 4,096 bytes per projected path; and
- at most 2,097,152 logical pieces.

This tactical may make those limits more structurally explicit or tighter for
v2, but it must not raise them.

### Format and source models

Use explicit enums rather than optional fields whose combinations have to be
rediscovered by callers. The exact names may follow local naming, but the
model must distinguish:

```text
MetainfoFormat = V1 | V2 | Hybrid
ParsedInfo = exact_info_bytes + InfoHashes + format-specific validated info
HashMaterial = CompletePieceLayers | UnavailableFromInfoOnly
ParsedOuterMetainfo = ParsedInfo + CompletePieceLayers + outer source fields
```

`ParsedInfo` accepts valid v2 and hybrid info dictionaries from an explicitly
selected pure API and represents outer layers as unavailable. It does not say
that a complete `.torrent` was supplied and cannot be passed to a later
runtime as complete hash material by type confusion.

The explicit complete-outer v2/hybrid parser requires a `piece layers`
dictionary even when it is correctly empty because every file fits within
one piece. It requires every layer mandated by the file tree and rejects an
absent or incomplete dictionary. This deliberately differs from libtorrent's
permissive magnet-compatibility parsing: BEP 9 info-only metadata has its own
honest representation, so an explicit `.torrent` need not masquerade as one.

Existing product-facing `Metainfo` construction remains the v1 admission
gate. It either continues to project the existing v1 shape or uses an
explicit v1-only wrapper around the richer parser. It deterministically
rejects pure v2 and hybrid values after they are safely classified. No engine
or application call site migrates to the pure v2 parser in this tactical.

### Identity derivation

- v1 hashes SHA-1 over the exact raw info dictionary and produces only
  `V1InfoHash`;
- pure v2 hashes SHA-256 over those exact bytes and produces only
  `V2InfoHash`;
- a hybrid produces SHA-1 and SHA-256 over the same exact bytes only after
  both format halves and their structural compatibility validate; and
- an all-zero full digest remains a present value. Absence is represented
  only by `Option` or a format variant.

A failed hybrid is not downgraded to v1 or v2. Hash values may be computed for
diagnostics before full validation, but no authoritative `InfoHashes` escapes
from an invalid model.

## V2 File Tree And Path Policy

Traverse the canonical bencoded file tree iteratively with an explicit stack;
do not let attacker-controlled depth consume the Rust call stack. Raw byte
components are the protocol identity of a path and remain available for
hybrid comparison. Local display/storage projection reuses the existing
component validation, sanitization, collision detection, and path-length
policy rather than growing a v2-only alternative.

The tree contract is:

- the root is a dictionary of nonempty raw path-component keys and cannot
  contain the empty leaf key;
- a leaf is exactly the value under an empty byte-string key inside a path
  node; a node cannot contain both that leaf and child components;
- leaf `length` is a nonnegative bounded integer;
- every nonempty file has an exactly 32-byte `pieces root`; an empty file
  omits it, and a supplied root for an empty file is rejected as ambiguous
  metadata;
- symlink (`l`) attributes and v2 padding-file (`p`) attributes are rejected
  in the v2 tree. Hybrid padding is represented only by its v1 BEP 47 half;
- hidden and executable attributes remain bounded metadata and do not change
  geometry. This tactical adds no platform file-attribute behavior; and
- traversal output uses canonical raw-key byte order. Projection may not
  reorder files or make two raw protocol paths alias one local path.

The total-payload-zero case remains rejected because existing selection,
piece-state, and publication owners assume at least one payload byte. Empty
files around nonempty files are valid, retain their canonical file indices,
and consume no logical piece.

## Format-Aware V2 Geometry

Add a pure v2 geometry type alongside the current v1 `TorrentLayout`. It may
share checked arithmetic and path projection, but current v1 runtime code
must not accidentally receive v2 geometry through an unchanged flat-layout
interface.

For each canonical file record retain only bounded derived facts such as its
file index, payload length, starting logical piece, logical start byte, and
local piece count. Derive mappings arithmetically rather than allocating a
record for every piece:

- every nonempty file begins at the next global piece boundary;
- a zero-length file consumes no piece and does not advance the cursor;
- local piece count is `ceil(file_length / piece_length)`;
- every local piece except the last represents `piece_length` payload bytes;
  the last represents its exact remaining bytes;
- the next file's aligned logical start creates an explicit semantic gap when
  the preceding file ended short; and
- total payload bytes exclude gaps while logical peer offsets and piece
  indices include their alignment.

Expose checked mappings for global piece to file/local-piece/payload range and
for file byte ranges to logical pieces. A gap is observable for validation and
diagnostics but is never selectable, requestable, writable, verified,
published, uploaded, or counted as payload. Empty files and a final short
piece must not create phantom peer work.

Use checked `u64` arithmetic for lengths and offsets. Reject an offset, piece
count, block count, range end, or alignment calculation that overflows or
exceeds the configured bounds. The v2 piece length is a power of two from
16 KiB through 256 MiB, the largest power of two below the existing v1
maximum. Sum of per-file logical piece counts is at most 2,097,152.

## Piece Layers And Merkle Contract

### Complete outer piece layers

The outer `piece layers` dictionary is keyed by exact 32-byte file pieces
roots. Each value is a byte string containing consecutive 32-byte hashes at
the torrent piece level for a file larger than one piece.

Validation must:

- reject a key that is not 32 bytes, a value whose length is not divisible by
  32, an unknown root, an impossible duplicate canonical key, or aggregate
  allocation beyond the byte and hash-count limits;
- require exactly `ceil(file_length / piece_length)` hashes for each file
  larger than one piece and no required layer for a file of one piece or
  less;
- reconstruct the declared file root using those piece hashes and the BEP 52
  padding hashes, rejecting any mismatch;
- require every mandated root in a complete outer source and reject
  unexpected layer entries; and
- allow multiple files with the same root to share one validated retained
  layer rather than duplicating its resident bytes.

Retain one flat `Vec<[u8; 32]>` with per-root or per-file checked ranges, or an
equally compact representation. Across all layers it contains at most
2,097,152 hashes, approximately 64 MiB of digest bytes, independently bounded
by the existing outer-source byte cap. Do not retain the same hashes in both
flat and per-file vectors.

### Merkle arithmetic and construction

Keep the Merkle module runtime-free and SHA-256-specific:

- leaf hash: SHA-256 over one exact payload block of at most 16 KiB; the final
  present block hashes its actual shorter bytes;
- parent hash: SHA-256 over the exact 64-byte concatenation of left and right
  child hashes;
- absent leaves up to the next power-of-two shape use a literal 32-byte zero
  hash at the leaf level and recursively hashed zero pairs at higher levels;
- a piece root is the subtree root at the piece-size level, and a file root
  is built from its piece roots plus the required higher-level padding; and
- zero-valued computed digests are data, never absence sentinels.

Provide checked shape arithmetic, pair hashing, per-layer zero hashes, a
streaming root accumulator, roots from block or piece hashes, and exact proof
verification. API names may follow local conventions; correctness and bounded
state are the contract.

Do not allocate a full `2n - 1` node tree. Given the tactical's maximum of
2,097,152 pieces, maximum 256-MiB piece size, and fixed 16-KiB leaves, one file
can span at most `2^35` leaves. Root construction therefore needs no more than
36 retained hashes (1,152 digest bytes) plus small scalar state, and a proof
accepts at most 35 sibling hashes (1,120 digest bytes). Assert actual
high-water marks in tests.

Proof validation receives an explicit tree shape, subject index and level,
siblings, and expected root. It rejects out-of-range subjects, overflow,
wrong sibling count, early or extra proof termination, invalid claimed
padding, and root mismatch. Failure returns no partially trusted tree state.
Loading, merging, attributing, or persisting a mutable sparse Merkle tree is
deferred to the v2 magnet and hash-exchange tactical.

## Hybrid Structural Compatibility

A hybrid info dictionary must first be a valid v1 model and a valid v2 model
with the same torrent-wide piece length. Then compare raw protocol content,
not sanitized display paths:

- the ordered non-padding payload file sequence, including empty files, has
  identical raw path components and lengths;
- every v1 non-padding file offset equals the logical aligned start derived
  from the v2 tree;
- internal BEP 47 padding files exactly and only fill each alignment gap;
- no padding file is surfaced as v2 payload or changes payload-file indices;
  and
- both identity schemes remain attached to the one validated model.

Accept two final-tail shapes: no final padding file, or one final BEP 47 pad
whose exact length fills the remaining bytes of the last logical piece.
Pinned libtorrent accepts the former historical omission even when its
creator would emit the latter. No other missing, extra, duplicated,
mis-sized, reordered, or misplaced padding is compatible. Padding paths are
advisory, but the `p` attribute, length, position, and offset are not.

Reject a mismatch atomically and preserve enough typed diagnostic context to
name the category without logging hostile path bytes unsafely. Do not choose a
preferred half, repair the metadata, or create a v1-only fallback owner.

## Ownership, Dependencies, And Cancellation

| Concern | Owner | Runtime work | Cancellation |
| --- | --- | --- | --- |
| Exact bencode spans, version selection, file tree, piece layers | `rstorrent-protocol` metainfo modules | None | Not applicable |
| Format-aware geometry and checked mappings | `rstorrent-protocol` storage/layout module | None | Not applicable |
| Merkle shapes, roots, padding, and proofs | `rstorrent-protocol` integrity module | None | Not applicable |
| Product metainfo admission | Existing v1-only wrapper | None added | Not applicable |
| Controlled oracle generation/comparison | Bounded test/interop process | Test-owned only | Process exits and temporary directory is removed |

Protocol values and transitions remain independent from Tokio, sockets,
filesystems, task handles, channels, SQLite, platform adapters, and generated
application types. Dependency direction remains protocol to engine to
session/application, never back outward.

The protocol crate may use the already locked workspace `sha2` dependency
currently used elsewhere in the repository. This tactical adds no new
external package. A second bencode library, a libtorrent runtime dependency,
or a general Merkle-tree framework requires review.

The concrete refactor is the shared two-phase direct parser: canonical
lexical traversal, exact-span ownership, limits, and hostile path policy stay
common, while format-specific semantic projection becomes explicit. Avoid a
parallel v2 parser whose behavior can drift from v1 limits and canonicality.

No long-lived owner, task, queue, descriptor, file, socket, database row,
client command, or platform callback is added. Android parity is therefore
the same Rust protocol crate compiled for both supported native ABIs; an AVD
or physical-device run would add no behavior evidence and is not required.

## Resource Limits And Observability

The implementation must make these ceilings executable and test them at the
boundary:

| Resource | Limit |
| --- | ---: |
| Peer info bytes | 30 MiB |
| Explicit/durable outer bytes | 64 MiB |
| Explicit/peer files | 374,998 / 312,498 |
| Total path components | 3,000,000 |
| Component/projected path | 240 / 4,096 bytes |
| V2 piece length | power of two, 16 KiB through 256 MiB |
| Logical pieces / retained layer hashes | 2,097,152 |
| Merkle leaf size | 16 KiB |
| Streaming root scratch | at most 36 hashes / 1,152 digest bytes |
| Proof siblings | at most 35 hashes / 1,120 digest bytes |
| Geometry records | at most one per admitted file; no per-piece gap table |

Decoded token and depth limits remain context-specific as stated above.
Piece-layer value length, aggregate hash count, and outer-source bytes are
checked independently before allocation. All integer conversions and
multiplications are checked before state changes.

Pure APIs return structured errors containing safe scalar context: source
kind, format stage, field category, bounded file or piece index, expected and
actual length/count, and limit name. They do not log raw metainfo, untrusted
path bytes, hash material, or full proof contents. Tests record peak retained
piece-layer hashes and Merkle scratch/proof sizes. Product snapshot, command,
event, and metrics contracts do not change.

## Implementation Stages

### Stage 1: Reconfirm sources, inventory, and baseline

- verify the exact BEP and libtorrent pins and record any dirty-reference
  warning without modifying sibling checkouts;
- inventory every current `Metainfo`, `TorrentLayout`, identity, and v2
  rejection call site;
- run the protocol and workspace baseline before edits; and
- write the module/dependency and product-admission boundary as executable
  assertions where practical.

### Stage 2: Shared scan, format model, and exact identities

- extract the bounded two-phase direct scan;
- add explicit source/format models and SHA-256 exact-span identity;
- preserve the existing v1 `Metainfo` projection and v2/hybrid product
  rejection; and
- land version-precedence, canonicality, exact-byte, unsupported-version,
  and all-zero-value tests.

### Stage 3: Merkle primitives

- implement checked tree shape and layer arithmetic, zero hashes, pair hash,
  and streaming accumulation;
- add piece/file root and proof verification APIs; and
- pass independently derived fixed vectors and scratch/proof high-water
  assertions before file-tree parsing depends on the module.

### Stage 4: File tree and aligned geometry

- implement iterative canonical tree traversal and common path projection;
- add explicit empty-file, attribute, branch/leaf, collision, and bound tests;
- implement format-aware arithmetic geometry and gap representation; and
- exercise every block/piece/file boundary and overflow case.

### Stage 5: Piece layers and source completeness

- parse outer layers into one bounded retained representation;
- validate counts, roots, unknown/duplicate entries, and complete coverage;
- make info-only unavailable state and complete outer state type-distinct; and
- assert the intentional strict difference from pinned libtorrent.

### Stage 6: Hybrid compatibility

- compare the independently valid v1 and v2 models by raw ordered payload
  paths, lengths, aligned offsets, and exact BEP 47 gaps;
- cover empty-file placement and both accepted final-tail variants; and
- reject every other mismatch without identity or model fallback.

### Stage 7: Independent oracle and hostile/resource evidence

- generate bounded temporary single- and multi-file v2/hybrid torrents and
  payload hashes independently of the RSTorrent parser;
- compare normalized results with pinned Python libtorrent without importing
  its fixtures;
- run adversarial depth/count/length/overflow/layer/proof cases; and
- capture exact resident hash, scratch, proof, process, and temporary-disk
  high-water marks and clean all generated artifacts.

### Stage 8: Regression and closure

- run protocol, workspace, architecture, and v1 admission/rejection tests;
- build both Android native ABIs with no generated-contract change;
- update this tactical with commands, results, intentional differences, and
  commit evidence; and
- reconcile the v2 topic, protocol ledger, readiness queue, and campaign
  checkpoint without advancing the BEP 52 support claim.

Each stage may commit once its tests and documentation are coherent. Do not
leave a commit that routes v2/hybrid content into production admission before
the later runtime tactical.

## Validation Matrix

### Deterministic parser and identity

- exact raw info span under unusual but canonical byte strings;
- v1, pure v2, hybrid, missing/duplicate/negative/future `meta version`, and
  semantic error precedence;
- non-power-of-two, below-minimum, maximum, above-maximum, and overflowing
  piece lengths;
- valid and invalid tree leaves, root-as-file, branch/leaf conflicts,
  excessive depth/items/files/components, invalid local projections,
  projection collisions, empty files, missing/wrong-length/all-zero roots,
  and rejected attributes;
- layer absent from info-only, required empty dictionary for complete outer,
  missing/incomplete/extra/unknown layers, non-32-byte keys, nonmultiple
  values, count mismatch, root mismatch, and duplicate-root reuse; and
- current v1 inputs and every current explicit v2/hybrid rejection entry
  point.

### Geometry

- file lengths `0`, `1`, `16,383`, `16,384`, `16,385`, exactly one torrent
  piece, one byte over, multiple pieces, and the admitted maxima;
- piece lengths 16 KiB and larger powers of two through 256 MiB;
- empty files before, between, and after nonempty files;
- one and multiple alignment gaps, exact-boundary no-gap, final-short-piece,
  global-to-local and reverse range mapping; and
- logical-piece cap, checked alignment, offset, block-count, range-end, and
  total-payload overflow rejection.

### Merkle

- one, two, three, and other non-power-of-two leaf counts;
- short final leaves and padding at each applicable layer;
- piece roots at 16 KiB and larger piece sizes and file roots over multiple
  pieces;
- left/right proof paths, boundary indices, explicit zero-valued digest data,
  and independently computed expected roots; and
- short, extra, out-of-range, wrong-level, malformed-padding, and wrong-root
  proofs with no partial state change.

### Hybrid

- canonical single- and multi-file hybrids;
- empty files in each canonical position;
- exact internal BEP 47 pads, creator-style final tail pad, and the accepted
  missing-final-tail-pad historical form;
- path, ordering, length, offset, piece-length, missing/extra/mis-sized pad,
  v1 pieces, v2 root, and piece-layer mismatches; and
- all invalid cases produce no downgraded format or authoritative identity.

### Repository and platform

Run, at minimum:

```bash
source ~/.profile
python3 scripts/references.py status
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
experiments/android-engine-bootstrap/build.sh
```

Inspect the protocol crate's dependency direction and keep the runtime-free
boundary covered by the workspace tests. Confirm that generated JSON Schema,
TypeScript, Tauri, UniFFI, Kotlin, React, and Android application contracts
have no diff. No visible product client,
public-swarm run, WAN transfer, AVD, physical device, persistence reset, or
payload publication is proportional to this runtime-free slice.

## Non-Goals

- accepting v2 or hybrid `.torrent` files in the application, session, CLI,
  Tauri, web, Android, or any generated boundary;
- pure-v2 download, checking, storage planning, selection, part storage,
  have-state, fast resume, publication, streaming, upload, or seeding;
- `btmh` magnet intake/export, SHA-256 BEP 9 runtime validation, or missing
  outer-hash acquisition;
- BEP 52 peer messages 21--23, hash request scheduling, attribution, sparse
  mutable Merkle knowledge, or its persistence;
- v2 tracker, DHT, peer handshake, hybrid upgrade, MSE, uTP, or incoming
  routing behavior;
- simultaneous v1 and v2 swarm participation or dual-scheme payload
  verification;
- torrent creation, piece-size selection policy, source export, or importing
  reference fixtures;
- schema, artifact, storage-root, generated-client, web, Tauri, Compose, or
  Android runtime changes;
- increasing metainfo or resident-hash limits, optimizing unmeasured hot
  paths, or adding speculative generic codecs; and
- changing, expanding, or closing Tactical `142` or `145`.

The next vertical slice is a complete local pure-v2 `.torrent` download,
checking, restart, publication, and seeding tactical using already available
validated piece layers. It is not authorized by completing this document or
the runtime-free work.

## Escalation Gates

Stop for maintainer direction before:

- adding an external dependency beyond the already locked workspace `sha2`;
- increasing any byte, item, file, path, piece, layer, proof, or resident-hash
  limit;
- weakening complete-outer piece-layer validation or admitting incomplete
  outer sources as if they were complete;
- widening the historical hybrid exception beyond one omitted final tail
  pad;
- treating an all-zero digest as absence or silently falling back from an
  invalid hybrid;
- changing existing product v1 admission or routing v2/hybrid content into an
  engine, session, persistence, storage, client, or platform boundary;
- adding schema/artifact compatibility or deleting payload in response to
  this pure protocol change;
- adding v2 wire, discovery, magnet, runtime integrity, or torrent-creation
  scope; or
- discovering a normative/reference conflict that changes integrity,
  identity, file order, geometry, or interoperability policy.

Once activated, routine module naming, local extraction, error variants,
independently authored test data, oracle scripting, and tighter limits that do
not reduce required compatibility are authorized implementation choices.

## Commit And Evidence Plan

Prefer coherent stage commits with subjects under 65 characters. Each
nontrivial commit records the motivation, adopted source behavior,
intentional difference, validation, and deliberate deferral where useful, and
uses:

```text
Topic: bittorrent-v2-and-hybrid
```

The closing update records exact commit IDs, commands and outcomes, generated
fixture/process/disk cleanup, resource high-water marks, oracle agreement,
known gaps, and the next unstarted slice. Completion must still say plainly
that BEP 52 product and wire support are **Unsupported**.
