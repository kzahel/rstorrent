# Tactical 081: V1 Torrent Byte Intake

Status: Planned and authorized from maintainer direction on 2026-08-04.
Amended on 2026-08-04 to make pinned libtorrent `v2.0.13` the compatibility
lead for large v1 metadata, geometry, and source intake, while requiring
measured comparison where its compact representations do not map directly to
RSTorrent. Implementation has not started. This planning work changes
documentation only.

Topics: `client-persistence`, `application-control`,
`application-connection-architecture`, `application-view-api`,
`tracker-discovery`, `protocol-support`, `client-surfaces`,
`download-correctness`, `android-saf-storage`, `capability-readiness`

Dependencies: completed Tacticals
[`060`](060-multiplexed-application-websocket.md),
[`073`](073-unified-storage-and-complete-recheck.md),
[`074`](074-context-specific-metainfo-limits.md),
[`075`](075-ephemeral-application-state.md), and
[`076`](076-authenticated-private-web-host.md) establish the application
connection, unified v1 storage path, context-specific parser limits, bounded
ephemeral persistence, and authenticated private browser host used by this
slice. Completed Tacticals [`011`](011-one-shot-udp-tracker.md),
[`014`](014-scheduled-udp-tracker-lifecycle.md), and
[`043`](043-live-tracker-inspection.md) establish bounded UDP tracker
retention, scheduling, and inspection.

## Decision And Motivation

Add one bounded v1 `.torrent` byte-intake operation to the application
service and adapt it to the ordinary browser WebSocket, an HTTP automation
endpoint, and the in-process Tauri desktop connection. The caller supplies
bytes; no portable command supplies a local path, file descriptor, document
URI, or remote URL.

The accepted boundary separates three kinds of data:

- **operational metadata** is the exact hash-authorized `info` dictionary,
  normalized tracker tiers and peer hints, file selection, verified-piece
  state, and user/runtime intent needed to operate and restart the torrent;
- **original source** is the bounded verbatim magnet string or complete outer
  `.torrent` byte sequence retained for provenance and later exact export but
  never required for runtime restart; and
- **synthesized export** is a later magnet or `.torrent` reconstructed from
  operational state and must not be described as the original source.

SQLite remains the one application-state authority in durable and ephemeral
modes. Durable mode retains an original `.torrent` as a BLOB in a source table,
not beside payload data. Ephemeral mode stores the same source row in its
bounded private in-memory database and loses it with the rest of that service
lifetime. This slice does not add a profile-private sidecar blob store,
payload-adjacent copy, export directory, or second catalog authority.

The BEP 9 transfer profile uses pinned libtorrent's 30-MiB
`max_metadata_size` default. Explicit caller-owned outer input and durable
`raw_info` use a separate provisional 64-MiB application bound. Libtorrent's
caller-owned span constructor has no equivalent byte cap, and its adopted
2,097,152-piece limit alone permits a 40-MiB v1 `pieces` string, so the BEP 9
ceiling cannot also be the explicit or durable ceiling. Gate 1 must confirm
64 MiB against identical generated inputs and measured resource use before
later gates rely on it. Explicit selection or an authenticated private host
changes the applicable resource profile, not the hostile-input posture: every
byte and unit of work remains bounded and validated before durable state or
engine work changes.

Where libtorrent exposes an apples-to-apples protocol or integer limit, this
tactical adopts it. Where libtorrent relies on compact borrowed token storage,
platform-specific filename sanitization, or an indirect input bound instead
of a file, collection, path, tracker, or URL count, implementation must run
the same generated boundary cases through both implementations, measure
retained memory and downstream work, and record the closest safe RSTorrent
policy. A current conservative constant may not survive merely because the
representations differ.

## Desired Outcome And Stopping Condition

The tactical stops when all of the following are true:

- one transport-neutral application operation accepts bounded v1 outer
  metainfo bytes plus the existing request identity, expected revision,
  storage-root, start-content, and file-selection intent;
- exact source length and SHA-256 participate in durable request replay and
  conflict detection without placing the source bytes in the JSON command or
  request-receipt representation;
- one atomic session transaction creates the torrent identity, exact
  `raw_info`, zeroed verified-piece state, normalized source intent, metainfo
  tracker tiers, original source BLOB, file selection, and ordinary lifecycle
  state;
- a malformed, unsupported, oversized, duplicate, stale, digest-mismatched,
  canceled, or resource-exhausted intake changes neither revision nor torrent
  state and leaves no source artifact;
- schema migration preserves every existing torrent, labels its retained
  magnet canonicalized unless an unambiguous successful add receipt proves the
  verbatim submission, and removes the assumption that every torrent has a
  magnet source;
- new magnet adds retain the bounded verbatim submitted URI separately from
  normalized operational hints, while existing canonical-only rows remain
  truthfully distinguishable;
- outer source upload, retained original source, explicit exact `info`, and
  durable exact `info` accept up to 64 MiB, while BEP 9 peer metadata remains
  independently capped at 30 MiB;
- a BEP 9 handshake size at most 4 MiB may establish allocation geometry, a
  larger positive advertisement remains only an untrusted hint, and piece
  zero may establish an authoritative bounded `total_size` up to 30 MiB;
- BEP 9 completion moves one bounded assembly buffer into validation and
  persistence without constructing a second full-size concatenation, and
  upload flow can serve all 1,920 blocks of a maximum received transfer or all
  4,096 blocks of locally imported metadata up to 64 MiB without a lifetime
  request cap;
- v1 piece count accepts libtorrent's `2,097,152` default and piece length
  accepts `536,854,528` bytes without eagerly constructing state proportional
  to every payload block or retaining one full piece in memory;
- BEP 9/durable decode depth accepts 200 and explicit outer decode depth
  accepts 100, while retained-tree work, collection, file, path, tracker, and
  URL policies are calibrated against pinned libtorrent cases and measured
  before their final numeric values land;
- file selection, file progress, tracker inspection, Android capability
  acquisition, peer availability, part-file allocation, recheck, and content
  scheduling operate through compact or paged state at the accepted
  cardinalities rather than inheriting the current 4,096-file or 52,428-piece
  ceiling;
- restart re-hashes and re-parses exact `raw_info`, restores normalized
  operational discovery, and neither selects nor parses the original outer
  source BLOB;
- BEP 12 `announce-list` tier order and `announce` fallback are normalized
  under the calibrated outer-input work bound, all accepted valid unique
  trackers are persisted, at most eight UDP operations execute concurrently,
  and retained HTTP/HTTPS trackers are truthfully visible through a paged view
  as configured but unsupported rather than silently discarded;
- a private torrent with no supported discovery source imports successfully
  but projects an actionable unsupported-discovery explanation rather than
  pretending to download or treating source validity as tracker reachability;
- the ordinary browser connection accepts one declared, bounded binary upload
  of up to 64 MiB without base64 while preserving the existing 64-KiB
  text-frame bound;
- an authenticated raw HTTP endpoint invokes the same semantic operation for
  automation without becoming the ordinary interactive browser lane;
- Tauri accepts a raw IPC body and invokes the same semantic operation without
  a loopback listener, path handoff, base64 expansion, or JSON number array;
- durable and ephemeral metadata-only, running, replay, restart, and
  exhaustion scenarios pass;
- a metadata-only maximum-geometry torrent remains cheap, while representative
  high-piece, high-file, long-path, many-tracker, and maximum-piece content
  cases prove bounded lazy execution and presentation;
- controlled libtorrent evidence downloads and verifies content from an
  imported metainfo source through its outer UDP tracker configuration; and
- the validation matrix, owning topics, readiness scoreboard, generated
  contracts, and this execution record contain the exact final evidence.

The result is an API-level `.torrent` intake capability usable by the bounded
private browser host, Tauri adapter, and scripted HTTP clients. A visible
browser/Tauri file picker is deliberately the next presentation slice.

## Existing Baseline And Gap

The current application command union accepts `AddMagnet` but no byte-bearing
source. The `torrents` table requires `magnet TEXT NOT NULL`, stores optional
one-MiB `raw_info`, and reconstructs supported peer hints and UDP trackers by
re-parsing a canonicalized magnet. The submitted magnet's parameter spelling,
ordering, display name, unsupported fields, and equivalent encodings are not
retained verbatim. The current BEP 9 state also rejects metadata above one MiB
and its uploader applies a 256-request lifetime ceiling, which cannot serve
more than four MiB of unique 16-KiB blocks.

`Metainfo::from_bytes_with_limits` can already identify and parse the exact
outer `info` span. Tactical `074` proves a parser-only 16-MiB explicit-import
profile and keeps generic bencode, BEP 9, and durable `raw_info` at independent
call sites. The session schema remains version 7 and constrains `raw_info` to
one MiB, so a large explicit parse cannot yet become durable application
state.

The ordinary browser uses one multiplexed `/api/v1/connect` WebSocket. Its
text messages, frames, and handshake are capped at 64 KiB, and any binary
message is currently fatal. HTTP routes share the same 64-KiB default body
limit. Tauri invokes typed JSON commands directly and has no byte-intake
command. The pinned Tauri 2.11 API supports raw `InvokeBody` bytes and exposes
them through `tauri::ipc::Request` on desktop; this slice must use that path
instead of serializing a byte array as JSON.

Tactical `076` already permits one maintainer-operated same-origin private
host behind HTTPS and Basic authentication. This intake operation may run
there under the existing authentication, exact-Origin, connection, and
storage-root capabilities. It does not broaden that host into a public remote
administration product.

## Accepted Source And Persistence Model

### Operational state and original source are distinct

The exact `raw_info` bytes remain the cryptographic and runtime authority.
They are extracted as a slice of the accepted outer source without
re-encoding, hashed with SHA-1 for the v1 identity, parsed under the durable
profile on restart, and stored with the ordinary piece count and zeroed have
state.

Outer source bytes are provenance only. They may contain unknown keys,
comments, creation fields, web seeds, DHT bootstrap nodes, unsupported tracker
schemes, and private tracker credentials. Preserving those bytes does not
claim operational support for their fields. Runtime startup and tracker
configuration use the hash-authorized `raw_info` and normalized rows, not a
fresh parse of the original source.

Source bytes and exact magnets are sensitive bounded user data. Routine
diagnostics, snapshots, tracker errors, HTTP errors, and logs must not emit
them or unredacted tracker credentials. A later explicit export/copy operation
may reveal source data to the requesting user; this tactical adds no such
operation.

### Schema direction

Advance the session schema from version 7. Exact table and column names may
follow existing store conventions, but the resulting relational contract must
express these values:

```text
torrents
  info_hash + runtime/resume/storage state
  raw_info BLOB <= 64 MiB
  piece_count <= 2,097,152
  have_state BLOB <= 262,178 bytes
  no mandatory magnet source

torrent_source (one row per torrent in this slice)
  info_hash foreign key
  kind = magnet | metainfo
  source fidelity = verbatim | canonicalized
  exact bounded magnet text, or exact outer metainfo BLOB <= 64 MiB
  exact source byte length and SHA-256

torrent_trackers
  info_hash foreign key
  tier + position without an eight-bit tier assumption
  bounded normalized URL and transport
  source = magnet | metainfo

torrent_peer_hints
  info_hash foreign key
  bounded normalized host + port
  source = magnet
```

One source row is deliberate because duplicate info hashes remain errors.
Do not create a multi-source provenance ledger, source-merge history, mutable
tracker editor, or source replacement operation in this slice.

Migration parses each existing canonical magnet under the current bounded
magnet parser and moves its supported trackers and peer hints into normalized
rows. The bounded request-receipt table may still contain the successful
original `AddMagnet` envelope for some torrents, but receipts are evictable
retry infrastructure rather than source authority. Migration may label a
source verbatim only when one retained successful receipt unambiguously
matches the row and its parsed v1 identity; otherwise it creates a
`magnet/canonicalized` source record. New magnet intake creates a
`magnet/verbatim` source record while deriving the same normalized operational
rows. Migration is transactional and retains request receipts, revisions, raw
metadata, have state, selection, roots, archive/removal state, and every
existing restart invariant.

The migration also raises piece-count/have-state checks, removes the 4,096
file-index assumption from selection and prepared-file tables, and gives
tracker tiers/positions enough width for every calibrated accepted outer
catalog. Sparse selection may remain row-based if maximum-cardinality and
alternating-selection measurements fit the page/RSS budget; otherwise use a
versioned range/bitmap representation with strict padding and corruption
checks. Existing selection rows migrate without semantic change.

An implementation may retain a legacy column for one migration gate if doing
so makes rollback and evidence safer, but the completed read/write path may
not depend on a non-null magnet or use a canonical magnet as the only
operational tracker/peer representation.

### Database BLOB, not a sidecar

The exact outer metainfo lives in a separate table so ordinary startup,
snapshot, and resume queries do not materialize it. Its transaction, backup,
removal cascade, and ephemeral lifetime therefore match the catalog without a
cross-filesystem commit window. The accepted duplication of `raw_info` inside
the original source is simpler than prefix/suffix reconstruction,
compression, or a file-backed blob store for this bounded first slice.

SQLite page exhaustion in ephemeral mode maps to the existing typed
`resource_limit` response and rolls back the current transaction. Durable WAL
and `synchronous=FULL` behavior remain unchanged. No implementation may spill
an ephemeral source to a temporary file or silently omit exact-source
retention merely to make an otherwise oversized transaction succeed.

The current 128-MiB ephemeral main-database page budget cannot safely admit a
maximum 64-MiB source plus its independently retained `raw_info`, indexes,
rows, and SQLite page overhead. The desired default is provisionally 256 MiB,
with the metrics database retaining its independent 32-MiB budget. Gate 1
measures the maximum accepted transaction and records the final page budget;
it must cover one such torrent plus ordinary catalog overhead without turning
ephemeral mode into an unbounded store. A second maximum import may still fail
atomically with `resource_limit`.

### Removal and corruption posture

Removing a torrent under the existing application policy cascades its source,
tracker, and peer-hint rows; the source is application state, not payload
data. Keeping or deleting managed payload does not preserve the catalog row.

Original-source corruption or absence must never invalidate independently
hash-authorized `raw_info` or verified payload. It removes the ability to
claim an exact future export and should be diagnosable when source access is
explicitly requested, but startup does not inspect it. Conversely, a valid
outer source cannot authorize corrupt `raw_info`, tracker rows, or have state.

## Metainfo And Tracker Semantics

### V1 identity and structural policy

Accept only a bencoded outer dictionary containing one supported v1 `info`
dictionary. Preserve the exact info span and reject v2 or hybrid identity,
invalid piece geometry, input beyond the selected byte/work profile, unsafe
or colliding operational path projection, integer overflow, and all other
profile-specific parser errors before mutation. Exact source path byte strings
stay authorized only through `raw_info`; storage uses a separately derived
safe, deterministic path projection.

The desired limits are:

| Resource | Maximum |
| --- | ---: |
| Outer upload and retained original source | 64 MiB |
| Explicit-import exact `info` | 64 MiB |
| Durable exact `info` | 64 MiB |
| BEP 9 peer metadata | 30 MiB |
| BEP 9 size trusted from the extension handshake | 4 MiB |
| BEP 9 received metadata blocks | 1,920 at 16 KiB each |
| BEP 9 uploaded local metadata blocks | up to 4,096 at 16 KiB each |
| Outgoing BEP 9 requests per peer | 2 |
| Deferred incoming BEP 9 requests per peer | 1,024 |
| BEP 9 send-buffer deferral threshold per peer | 160 KiB |
| BEP 9/durable decode depth | 200 |
| Explicit outer decode depth | 100 |
| Pieces | 2,097,152 |
| Piece length | 536,854,528 bytes |
| Decoded work | calibrated equivalents of libtorrent's 2,500,000 peer and 3,000,000 explicit lexical tokens |
| Files and collection entries | calibrated against matched 30-MiB peer and 64-MiB explicit decode-work cases; no inherited 4,096 ceiling |
| Path components and total path bytes | calibrated safe projection; no inherited 32-component/4,096-byte source rejection |
| Valid unique metainfo trackers and tracker URL bytes | bounded by outer bytes/decode work and calibrated runtime memory; no inherited 32/2,048 truncation |
| File selection | all accepted non-padding files through compact all/none/range or paged operations; no enumerated 4,096 ceiling |
| File/tracker view snapshot | bounded page within the existing 16-MiB snapshot ceiling, with total count and stable cursor/range |
| Concurrent buffered imports per application host | 1 |
| Ephemeral main database | provisional 256 MiB; calibrated to retain one maximum source plus exact info and catalog overhead |

The explicit, BEP 9, and durable metainfo profiles no longer reuse Tactical
`074`'s depth, decoded-item, collection, file, piece, or path values. Generic
bencode, DHT, extension-message, and unrelated protocol profiles remain
unchanged. Before implementation leaves the first calibration gate, this
table must be amended with the final RSTorrent decoded-item, collection, file,
path-projection, tracker, URL, and page values plus the measured comparison
that chose them. A calibrated value may be lower than libtorrent's emergent
maximum only when a concrete RSTorrent owner cannot safely represent it; that
difference requires an explicit stopping-condition update and maintainer
direction, not silent conservatism.

The durable metainfo profile rises to 64 MiB because a durable row may now
originate from either a 64-MiB explicit import or a 30-MiB BEP 9 acquisition.
The original-source BLOB is independently capped at 64 MiB; peer acquisition
persists the exact hash-authorized `info` without manufacturing an outer
source. As in libtorrent, `max_metadata_size` is a receive limit. Metadata
above 30 MiB imported explicitly remains valid local metadata and may be
advertised and served up to the 64-MiB durable bound; a default receiver still
rejects a `total_size` above 30 MiB.

### Pinned libtorrent limit audit

The required oracle is pinned libtorrent `v2.0.13`. Its values are not one
undifferentiated parser profile:

| Boundary | Pinned libtorrent | RSTorrent decision in this tactical |
| --- | ---: | --- |
| BEP 9 authoritative `total_size` | configurable `max_metadata_size`, 30 MiB default | adopt 30 MiB |
| Extension-handshake `metadata_size` used for eager geometry | 4 MiB | adopt 4 MiB; ignore a larger hint and request piece zero rather than rejecting the peer |
| Metadata block | 16 KiB | unchanged 16 KiB |
| Outgoing requests per metadata peer | 2 | unchanged 2 |
| Deferred incoming metadata requests per peer | 1,024 | replace the 256-request lifetime ceiling with a 1,024-entry queue bound |
| Metadata send-buffer deferral threshold | 160 KiB | adopt the threshold; defer new responses while buffered bytes are at or above it |
| Locally loaded metadata upload | `max_metadata_size` is receive-only; valid local metadata is advertised and requestable at its actual size | serve validated durable metadata up to 64 MiB under the same 160-KiB/1,024-entry bounds; do not apply the 30-MiB receive cap to upload |
| Peer metadata decode tokens | 2,500,000 lexical tokens | implement and record a measured retained-tree/streaming-work equivalent; current 1,000,000 is not presumed sufficient |
| Peer metadata decode depth | 200 | adopt 200 for BEP 9 and durable exact `info` |
| Explicit decode tokens/depth | 3,000,000 lexical tokens / depth 100 | calibrate decoded work and adopt depth 100 for the outer source profile |
| Peer metadata pieces | configurable `max_piece_count`, 2,097,152 default | adopt 2,097,152 across parser, engine identity, have state, schema, and generated contracts |
| Files | no dedicated count maximum; bounded indirectly by bytes/tokens and integer representation | remove the 4,096 semantic cap; calibrate against matched 30-MiB peer and 64-MiB explicit token fixtures and make downstream state lazy or paged |
| File/buffer `.torrent` intake | file loader defaults to 10,000,000 bytes; the caller-owned span/buffer loader has no file-read cap | use a provisional 64-MiB outer transport/source and exact-info cap, then validate and record this application-owned bounded-buffer policy against matched span inputs |
| Piece length | 536,854,528 bytes | adopt exactly and prove bounded block-at-a-time transfer, storage, and hashing |
| Path shape | v1 components are sanitized and long names are shortened; no equivalent 32-component/4,096-byte source rejection cap | compare exact pinned cases and implement one deterministic portable safe projection with collision resolution instead of rejecting otherwise valid metadata solely at the old caps |
| Tracker count and URL bytes | no dedicated metainfo count or URL-byte cap inside the bounded decode input | retain every valid unique tracker admitted by the outer byte/work profile; page presentation and keep runtime operation concurrency at eight |
| File selection | file-priority vectors scale with the torrent; magnet `so` defensively caps indices at 10,000 | decouple product selection from an enumerated JSON list and support accepted file cardinality through compact/paged operations; keep magnet `so` outside this slice |
| Concurrent imports | no equivalent application-host concern | retain one buffered import per host |

The decoded-item values are not numerically interchangeable. Libtorrent
counts lexical bdecode tokens, including closing delimiters, in a compact
borrowed representation. RSTorrent currently counts retained tree nodes and
dictionary keys but not closing delimiters. The implementation comparison
must therefore include identical shallow-wide, deep, many-file, many-tracker,
many-piece, and ignored-field inputs; token/item counts; parser time; and
allocator high water. It may replace the full retained generic tree with a
compact token arena, streaming semantic projection, or another inward parser
boundary when that is required to approach the reference limits safely.

The current 4,096 collection/file, 52,428-piece, depth-32, 32-tracker,
2,048-byte URL, 32-component, and 4,096-byte path limits are implementation
gaps owned by this tactical, not deliberate post-tactical compatibility
limits.

### Required downstream owner changes

The limit audit found these concrete blockers. Raising constants without
changing their owners does not satisfy the tactical:

| Owner | Current shape that does not scale | Required direction |
| --- | --- | --- |
| Bencode/metainfo parser | A retained generic node tree with one global decoded-item/collection profile | Separate peer/durable and explicit profiles; compare identical inputs to libtorrent's token accounting; use a compact arena or direct/streaming semantic projection if required by measured memory |
| Parsed metainfo | Copies every 20-byte piece hash into a separate `Vec` and owns cloned path strings | Keep exact `raw_info` alive through a compact hash-span/index representation where practical; own one safe operational file catalog without repeated full clones |
| BEP 9 owner | One-MiB validation, per-block payload allocations, a second completion concatenation, and a 256-request upload lifetime counter | The 30-MiB single transferable receive assembly and 1,920-block state, plus queue-bounded upload of as many as 4,096 blocks from 64-MiB valid local metadata |
| Premetadata/content availability | Piece ceilings are repeated in parser, premetadata HAVE/bitfield state, engine, and session | One shared 2,097,152 capability with compact bitsets and exact spare-bit validation; no per-piece map entry merely because a peer exists |
| Content scheduler | Builds plans and `BTreeMap` state for every wanted piece and every 16-KiB payload block before transfer | Retain compact wanted/verified geometry and instantiate piece/block state only for the bounded active scheduling window; terminal cleanup releases each window generation |
| Large piece execution | One piece can contain 32,767 request blocks at libtorrent's maximum | Preserve block-at-a-time receive/write and bounded resident payload; hash from storage incrementally; never allocate one piece-sized payload buffer or enqueue every block to peers at once |
| Part file | Eager slot header plus `slots`/generation vectors and an O(piece-count) temporary free-slot scan for allocation | Measure the compact 2,097,152-piece table, remove per-allocation O(piece-count) scans, and keep restart/checkpoint operations bounded and cancellable |
| Durable store | Schema checks 52,428 pieces, 6,588 have bytes, and file indices below 4,096 | Migrate to 2,097,152 pieces, 262,178 have bytes, calibrated file indices, 64-MiB raw info, 64-MiB source, and scalable selection/tracker rows; raise and measure the ephemeral main-database budget provisionally to 256 MiB |
| Selection and storage catalog | JSON index vectors, session checks at 4,096, Android vectors at 1,024, and eager per-file descriptor planning | Add compact all/none/range semantics and paged mutations; descriptor/document acquisition remains lazy under the existing handle/request concurrency bounds |
| File progress/view | Clones the complete layout/catalog, materializes every row, and sends the whole file list in one snapshot | Share immutable catalog geometry, compute spans without temporary per-file piece vectors, materialize bounded pages, and scope keyed patches/cursors to a page/filter |
| Tracker runtime/view | Assumes at most 32 retained schedule/view rows | Persist the full bounded catalog, activate at most eight UDP operations, lazily page inspection, and avoid loading source BLOBs or all rendered rows at startup |
| Web/generated contracts | TypeScript validates at most 4,096 files and 32 trackers; full snapshots must fit 16 MiB | Publish calibrated semantic maxima separately from bounded page sizes; update Rust schema, TypeScript, Tauri, gateway, and Kotlin contracts together |
| Android adapter | Selection/materialization lists stop at 1,024 and path-backed provider work assumes eager bounded lists | Accept compact selection/catalog pages while keeping provider requests and descriptors lazy; prove metadata-only and representative content behavior without eager document creation |

The pre-implementation code audit ties those owners to these concrete change
sites:

- `rstorrent-protocol/src/{bencode,metainfo,metadata}.rs` owns the generic
  1-MiB bencode default, 16-MiB explicit profile, 1-MiB BEP 9 profile,
  4,096-file/collection cap, 52,428-piece cap, 256-MiB piece-length cap,
  path-shape caps, and 256-request metadata-upload lifetime counter;
- `rstorrent-protocol/src/storage_layout.rs` owns the cloned `TorrentLayout`,
  per-file `Vec<bool>` selection, and allocating `file_pieces()` result;
- `rstorrent-engine/src/{driver,swarm}.rs` owns `MAX_ENGINE_PIECES`, decoded
  peer availability as `Vec<bool>`, and eager `PiecePlan` plus block state for
  every wanted piece; `selective_storage.rs` repeats full layout and verified
  vectors across storage owners;
- `rstorrent-engine/src/part_file.rs` allocates one `slots` and one
  `mapping_generations` entry per piece and performs a piece-count-sized free
  slot scan from `ensure_slot`;
- `rstorrent-session/src/{have,store,control,file_views}.rs` owns the durable
  have bitmap/schema checks, 128-MiB ephemeral database cap, 4,096-entry JSON
  selection, cloned file catalog, and eager whole-catalog progress model;
- `rstorrent-session/src/views` and `rstorrent-gateway` currently publish the
  whole file/tracker catalog under one 16-MiB view snapshot rather than a
  paged query contract;
- `clients/web/src/validation.ts` hard-codes 4,096 files and 32 trackers and
  validates whole-list patches; and
- `rstorrent-android/src/lib.rs` caps selection at 1,024 and accepts eager
  path lists, so its adapter must consume the same range/page contracts while
  preserving lazy document and descriptor acquisition.

The implementation must re-run this search when Gate 1 begins. Repeated
numeric guards in migrations, generated fixtures, adapters, and tests are
part of the owning change rather than permission to leave a narrower hidden
limit.

Piece count and file count are catalog cardinalities, not permission to retain
all corresponding hot-path state. Metadata-only add/restart must not construct
content plans, part-file tables, per-peer availability, file rows, or platform
descriptors. Starting content may construct compact identity/selection state,
but payload-block state remains proportional to the active window.

Gate 1 runs pinned libtorrent and RSTorrent in isolated subprocesses over the
same generated fixtures and records source bytes, info bytes, lexical tokens,
RSTorrent decoded work, files, pieces, path bytes, trackers, wall time,
baseline-subtracted peak RSS, and retained steady state. It must record final
non-apples-to-apples limits in this document before later gates rely on them.
If an implementation cannot approach the pinned reference without exceeding
a measured safe process, SQLite, view, or platform budget, it stops with that
evidence instead of restoring an old constant silently.

### Exact source paths and safe operational paths

Libtorrent accepts many v1 names that RSTorrent currently rejects, sanitizes
invalid elements, shortens long components while attempting to preserve a
short extension, and resolves duplicate filenames. RSTorrent follows that
separation rather than treating peer bytes as filesystem paths:

- the exact bencoded path byte strings remain only in hash-authorized
  `raw_info`;
- one deterministic, platform-independent projection produces nonempty safe
  relative components for path, Tauri, and Android SAF storage;
- absolute roots, `.`/`..`, separators inside a component, NUL, invalid UTF-8,
  reserved platform names, trailing-dot/space ambiguity, normalization
  collisions, and overlong components are sanitized or shortened rather than
  escaping the selected root;
- duplicate projected paths receive stable collision suffixes derived without
  re-encoding or changing torrent identity;
- the calibrated component and total operational-path maxima are chosen from
  pinned libtorrent cases plus macOS, Windows, Linux, and Android provider
  constraints and are recorded before schema/storage work proceeds; and
- restart derives exactly the same projection from `raw_info`; persisted
  publication/prepared rows must agree or fail conservatively.

This does not require byte-for-byte equality with libtorrent's
platform-conditional displayed filenames. The comparison must classify every
reference case and justify intentional projection differences while preserving
root containment, unique file identity, and exact payload mapping.

### BEP 9 allocation and request semantics

A positive handshake `metadata_size` in `1..=4 MiB` may establish the shared
torrent geometry. An absent value or a positive value above 4 MiB does not
allocate the complete transfer and does not by itself make the peer unusable;
malformed field types retain the bounded extension-message error policy. The
owner requests piece zero and accepts its `total_size` only in `1..=30 MiB`;
all subsequent data messages must agree exactly. Values above 30 MiB are
rejected before allocating block or payload state.

The torrent owner uses one contiguous 30-MiB-maximum assembly allocation, or
an equivalent representation with the same measured high water, plus a
bounded 1,920-block receipt/source bitmap. Completion hashes and transfers
ownership of those exact bytes into parsing and persistence. It must not keep
per-block payload allocations and then concatenate them into a second
full-size buffer. Hash failure, cancellation, peer removal, and successful
handoff release the same bounded owner state observably.

On upload, `1,024` is a queue bound, not a connection-lifetime request count.
A maximum received object requires 1,920 unique requests; locally imported
metadata may require up to 4,096, and retries may legitimately increase either
number. As in libtorrent, the 30-MiB `max_metadata_size` does not cap upload of
already validated local metadata. Once the send buffer is at or above the
160-KiB threshold, later valid requests are queued up to 1,024 and excess
requests are rejected or backpressured without closing a healthy transfer
merely because its lifetime count exceeded a fixed number. Invalid indices
are rejected without entering the queue.

### BEP 12 tracker projection

Survey BEP 12 and pinned libtorrent behavior before finalizing the pure outer
projection. The accepted baseline is:

- walk `announce-list` tiers in source order;
- ignore malformed tier/entry shapes and invalid tracker URLs while
  preserving the original outer bytes;
- retain valid HTTP, HTTPS, and UDP URLs, deduplicating by normalized tracker
  identity at the first occurrence;
- compact nonempty retained tiers to monotonically increasing bounded tier
  numbers while preserving tier grouping and within-tier source order;
- use a valid top-level `announce` only when `announce-list` yields no valid
  retained tracker; and
- retain every valid unique tracker admitted by the 64-MiB outer and
  calibrated decode-work profile rather than truncating at 32.

Pinned libtorrent shuffles trackers within each tier before operation. The
RSTorrent manager may retain its deterministic/testable ordering and existing
selection policy, but it must preserve the tier groups and source attribution
and must record any intentional scheduling difference in this tactical.

Magnet trackers continue to form one synthetic tier under the magnet parser's
existing independent source profile. Metainfo UDP trackers enter the existing
schedule with `Metainfo` source and their tier. The schedule and runtime
snapshots must no longer manufacture `Magnet/tier 0` for every record. Remove
the 32-record catalog assumption: one owner may retain or lazily read the
bounded full tracker catalog, while at most eight UDP operations run
concurrently. Retries, token cache, cancellation, and session network policy
remain unchanged unless reference evidence exposes a tier correctness bug
that must be fixed at this boundary.

HTTP and HTTPS tracker protocols remain absent. Persist those configured URLs
with explicit transport/source and project a credential-redacted display
identity plus unsupported status; do not send them to the UDP manager,
classify them as failed UDP operations, expose passkey-bearing paths/query
values, or drop them. A private torrent with only unsupported trackers has no
DHT fallback and must project an actionable discovery blockage. A non-private
torrent may still progress through DHT.

`url-list`, `httpseeds`, `nodes`, comments, creation metadata, collections,
similar-torrent fields, and other outer keys are retained only in the exact
source BLOB. Operational parsing, presentation, web-seed transfer, and DHT
bootstrap effects for those keys are non-goals.

## Semantic Application Operation

Introduce a typed `AddTorrentBytesRequest` or equivalently named value with:

- application API version;
- request ID;
- optional expected revision;
- storage-root ID;
- `start_content` intent;
- compact initial selection intent: all, none, or canonical sorted
  non-overlapping wanted/skipped index ranges within the 64-KiB control
  envelope;
- exact source length; and
- exact source SHA-256.

The byte payload is an attachment to that operation, not a field in the JSON
request or ordinary `Command` union. HTTP may let the server compute fields
that the WebSocket handshake must declare, but all adapters produce the same
internal operation identity before dispatch.

Generalize the durable receipt representation only as far as necessary to
record the operation kind, normalized semantic options, source length, and
source digest. Retrying the same request ID with the same options and bytes
returns the recorded response without creating another torrent or engine
generation. Reusing it with different options, length, or digest is a request
conflict. New metainfo and magnet receipts do not retain a second exact copy
of source input; legacy receipt compatibility and any migration-time verbatim
magnet recovery remain supported.

The service validates the storage root, request envelope, declared bounds,
digest, outer metainfo, exact info hash, tracker projection, and file indices
before opening the mutation transaction. The transaction then inserts all
catalog, source, operational discovery, metadata, selection, and initial-have
rows and advances the revision once. After commit, the existing application
reconciliation path owns metadata-ready paused/running state, storage
preparation, checking, download generation, observations, and terminal
cleanup.

`start_content=false` must create no payload, staging, part, or publication
artifact. `start_content=true` follows the current common storage path and
does not bypass full recheck, selection, root capabilities, or publication
rules. Selection ranges are canonicalized and validated against parsed
non-padding files. All/none remains constant-size at maximum cardinality;
later paged range mutations can express sparse adversarial patterns without
placing one index per file in one JSON command. Request replay compares the
canonical selection mode/ranges or their collision-resistant digest.

An existing info hash returns the current duplicate error without attaching
the source, replacing trackers, satisfying a magnet's missing metadata, or
changing revision. Supplying `.torrent` metadata to an existing magnet is a
later explicit enrichment operation.

## Transport Contracts

### Shared ownership and cancellation

Each application host admits at most one buffered torrent import across its
byte-bearing adapters. The gateway shares one permit across HTTP and all
WebSocket connections. Tauri holds the equivalent permit beside its in-process
application service. Admission occurs before accepting or allocating the full
declared body.

```text
browser / HTTP / Tauri caller
  -> adapter upload admission (one buffered import)
  -> bounded source buffer + length/SHA-256 verification
  -> pure outer/source projection
  -> ApplicationService semantic mutation
       -> one SQLite transaction
       -> ordinary post-commit torrent reconciliation
```

Disconnect, timeout, invalid framing, or shutdown before a complete accepted
body drops the buffer, releases admission, and mutates nothing. Once a
complete operation has entered semantic dispatch, the application owner—not
the transport connection—owns it through commit or typed failure. A lost
response is recoverable by retrying the durable request ID. Shutdown must
cancel or join in-flight parsing/dispatch before closing SQLite and must not
commit after the service has entered its terminal state.

Potentially expensive parsing/hashing may run on a bounded blocking worker so
it does not stall the async executor. One accepted upload must have one
observable task owner and joined terminal result; do not detach unbounded
blocking work or add a generic job framework.

### WebSocket

Extend the closed application frame family with a bounded upload handshake:

```text
client text:  begin_torrent_upload(call_id, upload_id, request metadata)
server text:  torrent_upload_ready(call_id, upload_id)
client binary: exact outer metainfo bytes
server text:  ordinary correlated semantic result or typed call error
```

Names may follow existing generated conventions. The stable rules are:

- text frames remain bounded to 64 KiB and ordinary calls/views continue to
  use text JSON;
- WebSocket frame/message configuration permits one binary message of at most
  64 MiB, but manual validation must not accidentally raise the text bound;
- one connection has at most one pending upload and the gateway has at most
  one admitted buffered upload across connections;
- the binary body belongs to the sole pending upload and must exactly match
  declared length and SHA-256;
- the client waits for `upload_ready` before sending bytes;
- an admitted upload expires after 120 seconds without a complete body,
  releases its permit, and returns or records a bounded timeout outcome;
- unexpected binary data, oversize framing, repeated begin, conflicting IDs,
  or binary data during handshake remains a protocol violation;
- digest or semantic validation failure clears only that upload and yields a
  correlated bounded error without durable mutation;
- ping/pong, close, pending-call, call-byte, heartbeat, view attachment,
  outbound queue, and invalid-message limits otherwise remain unchanged; and
- the execution record measures control-message delay during a representative
  maximum-size upload and states the known one-frame head-of-line cost.

Chunking, resumable upload, progress events, multiple concurrent uploads,
temporary-file spooling, and a second browser connection are deferred.

### HTTP automation

Add authenticated `POST /api/v1/torrents` with
`Content-Type: application/x-bittorrent`. Use one bounded query/header
metadata representation for request ID, optional expected revision,
storage-root ID, `start_content`, and optional canonical selection mode/ranges.
Exact field placement may follow Axum ergonomics, but it must be documented,
independently validated under the same semantic bounds, and convenient for
`curl` without multipart or base64.

The route receives a known-length or streamed HTTP body into the same bounded
in-memory representation, rejects over 64 MiB before or while collecting,
computes SHA-256, and invokes the common operation. Apply a route-specific
body limit without raising the 64-KiB default for JSON routes. Existing
Bearer, Basic, unauthenticated-development, exact-Origin/CORS, connection,
and owner-isolation behavior remains intact; Tactical `076` authentication
must cover this route before body processing.

This endpoint is for automation, diagnostics, and compatibility. The shared
interactive browser must use the ordinary WebSocket attachment rather than
silently opening a concurrent HTTP command lane.

### Tauri desktop

Add one in-process Tauri command that accepts request metadata and a raw IPC
body. The shared web adapter will eventually pass a browser-selected
`ArrayBuffer`; Tauri exposes it as `InvokeBody::Raw` through
`tauri::ipc::Request`. Reject JSON bodies for this operation so a 64-MiB file
cannot become base64 or a JSON number array by accident.

The Tauri adapter acquires its bounded import admission, validates the same
metadata, and calls the same application operation. It opens no socket and
does not receive or derive a filesystem path. A Tauri mock/raw-IPC test and
desktop compile/build evidence must prove the request body reaches the common
operation without launching a visible window.

Android's Tauri raw-body limitation is irrelevant because Android Compose is
not hosted by Tauri. A later Android SAF intent may read document bytes and
call the semantic application operation through a platform-specific adapter;
that is not part of this tactical.

## Reference Survey Required Before Implementation

Record exact findings and intentional differences from these sources before
finalizing state transitions:

### Normative specifications

- BEP 3 for v1 outer metainfo and exact info-hash identity;
- BEP 12 for multi-tracker tier structure and fallback intent;
- BEP 27 for private-torrent discovery gating; and
- BEP 9 to confirm that peer metadata remains an info-dictionary-only path,
  uses exact 16-KiB blocks and a reported total size, specifies no global byte
  maximum, and does not gain outer-source semantics.

Use the checkouts pinned by `reference/pins.toml`.

### Pinned libtorrent `v2.0.13`

- `src/ut_metadata.cpp` for the 4-MiB handshake-size allocation guard, 30-MiB
  configured authoritative transfer ceiling, two-request outgoing depth,
  160-KiB immediate send-buffer threshold, 1,024 deferred incoming-request
  queue, size consistency, piece-zero fallback, and uncapped-by-that-setting
  upload of already valid local metadata;
- `include/libtorrent/settings_pack.hpp` and `src/settings_pack.cpp` for the
  independent `max_metadata_size = 30 MiB`, `max_piece_count = 2,097,152`, and
  `metadata_token_limit = 2,500,000` peer defaults;
- `include/libtorrent/torrent_info.hpp::load_torrent_limits` and
  `src/torrent_info.cpp` for the distinct 10,000,000-byte file-read,
  2,097,152-piece, depth-100, and 3,000,000-token explicit-load defaults and
  the absence of that file-read cap on the caller-owned span constructor;
- `src/bdecode.cpp` and `include/libtorrent/bdecode.hpp` for exact lexical
  token/depth accounting and the compact borrowed-token representation;
- `include/libtorrent/file_storage.hpp::{max_piece_size,max_num_pieces}` and
  `src/torrent_info.cpp::parse_info_section` for piece geometry and the
  independent piece-count setting;
- `src/torrent_info.cpp::{sanitize_append_path_element,
  resolve_duplicate_filenames}` plus
  `test/test_torrent_info.cpp::{sanitize_path*,test_resolve_duplicates}` for
  invalid UTF-8, separators, dot names, reserved characters, long components,
  and projected collisions;
- `src/piece_picker.cpp` and its tests for compact whole-torrent identity,
  availability, priority, and active-piece state; inspect behavior and
  measured representation rather than copying its architecture;
- `src/magnet_uri.cpp` for scalable tracker vectors and range-based `so`
  syntax, including its distinct defensive 10,000 file-index ceiling;
- `simulation/test_metadata_extension.cpp::ut_metadata_token_limit` and the
  metadata cases in `test/test_fast_extension.cpp` for independent peer
  decode-work and invalid-request behavior;
- `src/torrent_info.cpp` around outer `announce-list`, `announce`, nodes, web
  seeds, URL validation, tier attribution, and malformed-field tolerance;
- `src/load_torrent.cpp::update_atp` and `load_torrent_buffer` for moving outer
  trackers, tiers, web seeds, and DHT nodes into `add_torrent_params`;
- `include/libtorrent/add_torrent_params.hpp` for the separation among parsed
  metainfo, tracker tiers, resume fields, and payload `save_path`;
- `test/test_torrent_info.cpp` and `test/test_torrent.cpp` for malformed outer
  values, tracker URL schemes, whitespace, duplicate/invalid trackers, tier
  behavior, `many_pieces.torrent`, long/duplicate names, large file-priority
  vectors, many files, and load-buffer cases;
- `include/libtorrent/session_params.hpp` and
  `examples/client_test.cpp::{resume_file,add_torrent}` plus its resume scan
  for the fact that session state does not own a torrent catalog and the
  embedding client chooses persistence; and
- `src/write_resume_data.cpp::write_torrent_file` only to distinguish later
  synthesis from retention of original source bytes.

Libtorrent is a completeness and edge-case oracle, not an architecture or
source donor. No source or fixture is copied without separate provenance and
license review.

### JSTorrent product history

Inspect the pinned sibling revision, especially:

- `packages/engine/src/core/bt-engine.ts` and `torrent-parser.ts` for byte
  intake and exact info-dictionary handling;
- `packages/engine/src/core/session-persistence.ts` for its explicit split
  among file-source torrent bytes, magnet info dictionaries, and mutable
  state;
- `packages/engine/src/node-rpc/controller.ts` and
  `packages/client/src/engine-manager/daemon-engine-manager.ts` for current
  remote/base64 and native event paths; and
- Android `LinkHandlerActivity.kt` and `PendingLinkManager.kt` for document
  intent and handoff lessons.

RSTorrent adopts the useful source/runtime distinction and browser-selected
byte semantics. It does not adopt base64 storage or transport, JavaScript KV
keys, an IO daemon, native-path RPC, or Android UI work in this slice.

## Shape-Changing Edge Cases That Must Land Now

- Existing version-7 rows use canonicalized magnets as catalog state; an
  evictable successful add receipt may prove the verbatim submission, but
  migration must not manufacture provenance when that evidence is absent or
  ambiguous.
- The outer dictionary may contain unknown or unsupported fields; exact
  retention must not turn them into runtime authority.
- `announce-list` may be present but contain no valid tracker, in which case a
  valid `announce` is the fallback.
- Empty/malformed tiers, duplicate URLs across tiers, leading whitespace,
  invalid schemes, excessive valid trackers, long URLs, HTTP-only private
  torrents, and mixed supported/unsupported tiers must have deterministic
  outcomes.
- A valid v1 info dictionary can exceed 30 MiB: 2,097,152 v1 pieces require a
  41,943,040-byte (`40 MiB`) `pieces` string before the rest of the dictionary.
  Explicit import and durable restart accept this under the 64-MiB profile,
  while default BEP 9 acquisition remains independently capped at 30 MiB.
- A BEP 9 peer may advertise more than 4 MiB without being rejected, after
  which piece zero establishes one consistent size up to 30 MiB; 30 MiB is
  accepted and one byte more is rejected before the full allocation.
- A 30-MiB download uses 1,920 blocks without duplicate full-size assembly,
  survives peer reassignment and hash-failure retry within the same resource
  bound, persists exactly, and restarts under the larger durable profile.
- Large-metadata interoperability fixtures derive their size from supported
  v1 file/path and piece-hash structure, not one ignored padding string, so
  the evidence exercises useful metadata rather than only transport bytes.
- A metadata uploader can serve all 1,920 unique blocks plus bounded retries;
  it can also serve all 4,096 blocks of valid 64-MiB local metadata. The
  1,024-request value limits only deferred queue occupancy and cannot be
  implemented as a connection-lifetime counter.
- A maximum 64-MiB source may duplicate nearly all of its bytes in retained
  `raw_info`; the measured ephemeral budget must admit one such transaction or
  roll it back wholly, and later exhaustion must leave the service usable.
- Request replay compares source digest and semantic options. It cannot depend
  on the original bytes still being resident in a transport buffer.
- A duplicate upload cannot silently upgrade a magnet, replace source
  provenance, or merge tracker tiers.
- Source BLOB corruption cannot establish or remove runtime metadata
  authority, and startup must not materialize that BLOB.
- An HTTP/HTTPS tracker cannot be mislabeled UDP or failed merely because its
  transport is unimplemented.
- WebSocket text and JSON body limits must remain 64 KiB even though one
  declared binary/raw route accepts 64 MiB.
- Disconnect before the body completes and disconnect after semantic dispatch
  have different owners and must be tested separately.
- Tauri must use raw IPC rather than an implementation that appears binary in
  TypeScript but expands to JSON internally.

## Staged Implementation And Intermediate Gates

### Gate 1: Pinned comparison and final limit calibration

Build independently generated shallow-wide, deep, many-file, many-tracker,
long-path, many-piece, and ignored-field fixtures and run them through pinned
libtorrent and RSTorrent in isolated subprocesses. Record outer/info bytes,
lexical tokens, retained decoded work, cardinalities, parser time,
baseline-subtracted peak RSS, and retained state. Confirm the exact
libtorrent-derived numeric limits and amend this tactical with final
decoded-work, file, path-projection, tracker, URL, page, 64-MiB attachment,
and ephemeral page-budget values. No later gate may preserve an old cap that
this audit identifies or rely on a provisional non-apples-to-apples value.

### Gate 2: Parser profiles and large BEP 9

Implement the separate 64-MiB explicit/durable and 30-MiB peer profiles, the
adopted depth/work/geometry limits, exact info-span ownership, and the safe
operational path projection. Implement the 4-MiB handshake-hint/piece-zero
flow, one transferable BEP 9 assembly, 160-KiB send threshold, and 1,024-entry
deferred queue in place of the lifetime counter. Prove exact maximum and
one-byte-over behavior, the 40-MiB piece-hash case through explicit/durable
parsing, multi-peer retry/cancellation, hash failure, and upload of more than
1,024 lifetime blocks. Also prove all 4,096 valid upload blocks for locally
imported metadata without raising the 30-MiB receive profile.

### Gate 3: Scalable v1 geometry and storage owners

Adopt 2,097,152 pieces and a 536,854,528-byte piece across engine identity,
compact availability/have state, layout, recheck, part-file, and storage.
Replace eager whole-torrent `PiecePlan`/block construction with a bounded
active window, avoid copied piece-hash tables where the exact info span can be
indexed safely, remove per-allocation piece-count scans, and hash large pieces
incrementally from storage. Prove that maximum-geometry metadata-only state is
cheap and that representative active content state depends on the scheduling
window rather than total pieces or blocks.

### Gate 4: Paged catalogs, selection, and platform contracts

Replace enumerated whole-file selection with canonical all/none/range and
paged mutation contracts. Share immutable file geometry, calculate file spans
without allocating one piece vector per file, and add bounded file and tracker
view pages with total count plus stable cursor/range semantics. Update Rust,
generated TypeScript, web reducers/validation, Tauri contracts, and Android
adapter contracts together. Prove high-file and high-tracker metadata-only
behavior without eager rows, descriptors, or documents; preserve the 16-MiB
view-snapshot ceiling as a page bound rather than a catalog bound.

### Gate 5: Pure outer-source projection

Complete runtime-independent extraction of exact `raw_info`, v1 metainfo,
source digest, normalized full tracker tiers, and bounded unsupported
transports. Complete normative and pinned-source adversarial fixtures. No
session, SQLite, async runtime, socket, or platform type enters this layer.

### Gate 6: Schema, source, and ephemeral migration

Advance the schema, migrate existing canonical magnets truthfully, normalize
magnet operational hints, retain exact new sources, raise durable `raw_info`
and source BLOBs to 64 MiB, raise piece/have/file/tracker representation, and
apply the measured ephemeral main-database page budget. Prove atomic
migration/rollback/restart/removal, including one maximum transaction and a
subsequent typed page-budget exhaustion. Existing application commands and
durable/ephemeral behavior remain green before byte intake is exposed.

### Gate 7: Semantic byte operation

Add request identity/digest receipts, atomic metadata-ready insertion,
duplicate/stale/error behavior, ordinary reconciliation, metadata-only and
running lifecycles, and bounded parse ownership. Prove direct in-process
durable and ephemeral operation before transport work.

### Gate 8: Tracker tiers and truthful unsupported state

Pass the normalized full tracker catalog into persisted state, activate no
more than eight UDP operations, preserve tier/source semantics, page combined
supported and unsupported inspection, and project private unsupported-only
blockage. Complete pure, scripted UDP, restart, and generated-contract
evidence without a 32-record catalog ceiling.

### Gate 9: WebSocket and HTTP adapters

Add shared gateway admission, one-frame 64-MiB binary handshake,
route-specific raw HTTP body handling, authentication-before-body behavior,
timeout/cancellation, protocol errors, replay after lost response, and
resource high-water metrics. Retain the 64-KiB text/JSON and all existing
connection/view tests.

### Gate 10: Tauri raw IPC

Add and prove the in-process 64-MiB raw-body command, shared semantic result,
no-path contract, and no-visible-window compile/mock evidence. Do not begin
picker UI.

### Gate 11: Controlled interoperability and closure

Use an independently controlled UDP tracker and pinned libtorrent seed to
download exact content from imported outer metainfo. Run the complete matrix,
record high-water values and intentional reference differences, update topics
and readiness, and close the tactical only when no required row is missing.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pinned comparison | Identical size-heavy and structure-heavy inputs through pinned libtorrent and RSTorrent; outer/info bytes, lexical tokens, retained decoded work, files, trackers, pieces, path bytes, parser time, baseline-subtracted peak RSS, and retained state; final calibrated limits recorded in this tactical before Gate 2. |
| BEP 9 protocol/state | Handshake sizes absent, invalid, 4 MiB, 4 MiB plus one, 30 MiB, and 30 MiB plus one; piece-zero fallback; exact 1,920-block geometry; two requests per peer; reassignment, timeout, reject, duplicate, size disagreement, hash failure, cancellation, and single-buffer ownership. |
| BEP 9 upload | Exact blocks and final geometry; 160-KiB send-buffer threshold and 1,024 deferred-request bound; invalid/excess rejection; more than 1,024 lifetime requests; complete 1,920-block received-profile and 4,096-block local-profile transfers; retry and disconnect cleanup. |
| Pure parser/source | Exact info span/hash; 30-MiB peer and 64-MiB explicit/durable/source boundaries plus one-byte-over limits; a 40-MiB v1 piece-hash string; 2,097,152 pieces and one more; 536,854,528-byte piece length and one more; adopted depths; calibrated work/cardinality limits; v2/hybrid rejection; BEP 12 tiers/fallback/dedup/malformed cases; mixed transports; private flag; and deterministic source digest. |
| Paths | Pinned libtorrent sanitization/duplicate cases plus invalid UTF-8, separators, absolute/dot/reserved names, trailing dot/space, overlong components, deep paths, normalization collisions, stable suffixes, root containment, platform constraints, and byte-identical projection after restart. |
| Geometry/scheduler | Maximum-piece-count metadata-only add/restart constructs no eager piece/block maps; compact availability and exact spare-bit validation; active state remains proportional to configured scheduling windows; cancellation and generation cleanup return to baseline. |
| Storage/large piece | A 536,854,528-byte piece is requested/written in bounded blocks and hashed incrementally from storage without a piece-sized allocation; part-file lookup/allocation avoids piece-count scans; checkpoint, recheck, restart, corruption, cancellation, and publication remain bounded. Sparse/generated backing may avoid retaining a second full fixture in memory. |
| Catalog/selection/views | Calibrated maximum file and tracker catalogs, all/none/range and adversarial sparse selection, page boundaries/cursors/filter changes, page-scoped patches, full traversal without omissions/duplicates, under-16-MiB snapshots, stable restart identity, and no eager whole-catalog rendered model. |
| Migration/store | Version-7 canonical-magnet migration, verbatim new magnet, exact outer BLOB, 64-MiB durable raw-info/source path, 2,097,152-piece and 262,178-byte have checks, calibrated file/tracker indices, transaction rollback, request replay/conflict, stale revision, duplicate identity, removal cascade, source-independent startup, and unchanged lower-bound rows. |
| Ephemeral | Same schema and semantics in private memory, no profile/temp/source file, metadata-only empty payload root, one maximum 64-MiB source plus raw info and catalog within the measured provisional 256-MiB main-database budget, source disappearance on close, following maximum transaction or page-cap rollback as applicable, typed resource limit, and following-call availability; metrics remains separately capped at 32 MiB. |
| Application lifecycle | Metadata-only add, running add, skip validation, pause/resume/recheck/removal, lost-response replay, disconnect-before-dispatch, disconnect-after-dispatch, joined shutdown, and no second engine generation. |
| Tracker runtime | Full calibrated metainfo tier/source catalog, no silent truncation, at most eight active UDP operations, fallback/retry/cancel, paged unsupported HTTP/HTTPS projection, private unsupported-only blockage, mixed DHT/tracker behavior, and restart reconstruction without volatile history or loading the source BLOB. |
| Android adapter | Generated contract and Rust/Kotlin tests prove compact selection and paged catalogs at representative high cardinality, deterministic safe paths, bounded provider requests, and lazy descriptor/document acquisition; compile/build without adding Android `.torrent` intent intake. |
| WebSocket | Ready/body/result sequence, exact length/digest, 64-KiB text bound, 64-MiB binary bound, unexpected/repeated frames, timeout, disconnect, global one-upload admission, heartbeat/control before and after upload, lost result plus replay, and queue/metric high-water values. |
| HTTP | Raw content type, bounded known/chunked body collection, route-only 64-MiB limit, semantic metadata validation, auth rejection before body work, existing auth modes and Origin behavior, replay/conflict, and script-friendly response. |
| Tauri | Raw `InvokeBody`, JSON-body rejection, shared request/result, exact bytes, admission release, no path/base64/number-array representation, mock invocation, and desktop compile/build without a visible window. |
| Interoperability | A controlled pinned-libtorrent peer supplies an independently generated info dictionary above 16 MiB and through the 30-MiB BEP 9 boundary, whose size comes from supported v1 file/path and piece-hash structure rather than ignored padding, and RSTorrent persists/restarts it; a 30-MiB-plus explicit metainfo exercises the separate caller-owned path; runtime-generated or independently authored v1 metainfo also points through a controlled UDP tracker to pinned libtorrent so RSTorrent imports, discovers, downloads, hash-verifies, publishes or retains metadata-only as configured, restarts, and removes with exact cleanup. |
| Resource profile | BEP 9 assembly/handoff/hash/parse high water at 30 MiB; explicit parser/application/transport high water at 64 MiB; SQLite/WAL growth and ephemeral page use for maximum retained source plus raw info; maximum-geometry metadata-only retained state; representative active scheduler/part-file/view/Android state; one concurrent upload; WebSocket control delay; and zero leaked task/permit/buffer after every terminal case. |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, generated TypeScript/schema/Kotlin checks, web tests/typecheck/build, desktop checks, Android Rust/Gradle unit and compile/build checks without an emulator, and `git diff --check`. |

No public swarm, fixture download, visible browser, visible Tauri window,
Android runtime, emulator, physical device, new external service, or
deployment mutation is required. Headless Android compile/build evidence,
loopback sockets, and controlled pinned libtorrent execution are authorized.
The implementation may add independent fixtures generated from public
protocol behavior; importing a reference fixture requires provenance and
license review first.

## Non-Goals And Deliberate Deferrals

- Visible browser/Tauri file picker, Add-dialog integration, upload progress,
  source filename presentation, drag-and-drop, or clipboard file handling.
- WebSocket/HTTP chunking at the application level, resumable upload,
  temporary-file spooling, or SQLite incremental BLOB I/O. A compact or
  streaming semantic bencode projection is in scope if Gate 1 measurements
  require it to match the adopted limits safely.
- More than one simultaneous upload or a separate upload socket/data plane.
- Adding a `.torrent` by remote URL, fetching arbitrary URLs, filesystem
  browsing, or allowing remote callers to create storage roots.
- Android intent/SAF intake, ChromeOS extension handoff, desktop file
  association, operating-system share targets, or native picker behavior.
- Payload upload, seeding, playback, or HTTP content transfer.
- HTTP/HTTPS/WebSocket tracker execution, proxying, authentication extensions,
  scrape, web seeds, DHT bootstrap nodes from metainfo, or BEP 41 URL data.
- V2/hybrid torrents, piece layers, mutable torrents, or non-v1 integrity.
- Duplicate-source enrichment, magnet-to-file upgrade, tracker editing,
  source replacement, or a multi-source provenance ledger.
- Exact-source copy/export UI, synthesized `.torrent` export, configurable
  export directory, payload-adjacent copy, or profile-private sidecar store.
- Fast-resume redesign, optimistic trust, peer-cache persistence, transfer
  history, or replacement of SQLite with libtorrent-style resume files.
- Libtorrent's architecture, platform-conditional display-name bytes, resume
  file model, or lack of an application attachment cap. This tactical does
  adopt its apples-to-apples v1 limits and requires measured calibration where
  RSTorrent's representation has no direct counterpart.
- Stable public wire compatibility, pairing, accounts, relay, per-principal
  authorization, TLS termination, credential rotation, or general Internet
  exposure beyond Tactical `076`.
- Implementing planned Tacticals `077`, `078`, or `080` as incidental work.

## Escalation And Autonomous Authority

Implementation may autonomously choose internal Rust names, module placement,
SQL column names and indexes, pure parser helper shapes, exact generated frame
names, the bounded HTTP query/header split, and test fixture construction. It
may refactor the current receipt, magnet normalization, tracker schedule, view
projection, gateway state, and Tauri adapter where necessary to establish the
accepted owners without widening public compatibility. It may add
adversarial cases implied by these invariants, correct a same-boundary bug
exposed by them, and update generated contracts and owning topics with actual
evidence.

Ordinary compiler errors, migration fixture changes, failed deterministic
tests, conservative error classification, Tauri raw-IPC ergonomics, Axum body
collection details, and internal differences from libtorrent do not require
maintainer input. Tightening an in-scope timeout below its stated maximum from
measured safety evidence is allowed if common `.torrent` intake still passes
and the execution record explains it. The accepted 30-MiB peer transfer,
64-MiB explicit/durable/source attachment, 2,097,152 pieces,
536,854,528-byte piece length, 4-MiB handshake-hint boundary, two-request
outgoing depth, 160-KiB send-buffer threshold, and 1,024-entry deferred queue
may not be reduced as implementation shortcuts.

Stop for maintainer direction if implementation would require:

- abandoning exact outer-source BLOB retention or making it runtime
  authority;
- a sidecar/blob filesystem, payload-adjacent source copy, or cross-store
  transaction;
- raising the 64-MiB explicit/durable/source attachment or 30-MiB BEP 9
  limits;
- lowering an adopted apples-to-apples libtorrent value, or landing a
  non-apples-to-apples file, decoded-work, collection, path, tracker, URL,
  page, or ephemeral budget that cannot meet the Gate 1 comparison contract;
- silently truncating valid trackers or accepting duplicate-source merge;
- implementing HTTP/HTTPS tracker transport, web seeds, v2/hybrid integrity,
  or chunked/resumable application uploads to meet the stopping condition;
- adding an external dependency with material runtime, security, license, or
  maintenance tradeoffs;
- changing the accepted private-host authentication or remote-product
  posture;
- visible UI, public-network, deployment, emulator, device, destructive user
  data, or external coordination not already authorized; or
- a product behavior materially different from the decisions above.

## Next Slice Boundary

After this tactical, the preferred next product slice is the shared
browser/Tauri Add flow: select a local `.torrent`, read it as an `ArrayBuffer`,
send it through the adapter-specific byte operation, reuse established
storage-root/start options, and report bounded progress and errors. That slice
may evaluate chunking from the one-frame latency and memory evidence recorded
here, but chunking is not a prerequisite unless the measurements contradict
this tactical's accepted bound.

Later source/export work may expose exact original magnet copy, exact original
`.torrent` download, synthesized export when no original exists, explicit
export directories, and duplicate metadata enrichment. Those operations must
continue distinguishing source provenance from runtime authority.
