# Tactical 006: Magnet Metadata Through A Peer Hint

Status: completed on 2026-07-30.

## Motivation And Outcome

RSTorrent currently requires a complete `.torrent` and explicit peer socket
address before it can enter the bounded content pipeline. Magnet links are a
primary product input, and BEP 9 already defines a deterministic way to test
them without introducing trackers or DHT: a v1 magnet may carry one or more
`x.pe` peer hints specifically to initiate direct metadata transfer.

Implement a deep bidirectional metadata slice. Starting with only a v1
`xt=urn:btih:...` topic and loopback `x.pe` hint, RSTorrent connects to a
controlled libtorrent seed, negotiates BEP 10, downloads the raw info
dictionary through `ut_metadata`, validates its SHA-1 against the magnet, and
continues over the same peer connection through the existing bounded
selective-content path. In the other direction, RSTorrent accepts one
loopback peer and serves its independently validated info dictionary to a
libtorrent client that started from a magnet.

This establishes magnet identity, metadata ownership, extension negotiation,
and the metadata-to-content transition. It does not introduce general peer
discovery or claim payload seeding.

## Dependencies And References

- [First verified piece execution record](000-first-verified-piece.md)
- [Bounded large-piece execution record](001-bounded-large-piece.md)
- [Selective multi-file storage execution record](002-selective-multi-file-storage.md)
- [SAF selective-storage execution record](005-saf-selective-storage.md)
- [Product and engine direction](../topics/product-direction.md)
- [Engineering principles](../engineering-principles.md)
- [BEP 3: The BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html)
- [BEP 4: Assigned Numbers](https://www.bittorrent.org/beps/bep_0004.html)
- [BEP 9: Extension for Peers to Send Metadata Files](https://www.bittorrent.org/beps/bep_0009.html)
- [BEP 10: Extension Protocol](https://www.bittorrent.org/beps/bep_0010.html)
- The pinned libtorrent `v2.0.13` magnet parser, extension protocol, and
  `ut_metadata` implementation and tests
- The local JSTorrent magnet, peer connection, metadata fetcher, and
  integration tests as behavior and failure-case references

No reference source or fixture is copied. Tests are independently authored
against the public protocol and observable interoperability behavior.

## Reference Findings

- BEP 9 transfers only the exact bencoded info dictionary. It uses 16 KiB
  blocks, appends data after the bencoded response dictionary, and validates
  the completed bytes against the magnet info hash.
- `x.pe` accepts hostname/port, IPv4/port, or bracketed IPv6/port forms and may
  repeat. It exists specifically to reduce dependence on external peer
  sources during metadata transfer.
- BEP 10 advertises support in reserved byte 5 bit `0x10`, uses standard
  message ID 20, and assigns extension IDs per direction and per peer.
- Extension handshakes may repeat. Their `m` mappings are additive, an ID of
  zero disables an extension, and unknown extensions and fields are ignored.
- The pinned libtorrent reference keeps at most two outgoing metadata
  requests in flight, bounds one metadata extension packet to approximately
  17 KiB, rejects invalid request indices, checks consistent total size, and
  accepts metadata only after info-hash verification.
- RSTorrent already rejects bencode inputs larger than 1 MiB. Metadata uses
  that existing ceiling instead of adopting a peer-controlled or
  reference-specific larger allocation.

## Scope

### V1 magnet input

Add a runtime-independent magnet parser with these contracts:

- the URI is UTF-8 and at most 16 KiB;
- at most 128 query parameters are examined;
- exactly one distinct v1 `btih` identity is accepted;
- both 40-character hexadecimal and 32-character base32 hashes are accepted
  case-insensitively;
- repeated identical `btih` topics are deduplicated and conflicting topics
  are rejected;
- v2-only and hybrid `btmh` inputs are explicitly unsupported;
- percent escapes are validated before interpretation;
- at most 32 valid `x.pe` hints are retained;
- hostnames are at most 253 bytes and ports are in `1..=65535`;
- IPv4, hostname, and bracketed IPv6 forms are parsed without accepting
  userinfo, paths, missing ports, or ambiguous unbracketed IPv6;
- malformed individual peer hints are ignored, but the explicit-peer
  diagnostic requires at least one valid hint; and
- unknown query parameters, including trackers, are bounded and retained
  neither as commands nor implied support.

The protocol value stores peer host and port separately. DNS resolution and
the diagnostic loopback restriction remain runtime concerns.

### Extension transport

The standard handshake exposes all eight reserved bytes and can deliberately
set the BEP 10 bit. Existing non-extension callers retain their current
behavior.

The peer-wire decoder accepts message 20 under a separate 17 KiB extension
frame ceiling and emits the extension ID and bounded payload without parsing
extension policy in the transport codec. Core piece messages retain the
existing 16 KiB block bound.

Add deterministic BEP 10 parsing and encoding for:

- the extension handshake under extension ID zero;
- the peer-local `ut_metadata` mapping;
- optional bounded `metadata_size`;
- repeated additive handshake updates;
- explicit disable through mapping ID zero; and
- ignored unknown extension names and top-level fields.

Extension IDs are never treated as global constants. RSTorrent advertises one
stable local receive ID, but sends each peer the ID that peer advertised.

### Metadata download

Add deterministic metadata state with:

- a 16 KiB metadata block size;
- a 1 MiB total allocation ceiling, matching bounded bencode parsing;
- at most 64 metadata blocks;
- no allocation until a valid bounded size is observed;
- a fallback request for block zero when `metadata_size` is absent, with
  allocation only after a valid `total_size` response;
- at most two in-flight requests;
- exact last-block length validation and exact 16 KiB non-final blocks;
- requested-piece membership checks for data and rejects;
- idempotent handling of identical duplicates without double accounting;
- rejection of conflicting duplicate data, negative or out-of-range indices,
  inconsistent sizes, unsolicited data, malformed dictionaries, and trailing
  bytes on non-data messages;
- explicit peer reject, disconnect, timeout, and extension-disable outcomes;
- SHA-1 verification of the exact assembled byte string before parsing or
  changing torrent state; and
- no exposure of a display name, file list, lengths, or piece hashes from
  unverified metadata.

The raw verified dictionary is parsed through the same v1 metainfo validation
used for `.torrent` input. The shared parser must preserve path safety,
piece/file limits, checked geometry, padding semantics, and v2 rejection.

### Premetadata peer state and content handoff

A real seed may send choke state, bitfield, or HAVE messages before metadata
is available. Do not reconnect or silently discard those messages to make the
happy path easier.

Retain only bounded state:

- the latest choke state;
- one bitfield no larger than the maximum supported piece count requires; and
- at most the maximum supported number of distinct HAVE indices.

After metadata verification, validate the deferred bitfield shape and HAVE
indices against the now-known piece count. Apply them to the existing piece
state before requesting content. Continue on the same TCP connection with the
same decoder so fragmented or coalesced frames are not lost.

The successful interoperability fixture has an info dictionary larger than
one metadata block and then completes the existing path-backed selective
download and verified publication. No `.torrent` bytes are supplied to
RSTorrent out of band.

### Metadata upload

Add a bounded loopback diagnostic server that:

- reads and validates a complete `.torrent` before listening;
- extracts the exact raw info-dictionary bytes and independently verifies
  their v1 info hash;
- accepts one TCP peer and validates the standard handshake identity;
- advertises BEP 10 and `ut_metadata` with exact `metadata_size`;
- learns the peer's directional metadata ID from its extension handshake;
- serves exact 16 KiB blocks and a correctly sized final block;
- rejects negative, overflowed, or out-of-range requests;
- rejects requests when metadata is unavailable;
- bounds total and queued requests so a peer cannot create an unbounded upload
  queue;
- tolerates unknown extensions and repeated legal extension handshakes;
- closes and reports an observable terminal result after the independent
  client obtains all metadata blocks or on timeout/failure; and
- never serves piece payload, advertises content availability, or claims
  torrent seeding.

A libtorrent client starts from only the corresponding magnet and the
RSTorrent listener as `x.pe`. Success requires libtorrent to report valid
metadata with the exact info hash and expected files; process liveness alone
is insufficient.

### Lifecycle and observability

Metadata acquisition and serving have explicit owners, timeouts, cancellation
or terminal paths, and socket joins. Structured reports distinguish:

- magnet parsed;
- peer hint resolved and connected;
- extension support negotiated;
- metadata size accepted;
- metadata block requested, received, rejected, or served;
- metadata hash accepted or rejected;
- verified metadata parsed;
- transition to content; and
- terminal success, peer failure, protocol failure, timeout, or cancellation.

High-volume payload and metadata bytes are not logged. Peer-controlled names
and host strings are bounded before inclusion in errors.

## Contracts And Invariants

- Magnet, bencode, extension, and metadata state remain independent of Tokio,
  sockets, filesystems, channels, and platform adapters.
- Peer-controlled sizes, indices, counts, frames, host strings, query
  parameters, queued state, and work are validated before allocation or
  mutation.
- The 1 MiB metadata allocation is separate from the existing piece-payload
  allowance and is released after verified parsing and handoff.
- Extension IDs are directional and peer-local.
- Only the SHA-1-authorized raw info dictionary becomes `Metainfo`.
- Existing payload reservation, storage placement, piece verification,
  selective publication, and cleanup behavior is reused rather than forked.
- A metadata hash failure cannot create storage, request content, or expose
  metadata-derived product state.
- Metadata upload serves only bytes extracted from validated metainfo.
- Every socket and background task has one terminal path and is joined.
- Tracker, DHT, and unrelated extension traffic remain disabled in controlled
  evidence.

## Nasty Cases Required Up Front

Unit or scripted-peer evidence covers at least:

- empty, oversized, non-magnet, missing-`xt`, invalid percent escape,
  malformed hex/base32, conflicting `btih`, v2-only, and hybrid magnets;
- peer-hint count/host/port bounds, invalid IPv4, bracketless IPv6, duplicate
  hints, hostname hints, and mixed valid/invalid hints;
- absent extension bit, malformed extension handshake, missing `m`, missing
  `ut_metadata`, ID zero, ID outside one byte, repeated additive update,
  remapped ID, and unknown extensions;
- absent, zero, negative, oversized, and changing `metadata_size`;
- fragmented and coalesced extended frames;
- metadata dictionaries with missing/wrong fields, unknown message types,
  negative/overflowed/out-of-range pieces, inconsistent `total_size`, wrong
  block lengths, trailing bytes on request/reject, unsolicited data,
  duplicate data, reject, and disconnect;
- an advertised metadata size just over 1 MiB without a large allocation;
- corrupt assembled metadata with the correct geometry but wrong SHA-1;
- a hash-correct info dictionary that still violates metainfo limits or path
  safety;
- bitfield and HAVE before metadata, including invalid post-metadata shape and
  indices;
- upload requests before extension negotiation, invalid upload indices,
  repeated requests, request flood, disconnect, and timeout; and
- cleanup after every failure before content storage begins.

## Non-Goals

- tracker announces, DHT, PEX, LSD, NAT traversal, or general peer discovery
- magnets without an explicit usable peer hint in the end-to-end diagnostic
- BitTorrent v2 or hybrid metadata
- BEP 53 select-only magnet parameters
- metadata persistence or durable resume
- multiple simultaneous metadata peers or general peer replacement
- payload upload, choking policy, seeding, ratios, or incoming swarm service
- Android UI, product link routing, background intent policy, or a stable
  public application API
- arbitrary non-loopback network access in the diagnostic

## Architecture Direction

Add coherent protocol modules for magnet values and metadata-extension state.
Keep message-20 framing in `peer_wire`, raw info parsing in `metainfo`, and
Tokio connection ownership in the engine. Do not add a generic extension
framework beyond the concrete BEP 10 ownership and dispatch needs proven here.

Refactor the download driver only enough to accept either validated `.torrent`
metadata with a fresh peer or verified magnet metadata with an already
negotiated peer. Both sources converge before selection, storage, piece
requests, hashing, and publication.

The metadata upload listener is an explicit diagnostic capability, not a
session daemon. Its protocol state should be reusable by future payload
seeding, but this tactical does not invent that broader owner.

## Implementation Sequence

1. Close Tactical `005` with its unavailable and unrun evidence stated
   explicitly, record this tactical, and update living direction.
2. Add bounded magnet parsing, raw info-dictionary parsing, handshake reserved
   bits, message-20 framing, and deterministic BEP 10/BEP 9 codecs and state.
3. Add explicit-peer resolution, metadata acquisition, bounded premetadata
   state, and same-connection handoff into existing content download.
4. Add the validated metadata upload listener and one-peer diagnostic binary.
5. Extend the controlled harness with multi-block metadata, magnet-only
   content download, libtorrent metadata-leech evidence, and scripted hostile
   peers.
6. Run the full repository and interoperability baseline, remove temporary
   artifacts, and record exact evidence and remaining limits.

## Validation

The execution record must list every command actually run. Expected baseline:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv run --project tests/interop \
  python tests/interop/magnet_metadata.py --runs 3
python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

Existing small, 32 MiB large-piece, selective-file, Android native, and
architecture tests remain green in proportion to touched boundaries.

## Stopping Condition

This tactical is complete when:

- all specified parser, codec, state, allocation, ordering, and failure cases
  pass deterministically without Tokio or sockets where I/O is unnecessary;
- three clean runs start RSTorrent from only a v1 magnet and loopback `x.pe`,
  fetch multi-block metadata from libtorrent, validate its info hash, continue
  on the same connection, and publish byte-identical selected content;
- libtorrent starts from only a magnet and RSTorrent `x.pe`, receives all
  metadata blocks from RSTorrent, and exposes the exact expected v1 info hash,
  file list, and geometry;
- neither direction enables trackers, DHT, LSD, UPnP, NAT-PMP, uTP, or public
  networking;
- metadata, extension frames, premetadata state, requests, and upload work
  remain within their declared bounds under hostile input;
- metadata failure never reaches storage or content requests;
- all peers, listeners, processes, temporary outputs, and reference sessions
  terminate and clean up on every tested path; and
- the execution record states what landed, exact interoperability evidence,
  unsupported magnet/protocol surface, and the next persistence boundary.

## Execution Record

### Landed implementation

- `e4bec0e` closed Tactical `005` without claiming its unavailable evidence
  and recorded this bounded slice.
- `fdca634` added bounded v1 magnet parsing, exact bencode-prefix parsing,
  shared raw-info validation, BEP 10 reserved bits and message-20 framing,
  deterministic extension and metadata codecs, two-request metadata download
  state, and bounded metadata upload state.
- `76f619f` added loopback `x.pe` resolution, extension negotiation, metadata
  acquisition, bounded premetadata choke/bitfield/HAVE state, and same-socket
  handoff into the existing content pipeline.
- `4cb0f81` added the validated one-peer metadata server and
  `rstorrent-metadata-seed` diagnostic.
- `ce24cc8` added independent bidirectional libtorrent interoperability with
  multi-block metadata.
- `d5306a9` and `1f66856` covered extension refusal, invalid upload ordering,
  timeout, and disconnect cleanup at the socket boundary.
- `7cae526` kept Android's existing engine-failure classifier exhaustive
  without adding Android magnet routing to this tactical.

No runtime dependency was added. Protocol code remains independent from
Tokio, sockets, filesystems, task handles, and platform adapters.

### Bounds and failure evidence

The implementation enforces the declared 16 KiB metadata block, 1 MiB
metadata, 64-block, two-download-request, 17 KiB extension-payload, 32-peer
hint, 128-query-parameter, and 256-upload-request limits.

Runtime-independent tests cover hex and base32 identities, conflicting and v2
topics, percent escapes, hostname/IP/port forms, query and peer bounds,
extension mapping disable/remap/absence, missing and changing sizes, fallback
piece-zero discovery, invalid dictionaries and indices, data suffix framing,
wrong lengths, unsolicited and duplicate data, rejects, hash mismatch, and
upload floods. Existing metainfo tests remain the authority for path safety,
file and piece counts, geometry, padding, collision, and v2 rejection.

Scripted socket tests additionally prove:

- bitfield and unchoke state sent before metadata survive the same connection
  and drive content requests after verification;
- an invalid deferred bitfield, absent BEP 10 support, and a metadata-phase
  disconnect fail before storage exists;
- upload requests before directional-ID negotiation fail terminally;
- negative and out-of-range requests receive rejects before valid blocks;
- an idle listener and disconnected upload peer terminate within their owned
  lifecycle; and
- successful upload exits only after every distinct metadata block was
  served.

### Independent interoperability evidence

The locked Python binding and native library both reported libtorrent
`2.0.13.0`. The fixture's exact raw info dictionary was 26,686 bytes, spanning
two metadata blocks. It described 121 files and a 40,000-byte payload in three
content pieces. Its v1 info hash was
`a962f460b83861cfb5faa1d7ad7da9c3f3cc2fc4`.

Three consecutive clean runs each proved both directions:

1. RSTorrent received only a magnet with loopback `x.pe`, fetched and hashed
   both metadata blocks from libtorrent, parsed 121 files, continued into all
   three content pieces, and published the byte-identical payload. The
   scripted engine test separately proves this handoff retains the same
   socket and decoder.
2. Libtorrent received only the magnet with the RSTorrent listener as `x.pe`,
   accepted both exact metadata blocks, exposed the expected info hash and
   complete file geometry, and caused the one-peer server to exit with
   `blocks=2 requests=2`.

Every run reported cleanup success. DHT, LSD, UPnP, NAT-PMP, incoming and
outgoing uTP, trackers, and public addresses were disabled; the controlled
transport was loopback TCP.

The existing interoperability profiles also remained green:

- 40,000-byte single-piece baseline: three blocks, exact payload and hash;
- 32 MiB single-piece profile: 2,048 blocks with a 256 KiB payload
  high-water ceiling; and
- five-piece selective profile: four verified pieces, one skipped piece,
  exact selected/part byte accounting, reopen, materialization, and cleanup.

### Validation run

The final implementation was validated with:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv run --project tests/interop \
  python tests/interop/first_verified_piece.py --runs 1
uv run --project tests/interop \
  python tests/interop/first_verified_piece.py --large-piece --runs 1
uv run --project tests/interop \
  python tests/interop/first_verified_piece.py --selective-files --runs 1
uv run --project tests/interop \
  python tests/interop/magnet_metadata.py --runs 3
python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

Focused development validation also ran protocol and engine clippy/tests plus
the named metadata acquisition, upload, timeout, negotiation, and disconnect
tests. An initial full-workspace clippy run found Android's exhaustive
`DownloadError` classifier missing the new variants; `7cae526` corrected it,
and the complete workspace baseline then passed.

Reference integrity reported the managed libtorrent checkout at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`, the BEP checkout at
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`, and all other managed
references healthy.

### Deliberate limits and next boundary

This is verified v1 metadata exchange through explicit peer hints, not general
magnet support. Tracker announces, DHT, PEX, LSD, v2/hybrid torrents, public
peers, simultaneous metadata peers, metadata persistence, payload upload,
incoming swarm service, Android link routing, and a stable product API remain
unsupported.

Tactical `007` should define durable resume and recheck. Its first persistence
boundary should retain only hash-authorized raw info bytes and explicit source
and selection intent, then front-load atomic replacement, truncation,
corruption, version mismatch, stale intent, bounded startup validation, and
fixed-buffer payload recheck before application API or discovery breadth.
