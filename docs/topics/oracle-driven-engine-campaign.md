# Oracle-Driven Engine Campaign

Topic: `oracle-driven-engine-campaign`

Status: Active. The campaign is completing the paired headless comparator,
then driving metadata, first-piece, sustained-transfer, endgame, and full
publication parity from pinned libtorrent source and tests. High-impact BEP
breadth follows the core common-denominator parity gate.

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

Campaign state: **active**.

Active tactical: the next numbered first-piece and sustained-transfer parity
slice. Tactical
[`019-torrent-owned-metadata-acquisition.md`](../tactical/019-torrent-owned-metadata-acquisition.md)
is complete.

Current milestone: use the completed metadata path and pinned libtorrent peer,
picker, and request owners to reach first-piece, 50%, and publication parity.

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
  requests, two blocks, zero hash failures, and clean shutdown.

Next executable action:

1. open Tactical `020` for first-piece and sustained-transfer parity;
2. survey pinned `peer_connection`, `request_blocks`, `piece_picker`, and
   related tests around request width, peer replacement, snubbing, and
   availability;
3. turn the 43.7%-after-900-seconds Big Buck Bunny snapshot into deterministic
   request-window and stalled-peer hypotheses before changing policy; and
4. screen first piece and 50% before the next full publication cohort.

Human blocker: **none**.
