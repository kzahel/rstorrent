# Tactical 074: Context-Specific Metainfo Limits

Status: Planned from maintainer direction on 2026-08-04. Implementation has
not started.

Topics: `protocol-support`, `peer-lifecycle`, `client-persistence`,
`capability-readiness`

## Decision And Motivation

Replace the accidental global relationship among bencode input bytes, BEP 9
metadata bytes, v1 piece count, durable-session shape, and future explicit
metainfo import with named, context-specific resource limits.

The current `MAX_BENCODE_INPUT_LENGTH` is one MiB. It supplies the default
bencode input bound, the BEP 9 metadata bound, the durable `raw_info` bound,
and the numerator used to derive `MAX_PIECES`. The bencode parser already
accepts a `Limits` value, but metainfo callers reconstruct only part of that
policy and production call sites do not name the trust and allocation boundary
they are enforcing.

SQLite still imposes a 26,214-piece and 3,311-byte have-state constraint left
behind by the former 512 KiB piece-hash parser ceiling. That is not an
independently justified durable resource limit: the parser and engine already
admit 52,428 pieces, and the controlled 10 GiB / 256 KiB geometry uses 40,960.
This tactical brings durable state up to the existing parser/engine ceiling
without increasing that ceiling.

One MiB is not a BEP 3 or BEP 9 format limit. It remains a valid conservative
limit for the already implemented peer-metadata path until a separate
interoperability and memory decision changes that path. It must not silently
become the maximum that the pure Rust parser can safely inspect for an
explicit local or authenticated import.

All metainfo bytes remain hostile regardless of whether they came from a peer,
an HTTP client, a browser picker, a native picker, a database, or a test. A
larger explicit-import profile means larger but still bounded hostile input;
it is not a trusted-parser bypass.

## Desired Outcome And Stopping Condition

The tactical stops when:

- every bencode and metainfo production caller selects a named limit context
  rather than inheriting an unrelated global maximum;
- bencode parsing enforces a total decoded-item budget in addition to
  input bytes, string bytes, depth, and per-collection entries;
- v1 metainfo parsing accepts an explicit limits value that independently
  bounds outer bytes, exact info bytes, pieces, files, paths, depth, decoded
  items, and collection size;
- the existing BEP 9 and generic wire paths retain their current accepted
  behavior and reject beyond their current bounds without partial state
  mutation;
- durable schema version 7 accepts the existing 52,428-piece parser/engine
  ceiling, bounds its encoded have state at 6,588 bytes, and preserves the
  one-MiB `raw_info` ceiling;
- a parser-only explicit-import profile accepts independently authored
  size-heavy and structure-heavy v1 metainfo fixtures above one MiB while
  rejecting byte, decoded-item, depth, collection, file, piece, and path
  adversaries at the declared bound;
- parser, engine, and durable piece-count limits no longer derive from a byte
  constant and every remaining difference is named and tested;
- durable byte, piece, or have-state excess returns a typed internal
  session/store resource error before a SQLite transaction begins;
- transient allocation and wall-time evidence for the explicit-import maximum
  are recorded; and
- the Rust workspace baseline passes and the owning topics record the exact
  implemented profiles and remaining product limits.

This tactical does not make `.torrent` intake a product capability. A parser
profile and evidence are complete even though no transport or application
command selects it yet.

## Initial Limit Profiles

Implementation may tighten an explicit-import value when deterministic
allocation evidence requires it, but may not change currently accepted
production input, raise these maxima, or expand product support without
direction.

| Context | Input / outer bytes | Exact info bytes | String bytes | Decoded items | Depth | Collection entries | Files | Pieces |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Generic current bencode | 1 MiB | n/a | 512 KiB | 1,000,000 | 32 | 4,096 | n/a | n/a |
| BEP 9 peer info dictionary | n/a | 1 MiB | 1,048,560 | 1,000,000 | 32 | 4,096 | 4,096 | 52,428 |
| Durable `raw_info` | n/a | 1 MiB | 1,048,560 | 1,000,000 | 32 | 4,096 | 4,096 | 52,428 |
| Explicit metainfo import parser | 16 MiB | 16 MiB | 16 MiB | 1,000,000 | 32 | 4,096 | 4,096 | 52,428 |

Every metainfo profile also retains at most 32 path components, 255 bytes per
component, and 4,096 bytes in one complete relative path. The input or exact
info byte ceiling remains authoritative, so bencode framing means a byte
string cannot consume literally every byte permitted by the enclosing input.

`max_decoded_items` follows RSTorrent's retained tree rather than
libtorrent's flat lexical-token representation. The root and every scalar,
list, or dictionary value count once; every dictionary key counts once more;
closing delimiters do not count because they create no retained tree element.
The parser rejects item `maximum + 1` before inserting it into a collection.

The explicit-import maximum is parser headroom, not a future HTTP or WebSocket
contract. A later intake tactical may choose a lower transport maximum,
introduce chunking, or revise the parser maximum from new evidence.

Schema version 7 changes only the durable piece-count and encoded have-state
checks from 26,214 and 3,311 bytes to 52,428 and 6,588 bytes. The latter is the
34-byte versioned header plus `ceil(52,428 / 8)` bits. The migration preserves
existing rows and the one-MiB `raw_info` check. A 52,429-piece torrent or any
other durable excess fails as a typed `StoreError` resource-limit variant
before a transaction rather than surfacing as a generic SQLite constraint
error.

This internal error does not add a generated application `ErrorCode` in this
tactical. Asynchronous magnet metadata acquisition retains a bounded
explanation in the torrent's existing error state. A future `.torrent` add
command may select a public resource-limit code with its transport contract.

The 16 MiB profile must keep measured transient parser memory, excluding the
caller-owned input buffer, at or below 128 MiB across two controlled resource
fixtures. A size-heavy fixture approaches the byte ceiling and proves exact
info-span hashing. A separate structure-heavy fixture approaches the decoded-
item ceiling without relying on one large ignored string. The implementation
records actual high water, wall time, and both fixture shapes.

## Stable Scenarios And Edge Cases

- A currently accepted BEP 9 info dictionary parses, hashes, downloads, and
  persists exactly as before.
- A peer advertising one byte more than the BEP 9 maximum is rejected before
  allocating the complete metadata buffer.
- A valid outer v1 metainfo larger than one MiB fails under a one-MiB outer
  bound and succeeds under the explicit-import parser profile with the same
  exact info span and SHA-1 identity.
- An outer file within the byte bound but beyond the decoded-item budget fails
  before building an unbounded node tree.
- One deeply nested value, one oversized collection, duplicate or unsorted
  dictionary keys, an oversized string, too many files, too many pieces, and
  an overlong path fail with the responsible named limit.
- An unknown large outer field cannot relax limits on the exact info
  dictionary or change the bytes used for the info hash.
- A valid 40,960-piece controlled geometry persists under schema version 7.
  The have-state and schema codecs accept their exact 52,428-piece boundary;
  piece 52,429, oversized `raw_info`, and oversized encoded have state are
  rejected by a typed session/store resource error before opening a SQLite
  transaction. The one-MiB `raw_info` ceiling may make a lower byte bound win
  for a complete minimally encoded dictionary near that piece ceiling.
- Reparse of persisted bytes uses an explicit durable profile. Database
  corruption or a future row outside that profile cannot establish trusted
  metadata on restart.
- Arithmetic at every `maximum + 1`, piece-hash-length, and block-count
  boundary is checked without overflow on 32-bit targets.

## Scope

- Extend the pure bencode `Limits` with a total decoded-item limit and
  enforce it in prefix, complete, strict-dictionary, and permissive-dictionary
  parsing.
- Add a plain runtime-independent metainfo-limits value and explicit
  `from_bytes_with_limits` / `from_info_bytes_with_limits`-style entry points.
  Exact names may follow existing Rust conventions.
- Keep convenience entry points only where their fixed profile is evident in
  the name or module ownership. No security-sensitive production caller may
  rely on an undocumented default.
- Decouple parser, BEP 9 assembly, engine availability, have-state, and SQLite
  bounds into named constants owned by the layer allocating the corresponding
  state.
- Add schema version 7 with only the bounded piece-count and have-state check
  expansion described above; preserve and validate existing rows.
- Audit every `MAX_BENCODE_INPUT_LENGTH`, `MAX_METADATA_LENGTH`, `MAX_PIECES`,
  fixed schema check, and metainfo parse call in protocol, engine, session,
  tests, and diagnostic binaries.
- Add independently generated semantic, size-heavy, and structure-heavy
  boundary fixtures plus a bounded allocation/time profile. Do not import a
  reference `.torrent` fixture.
- Preserve structured errors sufficiently to distinguish input bytes, decoded
  items, depth, collection entries, files, pieces, paths, and durable session
  capability. When more than one limit is exceeded, the earlier parser limit
  may win; custom-limit tests keep each semantic error independently
  reachable.
- Update protocol claims and the readiness row without claiming product
  `.torrent` intake or larger BEP 9 interoperability.

## Non-Goals

- A `.torrent` add command, HTTP endpoint, WebSocket attachment, Tauri picker,
  browser picker, chunked upload, or product UI.
- Deciding whether original outer metainfo is stored in SQLite, a profile
  blob directory, beside payload, only in memory, or not at all.
- Changing exact magnet-source retention, tracker-tier persistence, BEP 9
  `raw_info` placement, resume metadata, or session schema beyond the targeted
  version-7 check expansion.
- Increasing the BEP 9 one-MiB assembly limit, durable `raw_info` limit,
  52,428-piece parser/engine/durable ceiling, file count, engine resident piece
  state, or protocol support claim.
- Streaming or incremental bencode parsing, memory mapping, disk spooling, or
  a general upload framework.
- BEP 52 v2/hybrid metainfo or new outer metainfo fields.

## Reference Dossier

### Normative specifications

- `reference/bittorrent.org/beps/bep_0003.rst` defines bencoding, the exact
  encoded info-dictionary hash, v1 piece hashes, and single/multi-file shape.
  It specifies no maximum metainfo or info-dictionary byte size.
- `reference/bittorrent.org/beps/bep_0009.rst` transfers only the exact info
  dictionary in 16 KiB blocks and advertises its total size. It specifies no
  global maximum and permits client flood protection.

### Pinned libtorrent oracle

The required oracle is libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`.

- `include/libtorrent/torrent_info.hpp::load_torrent_limits` independently
  bounds file bytes, piece count, decode depth, and decode tokens. Its default
  file maximum is 10,000,000 bytes.
- `src/torrent_info.cpp` applies those limits separately while loading a file,
  a buffer, or an already-decoded node.
- `include/libtorrent/settings_pack.hpp::{max_metadata_size,max_piece_count,
  metadata_token_limit}` and `src/settings_pack.cpp` define distinct peer
  metadata limits rather than deriving them from file intake.
- `src/ut_metadata.cpp::on_extended` checks total peer metadata size before
  allocating its assembly buffer.
- `test/test_torrent_info.cpp` includes the `many_pieces.torrent` and malformed
  metainfo rejection matrix.
- `simulation/test_metadata_extension.cpp::ut_metadata_token_limit` proves
  the peer metadata token limit independently of successful exchange.

RSTorrent adopts the separation among transport bytes, decoded work, and
semantic structure. It intentionally retains tighter initial limits and its
own borrowing parser, error model, and ownership boundaries. Its decoded-item
definition follows its retained tree allocations rather than claiming numeric
equivalence with libtorrent's lexical tokens.

## Ownership, Tasks, And Dependency Direction

```text
raw bytes
  -> rstorrent-protocol bencode limits and borrowed node tree
  -> rstorrent-protocol metainfo structural limits and exact info hash
  -> rstorrent-engine BEP 9 assembly / piece-state limits
  -> rstorrent-session durable capability and SQLite checks
```

Parsing remains synchronous, deterministic, and runtime independent. This
tactical adds no task, channel, socket, filesystem owner, or cancellation
path. BEP 9's existing torrent owner remains responsible for reserving and
releasing the bounded assembly buffer; the parser does not learn about peers
or Tokio. Session capability checks may depend inward on protocol values, but
protocol code must not depend on SQLite or application configuration.

## Implementation Sequence And Gates

1. Add total decoded-item accounting to bencode and pass pure
   boundary/adversarial tests without changing current callers.
2. Introduce metainfo limit values and migrate pure metainfo entry points.
   Prove exact info-span hashing under every profile.
3. Name and migrate BEP 9, engine, durable reparse, have-state, and schema
   capability call sites, including schema version 7. Preserve all current
   peer and restart tests.
4. Add the semantic greater-than-one-MiB fixture plus separate size-heavy and
   structure-heavy allocation/time evidence without connecting any profile to
   an application command.
5. Run workspace gates and update the tactical, protocol-support topic,
   client-persistence topic, and readiness matrix with actual values.

Each stage must pass before the next broadens call-site behavior. A failure in
an existing controlled magnet transfer is a regression, not evidence that its
limit should be raised.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure bencode | Exact byte/decoded-item/depth/string/collection boundaries, prefix and complete parsing, strict/permissive dictionaries, overflow cases. |
| Pure metainfo | Exact info span/hash, current accepted fixtures, valid >1 MiB import fixture, every structural `maximum + 1`, 32-bit arithmetic review. |
| Scripted runtime | Oversized BEP 9 handshake/data rejected before allocation; cancellation and hash-failure behavior unchanged. |
| Persistence | Schema 6 migrates to 7 with existing rows intact; a valid 40,960-piece geometry persists; have-state and schema codecs accept 52,428 and reject 52,429; oversized raw info and have state fail as typed resource errors before a transaction; current raw info restarts, rehashes, and reparses under the named durable profile. |
| Controlled interoperability | Existing libtorrent metadata exchange passes unchanged in both directions. No large-metadata support claim is added. |
| Resource profile | Size-heavy and structure-heavy explicit-import fixtures record input size, decoded-item count, wall time, and transient memory high water within the declared bound. |
| Workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and `git diff --check`. |

No public swarm, visible UI, emulator, physical device, generated application
contract, schema migration beyond targeted version 7, or external fixture
download is required.

## Escalation And Next Boundary

Implementation may choose internal names, tighten a maximum within the table,
and fix same-boundary parser errors without further direction. Stop if evidence
requires increasing a declared maximum, changing a currently accepted input,
migrating durable schema beyond the targeted version-7 check expansion,
expanding piece-state memory, adding a dependency, adding a public generated
resource-limit contract, or selecting a `.torrent` transport or storage
policy.

The next boundary remains the maintainer discussion of `.torrent` intake,
transport framing/chunking, original-source storage, session/resume metadata,
and the present magnet/BEP 9 source-retention model.
