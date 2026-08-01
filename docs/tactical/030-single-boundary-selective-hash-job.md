# Tactical 030: Single-Boundary Selective Hash Job

Status: Active

Topics: `download-correctness`, `performance-and-live-evidence`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `029` reduced the common 256 KiB selective hash from 16 seeks and 16
reads to one seek and 16 reads, but its controlled median remained neutral and
every public screen still reached 66 occupied storage jobs. Each fixed read is
still a Tokio file operation that crosses into the runtime's blocking pool and
back before hashing the next chunk. Pinned libtorrent instead owns a complete
piece hash as one disk job and returns one completion to the session layer.

Move the common all-wanted selective piece hash behind one bounded
`spawn_blocking` job. The job uses positional reads from dedicated duplicated
handles, walks the already validated segment map in torrent order, retains a
single fixed 16 KiB buffer, and returns one digest or typed error. Establish
deterministic lifecycle and operation-shape evidence, then rerun the controlled
and public profiles before choosing another owner.

## Source Dossier

Pinned libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` is the completeness oracle. No
source or fixture is copied.

- `posix_disk_io.cpp::async_hash` submits one `read_and_hash` disk job for a
  piece. Its fixed 16 KiB buffer and sequential file mapping remain inside the
  disk owner; one handler returns to the session executor.
- `mmap_disk_io.cpp::do_hash` and `mmap_storage.cpp::hash` likewise keep the
  complete piece traversal below the session boundary and return one result.
- Libtorrent's hash threads, disk cache, mmap implementation, fence machinery,
  and job prioritization are important later completeness references, but are
  not prerequisites for testing this narrower ownership defect.

RSTorrent adopts the complete-operation boundary, not libtorrent's disk
architecture. `ContentStoragePipeline` retains queue admission and completion
ownership; `SelectiveStorage` retains logical mapping and verification input.

## Ownership And Bounds

Only a piece whose segments are wanted files or padding takes the new path.
This is the ordinary all-files-selected product path and the controlled
profile. A piece containing skipped-file bytes remains on the proven async
mixed-source implementation until positional part-file access has its own
bounded tactical evidence.

Before spawning, the storage owner duplicates at most one handle for each
wanted file touched by the piece and constructs a small immutable span list.
The blocking job owns those duplicates, uses platform positional reads that do
not mutate a shared cursor, hashes spans in torrent order, and closes every
duplicate on return. This adds no persistent descriptor cache and bounds
temporary descriptors by files crossed by one validated piece. Pieces are
hashed serially by the existing storage owner, so there is at most one such job
per torrent.

The job retains one `VERIFICATION_CHUNK_LENGTH` buffer and never allocates the
piece. Padding is synthesized. Positional reads loop across interrupted and
short system reads until the requested chunk is complete or return a typed I/O
failure. Offset and length conversions remain checked. Unix/Android and
Windows implementations must have the same contract; no unsafe I/O is
introduced.

Tokio cannot forcibly stop a running blocking filesystem call. This does not
weaken the existing contract: the storage owner already finishes its current
hash before observing cancellation. Shutdown waits for the bounded piece job,
then joins the storage owner and leaves have state conservative. Panics and
join failures become typed storage failures rather than detached work.

## Shape-Changing Edge Cases

- one all-wanted 256 KiB piece crosses the async/blocking boundary once and
  performs 16 positional fixed-buffer reads;
- a piece crossing two wanted files duplicates each file once and preserves
  torrent byte order without shared cursor mutation;
- padding before, between, or after file spans hashes exact zero bytes;
- a final short piece and maximum accepted piece keep the fixed buffer and
  checked offsets;
- a missing, truncated, or unreadable staging file returns a typed error and
  never marks the piece verified;
- a blocking task panic or runtime join failure cannot publish content or leak
  an owner task; and
- cancellation while the job is running waits for that bounded job, joins all
  owners, and leaves queues and payload accounting exact.

## Staged Implementation And Gates

1. Add a portable, safe positional-read helper and exact short-read/error
   tests without exposing runtime types to layout or protocol modules.
2. Prepare at most one duplicate handle per wanted file touched by an
   all-wanted piece, then execute its complete span traversal in one bounded
   blocking job. Retain the async mixed wanted/skipped/padding path.
3. Add deterministic tests for one-file operation shape, cross-file ordering,
   padding, truncation, task failure, and cancellation/join behavior.
4. Rerun the 32 MiB controlled profile three times and compare against
   Tactical `029`'s 1.121-second post-change median. Operation ownership is
   mandatory; timing is supporting evidence and may reject the hypothesis.
5. Run formatting, warning-denying workspace clippy, workspace tests,
   selective/mixed interop, comparator tests, and paired controlled
   publication.
6. Run three product tracker+DHT 50% screens and one complete screen if clean.
   Compare storage high water, milestone time, integrity, and cleanup.

The tactical completes when the common all-wanted hash crosses one blocking
boundary, positional reads are exact and portable on supported targets, every
owner joins cleanly, and retained evidence classifies the next bottleneck. If
storage no longer remains continuously saturated, the paired timeline selects
request service or peer utility. If it remains saturated and controlled
hashing improves, the next slice examines storage write operation ownership.
If the controlled profile remains neutral, do not stack storage changes from
speculation; use task-duration evidence to distinguish writes from hashes.

## Non-Goals

- skipped-file positional hashing, concurrent per-torrent hashes, a global
  disk pool, mmap, direct I/O, a block cache, write coalescing, or piece-sized
  buffers
- changing storage queue capacity, peer ranking, turnover, request windows,
  piece selection, connection limits, tracker/DHT behavior, or endgame
- durable single-file resume, incoming connections, seeding, BEP breadth, UI,
  Tauri, browser, AVD, or physical-device work

## Stopping And Escalation

No human decision is currently required. Stop only for an unavoidable unsafe
I/O requirement, new dependency or license posture, persistence compatibility
break, product-visible contract, destructive user-data action, visible or
physical-device interaction, or evidence requiring a general shared disk-pool
architecture. A neutral benchmark or public timeout is evidence, not a
blocker.
