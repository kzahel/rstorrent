# Tactical 026: Paired Peer Utility Timeline

Status: Complete

Topics: `peer-lifecycle`, `performance-and-live-evidence`,
`oracle-driven-engine-campaign`

## Motivation And Outcome

RSTorrent now completes the common-denominator Big Buck Bunny transfer, but
three retained paired runs took 2.76x at the median and 4.06x at the tail
versus pinned libtorrent. Tactical `025` disproved synchronous local storage as
that speed owner and retained asynchronous storage for peer-event liveness.

Terminal snapshots cannot distinguish a weak peer cohort from connection
admission, request service, or picker behavior over the preceding transfer.
The latest public run ended with 161 content candidates, 48 dial attempts, and
five connected peers, but that final state does not say when candidates became
available, which connections were useful, or why the aggregate rate changed.

Add one bounded endpoint-free timeline to both comparator owners. Use it to
classify the first owner-level divergence from libtorrent across three paired
full transfers, then record the exact source-derived behavioral tactical which
follows. This is measurement infrastructure, not permission to tune several
policies at once.

## Source Dossier

Pinned libtorrent `2.0.13` at `7d7fc38fac61177fa5e02148f791b2f65250b09d`
is the behavioral and diagnostic oracle. No source or fixture is copied.

Survey before finalizing fields and interpretation:

- `peer_list.cpp::find_connect_candidates`, `compare_peer`, and
  `connect_one_peer` for bounded candidate scan, failure preference, source
  ranking, last-connect ordering, and cached candidates;
- `torrent.cpp::want_peers`, `do_connect_boost`, `try_connect_peer`, and peer
  turnover handling for connection demand, startup breadth, limits, and
  replacement cadence;
- `peer_connection.cpp` request-queue, download-queue, payload accounting,
  snub, choke, and disconnect transitions;
- `include/libtorrent/peer_info.hpp` and `torrent_status.hpp` for the stable
  diagnostic meanings exposed through the Python binding; and
- adjacent libtorrent tests which pin candidate order, connect limits, queue
  state, and turnover.

RSTorrent's corresponding owners are `PeerRegistry`, `PeerSelector`,
`PeerSocketSet`, `SwarmState`, `ContentPeerActivitySnapshot`, the public probe,
and the Python comparator. Runtime-independent registry and swarm state remain
free of Tokio, process, and JSON types.

## Timeline Contract And Bounds

Sample each active owner once per second from metadata completion through the
terminal milestone, with an immediate first and final sample. Retain no more
than 1,024 samples per owner. A run beyond that bound deterministically halves
the interior history while retaining the first and final boundaries; exact
milestone times remain in the separate top-level milestone record. The vector
must never grow without bound.

Every sample uses elapsed monotonic time and endpoint-free aggregates:

- verified pieces and bytes plus interval verified-byte rate;
- discovered, eligible, dialing, connected, unchoked, interested, useful,
  stalled, and zero-payload peer counts when that owner exposes the meaning;
- active request, queued payload, writing-block, request-target, and storage
  backlog totals;
- aggregate payload rate and bounded per-peer rate/queue distributions; and
- source counts and connection churn totals where available.

Owner-specific fields remain explicitly named and nullable. Do not fabricate
equivalence where libtorrent and RSTorrent expose different meanings. Do not
retain IP addresses, ports, peer IDs, client strings, tracker URLs, raw alerts,
payload, or per-peer time series in the committed report schema.

Timeline collection must not change peer selection, request scheduling,
timeouts, task priorities, or event-channel behavior. Sampling reads existing
bounded snapshots and libtorrent status in the same monitor loop; it may not
introduce a new engine task or synchronous hot-path I/O.

## Classification And Decision Branches

For each pair, classify the earliest sustained divergence over at least three
samples:

1. **candidate supply**: RSTorrent lacks eligible peers while the reference
   has a materially larger useful set;
2. **admission or turnover**: eligible candidates exist while live capacity is
   unused or retained zero-utility peers occupy it;
3. **request service**: similarly useful cohorts exist but RSTorrent's active
   queue, payload service, or verified-byte rate diverges;
4. **picker/storage**: payload arrives comparably but verification or
   publication falls behind; or
5. **inconclusive swarm variance**: the paired cohorts are not comparable
   enough to name a repeated owner.

If all three pairs name candidate supply, the next tactical owns discovery or
candidate retention. If at least two name admission/turnover, it owns bounded
peer utility and replacement. If at least two name request service, it owns
the request/piece scheduler. If payload and verification diverge, it returns
to the measured storage/picker boundary. An inconclusive result rotates to one
additional catalog torrent rather than adding speculative policy.

## Staged Implementation And Gates

1. Add deterministic tests for sample aggregation, rates, nullable fields,
   the 1,024-sample bound, milestone retention, and endpoint scrubbing.
2. Add RSTorrent timeline capture to the existing public probe without
   changing the application or engine contract.
3. Add the closest honest libtorrent aggregates from its Python binding and
   tests for absent/version-dependent fields.
4. Extend comparator schema/classification tests and keep old reports readable
   if the repository already treats them as inputs.
5. Run formatting, warning-denying clippy, workspace tests, Python tests, and
   controlled paired publication.
6. Run three alternating common-denominator Big Buck Bunny full pairs with
   exact integrity and cleanup, classify the repeated owner, update the living
   topics, and open its decision-complete tactical.

The tactical succeeds when both owners emit bounded honest timelines through
the same report, deterministic and controlled gates pass, and retained paired
evidence selects a source-derived owner or the explicit rotation branch. It
does not require a latency improvement by itself.

## Implementation And Evidence

Both owners now emit the same endpoint-free utility sample shape. It includes
verified bytes and interval rate; known, eligible, connecting, connected,
unchoked, wanted, useful, active, stalled, and zero-payload peer counts;
request and disk/storage backlog; aggregate rate; bounded per-peer rate and
queue distributions; and source/dial totals where the owner exposes them.
Unavailable meanings are `null`, including libtorrent's internal request
target and RSTorrent's byte-exact disk backlog. The Rust probe samples existing
diagnostic snapshots; it adds no engine task or hot-path I/O.

Pure Rust and Python tests pin nearest-rank distributions, interval rates,
the 1,024-sample/coalescing bound, first/final retention, nullable fields, and
endpoint scrubbing. The installed Python `2.0.13.0` binding exposes
`torrent_status` peer-list, candidate, connection, piece, byte, and payload
rates plus `peer_info` choke, interest, snub, queue, total payload, current
rate, and pending-disk fields. It does not expose target download queue length,
timed-out request count, or several C++-only queue details, so those remain
honestly absent.

Formatting and warning-denying workspace clippy pass. The workspace lists 245
tests with 242 passing and three intentionally ignored public-network probes;
all nine comparator unit tests pass. The final controlled pair completed exact
publication, integrity, bounded timeline emission, and cleanup for both
owners.

The controlled 79,000-byte paired fixture reached verified publication and
clean cleanup for both owners through the new schema. Three alternating public
Big Buck Bunny full pairs then classified `both_reached` with exact
276,445,467-byte integrity and no cleanup or bound failures. RSTorrent
published in 132.89, 134.43, and 138.24 seconds; libtorrent published in
30.87, 30.89, and 31.11 seconds. The 4.35x median ratio misses the campaign's
comparable gate.

Normalizing at metadata makes the first divergence unambiguous. At three to
five seconds of content time, RSTorrent knew 10--16 peers, had three or four
connections and two useful peers, and received about 2.2--2.7 MB/s.
Libtorrent knew 60--65 peers, had 17--20 connections and 11--14 useful peers,
and ramped to about 12--29 MB/s. RSTorrent had no idle eligible candidate in
these samples: every known peer was connected, dialing, or backed off. This
classifies common-profile candidate supply before ranking, request service,
or picker policy.

A follow-up product-path screen changed the next premise. Tracker plus DHT
provided 159 content candidates by metadata; after one second, 119 were still
eligible while exactly eight were dialing and six were connected. Pinned
libtorrent permits unlimited half-open sockets under its 200-connection global
bound, issues a 30-attempt startup boost, and budgets 30 attempts per second.
RSTorrent took about 100 content seconds to grow from six to 29 connections
while holding the eight-attempt ceiling continuously. It reached 50% only at
143.94 seconds, with 30 connections, 92 still-eligible candidates, exact
cleanup, and no integrity failure. The changed candidate supply therefore
selects bounded half-open admission as the next falsifiable owner before
candidate ranking or another request-window change.

## Non-Goals

- changing peer ranking, connection limits, turnover, request windows,
  picker policy, tracker/DHT behavior, storage queues, or timeout constants
- UI, Tauri, browser, AVD, physical-device, or application-contract work
- incoming connections, upload/seeding, PEX, uTP, WebSeeds, IPv6, v2/hybrid,
  NAT traversal, or metered/VPN policy
- persistent peer history, raw packet capture, per-peer endpoint logs, or a
  general metrics service

## Escalation

Headless source inspection, comparator edits, controlled libtorrent work,
three bounded public pairs, temporary downloads, cleanup, documentation, and
commits are already authorized. No human decision is currently required.

Stop only for a product-visible contract, new external dependency or license
posture, destructive user-data action, persistence compatibility break,
visible or physical-device interaction, or evidence that requires broadening
beyond measurement and selection of the next owner. A public timeout, missing
optional Python field, or owner disagreement is a classification result, not
a blocker.
