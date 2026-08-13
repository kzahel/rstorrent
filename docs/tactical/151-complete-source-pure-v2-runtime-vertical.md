# Tactical 151: Complete-Source Pure-v2 Runtime Vertical

Status: **Active authoritative Now on 2026-08-13.** This decision-complete
tactical resumes after iOS selected-root correctness Tactical `152` completed.

Topics: `bittorrent-v2-and-hybrid`, `protocol-support`,
`download-correctness`, `client-persistence`, `application-view-api`,
`client-surfaces`, `code-organization-and-refactoring`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tacticals
[`143`](143-dual-identity-and-persistence-foundation.md) and
[`146`](146-runtime-free-bep52-metainfo-geometry-merkle.md), plus the existing
v1 storage, checking, restart, publication, upload, discovery, generated-
boundary, and Android owners. Completed Tactical `150` was a queue dependency
rather than a code dependency.

## Decision And Desired Outcome

Admit one deliberately narrow BEP 52 product subset: a complete local
pure-v2 `.torrent` whose exact outer source supplies every required piece
layer. RSTorrent will download, verify, checkpoint, restart, recheck,
publish, stream verified selected files, and seed that content through the
ordinary application and engine owners.

This is the first BEP 52 runtime and product-support slice. It is not a
SHA-1-to-SHA-256 field substitution. The implementation must carry v2's
per-file aligned piece space and Merkle integrity through every owner that
currently assumes one flat v1 byte space and one 20-byte expected piece hash.

The accepted input and peer contract is:

- intake begins from exact complete outer `.torrent` bytes;
- the metainfo is pure v2, not v1 or hybrid, and passes Tactical `146`'s
  strict complete-source validation;
- the full 32-byte SHA-256 info hash is the authoritative protocol identity;
- trackers, DHT, peer handshakes, MSE routing, and incoming lookup use the
  explicitly tagged 20-byte v2 truncation;
- every participating controlled peer begins with the complete `.torrent`,
  so no peer hash acquisition is required; and
- payload exchange uses ordinary bitfield, have, request, piece, and cancel
  messages over v2 logical piece indices.

The existing v1 runtime remains behaviorally unchanged. Hybrid metainfo,
`btmh` magnets, SHA-256 BEP 9 acquisition, and BEP 52 hash messages 21--23
remain rejected or safely unsupported. Completion promotes only the exact
complete-local-pure-v2 subset to **Partial** in the protocol ledger.

## Scope And Stopping Condition

This tactical owns:

1. an owned, runtime-safe pure-v2 content descriptor constructed only from a
   validated `ParsedOuterMetainfo` with complete piece layers;
2. pure-v2 byte intake, duplicate detection, exact-source persistence,
   restart reconstruction, export fidelity, and versioned v2 identity
   registration through the application store;
3. one format-aware content geometry and integrity boundary consumed by
   scheduling, selective storage, streamed hashing, have state, progress,
   publication, verified active reads, and upload;
4. file-local v2 piece writes and reads in which alignment gaps are never
   payload, destinations, selection entries, part-file bytes, or uploadable
   ranges;
5. fixed-buffer SHA-256 block hashing and Merkle verification against complete
   piece layers or the correct one-piece file-root shape;
6. durable have, conservative structural resume, full recheck, corruption
   invalidation, incomplete storage recovery, namespace transition, and
   exact selected-file publication;
7. outgoing and incoming pure-v2 peer routing, standard payload transfer,
   active verified-piece upload, completed seeding, tracker and DHT routing,
   and proportional TCP, uTP, and MSE evidence;
8. ordinary add-bytes, file-selection, force-recheck, remove, export,
   progress, Files, streaming-eligibility, and seed behavior across the
   application contract;
9. generated TypeScript, Kotlin, and Swift boundary consistency plus
   proportional headless web, desktop build, iOS build, Android ABI, and API
   34 AVD evidence; and
10. controlled pinned-libtorrent interoperability with RSTorrent in both
    leecher and seed roles, exact payload comparison, bounded resources, and
    terminal cleanup.

The tactical stops only when all of the following are true:

- complete pure-v2 input is accepted through the ordinary byte operation,
  while hybrid, v2 info-only, layer-incomplete, and `btmh` inputs remain
  outside this slice;
- single-file and aligned multi-file downloads complete against pinned
  libtorrent with RSTorrent in both roles;
- empty files, sub-block files, exact-block files, exact-piece files,
  multi-piece files, short file-final pieces, internal alignment gaps, and
  selected/skipped files produce exact output without gap artifacts;
- no piece becomes have, streamable, publishable, or uploadable before the
  right SHA-256 Merkle result and storage durability transition succeed;
- clean restart, interrupted storage, stale have state, missing and truncated
  files, same-length corruption under force recheck, read-only complete
  content, and publication interruption have explicit passing outcomes;
- active verified pieces and completed content are served correctly before
  and after application restart through versioned incoming and outgoing
  routes;
- supported tracker, DHT, TCP, default-uTP, and MSE paths receive the bounded
  regression matrix below without constructing a cross-product campaign;
- v1 deterministic, runtime, interoperability, persistence, client, and
  platform behavior remains green;
- both Android native ABIs compile and one owned API 34 no-window AVD proves
  pure-v2 intake, aligned selective storage, restart/recheck, publication,
  and proportional upload behavior through the real application; and
- owning topics, the readiness queue, protocol claim, correctness ledger,
  tactical evidence, and resource high-water marks describe only what was
  actually proved.

## Normative And Source-Oracle Record

The implementation must reconfirm the exact managed revisions before changing
code. This tactical's design review used the pins already recorded in
[`reference/pins.toml`](../../reference/pins.toml).

### Normative specifications

The BEP checkout is pinned at
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`.

- `reference/bittorrent.org/beps/bep_0052.rst` is normative for the exact
  SHA-256 info hash, 20-byte wire truncation, complete outer piece layers,
  per-file 16-KiB Merkle leaves, zero padding, file-aligned logical pieces,
  tracker requests, peer piece indices, short file-final requests, and
  messages 21--23. The last message family is studied but deferred here.
- `reference/bittorrent.org/beps/bep_0003.rst` supplies the base handshake,
  bitfield, have, request, piece, cancel, choking, and ordinary peer lifecycle
  reused by the v2 connection.
- `reference/bittorrent.org/beps/bep_0005.rst` supplies the 20-byte DHT key
  contract. The value is the tagged v2 truncation for this slice.
- `reference/bittorrent.org/beps/bep_0006.rst`, `bep_0010.rst`,
  `bep_0011.rst`, `bep_0015.rst`, `bep_0023.rst`, and `bep_0029.rst` remain
  applicable to already-supported peer extensions, compact values, tracker,
  and uTP behavior. They receive regression evidence rather than new breadth.
- `reference/bittorrent.org/beps/bep_0009.rst` explains why info-only metadata
  lacks outer piece layers. V2 BEP 9 and magnet intake are therefore explicit
  non-goals rather than accidental partial support.
- `reference/bittorrent.org/beps/bep_0004.rst` and `bep_0047.rst` are reviewed
  only to keep hybrid negotiation and v1 padding out of this pure-v2 slice.

### Pinned libtorrent source

Rasterbar libtorrent `2.0.13` is pinned at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. The design review inspected these
runtime areas in addition to Tactical `146`'s parser/Merkle dossier:

- `src/torrent.cpp::{initialize_merkle_trees,on_piece_verified,piece_passed,
  piece_failed,files_checked,force_recheck,set_metadata}` for loading complete
  layers, SHA-1/v2 result separation, piece authority, checking, and the
  distinction between complete outer metainfo and info-only metadata;
- `src/bt_peer_connection.cpp` handshake attachment and
  `protocol_v2` selection, plus `on_hash_request`, `on_hashes`,
  `on_hash_reject`, and their writers to identify the exact deferred hash-
  exchange boundary;
- `src/file_storage.cpp::{map_block,map_file,file_offset,files_compatible}` and
  its piece/file Merkle geometry helpers for aligned file-local requests and
  short file-final pieces;
- `src/storage_utils.cpp` for bounded file-span walking without assuming that
  every logical offset is writable payload;
- `src/hash_picker.cpp` for the distinction between trusted piece-layer
  hashes and sparse hash acquisition; only the complete-layer side applies;
- `src/piece_picker.cpp` and `src/torrent.cpp` for have/priority/verification
  transitions without adopting their C++ ownership graph;
- `src/mmap_storage.cpp` and `src/part_file.cpp` for skipped-file and lazy part
  behavior; v2's file-local pieces deliberately make ordinary selected/
  skipped boundary slots unnecessary in RSTorrent;
- `src/read_resume_data.cpp` and `src/write_resume_data.cpp` for v2 identity,
  have, Merkle, and restart case inventory, not as a persistence format to
  copy; and
- `simulation/transfer_sim.hpp`, `simulation/disk_io.cpp`, and
  `simulation/transfer_sim.cpp` for independently driven v2-only transfer
  fixtures and exact seed completion.

### Pinned libtorrent tests

The edge-case and interoperability inventory includes:

- `test/test_checking.cpp::{checking_v2,read_only_corrupt_v2,
  read_only_v2,incomplete_v2,corrupt_v2,single_file_v2,
  single_file_corrupt_v2,single_file_incomplete_v2,force_recheck_v2}`;
- `simulation/test_transfer.cpp` and `simulation/transfer_sim.hpp` for
  v2-only transfer, piece-layer retention, seeding, force recheck, empty
  files, small files, and varying aligned/pad-like shapes;
- `test/test_torrent_info.cpp` for valid complete layers, invalid or absent
  layers, empty files, large offsets, file order, and full v2 identity;
- `test/test_create_torrent.cpp` only to generate and inspect temporary
  v2-only oracle inputs with exact ordering, roots, and layers;
- `test/test_storage.cpp` for short I/O, missing/oversized files, zero-length
  files, fast-resume rejection, and read/write failures;
- `test/test_read_resume.cpp::{read_resume_info_hash2,mismatching_v2_hash,
  round_trip_have_pieces,round_trip_verified_pieces,
  round_trip_merkle_trees,v2_pieces_field,v2_trees_fields}`; and
- `test/test_hash_picker.cpp`, `test/test_merkle.cpp`, and
  `test/test_merkle_tree.cpp` for one-piece files, padded final v2 pieces,
  invalid roots, and the sparse/hash-message cases intentionally left to
  Stage 4.

No libtorrent source or checked-in fixture is copied. The controlled harness
generates temporary pure-v2 torrents with the pinned oracle, records a
normalized manifest and independent payload digests, and removes the source,
payload, profile, capture, and oracle processes it owns.

### JSTorrent product history

The local JSTorrent reference remains a product and storage-failure oracle,
not a v2 protocol implementation:

- `packages/engine/src/core/torrent-parser.ts` is v1-only;
- `packages/engine/src/core/peer-connection.ts` detects a v2 hybrid upgrade
  value but disconnects rather than applying v1 piece semantics to it;
- `packages/engine/src/core/torrent.ts`,
  `torrent-content-storage.ts`, and `parts-file.ts` reinforce lazy part
  creation, durable selection, materialization, and fail-closed restart; and
- its useful lesson is to keep product behavior ordinary while refusing to
  mix identity, geometry, or integrity modes that the runtime cannot prove.

No JSTorrent source or fixture is imported by this tactical.

## Extracted Shape-Changing Edge Cases

The following cases must be represented before the common transfer path is
considered architectural evidence:

- a file shorter than 16 KiB uses its actual final block bytes and pads only
  to that file tree's next power-of-two leaf count;
- a nonempty file no larger than one torrent piece has no outer piece-layer
  entry, so its declared `pieces root` is the expected verification root;
- a file larger than one piece takes an expected root from the validated
  piece layer for every file-local piece, including a padded short final
  piece;
- the short final piece of any file may occur before later global piece
  indices; only the final piece of the whole torrent is not a sufficient
  special case;
- every nonempty file starts on a piece boundary, while ordered empty files
  own no piece and must not disturb the next nonempty file's mapping;
- the logical gap after a short file is not a BEP 47 padding file and cannot
  be selected, requested, stored, hashed as received bytes, reported as
  payload, streamed, published, or uploaded;
- a v2 piece never overlaps two real files. An ordinary skipped v2 file owns
  only skipped pieces, so its absence must not create a part file merely to
  preserve the v1 storage shape;
- lowering a file priority after a destination exists retains that
  destination; raising a missing file starts conservative recheck/download;
  no selection transition manufactures have state;
- selected empty files still need exact publication semantics even though
  they contribute no requested or verified piece;
- a peer request may end at a file boundary but may not extend into the
  following alignment gap or file;
- bitfield spare bits, out-of-range have values, oversized blocks, duplicate
  payload, stale generations, and late blocks retain the existing hostile-
  peer behavior under v2 piece counts;
- missing, malformed, wrong-root, or incomplete outer piece layers fail
  before storage or network work begins;
- exact outer source, exact raw info, stored full v2 identity, content
  fingerprint, piece count, have state, and current geometry must agree on
  restart;
- structural fast resume remains historical trust, not a fresh hash claim;
  force recheck must find same-length payload corruption;
- read-only exact content may become verified and seed-ready, while corrupt
  read-only content cannot be repaired silently;
- cancellation during write, hash, durability, recheck, publication, or
  upload cannot let a stale completion mutate the next torrent generation;
  and
- v1 and v2 torrents whose 20-byte wire values collide remain distinct
  versioned registry keys and cannot alias one another.

## Accepted Runtime And Module Shape

### Owned content descriptor

Add one runtime-free owned content descriptor at the protocol/engine boundary
rather than widening the v1 `Metainfo` with optional incompatible fields. The
exact local names may follow the modules, but call sites must see an explicit
sum such as:

```text
TorrentContent
  V1 {
    metainfo: existing Metainfo,
    layout: TorrentLayout,
    expected: SHA-1 piece hashes
  }
  V2 {
    info_hashes: v2-only InfoHashes,
    metainfo: owned V2Metainfo,
    layout: V2TorrentLayout,
    piece_layers: CompletePieceLayers,
    trackers: bounded outer tracker projection
  }
```

Construct the v2 variant only by consuming or independently owning the
validated fields from `ParsedOuterMetainfo`. A borrowed parser view must not
be held across async tasks, and callers must not be able to construct
`Complete` v2 content from info-only `ParsedInfo`.

The descriptor owns deterministic facts only. It contains no Tokio handle,
socket, path, descriptor, task, channel, or retry policy. `rstorrent-protocol`
continues to own identities, canonical file metadata, geometry, expected hash
lookup, and Merkle arithmetic; engine and session layers depend inward on it.

Do not add a generic hashing or filesystem trait. Use an explicit integrity
enum or format branch where the real v1 SHA-1 and v2 Merkle implementations
differ. Extract a smaller private storage-plan module from
`selective_storage.rs` only if the format-aware mapping gives it an
independent deterministic invariant and test seam.

### One geometry authority

All consumers receive a checked format-aware view that supplies at least:

- real file count, raw and projected paths, lengths, and selection eligibility;
- logical piece count and torrent piece length;
- actual payload length for every global piece index;
- global piece to file index, file-local piece, file offset, and logical
  offset mapping;
- request validation within that actual piece payload;
- selected, skipped, and total real payload byte accounting;
- publication and active-read spans; and
- a format-tagged content/layout fingerprint for artifact and restart checks.

For v1, this delegates to the existing `TorrentLayout`. For v2, it delegates
to `V2TorrentLayout` and the canonical `V2Metainfo` file list. Alignment gaps
may remain observable deterministic geometry, but no storage API accepts a
gap as a write or read target.

Scheduling, progress, storage, checker, HTTP streaming, publication, and
upload must not independently recompute offsets from `piece_index *
piece_length` when that would lose a v2 short internal piece.

### Expected integrity plan

Expected v2 verification is a per-piece value carrying enough shape to avoid
two common errors:

- for a file larger than one piece, select the file-local hash from its
  complete validated piece layer and hash the piece's 16-KiB leaves padded to
  the torrent piece subtree height; and
- for a file no larger than one piece, use the file's declared root and hash
  only to the file tree's own next-power-of-two height, not automatically to
  the full torrent piece height.

The storage hash job reads at most one 16-KiB chunk at a time and streams leaf
hashes into Tactical `146`'s bounded `MerkleAccumulator`. It returns a typed
SHA-256 result and I/O statistics to the existing generation-fenced storage
pipeline. The torrent coordinator compares that result with the immutable
expected plan before `piece_verified`, have, checkpoint, stream eligibility,
or upload authority changes.

A failed v2 piece resets the same complete logical piece and applies the
existing exact-generation contributor attribution and retry policy. This
slice does not claim leaf-level repair because it does not acquire trusted
leaf hashes from peers.

## Storage, Selection, Checking, And Publication

### Positional storage

Extend `SelectiveStorage` and its immutable write, hash, active-read, upload-
read, observation, sync, and publication plans to consume the format-aware
geometry. V2 writes are always file-local and use the actual piece payload
length. No plan may synthesize a padding span for a v2 alignment gap.

The existing v1 path continues to synthesize BEP 47 zeroes where applicable.
The explicit distinction is:

```text
v1 padding segment       -> verification zeroes, never a payload file
v2 alignment gap         -> no piece payload and no storage span at all
v2 Merkle zero padding   -> deterministic hash value, never file bytes
```

These three concepts must not share an untagged `Padding` case.

### Selection and part storage

File priority remains binary `Normal` or `Skip`. A v2 piece is wanted exactly
when its one owning nonempty file is wanted. No v2 piece crosses a selected/
skipped boundary.

Therefore the initial v2 policy is:

- skipped-only v2 pieces are not requested;
- a fresh or resumed pure-v2 selection does not create a part artifact;
- demoting an existing destination retains its bytes and route, matching the
  existing product policy;
- promoting an absent file rechecks any current source and downloads missing
  pieces; and
- active unverified work is cancelled and joined through the existing whole-
  generation selection fence before routes change.

Do not invent a v2 part-file representation without a real write route. The
existing lazy v1 part-file format stays v1-owned. If implementation evidence
finds a necessary v2 part route, it must use format-aware nonuniform piece
lengths, a new artifact version, and the accepted pre-release reset contract
within this tactical; it may not reuse the v1 final-piece arithmetic
silently.

### Resume and recheck

`HaveState` remains one bit per logical piece and is already bound to the
opaque torrent owner, exact-info content fingerprint, and piece count. It does
not need an algorithm field as long as the runtime descriptor and artifact
evidence are independently format-checked before those bits are trusted.

Fast resume extends its structural observation to v2 files and nonuniform
file-final piece lengths. Accepted fast resume remains explicitly historical
durability evidence. Any missing, short, oversized, wrong-kind, stale-owner,
wrong-source, or geometry-incompatible artifact follows the existing local
full-check or repair decision without changing another torrent.

Full recheck hashes every readable logical v2 piece independently of current
selection, records a typed pass/mismatch/unreadable outcome, synchronizes
newly recovered managed content before committing new bits, and atomically
replaces durable have only after all jobs join. Force recheck always takes
this path.

### Publication and active reads

Publication derives its root name and ordered real file paths from the
validated v2 metainfo projection. It creates selected empty files, excludes
alignment gaps, publishes only after every wanted nonempty piece is verified
and durable, and retains the existing namespace-transition and process-death
contract.

Verified active file reads and HTTP streaming use file-local verified ranges.
They never expose an unverified block merely because another piece in that
file passed. The active-to-published handoff and existing volatile capability
authorization remain unchanged.

## Persistence And Reset Contract

The existing schema-19 shape already has the required durable concepts:

- one opaque `torrent_id`;
- separate full v1/v2 rows in `torrent_identities`;
- exact `raw_info` and its content fingerprint;
- exact verbatim outer metainfo in `torrent_source`;
- bounded tracker, selection, have, artifact, namespace, and lifecycle state;
  and
- generated `TorrentProtocolIdentities` projections.

Reuse that shape rather than adding a second v2 catalog. Pure-v2 byte intake
inserts only the full v2 identity, retains the exact outer source and exact
raw info, derives the v2 piece count, persists ordinary selection intent, and
uses the opaque owner for managed artifact names.

The resume projection must provide the verbatim outer source for pure v2.
Restart reparses it with durable limits, requires a pure-v2 format with
complete valid layers, and checks that its exact info bytes, full identity,
fingerprint, piece count, and stored identity row agree. `raw_info` alone may
not reconstruct a runnable v2 descriptor.

No compatibility migration is required. If implementation discovers that a
schema-19 constraint cannot represent the accepted model, replace it with one
fresh schema epoch and a pre-task reset rather than adding migration or dual-
reader code. The reset may discard RSTorrent-owned catalog, source, have,
staging, and part state after resolving exact targets. It must not delete
user-selected roots or already published payload as an incidental reset.

Metainfo export returns the retained verbatim source. It must not synthesize
piece layers from runtime state or re-encode the info dictionary.

## Peer, Discovery, And Seeding Contract

### Versioned peer routing

Pure-v2 outgoing and incoming connections select
`SwarmKey::V2Truncated`. The session registry remains keyed by
`(ProtocolVersion, 20-byte wire key)`, so a v1 value equal to a v2 truncation
cannot attach to the wrong owner.

The outgoing route already knows it selected v2. The incoming listener learns
the version from the tagged registration returned for the received key. Pure
v2 does not use the hybrid legacy-to-v2 reserved-bit upgrade; that transition
remains Stage 5 work.

Standard bitfield, have, request, piece, cancel, Fast, PEX, and rate-policy
behavior uses the selected connection version and v2 logical piece count.
Every incoming request is checked against the actual file-local piece payload
length before allocation or I/O. A request that reaches a logical alignment
gap is impossible by construction and fails closed if represented.

Unknown or deferred messages 21--23 must be bounded and handled without
state corruption, false have, or unbounded allocation. This tactical does not
send them, derive trusted hashes from them, or claim to serve sparse-hash
peers. Controlled peers receive the same complete `.torrent` and therefore do
not depend on them. If pinned libtorrent requires hash exchange despite that
contract, implementation stops at the escalation gate instead of silently
absorbing Stage 4.

### Trackers, DHT, MSE, and transports

Tracker and DHT operations receive the tagged v2 truncation from the runtime
identity rather than reading a v1 field from metainfo. Private-torrent policy,
tracker tiers, address-family behavior, endpoint advertisement, retry, and
task ownership remain unchanged.

MSE derives and resolves its request hash from the selected 20-byte v2 wire
key while retaining the version tag at registry attachment. TCP and uTP feed
the same peer framing and v2 content coordinator; no transport-specific
integrity path is introduced.

The proportional regression matrix is deliberately not every combination:

- direct controlled TCP transfer in both oracle roles is mandatory;
- one outgoing and one accepted default-uTP pure-v2 transfer cover both
  RSTorrent payload roles, potentially using the same two-role runs;
- one controlled MSE transfer plus deterministic initiated/accepted routing
  covers encrypted key selection;
- one tracker-discovered and one DHT-only transfer cover the v2 truncation;
- existing deterministic privacy, fallback, cancellation, and v1 transport
  suites cover unchanged surrounding policy.

### Upload and seeding

The torrent coordinator may advertise a v2 have bit only after successful
Merkle verification and durability for the current storage generation.
Verified active pieces use the existing bounded duplex upload path; complete
published path-backed content registers with the shared incoming listener and
is restored after restart.

Upload reads use the format-aware mapping and actual file-final piece length.
They cannot read an alignment gap or unverified range. Pause, archive, force
recheck, selection change, root replacement, removal, application shutdown,
or storage loss invalidates and joins the exact seed registration before
mutating durable or storage authority.

## Owner, Task, Cancellation, And Dependency Map

```text
runtime-free protocol values
  ParsedOuterMetainfo -> owned V2 content -> geometry -> expected Merkle plan
             no task, socket, filesystem, clock, or persistence dependency
                                |
                                v
application byte transaction and SessionStore
  exact source + raw info + v2 identity + selection + have + lifecycle
                                |
                                v
application torrent generation
  one runtime descriptor + one identity context + existing supervisor
          |                     |                         |
          v                     v                         v
tracker/DHT managers     peer coordinator          SelectiveStorage
tagged v2 key            piece/request owner       immutable I/O plans
          |                     |                         |
          v                     v                         v
existing tasks           TCP/uTP/MSE peers         bounded write/hash/check jobs
cancel + join            cancel + join             cancel + join + durability
                                |
                                v
active upload / completed seed registration
  same verified geometry and have authority; generation-fenced removal/join
```

No new long-lived task is authorized. The application owns one torrent
generation, the content coordinator owns piece/request state and peer child
tasks, storage owns bounded positional job futures, discovery retains its
existing supervised managers, and the shared listener owns connection
admission. Every callback carries the existing torrent/storage generation and
cannot mutate a replacement.

The concrete dependency improvement is an explicit runtime content/geometry/
integrity boundary replacing repeated v1 `Metainfo` assumptions. It remains
an enum and ordinary methods, not a generic engine framework. Private module
extraction is allowed when it makes deterministic mapping or expected-hash
selection independently testable; no new crate is selected.

## Security, Integrity, And Resource Invariants

- Only strict complete pure-v2 outer metainfo reaches this runtime. Hybrid,
  info-only, missing-layer, wrong-layer, and malformed sources fail before
  storage paths, task spawning, or network registration.
- The full 32-byte v2 hash remains the authoritative protocol identity. A
  truncated wire key is never used for database identity or untagged lookup.
- Unverified bytes never enter have state, completion, publication, active
  reads, HTTP service, or upload.
- Alignment gaps consume no payload allocation, queue entry, file, descriptor,
  part slot, progress byte, request, or hash input.
- Merkle zero padding is derived by the runtime-free hash core and never read
  from or written to storage.
- Every file, piece, request, offset, length, addition, multiplication, and
  conversion is checked before indexing, allocation, mutation, or I/O.
- Tactical `146` limits remain in force: outer source at most 64 MiB, at most
  2,097,152 logical pieces, the accepted file/component/path bounds, one
  validated complete piece-layer set, at most 36 retained accumulator hashes,
  and no per-piece geometry allocation in `V2TorrentLayout`.
- The runtime must not duplicate all piece-layer hashes into a second expected-
  hash vector. Indexed lookup may add bounded per-file ranges or offsets over
  the one validated set.
- Verification reads use the existing 16-KiB buffer. Piece length does not
  become resident verification memory.
- Existing desktop and Android request, buffered-payload, storage-intake,
  active-piece, peer, task, descriptor, and storage-job limits remain hard
  ceilings. V2 does not multiply them by file count, peer count, or format.
- At most one optional lazy part owner remains associated with a torrent
  generation; ordinary pure-v2 selection must retain a zero part-slot and
  zero part-artifact high-water.
- Exact outer source storage continues under the existing bounded SQLite page
  and source-byte limits. Diagnostics expose sizes and typed failure reasons,
  not metainfo, paths, hashes beyond intended identity fields, or payload.
- Hash, write, recheck, sync, publication, listener, and peer completion are
  generation-fenced. Cancellation joins every child before replacement or
  terminal success.
- A hash mismatch invalidates only the affected v2 logical piece and bounded
  exact-generation contributor evidence. It cannot clear unrelated have
  state or authorize leaf-level blame without authenticated leaf hashes.

## Required Observability

Extend structured engine/application facts only where the existing views
cannot explain the new behavior:

- metainfo format and selected protocol version;
- full identity presence through existing protocol-identity projections and
  the selected tagged wire version without presenting truncation as full ID;
- total, wanted, verified, active, and missing v2 logical pieces and real
  payload bytes;
- file index and file-local piece for a v2 storage/hash failure;
- verification algorithm and typed expected-root source (`file_root` or
  `piece_layer`), without dumping hash material;
- gap/request rejection, source/layer rejection, and restart disagreement
  reasons;
- checker phase, hashed payload bytes, passed/mismatched/unreadable pieces,
  and generation as already modeled;
- expected-layer bytes retained, Merkle scratch hashes, verification-buffer
  bytes, part slots, descriptors, storage jobs, resident payload, requests,
  peers, and tasks at high water; and
- terminal counts proving zero live torrent peer generations, storage jobs,
  seed registrations, discovery work, and owned temporary artifacts.

Do not add a separate v2 log API. Existing application snapshots, protocol
identity DTOs, Files/progress views, disk/checker views, and bounded structured
diagnostics remain the product surfaces.

## Application And Platform Contract

The existing semantic add-torrent-bytes operation accepts the new subset and
returns the opaque torrent ID with the existing new/already-present result.
Duplicate lookup uses the full v2 identity row. A v1 torrent with the same
20-byte wire value is not a duplicate. A hybrid source remains unsupported
rather than merging with either owner.

File selection uses the canonical v2 real-file indices and paths already
produced by Tactical `146`. Add options, live `Normal`/`Skip`, pause/resume,
force recheck, archive, remove, export, open/stream, and status operations use
their existing semantic commands. The shared web application, Tauri shell,
Android Compose client, and iOS client should not gain v2-specific duplicate
or storage policy.

`TorrentProtocolIdentities` already represents optional full v1 and v2
values. Any additional generated change must describe a real semantic fact
such as metainfo format; no client receives an ambiguous replacement
`info_hash` string. Regenerate and validate TypeScript, JSON schema, Kotlin,
and Swift artifacts whenever a crossing Rust type changes.

Required product/platform evidence is:

- authenticated headless web add-bytes, file selection, progress, completion,
  force recheck, export, and restart against the production application;
- Tauri compile and direct adapter tests without launching a visible window;
- iOS generated-boundary, simulator/build, and existing lifecycle regression
  proportional to an unchanged storage capability;
- Android x86_64 and arm64-v8a native builds; and
- one owned API 34 no-window AVD run using the real application, platform
  storage boundary, aligned selective fixture, process restart/recheck,
  publication verification, and one bounded upload/seed observation.

A physical-device run is not required merely to repeat an unchanged
descriptor/storage capability. New provider-specific behavior or a failure
that cannot be reproduced on the AVD reopens that evidence decision.

## Implementation Stages And Commit Gates

### Stage 0: Activation, source reconfirmation, and baseline

- Wait for Tactical `150` to close and for readiness to make this tactical
  the sole **Now**.
- Run `scripts/references.py status` and record exact BEP, libtorrent, and
  JSTorrent revisions or dirty-checkout warnings.
- Reinspect the exact source/test areas listed above and record corrections
  before code depends on them.
- Inventory every direct `Metainfo`, `TorrentLayout`, `[u8; 20]`, SHA-1,
  `piece_index * piece_length`, `raw_info`, publication, and seed assumption
  in protocol, engine, session, generated, and platform paths.
- Capture the full v1 baseline and confirm no unrelated Tactical `150`
  residue is included.

Commit gate: source/inventory corrections and any tactical clarification only.

### Stage 1: Owned descriptor, admission, and persistence

- Add the owned explicit v1/v2 runtime content model and complete-only v2
  constructor.
- Extend pure-v2 byte projection, duplicate lookup, exact source/raw-info
  retention, identity insertion, selection projection, resume, and export.
- Extend runtime identity construction to select `SwarmKey::V2Truncated`.
- Preserve hybrid and v2 info-only rejection.
- Reuse schema 19 or perform the authorized fresh-epoch reset if an actual
  constraint requires it; add no migration compatibility path.

Commit gate: deterministic protocol/store/application intake, duplicate,
restart-source, invalid-source, reset, export-fidelity, and v1 regression
tests pass before storage work.

### Stage 2: Format-aware geometry, storage, and integrity

- Introduce the shared checked content geometry and expected-integrity plans.
- Extend write, hash, active-read, upload-read, observation, sync, selection,
  and publication planning for v2 file-local pieces.
- Implement streamed 16-KiB SHA-256 Merkle verification with distinct
  one-piece file-root and multi-piece layer-root shapes.
- Prove no v2 gap span or ordinary v2 part artifact is created.
- Extract only a private deterministic mapping/integrity module if the
  current `selective_storage.rs` boundary demonstrably benefits.

Commit gate: exhaustive geometry/hash/storage tests, maximum accepted
arithmetic cases, cancellation, short I/O, and resource high-water assertions
pass with unchanged v1 behavior.

### Stage 3: Driver, durable have, checking, and publication

- Thread the descriptor through the content coordinator, storage pipeline,
  piece completion, contributor recovery, checkpoints, fast resume, full
  recheck, progress, selection restart, namespace transition, publication,
  verified active reads, and streaming eligibility.
- Preserve sync-before-have ordering and atomic full-recheck replacement.
- Add scripted missing, short, corrupt, read-only, interrupted, stale-state,
  selection, and cancellation cases.

Commit gate: a local scripted pure-v2 source completes, restarts, force-
rechecks, republishes, serves verified reads, and terminates with exact owner
cleanup before external interoperability.

### Stage 4: Versioned peer routing, discovery, upload, and seeding

- Route outgoing/incoming TCP, uTP, and MSE connections through the tagged v2
  key and descriptor.
- Apply v2 piece counts and actual request lengths to standard peer messages.
- Feed tracker and DHT managers the v2 truncation while preserving privacy and
  endpoint policy.
- Register active verified pieces and completed content for upload and restore
  eligible seeding after restart.
- Bound and safely reject/defer messages 21--23 without adding hash exchange.

Commit gate: scripted initiated/accepted transfers, invalid request and
message cases, tracker/DHT routes, active upload, completed seed, restart
seed, TCP/uTP/MSE, and exact task/slot cleanup pass.

### Stage 5: Application, generated clients, and Android

- Carry pure-v2 add and existing operations through all adapters.
- Regenerate every changed contract and update frontend/runtime validators.
- Add authenticated headless browser evidence without launching Tauri.
- Build Tauri and iOS regressions, both Android native ABIs, and the exact API
  34 AVD scenario.

Commit gate: generated drift checks and proportional first-party platform
evidence pass; screenshots are retained only if they prove a presentation
change, which is not expected.

### Stage 6: Pinned-libtorrent interoperability and closure

- Generate independent single-file and aligned multi-file pure-v2 fixtures
  with the pinned oracle.
- Run RSTorrent leecher/libtorrent seed and libtorrent leecher/RSTorrent seed
  over TCP, then the bounded uTP, MSE, tracker, and DHT matrix.
- Run selective, corruption, restart, incomplete, read-only, publication,
  active upload, completed seed, and terminal-cleanup variants without
  multiplying every dimension.
- Record exact payload hashes, selected-file manifests, protocol version,
  connection direction, transport, high-water marks, cleanup, and any
  intentional difference.
- Run full repository gates and update every owning document and claim.

Commit gate: the stopping condition is fully evidenced. Do not mark the
tactical complete because code exists or one happy-path transfer succeeds.

## Validation Matrix

### Deterministic protocol and geometry

- exact complete pure-v2 ownership; v1 preservation; hybrid/info-only/layer-
  incomplete rejection;
- expected-root lookup for sub-block, exact-block, sub-piece, exact-piece,
  piece-plus-one, multi-piece, and padded final-piece files;
- ordered empty files before, between, and after nonempty files;
- global/file-local piece and request round trips at every boundary;
- no gap write/read/hash/select/publish/upload representation;
- full maximum piece/file/offset arithmetic without per-piece geometry
  allocation; and
- versioned v1/v2 wire-key collision lookup.

### Storage and integrity

- partial and complete positional writes, short reads/writes, truncation,
  storage error, write cancellation, hash cancellation, and stale completion;
- SHA-256 Merkle pass/fail with fixed 16-KiB reads and at most the retained
  Tactical `146` scratch bound;
- no have before verified sync; one-piece failure resets only that piece;
- all-normal, all-skipped, mixed selection, demotion, promotion, selected
  empty files, and zero ordinary v2 part artifacts;
- path and platform observation, structural fast resume, full recheck, force
  recheck, wrong source, stale bitmap, and unreadable storage;
- publication before/after namespace transition death and exact selected-file
  output; and
- active and published verified reads with alignment-gap and unverified-range
  rejection.

### Scripted peer and lifecycle

- initiated and accepted v2 handshake routing over a colliding v1/v2 key;
- bitfield spare bits, out-of-range have, invalid request begin/length,
  request crossing a file end, duplicate/late payload, corrupt source, and
  hash failure retry;
- choke, disconnect, expiry, endgame duplicate, cancellation, pause, archive,
  force recheck, selection replacement, removal, and shutdown;
- active verified-piece upload, completed seed, and restart seed;
- deferred hash-message input cannot allocate unbounded state or change
  integrity authority; and
- terminal peer, request, storage job, descriptor, registration, and task
  counts return to zero.

### Controlled independent interoperability

At minimum retain exact results for:

| Role | Fixture | Route | Required result |
| --- | --- | --- | --- |
| RSTorrent leecher / libtorrent seed | single-file and aligned multi-file | direct TCP | Exact selected payload, v2 route, publication, cleanup |
| libtorrent leecher / RSTorrent seed | single-file and aligned multi-file | direct TCP | Exact payload, upload bounds, completed seed cleanup |
| Both RSTorrent payload roles | one representative aligned fixture | default uTP, initiated and accepted | Exact payload without TCP masking |
| One RSTorrent payload role | representative fixture | MSE | Tagged v2 key and exact payload |
| RSTorrent leecher | representative fixture | tracker then DHT-only | Correct truncated key, discovery, exact payload |
| Restart/recheck | aligned selective fixture | controlled TCP | No false have; missing/corrupt pieces repaired |
| Active upload | incomplete two-peer fixture | controlled TCP | Only verified pieces served; final exact payload |

Each peer starts with the complete `.torrent`. A run depending on BEP 52 hash
messages does not satisfy this tactical.

### Repository and platforms

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`;
- `cargo test --workspace`;
- `npm run generate --prefix clients/web` when any boundary type changes;
- `npm run typecheck --prefix clients/web`;
- `npm run test --prefix clients/web`;
- focused authenticated headless application/browser interop;
- Tauri compile/direct adapter tests without a visible product launch;
- iOS generated-boundary and build/simulator regression selected by
  `DEVELOPMENT.md`;
- Android x86_64 and arm64-v8a native builds; and
- one owned API 34 no-window AVD application run with exact cleanup.

Run additional focused commands recorded by `DEVELOPMENT.md` for changed
crates or generated surfaces. Report exactly what ran; do not imply an unrun
platform or transport matrix passed.

## Non-Goals And Deliberate Deferrals

- `btmh` magnet parsing, export, or duplicate reconciliation.
- SHA-256 BEP 9 metadata acquisition or info-only runtime admission.
- BEP 52 hash request, hashes, or hash reject messages 21--23.
- Sparse Merkle knowledge, hash-request scheduling, proof correlation,
  persistence, refetch, peer attribution, or leaf-level recovery.
- Hybrid metainfo admission, the reserved-bit upgrade, dual swarm
  participation, dual integrity, or v1/v2 owner merging.
- V2 or hybrid torrent creation, piece-size policy, canonical creator output,
  or first-party source generation.
- Web seeds, hole punching, new address-family policy, NAT traversal changes,
  or broader peer extensions.
- A second engine, daemon, socket/filesystem proxy, or libtorrent dependency.
- A new torrent UI, v2-only settings, priority scale, or separate client
  workflow.
- Compatibility migrations or readers for discarded pre-release database,
  have, part, source, or staging formats.
- Deleting published payload or user-selected root content during a reset.
- Public-swarm or performance-parity evidence as a completion requirement.
  Controlled exact interoperability and bounded resource regression are the
  authority for this correctness slice.
- Claims for hybrid, v2 magnet, sparse hash exchange, torrent creation, or all
  BEP 52 behavior.

Stage 4 of the owning topic remains v2 magnets and authenticated hash
exchange. Stage 5 remains hybrid dual-swarm closure. Creation remains a
separate later capability.

## Escalation Gates

Routine choices within this document are authorized once the queue activates
the tactical. Stop for maintainer direction if implementation would:

- add messages 21--23 because a complete-source controlled peer unexpectedly
  requires them;
- admit v2 info-only, magnet, or hybrid content;
- merge live torrent owners or weaken full-identity collision handling;
- accept payload without complete trusted expected hashes and successful
  verification;
- add a new runtime, storage, crypto, or engine dependency with meaningful
  tradeoffs;
- create a second filesystem, checker, scheduler, peer, persistence, or
  application owner;
- delete a published payload or modify a user-selected root as part of a
  persistence reset;
- weaken Android first-party engine parity or move payload/hash work across a
  Kotlin, Swift, JavaScript, IPC, or daemon boundary;
- require a product-policy or presentation change beyond existing semantic
  operations; or
- expand controlled evidence into public-network, remote-machine, physical-
  device, release, publish, or external side effects not already authorized.

An exact schema/artifact epoch reset of RSTorrent-owned unreleased state,
after resolving and reporting its targets and proving external payload is not
modified, is already authorized by this tactical and does not require a
migration design checkpoint.

## Commit And Evidence Plan

Use bounded commits that leave the repository buildable and preserve v1:

1. source/inventory corrections and final activated plan;
2. owned v2 runtime descriptor plus pure-v2 store/application intake;
3. format-aware geometry, storage plans, and streamed Merkle verification;
4. durable have, recheck, publication, and verified-read integration;
5. versioned peer/discovery routing plus active and completed upload;
6. generated clients and Android/platform composition;
7. controlled pinned-libtorrent harness and two-role evidence; and
8. full regression, resource record, topic/ledger closure, and cleanup.

Each nontrivial commit records the relevant validation and preserves
`Topic: bittorrent-v2-and-hybrid`. Implementation evidence belongs in this
document as it lands. Temporary oracle torrents, payloads, profiles, captures,
logs, packages, simulator/AVD state, and subprocesses are removed before
closure unless an explicitly linked bounded artifact is intentionally
retained.

## Execution Record

### 2026-08-13 Stage 0 baseline

- The readiness queue and campaign checkpoint both name this tactical as the
  sole `Now`; Tactical `150` and the intervening iOS repair Tactical `152` are
  complete, and the RSTorrent worktree was clean at activation.
- `scripts/references.py status` confirmed the BEP checkout at
  `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06` and libtorrent at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d`. The separate JSTorrent checkout
  has pre-existing documentation and image changes, so it remains read-only;
  no source or fixture will be imported from it.
- The source/test reconfirmation found no correction to the accepted design:
  complete outer piece layers remain sufficient expected-hash authority for
  the bounded peer contract, while hash messages 21--23 remain outside it.
- The direct-assumption inventory found 2,086 references across protocol,
  engine, session, and ungenerated web sources. The shape-changing owners are
  the v1 `Metainfo`/`TorrentLayout` parse boundary, application byte intake and
  `ResumeRecord`, `SelectiveStorage` plans, the content driver and storage
  pipeline, publication/active/published readers, incoming seeding, and the
  versioned peer/discovery registry. Test-only v1 fixtures remain v1-owned.
- `cargo test --workspace` passed the pre-change baseline. Opt-in public,
  remote, platform-trust, and large-allocation tests remained ignored by their
  existing contracts.

### 2026-08-13 Stage 1 descriptor and admission

- `TorrentContent` is the owned runtime-free v1/v2 sum. Its pure-v2 variant
  can be constructed only from strict complete outer metainfo and owns the one
  validated piece-layer set; hybrid and info-only input cannot enter it.
- The descriptor projects the full identity, tagged wire key, canonical files,
  file-aligned piece geometry, actual piece lengths, and indexed SHA-1 or
  SHA-256 expected-integrity plans. Multi-piece files index the retained layer;
  one-piece files retain their file-root tree height.
- Schema 19 required no change. Byte intake now deduplicates by the one full
  protocol identity, inserts pure v2 as a 32-byte `v2` alias, stores exact
  `raw_info` and verbatim outer source, projects selection over real v2 files,
  and reconstructs conservative wanted/have evidence from that source.
- Application runtime identity selection now chooses the tagged v2 truncation
  for v2-only owners while refusing an unselected hybrid owner. A paused
  application add and reopen proves that no payload artifact is created before
  the storage vertical lands.
- `cargo test -p rstorrent-protocol`, `cargo test -p rstorrent-session`, and
  `cargo clippy -p rstorrent-protocol -p rstorrent-session --all-targets --
  -D warnings` pass. The focused pure-v2 store and application restart cases
  pass with full-identity duplicate behavior and exact-source equality.

### 2026-08-13 Stage 2 geometry, storage, and integrity

- `ContentLayout` is the format-aware deterministic storage projection. V1
  retains its concatenated-file and padding semantics; v2 maps every logical
  piece to one file-local span and never represents the alignment gaps between
  files as payload, requests, or storage segments.
- `SelectiveStorage` now accepts the owned content descriptor. Pure-v2 writes
  go directly to their owning file, skipped-file pieces are rejected before
  mutation, and both create and resume paths have a hard guard against an
  ordinary part artifact or part slot for v2 content.
- The storage hash plan selects SHA-1 or SHA-256 Merkle verification from the
  descriptor. The v2 accumulator streams fixed 16 KiB leaves, retains only the
  active Merkle frontier, pads to the descriptor's authenticated target
  height, and reports its retained-hash high-water mark for regression tests.
- A mixed pure-v2 fixture covers a skipped file, a multi-piece file backed by
  a complete piece layer, and a one-piece file authenticated by its file root.
  It proves file-local writes, both Merkle shapes, bounded scratch state, no
  gap or part artifact, exact-path publication, and published restart without
  weakening durable have evidence.
- `cargo test -p rstorrent-engine` and `cargo clippy
  -p rstorrent-protocol -p rstorrent-engine --all-targets -- -D warnings`
  pass, including the unchanged v1 storage, publication, and resume suites.

### 2026-08-13 Stage 3 driver, checking, and path publication

- The content driver, storage queue, scheduler geometry, and managed full
  checker now consume `TorrentContent` and `ContentLayout`. Expected integrity
  and computed hash results are explicitly typed; the common pipeline compares
  SHA-1 only with v1 expectations and Merkle roots only with v2 expectations.
- Complete outer metainfo has a dedicated resumable engine entry point. It
  reparses the strict durable source, validates both the full identity set and
  selected tagged wire key, retains the exact raw info span for the artifact
  fingerprint, and bypasses the v1-only magnet metadata path.
- Active verified-file geometry now uses a format-aware piece-space file
  origin. V1 boundary offsets remain unchanged while each v2 file begins at
  its authenticated global piece, so file-alignment gaps cannot leak into
  streaming availability or read decisions.
- A controlled pure-v2 peer test downloads a skipped one-piece file plus a
  selected multi-piece file. It proves the tagged v2 handshake, no request for
  the skipped piece, standard payload transfer, durable Merkle verification,
  exact tree publication, no part bytes or reopen, then a full published
  restart recheck from the same complete source.
- The focused v2 vertical and all 98 non-network driver regressions pass; the
  two existing public-swarm cases remain ignored by their opt-in contract.
