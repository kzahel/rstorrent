# Tactical 029: Coalesced Selective Piece Hashing

Status: Complete

Topics: `download-correctness`, `performance-and-live-evidence`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `028` made discovery prompt and exposed a persistent downstream
ceiling. Its source-rich 50% run retained 30 connected peers but fell to about
0.1--0.8 MB/s while all 66 storage jobs remained occupied. A complete screen
timed out at 399 of 1,055 pieces with 65 writing blocks and 66 storage jobs,
despite zero hash failures and hundreds of retained candidates.

The ordinary single-file storage owner seeks once at the start of a piece and
reads each fixed verification chunk sequentially. `SelectiveStorage` instead
recomputes segments and calls async seek for every 16 KiB chunk. A common
256 KiB piece wholly inside one wanted file therefore performs 16 seek futures
plus 16 read futures rather than one seek plus 16 reads. Tokio file operations
cross a blocking boundary, so this is a concrete operation-count defect even
on SSD storage.

Coalesce contiguous wanted-file verification reads while retaining the fixed
16 KiB buffer, cross-file/skipped/padding correctness, and current storage-task
ownership. Establish a representative controlled multi-file timing profile
before the change and require deterministic proof of the reduced seek shape.

The implementation now obtains the complete piece segment map once, seeks
once at the start of each wanted-file segment, and streams that segment through
the fixed buffer. It reduced a common 256 KiB piece from 16 seeks and 16 reads
to one seek and 16 reads. The controlled timing remained neutral, and every
public screen still reached the 66-job storage high-water mark. This tactical
therefore proves the operation-count correction but does not claim a throughput
improvement. Tactical `030` owns the source-derived next step: one complete
all-wanted piece hash behind one blocking positional-I/O boundary.

## Source Dossier

Pinned libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` is the completeness oracle. No
source or fixture is copied.

- `posix_disk_io.cpp::async_hash` owns one disk hash job, allocates one fixed
  16 KiB buffer, and walks piece blocks inside that job before posting one
  completion.
- `mmap_disk_io.cpp::do_hash` likewise keeps one piece hash job and consults
  its store buffer before storage reads; hashing threads are separate from
  network/session callbacks.
- `mmap_storage.cpp::hash` maps logical piece ranges across files and hashes
  contiguous spans without making the session loop own file positioning.
- RSTorrent intentionally retains its smaller storage owner and Tokio files in
  this slice. It adopts contiguous-operation behavior, not libtorrent's mmap,
  cache, thread-pool, or storage architecture.

The corresponding RSTorrent owners are `SelectiveStorage::hash_piece`,
`TorrentLayout::segments`, `PartFile`, and the already bounded
`ContentStoragePipeline`.

## Ownership And Bounds

Hashing retains a single fixed `VERIFICATION_CHUNK_LENGTH` buffer regardless
of piece size. It asks `TorrentLayout` for the complete piece segment map once,
then seeks once at the start of each wanted-file segment and reads that segment
sequentially. Padding remains synthesized zeros. Skipped ranges remain in the
part file and keep its existing bounded slot mapping. The segment vector is
bounded by the validated metainfo file layout; there is no piece-sized byte
allocation.

The traversal is deterministic storage state, not async-runtime state. It
cannot change logical segment order, hash byte order, file selection,
verification authority, durable resume, or publication. Hash mismatch behavior
remains a whole-piece reset owned by `SwarmState`.

No piece-sized allocation, mmap, unsafe positional I/O, new descriptor, task,
queue, cache, or concurrent hash job is introduced. The 16 KiB buffer,
64-command plus two-local-command storage bound, payload charging, and exact
cancellation remain unchanged.

## Shape-Changing Edge Cases

- sixteen contiguous chunks inside one wanted file require one seek and hash
  the exact piece bytes;
- a piece crossing two wanted files seeks once in each file and preserves
  torrent byte order;
- every wanted-file segment starts with an explicit seek rather than trusting
  state retained across a file transition;
- skipped-file and padding segments cannot accidentally advance or reuse a
  wanted-file cursor;
- short reads, missing files, arithmetic overflow, and hostile layout ranges
  preserve their typed failures;
- a final short piece and the maximum accepted piece size retain the fixed
  verification buffer; and
- cancellation during a hash still joins the storage owner and leaves have
  state conservative.

## Staged Implementation And Gates

1. Extend the headless controlled storage profile with a representative
   256 KiB-piece multi-file fixture and retain three pre-change transfer-only
   timings, exact hashes, operation geometry, and cleanup.
2. Extract and test full-piece segment traversal, including file transitions,
   skipped ranges, and padding.
3. Apply it in `SelectiveStorage::hash_piece` without changing the public
   storage contract or buffer size. Add exact controlled and resume tests
   across file boundaries.
4. Retain three post-change timings. Operation-count reduction is mandatory;
   timing is supporting evidence and must not be overstated if noisy.
5. Run formatting, warning-denying workspace clippy, workspace tests,
   selective/mixed interop, nine comparator tests, and controlled paired
   publication.
6. Run three product tracker+DHT 50% screens and one complete screen if clean.
   Compare storage-job occupancy, verified/payload rates, peer utility, and
   publication against Tacticals `027` and `028`.

The tactical completes when common wanted-file pieces use one seek, all
cross-file integrity and lifecycle gates pass, and retained headless evidence
classifies whether storage remains the first sustained owner. If storage stays
saturated with little improvement, the next slice moves the complete hash job
behind one blocking/positional-I/O boundary. If storage drains while weak peers
retain slots, the next slice owns libtorrent-derived candidate ranking or
turnover. If useful peers remain but their queues collapse, it owns request
service.

## Non-Goals

- piece-sized buffers, mmap, direct I/O, unsafe positional reads, concurrent
  hash jobs, a general disk cache, or replacing Tokio storage
- changing peer ranking, turnover, requests, pieces, tracker/DHT behavior, or
  connection limits
- durable single-file resume, incoming connections, seeding, protocol breadth,
  UI, Tauri, browser, AVD, or physical-device work

## Stopping And Escalation

No human decision is currently required. Stop only for a new dependency,
unsafe I/O requirement, product-visible contract, destructive user-data
action, persistence compatibility break, visible or physical-device
interaction, or evidence requiring a general disk-cache architecture. A noisy
benchmark or negative public run is evidence, not a blocker.

## Implementation And Evidence

`SelectiveStorage::hash_piece` now obtains one full-piece segment map. Wanted
segments seek once and stream fixed 16 KiB reads; skipped segments preserve
part-file offsets and padding hashes zeroes. A focused 256 KiB test verifies
the exact hash, one wanted-file seek, 16 reads, and no part-file read. Existing
cross-file, skipped, padding, final-short, reopen, resume, publication, and
cancellation coverage remains green.

The new `tests/interop/selective_hash_profile.py` profile downloads a 32 MiB,
128-piece v1 torrent from controlled libtorrent into three deliberately
unaligned files. It checks all file SHA-1 values, final-piece SHA-1, info hash,
geometry, payload high water, publication, staging removal, part-file state,
and exact subprocess/session/temp-directory cleanup. Three pre-change timings
were 1.309, 1.101, and 1.093 seconds (median 1.101). Three post-change timings
were 1.458, 1.106, and 1.121 seconds (median 1.121). The 1.8% median regression
is ordinary local noise and explicitly rejects a speed-improvement claim.

Three tracker+DHT 50% screens reached the milestone twice at 77.76 and 77.89
seconds. The other timed out at 300 seconds with 506 of 1,055 pieces verified,
30 connected peers, 110 active request attempts, 61 writing blocks, 66 pending
storage jobs, and zero piece-hash failures. All three reached the 66-job
storage high-water mark. A full screen then verified all 276,445,467 bytes and
1,055 pieces at 180.61 seconds and published exact content at 180.64 seconds.
It had zero hash failures and drained all active requests and storage jobs;
its storage high-water mark was still 66.

The complete gate was `cargo fmt --all -- --check`, warning-denying workspace
clippy, and 249 listed workspace tests: 246 passed and the three explicit
public-network tests remained ignored. The selective profile passed 3/3, the
controlled mixed-peer profile passed, all nine comparator tests passed, the
paired controlled publication completed exactly for both implementations, and
the new profile passed before and after the change with exact cleanup.

Stopping condition: met. Common wanted-file pieces have the required seek
shape and all integrity/lifecycle gates pass. The neutral controlled result and
persistent live storage saturation select Tactical `030`; they do not justify
peer-policy tuning or a general disk cache yet.
