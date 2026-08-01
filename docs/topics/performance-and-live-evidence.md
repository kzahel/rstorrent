# Performance And Live Evidence

Topic: `performance-and-live-evidence`

Status: Active measurement contract. The catalog-backed paired
RSTorrent/libtorrent comparator emits milestone, geometry, diagnostic,
cleanup, and classification JSON without opening a product surface. Its first
controlled and public full-download baselines are recorded below.
Public-swarm speed remains a measured baseline, not a CI pass threshold.

## Purpose

Real swarms expose discovery, scheduling, timeout, completion, CPU, memory, and
disk behavior that isolated fixtures cannot reproduce completely. They are
valuable smoke evidence even though peer populations and network conditions
vary between runs.

This topic defines how to collect that evidence without confusing a noisy
observation with a correctness claim. It also keeps performance work tied to
end-to-end product outcomes instead of isolated micro-optimization.

[`oracle-driven-engine-campaign.md`](oracle-driven-engine-campaign.md) owns the
active source-first execution runbook, parity gates, and restart checkpoint.
This topic owns the measurement and evidence contract consumed by that
campaign.

## Evidence Roles

Use several complementary layers:

- **Deterministic tests** own codecs, state transitions, bounds, picker rules,
  retry decisions, persistence validation, and hostile inputs.
- **Scripted runtime tests** own sockets, timing, task cancellation, disk
  behavior, process restart, and reproducible failure injection.
- **Controlled interoperability** with the pinned libtorrent version owns
  protocol exchange and completion against an independent implementation.
- **Live public smokes** expose behavior against diverse peers and services.
  They can find failures and establish trends but cannot prove why two runs
  differed without supporting telemetry.

Correct bytes, verified piece hashes, successful publication, and explainable
termination are hard gates. Public download speed and reference ratios are
reported distributions until the harness has enough repeated evidence to set a
stable regression policy.

## Headless Comparator

The comparator is a CLI or application-service harness. It must not launch the
Tauri window, foreground Android UI, Chrome window, or a physical-device flow.
It runs each implementation in an isolated temporary profile and download
directory, captures machine-readable results, and cleans ordinary payload and
runtime artifacts after extracting the report.

The initial catalog is the small set of public test torrents recorded in
[`../test-torrents.md`](../test-torrents.md). Catalog entries identify the
stable info hash, expected name and size when known, enabled discovery modes,
and any license/source provenance. A failed public swarm is never silently
removed from the report.

Runs are sequential by default so the two implementations do not contend for
the same local bandwidth and disk. Repeated comparisons alternate or randomize
implementation order and record the order. Both sides use explicit time,
bandwidth, disk, connection, and storage bounds.

## Comparison Modes

Two general modes answer different questions:

### Common Denominator

Configure libtorrent to use only capabilities RSTorrent currently claims for
the scenario. For the initial tracker baseline this means the same magnet,
tracker set, TCP peer transport, and compatible storage behavior, with DHT,
PEX, LSD, web seeds, and other extra discovery disabled on the reference side.

This mode compares the implementation of shared behavior and avoids attributing
libtorrent's broader feature set to transfer efficiency.

### Full Reference

Run libtorrent with its ordinary production capabilities and RSTorrent with
its currently supported production capabilities. This measures the user-visible
gap, including missing discovery and scheduling breadth. The report must list
the enabled capabilities so the result is not described as an algorithm-only
comparison.

### Trackerless DHT

For the DHT campaign, remove tracker parameters and run the same info hash with
DHT as the peer source. Compare cold and warm session starts. Record time to
first valid DHT response, a defined routing-health threshold, first discovered
peer, metadata availability, first verified piece, and completion.

The exact reference configuration and command line are part of every result.
Reference defaults must not change silently when the pinned libtorrent version
changes.

## DHT Foundation Evidence: 2026-07-31

The controlled `tests/interop/dht_magnet.py` scenario uses an independent
Python KRPC router and libtorrent 2.0.13 peer. An info-hash-only magnet issued
one `find_node` and one `get_peers`, acquired 26,686 bytes of metadata in two
blocks, verified and published three pieces and 40,000 payload bytes, and then
answered independent `ping`, `get_peers`, token, and `announce_peer` probes.
The process-cleanup assertion passed; the recorded run took 0.815 seconds.

The opt-in public bootstrap test first exposed a deployed compatibility issue:
a libtorrent router returned a valid DHT dictionary in noncanonical key order.
RSTorrent's DHT-only bounded parser now tolerates ordering while rejecting
duplicate keys; strict canonical metainfo parsing is unchanged. The corrected
bootstrap reached a BEP 42-valid public node in 0.12 seconds.

A subsequent 120-second trackerless Big Buck Bunny RSTorrent probe built a
16-node routing table, received 830 valid DHT responses, and observed 1,563
peer values, but no contacted peer completed metadata. It was single-sided and
therefore has no paired classification under the table below. It is retained
as an honest historical outcome, not a public completion or speed claim.

Tactical `018` added a coherent peer-registry and BEP 9 acquisition snapshot,
then reran the trackerless smoke twice. The runs discovered 93 and 100 bounded
peer records, attempted 9 and 12 peers, and acquired the hash-verified
21,307-byte info dictionary in two requests and two blocks after 31.2 and 45.9
seconds. The latter DHT traversal retained 8 routing nodes, sent 80 queries,
received 56 valid responses, and discovered 100 peer values. Its 12 attempts
ended as 3 connection refusals, 3 connect timeouts, 3 handshake or extension-
phase resets, 1 verified metadata source, and 2 canceled losing dials. Final
metadata request, dial, worker, transaction, and lookup counts were zero.

A separate 90-second tracker-only rerun discovered no peers and made no dials.
Two trackers timed out, one no longer resolved, and two explicitly rejected
RSTorrent's port-zero announce. After scheduled announces began carrying the
provisional compatibility port `6881`, the same tracker-only smoke received
six candidates within 0.36 seconds and acquired hash-verified metadata in
11.41 seconds. RSTorrent still owns no incoming listener or NAT mapping, so
this is outbound metadata-acquisition evidence rather than inbound
reachability or seeding evidence. Neither single-sided result replaces the
still-pending paired comparator.

An immediate 20-run cohort then exercised the same fixed Big Buck Bunny
magnet ten times with trackers only and ten times with trackers absent and a
fresh public DHT owner. These are unpaired, changing-public-swarm samples with
different timeout bounds:

| Discovery | Completed | Bound | Successful latency min / median / mean / max | Final candidates median / range |
| --- | ---: | ---: | ---: | ---: |
| UDP trackers only | 8/10 | 90 s | 1.71 / 32.77 / 38.41 / 75.51 s | 12.5 / 6–131 |
| DHT only | 7/10 | 120 s | 30.84 / 78.69 / 72.59 / 104.35 s | 88 / 29–120 |

Both tracker failures ended with six attempted and zero eligible candidates,
four metadata requests, and two received 16 KiB blocks totaling 32,768 bytes,
but no verified dictionary. One later tracker success received 37,691 bytes,
consistent with an abandoned full first block plus a complete 21,307-byte
dictionary from another attempt. This is evidence for inspecting per-source
metadata progress and multi-source policy, not proof that aggregate blocks
can safely be combined.

All three DHT failures had successful lookup traffic and peer values: they
ended with 29, 79, and 83 candidates and 23, 35, and 38 dial attempts, but
zero metadata requests. Across all ten DHT runs, 2,759 queries received 2,223
valid responses (80.6%) and produced 906 final candidate records. The failure
boundary is therefore peer connection, selection, or extension negotiation,
not an empty DHT lookup. Every successful DHT run needed exactly two metadata
requests and two blocks once a usable peer was reached.

The same metadata-only metric was then run ten times per discovery mode
through pinned libtorrent `2.0.13.0`. Each run used a fresh temporary session
and storage root; LSD, PEX, UPnP, NAT-PMP, incoming peer connections, and uTP
were disabled. Tracker mode disabled DHT and used the same five UDP URLs; DHT
mode omitted every tracker. Libtorrent otherwise retained its ordinary peer
and metadata scheduler, including substantially more connection concurrency
than RSTorrent's three metadata work slots.

| Implementation and discovery | Completed | Bound | Successful latency min / median / mean / max |
| --- | ---: | ---: | ---: |
| RSTorrent, UDP trackers | 8/10 | 90 s | 1.71 / 32.77 / 38.41 / 75.51 s |
| libtorrent, UDP trackers | 10/10 | 90 s | 20.81 / 20.94 / 21.01 / 21.49 s |
| RSTorrent, DHT | 7/10 | 120 s | 30.84 / 78.69 / 72.59 / 104.35 s |
| libtorrent, DHT | 10/10 | 120 s | 0.75 / 0.90 / 1.08 / 2.72 s |

One isolated-process libtorrent DHT repetition completed in 0.757 seconds,
ruling out reuse of a preceding session's routing table. A tracker alert
timeline explained the reference's stable 21-second result: the first two
trackers each timed out after about 10 seconds, the third returned 71 peers,
and verified metadata arrived 0.11 seconds later. The RSTorrent and libtorrent
cohorts were sequential rather than alternated, so these are a reference
baseline and a large observed gap, not yet Tactical `015`'s paired comparator.

All figures above stop at hash-verified `ut_metadata`. The actual torrent is
276,445,467 payload bytes in 1,055 pieces across three files. Neither cohort
measured full payload verification or publication; a ten-run cohort for both
implementations would transfer about 5.53 GB before protocol overhead.

### First Alternating Comparator Evidence: 2026-07-31

The Tactical `015` runner completed its first common-denominator pair through
the same machine-readable schema. RSTorrent ran first; both owners used the
UDP-only magnet, TCP outgoing connections, no DHT, no LSD, no incoming
connections, and isolated temporary storage. Both acquired the same verified
metadata geometry: 276,445,467 bytes, 1,055 pieces of 262,144 bytes, and three
files. Owned tasks and payload roots were removed.

RSTorrent reached verified metadata in 51.32 seconds and libtorrent in 20.63
seconds, a 2.49x ratio for this one changing-swarm sample. At the RSTorrent
milestone, its bounded snapshot held 128 candidates, 110 still eligible, 20
metadata attempts, two metadata requests, and the two blocks comprising the
21,307-byte info dictionary. This is comparable paired evidence and confirms
the harness contract, but one sample is not a distribution or a parity claim.
It points the next source-first slice at metadata candidate concurrency and
torrent-wide request ownership.

### Torrent-Owned Metadata Result: 2026-07-31

Tactical `019` replaced per-peer dictionaries with one bounded torrent owner
and added endpoint-free per-attempt diagnostics to temporary comparator
reports. Two independent ten-pair Big Buck Bunny tracker cohorts each
completed 9/10 for RSTorrent versus 10/10 for libtorrent. RSTorrent medians
were 5.72 and 4.12 seconds versus 20.52 and 20.33 seconds. Median paired ratios
were 0.28x and 0.20x; p90 ratios were 1.50x and 1.58x. Each RSTorrent miss
retained the first 16 KiB block from six exhausted candidates and had no hash,
cleanup, or resource-bound failure. Tracker metadata therefore meets both the
functional and two-cohort comparable gates.

Three contemporaneous fresh-DHT pairs completed for RSTorrent but timed out
for libtorrent with zero torrent candidates. Giving the reference adapter the
same three bootstrap hosts documented by libtorrent and used by RSTorrent did
not change that result; a separate exact-settings session populated its DHT
routing table, so the retained boundary is live info-hash lookup evidence, not
an empty bootstrap table. The comparator now supports `--owner` to preserve
the same catalog, deadline, diagnostics, identity, cleanup, and JSON contract
without repeatedly running a blocked counterpart. Ten isolated RSTorrent DHT
runs completed 10/10 in 31.40–66.96 seconds, with a 56.64-second median and
59.80-second p90. All had exact identity, zero hash failures, and clean
shutdown. This is functional evidence, not a paired DHT latency claim.

The initial three-pair breadth run completed only 0/3 Cosmos, 2/3 Sintel, 1/3
Tears of Steel, and 2/3 WIRED CD for RSTorrent, while libtorrent completed all
twelve. Every miss retained block zero and repeatedly requested block one from
a capable peer until the 15-second progress deadline. Pinned source showed
libtorrent emits one metadata request per event/tick even though it permits
two outstanding. RSTorrent had filled both slots immediately. After a
deterministic one-request-at-a-time peer and a one-second request ramp were
added, the identical breadth matrix completed 12/12 for each owner. Every
RSTorrent run used exactly two requests and two accepted blocks, with zero
hash and cleanup failures. This before/after result is the causal evidence for
the pacing change.

### First Full-Download Comparator Evidence: 2026-07-31

A controlled loopback fixture first ran both exact adapters against one
libtorrent seed. Both independently verified and published a 79,000-byte,
two-file, three-piece torrent, matched the output file hashes, produced
`both_reached`, and cleaned up. This validates the completion schema and
publication checks; its sub-second timings are not public speed evidence.

The first public full pair used the common-denominator Big Buck Bunny profile
with a 900-second deadline per owner. RSTorrent verified metadata at 16.71
seconds and its first piece at 24.15 seconds. It then timed out at 461 of 1,055
verified pieces and 120,848,384 of 276,445,467 bytes (43.7%), before the 50%
milestone. Its terminal snapshot reported one connected, unchoked peer, four
requests, no writes in flight, 9,491 missing blocks, and
`requestwindowsfull`. An external read-only process sample saw about 15 MiB
RSS and low CPU, so neither unbounded buffering nor a spin explained the wait.

Libtorrent independently reached verified metadata at 20.57 seconds, first
piece at 20.62, 50% at 24.75, 95% at 28.88, 99% at 29.91, and verified
publication at 30.88 seconds. It downloaded 276,445,467 wanted bytes and the
published file geometry matched. The pair is `reference_only`: the swarm was
healthy enough for the constrained reference, while RSTorrent exposed a
sustained connection/request-width gap well before endgame. The earlier user
observation of a 99.9% stall remains relevant to endgame, but it was not the
first limiting boundary in this run.

### Sustained-Transfer Pre-Change Screen: 2026-07-31

Tactical `020` began with three alternating common-denominator Big Buck Bunny
pairs to first verified piece. Both owners completed 3/3. RSTorrent reached
the milestone in 0.74, 75.85, and 0.81 seconds versus libtorrent's 20.83,
20.40, and 20.68 seconds. The slow RSTorrent result spent 75.47 seconds in
tracker and metadata acquisition; the transfer interval from verified
metadata to first piece was only 0.22--0.38 seconds in all three samples.

Every RSTorrent terminal snapshot nevertheless reported one unchoked peer,
four outstanding requests, a 65,536-byte payload high-water mark, and
`requestwindowsfull`. First-piece startup is functional. Combined with the
older 43.7%-after-900-seconds run, the repeatable internal boundary is the
static sustained request window rather than initial piece selection.

The first adaptive-window screen completed 3/3 first-piece pairs and grew the
live RSTorrent request target to 21--46 with 344--754 KiB high-water. The next
three-pair 50% screen classified one `both_reached` and two `reference_only`.
The successful RSTorrent run reached 50% in 28.14 seconds versus 27.98 for
libtorrent (1.006x), verifying 529 pieces under an 8.68 MiB high-water mark.

The two RSTorrent timeouts stopped at 90 and 101 pieces. At 300 seconds each
still had two unchoked peers, 712 or 772 outstanding requests, 11.68 or 12.66
MiB high-water, and no stalled peer despite no further useful completion.
Those peers had delivered about 26 MiB and grown near the 500-request cap.
This is source-aligned evidence for adaptive connection inactivity detection,
not for reducing the now-proven healthy-peer window. All three libtorrent runs
reached 50% in 25.06--27.98 seconds with 16--22 peers at their milestones.

After sampled inactivity landed, a clean RSTorrent-only three-run screen
reached 50% once in 24.09 seconds and timed out twice at 8.5% and 23.1%. The
misses retained only four or nine current content candidates and two
connections; no unused candidate was eligible. The 23.1% run was receiving
about 4.45 MiB/s at termination, reinforcing that useful-peer throughput is no
longer the broad failure owner. Pinned libtorrent tracker startup and connect
boost behavior selects initial peer working-set breadth for Tactical `021`.

Bounded tracker fan-out produced two response batches, 14--15 candidates, and
17--19 dial attempts in every run of the next clean screen. It still reached
50% in 0/3 at 180 seconds, stopping at 36, 135, and 186 pieces. Five or six
connections and two or three pending dials exactly filled the former combined
eight-slot admission check even though two to five candidates remained
eligible. All terminal samples were receiving roughly 3.2--4.0 MiB/s. This is
evidence to separate half-open and established capacity and adopt a bounded
30-peer working set before changing transfer scheduling again.

The clean post-admission screen remained 0/3, at 25, 55, and 96 pieces after
180 seconds. It consumed all 14--15 candidates across five or six established,
six to eight dialing, and up to three backed-off records; none was eligible.
Four or five established peers were unchoked, but 479--710 requests remained
outstanding and the largest peer target reached 360 or 500. Aggregate sampled
rates of 3.0--4.1 MiB/s conflict with only 7.6--26.2 MiB of useful payload over
the full attempts. The headless snapshot therefore gains bounded endpoint-free
per-peer queue, utility, rate, phase, and age rows before another policy
change.

The first clean row-bearing run timed out at 24 pieces, but selected a local
deterministic owner. The final retained table was only one to two seconds old:
a fast peer had delivered 6.24 MiB, grown to a 385-request target, and held
383 requests, after which no newer swarm observation appeared during the
180-second wall clock. The 16-command supervisor channel and 64-event peer
channel can fill in opposite directions. Tactical `022` owns restoring duplex
progress before any rate, queue-target, or picker experiment.

Tactical `022` removed that cycle. Its clean owner-only 50% screen completed
3/3 at 34.70--55.37 seconds with current terminal peer tables and exact
cleanup. The alternating screen also completed 3/3 for both owners: RSTorrent
took 30.74, 34.14, and 45.82 seconds versus libtorrent's 24.00, 25.80, and
24.82, producing 1.28x--1.85x paired ratios. This passes the bounded tactical
screen but is not yet the campaign's two-cohort comparable confirmation.
Tactical `023` then reached verified publication in one clean owner-only run
at 72.66 seconds and all three alternating pairs. RSTorrent took 80.22, 82.53,
and 123.18 seconds versus libtorrent's 29.80, 29.93, and 30.32 seconds. The
2.76x median and 4.06x maximum paired ratios miss the comparable gate, while
exact integrity, publication, cleanup, and bounded endgame counters pass. This
is retained performance debt.

Tactical `024` then completed bounded piece-hash recovery without changing
transfer policy. Its clean public health screen published all 276,445,467
bytes and 1,055 pieces in 86.05 seconds, with metadata at 3.27 seconds, first
piece at 4.49, 50% at 40.18, and zero content hash failures. Exact integrity,
publication, task cleanup, and a 13,516,800-byte payload high water passed.

A controlled 32 MiB single-piece RSTorrent download from a local libtorrent
seed took 3.829 seconds, about 8.4 MiB/s. Source inspection found the content
supervisor awaits each 16 KiB physical write and every piece reread/hash before
consuming another peer event, whereas pinned libtorrent transfers accepted
blocks to a bounded asynchronous disk owner and releases peer progress from
write completion. Tactical `025` tests this owner with three-run before/after
evidence before any peer-window, picker, or discovery tuning.

## Result Classification

Classify each paired attempt before interpreting speed:

| Libtorrent | RSTorrent | Classification |
| --- | --- | --- |
| Completes | Completes | Comparable; report correctness and performance metrics. |
| Completes | Fails or times out | Actionable RSTorrent gap; retain diagnostics. |
| Fails or times out | Fails or times out | Inconclusive public-swarm attempt. |
| Fails or times out | Completes | Record RSTorrent success; reference comparison is inconclusive. |

A timeout is not automatically a product defect or a success. The report must
retain the last progress, active discovery state, peer counts, request state,
and terminal reason needed to distinguish no peers, slow peers, a scheduler
stall, integrity recovery, and harness failure.

## Required Measurements

Capture the following when the owner exists:

- repository commit, dirty state, platform, architecture, toolchain, wall
  clock, implementation order, catalog version, and exact configuration;
- torrent identity, expected and verified bytes, completion and publication
  result, final hash/integrity status, and terminal reason;
- time to first discovery response, peer, metadata, requested block, verified
  piece, 50%, 95%, 99%, and 100%;
- discovery-source counts, connection attempts, established peers, useful
  peers, and disconnect/failure reasons;
- useful payload bytes, duplicate bytes, rejected/late bytes, hash failures,
  and retransmitted or reassigned requests;
- aggregate and interval throughput, while avoiding precision unsupported by
  the sampling period;
- process peak RSS and CPU time or utilization;
- storage bytes, disk-write volume when available, and internal buffer/queue
  high-water marks; and
- DHT routing, transaction, lookup, and saved-state high-water marks for DHT
  scenarios.

Missing metrics are emitted as unavailable, not zero. Adding a metric follows
the owner that can report it accurately; the harness must not infer internal
facts from UI text.

## Performance Policy

Optimize only after correctness and ownership are visible. A proposed
optimization should identify:

1. the end-to-end symptom or measured resource cost;
2. the owner and hot path responsible;
3. the baseline workload and metric;
4. the correctness and resource invariants that must remain true; and
5. before/after controlled results plus representative live observations when
   useful.

CPU, memory, disk, allocation, wakeup, connection, and queue costs all matter.
Throughput alone is insufficient if it depends on unbounded buffering or work.
Do not add speculative abstraction, caching, unsafe code, or concurrency for an
unmeasured gain.

Initial public results are not required to beat libtorrent. The useful signal
is whether RSTorrent completes correctly, where time is spent, whether resource
use remains bounded, and whether repeated changes improve or regress the same
scenario. Stable hard performance gates may be introduced only after enough
controlled samples establish a defensible threshold.

## Execution And Artifact Safety

- Live network tests are opt-in and never part of the default unit-test run.
- Runs use explicit maximum duration, payload size, connections, bandwidth,
  disk space, and retained-log limits.
- The harness must be independently stoppable and leave no engine tasks or
  listeners behind.
- Ordinary runs use background processes only. Visible desktop, browser,
  emulator, and physical-device automation require a surface-specific reason
  and the authority already required by repository instructions.
- Downloaded payload, temporary profiles, databases, packet captures, and raw
  logs stay in ignored or temporary paths and are removed after the report is
  extracted unless the user explicitly asks to retain them.
- Committed summaries contain no peer IP addresses, tokens, machine-specific
  paths, or large binary artifacts.

## Comparator Outcome

Tactical `015` added the smallest harness that can:

1. select a catalog entry and comparison mode;
2. run a bounded libtorrent reference download and a bounded RSTorrent
   download headlessly in isolated temporary directories;
3. validate identity, verified completion, and output size;
4. emit the paired classification and core timing/resource metadata as JSON;
5. retain bounded diagnostic evidence on an actionable mismatch; and
6. exercise one available tracker-based catalog entry.

Selective pinned-reference checkout tooling and the single-sided DHT harness
landed as prerequisites. Deterministic catalog, classification, threshold,
order, and summary tests, a controlled full comparison, the first paired
metadata run, and the first bounded public full pair now pass or produce an
honest classified result. The comparator does not add product UI, establish a
CI speed gate, or change DHT scheduling itself.

## Maintenance Contract

Feature tacticals add measurements only when their owner can report them
honestly. DHT, multi-peer, endgame, picker, storage, and performance work update
the relevant scenario definitions and append summarized evidence without
turning this topic into a raw run log.

When results motivate a queue change, update
[`capability-readiness.md`](capability-readiness.md) and the relevant focused
topic. Public-swarm evidence never upgrades a protocol claim by itself.
