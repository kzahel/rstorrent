# Tactical 025: Bounded Async Content Storage

Status: Active

Topics: `download-correctness`, `performance-and-live-evidence`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

Correct public completion is now repeatable, but RSTorrent's three retained
paired full runs took 2.76x at the median and 4.06x at the tail versus pinned
libtorrent. The torrent supervisor currently awaits every 16 KiB seek/write
and every completed-piece reread before consuming another peer event. A clean
32 MiB single-piece localhost libtorrent-seed run takes 3.829 seconds, only
about 8.4 MiB/s, while the public reference completes 276 MB near 9 MiB/s.
This local ceiling is deterministic enough to investigate before changing
public peer selection.

Separate bounded storage execution from peer/scheduler progress. Accepted
payload remains torrent-owned and charged until a typed storage completion
returns. The peer supervisor can continue consuming messages, sending requests
and cancels, and observing disconnects while one storage owner serializes the
actual file operations. Piece verification and durable publication ordering
remain exact.

## Source Dossier

Pinned libtorrent `2.0.13` at `7d7fc38fac61177fa5e02148f791b2f65250b09d`
is the behavioral completeness oracle. No source or fixture is copied.

- `peer_connection.cpp::incoming_piece` validates request ownership, removes
  the network request, transfers the block to `disk_thread().async_write`,
  marks the picker block writing, cancels endgame copies, and immediately keeps
  request filling independent from write completion.
- The same path charges `queued_write_bytes` and per-peer
  `m_outstanding_writing_bytes`. Peers retain two disk blocks of headroom even
  when the shared watermark is exceeded so one busy peer cannot monopolize or
  deadlock disk admission.
- `peer_connection.cpp::on_disk_write_complete` releases queued write bytes,
  marks success or restores failed work, resumes a disk-throttled receive path,
  and initiates piece completion only from the serialized callback.
- `settings_pack.cpp` defaults `max_queued_disk_bytes` to 1 MiB.
  `settings_pack.hpp::max_queued_disk_bytes` and `disk_interface.hpp` define a
  bounded backpressure and observer contract rather than unlimited write
  spawning.
- `torrent.cpp::piece_failed` and `on_piece_sync`, surveyed in Tactical `024`,
  keep a failed piece unavailable until disk state and picker state agree.

RSTorrent adopts the separation, byte ownership, typed completion, and bounded
backpressure—not libtorrent's thread pool, disk cache, class graph, or buffer
allocator.

## Ownership And Design

One content-storage task owns the single-file or selective storage value and
is the only code allowed to seek, write, hash, sync, record verified state, or
return it for final publication. It consumes a bounded command channel and
emits a bounded typed completion stream. The task is created and joined inside
one torrent download and has an explicit cancellation/shutdown command.

An accepted block transitions to writing and transfers its `Vec<u8>` plus
winning attempt evidence to the storage owner. The existing torrent-wide
payload reservation remains charged through queueing and physical write; only
the write completion calls `finish_write` and releases it. At most 64 queued
16 KiB writes provide the initial 1 MiB storage-queue bound, while the existing
payload allowance remains the stricter total byte authority. One executing
command and bounded completion channel are included in snapshot high waters.

A piece hash is submitted exactly once only after every block write completed.
No block in a hashing piece is requestable. Hash success performs selective
sync and verified-record ordering before the torrent commits its durable
checkpoint and verified state. Hash mismatch returns Tactical `024`'s bounded
contributors and resets only after the storage job completes. Write failure
returns only the affected block to missing and preserves conservative durable
state before the existing error policy decides whether execution may continue.

The task may own storage by value and return it on a clean join. Ordinary
refactoring of `run_single_download`, `run_selective_download`, and
`ContentStorage` is in scope when required to establish that ownership. No
runtime, filesystem, or channel type may leak into `SwarmState`.

## Invariants And Bounds

- Peer payload, queued writes, executing writes, and storage completions remain
  within the existing aggregate payload allowance and explicit queue bounds.
- Each accepted block has exactly one storage command and one completion;
  cancellation, failure, and shutdown release its reservation exactly once.
- Peer events and advisory cancel delivery can progress while storage is slow;
  storage backpressure eventually stops new reads without dropping messages.
- One owner serializes physical operations, so overlapping writes and hashes
  cannot race the file cursor or selective mapping.
- Hashing begins once per ready piece only after all its writes completed.
- A verified/durable piece, failed generation, and publication retain their
  current ordering; shutdown never returns storage while its task is live.
- Command, completion, hashing-piece, task, payload, and diagnostic state are
  finite and observable without retaining payload in logs.

## Staged Implementation And Gates

1. Move storage behind an owned task with bounded commands, completions,
   cancellation, exact join, and high-water tests; do not change scheduling.
2. Transfer block writing to the task while preserving request evidence,
   endgame cancellation, write-failure restoration, and payload accounting.
3. Move hash, selective sync, verified recording, and storage return through
   the same owner; preserve Tactical `024` recovery and resume ordering.
4. Add a slow-storage multi-peer case proving peer-event freshness and bounded
   backpressure, plus cancellation during queued write and hash operations.
5. Run formatting, warning-denying clippy, workspace tests, controlled mixed
   peer, 32 MiB large-piece, paired controlled publication, and one public
   complete screen.

Before implementation, retain three clean 32 MiB baseline runs. Keep the
pipeline only if three post-change runs improve median wall time by at least
25% without weakening integrity, cleanup, or bounds, or if the adversarial
runtime gate proves a necessary liveness property the synchronous owner cannot
provide. Otherwise record the negative result and restore the simpler owner.

## Non-Goals

- changing request-window growth, piece rarity, peer scoring, tracker/DHT
  breadth, connection limits, or public timeout constants
- parallel writes to one storage value, `unsafe`, memory mapping, disk cache,
  read-ahead, direct I/O, file pools, or a session-wide disk scheduler
- changing the application contract, desktop/web/Android UI, SAF policy,
  checkpoint schema, or output publication semantics
- upload, seeding, incoming sockets, uTP, PEX, v2/hybrid torrents, or WebSeeds

If the controlled ceiling does not move, terminal and timed snapshots choose
peer selection or discovery as the next owner rather than widening this slice.

## Stopping And Escalation

This tactical completes when the storage task has explicit ownership,
cancellation, bounded backpressure, typed completion, and exact join; all
storage, integrity, endgame, resume, and interoperability gates pass; and the
controlled before/after rule either retains or rejects the optimization from
measured evidence. The public screen classifies its effect but cannot alone
prove causality.

No human decision is currently required. Stop only for a changed persistence
or product contract, external dependency, destructive user-data migration,
visible device action, or architecture broader than this torrent-local owner.
