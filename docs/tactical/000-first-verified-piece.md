# Tactical 000: First Verified Piece

Status: ready; implementation has not started.

## Motivation And Outcome

Establish the smallest honest vertical thread through the new engine: consume
a controlled v1 metainfo file, connect directly to one known peer, speak the
BitTorrent peer protocol, assemble one piece from multiple blocks, verify its
SHA-1 hash, and write the verified payload.

The result should be deep enough to expose real boundaries among protocol,
state, runtime, hashing, and output without pretending to be a generally useful
torrent client. Rasterbar libtorrent supplies an independent loopback peer, and
a Python harness owns fixture creation and process orchestration.

Success is externally observable: a fresh deterministic payload seeded by
libtorrent is reproduced byte-for-byte by RSTorrent, with the expected piece
hash, before a fixed timeout.

## Dependencies And References

- [Product and engine direction](../topics/product-direction.md)
- [Engine engineering principles](../engineering-principles.md)
- [Reference policy and license posture](../references.md)
- [BEP 3: The BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html)
- Rasterbar libtorrent `v2.0.13`, pinned in
  [`reference/pins.toml`](../../reference/pins.toml), as a separately running
  peer and fixture generator
- rqbit at its managed pin as a Rust implementation comparison
- JSTorrent's Python integration scenarios as behavioral evidence, not source
  to copy

Before implementation, verify the local reference set with:

```bash
python3 scripts/references.py sync
python3 scripts/references.py status
```

The harness may use Rasterbar libtorrent's Python bindings. It must print the
actual binding and libtorrent versions used so the completed tactical can
record them. It must not use or build libtorrent's GPL-3.0 `libsimulator`
submodule.

## Reference Observations Shaping This Slice

These are inputs, not inherited architecture:

- rqbit gives peer-wire encoding its own crate with no direct Tokio or socket
  I/O use, but that crate depends on `librqbit-core`, whose dependency graph and
  public modules include Tokio task utilities. RSTorrent will enforce a
  stricter lower-layer dependency boundary.
- libtorrent provides mature behavioral and resource-management evidence, but
  its peer connection and session code are integrated with its C++ asynchronous
  networking architecture. It is more valuable here as an executable oracle
  than as a module template.
- JSTorrent demonstrates that a Python-owned libtorrent seeder, deterministic
  generated payload, explicit peer address, bounded subprocess, and final byte
  comparison form an effective interoperability seam. RSTorrent's harness will
  be independently authored for this narrower contract.

No reference source, fixture, or BEP prose is to be copied into this slice. If
implementation reveals a reason to import material, pause and update the
provenance and license record before doing so.

## Scope

### Controlled fixture

The interoperability harness will:

- create a temporary directory and deterministic, nontrivial payload;
- create a single-file, v1-only `.torrent`;
- choose a piece length larger than the payload while keeping the payload
  larger than one 16 KiB request block, so the torrent contains exactly one
  multi-block piece;
- start a libtorrent seed bound to loopback on an operating-system-selected
  port;
- disable discovery and unrelated network services, including DHT, LSD, UPnP,
  NAT-PMP, and trackers;
- pass the `.torrent`, explicit `127.0.0.1:<port>` peer, and output path to a
  narrow RSTorrent diagnostic command;
- apply fixed startup and completion timeouts and preserve useful process and
  libtorrent diagnostics on failure; and
- compare the output bytes and digest with the generated source.

The fixture is generated at test time. It is not copied from JSTorrent,
libtorrent, rqbit, or the BEP repository.

### Metainfo and hashing

Implement only the bencode and v1 single-file metainfo behavior required for
the controlled fixture, while making the parser reject malformed, truncated,
overflowing, excessively nested, or unexpectedly large input without panics.

The v1 info hash must be computed from the exact bencoded byte span of the
original `info` dictionary, not from a decoded and re-encoded representation.
The expected piece hash must come from the metainfo. SHA-1 itself may be
provided by a focused, reviewed Rust cryptographic dependency; the torrent
protocol behavior remains first-party.

### Peer wire

Implement the ordinary TCP handshake and the smallest core message subset
needed to download from the controlled libtorrent peer:

- handshake validation, including protocol string and v1 info hash;
- length-prefixed framing and keepalives;
- peer availability observed through `bitfield` and `have`;
- `choke` and `unchoke`;
- outbound `interested`;
- outbound `request`; and
- inbound `piece`.

Unsupported or invalid messages must produce an explicit, bounded error or an
explicitly justified ignore action; they must not be mistaken for success.
Frame and block lengths must be validated before allocation or indexing.

### One-piece download state

Represent one piece as multiple request blocks. The deterministic state
transition layer owns which blocks are missing, requested, or received and
must:

- send no requests while the peer is choked;
- request only blocks inside the selected piece;
- reject blocks with an unexpected piece index, begin offset, or length;
- tolerate valid block arrival independent of request ordering;
- assemble every byte exactly once;
- verify the complete piece before exposing it as successful output; and
- return a typed failure on hash mismatch rather than writing corrupt data.

A general piece picker, fairness policy, or multi-peer scheduler is not part of
this tactical.

### Runtime driver and diagnostic command

Use Tokio in the outer engine/runtime layer for TCP and timeouts. One owner
should drive the initial peer connection and its state; this slice does not
need shared mutable session state or a task graph.

Provide a narrow diagnostic command suitable for the Python harness. Its
arguments and output are test surfaces, not a stabilized product CLI or
application API. It must return a nonzero status on parse, connection,
protocol, timeout, hash, or output failure.

Write bytes only after successful piece verification. General random-access
storage, preallocation, resume state, and crash-consistent persistence are
deferred.

## Initial Dependency Direction

Start with the smallest workspace that makes the critical boundary enforceable:

```text
rstorrent-protocol
    bencode, metainfo, peer-wire values/codecs,
    one-piece deterministic state transitions

rstorrent-engine
    Tokio TCP driver, timeouts, verified output,
    narrow diagnostic binary and integration seam

tests/interop
    Python/libtorrent orchestration
```

`rstorrent-engine` may depend on `rstorrent-protocol`.
`rstorrent-protocol` must not depend on `rstorrent-engine`, Tokio, an async
runtime, sockets, filesystem APIs, task handles, channels, or a clock. Its
normal behavior must be testable with byte slices, values, events, and actions.

Separate bencode, metainfo, peer wire, and download state into coherent modules
inside the protocol crate. Do not create more crates until ownership,
dependency, reuse, or independent-test evidence justifies them.

## Contracts And Invariants

- Reference implementations are test peers and evidence, never runtime engine
  dependencies.
- Protocol parsing and state transitions are deterministic and perform no I/O.
- Async and platform types do not appear in protocol-layer public contracts.
- Network input is length-bounded before allocation, slicing, or state change.
- The peer's handshake info hash must equal the metainfo's exact v1 info hash.
- No request is emitted for unavailable data or while the connection is
  choked.
- Received data cannot be reported or written as complete before SHA-1
  verification succeeds.
- Every failure path terminates within a configured timeout and reports enough
  context to identify its layer without dumping payload contents.
- Temporary files, peer processes, and sockets are cleaned up on success and
  failure.
- The test uses loopback only and requires no public tracker or Internet peer.

## Boundary Awareness

During implementation and review, keep an eye out for:

- unclear ownership of an invariant among codecs, domain state, orchestration,
  runtime I/O, storage, and test infrastructure;
- Tokio, networking, filesystem, clock, channel, or process types crossing
  inward into the protocol crate;
- modules accumulating unrelated responsibilities or becoming difficult to
  understand and test;
- important behavior that requires unrelated infrastructure to test; and
- repeated policy that suggests a missing shared concept.

Refactor when a better boundary has a concrete benefit and the work remains
proportionate to this slice. Otherwise, continue the bounded work. Record a
deferral only for a material known problem, and do not manufacture modules,
traits, or crates for hypothetical future needs.

## Non-Goals

- BitTorrent v2 or hybrid metainfo
- multi-file torrents
- trackers, magnets, metadata exchange, DHT, PEX, LSD, or peer discovery
- incoming peer connections or seeding
- more than one peer or one piece
- uTP, IPv6, proxies, or protocol encryption
- a general piece picker, bandwidth policy, connection pool, or session model
- retries after corruption, peer replacement, or durable resume
- production storage allocation or random-access abstractions
- a stable public engine API, application command/snapshot/event contract, or
  product CLI
- desktop or Android UI, bindings, services, notifications, or SAF
- benchmarks or performance claims
- copying implementation source or fixtures from a reference repository

## Implementation Sequence

1. Establish the workspace and enforce the inward dependency direction.
2. Implement and unit-test bounded bencode, exact-span v1 metainfo, and peer
   framing.
3. Implement and unit-test the pure one-piece transition model, including
   invalid blocks and hash failure.
4. Connect the model to a single Tokio TCP driver and verified output path.
5. Independently author the Python/libtorrent loopback harness and diagnostic
   command.
6. Run the complete validation set, check for material boundary or dependency
   problems, remove investigation artifacts, and record exact evidence here.

Later steps may refine earlier code. Passing the oracle once is not a reason to
ignore clear architectural problems, negative tests, or the standard Rust
validation.

## Validation

Source the configured toolchain before Rust or Python commands:

```bash
source ~/.profile
```

The completed tactical must run and record:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
python3 scripts/references.py status
```

Implementation must add one documented command that runs the deterministic
Python/libtorrent loopback scenario. Run it at least three consecutive times
from clean temporary directories and record:

- the command;
- Rust toolchain version;
- Python version;
- libtorrent binding and native library versions;
- fixture payload and piece sizes;
- expected and actual hashes;
- elapsed time; and
- whether cleanup and all three runs succeeded.

Unit validation must cover at least:

- exact raw `info` span hashing;
- truncated and structurally invalid bencode;
- oversized or invalid peer frame lengths;
- fragmented input and multiple frames in one input buffer;
- handshake protocol and info-hash mismatch;
- choke/unchoke request gating;
- duplicate, overlapping, out-of-range, short, and unexpected piece blocks;
- valid multi-block assembly; and
- failed and successful final piece verification.

Add an automated architecture check that fails if the protocol crate acquires
Tokio, engine, socket, filesystem, task, channel, or clock dependencies.

## Stopping Condition

This tactical is complete only when a documented command reproducibly creates
a fresh deterministic, single-file v1 torrent; seeds it from Rasterbar
libtorrent on loopback; launches RSTorrent with an explicit peer; receives one
piece through multiple request blocks; verifies it against the metainfo SHA-1;
writes exactly the original bytes; and exits successfully within the timeout
for three consecutive clean runs.

The standard Rust checks, negative protocol/state tests, architecture boundary
check, reference status, actual dependency licenses, and exact interoperability
evidence must also be recorded below. A successful handshake, receipt of
unverified bytes, or a passing in-process mock alone does not satisfy the
stopping condition.

## Execution Record

Not started.

When work begins, keep this section current with:

- boundary decisions and deferred extractions;
- dependency and license findings;
- implementation status;
- exact validation commands and results;
- interoperability environment and evidence;
- known gaps; and
- recommended tactical `001`.
