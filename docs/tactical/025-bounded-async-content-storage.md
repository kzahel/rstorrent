# Tactical 025: Bounded Async Content Storage

> Current resource ownership and bounds supersede the fixed queue and retained
> request/payload model in this historical tactical. See Tactical `039`.

Status: Complete

Topics: `download-correctness`, `performance-and-live-evidence`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

Correct public completion was repeatable, but RSTorrent's three retained
paired full runs took 2.76x at the median and 4.06x at the tail versus pinned
libtorrent. The torrent supervisor currently awaits every 16 KiB seek/write
and every completed-piece reread before consuming another peer event. A clean
32 MiB single-piece localhost libtorrent-seed run originally reported 3.829
seconds, but that wall clock also included deterministic fixture generation
and seeding setup. This tactical adds a transfer-only timer and retains the
corrected result below rather than treating the original total as an engine
ceiling.

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
payload allowance remains the stricter total byte authority. Two local
pending commands cover a saturated write queue plus a completion-triggered
verify command without consuming another peer payload. One executing command
and the bounded completion channel are included in snapshot high waters.

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

## Implementation And Evidence

One torrent-local task now owns either `StagingFile` or `SelectiveStorage` by
value. It serializes typed writes and hashes through a 64-command channel and
returns storage only after exact shutdown and join. Accepted payload remains
in `Writing` state and charged to the existing torrent allowance until its
typed completion is consumed. Selective sync and verified-record persistence
finish in the owner before the supervisor commits the resume checkpoint and
swarm verified state.

The task exposed two attempt-history bugs which are now covered directly. An
accepted late response changes its exact attempt from expired to received,
and finishing a write cannot prune the winning generation before a concurrent
piece hash attributes it. Hash-failure contributor collection includes both
received and still-writing blocks from the same generation.

The corrected 32 MiB harness times only the RSTorrent subprocess and can run a
specified old binary. Three runs of commit `9b87a1a` took 0.646, 0.326, and
0.331 seconds of transfer time (0.331-second median). Three runs of this
pipeline took 0.745, 0.426, and 0.426 seconds (0.426-second median). The async
owner is therefore about 29% slower in this localhost case and makes no speed
claim. It is retained under the tactical's alternate liveness rule: with the
first write delayed 250 ms, two peers deliver their complete payloads to the
supervisor within 100 ms while storage remains bounded, which the former
synchronous owner cannot do.

The 80-block saturation case reached exactly 64 queued commands and 66 pending
jobs including the executing and local-pending work, completed exact content,
and returned pending jobs to zero. Separate cases cancel during queued writes
and a delayed hash and prove exact task join and cleanup. Multi-peer liveness,
selective storage, endgame, corrupt-generation recovery, and the controlled
mixed-source cases remain green.

A controlled 79,000-byte paired publication reached both owners with exact
integrity and cleanup. RSTorrent published in 44.66 ms and pinned libtorrent
reached its milestone in 86.57 ms; the storage command, completion, and job
high waters were 1, 1, and 2. This is a correctness screen, not a throughput
claim.

One common-denominator public Big Buck Bunny owner screen verified and
published all 276,445,467 bytes and 1,055 pieces in 153.72 seconds. Metadata,
first piece, 50%, 95%, and 99% arrived at 0.42, 0.88, 64.53, 143.68, and
152.83 seconds. It ended with zero active requests, writes, storage jobs, or
hash failures; command, completion, storage-job, and payload high waters were
64, 1, 66, and 8,781,824 bytes. Public speed varied materially from the prior
86.05-second health screen, so this run proves retained completion and bounds,
not a causal latency change.

Formatting, warning-denying workspace clippy, seven comparator unit tests,
Python compilation, controlled interop, and the workspace suite pass with 243
listed tests including three ignored public tests. One earlier full-suite run
observed an extra valid retransmission in a 20 ms UDP test under concurrent
load; the focused
test passed five times, and its cached-token phase now uses a non-contentious
deadline so the test measures token reuse rather than scheduler latency.

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

This tactical completed when the storage task gained explicit ownership,
cancellation, bounded backpressure, typed completion, and exact join; all
storage, integrity, endgame, resume, and interoperability gates pass; and the
controlled before/after rule either retains or rejects the optimization from
measured evidence. The public screen classifies its effect but cannot alone
prove causality.

No human decision was required. Stop only for a changed persistence
or product contract, external dependency, destructive user-data migration,
visible device action, or architecture broader than this torrent-local owner.
