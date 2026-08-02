# Tactical 000: First Verified Piece

Status: complete; independently interoperable loopback result recorded.

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
- [BEP 3: The BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html);
  offline source after reference sync:
  `reference/bittorrent.org/beps/bep_0003.rst`
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

Completed on 2026-07-29.

### Implementation And Boundaries

The workspace now contains:

- `rstorrent-protocol`, with bounded bencode, exact-span v1 metainfo
  parsing, peer handshakes and incremental framing, and the deterministic
  one-piece transition model;
- `rstorrent-engine`, with the single-owner Tokio TCP driver, whole-operation
  timeout, verified-only output path, and `rstorrent-download-piece`
  diagnostic; and
- `tests/interop`, with a locked Python 3.12 environment and independently
  authored Rasterbar libtorrent fixture, seed, subprocess, assertion,
  diagnostics, and cleanup orchestration.

Protocol code owns validation and state transitions. It receives byte slices,
values, messages, and actions, and has no runtime or I/O contracts. The engine
owns the socket, timeout, bounded metainfo read, peer I/O, and final file write.
It creates no background task: the diagnostic future is the only state owner
and its caller observes termination directly.

The architecture test permits only the focused `sha1` direct dependency and
scans protocol source for engine, runtime, network, filesystem, process,
thread, synchronization, task, and clock imports. The engine depends inward
on the protocol crate. No additional crate, trait layer, channel, shared
mutable session, or storage abstraction was justified by this slice.

Metainfo is capped at 1 MiB before a read can grow further. Bencode also bounds
string length, nesting, and collection entries. Peer input is read in fixed
16 KiB chunks; the decoder rejects input chunks over 64 KiB, frames larger
than one 16 KiB piece response, and more than 1,024 messages per push. The
piece model permits at most a 1 MiB piece, so its block state and request
pipeline remain bounded.

Output creation occurs only after the complete payload produces the metainfo
piece hash. General random-access storage remains correctly deferred. The
diagnostic also rejects non-loopback peers so this test surface cannot
silently become a general downloader.

### Dependency And License Findings

The committed Rust lockfile resolves the direct dependencies to `sha1` 0.10.7
(`MIT OR Apache-2.0`) and Tokio 1.53.1 (`MIT`). The complete target-inclusive
transitive graph was checked from Cargo package metadata:

- `bytes` 1.12.1, `generic-array` 0.14.7, `mio` 1.2.2,
  `tokio` 1.53.1, and `tokio-macros` 2.7.1 declare `MIT`;
- `block-buffer` 0.10.4, `cfg-if` 1.0.4, `cpufeatures` 0.2.17,
  `crypto-common` 0.1.7, `digest` 0.10.7, `libc` 0.2.189,
  `pin-project-lite` 0.2.17, `proc-macro2` 1.0.107, `quote` 1.0.47,
  `sha1` 0.10.7, `socket2` 0.6.5, `syn` 2.0.119, `typenum` 1.20.1,
  `version_check` 0.9.5, `windows-link` 0.2.1, and
  `windows-sys` 0.61.2 declare MIT/Apache-2.0 alternatives;
- `unicode-ident` 1.0.24 declares
  `(MIT OR Apache-2.0) AND Unicode-3.0`; and
- `wasi` 0.11.1+wasi-snapshot-preview1 declares
  `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`.

The locked test-only `libtorrent` 2.0.13 Python package declares BSD in its
wheel metadata. The pinned upstream core is BSD-3-Clause and its Python
binding source is Boost Software License 1.0, as recorded in the upstream
license. It runs as a separate test peer. No libtorrent source, fixture, or
`libsimulator` component was imported or linked.

At the completion of this tactical, all resolved third-party declarations
were permissive. The two local crates were unpublished and had no license
field because RSTorrent had not yet selected a public license. RSTorrent later
selected MIT; the current package manifests and root `LICENSE` record that
decision.

### Validation

The final validation ran from a clean worktree with the configured profile:

```text
rustc 1.97.0 (2d8144b78 2026-07-07)
cargo 1.97.0 (c980f4866 2026-06-30)
Python 3.12.3
```

These commands passed:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
uv lock --project tests/interop --check
python3 scripts/references.py status
git diff --check
```

`cargo test --workspace` passed 23 tests: two diagnostic argument tests,
20 protocol/metainfo/wire/state tests, and the architecture boundary test.
The cases include all negative and positive behaviors required by this
tactical. Reference status matched all four managed revisions:

```text
bittorrent-beps 7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06
rqbit            4e5f94cbcf1d57ec500885c77cf1e24d70232d89
libtorrent       7d7fc38fac61177fa5e02148f791b2f65250b09d
jstorrent        main@0cad4dacf540f5be42ee53c4f1e1da27aa1b3685
```

### Interoperability Evidence

The documented command ran three consecutive fixtures:

```bash
uv run --project tests/interop --locked \
  python tests/interop/first_verified_piece.py --runs 3
```

Environment and fixture:

```text
Python                    3.12.3
libtorrent binding        2.0.13.0
libtorrent native library 2.0.13.0
payload size              40000 bytes
piece size                65536 bytes
request blocks            16384, 16384, and 7232 bytes
expected payload SHA-1    576143b2992ecf25c780ff41c79552f3bb50941b
info hash                 6096ba8e2f2855522ca32e9221c0976708e5646e
```

All three outputs were byte-identical to their freshly generated sources.
Actual SHA-1 equaled the expected SHA-1 on every run. End-to-end elapsed
times were 0.051, 0.053, and 0.054 seconds. The RSTorrent diagnostic exited
successfully, each libtorrent session terminated, every temporary directory
was removed, and the harness reported `all_runs=3 cleanup=ok result=pass`.

### Capability And Known Gaps

The controlled single-file v1, single-piece, explicit-peer path is
implemented, unit tested, and independently interoperable. It is not
real-world exercised or product supported. The diagnostic and its arguments
remain test surfaces.

The non-goals remain non-goals rather than incomplete work. In particular,
there is no multi-piece scheduling or random-access output, peer discovery,
tracker client, retry policy, resume, or seeding. No material boundary problem
was found that requires a deferred extraction inside this slice.

The initial next-slice recommendation was a complete multi-piece, single-file
download. Before tactical `001` was drafted, the maintainer selected the more
fundamental large-piece memory invariant instead. Tactical `001` completed
that bounded pipeline; the multi-piece explicit-peer path remains the
recommended tactical `002`.
