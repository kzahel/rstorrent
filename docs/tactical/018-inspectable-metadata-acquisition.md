# Tactical 018: Inspectable Metadata Acquisition

Status: Complete

Topics: `peer-lifecycle`, `performance-and-live-evidence`, `application-control`

## Motivation And Desired Outcome

The tracker-based Big Buck Bunny metadata smoke exceeded its 90-second bound
after Tactical `017`, but retained no peer-registry, dial, extension-handshake,
or metadata-request state. The result proves only that verified metadata did
not arrive. It cannot distinguish tracker silence, unusable candidates, dial
failure, peers without BEP 10 or BEP 9, rejected requests, partial metadata,
or a peer that remains active while making no metadata progress.

Add a bounded read-only engine diagnostic snapshot and use it to rerun the
headless smoke. The snapshot must answer what every known candidate is doing,
why it is or is not selectable, what each active/recent metadata attempt
negotiated, and how far its BEP 9 transfer progressed. A timeout must print the
snapshot before cancellation so cleanup does not erase the evidence.

This tactical may fix metadata-candidate lifecycle defects demonstrated by
deterministic tests or the retained live evidence. It does not add product UI.

## Dependencies And Reference Survey

- [`../topics/peer-lifecycle.md`](../topics/peer-lifecycle.md)
- [`../topics/performance-and-live-evidence.md`](../topics/performance-and-live-evidence.md)
- [`../topics/application-control.md`](../topics/application-control.md)
- Tactical `006` BEP 9 metadata exchange
- Tactical `010` peer registry and dial generations
- Tactical `012` bounded typed diagnostics
- Tactical `017` bounded parallel metadata acquisition
- BEP 9 and BEP 10 from the pinned specification checkout

Pinned Rasterbar libtorrent `2.0.13` is the completeness oracle. The survey
uses `src/ut_metadata.cpp`, `include/libtorrent/peer_info.hpp`,
`include/libtorrent/torrent_handle.hpp`, and the metadata-extension cases in
`test/test_fast_extension.cpp`. Relevant behavior is:

- the initial extension handshake decides whether a peer advertises
  `ut_metadata` and its remote message ID;
- metadata request and received-block state belongs to the torrent plugin and
  can span peers;
- each peer has a bounded request queue, explicit rejection/backoff state, and
  low-level `UT_METADATA` peer logs;
- request selection avoids letting a peer believed not to have metadata starve
  peers that do; and
- synchronous or asynchronous peer-info snapshots expose connected-peer
  flags, queues, timing, transfer counters, source, failure, and progress.

The first-party JSTorrent sibling supplies product-table vocabulary through
`PeerTable.tsx`, `SwarmTable.tsx`, and `__jstorrent_query_swarm_debug`, plus
metadata behavior in `metadata-fetcher.ts` and `peer-connection.ts`. It exposes
connected peers separately from all known swarm records and records source,
state, attempts, failures, last error, transfer counters, request counts, and
identity. Its current metadata fetcher logs absent `ut_metadata`, missing or
inconsistent size, requests, blocks, rejection, disconnect, and hash mismatch,
but retains per-peer assembly; that ownership is useful comparison evidence,
not a required RSTorrent design.

No reference source or fixture is copied.

## Scope

- A coherent `DownloadControl` diagnostic snapshot containing ordinary byte
  progress, the latest content-swarm snapshot when one exists, and metadata
  acquisition state.
- A full bounded peer-registry table for the current torrent with stable
  record identity, endpoint, sources, phase, dial eligibility, observation
  timing, attempts, failures, backoff, and last failure.
- Active and bounded recent metadata-attempt rows with dial identity, stage,
  BEP 10 support, remote `ut_metadata` ID, advertised size, request/block/byte
  counts, message activity, last progress, and terminal reason.
- Aggregate metadata counters and exact active/pending bounds.
- Deterministic cases for extension-incapable peers, an initial extension
  handshake that omits `ut_metadata`, request rejection, partial progress,
  terminal failure retention, cancellation, and history bounds.
- One opt-in background-only Big Buck Bunny metadata rerun with a snapshot at
  regular intervals and immediately before timeout cancellation.
- In-scope fixes to candidate release, retry, or metadata negotiation when the
  new deterministic/live evidence demonstrates a concrete defect.

## Ownership And Data Flow

```text
PeerRegistry (authoritative candidate records)
      | bounded pure snapshot
      v
metadata supervisor --------------------+
      | pending dials / terminal results |
      v                                  v
per-peer metadata worker ----------> DownloadControl diagnostic projection
  socket + BEP 10/BEP 9 state          read-only coherent snapshot API
```

The peer registry and metadata worker remain authoritative. The diagnostic
projection mirrors bounded facts and never drives selection, retry, protocol,
or completion. Protocol codecs remain runtime independent. The snapshot mutex
owns only the narrow diagnostic projection, not engine behavior or sockets.

## Contracts And Invariants

- Snapshot reads do not mutate scheduling or wait for network I/O.
- Candidate rows are bounded by the existing 1,000-record registry limit.
- Active metadata rows are bounded by the existing three-work-item limit.
- Terminal metadata history retains at most 64 rows and reports truncation.
- Error detail is bounded; payload, metadata bytes, magnets, and tokens are
  never included.
- Peer endpoints may appear in this explicit local diagnostic API, but
  committed summaries redact them and raw live output remains temporary.
- Missing measurements are represented as unknown, not zero.
- Initial extension-handshake omission of `ut_metadata` cannot masquerade as
  an indefinitely active metadata negotiation.
- Timely unrelated peer messages cannot grant an unbounded metadata slot when
  no BEP 9 progress is possible.
- Timeout evidence is captured before cancellation; cancellation still joins
  or aborts every owned task through the existing bounded fallback.

## Initial Bounds

| Resource | Bound |
| --- | --- |
| Peer registry rows | Existing 1,000 per torrent |
| Active metadata work | Existing 3 pending dials plus workers combined |
| BEP 9 requests per active worker | Existing 2 |
| Recent terminal metadata rows | 64 |
| Terminal error detail | 256 UTF-8 bytes |
| Live smoke duration | 90 seconds tracker-only; 120 seconds trackerless DHT |
| Snapshot interval | 10 seconds |
| Cancellation join allowance | Existing 5 seconds in the smoke |

## Validation

### Pure and deterministic

- Peer-registry snapshots classify every `DialEligibility` and preserve stable
  record/attempt history.
- Metadata snapshots retain exact stage and counters across negotiation,
  request/data/reject, failure, completion, and cancellation.
- Active and terminal row bounds hold under churn.
- The initial no-`ut_metadata` handshake releases its slot even if the peer
  would otherwise continue sending valid core messages.

### Scripted runtime

- Three mixed peers independently cover unsupported BEP 10, omitted BEP 9,
  partial metadata, and a later successful source.
- Timeout/cancellation retains the pre-cancel snapshot and joins all tasks.

### Live evidence

- Run the ignored Big Buck Bunny metadata probe headlessly under `Online`.
- Record periodic aggregate summaries and the full timeout snapshot locally.
- Report discovered, eligible, dialing, backed-off, exhausted, connected,
  extension-capable, BEP-9-capable, requesting, partially productive, and
  terminal-failure counts without committing peer addresses.
- Treat a changing public result honestly; completion is useful but not a
  required correctness gate.

### Repository gates

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --no-fail-fast`
- `git diff --check`

No Tauri, Chrome, emulator, AVD, or physical device is launched.

## Non-Goals

- No desktop, web, or Android presentation.
- No stable public wire format or remote diagnostic daemon.
- No payload endgame, cancel messages, hash-failure recovery, PEX, incoming
  listener, uTP, or connection-budget tuning.
- No claim that one public swarm completion proves general reliability.
- No automatic adoption of libtorrent's class graph or JSTorrent's table API.
- Multi-source metadata assembly is changed only if retained evidence proves
  it is required for this stopping condition; otherwise it remains a recorded
  subsequent correctness choice.

## Escalation And Stopping Condition

Ordinary snapshot design, proportional internal refactoring, deterministic
metadata lifecycle fixes, the bounded public rerun, and topic updates are
authorized in this slice. Stop only for a new dependency or license posture,
a stable application compatibility change, visible/device interaction, or a
material expansion into a non-goal.

The tactical is complete when a timeout or completion produces a coherent
pre-cancel state dump that explains candidate and metadata-attempt disposition;
the no-`ut_metadata` indefinite-slot case is closed deterministically; all
owners remain bounded and join; the public rerun is classified from retained
facts; and the repository gates pass.

## Implementation Record

Completed on 2026-07-31.

`DownloadControl::diagnostic_snapshot` now returns ordinary byte progress, the
last content-swarm state when one exists, and a coherent metadata-acquisition
projection. The metadata projection retains the full bounded peer-registry
table, aggregate eligibility counts, at most three active attempts, at most 64
recent terminal attempts with a dropped-row count, 256-byte terminal detail,
BEP 10 and `ut_metadata` negotiation, size/block/request/message/rejection
counters, and activity/progress timestamps. It is read-only and does not drive
protocol or selection state.

Deterministic and scripted evidence proves every registry eligibility class,
history and detail bounds, three extension-capable peers that omit
`ut_metadata` releasing their slots for a later useful peer, three peers that
send unrelated keepalives losing their slots to the metadata-progress
deadline, and three explicit BEP 9 rejections releasing their slots and being
counted before a later peer completes. Existing parallel-stall, late-tracker,
private-DHT, controlled DHT, and bidirectional metadata tests remain green.

The 90-second tracker-only Big Buck Bunny rerun retained zero candidates,
zero dials, and zero metadata requests. Two UDP tracker connects timed out,
one tracker no longer resolved, and two trackers rejected the port-zero
announce. This failure is before peer or BEP 9 behavior rather than an opaque
metadata failure.

A follow-up threads an explicit provisional port through tracker announce
state and advertises conventional port `6881`. A scripted tracker asserts the
exact port field on the wire. The same tracker-only public smoke then returned
six candidates in 0.36 seconds and acquired the hash-verified 21,307-byte info
dictionary in 11.41 seconds using three attempts and two metadata requests.
There is still no incoming listener or NAT mapping; those capabilities and
seeding remain deliberately lower priority than outbound download
correctness.

Two trackerless DHT reruns then completed verified metadata after 31.2 and
45.9 seconds. They retained 93 and 100 candidate records and made 9 and 12
dial attempts. In the latter run, 3 peers refused TCP, 3 timed out connecting,
3 reset during handshake or extension setup, 1 peer advertised extension
support and remote `ut_metadata` ID 2 and supplied both blocks, and 2 losing
dials were canceled. Exactly 2 requests returned 2 blocks and 21,307 bytes;
the final snapshot had 90 eligible records, 9 backed off, 1 connected winner,
and zero pending dials, active workers, active metadata requests, DHT
transactions, or DHT lookups. The DHT snapshot retained 8 routing nodes after
80 queries, 56 valid responses, and 100 discovered peer values.

An immediate ten-run cohort for each discovery mode then retained the
following distribution:

| Discovery | Completed | Bound | Successful latency min / median / mean / max |
| --- | ---: | ---: | ---: |
| UDP trackers only | 8/10 | 90 s | 1.71 / 32.77 / 38.41 / 75.51 s |
| DHT only | 7/10 | 120 s | 30.84 / 78.69 / 72.59 / 104.35 s |

The two tracker failures each ended with six attempted candidates, four
requests, two 16 KiB blocks, 32,768 received bytes, and no verified metadata.
The three DHT failures retained 29–83 candidates and 23–38 attempts but zero
metadata requests despite successful lookup traffic. Across all ten DHT
runs, 2,759 queries received 2,223 valid responses. These repeatable terminal
shapes make peer ordering, connection and extension negotiation, retry cadence,
and per-source metadata progress the next investigation boundary.

Repository validation passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace --no-fail-fast`: 98 engine tests passed with three
  opt-in live tests ignored, plus all remaining workspace tests and doc tests;
- focused metadata tests: 14 passed with both public tests ignored; and
- `git diff --check`.

No Tauri, Chrome, emulator, AVD, or physical device was launched or automated.
No live peer endpoint is retained in committed documentation. Product
projection of selected snapshot fields, incoming listening and NAT traversal,
multi-source metadata assembly, and the paired public comparator remain
separate work.
