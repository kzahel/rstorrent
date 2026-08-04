# Oracle-Driven Engine Campaign

Topic: `oracle-driven-engine-campaign`

Status: Resumed for the accepted maximum-throughput storage campaign by
maintainer direction on 2026-08-02. The
paired headless comparator and bounded utility timeline drive metadata,
first-piece, sustained-transfer, endgame, and full publication parity from
pinned libtorrent source and tests. The former serialized storage owner has
been replaced by bounded independent execution, and its accepted
multi-tactical architecture is recorded in
[`storage-throughput-architecture.md`](storage-throughput-architecture.md).
Tactical [`052`](../tactical/052-batched-durability-checkpoints.md) has
completed its checkpoint split and retained performance gate; Tactical
[`053`](../tactical/053-immutable-positional-storage-plans.md) has completed
the immutable positional-storage slice. Bounded independent write/hash
execution, raw ceilings and the repeated large-transfer matrix now pass in
Tactical [`054`](../tactical/054-bounded-independent-storage-execution.md);
its SQLite-backed application, closing interoperability, restart/crash,
platform and public evidence also pass, so that tactical is complete. The
intervening desktop inspection and view-set foundations are complete enough to
expose the new checkpoint stages as the engine evolves.
High-impact BEP breadth still follows the core common-denominator parity gate.

## Purpose And Scope

This topic is the durable runbook and restart checkpoint for the current
engine campaign. It exists so a fresh session or compacted context can resume
from repository state without relying on transcript memory.

The campaign owns:

- the source-first oracle procedure;
- milestone order and graduation rules;
- paired live-comparison policy above the detailed measurement contract;
- rotation and escalation rules;
- documentation and commit checkpoints; and
- the exact current tactical and next executable action.

It does not replace the focused correctness, peer, discovery, performance, or
protocol topics. It coordinates them. Individual code changes remain bounded
by numbered tacticals.

## Authority And Desired Outcome

The user has authorized autonomous source research, implementation,
refactoring within the active tactical, public tracker and DHT work, repeated
headless downloads, multi-gigabyte comparison cohorts, and reasonable commits.
The machine has unmetered gigabit home Internet. Network volume is therefore
not a reason to avoid a useful cohort, although every run still needs explicit
time, disk, process, and cleanup bounds.

The desired outcome is:

1. RSTorrent approaches pinned libtorrent common-denominator reliability and
   performance for metadata, first verified piece, 50%, and verified
   publication;
2. the result generalizes beyond one torrent and remains resource bounded;
3. deterministic and controlled evidence covers the state-changing edge cases
   learned from specifications and libtorrent; and
4. once the core parity gate is met, full-reference deltas drive high-impact
   BEP breadth.

Matching libtorrent does not mean copying its class graph, source, defaults,
or every optional feature. RSTorrent remains an independently authored Rust
engine with its own explicit ownership boundaries.

## Documentation And State Owners

| Record | Owns |
| --- | --- |
| `CLAUDE.md` / `AGENTS.md` | Mandatory entry path and source-first campaign contract |
| This topic | Runbook, parity rules, campaign checkpoint, and next action |
| `capability-readiness.md` | Exactly one **Now**, at most three **Next**, and readiness roll-up |
| `performance-and-live-evidence.md` | Comparator schema, measurement policy, cohort summaries, and artifact safety |
| `download-correctness.md` | Integrity, liveness, completion, and adversarial scenario ledger |
| `peer-lifecycle.md` | Candidate, dial, connection, request-owner, and replacement behavior |
| `protocol-support.md` | Exact BEP claims and supporting evidence |
| Active numbered tactical | One bounded implementation slice, source dossier, gates, results, and deferrals |
| Tests and comparator JSON | Executable evidence and bounded raw run results |

Raw public peer addresses, tokens, payloads, profiles, and verbose logs remain
temporary. The tactical and topics retain aggregate evidence and failure
shapes without committing sensitive or unstable endpoint data.

## Compaction And Fresh-Session Resume Protocol

A resumed agent must not infer campaign state from conversation summaries
alone. Before taking an engine action:

1. read `CLAUDE.md`, this topic, `capability-readiness.md`, and the active
   tactical named in **Current Checkpoint**;
2. read every focused topic named by that tactical;
3. inspect `git status`, recent commits, and the tactical's recorded evidence;
4. reconcile the working tree with **Last Completed Evidence** and **Next
   Executable Action** below;
5. if code and checkpoint disagree, establish the actual state with read-only
   inspection and update the checkpoint before proceeding; and
6. continue the next safe in-scope action without asking the user to repeat
   already recorded decisions.

At every bounded commit or deliberate pivot, update:

- the active tactical status and actual validation;
- the focused topics whose truth changed;
- `capability-readiness.md` if the queue or evidence changed; and
- **Current Checkpoint** in this topic.

The transcript is commentary. These repository records and executable tests
are the campaign authority.

## Source-First Oracle Procedure

Experiments validate and quantify a source-derived design. They are not the
primary way to rediscover mature behavior.

Before finalizing each implementation slice, record a source dossier in its
tactical containing:

1. the exact pin from `reference/pins.toml`;
2. normative BEPs and relevant ambiguity or interoperability notes;
3. exact libtorrent source paths, symbols, and tests inspected;
4. state owners and the data flow between torrent, peer, piece/block,
   discovery, storage, and session state;
5. connection, request, retry, timeout, cancellation, and shutdown behavior;
6. resource bounds, pruning, queue limits, and hostile-input handling;
7. shape-changing edge cases that must be represented before the common path;
8. behavior RSTorrent will adopt, behavior it will intentionally differ from,
   and why; and
9. independently authored deterministic and interoperability tests that will
   prove the chosen behavior.

The normal reference map is:

| Area | Primary pinned libtorrent owners |
| --- | --- |
| Metadata | `src/ut_metadata.cpp`, `src/bt_peer_connection.cpp`, `test/test_fast_extension.cpp` |
| Peer candidates | `src/peer_list.cpp`, `src/torrent_peer.cpp`, `test/test_peer_list.cpp` |
| Connection lifecycle | `src/peer_connection.cpp`, `src/bt_peer_connection.cpp`, `src/torrent.cpp` |
| Requests and endgame | `src/request_blocks.cpp`, `src/piece_picker.cpp`, `test/test_piece_picker.cpp` |
| Tracker lifecycle | `src/tracker_manager.cpp`, `src/udp_tracker_connection.cpp`, `test/test_tracker.cpp` |
| DHT | `src/kademlia/traversal_algorithm.cpp`, `get_peers.cpp`, `rpc_manager.cpp`, `test/test_dht.cpp` |
| Session/resource ownership | `src/session_impl.cpp`, `src/torrent.cpp`, `src/disk_io_thread_pool.cpp` |

Inspect adjacent code and tests when the named owner delegates an invariant.
Do not stop at a symbol list: understand the relevant transitions and failure
paths before choosing RSTorrent's design.

## Oracle Layers

Every material behavior advances through the cheapest layer that can prove it:

1. pure values, codecs, deterministic state transitions, and property tests;
2. scripted hostile peers, trackers, DHT nodes, clocks, storage, and process
   lifecycle;
3. controlled interoperability with pinned libtorrent;
4. common-denominator public comparison;
5. full-reference public comparison; and
6. product-surface validation only when engine behavior actually reaches a
   platform boundary.

Live evidence can expose or quantify a problem. It cannot by itself prove the
cause, integrity, or BEP support.

## Milestone Vocabulary

Comparator results never use an ambiguous top-level `success`. Each
implementation reports these independent milestones when applicable:

- `metadata_verified`;
- `first_piece_verified`;
- `payload_50_percent`;
- `payload_95_percent`;
- `payload_99_percent`;
- `all_pieces_verified`;
- `storage_published`; and
- `shutdown_joined`.

Each milestone records elapsed time and the owner-specific state needed to
explain delay or failure. Correct identity, hashes, byte counts, publication,
and owned-task termination are hard gates.

## Comparison Modes And Ordering

Use two reference modes:

- **Common denominator** enables only discovery and transport capabilities
  both implementations claim. It measures shared engine behavior and owns the
  parity gate.
- **Full reference** runs ordinary libtorrent capabilities and current
  RSTorrent product capabilities. Its delta measures the practical value of
  missing BEPs and informs breadth priorities.

Within a cohort:

- run implementations sequentially in isolated temporary roots;
- use an ABBA or recorded deterministic randomized order;
- distinguish cold and warm DHT sessions;
- retain exact settings, implementation revisions, platform, and run order;
- clean ordinary payload and runtime state after extracting results; and
- classify outcomes before comparing latency.

The campaign may run full multi-gigabyte cohorts without additional user
approval. Check available disk first, keep a wall-time/process bound, and do
not retain duplicate payloads merely because bandwidth is inexpensive.

## Functional And Comparable Gates

Public swarms vary, so the campaign uses two gates.

**Functional** means:

- every deterministic and controlled case in the tactical passes;
- at least 8/10 primary public common-denominator runs reach the milestone;
- no integrity, publication, cleanup, or resource-bound failure occurs; and
- every incomplete run has a classified terminal boundary rather than an
  opaque stall.

Reaching functional permits work on the next milestone while a named parity
gap remains active.

**Comparable** means two consecutive alternating primary cohorts show:

- RSTorrent completes no more than one fewer run than libtorrent;
- no integrity, publication, cleanup, or resource-bound failure;
- median paired milestone latency at most 2x libtorrent;
- 90th-percentile paired milestone latency at most 3x libtorrent; and
- CPU, peak RSS, disk, and queue high-water marks are either within 2x where
  both owners can measure them or have a source-backed bounded follow-up.

These ratios are initial campaign thresholds, not permanent product promises
or CI gates. Tighten or revise them only from recorded evidence, not to make a
failing result appear green.

For Big Buck Bunny, use ten paired runs for metadata and first piece, then ten
paired full runs once the harness is stable. After a milestone is functional
on the primary torrent, run three paired repetitions on every other catalog
torrent before over-optimizing the primary swarm. Cross-torrent completion or
integrity failures take priority over smaller latency improvements.

Core parity is complete when metadata, first piece, 50%, and verified
publication are comparable on the primary tracker and DHT scenarios, every
catalog torrent completes the common-denominator scenario in its confirmation
cohort, and no known deterministic correctness scenario remains failing.

## Source-Derived Change Loop

For each milestone:

1. capture the paired baseline and classify the slow or terminal boundary;
2. complete the source dossier before changing state shape;
3. turn reference edge cases and the observed failure shape into deterministic
   scenarios;
4. implement one coherent owner-level slice, including proportional
   refactoring;
5. run formatting, lint, workspace, focused adverse, and controlled interop
   gates;
6. screen the live change with a small paired cohort when useful;
7. confirm a meaningful result with the full milestone cohort;
8. record negative experiments as evidence rather than silently discarding
   them; and
9. commit the bounded slice with the owning `Topic:` trailer.

One-variable A/B experiments are appropriate when libtorrent exposes a policy
range or when resource/calibration choices remain after the state model is
correct. Do not tune around a state model already known to be incomplete.

## Rotation And Anti-Stall Policy

Keep exactly one implementation tactical active and no more than three named
hypotheses within it. If a source-surveyed, deterministically tested change
does not move the live boundary:

1. preserve its result and decide whether the invariant is still required for
   correctness;
2. inspect the next owner in the recorded critical path rather than randomly
   changing constants;
3. after three exhausted owner-level hypotheses, mark the bottleneck for
   revisit and rotate to another milestone or catalog torrent that can expose
   new evidence; and
4. return when a later source survey, feature, or trace changes the premise.

Rotation does not waive integrity failures or abandon an active task with
unjoined work. It prevents one noisy public condition from blocking progress
on independent engine owners.

## Transition To BEP Breadth

Before core parity, add protocol breadth only when it is required for the
common path, closes an integrity/security hole, or unlocks several parity
milestones. Once core parity is complete:

1. run common-denominator and full-reference cohorts across the catalog;
2. attribute material deltas to missing capabilities where the evidence
   supports that conclusion;
3. rank BEP work by completion impact, cross-torrent frequency, prerequisite
   value, integrity/security risk, and implementation cost;
4. select one bounded BEP tactical; and
5. apply this same source-first, deterministic, controlled, and live evidence
   ladder before changing `protocol-support.md` claims.

Likely breadth candidates include PEX, uTP, IPv6 discovery, WebSeeds, incoming
participation, and v2/hybrid torrents. The measured full-reference delta, not
list order or novelty, chooses the actual sequence.

## Escalation Contract

Continue autonomously for source inspection, tactical and topic updates,
ordinary refactoring, deterministic fixtures, controlled libtorrent work,
headless public cohorts, bounded temporary downloads, cleanup, and reasonable
commits.

Stop for human direction only when evidence requires:

- a materially different product or architecture decision;
- a new external runtime dependency or changed license posture;
- a persistence or compatibility break outside the tactical;
- destructive or non-recoverable user-data action;
- visible UI or physical-device interaction not already authorized; or
- a scope expansion that changes the campaign outcome rather than a normal
  implementation route.

An ordinary failure, public timeout, difficult refactor, conservative resource
choice, or disagreement between RSTorrent and libtorrent architecture is not a
reason to stop.

## Current Checkpoint

Campaign state: **active by maintainer direction**.

Active tactical:
[`054-bounded-independent-storage-execution.md`](../tactical/054-bounded-independent-storage-execution.md).
Most recently completed tactical is
[`053-immutable-positional-storage-plans.md`](../tactical/053-immutable-positional-storage-plans.md).
Tacticals `025` through
[`032-bounded-coalesced-write-batches.md`](../tactical/032-bounded-coalesced-write-batches.md)
are complete.

Current milestone: execute bounded independent write/hash jobs and an explicit
piece-generation join over the completed positional-plan boundary.

Last completed evidence:

- commit `948ea96` closed Tactical `018`'s inspectable metadata acquisition,
  lifecycle fixes, and provisional tracker-port implementation;
- commit `e199b1f` established this runbook and retained the initial paired
  reference evidence;
- the Tactical `015` comparator now has its catalog, RSTorrent probe,
  libtorrent adapter, alternating order, deterministic classification tests,
  and bounded cleanup;
- its first common-denominator metadata pair completed both owners at 51.32
  seconds for RSTorrent and 20.63 seconds for libtorrent, with identical
  276,445,467-byte, 1,055-piece, three-file geometry;
- a controlled three-piece comparison verified both adapters, publication,
  classification, and cleanup through the shared schema;
- its first public full pair classified `reference_only`: RSTorrent timed out
  at 461/1,055 pieces and 43.7% after 900 seconds, while libtorrent published
  in 30.88 seconds through the same common-denominator profile;
- RSTorrent metadata-only Big Buck Bunny cohorts completed 8/10 through UDP
  trackers and 7/10 through fresh DHT sessions;
- pinned libtorrent `2.0.13.0` completed both corresponding cohorts 10/10;
- successful medians were 32.77 versus 20.94 seconds for trackers and 78.69
  versus 0.90 seconds for DHT;
- repeated tracker failures retained partial metadata from multiple attempts;
  repeated DHT failures discovered many peers but sent no metadata request;
- Tactical `019` installed one runtime-independent torrent metadata
  owner shared by eight workers, with two requests per peer, three-second
  reassignment, cross-peer assembly, source attribution, and hash recovery;
- its pure hostile cases and scripted two-source, three-block socket case pass;
- direct metadata, DHT-only metadata/content, and paired controlled full
  publication remain green against locked libtorrent 2.0.13;
- a two-pair tracker screen completed 2/2 for both owners at 3.64 seconds
  median for RSTorrent versus 20.51 seconds for libtorrent; and
- a two-pair fresh-DHT screen completed 2/2 for RSTorrent at 30.10 and 55.37
  seconds, while libtorrent found zero candidates and timed out twice at 120
  seconds, so the screen proves RSTorrent function but not paired parity;
- commits `11eecb1` and `111e2a6` established torrent-wide assembly, corrupt
  generation recovery, and hash-failure diagnostics;
- two independent ten-pair tracker cohorts each completed 9/10 for RSTorrent
  and 10/10 for libtorrent, with RSTorrent median paired ratios of 0.28x and
  0.20x and p90 ratios of 1.50x and 1.58x;
- ten owner-only fresh-DHT RSTorrent runs completed 10/10 with a 56.64-second
  median and no integrity or cleanup failure, while the contemporaneous
  libtorrent torrent lookup produced zero candidates in three bounded runs;
- the first four-torrent breadth matrix exposed immediate request pipelining
  as a sparse-peer interoperability failure; pinned `ut_metadata.cpp` showed
  one request per event/tick, and a deterministic one-at-a-time peer now owns
  that edge case; and
- after the source-derived one-second ramp, Cosmos, Sintel, Tears of Steel,
  and WIRED CD completed 12/12 paired confirmations for each owner with two
  requests, two blocks, zero hash failures, and clean shutdown; and
- Tactical `020`'s pre-change first-piece screen completed 3/3 for both
  owners. RSTorrent took only 0.22--0.38 seconds from metadata to first piece,
  but every terminal snapshot remained capped at four requests and 64 KiB
  with `requestwindowsfull`, confirming sustained width as the next owner;
- the adaptive window grew live targets to 21--46 and retained 3/3
  first-piece completion; and
- the first 50% screen completed 1/3 for RSTorrent. Its successful pair was
  28.14 versus 27.98 seconds, while both misses stranded more than 700
  requests on formerly productive peers without marking them stalled,
  selecting adaptive request-response inactivity as the next owner;
- commit `6c636ab` added bounded response-time sampling, whole-window stall
  release, and current content-registry diagnostics; and
- its clean owner-only 50% screen completed 1/3 in 24.09 seconds. The misses
  retained only four or nine candidates and two connections, while pinned
  libtorrent source showed bounded initial fan-out across not-yet-working
  tracker tiers and a 30-peer connect boost, selecting Tactical `021`'s
  startup working-set owner;
- commit `f85e2a0` installed eight-operation tracker fan-out with exact task
  cleanup and endpoint-free discovery totals; and
- the next clean 50% screen completed 0/3, but expanded every run to two
  tracker batches, 14--15 candidates, 17--19 dials, and five or six live
  peers. Established plus pending counts exactly filled the old combined
  eight-slot check while eligible candidates remained, selecting content-peer
  admission rather than tracker breadth;
- commit `5bc4719` separated eight half-open attempts from a source-derived
  30-peer live bound; and
- its clean 50% screen also completed 0/3, at 25, 55, and 96 pieces. It
  exhausted all 14--15 candidates across established, dialing, and backed-off
  states, while aggregate request targets and rates could not identify which
  live peer owned hundreds of outstanding requests. A bounded endpoint-free
  peer queue and utility table became the narrow diagnostic owner; and
- commit `9bdb8a9` added that table. Its clean classification run froze after
  a one-to-two-second-old peer snapshot in which a fast peer had delivered
  6.24 MiB and accumulated 383 requests. The 16-command producer and 64-event
  consumer can block each other exactly at that boundary, selecting Tactical
  `022`'s duplex task liveness owner; and
- commit `b91109d` made peer tasks drain bounded commands while inbound event
  delivery waits. The clean owner-only screen reached 50% in 3/3 runs at
  34.70--55.37 seconds, and the alternating screen classified `both_reached`
  in 3/3 at 30.74--45.82 seconds for RSTorrent versus 24.00--25.80 for
  libtorrent. All runs cleaned up exactly, selecting strict endgame and
  verified publication rather than another transport-liveness change; and
- commit `85b200c` installed strict endgame ownership and core cancellation.
  One owner-only and three alternating full Big Buck Bunny screens published
  exact content and cleaned up. The alternating RSTorrent runs took
  80.22--123.18 seconds versus libtorrent's 29.80--30.32 seconds, for a 2.76x
  median and 4.06x maximum ratio. Endgame remained bounded at 12--59
  assignments, 12--62 cancellations, 0--432 KiB redundancy, and zero active
  attempts. The functional completion gate passes; comparable performance
  remains open after the higher-priority fatal hash path; and
- Tactical `024` installed whole-piece v1 hash reset, exact retained
  connection-generation contributors, asymmetric bounded trust, immediate
  exclusion of a sole corrupt source, and ambiguous suspicion without false
  immediate bans. Pure transitions and scripted sole-source and mixed-source
  corrupt generations retry and publish cleanly with zero leaked attempts or
  payload reservations;
- its workspace gates pass with 240 listed tests including three ignored
  public tests. Controlled mixed-peer publication remains exact, and the
  paired 79,000-byte controlled fixture took 46.93 ms for RSTorrent versus
  72.21 ms for libtorrent with exact cleanup for both; and
- its clean public Big Buck Bunny screen published all 276,445,467 bytes in
  86.05 seconds with zero hash failures and exact cleanup. A separate clean
  32 MiB single-piece localhost run took 3.829 seconds. Pinned libtorrent's
  accepted-block async-write and 1 MiB queued-disk-byte ownership, compared
  with RSTorrent awaiting every 16 KiB write and piece hash in its supervisor,
  selected Tactical `025` before peer-policy tuning; and
- Tactical `025` installed one torrent-local storage task with a 64-command
  queue, two-command local admission bound, typed write/hash completions,
  retained payload charging, resume-before-verified ordering, cancellation,
  and exact join. Slow-storage, queue-saturation, cancellation, integrity,
  selective-storage, endgame, and controlled-publication gates pass;
- Tactical `039` later supersedes that resource model for current product
  behavior. Outstanding request reservations now release when accepted payload
  transfers to an independently byte-bounded storage owner; desktop uses
  256 MiB request, 32 MiB received-payload, and 256 MiB active-piece limits,
  while Android uses 128 MiB, 16 MiB, and 128 MiB. The historical evidence in
  this checkpoint remains evidence for the commits that produced it;
- the corrected transfer-only 32 MiB timer measured the synchronous commit at
  a 0.331-second three-run median and the asynchronous owner at 0.426 seconds,
  a 29% regression. The owner is retained only because a 250 ms write-delay
  case proves two peers can deliver complete payload within 100 ms while the
  prior synchronous supervisor cannot consume those events;
- one public common-denominator Big Buck Bunny owner screen published all
  276,445,467 bytes and 1,055 pieces in 153.72 seconds with zero hash failures,
  active storage jobs, active requests, writes, or cleanup failures. Storage
  command, completion, job, and payload high waters were 64, 1, 66, and
  8,781,824 bytes; and
- Tactical `026` added one-second endpoint-free utility timelines to both
  comparator owners with deterministic 1,024-sample bounds, rate aggregation,
  nullable owner-specific fields, and endpoint scrubbing;
- its three alternating common-profile Big Buck Bunny full pairs completed
  exact publication for both owners. RSTorrent took 132.89--138.24 seconds
  versus libtorrent's 30.87--31.11 seconds, a 4.35x median paired ratio;
- three to five content seconds after metadata, RSTorrent knew 10--16 peers,
  had three or four connections and two useful peers, while libtorrent knew
  60--65, had 17--20 connections and 11--14 useful peers. This classified
  common-profile candidate supply; and
- a product tracker+DHT screen changed that premise by supplying 159
  candidates at metadata. After one content second, 119 remained eligible,
  eight were dialing, and six were connected. The run held that eight-attempt
  ceiling while taking roughly 100 seconds to grow to 29 connections and
  reached 50% at 143.94 seconds, selecting Tactical `027`'s source-derived
  30-attempt startup cohort;
- Tactical `027` raised only the pending-dial bound. A useful position-30 peer
  completed behind 29 silent handshakes in about 50 ms, and 30 fully silent
  attempts canceled, joined, and returned the registry to idle exactly;
- three clean product 50% screens completed in 61.47--68.34 seconds. Their
  sparse 12--16-candidate swarms made the latency result inconclusive for the
  expanded cohort;
- one clean complete screen published 276,445,467 bytes and 1,055 pieces in
  149.42 seconds with zero hash failures and all request/storage jobs drained;
  and
- that timeline reported 171 cumulative DHT peers around content second 30
  and 340 around second 120 while the content registry remained at 12 known
  peers until termination. The storage-backpressured supervisor branch awaits
  storage alone and the ordinary biased branch ranks storage before discovery,
  selecting Tactical `028`'s fair intake owner;
- Tactical `028` installed deterministic three-owner rotation and starts dials
  independently from storage-command readiness. Its saturated 80-block test
  admits and dials delayed DHT discovery while exactly 66 storage jobs remain
  occupied, then publishes and joins every owner;
- three live 50% screens put each first DHT report and registry increase in the
  same sample and immediately filled 30 dials. Two took 69.15 and 69.29
  seconds; one source-rich run took 282.74 seconds;
- a 300-second full screen timed out at 399 pieces and 104,595,456 verified
  bytes with zero hash failures, 30 connections, 91 requests, 65 writes, and
  66 storage jobs; and
- the persistent storage saturation plus `SelectiveStorage`'s 16 seeks per
  common 256 KiB hash, versus one seek in the single-file owner, selects
  Tactical `029` before peer ranking or request tuning;
- Tactical `029` now maps each piece once and reduces that common hash to one
  seek plus 16 fixed reads. A focused test proves the operation count, and all
  controlled integrity, lifecycle, resume, publication, and cleanup gates pass;
- the representative 32 MiB three-file profile moved from a 1.101-second
  pre-change median to 1.121 seconds post-change. This neutral result rejects a
  speed claim rather than being hidden;
- three live 50% screens reached the 66-job storage high-water mark. Two
  reached 50% at 77.76 and 77.89 seconds; one timed out at 506 of 1,055 pieces;
  and
- a complete screen verified all 276,445,467 bytes at 180.61 seconds and
  published exact content at 180.64 seconds with zero hash failures and drained
  queues. Persistent saturation selects Tactical `030`'s complete hash-job
  boundary;
- Tactical `030` now gives each all-wanted piece one bounded blocking
  positional-I/O job with exact one-file, cross-file, padding, truncation,
  task-failure, mixed-source, cancellation, and Android-target evidence;
- its 32 MiB controlled median was a neutral 1.139 seconds versus 1.121 before.
  Two public 50% screens took 79.47 and 223.85 seconds, while one timed out at
  359 of 1,055 pieces; and
- a complete screen timed out at 375 pieces and 98,304,000 verified bytes with
  zero hash failures, 30 peers, 86 requests, 66 writes, and 66 storage jobs.
  Repeated full queues without controlled hash improvement select Tactical
  `031`'s per-command duration evidence;
- Tactical `031` now publishes fixed saturating per-kind starts, completions,
  cumulative/max queue wait, cumulative/max service duration, and active
  operation age with exact controlled lifecycle and nullable reference schema
  evidence;
- its formatting, warning-denying clippy, 253-test workspace gate, selective
  and mixed-source interop, nine comparator tests, paired controlled
  publication, and both Android target checks pass; and
- three public 50% screens attributed 93.2--93.7% of wall time to serialized
  storage service. Writes consumed 87.7--88.2%, hashes 5.5--5.7%, and exact
  integrity and cleanup held. This selected write execution rather than
  hashing, peer ranking, or request policy; and
- Tactical `032` now drains only already-admitted writes into 16-block/256 KiB
  batches, coalesces exact adjacent same-piece ranges, preserves logical
  completions and verification fences, and exposes physical versus logical
  diagnostics. Its full 258-test workspace gate, controlled interop,
  comparator, and both Android ABI checks pass;
- the controlled 32 MiB profile reduced 2,048 logical blocks to 144--154
  physical writes but produced a neutral 1.143-second median versus 1.196
  before; and
- three alternating product 50% pairs classified `reference_only`.
  Libtorrent reached 50% in 27.26--29.94 seconds while RSTorrent timed out at
  345--351 pieces. Batching reduced roughly 5,700 blocks to about 500 physical
  operations, but combined write/hash service remained 93.0--94.2% of wall
  time with zero hash failures and successful cleanup. This selects serialized
  storage execution without opening its concurrency design.

Tactical `052` pre-change evidence now includes:

- a 128 MiB engine-only steady profile with 512 pieces, a 37.594-second
  three-run median, 542--546 physical writes and 31.108--34.088 seconds of
  serialized write service; and
- a separate SQLite-backed 512-piece session profile built from exact commit
  `e618d2b`, with a 50.085-second median and an exact 514-revision
  post-metadata amplification on every run. A prior stale-binary observation
  was rejected after executable fingerprinting was added.

`SessionStore::record_pieces`, the de-duplicating application sink and one
coherent batched `ViewHub` Piece/Files transition pass exact duplicate,
bounds, revision and rollback tests. The engine now queues hash-verified
pieces to one bounded joined checkpoint owner, synchronizes unique targets,
and invokes the batch sink outside the supervisor. Its matched three-run
profile has a 45.221-second median and 16--18 revisions, with exact content and
cleanup; forced-death resume also passes after 112 durable pieces.

The fixed checkpoint stage and counter contract now reaches engine snapshots,
the application Disk projection, generated clients and the existing Disk
panel. Hash timing ends at SHA-1; dirty, syncing and committing are distinct,
with separate sync and SQLite service durations.

Deterministic delay, true byte-bound backpressure, forced partial flush, typed
sync/callback failure, supervisor notification and exact task join tests now
pass. The three-case subprocess matrix also proves zero have bits before the
SQLite boundary and exact retained/redownloaded pieces after it.

The complete Tactical `052` gate now passes workspace, generated-contract,
controlled interop, paired publication, Android cross-build, adversarial
delay/failure and crash validation. Its final source-fingerprinted session
cohort has a 46.380-second median versus 50.085 seconds before, with exactly 18
rather than 514 post-metadata revisions and exact payload/publication/cleanup.
A contemporaneous SQLite-independent control ran exact pre-change commit
`e618d2b` at a 36.530-second median and the current binary immediately after
at 36.326 seconds, rejecting an earlier noisy cohort as a persistent
regression. The public comparator is not causal for this slice because its
RSTorrent adapter bypasses the session checkpoint path.

Tactical `053` records the exact positional I/O, part-file ownership and fence
cases from pinned libtorrent plus rqbit and JSTorrent. Its fresh engine-only
baseline is a 35.792-second median with 544--548 physical writes and exact
content/cleanup.

Tactical `053` then completed in commits `a495010` and `b8847fa`. Full-range
positional loops, retained wanted/part handles, immutable no-extra-copy write
plans, per-piece part generations and one positional mixed-file hash job pass
workspace, web, Android cross-build, selective, mixed-source, resume and crash
gates. The exact 128 MiB engine median fell from 35.792 to 33.679 seconds and
write service fell from 30.928--31.979 to 27.131--28.353 seconds. The exact
SQLite-backed median was 45.594 seconds with 17--18 revisions, unchanged
checkpoint semantics and exact payload/publication/cleanup.

Tactical `054`'s retained SQLite-backed application cohort now completes in a
0.534-second median at the selected `4/4` bound, versus a 0.555-second
SQLite-independent engine control. Process evidence identified and removed
per-block synchronous Disk projection while retaining exact checkpoint state,
payload, publication and cleanup.

Its closing selective, mixed-peer, forced-restart, three-boundary crash,
workspace, web, Android and controlled libtorrent gates pass. One authorized
full-reference public Big Buck Bunny pair published exact content after
29.323 seconds for RSTorrent and 36.599 seconds for libtorrent; that changing
swarm remains contextual evidence.

Tactical `073` is the latest engine-correctness restart checkpoint. Commits
`7abea41` through `99a5369` remove the v1 metainfo-mode storage fork, install
all-wanted managed full recheck and force recheck, close path publication
crash windows, and require dynamic platform publication to re-enter the same
piece checker. Its structured local campaign passes exact `length`, one-entry
`files`, and cross-file repair against pinned libtorrent 2.0.13, three
checkpoint deaths, three path-publication deaths with the seed unavailable,
and exact cleanup. The API 34 product path passes provider-rename death and
fresh published-piece recheck at the established 40-handle/one-pending-request
high waters.

Next executable action: follow the single authorized **Now** item in
`capability-readiness.md`. No pending-write read-through, performance, or BEP
breadth slice is implied by Tactical `073` completion.

Human blocker: **none**.
