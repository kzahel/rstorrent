# Tactical 002: Selective Multi-File Storage Foundation

Status: completed on 2026-07-29.

## Motivation And Outcome

Establish the storage model for selective multi-file downloads before a
single-file happy path fixes assumptions that cannot represent file
boundaries, skipped content, padding, or durable partial state.

BEP 3 treats a multi-file torrent as one concatenated byte space. Pieces and
16 KiB requests may cross file boundaries, and a piece needed by one selected
file may include bytes belonging to skipped files. BEP 47 padding bytes are
part of that hash space but are synthetic zeroes that an aware client should
neither request nor write.

This slice will download an edge-rich deterministic v1 multi-file fixture from
Rasterbar libtorrent through tactical `001`'s bounded block pipeline. Selected
file ranges will be written to a hidden staging tree. Skipped ranges required
by boundary pieces will be written to one versioned part file with compact,
reusable piece slots. Padding ranges will be supplied as zeroes. Piece hashing
will stream across those storage sources, and no selected final path will
appear before all of its data is verified.

The scenario will leave one file skipped, materialize a second initially
skipped file from verified part-file ranges, reopen the part file before that
materialization, and prove that a piece containing only skipped content was
never requested. The result is a storage foundation, not yet a general
torrent session.

## Dependencies And References

- [Tactical 000 execution record](000-first-verified-piece.md)
- [Tactical 001 execution record](001-bounded-large-piece.md)
- [Product and engine direction](../topics/product-direction.md)
- [Engine engineering principles](../engineering-principles.md)
- [Reference policy and license posture](../references.md)
- BEP 3; offline source:
  `reference/bittorrent.org/beps/bep_0003.rst`
- BEP 47; offline source:
  `reference/bittorrent.org/beps/bep_0047.rst`
- Rasterbar libtorrent `v2.0.13`, pinned in
  [`reference/pins.toml`](../../reference/pins.toml)
- The current first-party JSTorrent sibling at `~/code/jstorrent`

No reference source, fixture, or format is copied into this slice.

## Reference Findings

### BEP 3 and BEP 47

- Multi-file payload is one concatenated byte space in file-list order.
  Piece indices and offsets refer to that space, not to individual files.
- File `path` is a non-empty list of UTF-8 components relative to the torrent
  root. `length` may be zero.
- BEP 47 marks padding files with the `p` attribute. Their bytes hash as
  zeroes. Aware clients should not create them or request ranges that cover
  them.
- BEP 47 warns that malicious padding and symlink metadata can make a torrent
  internally inconsistent. This tactical accepts padding and rejects
  symlinks.

### Libtorrent

These observations describe the pinned implementation and inform an
independently authored RSTorrent design:

- File priorities are converted to piece priorities by taking the maximum
  priority of every non-padding file overlapping a piece. Therefore an
  otherwise skipped boundary piece is downloaded in full when any overlapping
  file is wanted; a piece touching only skipped files remains priority zero.
- One read or write in torrent coordinates is split across every overlapping
  file. Ordinary wanted ranges use their file, skipped ranges use a part file,
  padding reads as zeroes, and padding writes are no-ops.
- The part file has a header whose size is determined by the torrent piece
  count and rounded to 1 KiB. It contains the torrent piece count, piece
  length, and a piece-index-to-slot table. Payload follows in piece-sized
  slots allocated compactly and reused after release.
- The part-file table records placement, not unfinished block completion.
  Unfinished block bitmaps are persisted separately in resume data.
- Piece hashing traverses the same mapping and combines ordinary files,
  part-file ranges, and padding zeroes in torrent order.
- Raising a skipped file to wanted exports its ranges from the part file.
  Lowering an already created wanted file to skipped does not currently
  migrate that file into the part file.
- File priority changes are asynchronous disk operations. Libtorrent keeps
  network policy, file priority state, storage placement, and part-file
  ownership as distinct concepts.

RSTorrent will follow the concatenated-space mapping, boundary-piece
selection, compact slot, mixed-source verification, and materialization
concepts. It will not copy libtorrent's byte format, mmap backend, class graph,
priority scale, or incomplete-piece resume protocol.

### JSTorrent

The current sibling has evolved toward a compact, fixed-header `.parts`
layout and keeps complete boundary pieces until their files can be
materialized. Its current `PartsFile` also retains every loaded piece as a
resident `Uint8Array`, while the active piece path assembles a complete piece
before writing it.

That behavior is useful product and failure-case evidence but conflicts with
tactical `001`'s piece-length-independent resident payload bound. RSTorrent
will retain block-granular network ownership and will not load all part-file
payload into memory.

## Scope

### Bounded multi-file metainfo

Extend the controlled v1 parser to represent:

- single- or multi-file mode;
- torrent root name;
- file-list order, lengths, checked global offsets, and path components;
- BEP 47 padding status;
- every v1 piece hash; and
- checked total length and exact piece count.

The parser must:

- continue hashing the exact original `info` dictionary span;
- require exactly one of `length` and `files`;
- accept zero-length entries in a non-empty multi-file list;
- require non-padding files to have a non-empty path;
- accept a missing path only for a padding file;
- recognize `p` in `attr`, ignore unknown attribute characters, and reject
  the `l` symlink attribute;
- require UTF-8 path components and reject empty, `.`, `..`, NUL-containing,
  separator-containing, absolute, duplicate, and file/directory-colliding
  paths;
- enforce explicit file, path-component, path-length, piece-count, piece-hash,
  checked-offset, and existing bencode bounds; and
- reject inconsistent piece-hash counts before creating download state.

The controlled parser need not sanitize hostile paths into alternate names.
Rejecting unsafe or ambiguous layout is preferable to silently changing the
content tree.

### Pure torrent layout and selection

Add a runtime-independent layout owner that maps a validated
`(piece, begin, length)` interval to ordered segments:

```text
wanted file(index, file offset)
skipped file(index, file offset)
padding zeroes
```

The mapper must handle:

- a block spanning multiple files;
- multiple zero-length files at one offset;
- the last short piece;
- exact end-of-file and end-of-torrent boundaries;
- padding beginning or ending inside a 16 KiB request grid;
- checked 64-bit torrent and file offsets; and
- a bounded number of output segments.

Selection is initially binary: wanted or skipped. Padding is always
synthetic/skipped. A piece is wanted when at least one non-padding wanted file
overlaps it. All real, non-padding bytes of a wanted piece are requested,
including skipped-file bytes needed for the piece hash. Padding subranges are
removed from the request plan, so a request may be shorter than 16 KiB at a
padding boundary.

The pure layer reports piece classification, request ranges, and mapping
segments. It owns no path, file, async, or task types.

### Multi-piece download state

Keep one active piece at a time for this diagnostic. Reuse the existing block
lifecycle and payload reservations while generalizing it to:

- arbitrary piece index and final-piece length;
- a torrent-wide bitfield length and padding validation;
- peer `have` messages for inactive pieces without treating them as a
  protocol error;
- caller-supplied request ranges that exclude padding; and
- cumulative piece, block, requested-byte, and payload high-water counters.

The peer connection, choke state, availability, and decoder remain owned by
one diagnostic future across piece transitions. Selected pieces are processed
in ascending order. Rarest-first selection, concurrent pieces, retries, and
more than one peer remain outside this slice.

### Selected-file staging

For multi-file mode, treat the configured output as the explicit torrent root.
Create a hidden sibling staging directory and reproduce only wanted
non-padding paths beneath it.

- Create intermediate directories safely beneath the staging root.
- Size wanted files, including empty files, before download.
- Write received wanted segments at file-relative offsets.
- Never create skipped or padding paths.
- Keep every selected final path absent until all initially wanted pieces
  verify.
- Flush open files and rename the complete staging root to the final root only
  after success.
- Refuse existing final, staging, or part-file paths.
- Remove staging and the part file on protocol, hash, timeout, or I/O failure.

This diagnostic publishes the selected tree as one unit. Per-file early
publication and production crash-consistent directory syncing are deferred.

### Versioned part file

Create one hidden sibling part file for skipped ranges of wanted boundary
pieces. Its independently authored format will contain:

- a magic value and explicit version;
- the v1 info hash;
- piece count, piece length, and total torrent length;
- one signed slot index per torrent piece;
- reserved bytes required to be zero; and
- zero padding to a 1 KiB header boundary.

Payload slots are addressed as:

```text
header length + slot index * piece length + piece-relative offset
```

Slots are allocated compactly, holes are reused, and offsets use checked
64-bit arithmetic. The part file writes and reads caller-provided block
slices; it must never load a piece-sized slot or all slots into memory.

Allocation metadata is flushed before a slot is used. Releasing a piece clears
and flushes its map entry before the slot is reused. On reopen, validate magic,
version, reserved bytes, layout identity, duplicate/negative/out-of-range
slots, header length, and every accessed payload range. Corruption or
truncation returns a typed error and is never treated as verified data.

This format does not persist unfinished block bitmaps or a verified-piece
bitfield. The end-to-end diagnostic will reopen a successfully written part
file in the same run to prove placement durability. Full resume and crash
recovery remain separate work.

Large piece-sized slot spacing may produce a large sparse logical file when
only small skipped ranges are written. The diagnostic must use 64-bit offsets
and record this cost. Whether Android SAF providers preserve sparse allocation
efficiently requires physical platform evidence before this becomes the
product storage format.

### Streamed mixed-source verification

Once every requested block of a wanted piece is stored, hash the complete piece
in order through a fixed 16 KiB buffer:

- wanted segments read from selected-file staging;
- skipped segments read from the part-file slot; and
- padding segments feed zeroes without disk I/O.

Short reads, missing part slots, invalid mapping, and I/O errors fail
verification. A matching SHA-1 marks the piece verified. A mismatch leaves all
final paths absent and terminates this no-retry diagnostic.

### Materialization

After all initially wanted pieces verify and the selected staging tree is
published:

1. close the part file;
2. reopen and validate its durable header;
3. materialize one configured initially skipped non-padding file whose every
   overlapping piece is verified;
4. stream its ranges from part-file slots into a hidden sibling file;
5. flush and rename that file into the published tree; and
6. release a part slot only when no remaining skipped file overlaps its piece.

Materializing a file with a missing piece must return a typed error without
publishing a partial final file. Changing wanted files to skipped, changing
priorities during network I/O, and materializing a file with skipped-only
missing pieces are non-goals.

### Edge-rich interoperability fixture

Use a v1-only torrent named `fixture`, piece length 32,768 bytes, and these
files in order:

| Index | Initial state | Path | Length |
| ---: | --- | --- | ---: |
| 0 | wanted | `wanted/start.bin` | 20,000 |
| 1 | skipped | `skip/large.bin` | 50,000 |
| 2 | skipped, then materialized | `later.bin` | 7,000 |
| 3 | wanted | `wanted/end.bin` | 18,000 |
| 4 | wanted | `wanted/empty.bin` | 0 |
| 5 | padding | `.pad/3304` | 3,304 |
| 6 | wanted | `tail.bin` | 35,000 |

The concatenated length is 133,304 bytes and spans five pieces.

- Piece 0 is wanted/skipped boundary data. Its second 16 KiB request crosses
  from file 0 into file 1.
- Piece 1 lies wholly in skipped file 1 and must not be requested.
- Piece 2 crosses file 1, file 2, file 3, an empty-file offset, and padding.
  Its first request crosses three real files. Its final 3,304 padding bytes
  must not be requested.
- Piece 3 is a normal full wanted piece.
- Piece 4 is a 2,232-byte final piece.

The run initially requests pieces 0, 2, 3, and 4: 97,232 peer payload bytes in
seven requests. It publishes files 0, 3, 4, and 6; never creates file 1 or the
padding path; reopens two part-file piece slots; materializes file 2; and keeps
both slots because file 1 still overlaps both boundary pieces.

The Python harness will compare every published/materialized file
incrementally to the fresh deterministic seed, assert absent skipped/padding
paths, parse all diagnostic counters, inspect that the expected part file
survived, and remove every temporary path after each run.

## Contracts And Invariants

- A multi-file metainfo file cannot escape or ambiguously alias its configured
  output root.
- Checked file offsets exactly cover the concatenated torrent byte space.
- A mapped interval covers its input exactly once and in order.
- A piece touching only skipped files causes no peer request.
- A wanted boundary piece requests all real bytes required for its hash.
- Padding bytes are neither requested nor written and hash as zeroes.
- Every peer request reserves payload bytes before emission.
- Payload reservations remain bounded independently of piece and torrent size.
- A block is acknowledged stored only after all of its mapped segment writes
  complete.
- Unverified selected bytes remain under a hidden staging root.
- Part-file bytes are never treated as verified without a matching piece hash
  or a same-run verified-piece record.
- Piece hashing is ordered and uses no more than one 16 KiB readback buffer.
- No skipped or padding path is created as a side effect of boundary data.
- No initially wanted final path exists until all initially wanted pieces
  verify.
- Materialization publishes only a complete file whose overlapping pieces are
  already verified.
- Part-file slot metadata is validated and flushed around allocation/reuse.
- Corrupt, truncated, mismatched, or missing durable state cannot become
  verified content.
- Protocol/layout state remains independent from async, filesystem, path,
  socket, clock, and task types.

## Non-Goals

- trackers, DHT, PEX, LSD, magnets, or metadata exchange
- more than one peer or active piece
- rarest-first/endgame scheduling or corruption retry
- file priority levels beyond wanted/skipped
- changing priorities during peer I/O
- wanted-to-skipped migration
- resuming an unfinished piece or persisting the verified bitfield
- seeding or serving mixed file/part-file content
- symlink creation or BEP 47 file attributes other than padding
- v2/hybrid torrents or v2 per-file Merkle verification
- per-file early publication
- pre-existing content reuse, overwrite, or merge
- mmap, sparse-allocation guarantees, compaction, or hole punching
- Android SAF implementation or physical-device claims
- exact total-process memory or disk-space limits
- making the diagnostic a generally useful CLI

## Initial Dependency Direction

The existing crate direction remains:

```text
rstorrent-protocol
    v1 metainfo layout, safe paths, selection, interval mapping,
    request ranges, piece lifecycle and payload reservations

rstorrent-engine
    Tokio peer loop, selected staging tree, part-file bytes,
    mixed-source hashing, publication, materialization, cleanup
```

A concrete storage owner and pure layout boundary are justified by this
fixture. Do not introduce a generic virtual filesystem, async storage trait,
session actor, channel graph, or platform adapter in this slice.

## Implementation Sequence

1. Record this tactical and reference findings before code changes.
2. Generalize bounded v1 metainfo parsing and add hostile layout tests.
3. Add the pure interval mapper, selection/classification, request planning,
   and exhaustive boundary tests.
4. Generalize piece state for torrent-wide availability and supplied request
   ranges while preserving payload accounting.
5. Implement selected staging and the versioned compact-slot part file with
   reopen, corruption, truncation, and sparse-offset tests.
6. Implement mixed-source block writes, hashing, publication, cleanup, and
   post-reopen materialization.
7. Extend the diagnostic and locked libtorrent harness with the edge fixture.
8. Run standard validation, three consecutive oracle runs, dependency/license
   and artifact audits, and record exact evidence.

## Validation

Run and record:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv lock --project tests/interop --check
uv run --project tests/interop --locked \
  python -m py_compile tests/interop/first_verified_piece.py
python3 scripts/references.py status
git diff --check
```

The final unit suite must cover at least:

- valid single- and multi-file metainfo with exact info hashes;
- exact piece-hash count and checked total-length failures;
- zero-length, padding, symlink, unsafe, duplicate, prefix-colliding, excessive
  file/path/piece inputs;
- mapping intervals at every fixture file and piece boundary;
- a block spanning three files;
- wanted, boundary, skipped-only, pad-ending, full, and final-short pieces;
- request planning that excludes padding and all-skipped pieces;
- multi-piece bitfield length/padding, unrelated `have`, wrong-piece payload,
  choke, cancellation, duplicate, malformed, and slow-storage behavior;
- selected writes and mixed-source streamed hashing;
- absent final/skipped/padding paths before publication;
- empty selected file publication;
- part-file slot allocation, reuse, 64-bit offsets, reopen, layout mismatch,
  duplicate slots, bad reserved bytes, truncation, and missing payload;
- successful and incomplete materialization;
- slot retention while another skipped file overlaps;
- hash, storage, timeout, and cleanup failures; and
- the existing architecture boundary.

The existing small and 32 MiB single-piece interoperability profiles remain
regression gates.

The selective command must pass three consecutive fresh runs and record:

- Rust and Python versions;
- libtorrent binding and native versions;
- info hash and every piece hash;
- file/piece/block counts and fixture lengths;
- selected, skipped, padding, requested, written, and materialized bytes;
- configured and high-water payload bytes;
- verification buffer size;
- skipped-only pieces and padding ranges not requested;
- part slots before and after materialization and successful reopen;
- published, absent, and materialized paths;
- elapsed time; and
- cleanup status.

## Stopping Condition

This tactical is complete when the documented selective command downloads the
five-piece fixture from libtorrent on loopback and proves the contracts above
across three fresh runs. In particular, piece 1 and 3,304 padding bytes are
not requested; pieces 0 and 2 survive in two validated part-file slots; every
wanted file is byte-identical and published only after verification; the
empty wanted file exists; the permanent skipped and padding paths do not;
`later.bin` is materialized after part-file reopen; all seven network requests
respect tactical `001`'s payload allowance; and every temporary path is
removed by the harness.

The parser, mapper, piece state, part-file corruption/reopen,
mixed-source verification, materialization, cleanup, architecture, standard
validation, reference status, dependency posture, and exact evidence must
also pass and be recorded below.

## Execution Record

Completed on 2026-07-29.

### What Landed

- The bounded v1 metainfo parser now represents single- and multi-file
  torrents, exact file offsets, zero-length files, padding attributes, safe
  UTF-8 path components, every piece hash, and exact piece geometry. It
  rejects symlinks, unsafe or colliding paths, excessive files, paths, or
  pieces, length overflow, and inconsistent piece hashes.
- A runtime-independent `TorrentLayout` maps any validated piece interval to
  ordered wanted-file, skipped-file, or padding segments. It classifies
  pieces, omits skipped-only pieces, removes padding from request plans, and
  handles the fixture's three-file request and final short piece.
- `OnePieceDownload` now accepts an arbitrary torrent piece and a validated
  sparse request plan. Full-torrent bitfields, padding bits, unrelated
  `have` messages, choke cancellation, and the existing reservation-before-
  request payload accounting remain explicit and bounded.
- The engine owns a new independently versioned part-file format. Its fixed
  fields bind it to the info hash and torrent geometry; its piece-to-slot map
  and reserved padding form a 1 KiB-aligned header. Compact payload slots use
  checked 64-bit offsets. Allocation and release metadata is flushed before
  payload use or slot reuse.
- Selected non-padding paths are sized under a hidden staging root. Blocks
  split directly into wanted files and skipped part slots without a
  piece-sized buffer. Padding is never written. Verification reads all three
  sources in torrent order through one 16 KiB buffer.
- Publication renames the selected tree only after every required piece is
  verified. The diagnostic closes and strictly reopens the part file before
  materializing an initially skipped file. It publishes that file through a
  hidden sibling and releases a slot only when no remaining skipped file
  overlaps the piece.
- The explicit loopback diagnostic keeps one peer connection, decoder,
  availability view, and choke state while processing one selected piece at a
  time. Repeatable `--skip-file` and `--materialize-file` controls expose the
  bounded scenario without claiming a general torrent CLI.
- The locked Python oracle independently creates the exact seven-file
  libtorrent fixture, verifies every published file incrementally, checks all
  absent paths and counters, inspects the surviving part file, and removes the
  fresh temporary tree after every run.

The implementation was committed in bounded milestones:

- `7891507` planned the selective foundation;
- `1ea03c2` added bounded multi-file metainfo;
- `b8175ad` added pure selective layout mapping;
- `699838b` generalized bounded piece state;
- `6bbc2e0` added the durable compact-slot part file;
- `d6c926a` added selected staging and mixed-source verification;
- `becd282` added the one-peer selective driver;
- `fc6c01d` locked the libtorrent oracle; and
- `af72575` hardened corruption, preservation, and cleanup boundaries.

### Edge And Failure Evidence

The final workspace suite contains 17 engine tests, two diagnostic argument
tests, 31 protocol tests, and one architecture test. In addition to the
successful fixture, these cover:

- malformed multi-file modes, excessive and inconsistent piece geometry,
  zero-length and padding entries, symlinks, unsafe components, duplicate
  paths, and file/directory prefix collisions;
- every fixture boundary, a request split across three real files,
  skipped-only and padding-ending request omission, and the final 2,232-byte
  piece;
- full-torrent bitfield size and padding, unrelated and out-of-range `have`,
  wrong, duplicate, overlapping, unsolicited, and short payloads, choke and
  cancellation, slow storage, hash failure, and a 256 MiB piece without
  resident piece payload;
- missing slots, payload and header truncation, bad magic and version,
  incorrect header length, nonzero reserved bytes, every mismatched identity
  field, duplicate, invalid-negative, and out-of-range slots, durable reopen,
  hole reuse, and offsets beyond 32 bits;
- absent final, skipped, and padding paths, incomplete publication and
  materialization, empty-file publication, slot retention and release,
  pre-existing artifact preservation, and single- and multi-file timeout
  cleanup.

The architecture test continued to prove that the protocol crate has no
runtime, socket, filesystem, path, task, or clock dependency.

### Interoperability Evidence

The required command passed three fresh runs:

```bash
uv run --project tests/interop --locked \
  tests/interop/first_verified_piece.py --selective-files --runs 3
```

Environment:

- Rust `1.97.0`, Cargo `1.97.0`;
- Python `3.12.3`, uv `0.9.18`; and
- libtorrent Python binding and native library `2.0.13.0`.

Fixture identity:

- info hash:
  `f2c09c855c0749be70ae5b5caa5f79077f914932`;
- piece hashes in order:
  `256c168ba6e41045f5033fa95d678cfa590b374d`,
  `19776588cc3ab2eed9bdd0d35c67f2a6af816b33`,
  `0f6c5cf71b30a147fdc3ccb161876e3bc1b10d89`,
  `251122fab4f7489c236113a7cccc1d48232975c2`, and
  `0d6bd635b8e7eec1065b19e0d54de707e00f5209`;
- total length 133,304 bytes, piece length 32,768 bytes, five pieces,
  seven files, and a final piece of 2,232 bytes.

All three runs reported:

- four verified pieces and one skipped-only piece;
- seven peer requests totaling 97,232 bytes;
- a 32,768-byte payload limit and 32,768-byte high-water;
- a 16,384-byte verification buffer;
- 73,000 initially selected file bytes, 57,000 initially skipped file bytes,
  and 3,304 synthetic padding bytes;
- 73,000 bytes written to selected files and 24,232 boundary bytes written to
  the part file;
- 7,000 bytes materialized after a successful part-file reopen; and
- two part slots before and after materialization because permanently skipped
  file 1 still overlaps pieces 0 and 2.

The three elapsed times were 0.164, 0.166, and 0.166 seconds. Every run proved
that file 1 and the padding path were absent, files 0, 2, 3, 4, and 6 matched
the deterministic seed, the selected staging root was absent after
publication, the validated part file survived until inspection, and the
entire temporary run directory was then removed.

Published and materialized file SHA-1 values were:

- `wanted/start.bin`:
  `dbdc27359b5e3e4b29215c5b06fa040cfa512abf`;
- `later.bin`: `62379fb375055577941732d7abf9401a8940eafa`;
- `wanted/end.bin`:
  `7190d6915399c7318f2f0955bcd757cb9a0f157f`;
- `wanted/empty.bin`:
  `da39a3ee5e6b4b0d3255bfef95601890afd80709`; and
- `tail.bin`: `f7494d006948bdb20280042ea3962d990f68c75f`.

Both prior profiles also passed three fresh runs:

- the 40,000-byte small profile completed in 0.054, 0.057, and 0.053 seconds
  with three blocks, a 40,000-byte high-water, and exact payload equality;
- the 32 MiB profile completed in 8.988, 11.889, and 14.213 seconds with 2,048
  blocks, a 256 KiB high-water under its 256 KiB allowance, a 16 KiB
  verification buffer, and exact payload equality.

### Validation And Audits

These commands passed:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv lock --project tests/interop --check
uv run --project tests/interop --locked \
  python -m py_compile tests/interop/first_verified_piece.py
python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

The managed references were clean at:

- BitTorrent BEPs `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`;
- rqbit `4e5f94cbcf1d57ec500885c77cf1e24d70232d89`;
- libtorrent `7d7fc38fac61177fa5e02148f791b2f65250b09d`
  (`v2.0.13`); and
- JSTorrent `main@0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`.

No dependency manifest or lockfile changed. `cargo tree --workspace --locked`
confirmed the existing Rust dependency graph, and the locked Python oracle
continued to use libtorrent as a separate process peer. No source, fixture, or
format was copied from a reference implementation. The final artifact audit
found no generated torrent, payload, part-file, Python bytecode, or temporary
test output in tracked source paths.

### Deliberate Limits And Next Boundary

The stopping condition is satisfied. The format records placement, not
unfinished block completion or a persistent verified-piece bitfield. The
diagnostic still has one peer, one active piece, no corruption retry, binary
startup-only selection, and no tracker or general session policy. Directory
syncing, pre-existing content reuse, per-file early publication, compaction,
and hole punching remain outside this slice.

The part file deliberately follows libtorrent's piece-sized slot geometry,
which can create large sparse logical offsets. Desktop tests prove checked
64-bit addressing but do not establish physical allocation behavior through
Android SAF providers. The recommended next tactical is therefore a bounded
physical Android/ChromeOS storage feasibility probe before accepting this
desktop format as the product storage seam.
