# Tactical 028: Fair Content Supervisor Intake

Status: Active

Topics: `peer-lifecycle`, `download-correctness`,
`performance-and-live-evidence`, `oracle-driven-engine-campaign`

## Motivation And Outcome

Tactical `027` raised the bounded half-open cohort, but its exact public
completion exposed why newly supplied candidates still did not participate.
DHT reported 171 peers near content second 30 and 340 cumulatively near second
120 while the content registry stayed at 12 known peers until completion.
Storage retained its 66-job high water throughout the same interval.

The content supervisor deliberately receives only a storage completion while
its two-command local storage queue is occupied. In the ordinary path a biased
`tokio::select!` also ranks storage before deadlines, peer events, and
discovery. Continuous accepted-block storage can therefore starve the bounded
discovery channel and delay candidate admission until payload work ends.

Make event ownership explicitly fair without weakening storage or payload
bounds. Admit bounded discovery batches promptly during storage pressure,
start eligible dials independently of storage-command readiness, and rotate
ready storage, peer, and discovery owners when it is safe to consume peer
payload. Preserve exact cancellation and task join.

## Source Dossier

Pinned libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` remains the behavior oracle. No
source or fixture is copied.

- `torrent.cpp::tracker_response` adds validated peers to the peer list and
  immediately invokes `do_connect_boost`; accepted disk work does not defer
  peer-list intake until transfer completion.
- `torrent.cpp::do_connect_boost`, `want_peers`, and
  `session_impl.cpp::try_connect_more_peers` keep connection admission under
  explicit attempt and connection quotas rather than a disk-queue condition.
- `peer_list.cpp::connect_one_peer` consumes the bounded candidate cache while
  filtering reconnect delay, failure, and reachability state.
- libtorrent's disk and network schedulers are architecturally different, so
  RSTorrent adopts the observable liveness rule, not its task graph.

RSTorrent's exact owners are `ContentDiscovery`'s eight-event channel,
`ContentStoragePipeline`'s 64-command plus two-local-command bound,
`PeerSocketSet`'s 64-event channel, `SwarmState`'s 30 pending and 30 live
limits, and `run_selective_swarm_loop`.

## Ownership And Bounds

Discovery intake validates each address through `PeerSession` and inserts it
into the existing bounded `PeerRegistry`; it owns no payload bytes. The
supervisor may consume a discovery batch while storage is backpressured and
may start dials up to the existing pending limit because a half-open task owns
no request payload.

Established peer messages which may contain payload remain unread while the
two-command local storage queue is full. Pending dial completions may remain in
the fixed peer event channel until storage has capacity; the 30-attempt cohort
fits beneath its 64-event bound. Once peer consumption is safe, an explicit
rotating priority prevents any continuously ready storage, peer, or discovery
owner from monopolizing the loop. Cancellation is checked before every wait.

No new queue or task is introduced. The change cannot increase the 64-command
storage channel, two local pending commands, 66 total storage jobs, 64 peer
events, 30 half-open tasks, 30 established peers, or torrent payload
allowance. Request scheduling remains disabled while storage cannot accept its
resulting writes.

## Shape-Changing Edge Cases

- a full storage command queue plus two local commands cannot prevent an
  already queued discovery batch from entering the registry;
- repeated storage completions cannot starve a ready peer or discovery event
  once peer payload consumption is safe;
- a continuously replenished discovery source cannot starve storage progress;
- discovery during storage pressure may fill but never exceed 30 half-open
  attempts, and dial completions cannot exceed the 64-event peer queue;
- a payload message is not consumed when its write could exceed the local
  storage admission bound;
- cancellation with all discovery, peer, and storage queues ready is terminal
  and joins every owner; and
- tracker/DHT source tagging, address policy, private-torrent DHT gating,
  reconnect backoff, ranking, and established replacement remain unchanged.

## Staged Implementation And Gates

1. Extract a small runtime-independent rotating owner cursor or equivalent
   deterministic state and test complete rotation, unavailable-owner skipping,
   and bounded return to storage.
2. Add nonblocking bounded discovery intake under storage backpressure and
   remove the storage-readiness condition from half-open dial fill without
   consuming established peer payload in that state.
3. Add a scripted slow-storage case where DHT or tracker discovery supplies a
   useful peer after the storage queues saturate. Prove prompt registry intake,
   bounded dialing, continued exact storage accounting, and publication.
4. Add an adversarial always-ready-source or finite burst case proving storage
   cannot be starved, plus cancellation with saturated owners.
5. Run formatting, warning-denying workspace clippy, workspace tests, the
   mixed-peer and controlled paired gates, and comparator unit tests.
6. Run three product tracker+DHT 50% screens and one complete screen if clean.
   Compare discovery-report time to registry-growth and dial-start time in the
   retained timeline before interpreting throughput.

The tactical completes when deterministic and scripted evidence proves prompt
discovery admission without queue growth or payload overflow, every gate is
clean, and live timelines no longer defer supplied peers until terminal state.
If useful peers then grow while payload and verified rates remain flat, the
next owner is request service or storage. If admitted candidates repeatedly
form a weak cohort, the next owner is bounded libtorrent-derived ranking.

## Non-Goals

- changing tracker/DHT wire behavior, retry intervals, source ordering,
  candidate ranking, failure backoff, connection turnover, request windows,
  piece picking, endgame, or storage implementation
- adding queues, tasks, a session-wide scheduler, incoming connections,
  seeding, PEX, uTP, WebSeeds, IPv6, or NAT traversal
- UI, Tauri, browser, AVD, physical-device, or application-contract work

## Stopping And Escalation

No human decision is currently required. Stop only for a new external
dependency, product-visible contract, destructive user-data action,
persistence compatibility break, visible or physical-device interaction, or
evidence that safety requires a session-wide scheduler rather than the
bounded single-torrent owner. Public variance or a negative speed result is
evidence, not a blocker.
