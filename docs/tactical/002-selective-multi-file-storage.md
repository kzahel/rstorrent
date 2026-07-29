# Tactical 002: Selective Multi-File Storage Foundation

Status: ready; implementation has not started.

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

Not started.
