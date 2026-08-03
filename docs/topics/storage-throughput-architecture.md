# Maximum-Throughput Storage Architecture

Topic: `storage-throughput-architecture`

Status: Accepted by maintainer direction on 2026-08-02. Tacticals
[`052`](../tactical/052-batched-durability-checkpoints.md) and
[`053`](../tactical/053-immutable-positional-storage-plans.md) completed the
durability split and immutable positional-plan foundation. Tactical
[`054`](../tactical/054-bounded-independent-storage-execution.md) completes
the bounded generation join, independent write/hash execution, large-transfer
gate and application-path correction. The accepted end-state still leaves
measurement of pending-write read-through and session/root fairness as later
slices. Tactical `063` implements live path-backed file selection with a
coarse joined torrent-generation fence, retained physical routes, lazy part
creation, and exact verified-span promotion. Tactical `067` now routes path
and Android SAF files through one session-wide 40-descriptor pool and performs
all payload I/O in Rust after lazy platform capability acquisition.

## Purpose And Scope

RSTorrent should not limit a healthy download through an unnecessary
write/hash/checkpoint barrier. Under sustained useful peer input, the engine
should remain work-conserving until the limiting resource is the configured
network policy, storage device, hash CPU or memory bandwidth, rather than a
single mutable file cursor, one FIFO storage task, per-piece synchronization,
or per-piece SQLite transactions.

This topic owns the intended shape of the receive-to-storage hot path across:

- accepted peer blocks and bounded resident payload;
- torrent-to-file and skipped-file placement;
- positional writes and write completion;
- v1 piece hashing and hash-failure recovery;
- durable resume checkpoints;
- publication, cancellation and storage fences;
- path-backed desktop storage and descriptor-backed Android SAF storage; and
- the eventual session-wide scheduling of more than one active torrent.

It is deliberately broader than one implementation tactical. Each stage below
must still open a bounded tactical with exact initial limits, tests and
before/after evidence before behavior changes.

This topic does not promise that every swarm can saturate the machine. Peer
supply, congestion, protocol breadth and external rate limits can remain the
actual ceiling. The storage objective is narrower and falsifiable: when safe
eligible storage or hash work is backlogged, no unrelated storage stage should
leave usable configured capacity idle.

## Pre-054 Evidence And Bottleneck

Before Tactical `054`, the implementation had several individually bounded
components but one serialized execution path:

1. `ContentStoragePipeline` in `crates/rstorrent-engine/src/driver.rs` owns
   one command channel and one `run_content_storage_task` per active download,
   and the session currently runs at most one. Channel capacity derives from
   the received-payload budget (2,048 16 KiB slots on desktop, 1,024 on
   Android) plus a two-command supervisor-side pending queue. The command enum
   has exactly two variants: `Write` and `Verify`.
2. `collect_content_write_batch` drains at most
   `CONTENT_STORAGE_WRITE_BATCH_BLOCKS` (16) already-admitted blocks or
   `CONTENT_STORAGE_WRITE_BATCH_BYTES` (256 KiB), `coalesce_content_writes`
   merges exact adjacent same-piece ranges by copying into one `Vec<u8>`, and
   `execute_content_storage_writes` awaits every resulting write serially.
3. `StagingFile`, `SelectiveStorage` and `PartFile` now retain stable
   positional handles. Each write transfers its existing `Vec<u8>` into one
   immutable shared plan without another payload copy, resolves checked
   physical spans, validates wanted-file or part-slot generations, and runs in
   one awaited blocking job. The one storage owner still awaits that entire
   job before starting another operation.
4. A `Verify` command is created only after the supervisor consumes the final
   write completion for a piece. It joins the tail of the same FIFO that holds
   later writes and, once reached, acts as a batch barrier: writes queued
   behind it wait for the hash and its synchronization to finish.
5. Every wanted/skipped/padding hash is now one fixed-buffer positional job
   over retained handles with no per-piece descriptor duplication or cursor
   fallback. The storage task nevertheless executes only one hash and no
   writes at the same time.
6. Hash success marks the piece usable immediately and queues it to Tactical
   `052`'s bounded checkpoint owner. That owner batches dirty pieces,
   synchronizes each touched stable handle once per epoch, and persists one
   merged SQLite have transition without holding the supervisor or hash stage.
7. The remaining causal barrier is therefore execution rather than cursor or
   durability ownership: one FIFO command stream and one executing operation
   prevent unrelated writes from overlapping a hash and make an encountered
   `Verify` delay later writes.

`sync_data` is a durability operation. It asks the operating system to flush
dirty file data, and the metadata required to retrieve it, toward stable
storage. It is not required to make a completed write visible to a hash read
in the same process. A successful buffered write is already visible through
the operating-system page cache, although it may not survive a crash or power
loss.

Tactical [`031`](../tactical/031-storage-command-duration-evidence.md)'s
retained public screens attributed 93.2--93.7% of wall time to this serialized
storage service: 16 KiB writes consumed 87.7--88.2% and hash service
5.5--5.7%. Tactical
[`032`](../tactical/032-bounded-coalesced-write-batches.md) then coalesced
5,648--5,740 logical blocks into 500--509 physical writes, about 11.3 blocks
per call at the exact 16-block/256 KiB caps, while its controlled 32 MiB
profile stayed neutral at a 1.143-second median against 1.196 seconds.
Combined service still consumed 93.0--94.2% of wall time, but its composition
moved: write service fell to 51.4--54.9% while hash service rose to
39.1--41.6%. In those samples, "hash service" included the per-piece payload
synchronization described above, so the rising share indicts the serialized
durability boundary rather than SHA-1 arithmetic alone.

The product observation that prompted this topic showed a long visible
hashing backlog on hardware capable of much faster SHA-1. The current
`DiskPieceStage::Hashing` now covers SHA-1 service only, while queued verifies
age as `Stored` and durability has distinct dirty/syncing/committing stages.
The UI therefore exposes the remaining serialized execution queue more
truthfully, but the backlog still does not imply that SHA-1 arithmetic itself
is the limiting resource.

## Throughput Invariants

The end-state architecture must preserve these invariants.

### Non-overlapping payload generations

- Within one piece generation, the torrent request owner accepts at most one
  winning payload for each block range. Endgame losers, unsolicited payload
  and stale connection generations do not enter storage.
- Two accepted writes in the same generation may be adjacent but must never
  overlap in torrent coordinates or in their resolved physical destination.
- A piece generation is not reused after hash or write failure until every
  queued or running write from the old generation is canceled or joined.
- Every storage plan carries the torrent, piece, piece generation, logical
  block and storage-routing generation needed to reject a stale completion.
- Overlap is an invariant violation to detect before dispatch, not a normal
  condition that justifies serializing all writes.

Ordinary torrent data therefore has the property identified by the maintainer:
disjoint ranges are the common path. Concurrency should be built around that
property.

### Integrity joins independent work

- Peer payload remains unverified until the complete logical piece hash
  matches trusted metainfo.
- A piece becomes usable in memory only after its hash passed and every
  accepted physical write in that generation succeeded.
- Hash completion and final write completion may arrive in either order. The
  piece state explicitly joins both facts rather than requiring one execution
  order.
- A hash pass followed by a write failure does not establish the piece.
- A write-complete piece followed by a hash mismatch is fenced, attributed and
  reset as one v1 generation.
- Padding contributes synthetic zeroes to the hash and never creates a payload
  write.

The join is an explicit outcome table, not an emergent channel order:

| Generation writes | Hash outcome | Piece result |
| --- | --- | --- |
| All completed | Passed | `hash_verified`; joins the next checkpoint epoch |
| All completed | Failed | Fence, attribute and reset one v1 generation |
| Any failed | Passed or failed | Not established; fence and reset after every write joins |
| Outstanding | Passed | Hold the hash pass until the final write joins |
| Outstanding | Failed | Cancel or join remaining writes, then fence and reset |

### Durability is not hash verification

- `hash_verified` means that the trusted piece hash matched and all of that
  generation's writes completed successfully.
- `checkpoint_dirty` means the verified piece has not yet entered a durable
  resume checkpoint.
- `durably_checkpointed` means the checkpoint owner synchronized the captured
  payload generation as required and committed the corresponding have state.
- The picker, transfer scheduler and ordinary in-process progress do not wait
  for `durably_checkpointed`.
- A crash may lose recent checkpoint-dirty progress and cause conservative
  recheck or redownload. It must not make unchecked content authoritative.

### Bounded work and exact ownership

- Request reservations, accepted resident payload, queued writes, running
  writes, hash work, pending completions and dirty checkpoint state each have a
  distinct byte or item owner and limit.
- One immutable payload buffer may be referenced by a pending-write lookup and
  a write job, but its bytes are charged once until both consumers release it.
- Every background owner has cancellation, completion observation and an exact
  join path. Running blocking filesystem calls may finish during cancellation;
  they may not detach.
- One individually valid piece larger than an ordinary configured working-set
  limit retains the existing liveness exception without making aggregate work
  unbounded.

### Work conservation and fairness

- Backlogged writes keep available write capacity busy unless an affected
  storage fence or byte limit prevents dispatch.
- A hash-eligible piece does not wait behind unrelated later writes in one
  FIFO.
- A checkpoint does not stop peer intake, writes or hashes merely because it
  is committing older progress.
- One slow storage root, provider or torrent does not consume every session
  slot or block work for another independent root.
- No torrent may starve indefinitely under a continuously busy peer; fairness
  is defined at admission and completion rather than obtained accidentally
  from channel ordering.

## Proposed Data Flow

```text
peer/request owner
        |
        | accepted winning block, one charged immutable buffer
        v
storage planner / piece-generation owner
        |                         |
        | immutable physical      | pending-write lookup
        | span plan               | for hash reads
        v                         v
session/root write scheduler   hash-ready queue
        |                         |
bounded positional workers     bounded hash workers
        |                         |
        +---- write result -------+---- hash result
                         |
                  piece-generation join
                         |
              hash-verified in-memory have
                         |
                 dirty checkpoint epoch
                         |
       sync each touched destination once per epoch
                         |
             one merged SQLite transaction
                         |
                 durably checkpointed
```

The control-plane owner remains explicit. Worker threads receive immutable
plans and buffers and return typed completions; they do not mutate picker,
selection, have, publication or application state.

## Positional Storage Plans

The mutable `seek` plus `write_all` storage contract should be replaced by a
two-step contract.

First, a deterministic planner maps one validated torrent range to immutable
physical spans:

```text
StorageSpan {
    destination: WantedFile { file_index } | PartSlot { piece_index },
    destination_offset: u64,
    payload: shared immutable buffer reference and byte range,
    identity: torrent, piece generation and logical block,
    routing: storage-routing generation of the destination,
}
```

The planner owns checked arithmetic, cross-file splitting, selected/skipped
classification and padding rejection. It does no I/O and is testable without
Tokio or files.

Two consequences are visible in current code. The exclusively owned `Vec<u8>`
command payload must become a cheaply shareable immutable buffer so a
pending-write entry and a write job charge the same bytes once. And the
storage-routing generation gains a concrete owner: a per-file counter that
relocation, publication, descriptor replacement and selection migration
increment so the join can discard completions from a replaced route.

Second, workers execute each span with positional I/O:

- Unix and Android path/descriptor storage use `FileExt::write_at`/`pwrite`
  style loops.
- Windows uses the corresponding positional `seek_write` operation.
- Reads use the existing `read_at`/`seek_read` direction.
- Short operations, interruption and offset overflow are handled explicitly.
- Retained shared file handles have no shared logical cursor. Cloning a Unix
  descriptor and continuing to use `seek` is not sufficient because duplicated
  descriptors may share an underlying file offset.

This contract is also how the pinned oracle behaves on ordinary hardware: with
default settings, `mmap_storage.cpp::write` reaches `aux::pwrite_all` rather
than a mapped copy, and `part_file.cpp::write` resolves a slot under its mutex,
unlocks and then writes positionally.

The scheduler may combine physically adjacent ready spans for the same
destination under a byte/count cap, regardless of whether their logical blocks
belong to one piece, as long as every member retains an independent completion.
It must not wait for a batch to fill. Gaps, destination changes, routing
generations and fences stop coalescing.

The initial implementation need not adopt memory mapping, direct I/O,
`io_uring`, unsafe code or a new dependency. The plan/completion boundary
should allow a later backend to use those mechanisms only if measurements
justify them.

## Piece Hash Scheduling

Hash work needs a separate readiness queue and capacity owner. There are two
valid implementation stages.

### Initial safe stage: write-complete fence

A piece becomes hash-eligible as soon as every logical write completion for
its generation succeeds. It enters a hash queue independent of later writes.
Several pieces may hash concurrently, and writes for unrelated pieces continue
while they hash.

This is simpler than the final store-buffer path and already removes the
current global FIFO barrier. It rereads recently written data through the OS
page cache and should be measured before adding more buffer lifetime.

### Maximum-overlap stage: read through pending writes

For the end-state path, a piece may become hash-eligible when every block
payload has been accepted, even if some physical writes remain in flight. This
is the pinned libtorrent trigger: `incoming_piece` starts `verify_piece` once
the picker reports every block writing or finished. A bounded pending-write
lookup keyed by torrent, piece, generation and block offset exposes immutable
accepted buffers to the hasher. For each hash range:

- use the accepted buffer while its write is queued or running;
- otherwise use a positional file or part-file read after the write completed;
- synthesize padding zeroes; and
- preserve torrent byte order across physical destinations.

Once a write system call completes, a later file read sees that data through
the page cache, so retiring its pending-buffer entry is safe. Hash pass and
write completion remain separate facts in the piece join. This follows
libtorrent's store buffer, which `mmap_disk_io.cpp::do_hash` reads through
before falling back to a positional storage read, without requiring its
allocator, cache or class graph.

Hash concurrency is bounded independently from write concurrency. A v1 SHA-1
piece is sequential internally, but several independent pieces can use several
cores. Hash jobs that miss the pending buffer also consume storage-read
capacity so they cannot create unbounded read amplification or starve writes.

## Part-File Design

The part file is not a reason to serialize ordinary wanted-file writes. Its
metadata allocation and its payload I/O have different ownership needs.

For one stable selection generation:

- the planner determines which wanted pieces require skipped bytes;
- one coordinator assigns at most one stable part-file slot to each such piece
  before dispatching its first part-file payload span;
- workers use the assigned slot's absolute payload offset with positional I/O;
- writes to different piece slots are disjoint and may run concurrently; and
- multiple ranges within one slot follow the same per-piece non-overlap rule as
  wanted files.

The slot map is small mutable metadata. It remains owned by the coordinator,
not by arbitrary write workers. Slot-entry changes become dirty metadata and
are flushed in a checkpoint epoch; allocating or releasing one slot must not
call `sync_data` on the whole part file in the hot path.

A slot is never reused while a write, hash, checkpoint snapshot or
materialization job still references its old mapping generation. Reuse follows
an affected-piece fence. A crash may leave an uncheckpointed payload slot
orphaned; restart ignores or reclaims it rather than treating it as verified.

The current format now writes each newly allocated slot entry positionally
without a standalone `sync_data`; the joined checkpoint synchronizes the
mapping and payload before durable have state. `release_piece` forces its
missing entry before making the physical slot reusable, and per-piece mapping
generations reject stale spans. A later tactical may introduce a new versioned
mapping representation, but a format change requires an explicit resume
compatibility and migration decision.

## File Selection And Placement Changes

File priority and physical placement should be separate concepts. Changing
what the scheduler wants need not move bytes that already have a safe
destination.

The common fast path is a selection fixed while payload is active. Dynamic
changes are infrequent control operations and may use targeted fences:

- **Wanted to skipped:** stop selecting pieces needed only by that file. If the
  destination file already exists, retain it and keep any already established
  physical route rather than moving its bytes into the part file on the hot
  path. Removing or compacting the file is a separate explicit operation.
- **Skipped to wanted:** fence the affected file/ranges, create or acquire its
  destination, materialize verified bytes available from part slots, install a
  new storage-routing generation, then resume affected dispatch. Unrelated
  files and torrents continue.
- **Materialization after completion:** retain the current coarse operation,
  but execute part-file reads and destination writes positionally and force the
  required publication durability before releasing slots.
- **Relocation, deletion and publication:** use explicit storage fences because
  these operations change handle or path identity. They are not ordinary
  per-block barriers.

Tactical [`062`](../tactical/062-user-visible-publication-layout.md) now gives
path identity an engine-owned plan: the final multi-file tree uses the
verified recognizable torrent name, while staging and part artifacts use the
full info hash. For path storage the session separately persists whether it
owns no artifacts, only internal staging, a published tree, or the legacy hash
layout. That ownership boundary lets joined removal clean exact artifacts
without deleting an unrelated named destination that previously caused a
collision.

Libtorrent also treats file-priority changes as asynchronous fenced disk jobs
and documents that changing an already-created file to skipped does not move
it into the part file. RSTorrent may choose different product semantics later,
but it should not pay a migration barrier on every normal block in anticipation
of that uncommon operation.

Tactical `063` implements the first correct control boundary more coarsely than
the targeted end-state above. Durable selection commits first; the application
then cancels and joins the entire matching engine generation before reopening
with immutable plans. Existing destinations retain their physical route when
lowered. Missing skipped destinations route through a part file created only
by the first actual part write. Promotion creates the destination, exports
available verified spans, rechecks missing sources conservatively, and unlinks
the path part file after its final slot is released. This deliberately spends
peer reconnection on an uncommon user action while keeping hot writes free of
a mutable priority fence.

## Durability And Batched Resume Checkpoints

Per-piece `sync_data` plus one SQLite transaction has been removed from the
critical path. One application-service checkpoint owner collects
verified-dirty pieces into bounded epochs, and SQLite no longer runs inline on
the transfer supervisor.

A checkpoint epoch performs these steps:

1. Capture a fixed set of hash-verified pieces, their write generations,
   touched wanted-file identities, part-file mapping generation and total dirty
   bytes.
2. Confirm that every captured write completed before the durability barrier
   is issued. Later writes may continue through positional I/O.
3. Persist dirty part-file slot metadata for the captured generation.
4. Call one storage durability operation per touched destination, not once per
   piece. Independent files may synchronize concurrently when the backend
   supports it. The part-file metadata and payload use one ordered barrier.
5. Commit all captured piece bits and one revision in one SQLite transaction.
6. Publish one typed durable-checkpoint completion and leave pieces that became
   dirty after the capture for the next epoch.

For ordinary path-backed files, the backend contract must establish that a
durability operation covers writes that completed before it was issued even if
later positional writes proceed concurrently. If a platform provider cannot
make that guarantee, only that destination receives a short checkpoint fence;
the entire torrent and session do not automatically stop.

Candidate checkpoint triggers should be time and dirty bytes, with a hard
maximum age, rather than piece count alone because valid piece sizes vary from
small blocks to hundreds of MiB. Exact values require measurement. A first
tactical may choose within a declared range such as 1--5 seconds and
16--64 MiB, then tighten it from crash-cost and device evidence. Checkpoints
are also forced at graceful pause/shutdown, final publication and any operation
that will discard or replace the referenced storage generation.

SQLite may retain one serialized writer and `synchronous=FULL`. The avoidable
cost is not serialization inside SQLite; it is decoding and rewriting the have
bitmap and committing a full transaction once per piece on the download's
completion path. `SessionStore` currently exposes only per-piece
`record_piece` and whole-bitmap `replace_have`; the epoch commit needs a
`record_pieces` operation between those extremes that decodes once, merges
all epoch bits, encodes once and commits once.

### Crash outcomes

| Crash point | Restart consequence |
| --- | --- |
| Before a block write completes | The piece is not checkpointed and remains missing or is rechecked. |
| After writes and hash pass, before payload sync | The piece may exist on disk but its have bit is absent; recheck or redownload is safe. |
| After payload sync, before SQLite commit | Durable bytes may be ahead of durable metadata; this is a safe false negative. |
| During the SQLite transaction | SQLite exposes either the old or committed epoch; no partial have update is trusted. |
| After the epoch commit | Existing conservative restart rehashes the claim before presenting it as verified. |

The current restart path rehashes every claimed piece, which remains the final
integrity authority. A later clean-shutdown fast-resume design may skip some
hashing only after it defines stronger file identity, directory durability and
storage-generation evidence.

## Session And Storage-Root Scheduling

The current per-torrent owner is sufficient for one active download but should
not become the final resource authority. The end-state scheduler is
session-owned and groups work by storage root or another concrete backend
identity.

It owns:

- aggregate resident and queued write bytes;
- per-root write and read concurrency;
- independent bounded hash concurrency;
- per-torrent admission and fair progress;
- completion batching;
- root-scoped pressure and failure; and
- control fences for pause, publication, priority changes and removal.

Scheduling should be work-conserving and fair, for example through bounded
per-torrent ready queues and deficit/round-robin admission rather than one
global FIFO. A small torrent must make progress beside a large saturated one,
while an idle torrent's unused share is immediately available to others.

Storage roots need separate concurrency profiles. Excess concurrency can hurt
a rotational disk or a serialized document provider while too little leaves
an SSD or NVMe device idle. The architecture therefore does not canonize
libtorrent's default ten generic I/O threads, JSTorrent's six workers or one
hard-coded RSTorrent value. A tactical should sweep a bounded candidate range
such as 1, 2, 4, 8 and 16 operations on each supported backend class and select
the throughput plateau without hiding latency, CPU or memory regressions.

Android SAF remains a first-class bulk-I/O capability, not a callback payload
path. Providers already proven to expose seekable duplicated descriptors are
candidates for the same positional worker path. Concurrent positional I/O and
durability ordering must be tested on the AVD and authorized physical devices.
A provider capability may conservatively select one worker for its root without
forcing desktop or another root to serialize.

## File-Handle Ownership

Workers need stable destination identities without opening a file per block.
Path and Android SAF storage now share a session-wide bounded retained
handle table whose entries can be shared immutably by positional jobs. Jobs
keep their handle alive through completion; eviction removes the cache
reference but never invalidates running work.

Acquisition differs without forking payload I/O. Path storage opens a safe
native path locally. On a SAF cache miss, the Android platform owner resolves
or creates one exact provider document and lends a descriptor for Rust to
duplicate. Hits, positional I/O, hashing, durability, LRU eviction, and
in-flight ownership remain in Rust. Descriptor count is bounded by the shared
pool rather than validated metainfo size or a startup manifest. The detailed
capability, cancellation, part-file, and provider lifecycle is owned by
[`android-saf-storage.md`](android-saf-storage.md).

The hash path should stop duplicating each wanted-file handle for every piece
once handles are safely shareable for positional access;
`prepare_blocking_hash_plan` currently calls `try_clone` on every wanted file
a piece touches and reports it as `wanted_file_duplicates`. Handle reuse is a
resource and syscall improvement to measure after positional ownership is
correct.

## Backpressure And Memory Accounting

The existing separation between outstanding request reservations, received
resident payload and active-piece bytes remains. Storage concurrency adds
separate observable bounds for:

- queued and running write bytes per root and session;
- pending-write lookup bytes, charged as references to resident payload rather
  than as a second payload copy;
- queued and running hash pieces/bytes;
- hash read buffers and temporary coalescing buffers;
- queued completion items/bytes;
- dirty checkpoint bytes, piece count and oldest age;
- dirty part-file metadata entries;
- open or retained handles; and
- fenced/blocked work.

Backpressure uses high/low hysteresis. It stops new payload reads before
accepted buffers exceed their owner, but does not stop discovery, peer control,
write completions, hash completions or checkpoint completions. Existing
promised requests may overshoot only within their separately declared bound.

Batch coalescing must not double the whole batch by default. Where a contiguous
copy is measurably useful, its temporary bytes receive their own cap. Vectored
or multiple positional writes may be preferable when copying costs more than
the syscall it removes.

## Failure, Cancellation And Fences

Normal writes are concurrent; exceptional state changes are coordinated.

- A validation failure rejects a plan before any of its spans mutate storage.
- A short or failed physical write fails its logical member and prevents its
  piece-generation join. Other already-running writes return typed outcomes.
- A storage-wide failure stops new admission for the affected root, drains or
  joins owned work and retains conservative have state.
- A hash mismatch waits for or cancels all writes in that piece generation,
  raises an affected-piece fence, clears pending-buffer entries, then permits a
  new generation.
- Pause stops new torrent admission, cancels queued work, waits for running
  syscalls and joins workers. Policy decides whether to force a dirty
  checkpoint or leave safe false-negative progress.
- Graceful shutdown forces the bounded checkpoint, joins all storage and
  application owners and then closes handles and SQLite.
- Publication, relocation, descriptor replacement, selection migration and
  deletion use explicit affected-storage fences. They do not share the normal
  block queue's accidental FIFO semantics.

A first implementation may use a torrent-wide fence for rare control
operations while the data model already names affected destinations and
generations. Per-piece and per-file fences can then replace the broad fence
when evidence justifies the additional bookkeeping.

## Observability Contract

The Disk inspection vocabulary should expose real stages rather than label a
durability backlog as hashing.

At minimum distinguish:

- write queued and write active;
- hash eligible, hash queued and hash active;
- hash passed but writes pending;
- hash verified and checkpoint dirty;
- checkpoint syncing and SQLite committing; and
- durably checkpointed.

Concretely, `DiskPieceStage` (today `Receiving`, `Queued`, `Writing`,
`Stored`, `Hashing`, `Failed`) and its session view mappings gain the missing
stages, and the per-kind `storage_write_*`/`storage_hash_*` timing fields
Tactical [`031`](../tactical/031-storage-command-duration-evidence.md) added
to `DownloadProgress` and `DiskPipelineView` gain sync, checkpoint and
utilization counterparts instead of a parallel metrics system.

Global/root metrics should include configured and active write/hash
concurrency, worker utilization, queue depth and bytes, oldest ready age,
physical and logical write counts, batch/coalescing shape, pending-buffer hit
rate, hash bytes from buffers versus files (the analogue of libtorrent's
`num_read_back` counter), sync calls and bytes/pieces amortized per sync,
SQLite pieces per transaction, dirty checkpoint age and bytes, and per-kind
service/queue time.

Counts and cumulative service time must remain separate from wall time when
operations overlap. The useful utilization question becomes whether every
eligible worker was busy, not whether summed concurrent service exceeds wall
clock.

The piece map should clear `Hashing` when SHA work ends. A piece waiting for
write completion or durability uses its own state. This makes future backlog
reports actionable without turning logs into application state.

## Reference Findings

### RSTorrent and libtorrent comparison

| Concern | Current RSTorrent | Pinned libtorrent | Proposed RSTorrent |
| --- | --- | --- | --- |
| Normal writes | One torrent FIFO, mutable cursors, one executing operation | Session disk-job pool with mapped or positional I/O | Bounded root workers execute immutable disjoint spans positionally |
| Hash readiness | Enqueued behind later writes after final write completion is observed | Starts when every block is writing or finished | Separate queue; begin behind a per-piece write fence first, then optionally at all-payload-accepted |
| Hash input | Positional reread after writes | Store-buffer read-through, then storage | Pending accepted buffer when present, positional storage read otherwise |
| Write/hash result | One enforced execution order | Hash and final write callbacks can arrive in either order | Explicit piece-generation join accepts either completion order |
| Part-file payload | Mutable cursor behind the torrent owner | Slot map is locked; payload I/O is positional outside it | One slot coordinator plus concurrent positional slot payload workers |
| Part-file metadata | Slot change is immediately synchronized | Dirty slot metadata flush is separate from payload writes | Dirty mapping generations join batched durability epochs |
| Selection changes | Live path and dynamic-SAF Normal/Skip use a joined torrent-generation fence | Asynchronous fenced jobs export part data; wanted-to-skipped does not migrate existing files | Targeted file/range routing-generation fence if measured reconnection cost justifies it |
| Durability/resume | Resumable selective storage performs per-piece payload sync followed by a per-piece `FULL` SQLite transaction; single-file staging syncs only at finalization | Resume snapshot persistence remains caller-owned rather than a per-piece SQL barrier | Bounded dirty epoch, one sync per destination, one merged SQLite transaction |
| Aggregate scheduling | Torrent-local resource authority | Session disk pool, 1 MiB default queued-byte watermark and ten generic threads | Session/root capacity and fairness with measured backend-specific limits |

The normative storage shape comes from pinned BEP sources:

- `reference/bittorrent.org/beps/bep_0003.rst` defines v1 pieces as SHA-1 over
  fixed torrent-coordinate ranges and treats multi-file payload as the files
  concatenated in metainfo order.
- `reference/bittorrent.org/beps/bep_0047.rst` defines padding content as
  synthetic zeroes for piece hashing and says aware clients need not request or
  write padding ranges.

Pinned libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` supplies the completeness oracle:

- `src/peer_connection.cpp::incoming_piece` submits an accepted block through
  `async_write`, marks it writing, keeps request filling independent from disk
  completion and starts `verify_piece` once every block is writing or finished.
- `src/peer_connection.cpp::on_disk_write_complete` independently changes a
  block from writing to finished and handles write failure.
- `src/torrent.cpp::verify_piece` and `on_piece_verified` schedule and join the
  hash result without assuming it arrives after every write callback.
- `src/mmap_disk_io.cpp::async_write` publishes each accepted block in a
  bounded store buffer before queueing its generic write job, `do_write`
  retires that entry only after the storage write completes, and `do_hash`
  consumes store-buffer entries directly, counting each fallback positional
  read as `num_read_back`.
- `src/mmap_storage.cpp::write` calls `aux::pwrite_all` whenever mapped writes
  are disabled or the file has no memory map. `set_file_priority` moves
  part-file data out under a fenced control operation and does not implement
  moving an already-created file into the part file.
- `src/mmap_disk_io.cpp::async_set_file_priority`, `async_clear_piece` and the
  disk job fence owner serialize exceptional mutations without serializing all
  normal I/O.
- `src/settings_pack.cpp` defaults `max_queued_disk_bytes` to 1 MiB,
  `aio_threads` to ten generic I/O threads and `hashing_threads` to one;
  `include/libtorrent/settings_pack.hpp` documents that extra hash thread as
  full-check only, with download-time hashes on the regular I/O pool. The
  default `auto_mmap_write` mode maps writes only on DAX-class storage, so
  ordinary devices already take the positional `pwrite_all` path.
- `src/part_file.cpp::write` holds its mutex only to resolve or
  `allocate_slot`, marks the slot map dirty, then unlocks and performs the
  payload `pwrite_all` outside the lock; `flush_metadata` rewrites the header
  separately rather than synchronizing every payload range.
- `test/test_piece_picker.cpp::{piece_passed,
  set_piece_priority_passed_hash_check,
  set_piece_priority_passed_hash_check_unfilter,
  set_piece_priority_passed_hash_check_bulk_filter}` covers a hash result
  arriving before final write completion and priority transitions in that
  state.
- `test/test_storage.cpp::{mmap_unaligned_read_both_store_buffer,
  posix_unaligned_read_both_store_buffer}` drive the four
  `*_from_store_buffer` helper cases covering reads spanning queued and
  completed writes.
- `test/test_fence.cpp::{empty_fence,job_fence,double_fence}` covers jobs
  before, during and after storage fences.
- `test/test_priority.cpp::{export_file_while_seed,
  file_priority_stress_test}` covers asynchronous part-file export and repeated
  priority changes.
- `test/test_part_file.cpp::{part_file,posix_part_file}` covers slot mapping,
  positional payload access, reopen, export, free and metadata flush.

RSTorrent adopts the behavioral lessons: positional non-overlapping work,
independent write/hash completion, bounded pending-buffer reads, exceptional
fences and separated durability. It does not adopt libtorrent's C++ object
model, memory mapping, buffer allocator, exact worker count or resume format.

The local JSTorrent `main` sibling at
`9895410beeed6aff554053769bd006a3fbd373ef` provides additional
product/platform history:

- `packages/engine/src/core/disk-queue.ts` uses a bounded multi-worker queue
  with a six-worker default;
- `packages/engine/src/adapters/native/native-async-write.ts` batches one tick
  of Android/iOS unverified writes for parallel native dispatch;
- `packages/engine/src/adapters/native/native-batching-disk-queue.ts` caps one
  native batch at 128 writes and 4 MiB and separately tracks pending and
  in-flight writes and bytes; and
- `packages/engine/src/core/torrent-content-storage.ts::tryBatchWrite` batches
  same-file work only under backlog and separately accounts in-flight bytes.

Those paths confirm that platform dispatch overhead, byte bounds and completion
ownership matter. Their JavaScript/FFI topology and worker defaults are not an
RSTorrent architecture template.

## Validation And Graduation Evidence

Each implementation tactical takes the cheapest applicable subset, while the
complete architecture requires all layers below.

### Deterministic state and planning

- prove torrent and physical spans never overlap within a generation;
- reject stale write, hash and checkpoint completions;
- join hash/write outcomes in both orders, including hash pass plus write
  failure;
- prevent a new piece generation until old writes drain;
- cover cross-file, skipped, padding, final-short and maximum-size pieces;
- cover part-slot allocation, deferred metadata, fenced release and routing
  generation changes; and
- prove byte accounting under shared pending-buffer references.

### Scripted runtime

- complete many out-of-order positional writes concurrently to one file and
  verify exact bytes;
- delay one write without blocking unrelated writes or hash-ready pieces;
- complete hash before the final write callback and final write before hash;
- inject partial writes, disk full, permission loss, truncation, panic/join
  failure and cancellation;
- crash after every durability step in the table above and restart
  conservatively;
- change skipped/wanted state while affected work exists and prove the routing
  fence;
- run two torrents on one root and two roots with one deliberately slow root;
  and
- prove pause, shutdown and publication leave no detached work.

### Hardware-ceiling profiles

Before choosing worker limits, record on the same destination:

- raw bounded positional write throughput for representative batch and block
  shapes;
- raw file-backed and in-memory SHA-1 throughput at several hash worker counts;
- combined write-plus-hash throughput with warm and cold cache observations;
- CPU time, peak RSS, page-cache assumptions, physical write volume, queue
  depth, worker utilization and tail service latency; and
- a concurrency sweep rather than only one before/after value.

The integrated controlled torrent should approach the slowest relevant raw
stage without a persistently idle eligible worker. A tactical should declare a
specific ratio only after the raw profile is stable; a useful initial
graduation target is within 15--20% of the equivalent bounded raw pipeline on
the same machine, with exact integrity and no resource regression. Missing
that target keeps the owner open rather than lowering the measurement.

### Interoperability and product evidence

- retain exact selective and mixed-source libtorrent fixtures;
- add a large same-file controlled transfer that saturates local storage and
  hash resources; the retained 32 MiB profile completes in about 1.1 seconds
  and cannot separate steady-state throughput from startup;
- rerun the paired Big Buck Bunny comparator and the remaining catalog after
  controlled scaling is causal;
- measure desktop path storage first, then Android descriptor storage through
  the established AVD and explicitly authorized physical-device process; and
- update Disk/Pieces stages so live inspection distinguishes hashing from
  checkpoint backlog.

Public swarm speed remains a distribution. Correct hashes, publication,
bounded memory, exact cleanup and classified failures are hard gates.

## Multi-Tactical Sequence

### 1. Decouple verification from durability

Split hash-verified state from durable checkpoint state. Remove per-piece
payload synchronization and per-piece SQLite commits from the transfer
critical path. Add bounded dirty epochs, one sync per touched destination and
one merged have transaction. Update Disk observability to show the distinction.

This is the recommended first tactical because the current purple hashing
stage includes these barriers and because it changes the state model required
by every later concurrency slice.

### 2. Establish immutable positional write plans

Extract deterministic span planning, convert wanted-file and part-file payload
access to positional I/O, separate part-slot metadata ownership, reuse safe
handles, and keep one executing write initially. This proves the concurrency
foundation without attributing failures to worker scheduling.

### 3. Add bounded concurrent write and hash execution

Introduce root-aware write capacity, an independent hash-ready queue,
per-piece generation joins, batched completions and fairness. Sweep worker
counts against raw hardware profiles. Hashing may initially wait for that
piece's writes while unrelated work overlaps.

### 4. Add pending-write read-through if still material

Retain accepted buffers through write/hash consumption, allow hash start before
final write completion and measure page-cache rereads, hash latency and memory
cost. Keep this only if it produces a material controlled improvement beyond
the simpler per-piece write fence.

### 5. Graduate to session-wide multi-root scheduling

Move aggregate resource authority above one torrent, add fair multi-torrent
admission, root-specific concurrency profiles and slow-root isolation. This
slice must arrive before claiming scalable concurrent torrents.

Each tactical stops at its own falsifiable boundary. The sequence may combine
small adjacent refactors when evidence makes separation artificial, but it
must not land an unmeasured general disk framework in one change.

## Non-Goals Of This Topic

- selecting exact production worker counts or checkpoint intervals without
  controlled profiles;
- implementing the architecture in this documentation change;
- making unchecked payload authoritative or weakening conservative restart;
- assembling every valid piece into one contiguous piece-sized allocation;
- requiring memory mapping, direct I/O, `io_uring`, unsafe code or a new
  storage dependency;
- changing peer ranking, request-window growth, piece rarity, discovery,
  endgame or protocol breadth to conceal a storage bottleneck;
- defining v2/hybrid Merkle verification, upload/seeding reads or streaming
  priorities; or
- promising identical concurrency on desktop paths and every Android document
  provider.

## Active Execution

Tactical [`052`](../tactical/052-batched-durability-checkpoints.md) completed
the first slice: hash verification now precedes bounded batched payload and
SQLite durability in separate joined stages. Its final SQLite-backed cohort
reduced the median from 50.085 to 46.380 seconds and post-metadata revisions
from 514 to 18 without a persistent raw storage-control regression. Tactical
[`053`](../tactical/053-immutable-positional-storage-plans.md) then established
retained positional handles, immutable no-extra-copy plans and generation
checked part slots. Its engine median fell from 35.792 to 33.679 seconds and
write service fell from 30.928--31.979 to 27.131--28.353 seconds; its
SQLite-backed median was 45.594 seconds with unchanged checkpoint shape and
exact restart/crash evidence.

Tactical [`054`](../tactical/054-bounded-independent-storage-execution.md)
now runs independently bounded write and hash jobs with an explicit
piece-generation join. Its large local baseline additionally replaced
per-event whole-swarm snapshots, per-piece whole-block contributor scans and
active-piece scans that included fully requested work with checked incremental
indexes. A later process profile exposed repeated per-block scans within each
piece; checked missing/active counters and a first-missing cursor now remove
those geometry costs while the generation join remains the hash-integrity
authority. The repeated 10 GiB matrix finishes every RSTorrent observation in
9.408--31.652 seconds with exact hashes and cleanup.

The large comparator now owns the first optimization gate rather than serving
only as an observational screen. It can rotate several `WRITE/HASH` points
against one libtorrent observation on a shared fixture, reports cohort medians
and ratios, and can fail an explicit throughput floor. The current machine's
retained floor is 170.667 MiB/s, equivalent to 10 GiB in 60 seconds, across
every representative piece size. The final three-run 10 GiB medians are
336.2, 804.4, 1,031.6 and 720.3 MiB/s at 256 KiB, 1 MiB, 4 MiB and 16 MiB
pieces. Pinned libtorrent reaches 471.5, 559.2, 1,074.4 and 1,031.8 MiB/s in
the matching cohorts. A current-code finalist gives `4/4` a 1,074.1 MiB/s
median versus 1,028.6 MiB/s for `8/4`, so the selected desktop bound remains
`4/4` rather than spending twice the write-job capacity without a measured
gain.

The raw-ceiling side is now executable too. The engine diagnostic uses the
same positional helper, 16 KiB hash reads and SHA-1 implementation as the
download path; its driver rotates worker points and proves operation counts,
full allocation, piece hashes, ready-backlog bounds and cleanup. Its default
10 GiB/4 MiB workload permutes all 256 KiB write spans with a deterministic
bijection, hashes a piece only after its writes finish, bounds the ready
channel at `write + hash`, and reports post-interval sync separately.

The initial full raw sweep reached 2,020.9 MiB/s combined at `4/4`, 1,987.3 at
`8/4` and 2,237.1 at `8/8`. Warm file SHA-1 alone reached 4,515.1 MiB/s at
four workers and 8,051.7 MiB/s at eight; raw positional writes remained above
3,369 MiB/s at every point. All exactness and cleanup gates passed. Against
that adversarial bounded workload, the integrated 10 GiB/4 MiB `4/4` median
initially used only 21.4% of raw combined capacity. Removing the checked-index
hot path raised the final integrated median to 1,031.6 MiB/s, or 51.0% of the
raw combined observation, without increasing concurrency. The remaining
small- and large-piece gaps are now bounded optimization questions rather than
a throughput gate failure.

The retained SQLite-backed application cohort then isolated a separate
observation barrier: synchronous full Disk projection on nearly every block
event produced a 7.006-second median despite the 0.555-second engine control.
Coalescing ordinary `StorageState` delivery to 100 ms while forcing checkpoint,
error and terminal observations reduced the repeated application median to
0.534 seconds (239.7 MiB/s). All three runs retained exact 512-piece state,
four durable revisions after metadata, payload hash, publication and cleanup.
Closing selective, mixed-peer, resume, crash, web, Android and controlled
libtorrent gates all pass. One headless full-reference Big Buck Bunny pair
published exact content after 29.323 seconds for RSTorrent and 36.599 seconds
for libtorrent, with post-metadata payload intervals of 12.059 and 13.875
seconds. Changing public peers make that one sample contextual rather than a
parity threshold.

Tactical `054` is complete. Any pending-write read-through slice must first
show that page-cache rereads remain a material limit under the retained large
matrix; it is not the default next implementation merely because libtorrent
supports it. Session/root-aware fairness remains deferred until more than one
active download can share the owner.
