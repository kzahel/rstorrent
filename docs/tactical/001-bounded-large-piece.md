# Tactical 001: Bounded Large-Piece Pipeline

Status: ready; implementation has not started.

## Motivation And Outcome

Remove the first slice's known assumption that resident payload memory is
proportional to piece length.

Real v1 torrents may use unusually large pieces. A 32 MiB piece is large but
observed in practice, and a hostile or merely extreme metainfo file may declare
a 256 MiB piece. Requesting several such pieces must not multiply into
piece-sized allocations and an Android out-of-memory termination.

This slice will download one deterministic 32 MiB piece from Rasterbar
libtorrent while RSTorrent enforces a 256 KiB engine-owned peer-payload
allowance. Received 16 KiB blocks will be written to unverified staging
storage. Once every block is stored, RSTorrent will read the piece back in
fixed-size chunks, compute SHA-1 incrementally, and publish the output only
after verification.

Success is externally observable and internally accountable: the output is
byte-identical, the piece hash matches, the diagnostic reports a payload
high-water mark no greater than the configured allowance, and the same
scenario passes three consecutive times without retaining temporary files.

## Dependencies And References

- [Tactical 000 execution record](000-first-verified-piece.md)
- [Product and engine direction](../topics/product-direction.md)
- [Engine engineering principles](../engineering-principles.md)
- [Reference policy and license posture](../references.md)
- BEP 3; offline source:
  `reference/bittorrent.org/beps/bep_0003.rst`
- Rasterbar libtorrent `v2.0.13`, pinned in
  [`reference/pins.toml`](../../reference/pins.toml)

No reference source, fixture, or prose is copied into this slice.

## Libtorrent Observations

These observations describe the pinned libtorrent implementation and inform
RSTorrent's independent design:

- Peer payload and disk operations use 16 KiB blocks independently of piece
  length.
- `max_queued_disk_bytes` governs the user-space disk write/store-buffer
  watermark. The default is 1 MiB. When it is exceeded, peer connections stop
  accepting more payload until disk work drains.
- The disk buffer pool derives its target block count from that byte setting
  and uses high and low watermarks. The setting is backpressure, not a precise
  total-process cap: the implementation permits bounded overrun to prevent a
  connection from deadlocking without a buffer.
- Completed v1 pieces are hashed incrementally in 16 KiB blocks from the store
  buffer or storage rather than copied into a piece-sized hash buffer.
- Libtorrent 2 no longer manages the old `cache_size`; its normal disk backend
  relies on memory-mapped files and the operating system page cache while
  retaining a bounded user-space store buffer.
- `min_memory_usage()` composes several controls rather than promising one
  total-memory ceiling: disk queue, per-peer receive buffer, socket buffers,
  request queues, peer lists, alert queue, checking concurrency, and open-file
  count all contribute.
- Libtorrent accepts piece sizes far above 32 MiB. Its current structural
  maximum is 32,767 16 KiB blocks, just under 512 MiB.

RSTorrent will adopt the block-granular ownership, reservation-before-request,
backpressure, and streamed verification lessons. It will not inherit
libtorrent's class graph, mmap backend, settings surface, or allowance
semantics automatically.

## Scope

### Piece-size acceptance

Raise the controlled v1 single-piece metainfo ceiling to 256 MiB. Piece length
is an input/work bound, not a payload allocation size.

The protocol state may allocate metadata proportional to the number of 16 KiB
blocks in one selected piece. At 256 MiB that is 16,384 block records and is a
known bounded cost. It must not allocate a byte buffer proportional to piece
length.

Inputs above the accepted ceiling must return a typed error before allocating
piece state. Supporting libtorrent's nearly 512 MiB structural maximum is not
required without product evidence.

### Engine-owned payload allowance

Add a configurable byte allowance for peer piece payload owned by the engine.
The initial diagnostic default and interoperability setting is 256 KiB.

- The allowance must be at least one 16 KiB request block.
- A block's length is reserved before its request is emitted.
- Requested, received, and storage-pending blocks retain their reservations.
- A reservation is released only after the block payload has been consumed by
  storage, or after a request is cancelled or fails.
- The scheduler emits no request whose reservation would exceed the allowance.
- Releasing one or more reservations may refill the request window.
- Current and high-water reserved bytes are observable.
- Every terminal and cancellation path releases its reservations.

This allowance covers engine-owned requested peer payload. It is not a promise
about exact process RSS. The metainfo buffer, fixed network scratch buffer,
one bounded frame under decode, block-state metadata, allocator overhead,
runtime state, thread stacks, kernel socket buffers, and page cache have
separate bounds or external ownership. Record those distinctions rather than
folding them into a misleading number.

### Block lifecycle and backpressure

Replace the current missing/requested/received model with explicit transitions
that distinguish at least:

```text
missing -> requested -> writing -> stored
```

The pure state layer validates incoming blocks and emits a storage action. It
must not mark a block stored when merely received from the network. The runtime
acknowledges storage completion, at which point the state releases the payload
reservation and may emit another request.

Withholding storage acknowledgements is the deterministic slow-storage test:
the request window must remain stopped at its configured byte allowance.

Choke, disconnect, cancellation, storage failure, duplicate data, unexpected
data, and invalid transition handling must leave block state and reservations
consistent. Retrying after a real storage failure remains outside this
diagnostic, but the state transition must be testable.

### Unverified staging storage

Create a staging file beside the requested output and size it for the
controlled single file.

- Write each accepted block at its piece-relative offset.
- Await the write before acknowledging it as stored.
- Do not create or replace the final output path before verification.
- On timeout, protocol failure, hash failure, I/O failure, or cancellation,
  close and remove the staging file.
- On successful verification, flush file data, close the staging handle, and
  rename it to the final output path.
- Refuse to overwrite an existing final output or staging path in this
  diagnostic.

This is deliberately one concrete storage owner. Do not introduce a generic
filesystem framework. Extract a narrow capability only if deterministic
failure testing or a second real implementation makes it necessary during
the slice.

### Streamed verification

After all block writes complete:

- seek to the selected piece;
- read no more than one 16 KiB verification chunk at a time;
- feed SHA-1 incrementally in piece order;
- reject short reads and storage failures;
- compare with the expected metainfo hash; and
- finalize the staging file only on success.

The protocol state reports verified metadata, not a payload `Vec`.

### Peer framing ownership

Keep inbound piece frames capped at one 16 KiB block. Avoid an unnecessary
second heap copy when transferring a decoded block from the frame decoder into
the storage action, or account for the copy explicitly if evidence shows that
moving ownership would obscure the codec.

An unsolicited or invalid piece frame may consume one bounded decode buffer
before rejection. It must terminate the connection rather than accumulate.

### Interoperability harness

Extend the locked Python/libtorrent harness without invalidating tactical
`000`'s small default fixture. Add a documented large-piece command that:

- generates a deterministic 32 MiB payload without retaining another
  piece-sized comparison buffer;
- creates a v1-only torrent with exactly one 32 MiB piece;
- runs a loopback-only libtorrent seed with discovery disabled;
- launches RSTorrent with a 256 KiB payload allowance;
- compares source and output incrementally;
- checks the expected and actual SHA-1;
- parses and asserts RSTorrent's configured and high-water payload bytes;
- records block count, timing, versions, and cleanup; and
- passes three consecutive fresh runs under fixed timeouts.

The Python harness is a separate process and its memory is not part of the
RSTorrent engine allowance.

## Contracts And Invariants

- Accepted piece length never causes a piece-sized payload allocation.
- Engine-owned requested payload reservations never exceed their configured
  allowance.
- A request cannot be emitted without its prior reservation.
- Backpressure happens before additional payload is requested, not after an
  unbounded queue has accumulated.
- Received bytes remain unverified even after they are written to staging.
- A block becomes stored only after its write completes successfully.
- Verification reads and hashes fixed-size chunks in piece order.
- The final output path contains only a fully verified piece.
- Hash, I/O, protocol, timeout, and cancellation failures cannot leave the
  final output or leak reservations.
- Fixed and metadata bounds remain explicit alongside the payload allowance.
- Protocol state and accounting remain independent from async, filesystem,
  network, clock, and process types.
- Diagnostic counters describe engine-owned state and do not claim an exact
  process-RSS ceiling.

## Non-Goals

- more than one piece, peer, or file
- tracker, DHT, PEX, LSD, magnet, or metadata discovery
- a general piece picker or multi-torrent session
- concurrent disk writes or a disk thread pool
- mmap, direct I/O, cache policy, or Android SAF implementation
- durable resume metadata or reuse of an incomplete staging file
- corruption retry, peer replacement, or bad-peer attribution
- production output replacement and crash-consistent directory syncing
- a global allocator or exact total-process memory enforcement
- performance claims from the diagnostic throughput
- copying libtorrent's buffer pool or disk backend

## Initial Dependency Direction

The existing two-crate boundary remains:

```text
rstorrent-protocol
    metainfo bounds, block lifecycle, reservations, actions

rstorrent-engine
    socket/frame ownership, staging file, async writes,
    streamed readback hashing, timeout, cleanup, diagnostics
```

The SHA-1 implementation remains a focused commodity dependency. Storage and
runtime dependencies do not move inward.

## Implementation Sequence

1. Record this tactical and the libtorrent findings before code changes.
2. Refactor peer-frame ownership and the pure piece state so payload
   reservations and storage acknowledgements are explicit.
3. Add 256 MiB construction and slow-storage/backpressure tests.
4. Implement staging writes, streamed readback hashing, finalization, and
   cleanup in the engine.
5. Add diagnostic configuration and current/high-water reporting.
6. Extend the locked harness with streaming fixture generation/comparison and
   the 32 MiB scenario.
7. Run the full validation and three consecutive oracle runs.
8. Audit boundaries, dependency changes, artifacts, and exact evidence.

## Validation

Run and record:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
uv lock --project tests/interop --check
python3 scripts/references.py status
```

The final unit suite must cover:

- constructing the accepted 256 MiB piece state without resident payload;
- rejecting piece lengths over the product ceiling;
- initial requests limited by a small byte allowance;
- no additional requests while simulated storage remains slow;
- request-window refill only after storage acknowledgement;
- reservation release on choke, cancellation, and storage failure;
- duplicate, unsolicited, short, overlapping, and wrong-index blocks;
- out-of-order block writes and state acknowledgements;
- streamed successful and failed SHA-1 verification;
- short read, write failure, existing-output, timeout, and staging cleanup;
- no final output before verification; and
- the existing architecture boundary.

The documented interoperability command must pass three times and record:

- Rust and Python versions;
- libtorrent binding and native versions;
- payload and piece sizes;
- configured allowance and observed high-water bytes;
- block count;
- expected and actual SHA-1;
- elapsed time; and
- cleanup status.

## Stopping Condition

This tactical is complete when a documented command downloads a fresh
single-piece 32 MiB v1 fixture from libtorrent on loopback, while the diagnostic
reports no more than 256 KiB of reserved engine-owned payload; stores every
block as unverified staging data; hashes the stored piece using a 16 KiB
readback buffer; publishes exactly the source bytes only after SHA-1 succeeds;
and completes three consecutive clean runs.

The 256 MiB state construction, deterministic slow-storage backpressure,
failure cleanup, negative protocol/state tests, architecture check, standard
validation, reference status, and exact evidence must also pass and be
recorded below.

## Execution Record

Not started.
