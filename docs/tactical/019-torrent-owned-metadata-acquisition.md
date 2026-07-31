# Tactical 019: Torrent-Owned Metadata Acquisition

Status: Complete

Topics: `peer-lifecycle`, `performance-and-live-evidence`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

The paired comparator exposed two metadata reliability cohorts below the
campaign gate and a 2.49x metadata latency in its first alternating tracker
pair. RSTorrent started up to three workers, but each owned a complete
independent `MetadataDownload`. Partial blocks died with a connection, peers
duplicated whole transfers, and one worker could not schedule around another.

Replace that boundary with one bounded, runtime-independent torrent metadata
owner shared by all connected metadata peers. The owner must assemble blocks
across peers, cap work per peer, release stalled/disconnected/rejected work,
accept harmless late responses, attribute corrupt assemblies, reset safely,
and preserve one winning connection for content. Then validate it through
hostile deterministic cases, scripted sockets, controlled libtorrent
interoperability, and paired public cohorts.

## Source Dossier

Pinned libtorrent `2.0.13` at `7d7fc38fac61177fa5e02148f791b2f65250b09d`
is the behavioral completeness oracle. No source or fixture is copied.

- `src/ut_metadata.cpp::ut_metadata_plugin` owns one torrent-wide metadata
  buffer and one `m_requested_metadata` block table shared by peer plugins.
- Each block retains request count, last request time, and the source that
  supplied accepted bytes. Selection chooses the least-requested missing
  block.
- A live source suppresses a duplicate request for three seconds. A peer has
  at most two outstanding metadata requests, but `maybe_send_request()` emits
  only one per invocation: another follows a response or the next peer tick.
- Size is bounded before allocation; inconsistent size, invalid piece, and
  invalid block geometry do not mutate accepted state.
- Hash failure resets every block. Contributing peers receive a request
  cooldown so a retry mixes sources; a one-block corrupt source receives a
  substantially longer penalty.
- `dont_have` applies a peer request cooldown instead of making the whole
  torrent terminal. Disconnect releases the source for reassignment.
- `test/test_fast_extension.cpp` covers directional extension IDs, reject of
  invalid pieces, and exact metadata message behavior.

RSTorrent inputs are `crates/rstorrent-protocol/src/metadata.rs`,
`PeerSession::acquire_metadata_inner`, `run_metadata_peer`, the Tactical `018`
diagnostic projection, and existing stalled/unsupported/rejecting scripted
peers. JSTorrent's `metadata-fetcher.ts` remains useful product vocabulary but
retains per-peer assembly and is not the owner model for this slice.

## Design And Ownership

`rstorrent-protocol::metadata::TorrentMetadataDownload` is the pure owner. It
contains the expected info hash, canonical bounded size, block bytes, accepted
source per block, request history, current assignments with issuance time,
peer outstanding sets, reject/hash cooldowns, and terminal completion. It
takes explicit peer IDs and caller-supplied monotonic millisecond instants; it
has no clock, sockets, Tokio types, locks, channels, files, or platform state.

The metadata supervisor owns one instance behind a narrow mutex because
several socket workers act concurrently. Each worker retains only its socket,
directional remote extension ID, peer-wire premetadata state, and progress
deadline. Before waiting, and after every relevant event, a worker asks the
torrent owner for at most its per-peer request allowance. A short bounded
timer observes assignment expiry without relying on a lossy notification.

```text
metadata supervisor
  registry + dial/socket/task ownership
             |
             v
  TorrentMetadataDownload (one per torrent)
     block bytes / assignments / sources / cooldowns
        ^               ^                ^
        |               |                |
     worker A         worker B         worker C ...
     socket + ID      socket + ID      socket + ID
```

The worker delivering the final hash-valid block parses the metainfo and
becomes the content connection. Losing workers are canceled and joined through
the existing supervisor cleanup path. Diagnostics project per-peer request and
receive counts plus torrent-wide size/block state; they never drive policy.

## Invariants And Initial Bounds

- At most eight pending or connected metadata peers exist in the engine; this
  matches the current ordinary content connection bound and remains explicit.
- Each peer owns at most two metadata assignments. At most sixteen wire
  requests are outstanding; ordinary scheduling avoids duplicate live
  assignments.
- A peer receives one request immediately. A second outstanding request is
  permitted after a response or a one-second ramp, matching libtorrent's
  event/tick cadence without importing its plugin shape.
- A block is at most 16 KiB and the existing one-MiB metadata ceiling remains.
- The first accepted valid size becomes canonical. No allocation occurs for
  invalid or inconsistent size.
- Missing blocks, current assignments, request history, accepted bytes, and
  source attribution have exactly one torrent owner.
- An assignment becomes reassignable after three seconds or immediately on
  disconnect/reject. A late response is accepted only if that peer previously
  requested the block; identical post-acceptance duplicates are harmless.
- `dont_have` cools that peer for at least twenty seconds and releases all its
  assignments. It does not fail another peer or the torrent.
- Hash mismatch never exposes bytes as metadata. It resets the entire block
  generation, releases assignments, and cools every contributing peer. A
  later source can complete without reconnecting unaffected peers.
- Request/source history is bounded by the metadata block and peer ceilings.
- Cancellation joins every dial and worker and leaves no active coordinator
  state reachable by a task.

The pure owner retains reject cooldown state while a peer is registered. The
current runtime retires a rejecting connection immediately, releases its work,
and records the failed attempt in the peer registry rather than occupying a
scarce metadata slot with a peer that said it cannot serve the request. This
is an intentional connection-lifecycle difference from libtorrent; the
torrent-wide nonterminal behavior and retry backoff are preserved.

## Adversarial Validation

### Pure state

- two peers each provide a disjoint block and only their combined dictionary
  completes;
- a stalled assignment expires and a late original response is harmless;
- disconnect and reject immediately release work while peer cooldown holds;
- conflicting sizes and malformed blocks preserve accepted geometry/bytes;
- a mixed-source corrupt assembly resets, attributes all contributors, and a
  clean third source completes;
- per-peer, total-request, block, peer, history, and allocation bounds hold.

### Scripted runtime

- peers serving disjoint metadata blocks complete although no peer can serve
  the whole dictionary;
- one peer stalls assigned blocks while another completes after expiry;
- a peer that handles only one metadata request at a time receives no
  immediate pipeline and completes before assignment expiry;
- a rejecting peer retires without failing the torrent, a corrupt mixed-source
  generation cools its contributors, and all tasks terminate exactly once;
- existing unsupported-extension, unrelated-message, late-discovery, DHT,
  private-torrent, and cancellation scenarios stay green.

### Interoperability And Live Evidence

- the controlled multi-block libtorrent metadata seed and full publication
  scenarios complete through the shared owner;
- run two ten-pair common-denominator tracker cohorts; attempt paired DHT and
  use the same bounded owner-only contract if the live reference cannot
  produce candidates;
- require at least 8/10 RSTorrent success, no opaque terminal state, and the
  campaign's two-cohort latency gate before declaring metadata comparable;
- if one cohort remains below the gate after source-derived owner work,
  classify its next boundary and rotate to sustained-transfer work rather
  than tuning blindly.

## Non-Goals

- No product UI, incoming listener, upload/seeding path, PEX, LSD, uTP, NAT
  traversal, v2/hybrid metadata, payload picker, endgame, or storage change.
- No copy of libtorrent's plugin/class layout, random penalty distribution, or
  raw implementation details.
- No unbounded connection race intended solely to win a public benchmark.
- No CI public-swarm threshold.

## Implementation And Evidence

The first bounded implementation checkpoint establishes:

- one `TorrentMetadataDownload` shared by up to eight metadata workers;
- two requests per peer, sixteen total active assignments, least-requested
  missing-block selection, one-second per-peer ramp, three-second expiry, safe
  late responses, and bounded source history;
- cross-peer assembly, canonical geometry, whole-dictionary hash validation,
  contributor attribution, generation reset, and contributor cooldown;
- an engine scheduler wake independent of peer traffic, a hard metadata
  progress deadline, and cooperative worker removal/cancellation;
- pure tests for disjoint blocks, expiry/late duplicates, rejection,
  mixed-source corruption recovery, and geometry/peer bounds;
- scripted real-socket completion where two peers each serve only part of a
  three-block dictionary, stalled-source reassignment, and recovery from a
  complete corrupt three-block generation through a delayed clean source;
- snapshot and public-probe counters for whole-dictionary hash failures and
  the number of contributors in the latest failed generation; and
- passing loopback `magnet_metadata.py`, `dht_magnet.py`, and paired controlled
  full-publication interoperability against libtorrent 2.0.13.

The protocol architecture gate rejected an initial `std::time::Duration`
input. The final owner instead accepts `MetadataInstant`, a millisecond
monotonic value with no runtime dependency; the engine performs the conversion
at its outward boundary.

The first public screen ran two alternating Big Buck Bunny pairs per profile.
Tracker discovery completed 2/2 for each owner: RSTorrent verified metadata in
3.40 and 3.88 seconds (3.64-second median), while libtorrent took 20.35 and
20.67 seconds (20.51-second median). Fresh DHT completed 2/2 for RSTorrent in
30.10 and 55.37 seconds, but libtorrent timed out both 120-second runs with
zero candidates. Those runs classify `rstorrent_only`; they prove functional
RSTorrent DHT acquisition but cannot establish paired latency while the
reference bootstrap is unavailable. All four RSTorrent runs received the
exact two-block dictionary with zero hash failures and clean shutdown.

Two independent ten-pair Big Buck Bunny tracker cohorts then completed 9/10
for RSTorrent and 10/10 for libtorrent in each cohort. RSTorrent's successful
medians were 5.72 and 4.12 seconds versus 20.52 and 20.33 seconds. Median
paired ratios were 0.28x and 0.20x; p90 ratios were 1.50x and 1.58x. Each lone
RSTorrent miss retained block zero from a six-candidate pool without hash,
cleanup, or resource failure, satisfying the functional and two-cohort
comparable gates.

The fresh-DHT reference remained unable to produce peers even after its
adapter received the same three bootstrap hosts documented by libtorrent and
used by RSTorrent. A separate exact-settings libtorrent session populated its
routing table, but three paired torrent attempts still reported zero
candidates. The comparator therefore gained a schema-compatible owner-only
mode. Ten isolated RSTorrent DHT runs completed 10/10 in 31.40–66.96 seconds
(56.64-second median, 59.80-second p90), with exact identity, zero hash
failures, and clean shutdown. This closes RSTorrent's former 7/10 functional
gap while retaining paired DHT latency as an external live-reference boundary.

The initial three-pair confirmation on each other catalog torrent exposed a
second-block interoperability failure: RSTorrent completed Cosmos 0/3,
Sintel 2/3, Tears of Steel 1/3, and WIRED CD 2/3 while libtorrent completed
all. Endpoint-free attempt diagnostics showed a capable peer serving block
zero, then ignoring an immediately pipelined block one while RSTorrent retried
until its 15-second progress deadline. Source inspection found libtorrent's
one-request-per-invocation cadence. A deterministic one-at-a-time peer now
proves the one-second ramp, and the corrected public matrix completed 12/12
for each owner. Every RSTorrent run used exactly two requests for two blocks,
had zero hash failures, and cleaned up. Tears of Steel improved from 1/3 to
3/3 in the direct A/B screen.

## Validation And Stopping Condition

Run focused protocol and engine tests, `magnet_metadata.py`, `dht_magnet.py`,
the paired metadata cohorts, `cargo fmt --all -- --check`, workspace clippy
with warnings denied, workspace tests, locked Python tests, and
`git diff --check`. Keep all work headless and remove raw reports/payloads.

This tactical is complete when one pure torrent owner drives all metadata
workers, every adversarial case above passes, controlled libtorrent exchange
still publishes correct bytes, public cohorts meet the functional gate or
retain a newly classified external boundary, the owner remains bounded, and
the living topics and campaign checkpoint record the evidence.
