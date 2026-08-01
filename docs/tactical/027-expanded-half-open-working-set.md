# Tactical 027: Expanded Half-Open Working Set

Status: Complete

Topics: `peer-lifecycle`, `performance-and-live-evidence`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `026` first found sparse tracker-only candidate supply. Its product
tracker+DHT screen then supplied 159 candidates by metadata and changed the
boundary: after one content second, 119 candidates were eligible, exactly
eight were half-open, and only six were connected. RSTorrent held that
eight-attempt ceiling while taking about 100 seconds to grow to 29 live peers.
It reached 50% at 143.94 seconds with 92 candidates still eligible.

Raise the torrent-local half-open working set from eight to 30 so a source-rich
torrent can evaluate one libtorrent-sized startup cohort without waiting for
sequential 15-second connection timeouts. Preserve the separate 30-established
peer bound, torrent-wide payload allowance, endpoint validation, cancellation,
and exact task join. Do not change peer ranking or request scheduling in this
slice.

## Source Dossier

Pinned libtorrent `2.0.13` at `7d7fc38fac61177fa5e02148f791b2f65250b09d`
is the behavioral completeness oracle. No source or fixture is copied.

- `torrent.cpp::tracker_response` and `do_connect_boost` immediately offer new
  tracker peers to the connection owner. `torrent_connect_boost` defaults to
  30 and consumes the session's current connection quota.
- `session_impl.cpp::try_connect_more_peers` distributes a default 30 new
  attempts per second across torrents while respecting the 200-connection
  session limit. Boost attempts are deducted from the next ordinary tick.
- `torrent.cpp::want_peers` continues requesting attempts while the torrent is
  active, below its connection limit, and has connect candidates.
- `peer_list.cpp::find_connect_candidates` scans at most 300 records, retains
  ten ranked candidates at a time, and skips peers still inside fail-count
  scaled reconnect delay. `connect_one_peer` drains that cache.
- `settings_pack.hpp` documents the 15-second peer connect timeout as
  especially important when half-open attempts are limited. The pinned ABI's
  old half-open limit defaults to unlimited; the session connection budget is
  the actual hard owner.
- `peer_info.hpp` and Tactical `026`'s live binding evidence distinguish
  connecting from established peers. In all three common pairs libtorrent
  retained 29--45 connecting peers while 17--20 were established during its
  ten-second content transfer.

RSTorrent adopts the 30-attempt startup breadth under its existing explicit
torrent bounds. It does not adopt libtorrent's session graph, unlimited
per-torrent connection setting, or 200 live sockets.

## Ownership And Bounds

`SwarmConfig` remains the pure owner of pending and established limits.
`SwarmState` owns exact pending identifiers and rejects the 31st. The content
supervisor owns dial creation and immediately refills freed pending capacity
while an eligible candidate exists. `PeerSocketSet` owns every connect and
handshake task until a typed completion or cancellation joins it.

The new defaults are at most 30 half-open plus 30 established peers for one
torrent. Half-open tasks own no request payload. Established peers continue to
share one fixed torrent payload allowance; increasing sockets cannot multiply
buffered piece data, active-piece count, storage queue, or request bytes.
Thirty dial completions fit within the existing 64-event peer channel.

A late successful dial is admitted only below the established bound or through
the existing deterministic replacement path. Cancellation closes admission,
cancels and joins all pending and established tasks, clears registry phases,
and releases every socket. Failed and timed-out attempts retain the current
registry backoff and failure limits.

No session-wide socket scheduler exists yet. That remains a named prerequisite
before general concurrent multi-torrent operation; it does not make this
single-torrent source-backed bound unbounded.

## Shape-Changing Edge Cases

- 30 silent handshakes cannot prevent a prompt cancellation and exact join.
- the 31st candidate remains eligible rather than disappearing or consuming a
  live slot;
- a useful candidate inside positions 9--30 can complete before the old
  eight-attempt cohort's connect timeout;
- simultaneous dial completions cannot exceed 30 established peers or mutate
  a newer registry generation;
- disconnect, handshake failure, protocol rejection, and timeout refill the
  pending cohort without duplicate attempts;
- a full established set permits at most the existing single replacement
  probe and does not create a 31st ordinary connection; and
- offline/loopback policy, private-torrent DHT gating, IP/port validation, and
  integrity bans remain final checks before dialing.

## Staged Implementation And Gates

1. Change the pure default and strengthen exact 30/31 pending-bound and
   established/pending truth-table tests.
2. Add a scripted source-rich case with at least 30 slow or silent candidates
   and a useful peer beyond the former first eight; prove useful payload before
   the 15-second connect timeout and retain exact cleanup.
3. Add saturated cancellation and simultaneous-completion cases if existing
   peer-task tests do not already cover the larger cardinality.
4. Run formatting, warning-denying clippy, workspace tests, the mixed-peer and
   controlled paired publication gates, and comparator unit tests.
5. Run three headless tracker+DHT owner screens to 50% and retain candidate,
   pending, established, useful-peer, request, storage, and milestone
   timelines. Run one complete product-path screen if the cohort remains
   integrity-clean.
6. Compare the source-derived ramp directly: supplied runs should begin up to
   30 attempts without exceeding the bound and should not retain eligible
   candidates solely because only eight attempts are in flight.

Retain the change when deterministic liveness and exact resource gates pass.
Public latency classifies the next owner but cannot veto a necessary bounded
startup cohort from one changing swarm. If broader admission leaves payload
and verified rate unchanged while storage remains continuously saturated, the
next tactical returns to a representative multi-file storage profile. If
useful-peer count rises but request service does not, it selects the
request/picker owner. If candidate ordering repeatedly chooses a weak cohort,
it selects source-independent bounded peer ranking.

## Non-Goals

- changing tracker or DHT protocols, reannounce intervals, peer-source
  retention, ranking, failure backoff, established-peer turnover, request
  windows, piece picking, or storage execution
- adding a session-wide multi-torrent scheduler in this slice
- UI, Tauri, browser, AVD, physical-device, or application-contract work
- incoming connections, upload/seeding, PEX, uTP, WebSeeds, IPv6, NAT
  traversal, v2/hybrid torrents, or VPN/metered policy

## Stopping And Escalation

The tactical completes when the 30/31 bounds, useful-peer liveness, saturation
cancellation, controlled interop, and product-path timeline screens pass; the
living topics record whether the next owner is storage, request service,
ranking, or confirmation breadth. No human decision is currently required.

## Implementation And Evidence

The pure default now permits exactly 30 pending dial identifiers independently
from the existing 30 established connections. The 31st attempt is rejected at
the pure owner and becomes admissible only after one exact pending identifier
finishes. The torrent payload allowance, active-piece limit, request windows,
storage queues, and peer event channel are unchanged.

A scripted runtime places one useful peer at observation position 30 behind 29
peers which accept the TCP and BitTorrent handshakes but never reply. All 30
attempts begin together and the useful peer publishes two verified pieces in
about 50 ms, well before the configured five-second handshake deadline which
would have held the old first cohort. A separate 30-silent case observes every
outbound handshake, cancels, joins every peer task, returns every registry
record to idle, and leaves only the documented resumable staging artifact
which the test removes.

Formatting, warning-denying workspace clippy, and 246 listed workspace tests
pass; 243 tests pass and three changing-public-network probes remain
intentionally ignored. The controlled 1 MiB mixed swarm, nine comparator unit
tests, and controlled paired publication all pass with exact integrity and
cleanup.

Three product tracker+DHT 50% screens reached the exact 528-piece threshold in
61.47, 64.21, and 68.34 seconds with clean cancellation and integrity. Those
runs supplied only 12--16 content candidates, so their 64.21-second median
cannot attribute an improvement to the wider cohort. One complete screen
published all 276,445,467 bytes and 1,055 pieces in 149.42 seconds with zero
hash failures and all request and storage jobs drained.

That completion timeline selected a preceding supervisor-fairness defect. DHT
reported 171 peers around content second 30 and 340 cumulatively around second
120, while the content registry stayed at 12 known peers until the terminal
sample. The code agrees: while storage has local backpressure,
`next_content_supervisor_event` awaits only storage completion; outside that
state its biased selection ranks storage before peer and discovery events.
Candidate intake and dial refill can therefore remain queued for most of a
healthy transfer. Tactical `028` owns fair bounded discovery admission before
storage, ranking, or request-policy tuning.

Stop only for a new external dependency, product-visible contract, destructive
user-data action, persistence compatibility break, visible or physical-device
interaction, or evidence requiring a session-wide architecture in order to
make the bounded single-torrent change safe. Ordinary public variance, failed
peers, or a negative latency screen is evidence, not a blocker.
