# Tactical 053: Immutable Positional Storage Plans

Status: Complete on 2026-08-02.

Topics: `storage-throughput-architecture`, `download-correctness`,
`client-persistence`, `disk-and-piece-inspection`,
`performance-and-live-evidence`, `oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `052` removed payload synchronization and SQLite commits from SHA-1
service, but ordinary payload execution still depends on mutable file cursors.
`StagingFile::write_block`, `SelectiveStorage::write_block` and
`PartFile::write_piece_range` seek and then write. Mixed wanted/skipped hashes
also seek and read through the same mutable Tokio handles. Disjoint torrent
ranges therefore cannot safely become independent jobs until their physical
destinations, offsets, payload ownership and part-slot identity are explicit.

Replace that contract with immutable physical write and hash plans executed by
cross-platform positional I/O. Retain one executing storage operation in this
tactical. The result is a correctness foundation for later bounded workers,
not a concurrency or speed claim.

The part-file slot table remains the single mutable placement coordinator. A
planned part span captures the piece, slot and per-piece mapping generation so
a released or reused slot invalidates stale work. First allocation writes its
slot entry without forcing a per-slot `sync_data`; the existing Tactical `052`
checkpoint sync makes both mapping and payload durable before its have commit.
Slot release and reuse remain fenced control operations and force their
metadata transition before the old slot may be reassigned.

## Stable Scenarios

- One block crossing wanted files, skipped files and padding either produces a
  complete validated immutable plan or mutates no payload range.
- Every physical span names one stable destination, checked absolute offset,
  shared immutable payload range and storage-routing generation.
- Disjoint out-of-order writes to one retained handle do not share or mutate a
  logical file cursor.
- A short positional read is typed as truncation; a zero or short positional
  write cannot be reported as complete; interrupted operations retry without
  changing their remaining range.
- A part-file slot is allocated once by the coordinator. Payload I/O uses its
  absolute positional offset outside metadata mutation.
- Releasing and reusing a slot invalidates every plan carrying the prior
  per-piece mapping generation before any stale payload write occurs.
- Wanted-file and part-file hash input use retained positional handles in
  torrent order, with synthetic padding zeroes and the fixed 16 KiB hash
  buffer. Mixed pieces no longer fall back to cursor reads.
- Path-backed files and Android preopened descriptors use the same safe
  positional contract on Unix; Windows uses the corresponding standard
  `seek_read`/`seek_write` operations.
- Publication, materialization, descriptor finalization, relocation and
  deletion remain coarse fenced control operations after the storage owner
  joins. They need not become concurrent in this slice.

## Normative And Reference Dossier

No reference source, fixture or part-file format is copied.

- Pinned BEP 3 at `reference/bittorrent.org/beps/bep_0003.rst` defines a v1
  piece over concatenated torrent-coordinate files. Pinned BEP 47 at
  `reference/bittorrent.org/beps/bep_0047.rst` makes padding synthetic zeroes
  that need not be requested or written.
- Pinned libtorrent `2.0.13` at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d` provides the primary completeness
  oracle. `src/mmap_storage.cpp::{read,write}` uses `aux::pread_all` and
  `aux::pwrite_all` when a file is not mapped; `src/file.cpp::{pread_all,
  pwrite_all}` handles full-range positional transfer on Unix and Windows.
- `src/part_file.cpp::{allocate_slot,write,read,flush_metadata_impl}` keeps
  slot mutation under its mutex, marks metadata dirty, releases the mutex
  before positional payload I/O and persists mapping separately. The matching
  `test/test_part_file.cpp::{part_file,posix_part_file,part_file_short_read}`
  covers write/read/hash, explicit metadata flush, reopen, export, release and
  truncation.
- `test/test_fence.cpp::{empty_fence,job_fence,double_fence}` proves that
  exceptional mutations wait for old work, block new affected work and then
  release it in order. This tactical keeps the existing torrent-wide owner as
  that coarse fence rather than adding a general fence graph.
- `test/test_storage.cpp::{mmap_unaligned_read_both_store_buffer,
  posix_unaligned_read_both_store_buffer}` and their four
  `*_from_store_buffer` helper cases remain later read-through oracles. This
  tactical adopts only their exact cross-boundary storage-read behavior; it
  does not add a pending-write buffer.
- Pinned rqbit at `4e5f94cbcf1d57ec500885c77cf1e24d70232d89`
  documents the Rust alternative in `crates/librqbit/src/storage/mod.rs`,
  implements Unix/Windows positional loops in
  `storage/filesystem/opened_file.rs`, and maps torrent ranges to file-relative
  positional operations in `file_ops.rs::{read_chunk,write_chunk}`. RSTorrent
  retains its explicit selection, part-file and resume owners instead of
  adopting rqbit's storage trait graph or vectored-write dependency.
- Local JSTorrent sibling HEAD
  `9895410beeed6aff554053769bd006a3fbd373ef` confirms the product history in
  `packages/engine/src/core/disk-queue.ts` and
  `packages/engine/src/adapters/native/native-batching-disk-queue.ts`: pending
  and running bytes need distinct ownership, and native batches have explicit
  count/byte caps. Its six-worker and 128-write/4 MiB values are not selected
  by this one-executor tactical.

Intentional differences from libtorrent are unchanged: no memory mapping,
direct I/O, `io_uring`, unsafe code, new dependency, store buffer, generic disk
job pool or part-file format import.

## Owner, Task And Cancellation Map

| Owner | Mutable state | Work and termination |
| --- | --- | --- |
| Content supervisor | Piece generation and logical completions | Unchanged; sends writes/verifies and joins storage on completion or cancellation. |
| Content storage task | One command queue and one executing operation | Plans and awaits one positional blocking job at a time; cancellation closes admission and joins the running syscall before returning storage. |
| Selective storage planner | Layout, fixed selection and per-file route generations | Resolves validated logical segments to immutable wanted-file or part-slot spans; performs no payload I/O. |
| Part-file coordinator | Slot map and per-piece mapping generations | Lazily assigns, releases and reuses slots; returns immutable absolute payload spans and rejects stale generations. |
| Positional job | Retained handles, immutable spans and shared payload | Performs full-range reads/writes only and returns a typed result; it owns no picker, have, checkpoint or publication state. |
| Checkpoint owner | Tactical `052` dirty epochs and stable sync handles | Unchanged; its part-file sync orders the written slot entry and payload before SQLite. |

No new long-lived task is added. `spawn_blocking` work is always awaited by the
storage owner. A canceled future does not imply a canceled filesystem call;
shutdown waits for that call and never detaches it.

## Module And Data Shape

The protocol crate retains deterministic torrent-coordinate layout and knows
nothing about files, handles, Tokio or tasks. A small engine-only positional
I/O module owns standard-library platform loops. `storage.rs` owns the
single-file plan; `selective_storage.rs` composes pure layout segments with
retained destination routes; `part_file.rs` alone owns slot allocation and
mapping generations.

The concrete write boundary is equivalent to:

```text
StorageWritePlan {
    piece_index,
    logical_begin,
    payload: Arc<Vec<u8>> held immutably,
    spans: [StorageWriteSpan {
        destination:
            SingleFile { routing_generation }
          | WantedFile { file_index, routing_generation }
          | PartSlot { piece_index, slot, mapping_generation },
        destination_offset: u64,
        payload_range: Range<usize>,
    }],
}
```

Planning validates nonempty length, interval bounds, padding rejection,
payload-range coverage, destination existence and checked offsets before a
payload job starts. Execution revalidates route/mapping generations before
the first span mutates storage. All spans reference one immutable payload;
they do not copy it again. The existing coalescer may still create its one
bounded contiguous `Vec<u8>` and converts ownership once at this boundary.

Retained positional handles are created once with the storage object and are
shared safely because positional calls do not consume a common cursor.
Control-only Tokio handles may remain temporarily for sizing, syncing and
post-join materialization/publication. Duplicating a descriptor and then using
`seek` is not treated as positional safety.

## Part-File Durability And Compatibility

The on-disk `RSPART01` format and reopen validation remain unchanged.
Allocation sizes the slot, writes the four-byte mapping entry positionally and
updates the in-memory map/generation before returning a payload span. It no
longer synchronizes that entry in isolation. A resumable boundary piece always
names `DurabilityTarget::PartFile`, so the checkpoint epoch synchronizes the
mapping entry and payload before committing its have bit.

Crash outcomes remain one-sided:

| Crash point | Restart consequence |
| --- | --- |
| Before mapping-entry write completes | No plan is dispatched; no have bit exists. |
| After mapping entry, before payload completion | The slot may reopen with partial bytes, but the piece is missing and must be rewritten/rehashed. |
| After hash, before checkpoint sync | Mapping and payload may survive, but no durable have bit trusts them. |
| After part-file sync, before SQLite | Durable mapping/payload are a safe false negative. |
| After SQLite | Conservative restart reopens the same slot and rehashes the claim. |

Release writes the missing-slot entry and forces its metadata transition before
the old slot becomes eligible for reuse. A format change, journal, slot-table
rewrite or migration is outside this tactical.

## Staged Implementation And Gates

1. Add standard-library positional full-read/full-write helpers with Unix and
   Windows implementations. Prove unaligned, out-of-order, short-read,
   interrupted/zero-progress and offset-overflow behavior without unsafe code.
2. Convert `StagingFile` to one retained positional handle and immutable
   write/hash plans. Keep final sync and rename behavior unchanged.
3. Add retained wanted-file handles and immutable selective write spans.
   Validate exact one-file, cross-file, skipped, padding, final-short and
   out-of-range geometry before payload mutation.
4. Give `PartFile` per-piece mapping generations and positional payload plans.
   Remove allocation-time sync, retain release/reuse fencing, and prove an old
   plan cannot write into a reused slot.
5. Make all-wanted and mixed wanted/skipped piece hashes one retained-handle
   positional blocking plan. Preserve fixed-buffer hashing and synthetic
   padding order; eliminate per-piece wanted-handle duplication and cursor
   fallback.
6. Pass formatting, warning-denying workspace clippy/tests, selective and
   mixed-source interop, session resume/crash matrices, generated-contract
   stability, and both Android Rust ABI cross-builds.
7. Run the 128 MiB engine and SQLite-backed session cohorts. Retain exact
   hashes, publication, cleanup, physical-write shape and source fingerprints.
   A neutral result is acceptable because worker concurrency remains closed;
   a regression must be explained or fixed before graduation.

## Pre-Change Controlled Baseline

The exact post-Tactical-052 engine binary at commit `cede50c` had SHA-256
`3c3fe38c5ac0e96a4fbcf19a72f5ca4e7a580f3a91fc1420a6ed05cd212ca557`.
The 128 MiB engine-only profile completed at 36.482, 35.500 and 35.792
seconds, for a 35.792-second median. It converted 8,192 logical blocks into
544--548 physical writes, spent 30.928--31.979 seconds in write service,
matched all three file hashes and cleaned every owned artifact.

The direct application-path baseline is Tactical `052`'s final exact
source-fingerprinted cohort: 45.740, 46.735 and 46.380 seconds, with a
46.380-second median, exactly 18 post-metadata revisions, complete 512-piece
have state, exact payload/publication and cleanup. Tactical `053` must not
change checkpoint policy or transaction shape.

## Implementation And Final Evidence

Commit `a495010` added one engine-owned full-range positional I/O boundary.
Its Unix implementation uses `FileExt::{read_at,write_at}`, Windows uses
`FileExt::{seek_read,seek_write}`, and unsupported platforms fail explicitly.
The loops retry interruption, advance partial operations, reject zero-progress
writes and short reads, and check every offset advance. `StagingFile` now
retains one positional handle, transfers its existing `Vec<u8>` into an
immutable `Arc<Vec<u8>>` without another payload copy, and performs writes and
fixed-buffer hashes in awaited blocking jobs.

Commit `b8847fa` applied the same boundary to selective storage. Wanted files
retain separate control and positional handles for their storage lifetime.
One selective plan resolves every wanted-file or part-slot destination,
payload interval, absolute offset and route generation, validates every span
before payload execution, and then performs the operation in one awaited
blocking job. All-wanted and mixed wanted/skipped hashes now consume retained
handles in torrent order in one job; the old per-piece wanted-handle
duplication and mixed cursor fallback are gone.

`PartFile` now owns per-piece mapping generations and returns immutable
`PartFileSpan` values. Allocation writes its four-byte mapping entry
positionally without a standalone `sync_data`; the joined checkpoint still
orders that entry and payload before SQLite. Release forces the missing entry
before making the slot reusable and increments the generation. Deterministic
tests prove a stale slot plan is rejected after reuse and a stale wanted-file
route prevents every span—including an earlier valid span—from mutating
payload.

The complete repository gate passed formatting, warning-denying workspace
clippy and every workspace test: 170 engine tests passed with three opt-in live
tests ignored, 78 session tests passed, and all protocol, gateway, desktop and
Android tests passed. Fresh generated web contracts produced no diff; 95 web
tests passed with two skipped, followed by strict TypeScript and the production
build. Bundled SQLite and the session library cross-compiled through the
installed NDK for `x86_64` and `arm64-v8a` at API 28.

Controlled evidence retained exact content and cleanup across three
selective-file runs, including skipped bytes, padding, part-file reopen,
7,000-byte materialization and all five piece hashes; the adverse plus healthy
mixed-source run; all nine comparator classification tests; and the
79,000-byte paired publication fixture. Three forced-restart runs retained
96--98 conservative claims and redownloaded only the missing remainder. The
three crash markers also passed: pre-sync and post-sync/pre-commit reopened
with zero durable pieces, while post-commit reopened with 11 durable pieces
and uploaded exactly the remaining 245 pieces. Every result matched the exact
payload hash and removed its owned artifacts.

The final 128 MiB engine cohort used executable SHA-256
`d6244ffca595fe57251a45254ba7ed7b3a74d7f500cd184629fd312b873ae8ea`.
Its three transfer times were 33.679, 34.134 and 33.279 seconds, for a
33.679-second median: 5.9% below the exact 35.792-second pre-change median.
All 8,192 blocks reached the same 16-block/256 KiB caps and exact file hashes.
Physical writes varied from 562 to 566 rather than the baseline's 544--548,
but serialized write service fell materially from 30.928--31.979 seconds to
27.131--28.353 seconds, and cleanup remained exact.

The SQLite-backed application cohort used executable SHA-256
`42247bbcdfc91bac4964eb975d7b0a038f89203228616589fe44860d23ab5594`.
It completed at 46.069, 45.594 and 45.590 seconds, for a 45.594-second median,
1.7% below Tactical `052`'s final 46.380-second median. Every run retained all
512 pieces, exact payload SHA-1
`9224038c2041d03f6f8eb46a7f618fc32cf34e67`, publication and cleanup, with
17--18 post-metadata revisions. Checkpoint policy and transaction shape are
unchanged.

The result is intentionally a foundation result rather than a worker-speed
claim. Positional execution removed cursor and handle barriers and improved
both retained medians without weakening integrity, restart, durability or
resource bounds. The remaining causal bottleneck is the one executing storage
operation and its FIFO write/verify ordering.

## Stopping Condition

The tactical completes when normal single-file, wanted-file and part-file
payload writes plus all piece-hash reads use immutable positional plans over
retained handles; part-slot identity is generation checked; allocation no
longer forces per-slot durability; stale plans fail before payload mutation;
the one-executor owner still cancels and joins exactly; path and descriptor
backends pass; and retained controlled profiles show no unexplained integrity,
resource or throughput regression.

The next boundary is a separate tactical for bounded independent write/hash
execution and an explicit piece-generation join. Tactical `053` does not open
that boundary merely because its plan types can support it.

## Non-Goals

- more than one executing write or hash, a session/root worker pool, worker
  count selection, fairness or multi-torrent scheduling;
- starting a hash before all writes for that piece complete, pending-write
  read-through or retaining payload beyond current completion ownership;
- dynamic file-priority migration, relocation, publication concurrency or a
  general fine-grained fence implementation;
- part-file format changes, compaction, journaling or migration;
- changing checkpoint intervals, SQLite policy, resume trust or publication
  semantics;
- memory mapping, direct I/O, vectored I/O, `io_uring`, unsafe code or a new
  dependency;
- peer, picker, request-window, discovery, protocol or UI layout policy; or
- visible UI, emulator or physical-device interaction.

## Escalation Contract

No routine implementation input is required. Internal engine refactoring,
bounded temporary fixtures, generated checks, headless controlled cohorts and
reasonable commits are authorized. Stop only for a new dependency or public
compatibility break, a part-file format/migration change, weaker crash or
resume semantics, destructive user-data action, visible/physical-device
interaction, or evidence that requires abandoning the accepted
storage-throughput architecture.
