# Tactical 023: Strict Endgame Ownership

Status: Complete

Topics: `download-correctness`, `peer-lifecycle`,
`performance-and-live-evidence`, `oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `022` restored sustained transfer and reached the 50% screen in 3/3
owner-only and 3/3 paired Big Buck Bunny runs. The next campaign milestone is
verified publication. RSTorrent currently permits only one active request for
a block, has no core cancel message, and must wait for a slow final request to
expire even when another idle peer has the block. A losing response is only
safe after timeout-based reassignment, not as an intentional duplicate.

Install strict, bounded endgame request ownership before using public complete
runs to choose further policy. The outcome is a runtime-independent block
owner that duplicates only after every remaining requestable block has an
active request, accepts the first valid response, cancels every live loser,
treats late losing payload as redundant, and keeps exact payload accounting.

## Source Dossier

Pinned libtorrent `2.0.13` at `7d7fc38fac61177fa5e02148f791b2f65250b09d`
is the behavioral completeness oracle. No source or fixture is copied.

- `settings_pack.hpp::strict_end_game_mode` permits a duplicate only after
  every block left to download has a request, avoiding premature redundancy.
- `request_blocks.cpp::request_a_block` first consumes all ordinary picks. If
  none remain, an otherwise idle unchoked connection may take one busy block;
  a connection with an existing request does not keep adding duplicates.
- `piece_picker.cpp::pick_pieces` excludes parole peers, chooses at most one
  busy block per pick, never duplicates a block from the same peer, and keeps
  all requesters until one response wins.
- `peer_connection.cpp::incoming_piece` marks the winning block as writing
  and calls `torrent::cancel_block` when more than one peer requested it.
- `peer_connection.cpp::cancel_request` removes unsent requests locally and
  emits a core cancel for sent requests while retaining a harmless
  not-wanted record for a response already in flight.
- `piece_picker.cpp::abort_download` removes only the named peer's ownership;
  the block remains requested while another peer still owns it.
- `test_piece_picker.cpp::picking_downloading_blocks` proves that ordinary
  blocks win over busy blocks and that an endgame pick returns only one busy
  block.

RSTorrent retains its own state and task architecture. The imported invariant
is strict duplicate timing, per-attempt ownership, first-response cancellation,
and harmless late payload.

## Ownership And Design

`SwarmState` remains the sole request owner. A requested block may retain
multiple active attempts, each identified by attempt and connection. Normal
scheduling remains unchanged until no missing requestable block remains.
Only then may an unchoked connection with no current request take one block
already requested from another connection. One connection cannot own two
active attempts for the same block.

Each active attempt reserves its possible payload against the existing
torrent-wide allowance. The number of duplicates is therefore bounded by the
30 established connections, one active attempt per connection and block, each
connection's request window, and the aggregate payload allowance. No separate
endgame buffer or unbounded history is introduced.

The first response carrying evidence from any retained attempt transitions the
block to writing. Every other active attempt becomes superseded, releases its
payload reservation, and returns a typed cancellation to the runtime. The
runtime sends core cancel messages before the storage wait. Failure to deliver
an advisory cancel does not invalidate the accepted block; normal socket
termination handles that connection. A losing response arriving before or
after storage completion is redundant and cannot release another reservation.

Choke, disconnect, expiry, and cancellation terminate only the affected
attempt. A block returns to missing only when it has no active attempt.
Terminal attempt history remains capped by the existing per-block bound.
Snapshots expose active request attempts, active duplicate attempts,
cumulative endgame assignments, cancellations, and redundant payload bytes.

The core peer codec gains message ID 8 with the same validated request shape
as message ID 6. Incoming cancel is decoded and ignored while upload remains
out of scope; malformed and oversized cancel frames remain protocol errors.

## Invariants And Bounds

- Ordinary missing blocks are always selected before any busy block.
- Endgame starts only when every incomplete requestable block is requested,
  writing, received, or verified; an idle peer receives at most one duplicate
  per scheduling pass and none when it already owns a live request.
- A connection has at most one active attempt for a block, and each active
  attempt has exactly one payload reservation.
- Accepting, expiring, choking, disconnecting, or cancelling an attempt
  releases its reservation exactly once without changing another attempt.
- The first evidenced response wins. All live losers become superseded and
  yield core cancels; later losing payload is redundant, never unsolicited.
- A block remains requested while at least one active attempt remains and
  returns to missing only after the last active attempt terminates.
- Per-block terminal history, established peers, request windows, command and
  event queues, and total payload retain their existing finite bounds.
- Cancellation and shutdown join every socket task and leave zero active
  attempts, pending dials, or payload reservations.

## Adversarial Validation

- Two peers own the only remaining block; the faster response wins, returns a
  cancellation for the slow peer, and a late losing response is redundant.
- Disconnect, choke, and expiry of one duplicate leave the other request live;
  terminating the final owner makes the block schedulable again.
- A peer with ordinary missing work or an existing request never receives an
  endgame duplicate; a peer never duplicates its own request.
- Duplicate reservations cannot exceed the torrent payload allowance, and all
  success, write-failure, cancellation, and shutdown paths return it to zero.
- Scripted peers observe the exact core cancel frame before a delayed storage
  barrier opens, then complete and join without queue leaks.
- Malformed cancel frames fail codec validation; valid incoming cancel does not
  disturb download ownership while upload is absent.
- Existing reassignment, late-payload, mixed-peer, tracker, DHT, resume,
  storage, and paired controlled publication gates remain green.

## Live Gate

After deterministic and controlled gates, run one clean owner-only common Big
Buck Bunny attempt to verified publication. If it completes, run three
alternating pairs. If it fails, retain endgame counts, remaining block and
piece counts, per-peer request state, and current capture time. Endgame owns a
failure only when all ordinary blocks are already covered and duplicate or
cancel progress is absent; hash mismatch selects the integrity owner instead.

## Implementation Evidence

`SwarmState` now represents a requested block with one or more active attempts
instead of embedding a single attempt ID in the block phase. Normal scheduling
still exhausts every missing block first. Once none remain, an unchoked peer
with no active request may take one busy block that it does not already own.
Every attempt reserves its full possible payload against the existing
torrent-wide allowance. Choke, disconnect, expiry, and cancellation remove
only the named connection's attempts and return the block to missing only
after its final owner terminates.

The first evidenced response transfers one reservation to the write, marks
every other active attempt superseded, releases their reservations, and
returns typed cancellations. The runtime writes all matching core ID-8 cancel
frames before entering the storage delay. Losing payload remains redundant
and cannot alter accounting. The terminal-attempt bound is now 30, matching
the established-connection bound so evidence for every possible live loser
can remain classifiable without an unbounded history.

Pure adversarial cases prove strict timing, one duplicate for an idle peer,
no duplicate while ordinary work remains, first-response cancellation, late
loser redundancy, partial and final owner teardown, rescheduling, and exact
payload high water. A scripted two-peer test holds both requests at a barrier,
lets the winner respond, observes the loser's exact cancel while the download
is still inside a 250 ms storage delay, then verifies publication, cleanup,
and zero active attempts. Valid and malformed request/cancel codec cases pass.

Formatting, warning-denying workspace clippy, and 230 workspace tests with
three explicitly ignored public-network tests pass. Controlled mixed-peer
liveness and paired 79,000-byte exact publication both pass; the paired gate
classified `both_reached` in 50 ms for RSTorrent and 94 ms for libtorrent.
This is a harness guard, not a public performance claim.

The clean owner-only common-profile Big Buck Bunny run published all
276,445,467 bytes and 1,055 pieces in 72.66 seconds, with 88 endgame
assignments, 88 cancellations, 32 KiB of redundant payload, and zero active
attempts at shutdown. Three alternating complete pairs then published exact
content for both owners in every run. RSTorrent took 80.22, 82.53, and 123.18
seconds versus libtorrent's 29.80, 29.93, and 30.32 seconds. Its bounded
endgame counters covered 12--59 assignments, 12--62 cancellations, 0--432 KiB
of redundant payload, and zero active attempts at every terminal snapshot.
Payload high water remained 13.68--15.42 MB and all temporary roots and tasks
cleaned up exactly.

This completes the tactical's functional and safety gate. The 2.76x median
and 4.06x maximum paired ratios do not meet the campaign's comparable gate;
that variance remains explicit performance debt. The next higher-priority
correctness slice is whole-piece recovery after a v1 hash failure.

## Non-Goals

- piece hash-failure reset, contributor reputation, banning, or parole
- rarest-first or measured peer selection, throughput tuning, PEX, upload,
  seeding, incoming sockets, NAT traversal, or persistent peers
- changes to request-window growth, tracker, DHT, connection limits, storage
  publication, UI, Tauri, browser, Android, or physical-device surfaces

## Validation And Stopping Condition

This tactical completed when focused codec and pure ownership tests, scripted
two-peer endgame and slow-storage cases, formatting, warning-denying workspace
clippy, workspace tests, controlled interoperability, and the public complete
gate established strict ownership, cancel delivery, late-loss safety, exact
accounting, publication, and cleanup. Tactical `024` owns the classified
integrity boundary. No human decision was required.
