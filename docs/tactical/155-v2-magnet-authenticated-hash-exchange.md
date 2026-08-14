# Tactical 155: V2 Magnet And Authenticated Hash Exchange

Status: **Decision-complete and authoritative Now on 2026-08-14.** No
implementation has begun.

Topics: `bittorrent-v2-and-hybrid`, `protocol-support`,
`download-correctness`, `client-persistence`, `peer-lifecycle`,
`incoming-reachability-and-seeding`, `application-view-api`,
`client-surfaces`, `code-organization-and-refactoring`,
`capability-readiness`, `oracle-driven-engine-campaign`

Dependencies: completed Tacticals
[`143`](143-dual-identity-and-persistence-foundation.md),
[`146`](146-runtime-free-bep52-metainfo-geometry-merkle.md), and
[`151`](151-complete-source-pure-v2-runtime-vertical.md), plus the existing
BEP 9 metadata, discovery, peer, storage, checking, persistence, upload,
generated-boundary, and Android owners. Completed iOS Tactical
[`154`](154-ios-truthful-progress-and-system-preview.md) was a queue
dependency rather than an engine dependency.

## Decision And Desired Outcome

Add one bounded pure-v2 magnet vertical. RSTorrent will accept and export the
canonical SHA-256 `btmh` form, acquire and authenticate the exact info
dictionary through BEP 9, obtain the missing piece or leaf hashes through BEP
52 messages 21--23, verify selected payload, restart conservatively, and
serve metadata, authenticated hashes, and verified payload to another v2
peer.

This slice closes the largest missing part of ordinary pure-v2 consumption.
It also makes the Merkle design earn its keep: the magnet carries one trusted
full info hash; the info dictionary supplies compact per-file roots; and a
peer can prove only the piece-layer or leaf ranges needed for selected files
without an unauthenticated hash list becoming torrent truth.

The accepted contract is deliberately narrower than all BEP 52 behavior:

- the magnet contains exactly one supported full identity,
  `urn:btmh:1220` followed by 64 hexadecimal SHA-256 digits;
- acquired metadata must parse as pure v2 and its exact bytes must match that
  full SHA-256 identity;
- mixed `btih` plus `btmh`, metadata that reveals a hybrid torrent, and
  separate-owner reconciliation remain Stage 5 work;
- the trusted file roots come from authenticated raw info, while missing
  piece and leaf hashes enter a separate sparse integrity owner only after a
  valid proof reaches the applicable file root;
- normal payload requests are hash-first: the expected root for a logical
  piece must be authenticated before new network payload for it is requested;
- payload already on disk may remain candidate data while hashes are absent,
  but it is not have, streamable, publishable, or uploadable; and
- complete-source pure-v2 `.torrent` and all v1 behavior remain unchanged.

## Scope And Stopping Condition

This tactical owns:

1. strict bounded `btmh` parsing, typed pure-v2 identity, canonicalization,
   duplicate handling, source-aware export, tracker/peer-hint/select-only
   composition, and explicit rejection of mixed identity;
2. SHA-256 BEP 9 acquisition of the exact info dictionary through the
   existing bounded cross-peer metadata owner, followed by strict info-only
   pure-v2 parsing and metadata-time identity/format conflict handling;
3. a deterministic split between immutable v2 content geometry and mutable
   authenticated hash knowledge, without optional hash fields leaking
   through every storage consumer;
4. codecs and validation for BEP 52 hash request, hashes, and hash reject
   messages 21, 22, and 23;
5. bounded piece-layer request selection, exact peer correlation, proof
   validation, duplicate attempts, rejection, timeout, reconnect, and
   cancellation;
6. one conservative state for payload present before expected hashes arrive,
   including restart recovery and later verification without presenting the
   bytes as valid content;
7. leaf-layer acquisition after a failed piece, exact corrupt-block recovery
   and contributor attribution when proofs are available, plus a bounded
   whole-piece fallback when they are not;
8. authenticated hash upload for announced pieces, including the BEP 52
   base-layer-zero obligation when RSTorrent services the associated payload
   request;
9. an explicit no-sparse-hash-persistence policy with incomplete restart
   refetch and complete-file local reconstruction;
10. ordinary application add/export, selection, progress, pause/resume,
    recheck, remove, publication, active reads, upload, and diagnostics;
11. proportional web, Tauri, Android, and iOS boundary/build evidence, with
    one API 34 application run through the real platform storage owner; and
12. pinned-libtorrent interoperability in both roles, hostile scripted peers,
    bounded resource high-water marks, and terminal cleanup.

The tactical stops only when all of the following are true:

- a pure-v2 `btmh` magnet reaches exact selected content through the ordinary
  application service, including multi-file and one-piece-file shapes;
- the first controlled magnet gate uses only `btmh`, one `x.pe` peer hint,
  and BEP 53 `so` selection against a pinned-libtorrent seed, with tracker and
  DHT discovery disabled. Wire evidence must show SHA-256 metadata, required
  hash exchange before selected payload requests, no payload or piece-layer
  request for a skipped multi-piece file, and no unnecessary piece-layer
  request for a selected one-piece file;
- SHA-256 BEP 9 metadata succeeds across peers and rejects wrong hashes,
  malformed info, v1 metadata, and hybrid metadata before storage authority
  changes;
- every requested piece-layer range is proved to the root named by the
  authenticated file tree before any contained hash can authorize payload;
- malformed, oversized, unsolicited, mismatched, duplicate, rejected,
  stalled, late, and bad-proof hash messages have deterministic bounded
  outcomes and cannot mutate shared truth;
- data present before hashes arrive remains quarantined, then becomes have
  only after the expected hash arrives, the data verifies, and the existing
  durability transition completes;
- valid leaf proofs identify corrupt 16-KiB blocks and their exact retained
  contributors without discarding good blocks; unavailable leaf proofs make
  bounded progress through the conservative whole-piece reset;
- RSTorrent can answer valid piece-layer and leaf-layer requests required by
  its announced availability, or reject requests it is not required and not
  able to serve;
- an incomplete restart refetches sparse hash knowledge before restoring
  advertised have, while a complete file can reconstruct and validate its
  tree locally without another peer;
- RSTorrent and pinned libtorrent each complete as leecher from the other's
  pure-v2 `btmh` plus `x.pe` magnet-capable seed, with independent payload
  comparison and no discovery path masking the direct peer hint;
- selected TCP, default-uTP, MSE, tracker, DHT, initiated, and accepted paths
  receive the proportional matrix below without an unnecessary cross-product;
- v1 and complete-source pure-v2 regressions remain green;
- both Android native ABIs compile and an owned API 34 no-window run proves
  v2 magnet intake, missing-hash state, selected transfer, restart recovery,
  publication, hash service, and exact cleanup; and
- owning topics, protocol claims, tactical evidence, high-water marks, and
  deferrals describe only what was actually proved.

## Normative And Source-Oracle Record

The implementation must reconfirm every managed revision before code changes.
This design review used the pins in
[`reference/pins.toml`](../../reference/pins.toml).

### Normative specifications

The BEP checkout is pinned at
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`.

- `reference/bittorrent.org/beps/bep_0009.rst` defines `btmh` as the
  hex-encoded multihash-formatted full v2 info hash and defines metadata
  exchange as the exact info dictionary in 16-KiB blocks. `btih` and `btmh`
  may coexist only for the same hybrid torrent, which this slice rejects.
- `reference/bittorrent.org/beps/bep_0052.rst` defines the exact SHA-256 info
  identity, tagged 20-byte swarm key, file roots, aligned geometry, and hash
  messages 21--23. A request names the 32-byte file root, base layer, index,
  count, and proof layers. Index is a multiple of count; count is at least two
  and a power of two; 512 is the normative recommended ceiling.
- BEP 52 requires a hash response to correlate with a request and orders base
  hashes before uncle proof hashes. It requires clients to serve hash blocks
  covering announced pieces and forbids rejecting an applicable leaf-layer
  request immediately associated with a serviced payload request.
- BEP 52 permits hash exchange regardless of choke state and calls for a
  separate rate policy. Hash work therefore cannot borrow unbounded capacity
  from the ordinary piece-request window.
- `reference/bittorrent.org/beps/bep_0003.rst`, `bep_0005.rst`,
  `bep_0010.rst`, `bep_0011.rst`, `bep_0015.rst`, `bep_0023.rst`, and
  `bep_0029.rst` continue to govern the already-supported peer, discovery,
  tracker, extension, and uTP paths composed here.
- `reference/bittorrent.org/beps/bep_0004.rst` and `bep_0047.rst` are reviewed
  only to keep hybrid upgrade and v1 padding behavior out of this slice.

### Pinned libtorrent source

Rasterbar libtorrent `2.0.13` is pinned at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. The review inspected:

- `src/magnet_uri.cpp` for pure-v2 and hybrid parsing/generation and
  `test/test_magnet.cpp::{parse_v2_hash,parse_v2_short_hash,
  parse_v2_invalid_hash_prefix,parse_v2_invalid_hex_hash,parse_hybrid_uri,
  info_hash_v2,hybrid_info_hashes}` for exact accepted and rejected forms;
- `src/ut_metadata.cpp` for info-only transport and bounded metadata
  acquisition, while retaining RSTorrent's existing 30-MiB peer-metadata
  profile instead of copying libtorrent's configurable default;
- `src/torrent.cpp::set_metadata()` for exact SHA-256 validation, strict info
  parsing, metadata-time identity expansion/collision handling, and Merkle
  initialization;
- `src/hash_picker.cpp::{validate_hash_request,pick_hashes,add_hashes,
  hashes_rejected,verify_block_hashes}` and
  `include/libtorrent/hash_picker.hpp` for 512-piece range selection,
  peer-availability gating, three-second retry eligibility, sparse proof
  adoption, rejection, and reconciliation with payload that already exists;
- `src/bt_peer_connection.cpp::{on_hash_request,on_hashes,on_hash_reject,
  write_hash_request,write_hashes,write_hash_reject,
  maybe_send_hash_request}` for exact framing, v2-only dispatch, response
  sizing, per-peer outstanding work, serving, and failure behavior;
- `src/torrent.cpp::{pick_hashes,get_hashes,add_hashes,hashes_rejected,
  verify_block_hashes}` for torrent-owned shared truth, valid-proof adoption,
  already-present block pass/fail outcomes, peer disconnect after bad proofs,
  and serving from known trees;
- `src/torrent.cpp::{initialize_merkle_trees,load_merkle_trees}`,
  `src/read_resume_data.cpp`, and `src/write_resume_data.cpp` for complete and
  sparse resume shapes; RSTorrent intentionally selects refetch/reconstruct
  rather than copying that persisted tree representation; and
- `simulation/transfer_sim.cpp`, `simulation/transfer_sim.hpp`, and
  `simulation/test_transfer.cpp` for v2 magnet, corruption, restart, and
  complete-source restoration scenarios.

### Pinned libtorrent tests

The edge-case inventory includes:

- `test/test_hash_picker.cpp` cases for piece and leaf ranges, 512-hash
  batching, padded final ranges, rejection/retry, request completion, peer
  availability, already-present data, bad hashes and proofs, and hostile file,
  base, index, count, proof, pad-file, empty-file, and single-block shapes;
- `test/test_merkle_tree.cpp` cases for full and sparse tree modes, one-piece
  addition, piece and block layers, padded and unpadded tails, partial proofs,
  invalid proofs and hashes, and existing valid or invalid block data;
- `test/test_read_resume.cpp::{round_trip_info_hash,
  round_trip_merkle_trees,round_trip_merkle_tree_mask,
  round_trip_verified_leaf_hashes}` for the restart case inventory, not a
  persistence format to adopt; and
- the transfer simulation matrix for v1/v2/hybrid, magnet, corruption, and
  resume. This tactical selects only the pure-v2 magnet cells and keeps the
  complete-source pure-v2 cells as regression gates.

RSTorrent deliberately differs from or narrows the oracle in three places:

1. outbound requests follow BEP 52's count shape and 512-hash recommendation,
   rather than libtorrent's 8,192-hash defensive receive ceiling. Inbound
   service accepts libtorrent's observed count-one leaf/piece compatibility
   shape only when every other root, layer, range, proof, and size check passes;
2. a hashes response or reject must match a live or bounded timed-out attempt;
   unsolicited or tuple-mismatched messages are never adopted even where the
   inspected libtorrent path is permissive; and
3. sparse hashes are not durable in this initial slice. Restart refetches or
   reconstructs them and revalidates candidate payload conservatively.

No libtorrent source or checked-in fixture is copied. The controlled harness
generates temporary pure-v2 inputs with the pinned oracle, records independent
payload digests, and removes every source, profile, capture, and process it
owns.

### JSTorrent product history

The local JSTorrent checkout is at
`9895410beeed6aff554053769bd006a3fbd373ef`. It was already dirty with
unrelated untracked attachment, design, and investigation files; this review
treated it as read-only.

- `packages/engine/src/core/peer-connection.ts` recognizes the BEP 52
  `info_hash2` extension value and disconnects because its v1 piece model
  cannot safely continue.
- `docs/archive/tasks/bep52-v2-implementation-plan.md` separately identified
  v2 magnets, SHA-256 metadata, a missing-piece-layer state, and messages
  21--23, but it is an unimplemented archived plan rather than a runtime
  oracle.
- Its useful product lesson remains fail-closed format separation and ordinary
  add/download presentation. RSTorrent does not adopt its proposed identity,
  daemon, storage-adapter, or optional-field architecture.

## Extracted Shape-Changing Edge Cases

The following cases must exist before the common path is architectural
evidence:

- exact `1220` multihash tag, uppercase/lowercase hex input, lowercase export,
  short, long, nonhex, unknown multihash, duplicate equal `btmh`, conflicting
  `btmh`, mixed `btih`/`btmh`, and no-supported-identity magnets;
- pure-v2 metadata whose SHA-256 is wrong, whose exact hash is right but parse
  fails, or whose parsed form is v1, hybrid, future-version, or over a bound;
- two v1/v2 torrents with identical 20-byte wire values remain distinct; a
  full v2 duplicate is idempotent and only explicit BEP 53 intent may promote
  files;
- one-block and one-piece files need no peer piece-layer hash; multi-piece
  files do, including the short padded final range and files later in the
  global aligned index space;
- a request may use only the leaf layer or the torrent piece layer in this
  subset; a structurally valid other base receives hash reject rather than
  entering unsupported state;
- roots for empty files, unknown roots, negative-as-signed wire integers,
  overflowing index plus count, count zero/non-power-of-two/over 512,
  misaligned index, impossible proof height, and exact-length mismatches fail
  before allocation or mutation. Count one is accepted only for the bounded
  inbound libtorrent leaf/piece compatibility case and is never selected by
  RSTorrent's requester;
- a padded final power-of-two request may cover deterministic zero-padding
  nodes beyond the file's real piece count, but those nodes never become
  payload or logical have entries;
- a hashes frame must match root, base, index, count, proof layers, expected
  hash count, peer attempt, and torrent generation before proof validation;
- one bad proof cannot poison a shared catalog or invalidate hashes proved by
  another peer; one valid duplicate response commits once and a later valid
  duplicate is harmless;
- hash reject, disconnect, and timeout release the logical range for another
  eligible v2 peer without losing already-proved neighboring ranges;
- ordinary payload is never newly requested without an authenticated expected
  piece root. Candidate payload from restart or a prior generation remains
  present-but-unverified until hash knowledge and verification converge;
- piece failure with multiple block contributors requests bounded leaf ranges;
  authenticated mismatches blame only exact bad block contributors and retain
  good blocks. Reject/stall exhausts into whole-piece reset with ambiguous
  attribution rather than an integrity stall;
- hash requests remain legal while choked. Choke cannot silently discard
  their correlation, and unchoke does not multiply the independent ceiling;
- a peer to which RSTorrent sends a piece can immediately request applicable
  leaf hashes. The upload owner must serve that proof even if it requires one
  bounded read/hash job;
- a valid incomplete restart has raw info and candidate have bits but no
  sparse hash authority. It must not advertise those bits until refetch plus
  verification succeeds;
- a complete file may rebuild its block and piece tree locally, but the result
  becomes authority only when its root equals the authenticated file root;
- missing, corrupt, stale-owner, or wrong-geometry candidate state never
  borrows authority from the magnet, have bitmap, filename, length, or a
  truncated identity; and
- cancellation during metadata, request dispatch, proof validation, payload
  hashing, leaf diagnosis, reconstruction, upload hashing, or publication
  cannot mutate a replacement generation.

## Accepted Runtime And Module Shape

### Magnet identity and metadata admission

Replace `Magnet`'s v1-only `[u8; 20]` field with an explicit supported full
identity enum. Pure-v1 retains its current parser and behavior. Pure-v2
accepts only the 34-byte multihash value represented as `1220` plus 64 hex
digits. A magnet carrying both identity versions remains a typed
`UnsupportedHybrid` result rather than silently selecting one.

Canonical and synthesized export select the identity's protocol form:

```text
v1 -> xt=urn:btih:<40 lowercase hex>
v2 -> xt=urn:btmh:1220<64 lowercase hex>
```

Verbatim source export retains the existing digest/length validation. A valid
v2 source is reparsed and must contain the same full v2 identity. Fallback
canonicalization and synthesis preserve the existing bounded tracker,
peer-hint, and select-only policy. No client-specific export route is added.

Generalize the existing torrent-owned BEP 9 assembler rather than creating a
v2 metadata downloader. It continues to own at most 30 MiB, exact 16-KiB
blocks, cross-peer contributors, rejection, retry, and cancellation. At
completion, the expected full identity selects SHA-1 or SHA-256 over the exact
assembled info bytes. Pure-v2 then constructs the strict info-only BEP 52
descriptor and rejects any discovered v1 or hybrid shape.

### Geometry and authenticated hash knowledge

Refactor the current complete-source `TorrentContent::V2` boundary into two
plain task-free facts:

```text
V2ContentDescriptor
  exact raw info + full identity + file roots + V2TorrentLayout

V2HashCatalog
  authenticated piece roots + minimal proof nodes + volatile leaf evidence
  query: Known(expected piece plan) | Missing(hash need)
```

Complete outer `.torrent` input seeds the catalog from its already-validated
`CompletePieceLayers`. Magnet metadata starts with file roots and only the
direct one-piece expected plans. This is a concrete boundary improvement over
making `CompletePieceLayers` optional inside the content descriptor.

The catalog owns deterministic proof checking, idempotent insertion, conflict
rejection, expected-piece queries, server response construction, and compact
resource accounting. It owns no socket, clock, task, filesystem handle, or
persistence transaction. The torrent coordinator owns when to ask peers and
when accepted facts may be used.

### Integrity state and data before hashes

The scheduler observes an explicit per-piece readiness shape:

```text
Absent + MissingHash
  -> authenticate expected piece root
  -> Requestable
  -> payload blocks present
  -> hash pending / durability pending
  -> VerifiedHave

CandidatePresent + MissingHash
  -> authenticate expected piece root
  -> hash existing payload
  -> VerifiedHave | repair/redownload
```

`CandidatePresent` is not a second kind of have. It only records that storage
may contain useful bytes. Progress, bitfield, have, streaming, publication,
and upload continue to consume `VerifiedHave` alone.

Normal peer scheduling is hash-first. This avoids spending payload bandwidth
on pieces RSTorrent cannot yet verify. The candidate path exists for restart,
recheck, and bounded late work, not as the common download policy.

### Sparse knowledge and restart policy

Sparse authenticated hash knowledge is volatile in Tactical `155`. Do not
add a SQLite hash blob, sidecar tree, second have format, or migration.
Schema 19 already stores the opaque owner, full v2 identity, exact magnet,
authenticated raw info, fingerprint, selection, and candidate have bitmap.

On restart:

- complete-source v2 reconstructs the catalog from retained complete outer
  piece layers, preserving Tactical `151` behavior;
- a one-piece magnet file uses its authenticated file root directly;
- an incomplete multi-piece magnet file refetches the needed piece-layer
  ranges and rehashes candidate have pieces before advertising them; and
- a complete multi-piece file may stream all 16-KiB block hashes, reconstruct
  its tree, compare the file root, and restore its local piece layer without
  a peer. A root mismatch leaves pieces non-have and enters bounded repair.

No candidate bytes are deleted merely because hash knowledge was not
persisted. Missing peers leave a truthful waiting-for-hashes state. Remove
still cleans only the exact managed artifacts selected by the existing data
policy. The unreleased project needs no compatibility migration; if
implementation finds schema 19 unable to represent authenticated raw v2 info,
a fresh fail-closed schema epoch and exact pre-task reset is authorized, while
published payload and user-selected roots remain outside incidental deletion.

## Hash Wire, Scheduling, And Upload Contract

### Runtime-free messages

Add an explicit `HashRequest` value containing file root, base, index, count,
and proof layers. `PeerMessage` gains request, hashes, and reject cases. The
decoder recognizes IDs 21--23 only inside a negotiated v2 connection; a v1
connection closes with the existing unsupported/invalid-message policy.

Request and reject frames have an exact 49-byte length-prefix value. Hashes
frames have that header plus a checked multiple of 32 bytes. The decoder
computes the exact proof-hash count before allocation and permits no more than
512 base hashes plus 35 proof hashes: 547 hashes, 17,504 hash bytes, and a
17,553-byte length-prefix value. The existing 64-KiB decoder input ceiling
remains unchanged; hash frames receive their own bound instead of widening
all ordinary core frames.

Outbound requests require:

- count in `2..=512`, power of two;
- index a multiple of count;
- checked range within the padded layer width;
- base equal to zero or the torrent piece layer;
- proof layers no greater than the file shape and Tactical `146`'s maximum
  of 35; and
- a nonempty known file root whose tree has a requestable layer.

Inbound hash service applies the same checks, except it also accepts count one
for a known in-range leaf or piece-layer request. The pinned libtorrent picker
can emit this shape for a one-hash tail and its validator tests explicitly
accept it even though BEP 52 says at least two. RSTorrent never emits count
one, never widens the 512 ceiling, and tests this exception separately.

A malformed frame closes the peer. A well-framed, in-range request for an
unsupported base or currently unavailable hash range receives hash reject.
Unknown roots and shapes that cannot be correlated to the torrent receive
reject without disclosing another torrent's state.

### Request ownership and retry

The torrent coordinator owns logical hash needs. Each peer generation owns
only its attempts and wire sends:

- at most two outstanding attempts per peer generation;
- at most 16 outstanding attempts per torrent generation;
- at most two peer attempts for one logical range;
- one three-second minimum interval before a stalled logical range may be
  duplicated to another eligible peer; and
- the existing peer inactivity/termination owner closes and releases attempts
  that never answer.

Piece-layer ranges are at most 512 hashes and are selected only from a peer
whose bitfield includes at least one relevant wanted or candidate piece.
Selection favors missing expected hashes that unblock wanted payload, then
candidate verification, then bounded serving completeness. It does not fetch
all skipped-file layers speculatively.

Reject or disconnect releases that attempt immediately. A timed-out attempt
remains correlated until response or peer termination while one duplicate may
run elsewhere. The first valid proof commits idempotently; a later valid
correlated result is a no-op. An invalid proof fails only its source attempt,
records integrity evidence, and closes that peer generation. Unsolicited or
tuple-mismatched responses and rejects close without changing the logical
need.

### Hash upload

The existing active/completed seed registration exposes the same catalog used
for verification. It may serve:

- known piece-layer ranges and proofs from complete source or authenticated
  sparse knowledge; and
- leaf-layer ranges for announced pieces by hashing verified payload through
  the existing positional storage owner and combining it with authenticated
  piece-to-file-root knowledge.

At most two hash-service requests are admitted per peer and eight across one
torrent. On-disk leaf service uses the existing shared eight upload slots,
ten read jobs, descriptor limits, transfer rate owner, cancellation, and
generation fence. It streams fixed 16-KiB reads; piece length never becomes a
single allocation.

Every valid request is answered with hashes or reject. RSTorrent must not
announce a piece if it cannot prove the hash ranges required for that piece.
If it services a payload request, an immediately associated valid base-zero
request for that range is mandatory work rather than an optional reject.

## Verification, Corruption, And Attribution

Once the catalog returns a known expected plan, Tactical `151`'s streamed
piece verification and durability order remain authoritative. A newly
downloaded piece cannot become have before expected-root authentication,
payload hash success, storage sync, and have checkpoint.

On piece mismatch, enqueue at most one active leaf-diagnosis piece per torrent.
Request leaf hashes in aligned power-of-two ranges of no more than 512. Compare
authenticated leaf hashes with stored block hashes and the existing retained
connection-generation contributor map:

- exact bad blocks are reset and rerequested;
- good blocks and their storage bytes remain;
- only contributors of authenticated bad blocks receive known-bad evidence;
- conflicts above the block layer or insufficient attribution remain
  ambiguous; and
- rejection, peer loss, or the ordinary progress deadline falls back to the
  existing whole-piece reset, so diagnostics cannot stall content forever.

Leaf hashes are volatile diagnostic/recovery facts. They are released after
the piece verifies or resets and do not become a persistent per-payload-block
index. This caps retained attribution by the current active-piece and buffered
payload owners rather than multiplying it by total content size.

## Owner, Task, Cancellation, And Dependency Map

```text
runtime-free protocol
  Magnet identity -> ParsedInfo -> V2ContentDescriptor -> V2HashCatalog
  HashRequest/messages -> exact bounds -> Merkle proof transitions
               no task, socket, filesystem, clock, or database
                                  |
                                  v
application transaction / SessionStore
  source + full identity + raw info + selection + candidate have
                                  |
                                  v
one torrent generation / existing supervisor
       metadata owner       integrity coordinator       storage owner
       SHA-256 BEP 9        needs/catalog/attempts      candidate/hash/sync
             |                       |                         |
             v                       v                         v
       peer generations ---- messages 21--23 -------- bounded I/O jobs
             |                       |                         |
             +---------- cancel + join + generation fence ----+
                                  |
                                  v
active/completed registration
  metadata + authenticated hash service + verified payload service
```

No second torrent supervisor, metadata task family, filesystem owner, or
persistence actor is authorized. The existing coordinator gains one task-free
catalog and bounded hash-attempt state. Peer tasks parse and report messages
but cannot mutate shared Merkle truth independently. Storage owns every read,
hash, write, sync, reconstruction, and publication job.

On pause, selection replacement, force recheck, remove, application shutdown,
or torrent generation replacement, stop new metadata/hash/payload work,
cancel peer attempts and storage jobs, drain already-admitted results through
generation fences, join children, and only then expose terminal state. A late
valid proof from an old generation is still stale.

## Security, Integrity, And Resource Invariants

- The full 32-byte v2 identity is authoritative. Its 20-byte truncation is a
  tagged discovery and wire key only.
- Only exact authenticated raw info creates file roots. Peer-supplied piece
  or leaf hashes never become authority without a complete valid proof to one
  of those roots.
- Missing expected hashes and present payload are independent facts. Neither
  a have bitmap, filename, size, prior contributor, nor completed write may
  substitute for authenticated expected integrity.
- The current 16-KiB/128-parameter magnet, 32 peer-hint, 32 tracker, 4,096
  select-range, 30-MiB BEP 9 info, 2,097,152-piece, 312,498 peer-file, path,
  depth, token, and arithmetic limits remain in force.
- RSTorrent requests 2--512 base hashes; bounded inbound compatibility may
  contain one. A response has at most 35 proof hashes and 17,504 hash bytes.
  Exact response size is derived before allocation.
- Two attempts per peer, 16 per torrent, two duplicate attempts per logical
  range, two inbound requests per peer, and eight inbound hash-service jobs
  per torrent are hard ceilings.
- Sparse catalog growth is bounded to at most 2,097,152 authenticated real
  piece roots plus 131,072 retained proof nodes. Raw hashes therefore retain
  at most 68 MiB and the complete catalog, including range indices and
  bitsets, at most 80 MiB per torrent. Deterministic padding nodes are derived
  rather than retained. Reject resource exhaustion before mutation; do not
  clone the catalog per peer, checker, storage job, or application snapshot.
- Only one leaf-diagnosis piece is active per torrent. Each message retains no
  more than 547 hashes, verification reads remain 16 KiB, and no whole piece
  or file is allocated for hashing or reconstruction.
- Hash requests do not consume ordinary payload request slots, but both use
  the established torrent/session rate and task owners. Choked peers cannot
  create an unbounded parallel hash queue.
- Existing desktop and Android payload, active-piece, storage-intake, peer,
  dial, task, descriptor, read-job, writer, and upload ceilings remain hard.
  V2 hash exchange does not multiply them by file count or peer count.
- Proof, payload, persistence, and upload results are generation-fenced.
  Checked arithmetic precedes every index, layer, count, offset, conversion,
  allocation, and mutation.
- Diagnostics may expose identities already intended for product display and
  bounded counters, not raw metadata, proof material, payload, private paths,
  or peer-controlled unbounded strings.

Stage 0 must add a generated maximum-geometry resource test before the common
path. It must measure compact catalog construction, insertion, lookup,
conflict, release, and server response without materializing payload. If the
80-MiB ceiling is not viable on the Android profile, tighten the admitted
sparse-node limit and return a typed resource error; do not silently evict
hashes that currently authorize have. Raising a recorded ceiling or adding
durable sparse state requires tactical reconciliation.

## Required Observability

Extend existing structured state with:

- protocol identity version and metadata hash algorithm;
- metadata bytes/blocks, contributing peers, SHA-256 pass/failure, and parsed
  pure-v2 rejection reason;
- per-torrent files needing piece hashes, logical hash needs, known piece
  roots, candidate pieces, and verification-ready pieces;
- hash requests sent/received, hashes/rejects sent/received, retries,
  duplicates, timeouts, unsolicited/mismatched responses, bad proofs, and
  peers closed for hash integrity;
- current/high-water peer attempts, torrent attempts, retained sparse nodes,
  retained hash bytes, response bytes, leaf-diagnosis pieces, and hash-service
  jobs;
- restart recovery source: complete outer layers, direct file root, peer
  refetch, or local complete-file reconstruction;
- candidate bytes/pieces promoted, repaired, reset, or still waiting for
  hashes;
- exact bad blocks and contributor count without exposing payload or proof
  hashes; and
- terminal zero metadata peers, hash attempts, diagnosis jobs, hash-service
  jobs, storage jobs, peer generations, seed registrations, and run-owned
  temporary artifacts.

Reuse application snapshots, Files/progress views, existing error/log
channels, and protocol identity DTOs. Do not add a separate v2 event stream or
present sparse-node counts as user-facing download progress.

## Application And Platform Contract

The existing add-magnet command accepts pure-v2 text and returns the same
opaque torrent ID and duplicate result. The existing BEP 53 select-only
intent applies after v2 metadata reveals canonical real-file indices. A
full-v2 duplicate is a no-op unless explicit selection intent promotes files.
A mixed identity source receives a typed unsupported-hybrid result.

The source-aware export command returns a verified verbatim v2 magnet where
possible and canonical/synthesized `btmh` otherwise. Complete-source pure-v2
torrents can synthesize a magnet from their full v2 identity. This tactical
does not synthesize a complete outer `.torrent` from acquired sparse hashes.

Pause/resume, force recheck, archive/restore, file priority, remove, open,
stream, and status retain their semantic commands. Waiting-for-hashes and
candidate verification must be truthful through existing state/error
surfaces; no v2-only toggle or Merkle-tree UI is added.

`TorrentProtocolIdentities` already represents full v2 identity. Change the
generated TypeScript, Kotlin, or Swift contract only for a semantic fact the
clients actually consume. Regenerate every boundary when a crossing Rust type
changes.

Required platform evidence is:

- authenticated headless web add/export, selected transfer, waiting state,
  completion, restart, and removal through the production application;
- Tauri adapter tests and build without launching a visible window;
- iOS generated-boundary tests and unsigned simulator/development build,
  proportional to unchanged in-process engine behavior;
- Android x86_64 and arm64-v8a native builds; and
- one owned API 34 no-window AVD run through the real application and SAF
  owner, covering pure-v2 magnet add, selective file indices, metadata/hash
  acquisition, candidate restart recovery, exact publication, bounded hash
  upload, and cleanup.

No physical device is required unless a new provider/lifecycle behavior or an
AVD-only ambiguity appears. Android parity may not be replaced by a host-only
Rust test because hash scheduling and persistence affect the in-process
engine on that surface.

## Implementation Stages And Commit Gates

### Stage 0: Activation, source reconfirmation, and resource baseline

- Reconfirm BEP, libtorrent, and read-only JSTorrent revisions and exact paths.
- Record the pre-change v1 and complete-source pure-v2 focused gates.
- Add maximum-shape catalog and hash-frame resource tests before runtime work.
- Reconcile any source discovery that changes this plan before continuing.

Gate: the tactical remains decision-complete, the baseline is green, and
numeric resource ceilings fail before unbounded allocation.

### Stage 1: Typed `btmh` intake, export, and SHA-256 metadata

- Generalize magnet identity without changing v1 behavior.
- Extend source verification, canonicalization, synthesis, duplicate lookup,
  persistence, resume, and generated consumers.
- Generalize the one existing BEP 9 owner to SHA-256 exact-info validation and
  strict pure-v2 info-only admission.
- Reject mixed identity and metadata-discovered hybrid before content work.

Gate: deterministic/store/application tests cover exact forms, failure,
restart, export fidelity, selection intent, and v1 regressions.

### Stage 2: Descriptor/catalog split and pure hash wire

- Separate immutable v2 content geometry from authenticated hash knowledge.
- Add deterministic sparse insertion, proof verification, conflict, missing-
  hash query, response construction, and compact accounting.
- Add message 21--23 codecs, exact sizing, hostile bounds, and negotiated-v2
  dispatch while retaining fail-closed v1 behavior.

Gate: runtime-free vectors agree with independently generated oracle values;
maximum frames and trees remain within recorded bounds.

### Stage 3: Hash scheduling and candidate verification

- Compose logical needs, peer bitfields, bounded attempts, retry, rejection,
  timeout, duplication, and cancellation into the existing torrent owner.
- Gate payload scheduling on known expected roots.
- Restore candidate payload through refetch and verification.
- Implement complete-file local reconstruction and root validation.

Gate: scripted peers prove normal, rejected, stalled, disconnected, late,
duplicate, unsolicited, mismatched, bad-proof, restart, and no-peer complete-
file cases with terminal zero work.

### Stage 4: Leaf diagnosis, upload, and seeding

- Acquire bounded leaf ranges after a piece failure.
- Repair exact bad blocks and preserve existing good blocks/contributors.
- Add catalog and on-demand leaf hash service to initiated and accepted v2
  peers under existing upload/storage budgets.
- Prove the base-zero-after-piece obligation and conservative fallback.

Gate: corruption, mixed contributor, reject/stall fallback, active upload,
completed seeding, cancellation, and resource evidence pass.

### Stage 5: Application and first-party platforms

- Carry v2 magnets and waiting/recovery truth through ordinary application
  operations and source-aware export.
- Regenerate contracts only as needed.
- Run headless web, Tauri, both Android targets, API 34 AVD, and proportional
  iOS build/boundary gates.

Gate: no platform forks protocol/integrity policy and exact managed cleanup
passes.

### Stage 6: Controlled interoperability and closure

- RSTorrent leecher: add a `btmh` magnet and acquire metadata, piece hashes,
  selected payload, and leaf recovery from a pinned-libtorrent seed.
- RSTorrent seed: start from complete source or a completed magnet result;
  have pinned libtorrent add the `btmh` magnet and obtain metadata, hashes,
  and exact payload from RSTorrent.
- Run the proportional transport/discovery matrix, maximum resource profiles,
  full repository gates, and documentation reconciliation.

Gate: both roles independently compare exact payload, all owned processes and
artifacts terminate, and the protocol ledger claims only the demonstrated
pure-v2 magnet subset.

## Validation Matrix

### Deterministic protocol and state

- `btmh` positive/negative/canonical/hybrid/duplicate/select-only vectors;
- SHA-256 exact-info assembly, contributor, limits, malformed and wrong-format
  metadata;
- one-block, one-piece, multi-piece, selected/skipped, empty, tail-padded, and
  maximum-height geometry;
- request/reject exact framing and hashes computed length for fragments,
  coalesced messages, signed-field attacks, oversized frames, the isolated
  inbound count-one compatibility case, and v1 peers;
- sparse proof insertion, overlap, duplicate, conflict, invalid proof,
  padding, missing query, compact release, and maximum resource profile;
- candidate-present state transitions and generation fencing.

### Scripted runtime, storage, and failure

- multi-peer hash range selection, peer availability, reject, timeout,
  duplicate retry, disconnect, late valid result, bad proof, unsolicited and
  mismatched response;
- payload cannot be requested before expected hash authority;
- preexisting candidate payload passes or fails after hashes arrive;
- valid sparse state, process restart without sparse state, incomplete
  refetch, complete-file local reconstruction, root mismatch, and no-peer
  waiting;
- exact corrupt-block diagnosis across one and multiple contributors, good-
  block retention, leaf reject/stall whole-piece fallback;
- initiated/accepted, choked/unchoked, piece-layer, leaf-layer, active, and
  completed hash upload;
- pause, force recheck, selection change, remove, and shutdown at every
  metadata/hash/storage phase with terminal counters.

### Controlled pinned-libtorrent interoperability

The minimum independent matrix is:

1. direct-TCP RSTorrent leecher from a pinned-libtorrent seed using only a
   `btmh` magnet, one `x.pe` hint, and `so` for an aligned multi-file fixture;
   tracker and DHT discovery are disabled, a selected multi-piece file
   requires messages 21--22 before payload, a selected one-piece file uses
   its authenticated file root directly, and a skipped multi-piece file
   receives neither piece-layer nor payload requests;
2. promote that skipped file in the same session, acquire only its newly
   required hash ranges and payload, then restart once with candidate data and
   prove selection persistence plus conservative hash refetch;
3. direct-TCP pinned-libtorrent leecher from RSTorrent using only the same
   `btmh` plus `x.pe`, proving RSTorrent's BEP 9, messages 21--23, payload,
   accepted-peer service, and no tracker/DHT masking;
4. one default-uTP DHT-only RSTorrent magnet download using the existing
   tagged v2 routing; and
5. one forced-RC4 TCP RSTorrent magnet download to preserve versioned MSE
   routing.

At least one run includes a rejected/stalled hash peer before successful
failover, one includes candidate data before hashes, and one corrupt-payload
run proves leaf diagnosis. Exact payload digests, wire message counts,
resource highs, and terminal cleanup are required. A public swarm is optional
context and cannot replace controlled evidence.

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

Also run focused interoperability, Tauri adapter/build, both Android native
targets, owned API 34 AVD application evidence, and applicable iOS generated-
boundary/build commands from `DEVELOPMENT.md`. Report exact commands and do
not imply an unrun transport, platform, or public-network gate passed.

## Non-Goals And Deliberate Deferrals

- Mixed `btih`/`btmh` magnet admission, metadata-discovered hybrid adoption,
  live owner merging, hybrid reserved-bit upgrade, dual swarms, or dual
  verification.
- Durable sparse Merkle trees, a hash-cache artifact, fast incomplete restart
  without refetch, or a compatibility migration for unreleased state.
- Synthesis/export of a complete outer `.torrent` from acquired sparse
  knowledge.
- V2 or hybrid torrent creation, creator ordering, piece-size selection, or
  first-party source generation.
- Speculative fetching of every skipped-file hash, arbitrary Merkle base
  layers beyond leaf and piece, or persistent per-block leaf catalogs.
- New choking, tit-for-tat, parole, long-term reputation, or general peer-
  scoring policy. Exact authenticated bad-block attribution composes with the
  current bounded integrity evidence only.
- Web seeds, hole punching, local discovery, IPv6/uTP expansion,
  MSE-over-uTP, transport racing, new NAT policy, or a performance-parity
  campaign.
- A new engine dependency, native host, daemon, server, IPC hash service, or
  platform implementation of protocol/hashing policy.
- A Merkle UI, v2 settings, new file-priority scale, or presentation redesign.
- Public-swarm, remote-machine, physical-device, release, publish, tag, or
  push work as a completion requirement.
- Claims for all BEP 52, hybrid torrents, creation, or durable sparse resume.

Stage 5 of the owning topic remains hybrid dual-swarm closure. Torrent
creation remains a separate later capability. Durable sparse persistence may
be reconsidered only after measured incomplete-restart cost justifies its
format and lifecycle complexity.

## Escalation Gates

Routine implementation decisions inside this document are authorized once
work begins. Stop for maintainer direction if implementation would:

- accept mixed identity or hybrid metadata, merge live owners, or weaken full
  identity collision handling;
- request or expose payload without authenticated expected hash authority;
- persist sparse Merkle state or add a schema/artifact format beyond the
  explicit refetch/reconstruct policy;
- raise the hash frame, request, proof, node, task, memory, peer, or upload
  ceilings recorded here;
- add a new crypto, runtime, storage, engine, or platform dependency with
  meaningful tradeoffs;
- create a second metadata, peer, checker, filesystem, persistence, or
  application owner;
- delete published payload or user-selected root content during reset or
  recovery;
- move peer, Merkle, payload, or hashing work across a Kotlin, Swift,
  JavaScript, IPC, or daemon boundary;
- weaken Android first-party engine parity; or
- require external publication, public-network, remote-machine, or physical-
  device side effects not already authorized.

A fresh fail-closed reset of exact RSTorrent-owned unreleased catalog and
managed staging state is already authorized if schema 19 proves inadequate.
It must resolve and report exact targets and leave external/published payload
untouched.

## Commit And Evidence Plan

Use logical commits that leave the repository buildable and preserve v1:

1. activate Tactical `155`, reconfirm sources, and add resource baselines;
2. typed `btmh` magnet intake/export and SHA-256 BEP 9 metadata;
3. descriptor/catalog split plus deterministic sparse proof state;
4. messages 21--23 codecs and hostile bounds;
5. hash scheduling, candidate payload, restart refetch/reconstruction;
6. leaf diagnosis, authenticated hash upload, and seeding;
7. application/generated clients and Android/platform composition;
8. controlled pinned-libtorrent two-role evidence; and
9. full regression, resource record, topic/ledger closure, and cleanup.

Each nontrivial implementation commit records its validation and includes
`Topic: bittorrent-v2-and-hybrid`. Evidence belongs in this document as it
lands. Temporary oracle torrents, payloads, profiles, captures, logs,
packages, AVD/simulator state, and subprocesses are removed before closure
unless a bounded artifact is explicitly retained and linked.

## Execution Record

Implementation has not begun. The first action is Stage 0 source
reconfirmation, baseline, and maximum-shape resource evidence. Do not infer
support from this decision-complete plan.
