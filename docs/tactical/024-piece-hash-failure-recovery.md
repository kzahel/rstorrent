# Tactical 024: Piece Hash-Failure Recovery

Status: Active

Topics: `download-correctness`, `peer-lifecycle`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

A SHA-1 mismatch currently protects verified state but terminates the entire
download. One corrupt block can therefore turn an otherwise healthy public
swarm into a fatal near-completion failure. This deterministic integrity and
liveness gap outranks Tactical `023`'s retained throughput variance.

Make a failed v1 piece schedulable again without disturbing unrelated verified
pieces. Retain bounded connection-generation attribution for every stored
block, count failed bytes and generations, and use only evidence strong enough
to justify peer exclusion. A uniquely contributing peer is known bad and is
banned; ambiguous contributors accumulate bounded suspicion without being
falsely identified as the corrupt source.

## Source Dossier

BEP 3 defines the v1 piece SHA-1 as the authoritative payload integrity check.
It does not identify which block within a failed piece was corrupt or prescribe
peer reputation policy.

Pinned libtorrent `2.0.13` at `7d7fc38fac61177fa5e02148f791b2f65250b09d`
is the behavioral completeness oracle. No source or fixture is copied.

- `torrent.cpp::piece_failed` obtains the piece picker's per-block
  contributors. For v1 it attributes an unknown failed block to the unique set
  of every contributor, accounts the entire piece as failed payload, and does
  not set the have bit.
- `torrent.cpp::penalize_peers` subtracts two trust points, increments a bounded
  hash-failure count, enables parole, and bans immediately only when the bad
  peer is known or after accumulated trust reaches its floor.
- `torrent.cpp::piece_passed` rewards every contributor by one trust point,
  capped at eight, and clears parole.
- `torrent.cpp::piece_failed` locks the piece while the disk owner clears its
  cached piece state. `torrent.cpp::on_piece_sync` then calls
  `piece_picker::restore_piece` and revisits outstanding queues.
- `piece_picker.cpp::restore_piece` unlocks and resets all failed finished or
  writing blocks for a v1 piece; block-selective restoration belongs to v2.
- `piece_picker.cpp::get_downloaders` retains one bounded contributor pointer
  per block. `test_piece_picker.cpp` covers contributor retrieval and full and
  selective restore transitions.

RSTorrent's storage operations and swarm transition are awaited serially by
one torrent owner; there is no independent disk cache to synchronize. It will
therefore reset the state only after the completed hash operation, without a
synthetic async-clear task. It adopts whole-piece reset, bounded attribution,
asymmetric reputation, and known-bad exclusion. Full parole scheduling is a
named next-slice boundary rather than an implicit partial claim.

## Ownership And Design

`SwarmState` remains the runtime-independent authority. A stored block retains
the winning connection generation and request-attempt evidence until its piece
passes or fails. `mark_piece_hash_failed` is valid only for a ready,
unverified piece. It returns the sorted unique contributing connection IDs,
changes every block in that piece from received to missing, increments bounded
cumulative failure diagnostics, and leaves all unrelated pieces unchanged.
No payload reservation remains after the reset.

The torrent driver treats a mismatch as a recoverable piece-generation result,
not a `DownloadError`. It emits typed bounded diagnostics and reschedules the
piece. Existing bytes need not be zeroed: every full block must be accepted
again before the owner can hash the new generation, and the storage mapping
overwrites every logical byte in the piece.

The peer registry owns bounded integrity reputation independently from
transport backoff. Passing a piece adds one trust point up to eight and clears
ambiguous suspicion. Failing a piece subtracts two down to minus seven and
increments a saturating hash-failure count. One unique contributor is banned
and disconnected immediately. Multiple contributors are recorded as
ambiguous; none is called known bad, and only an accumulated trust floor may
ban one. Reputation is attached through the retained dial attempt so stale
connection generations cannot affect a replacement record.

If the implementation cannot safely apply reputation while a connection is
live, it may close the exact generation before applying the terminal peer
transition. It must not ban an endpoint based on a stale callback. A peer
already gone still receives history through its matching attempt when the
registry can prove identity.

## Invariants And Bounds

- Hash failure never sets have state, publishes content, or invalidates another
  piece.
- Every block in a failed v1 piece becomes missing only after hashing finishes;
  no partial block-level salvage is claimed.
- Contributor attribution contains at most one entry per piece block and is
  deduplicated to at most the established-connection bound.
- Attribution uses the accepted request-attempt generation, not merely an IP
  address or whichever socket happens to be current.
- A uniquely contributing generation is known bad. Multiple contributors are
  suspects, not proof that each supplied corrupt data.
- Reputation values saturate at fixed bounds and do not create unbounded event
  or history retention.
- Retry reuses the existing block, payload, connection, cancellation, and
  shutdown bounds and cannot double-release a reservation.
- A later valid generation verifies and publishes exactly once; cumulative
  storage-write counters may include discarded bytes, while verified progress
  derives only from piece state.

## Staged Implementation And Gates

1. Preserve accepted source/evidence through stored state; add pure reset,
   attribution, unrelated-piece, accounting, and valid-retry cases.
2. Add bounded peer integrity reputation with known-single and ambiguous-multi
   deterministic cases, including stale-attempt rejection.
3. Make the content owner recover from mismatch, apply exact-generation peer
   actions, and expose typed snapshot/event counters.
4. Add scripted one-piece peers: a sole corrupt source is banned and a clean
   replacement completes; mixed corrupt contributors reset without false
   immediate bans and a clean generation completes.
5. Run formatting, warning-denying workspace clippy, workspace tests,
   controlled libtorrent publication, and one clean headless public complete
   screen. No visible desktop, browser, AVD, or physical device is required.

## Non-Goals And Next Boundary

- v2 block hashes, hash requests, hybrid torrents, or block-selective recovery
- persistent peer reputation, smart-ban disk comparison, IP filtering, or a
  session-wide ban store
- full parole piece ownership, clean-source preference, or malicious colluding
  peer defense beyond bounded evidence and accumulated trust
- request-window, picker-rarity, tracker, DHT, upload, incoming, UI, or product
  lifecycle changes
- tuning Tactical `023`'s retained complete-download throughput variance

If controlled evidence shows that immediate reselection of ambiguous suspects
prevents recovery, strict parole ownership becomes the next integrity tactical
rather than an unbounded addition here. Otherwise the campaign returns to the
paired completion critical path after this slice.

## Stopping And Escalation

This tactical completes when DL-C09 passes pure and scripted adversarial
coverage, a hash mismatch is nonterminal, a clean generation verifies and
publishes, known-bad and ambiguous attribution remain distinct and bounded,
all tasks and reservations clean up, and controlled interoperability remains
green. Public-swarm corruption is not required and must not be induced.

No human decision is currently required. Ordinary internal refactoring,
additional same-owner adversarial cases, conservative bounded counters, and
headless temporary artifacts are authorized. Stop only if evidence requires a
new persistence/product contract, external dependency, visible device action,
or materially broader peer-security policy.
