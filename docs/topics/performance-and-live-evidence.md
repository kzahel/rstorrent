# Performance And Live Evidence

Topic: `performance-and-live-evidence`

Status: In progress. A controlled headless DHT/libtorrent completion harness
and one bounded single-sided public DHT probe now exist. Tactical 015 still
owns the paired RSTorrent/libtorrent public comparator, result schema, and
resource/timing report. Public-swarm speed remains a measured baseline, not a
CI pass threshold.

## Purpose

Real swarms expose discovery, scheduling, timeout, completion, CPU, memory, and
disk behavior that isolated fixtures cannot reproduce completely. They are
valuable smoke evidence even though peer populations and network conditions
vary between runs.

This topic defines how to collect that evidence without confusing a noisy
observation with a correctness claim. It also keeps performance work tied to
end-to-end product outcomes instead of isolated micro-optimization.

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
as evidence that discovery is operating and that peer connection/transfer
ownership remains an ordinary-swarm reliability gap; it is not a public
completion or speed claim.

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

## First Tactical Boundary

The next tactical should add the smallest harness that can:

1. select a catalog entry and comparison mode;
2. run a bounded libtorrent reference download and a bounded RSTorrent
   download headlessly in isolated temporary directories;
3. validate identity, verified completion, and output size;
4. emit the paired classification and core timing/resource metadata as JSON;
5. retain bounded diagnostic evidence on an actionable mismatch; and
6. exercise one available tracker-based catalog entry.

Selective pinned-reference checkout tooling and the single-sided DHT harness
have landed as prerequisites. The remaining tactical does not add product UI,
establish speed gates, or change the DHT implementation itself.

## Maintenance Contract

Feature tacticals add measurements only when their owner can report them
honestly. DHT, multi-peer, endgame, picker, storage, and performance work update
the relevant scenario definitions and append summarized evidence without
turning this topic into a raw run log.

When results motivate a queue change, update
[`capability-readiness.md`](capability-readiness.md) and the relevant focused
topic. Public-swarm evidence never upgrades a protocol claim by itself.
