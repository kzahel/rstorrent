# Tactical 029: Coalesced Selective Piece Hashing

Status: Active

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
of piece size. A small cursor records only the last wanted file and next file
offset; a new seek is required at piece start, a file transition, or a
noncontiguous range. Sequential chunks of the same file read without another
seek. Padding remains synthesized zeros. Skipped ranges remain in the part
file and keep its existing bounded slot mapping.

The cursor is deterministic storage state, not async-runtime state. It cannot
change logical segment order, hash byte order, file selection, verification
authority, durable resume, or publication. Hash mismatch behavior remains a
whole-piece reset owned by `SwarmState`.

No piece-sized allocation, mmap, unsafe positional I/O, new descriptor, task,
queue, cache, or concurrent hash job is introduced. The 16 KiB buffer,
64-command plus two-local-command storage bound, payload charging, and exact
cancellation remain unchanged.

## Shape-Changing Edge Cases

- sixteen contiguous chunks inside one wanted file require one seek and hash
  the exact piece bytes;
- a piece crossing two wanted files seeks once in each file and preserves
  torrent byte order;
- returning to a previously used file after a noncontiguous segment requires
  a new seek rather than trusting stale position;
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
2. Extract and test the minimal contiguous wanted-file cursor, including file
   transitions, discontinuities, skipped ranges, and padding.
3. Apply the cursor in `SelectiveStorage::hash_piece` without changing the
   public storage contract or buffer size. Add exact controlled and resume
   tests across file boundaries.
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
