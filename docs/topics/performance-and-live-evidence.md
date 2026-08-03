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

## Browser Adapter Throughput Evidence: 2026-08-03

Tactical `060` adds
`tests/interop/application_transport_throughput.py`, an opt-in paired browser
adapter smoke. It generates one deterministic multi-file v1 torrent with a
large payload, keeps one pinned libtorrent 2.0.13 seeder, starts a clean
gateway/profile for each adapter, and drives the production web build through
headless Playwright. Each case must publish the exact payload SHA-1 and cleanly
join before the next begins. The report records hardware, run order, semantic
HTTP requests, WebSocket upgrades, gateway frame/byte counters and queue/
latency high waters. It deliberately defines no pass floor from a single
cohort.

On `kmacbook` (Apple M4 Pro, 14 logical CPUs, 48 GiB) the retained HTTP-then-
WebSocket run used a 1 GiB payload, 1 MiB pieces and warm uncontrolled OS page
cache. HTTP completed in 3.348 seconds at 305.9 MiB/s and made 38 semantic API
requests. WebSocket completed in 2.815 seconds at 363.8 MiB/s, a 1.189
throughput ratio, with exactly one application upgrade and zero semantic HTTP
requests. It carried 24 view batches and acknowledgements, 20,102 encoded
view-batch bytes, an 846-byte largest outbound message and 62 microseconds
maximum enqueue-to-send latency. Both outputs matched SHA-1
`5c60b5002d23d55a1f6ca8498c4fa16d40e163bd`; both gateways and payload roots
cleaned up.

The ordinary controlled browser transfer separately crossed Transfers and
Workbench, observed Files and Peers, and published its 122-file fixture using
one WebSocket and no semantic HTTP requests. The adversarial 4,096-file
snapshot encoded to 1,426,924 bytes while an adjacent command completed in
11,835 microseconds. These are direct loopback observations, not remote
latency or codec claims.

The reproducible full command is:

```bash
source ~/.profile
uv run --project tests/interop \
  tests/interop/application_transport_throughput.py \
  --size-mib 1024 --piece-size-kib 1024 --timeout-seconds 240 \
  --output /tmp/rstorrent-application-transport-1g.json
```

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

### Bounded Storage Result: 2026-08-01

Tactical `025` corrected that large-piece harness to time only the RSTorrent
subprocess. The prior 3.829-second total included Python payload generation,
torrent construction, and seed startup and therefore was not an engine
ceiling. Three transfer-only runs of the synchronous commit took 0.646, 0.326,
and 0.331 seconds (0.331-second median); the bounded asynchronous owner took
0.745, 0.426, and 0.426 seconds (0.426-second median). The 29% regression
rejects storage as a speed improvement in this localhost profile.

The owner is retained for a separate causal result: with a 250 ms first-write
delay, two peers deliver their complete payloads to the supervisor within 100
ms while the write remains in flight. An 80-block saturation case reached the
declared 64-command and 66-job high waters, completed exact content, and
returned every queue to zero. Cancellation during queued writes and hashing
joins the owner exactly. This establishes peer-event liveness and bounded byte
ownership without claiming throughput.

The controlled paired publication remained exact at 44.66 ms for RSTorrent
and 86.57 ms for pinned libtorrent. A clean common-denominator Big Buck Bunny
owner screen published all 276,445,467 bytes in 153.72 seconds with zero hash,
publication, cleanup, or bound failures. Its storage command, completion, job,
and payload high waters were 64, 1, 66, and 8,781,824 bytes. The result is
slower than the prior 86.05-second public health screen, but one changed swarm
cannot attribute that delta. It retains completion evidence and selects a
time-series peer working-set comparison rather than further storage tuning.

### Product Pipeline Budget Correction: 2026-08-01

Tactical `039` found that the product application still used a 32 KiB
allowance inherited from the original controlled harness. Outstanding peer
requests retained that same allowance through storage completion, so the
entire desktop torrent could reserve only two ordinary 16 KiB blocks. Earlier
live campaign commands commonly supplied larger explicit allowances--their
8.8--13.5 MiB high waters therefore did not validate the product default.

Pinned libtorrent keeps its four-request initial peer window, three-second
adaptive target, 500-request per-peer maximum, and received disk queue as
separate controls. RSTorrent now follows that resource separation with explicit
product profiles: desktop permits 256 MiB of outstanding requests, 32 MiB of
received resident payload, and a 256 MiB active-piece working set; Android uses
128 MiB, 16 MiB, and 128 MiB respectively. The storage job bound is derived
from its resident byte allowance instead of the historical fixed 64-command
and 66-job limits.

Deterministic and delayed-storage tests prove that all 30 default peers can
receive four initial requests, request reservations release when accepted
payload changes owner, the active-piece byte set refills, and resident payload
remains independently bounded. This is a correctness and capacity correction,
not a new public-swarm throughput or parity claim. Historical measurements
below retain the resource model of the commits on which they were captured.

### Paired Utility Timeline: 2026-08-01

Tactical `026` added a one-second, 1,024-sample bounded endpoint-free timeline
to both comparator owners. Exact milestone times remain separate; older
interior samples are deterministically coalesced if the bound is reached.
Fields cover candidate, connection, request, payload, verification, and
storage/disk aggregates with explicit `null` where an owner cannot expose an
equivalent meaning. The controlled paired publication and deterministic Rust
and Python aggregation, rate, bound, and scrubbing tests pass.

Three alternating common-profile Big Buck Bunny full pairs reached verified
publication and clean shutdown for both owners. RSTorrent took 132.89, 134.43,
and 138.24 seconds; libtorrent took 30.87, 30.89, and 31.11 seconds. The median
paired ratio is 4.35x, so functional completion remains green but comparable
latency remains open.

Libtorrent spent about 20.3--20.6 seconds reaching metadata and transferred
content in only 10.3--10.6 seconds. Three to five content seconds after
metadata, RSTorrent knew 10--16 peers, had three or four connections and two
useful peers, and received roughly 2.2--2.7 MB/s. Libtorrent knew 60--65 peers,
had 17--20 connections and 11--14 useful peers, and ramped to roughly 12--29
MB/s. RSTorrent had no idle eligible candidates in that interval, selecting
candidate supply before ranking or request policy for the isolated tracker
profile.

The product tracker+DHT path supplied a different state. One source-timed 50%
screen had 159 candidates by metadata; after one content second, 119 remained
eligible, eight were dialing, and six were connected. DHT supplied another
148 observations around content second 30, mostly merging existing tracker
records. RSTorrent held exactly eight half-open attempts while growing to 29
connections over about 100 seconds, then reached 50% at 143.94 seconds with 30
connections and 92 candidates still eligible. This selects Tactical `027`'s
source-derived 30-attempt startup cohort. Continuously saturated 64-command
storage and 66-job high waters remain a measured secondary hypothesis if
broader admission does not improve service.

### Expanded Cohort Result: 2026-08-01

Tactical `027` changed only the pending-dial default from eight to 30. A useful
peer at position 30 behind 29 silent handshakes completed a controlled transfer
in about 50 ms; 30 silent attempts also canceled and joined exactly. Three
product-path 50% screens completed in 61.47--68.34 seconds, but supplied only
12--16 candidates and therefore do not attribute their 64.21-second median to
the wider cohort.

One product-path completion published all 276,445,467 bytes in 149.42 seconds
with zero hash failures and drained request and storage queues. Its timeline
reported 171 cumulative DHT peers near content second 30 and 340 near second
120, while the content registry stayed at 12 known peers until the final
sample. Storage remained at its 66-job high water throughout. The supervisor's
storage-backpressured branch receives only storage completions, and its
ordinary biased selection also places storage before peer and discovery
events. Tactical `028` therefore owns prompt bounded discovery admission and
explicit safe event-owner fairness before further storage or request tuning.

### Fair Intake Result: 2026-08-01

Tactical `028` rotates safe ready storage, peer, and discovery owners and
starts bounded dials independently from storage-command readiness. A scripted
case admits and dials a delayed DHT peer while all 66 storage jobs remain
occupied, then publishes exact content and drains every queue.

Three live 50% screens put each first DHT result and registry increase in the
same sample. A 149-peer DHT result produced 302 known peers; 145 and 148-peer
results produced 157 and 161. All immediately reached 30 pending attempts, so
the delayed-intake defect is closed. Two screens reached 50% in 69.15 and
69.29 seconds; a source-rich screen took 282.74 seconds. One full screen timed
out at 300 seconds with 399 pieces and 104,595,456 verified bytes, zero hash
failures, 30 connections, 91 requests, 65 writes, and 66 storage jobs.

Tactical `029` now maps each piece once and reduces a common 256 KiB
multi-file hash from 16 seeks to one while retaining 16 fixed 16 KiB reads.
Its representative 32 MiB controlled medians were 1.101 seconds before and
1.121 seconds after, so no speed improvement is claimed. Three public 50%
screens still reached 66 occupied storage jobs; two reached the milestone at
77.76 and 77.89 seconds, while one timed out at 506 of 1,055 pieces. A full
screen published all 276,445,467 exact bytes at 180.64 seconds with zero hash
failures and drained queues.

Tactical `030` now executes a complete all-wanted piece hash in one bounded
blocking positional-I/O job. The 32 MiB controlled median remained neutral at
1.139 seconds versus Tactical `029`'s 1.121. Two public 50% samples took 79.47
and 223.85 seconds; a third timed out at 359 of 1,055 pieces. A complete screen
timed out at 375 pieces and 98,304,000 verified bytes. All terminal snapshots
retained 66 storage jobs, all had zero hash failures, and cleanup was exact.

Full queue occupancy has now survived two hash operation-shape changes without
a controlled speed improvement. It was not sufficient attribution. Tactical
`031` now records command queue wait, complete write service, complete hash
service, and the active operation age without changing scheduling.

### Storage Duration Attribution: 2026-08-01

Controlled write/hash delays, queued cancellation, saturation, nullable
libtorrent schema fields, and exact active-operation cleanup all pass. The
paired controlled publication remained exact. Both Android targets also pass
compilation checks through the shared engine path.

Three tracker+DHT Big Buck Bunny screens targeted 50% with a 300-second limit.
One reached 528 of 1,055 pieces in 82.48 seconds; two ended at 425 and 458
pieces. All retained zero hash failures, exact cleanup, and no active storage
operation at termination. Write service consumed 87.7%, 88.2%, and 87.7% of
wall time. Hash service consumed 5.5%, 5.5%, and 5.7%, making combined
serialized service 93.2%, 93.7%, and 93.3%.

Average 16 KiB writes took 8.5, 38.3, and 35.1 ms, with maxima of 272.5,
842.0, and 697.4 ms. The queue reached its 66-job bound and individual waits
reached 0.95--8.16 seconds. Summed wait is overlapping backlog evidence and is
not compared directly with wall time. Serialized service time is comparable
and selects write execution shape or bounded concurrency as the next owner;
hashing, peer ranking, and request policy remain unchanged until that owner is
measured.

## Application View Delivery Costs

The live inspection direction adds a second performance surface: engine facts
must become bounded application projections, cross the selected codec, reduce
into a local store, and render without distorting the engine being observed.
Tactical `034` measured deterministic frontend rendering only. Its 2,000
torrent / 10,000 peer scenario did not measure Rust projection work, view-hub
locking, JSON accounting/encoding, gateway delivery, or live reducer volume.

Current source inspection records these costs as **observed in code but
unmeasured**, not as proven bottlenecks:

- individual activity and progress updates clone the complete `ViewHub`
  torrent `BTreeMap` while holding its central mutex;
- torrent models copy nested strings, verified ranges, and active-piece
  vectors during broad comparisons;
- both legacy subscriptions and view sets serialize values to JSON to account
  queue bytes before the transport encodes a response;
- exact replay intentionally retains and clones one emitted unacknowledged
  batch under the declared bound;
- the TypeScript view-set reducer clones its keyed view record per batch and
  rebuilds torrent-list arrays; and
- the inspection reducer clones the selected torrent's peer-row record when a
  peer patch changes it; and
- diagnostics are independently retained in the view reducer and mapped
  inspection store, rebuild bounded arrays on append, and currently discard
  structured context only after paying its decode/retention cost.

Tactical `035` removes the full torrent-map clone from targeted high-frequency
publication before adding live peer cadence, while preserving coherent
snapshots and legacy consumers. Its deterministic source-rich snapshot uses
30 connecting plus 30 connected rows. One representative optimized run spent
479 microseconds publishing the projection and 501 microseconds opening and
materializing the selected snapshot; its encoded update and queue high-water
were both 79,230 bytes beneath the 256 KiB default, with zero resets. These
single-machine timings are observations, not regression thresholds.

Tactical `049` measures the diagnostic path while unifying its frontend record
shape, raising a deliberately bounded console history, batching ordered append
delivery, and virtualizing visible entries. Its results are recorded below;
they do not justify additional indexing, binary encoding, or frame-coalesced
rendering.

The unchanged Tactical `034` 2,000-torrent/10,000-peer scenario remains the
frontend pressure evidence: at most 100 rendered rows, 840 DOM elements,
30,727,035 bytes of sampled used JavaScript heap, a 247 ms initial render, a
50 ms simulated update and paint, and zero observed long tasks. Tactical `035`
adds controlled live evidence rather than another scale measurement: the
production web build observed one libtorrent 2.0.13.0 connection, pending
requests, verified three-piece completion, and row cleanup in 27.7 seconds.

The same controlled run withheld browser client operations past an explicit
500 ms test lease. It observed the stale/reconnecting presentation, a distinct
replacement view-set identity and epoch, coherent fresh-snapshot recovery,
and joined gateway cleanup. The normal five-minute lease and single reaper
task remain unchanged. More detailed lock-hold, allocation, selector-
notification, and live multi-peer browser profiles are still unmeasured and
must not be inferred from these bounded results.

Replay retention remains a deliberate bounded correctness cost. Field masks,
entity sharding, mutable versioned containers, streaming, animation-frame
delivery, and binary codecs remain later measured responses rather than
assumed fixes.

### Diagnostic Console Evidence: 2026-08-02

Tactical `049` attempted 10,000 deterministic diagnostic records through both
source and browser pressure fixtures. The Rust ring retained the newest 2,048,
reported 7,952 source evictions, and occupied 733,185 encoded bytes, below its
2 MiB ceiling. The Zustand projection independently retained 2,048 and
reported 7,952 local evictions. The wide browser viewport mounted 500 DOM
elements including 24 virtualized records and sampled 26,019,809 bytes of used
JavaScript heap. Deterministic browser checks retain ceilings of 1,500 DOM
elements, 60 rendered records, and 256 MiB sampled heap. These are bounded
development observations, not whole-product memory guarantees.

The controlled libtorrent 2.0.13.0 run observed a real successful UDP tracker
record, expanded its exact URL and announce interval, and changed producer
capture and display-category controls while the verified transfer continued.
The 122-file, three-piece payload showed first Done at 9,036 ms and first
Verified at 9,041 ms, and the browser proof completed in 34.1 seconds with
joined cleanup. Its first invocation hit the already-recorded one-shot UDP
tracker timing transient; the unchanged rerun passed. This proves bounded live
inspection and structured context, not public-swarm throughput.

### File Projection Evidence: 2026-08-01

Tactical `041` adds a larger but still bounded application-view shape. The
4,096-row long-path Rust fixture encoded to 1,481,877 bytes. Its coherent
snapshot is retained under a separate 16 MiB ceiling instead of raising the
512 KiB steady-state patch queue. Static Rust catalog geometry is shared by
`Arc`; stored, verified, and failed piece transitions derive only intersected
complete rows. Piece checkpoints now publish a targeted durable update rather
than reconstructing the complete SQLite-backed service snapshot on every
verified piece.

The deterministic browser scenario retained 4,096 semantic rows, hid one
padding row, and mounted 690 DOM elements. Headless Chrome sampled 66,468,705
bytes of JavaScript heap; a complete ten-second scenario rebuild and paint
took 55 ms. The table remained keyboard-resizable, correctly sorted exact
decimal values beyond JavaScript's safe integer range, and serious/critical
axe findings were empty. These values are observations from one development
run, not hard regression thresholds.

The live adapter caches the mapped file catalog by projection identity, so a
high-frequency torrent-summary patch does not remap 4,096 file DTOs. A Files
patch maps only source rows whose identity changed. Two bounded shallow O(n)
costs remain: the generic TypeScript view-set reducer rebuilds the generated
file DTO array for a keyed patch, and the normalized inspection record copies
its key table when one or more rows change. At the current 4,096-file ceiling
and 250 ms minimum Files delivery interval this is acceptable initial
evidence, not proof that entity sharding is unnecessary. Retain this item for
profiling before raising file-count or update-cadence limits.

The controlled libtorrent 2.0.13.0 proof transferred 26,731 bytes of
multi-block metadata, a 7,000-byte nested prefix, and a 40,000-byte payload
across 122 files and three pieces. Piece zero crossed the two nonempty files.
From selecting Files, first Done appeared at 20,406 ms and first Verified at
20,413 ms. A deliberate 500 ms lease expiry reopened from a fresh snapshot
while transfer continued; final payload display was 39.0 KiB Done and
Verified, both external content comparisons matched, and every child joined
and cleaned up. The seed's 4 KiB/s upload limit makes these interoperability
milestones diagnostic rather than product throughput measurements.

### Tracker Projection Evidence: 2026-08-01

Tactical `043` adds at most 32 small complete tracker rows at a 250 ms minimum
selected-view cadence. One delivered monotonic deadline is mapped to a local
wall-clock target, so the UI's one-second countdown does not create backend
updates, queue traffic, or JSON encoding work. Same-catalog piece checkpoints
preserve the tracker projection instead of rebuilding it from durable state.

The deterministic tracker-recovery browser suite passed at wide, compact, and
phone viewports. Its scale scenario retained 855 DOM elements, sampled about
32.7 MiB of JavaScript heap, rendered initially in 258 ms, and applied its
simulated update in 68 ms; one 50 ms long task was observed. These are
single-machine smoke observations, not thresholds. The responsive proof also
caught and closed a phone-width overflow that exposed a sliver of the inactive
library pane.

The controlled live browser proof delayed its owned UDP tracker response for
three seconds so the UI could observe `announcing`, then displayed the exact
response of one peer, 37 seeds, 11 leeches, and a 30-minute reannounce
deadline. It completed and hash-verified the same 40,000-byte payload. This
run validates state delivery and presentation; its seeded transfer timing is
not a throughput comparison.

### Disk Projection Evidence: 2026-08-02

Tactical `044` adds a global session projection composed from bounded
torrent-owner snapshots. Engine emission may occur at block transitions, but
view-set delivery coalesces semantic replacements at the requested cadence and
the browser receives piece-attempt rows rather than 16 KiB jobs or payload.
The generic Rust projection currently rebuilds the small active-piece vector
and the TypeScript reducer rebuilds its keyed array on a patch. Those costs are
bounded by active working-set limits but remain unprofiled for future
concurrent multi-torrent scheduling; do not infer that the current one-active-
download application proves aggregate scalability.

The deterministic `slow-disk-pressure` fixture retained 64 active piece rows,
mounted fewer than 100 virtual grid rows, and passed serious/critical axe
checks at wide, compact, and phone sizes. A controlled production-web proof
used a libtorrent `2.0.13.0` loopback seed, a 4 MiB payload plus a 7,000-byte
prefix, 17 pieces, a 128 KiB resident cap, and a 150 ms test-only write delay.
It observed pressure exactly at 96 KiB, paused intake, then exact verified
completion, idle recovery, empty active rows, matching external SHA-1, and
joined cleanup. This is backpressure and inspection evidence, not a disk
throughput benchmark.

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

## Bounded Write-Batch Evidence: 2026-08-01

Tactical `032` separates logical accepted blocks from physical write batches.
It drains no more than 16 already-admitted blocks or 256 KiB without waiting,
coalesces exact adjacent same-piece ranges, and retains per-block completions
and verification fences.

Three controlled 32 MiB selective-hash runs published exact content and
cleaned up at 1.354, 1.143, and 1.124 seconds. Their 1.143-second median is a
neutral 4.4% improvement over the preceding 1.196-second measurement, below
the predeclared 20% claim threshold. The shape did change materially: every
run reduced 2,048 logical blocks to 144--154 physical writes, reached the
16-block/256 KiB caps, and spent 0.232--0.331 seconds in write service.

The retained three-pair product tracker+DHT Big Buck Bunny cohort used a newly
built probe and 300-second owner limits. All reference owners reached 50% in
27.26--29.94 seconds. All RSTorrent owners timed out at 345--351 of 1,055
pieces with zero piece-hash failures and successful cleanup. Each reduced
5,648--5,740 logical blocks to 500--509 physical writes. Write service consumed
51.4--54.9% and hash service 39.1--41.6% of wall time, leaving combined
serialized storage service at 93.0--94.2%. This is a structural batching claim,
not a latency, functional-parity, or protocol claim.

[`storage-throughput-architecture.md`](storage-throughput-architecture.md)
now records the source-derived end-state needed to test the next causal slices:
batched durability outside hash service, immutable positional spans,
independent bounded write/hash queues, optional pending-write read-through,
and eventual session/root fairness. The proposal does not convert this evidence
into an implementation or speed claim.

## Batched-Checkpoint Controlled Evidence: 2026-08-02

Tactical `052` adds two retained 128 MiB loopback profiles before changing
behavior. The three-file engine-only profile uses 512 256 KiB pieces and
8,192 blocks and completed three exact runs in 36.564--38.896 seconds, with a
37.594-second median. Its serialized write service consumed
31.108--34.088 seconds while 8,192 logical blocks became 542--546 physical
writes. It retains the current execution baseline independently from SQLite.

The application-service profile runs a separate 512-piece multi-file torrent
through the path-backed session and `synchronous=FULL` SQLite checkpoint sink.
The exact `e618d2b` pre-change executable completed three runs in
50.019--50.301 seconds, with a 50.085-second median and exactly 514
post-metadata revisions every time: 512 per-piece have transactions plus two
final state transitions. Its SHA-256 fingerprint was
`323722b2e925ffc9e7844a624af5d8f1fe2601dda59d61983a8c264b97bb28c6`.
An earlier 12.707--13.370-second observation came from a stale binary and is
explicitly rejected.

After the joined checkpoint owner removed per-piece payload sync and batched
have persistence, the source-matched executable completed three exact runs in
44.580--45.282 seconds, with a 45.221-second median and only 16--18
post-metadata revisions. That is a 9.7% controlled median reduction and a
28.6--32.1x transaction-amplification reduction. Payload hashes, raw info,
complete have geometry, publication and cleanup matched in all six retained
runs. Latency remains a single-machine development observation rather than a
product threshold; positional and concurrent storage work is still pending.

The final graduation cohort used current executable SHA-256
`b0aa5215db00ee32243a241c12f50bc68f0b1942fba88a4326958d70eb04de63`.
It completed at 45.740, 46.735 and 46.380 seconds, for a 46.380-second median
and exactly 18 post-metadata revisions every time. This remains 7.4% below the
exact pre-change median with a 28.6x revision reduction. All three runs matched
the 512-piece have geometry, exact payload SHA-1, raw info, publication and
cleanup.

The contemporaneous engine-only control also records an intentionally rejected
noisy cohort. Current code first measured a 41.959-second median, but an
isolated rebuild of exact pre-change commit `e618d2b` then measured 36.530
seconds and the current binary immediately followed at 36.326 seconds. Exact
content and batch geometry held, so there is no persistent raw write/hash
regression attributable to the checkpoint split. The public comparator is not
causal for this tactical because its RSTorrent owner is the non-resumable
engine probe and never instantiates the SQLite checkpoint path; public
full-download comparison resumes when later slices change that shared hot
path.

## Immutable Positional-Plan Evidence: 2026-08-02

Tactical `053` replaced ordinary single-file, wanted-file and part-file
payload writes plus every piece-hash read with immutable positional plans over
retained handles. It deliberately retained one executing operation, so this is
a foundation and service-cost result rather than a worker-concurrency claim.

The final 128 MiB engine cohort used executable SHA-256
`d6244ffca595fe57251a45254ba7ed7b3a74d7f500cd184629fd312b873ae8ea`.
It completed at 33.679, 34.134 and 33.279 seconds, for a 33.679-second median,
5.9% below the exact 35.792-second pre-change median. All 8,192 blocks reached
the 16-block/256 KiB batch caps, all three file hashes matched and cleanup was
exact. Physical calls varied from 562 to 566 versus 544--548 before, while
serialized write service fell from 30.928--31.979 seconds to
27.131--28.353 seconds. The service reduction, rather than call count alone,
is the retained causal observation.

The SQLite-backed application cohort used executable SHA-256
`42247bbcdfc91bac4964eb975d7b0a038f89203228616589fe44860d23ab5594`.
Its 46.069, 45.594 and 45.590-second transfers produced a 45.594-second
median, 1.7% below Tactical `052`'s final 46.380-second median. Every run
retained exact 512-piece state, payload SHA-1
`9224038c2041d03f6f8eb46a7f618fc32cf34e67`, publication and cleanup, with
17--18 post-metadata revisions. Checkpoint policy and transaction shape did
not change.

Three selective-file runs additionally proved the wanted/skipped/padding
join, part-file reopen and materialization with exact piece/file hashes. The
mixed-source and controlled paired-publication cases passed, three forced
restarts retained conservative have state, and all pre-sync,
post-sync/pre-commit and post-commit crash outcomes remained one-sided. The
next profile-changing slice is bounded independent write/hash execution; it
must retain these exact fingerprints, integrity outcomes and cleanup bounds.

## Large Local Throughput Baseline: 2026-08-02

Tactical `054` adds
`tests/interop/local_throughput_compare.py` as the retained large-transfer
screen. It uses one pinned libtorrent `2.0.13` loopback seeder, a materialized
deterministic single-file source, one direct peer, no discovery, alternating
client order (rotating when several RSTorrent points are present), immediate
per-output cleanup and exact whole-file SHA-1. Transfer
time excludes fixture construction, torrent hashing and final validation.

Schema `2` can run multiple RSTorrent `WRITE/HASH` concurrency points against
one libtorrent observation per workload while rotating order across
repetitions. Each raw result carries its point; the report emits medians,
RSTorrent/libtorrent ratios and an optional executable minimum-throughput or
minimum-ratio gate. Tactical `054` treats this simple large-transfer matrix as
the first optimization gate. The current-machine graduation command uses
three repetitions and a 170.667 MiB/s RSTorrent floor, which is 10 GiB in 60
seconds; exact bytes, SHA-1, publication and cleanup remain unconditional.

The first 1 GiB/256 KiB RSTorrent run failed to finish in more than four
minutes because full swarm geometry was recomputed after each event. Two later
10 GiB process samples exposed per-piece whole-block contributor scans and
active-piece scans that included pieces with no missing request. Checked
incremental phase/request/contributor indexes, a 100 ms maintenance snapshot
cadence and a requestable-active-piece index removed those costs. The
10 GiB/256 KiB row moved from 119.525 seconds immediately before the final
index to 30.042 seconds after it.

The final one-run-per-point screen used executable SHA-256
`1ac603546048301173505dc784b77a073379878bb6642c339ab240f3d95fa097`:

| Size | Piece | RSTorrent MiB/s | libtorrent MiB/s | RST/libtorrent |
| --- | ---: | ---: | ---: | ---: |
| 1 GiB | 256 KiB | 479.5 | 477.9 | 100.3% |
| 1 GiB | 1 MiB | 638.5 | 485.9 | 131.4% |
| 1 GiB | 4 MiB | 609.5 | 504.5 | 120.8% |
| 1 GiB | 16 MiB | 342.3 | 484.4 | 70.7% |
| 10 GiB | 256 KiB | 340.9 | 362.6 | 94.0% |
| 10 GiB | 1 MiB | 490.1 | 520.6 | 94.1% |
| 10 GiB | 4 MiB | 597.0 | 959.0 | 62.3% |
| 10 GiB | 16 MiB | 288.9 | 948.4 | 30.5% |

Every one of the 16 transfers reported exact bytes, zero failed/redundant
payload, the expected full-file SHA-1, complete publication and successful
cleanup. All RSTorrent 10 GiB rows finished in 17.151--35.451 seconds, meeting
the maintainer's informal under-one-minute screen. This cohort used a warm,
uncontrolled operating-system page cache and only one run per point, so it is
a baseline and causal scaling result rather than a stable parity or hardware
ceiling claim. The larger-piece write-service gap remains open for Tactical
`054`'s raw-stage and concurrency sweeps.

The first common-fixture concurrency discriminator used 10 GiB/4 MiB, three
runs and `4/4` versus `8/4`. RSTorrent medians were 23.631 seconds (433.3
MiB/s) and 16.136 seconds (634.6 MiB/s), while libtorrent's median was 9.627
seconds (1,063.6 MiB/s). The six RSTorrent samples spanned 15.477--27.850
seconds despite the common fixture. The candidate therefore remains `8/4`,
but the result also demonstrates why isolated rows cannot select a default;
the full rotating matrix and repeated finalist cohort remain required.

The retained raw comparator now consists of the
`rstorrent-storage-stage-profile` diagnostic and
`tests/interop/storage_stage_profile.py`. Its default 10 GiB/4 MiB workload
uses a deterministic bijection to permute every 256 KiB positional write, a
bounded write-complete-per-piece hash queue, the engine's 16 KiB positional
hash reads, full allocation, exact piece hashes and cleanup. Sync is measured
after and separately from the transfer-like interval; file hashes are labeled
warm-cache observations.

The first one-run five-point screen measured combined throughput of 1,017.4,
1,008.0, 2,020.9, 1,987.3 and 2,237.1 MiB/s at `1/1`, `2/2`, `4/4`, `8/4`
and `8/8`. Raw writes were 3,369.9--4,018.5 MiB/s; warm file SHA-1 scaled from
1,278.7 MiB/s at one worker to 8,051.7 MiB/s at eight. Every stage used exact
operation counts and hashes and cleaned both materialized 10 GiB files. The
integrated 10 GiB/4 MiB `4/4` median is therefore only 21.4% of its raw
combined point. SHA-1 hardware capacity is not the current owner; integrated
write-service inflation and scheduling remain to be profiled.

The resulting five-second integrated process sample found that the supervisor
still scanned all blocks in a piece to locate the first missing request,
refresh requestability, detect idleness and confirm hash eligibility. Checked
per-piece missing/active counts and a retry-safe first-missing cursor now make
those hot decisions scalar. The existing generation join already proves one
successful write completion for every planned block before hashing; test-only
recomputation independently validates the cached values from block phases.

With executable SHA-256
`7bf6b1f3f129dff1e5e3eba5c5cc802680db12120743003428d6c1119db32ac7`,
the final three-run `4/4` medians are:

| Size | Piece | RSTorrent MiB/s | libtorrent MiB/s | RST/libtorrent |
| --- | ---: | ---: | ---: | ---: |
| 1 GiB | 256 KiB | 506.3 | 481.6 | 105.1% |
| 1 GiB | 1 MiB | 699.3 | 497.5 | 140.6% |
| 1 GiB | 4 MiB | 757.6 | 515.3 | 147.0% |
| 1 GiB | 16 MiB | 715.3 | 491.4 | 145.6% |
| 10 GiB | 256 KiB | 336.2 | 471.5 | 71.3% |
| 10 GiB | 1 MiB | 804.4 | 559.2 | 143.8% |
| 10 GiB | 4 MiB | 1,031.6 | 1,074.4 | 96.0% |
| 10 GiB | 16 MiB | 720.3 | 1,031.8 | 69.8% |

All 24 RSTorrent observations passed exact-byte, whole-file SHA-1, zero
failed/redundant payload, publication and cleanup checks. Every 10 GiB run
finished in 9.408--31.652 seconds. A final 10 GiB/4 MiB concurrency cohort
measured `4/4` at 1,074.1 MiB/s, `8/4` at 1,028.6 MiB/s and libtorrent at
1,061.4 MiB/s. The retained desktop selection is therefore `4/4`; the former
`8/4` advantage disappeared with the scheduling correction.

The selected point's SQLite-backed application profile initially remained far
slower than its engine control: three 128 MiB transfers took 6.894, 7.006 and
7.044 seconds, while the SQLite-independent steady median was 0.555 seconds.
A process sample attributed the application gap to synchronous full Disk view
projection on nearly every block transition, not payload sync, SQLite or
SHA-1. Ordinary storage observations are now coalesced to 100 ms while
checkpoint transitions, errors and terminal state force immediate delivery.

The source-fingerprinted application executable
`b7ab993953f760219b35d9f00cabf3d71cb6877ed7fd1108f11e4853fa081516`
then completed three transfers in 0.567, 0.534 and 0.524 seconds: a
0.534-second median, or 239.7 MiB/s. Each run retained exact payload SHA-1,
512 pieces, four post-metadata durable revisions, publication and cleanup.
This is a controlled application-path result rather than a public-swarm claim.

Tactical `054` closed with one authorized headless full-reference Big Buck
Bunny pair against changing public peers. RSTorrent ran first. Both clients
published the catalog's exact 276,445,467 bytes, 1,055 pieces and three files
with successful cleanup:

| Implementation | Metadata | Publication | Post-metadata payload interval |
| --- | ---: | ---: | ---: |
| RSTorrent | 17.264 s | 29.323 s | 12.059 s |
| libtorrent 2.0.13 | 22.723 s | 36.599 s | 13.875 s |

RSTorrent reported zero piece-hash failures, 4,849,664 redundant bytes,
2.323 seconds summed write service, 1.011 seconds summed hash service and exact
16-block/256 KiB batch high waters. Libtorrent reported zero failed bytes and
21,036,611 redundant bytes. The 0.801 publication-time ratio is one
source-order- and peer-population-dependent observation, not evidence that
RSTorrent is generally faster or a new public regression threshold.

## Hardware Performance Baselines: 2026-08-03

Tactical `057` turns the Tactical `054` matrix into explicit, reviewable
performance policy. Profiles under `tests/perf/baselines` select exact rows,
repetitions and hardware/runner requirements; they keep observed calibration
separate from required floors. An inapplicable environment exits before
fixture allocation and reports `not_applicable`. Hostname, CPU, memory,
temporary filesystem, toolchain, commit, binary SHA-256 and uncontrolled cache
policy remain evidence rather than implicit profile selectors.

The local retained commands are:

```bash
uv run --project tests/interop --locked python \
  tests/interop/local_throughput_compare.py \
  --baseline-profile kmacbook-m4pro --profile-tier smoke \
  --output /tmp/rstorrent-engine-throughput.json

uv run --project tests/interop --locked python \
  tests/interop/application_view_throughput.py \
  --baseline-profile kmacbook-m4pro --profile-tier full \
  --output /tmp/rstorrent-application-views.json
```

`smoke` protects 1 GiB/256 KiB at `4/4`; `full` retains the three-run 1 GiB
and 10 GiB matrix at 256 KiB, 1 MiB, 4 MiB and 16 MiB. The local smoke after
profile integration measured RSTorrent at 615.6 MiB/s and libtorrent at
485.1 MiB/s, passed the row's 275 MiB/s and 0.50-ratio floors, and retained
exact bytes, whole-file SHA-1, zero failed/redundant payload, publication and
cleanup.

The application runner holds one deterministic 1 GiB/256 KiB fixture and
pinned libtorrent seed while rotating three runs over idle, each production
view combination, every view, and a one-second all-view consumer. Every one of
the 39 application transfers verified 4,096 pieces, compared the exact
published payload and removed its case root. The calibrated medians are:

| Mode | MiB/s | Idle ratio | Serialized/batch stream | Maximum resets |
| --- | ---: | ---: | ---: | ---: |
| idle | 177.9 | 100.0% | 0 | 0 |
| library | 166.4 | 93.5% | 49 KB | 0 |
| general | 160.2 | 90.0% | 1.43 MB | 898 |
| peers | 158.7 | 89.2% | 2.96 MB | 902 |
| trackers | 158.3 | 89.0% | 1.60 MB | 900 |
| disk | 154.7 | 86.9% | 65.3 MB | 924 |
| files | 152.1 | 85.5% | 50.1 MB | 1,000 |
| logs-normal | 134.8 | 75.8% | 397.4 MB | 914 |
| logs-detailed | 126.6 | 71.1% | 547.6 MB | 921 |
| pieces | 123.6 | 69.5% | 48.6 MB | 1,231 |
| slow-all | 122.3 | 68.8% | 9.21 MB | 9 |
| logs-trace | 98.4 | 55.3% | 1.081 GB | 1,287 |
| all | 74.0 | 41.6% | 1.742 GB | 1,737 |

Library alone stays incremental. Summary introduces repeated overflow resets;
trace Diagnostics is the worst individual throughput and serialized-volume
offender, and the all-view adversary compounds the costs. The profile accepts
the measured behavior with regression headroom while the application-view
topic owns reducing it; those ceilings do not classify the reset storm as an
efficient target.

`.github/workflows/performance-baseline.yml` runs the uncalibrated, generous
GitHub Ubuntu 24.04 x64 smoke on relevant pull requests, the full tier weekly,
and either tier on manual dispatch. It validates explicit runner identity,
runs both comparators and uploads only JSON evidence. Its initial 20 MiB/s,
0.10 libtorrent-ratio and 3--10 MiB/s application floors are catastrophe
detectors, not hosted-hardware performance claims; calibration requires a
retained CI cohort.

## Maintenance Contract

Feature tacticals add measurements only when their owner can report them
honestly. DHT, multi-peer, endgame, picker, storage, and performance work update
the relevant scenario definitions and append summarized evidence without
turning this topic into a raw run log.

When results motivate a queue change, update
[`capability-readiness.md`](capability-readiness.md) and the relevant focused
topic. Public-swarm evidence never upgrades a protocol claim by itself.
