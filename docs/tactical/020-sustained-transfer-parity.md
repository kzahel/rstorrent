# Tactical 020: Sustained Transfer Parity

Status: Completed; peer-source breadth continues in Tactical `021`.

Topics: `download-correctness`, `peer-lifecycle`,
`performance-and-live-evidence`, `oracle-driven-engine-campaign`

## Motivation And Outcome

The first common-denominator Big Buck Bunny publication comparison reached
verified metadata and a first piece, but RSTorrent stopped at 461 of 1,055
pieces after 900 seconds while libtorrent published in 30.88 seconds. The
terminal RSTorrent snapshot had one connected unchoked peer, four outstanding
16 KiB requests, no writes, and 9,491 missing blocks. That is a 64 KiB static
per-peer flight window even though the live probe's torrent payload allowance
is 64 MiB.

Make request depth a bounded connection-owned feedback state rather than a
fixed scheduler constant, and remove the four-active-piece ceiling as an
accidental throughput limit while retaining explicit bounds. The resulting
engine must fill capable peers, keep slow peers from consuming unbounded work,
preserve torrent-wide payload accounting and fairness, and reach first-piece
and 50% milestones reliably under the paired headless harness.

This tactical stops at sustained ordinary transfer. Endgame duplicates,
cancel messages, hash-failure recovery, and final publication parity remain
the next correctness owner unless evidence shows that an existing transition
must be corrected here to reach 50%.

## Source Dossier

Pinned libtorrent `2.0.13` at `7d7fc38fac61177fa5e02148f791b2f65250b09d`
is the behavioral completeness oracle. No source or fixture is copied.

- `src/request_blocks.cpp::request_a_block` computes missing queue capacity as
  the connection's desired queue less sent and pending requests, then asks the
  torrent piece picker for that many blocks. Ordinary mode avoids duplicate
  busy blocks.
- `include/libtorrent/peer_connection.hpp` starts a connection's desired
  queue at four requests. Endgame or a snubbed peer presents a target of one.
- `src/peer_connection.cpp::incoming_piece` increases the target by one for
  each useful block during slow start, recalculates the bounded target, and
  immediately refills the request queue.
- `peer_connection.cpp::update_desired_queue_size` uses three seconds of
  measured payload rate divided by the torrent block size outside slow start,
  with a minimum of two and the configured maximum. The pinned default
  `max_out_request_queue` is 500.
- `peer_connection.cpp::second_tick` exits slow start when the previous
  one-second rate no longer improves materially, refreshes the rate-derived
  target, and detects request and piece stalls.
- `peer_connection.cpp::request_timeout` derives a two-to-sixty-second bound
  from request-time samples. `snub_peer` reduces the target to one and avoids
  abandoning a block unless it is holding up piece completion.
- `request_blocks.cpp` and `piece_picker.cpp` let queue demand drive the amount
  of picked work. At sufficient rate, the default twenty-second whole-piece
  threshold favors locality; there is no four-piece global ceiling.
- `test/test_piece_picker.cpp` covers partial-piece priority, availability,
  busy-block avoidance, whole-piece selection, and download-queue accounting.

RSTorrent's current inputs are `swarm::ConnectionState`, `SwarmConfig`,
`SwarmState::schedule`, request expiry and receive transitions,
`ContentSwarmDownload::handle_message`, and the bounded diagnostic projection.
The current defaults are four requests per connection and four active pieces;
the global payload allowance is already authoritative and checked before each
assignment.

## Ownership And Design

Each `ConnectionState` owns a small runtime-independent request-window state:
current target, slow-start or steady/stalled phase, useful payload samples,
and the monotonic times needed for rate and request observations. It receives
explicit elapsed `Duration` values from the torrent supervisor and owns no
socket, Tokio task, wall clock, storage handle, or diagnostic sink.

`SwarmState` remains the sole owner of block assignments and payload
reservations. Scheduling asks each eligible connection for its current target
and continues round-robin assignment until connection, payload, availability,
or active-work bounds stop it. Accepted requested payload is the only input
that grows a peer's window. Redundant, unsolicited, keepalive, `have`, and
other messages do not.

The initial policy follows the reference behavior rather than its class
layout:

- start at four outstanding requests per connection;
- add one target slot per accepted block during slow start;
- sample useful payload at one-second boundaries and leave slow start when
  throughput no longer improves materially;
- in steady state target three seconds of measured payload, rounded to block
  requests and clamped to two through 500;
- reduce a peer that expires useful work to a one-request probe state; and
- let a subsequent timely accepted block clear that probe state without
  crediting unrelated traffic.

The torrent-wide payload allowance remains the hard memory/work reservation
bound, including production configurations much smaller than the reference
maximum. The default partial-piece bound may increase conservatively up to 64
active pieces after deterministic evidence; scheduling fills already-active
pieces before opening another, preserving locality. This permits the pinned
500-request maximum for common 256 KiB pieces without allowing arbitrary
piece scattering.

## Invariants And Bounds

- Every request still has one active connection generation, attempt ID,
  issuance time, and one payload reservation.
- A connection target is always between one and 500; ordinary healthy targets
  are at least two and start at four.
- Total requested and writing bytes never exceed the caller's payload
  allowance, regardless of the sum of connection targets.
- At most eight established peers, three pending dials, 64 active pieces, and
  500 requests per peer are considered under the default configuration.
- Only accepted payload for an actually requested block changes rate or
  slow-start state. Late accepted payload keeps ownership accounting correct
  but cannot grow a disconnected generation.
- Choke, disconnect, replacement, cancellation, and expiry release exactly
  their existing assignments. Window changes do not synthesize or transfer
  block ownership.
- A silent peer retains only its bounded initial window and is reduced after
  expiry; a fast peer can grow without waiting for unrelated peers.
- Window and rate diagnostics are bounded typed projections and never become
  scheduler input.
- Arbitrary peer messages and time values cannot overflow byte, rate, target,
  deadline, or payload arithmetic.

## Adversarial Validation

### Pure state

- a new peer receives four requests when payload and work permit;
- successive accepted blocks grow and refill only that peer's slow-start
  window;
- a silent peer remains bounded while a responding peer expands and receives
  the remaining work;
- a one-second non-improving sample enters steady state and derives a bounded
  three-second target from measured bytes;
- expiry reduces only the responsible connection to one probe request, and a
  timely response recovers it without leaking the expired reservation;
- choke, disconnect, late payload, global payload pressure, small allowances,
  active-piece pressure, and integer-boundary cases retain their existing
  ownership and fairness guarantees.

### Scripted runtime

- a loopback peer that requires more than four requests to fill its
  bandwidth-delay product observes a growing bounded pipeline and publishes
  exact bytes;
- a fast peer progresses while another valid unchoked peer accepts requests
  but withholds payload;
- slow storage holds the configured payload allowance without unbounded socket
  tasks, decoder input, writes, or request state; and
- cancellation and every terminal path join all owned tasks exactly once.

### Interoperability And Live Evidence

- retain the controlled libtorrent multi-piece publication and mixed-peer
  liveness scenarios;
- record a pre-change three-pair first-piece screen and endpoint-free terminal
  scheduler facts;
- after implementation, run at least three alternating Big Buck Bunny
  common-denominator pairs to first piece and three to 50%; require correct
  identity, hashes, cleanup, and at least 2/3 RSTorrent milestone success;
- if the initial result is functional, run an independent confirmation cohort
  and report the paired median and p90 ratio without hiding public variance;
- call the milestone comparable only when two independent cohorts each have
  at least 8/10 RSTorrent successes and median paired latency no worse than
  2.0x libtorrent. A smaller screen may justify the next implementation
  checkpoint but cannot make the parity claim; and
- if 50% remains below the gate, retain a detailed bounded snapshot, derive at
  most three new hypotheses from pinned source, and continue at the narrowest
  demonstrated owner. Rotate away from changing-swarm experimentation when
  source or deterministic evidence is more informative.

Public network use and multi-gigabyte payload transfer are explicitly in
scope. All runs remain headless, use isolated temporary storage, and remove
their reports and payloads after durable aggregate evidence is recorded.

## Implementation Order And Intermediate Gates

1. Capture the pre-change first-piece screen and add deterministic request
   window tests that fail under the static policy.
2. Land the pure per-connection feedback state and target-aware scheduling;
   run focused state, architecture, clippy, and workspace tests.
3. Extend bounded aggregate/per-connection scheduler diagnostics as needed to
   distinguish window, payload, availability, and active-piece limits.
4. Prove pipeline growth, slow-peer coexistence, bounded storage pressure, and
   exact cleanup with scripted sockets and controlled libtorrent.
5. Run first-piece and 50% paired screens. Change further policy only from a
   retained snapshot plus the pinned-source dossier.
6. Run the publication comparator only after first-piece and 50% are
   functional; leave endgame/integrity work to the following tactical.

Each step may be committed independently when its focused gates pass.

## Initial Evidence

The retained pre-change common-denominator screen ran three alternating Big
Buck Bunny pairs to first verified piece. Both implementations completed 3/3.
RSTorrent's first-piece times were 0.74, 75.85, and 0.81 seconds versus
libtorrent's 20.83, 20.40, and 20.68 seconds. The slow RSTorrent sample spent
75.47 seconds reaching metadata; transfer from metadata to first piece took
0.22, 0.38, and 0.22 seconds across the three runs.

All three RSTorrent terminal snapshots had one unchoked content peer, exactly
four outstanding requests, a 65,536-byte payload high-water mark, and
`requestwindowsfull`. They had stored 262,144 or 278,528 bytes and retained
16,852 or 16,853 missing blocks. Correct identity, geometry, milestone, and
temporary-root cleanup passed. First-piece startup is therefore functional;
the falsifiable owner remains the static sustained request window already
present in the older 43.7%-after-900-seconds publication snapshot.

## Implementation Evidence

The first implementation checkpoint installs a runtime-independent
`RequestWindow` inside each connection state. It starts at four, grows only on
accepted requested payload, samples one-second useful-payload rates, settles
on a bounded three-second rate target, and drops to a one-request probe after
expiry. Targets cap at the pinned reference default of 500, while the existing
torrent payload allowance remains authoritative. The separate active-piece
bound is now 64 and existing active pieces are still filled before another is
opened.

Pure tests prove two-peer selective growth/refill, slow-start exit and rate
clamping, stalled probing and recovery, payload pressure, expiry, late data,
choke, disconnect, active-piece and history bounds, and overflow-safe target
math. A loopback 512 KiB peer deliberately buffers the request pipeline and
proves that the observed queue and engine payload high-water both exceed the
old four-request ceiling while exact bytes publish and the task joins.

At the first checkpoint, the engine's 105 non-live tests passed with three
public tests ignored. The
three-run controlled first-piece scenario, mixed healthy/permanently-choked
16-piece scenario, and full paired controlled comparator all pass. The
controlled comparator's final RSTorrent snapshot reported a target of nine,
79,000 accepted useful bytes, exact three-piece publication, and clean
shutdown. Public first-piece and 50% evidence was still unmeasured at that
checkpoint; it did not claim live parity.

The post-change first-piece screen again completed 3/3 for both owners.
RSTorrent's terminal request targets grew to 21--46 with 344--754 KiB payload
high-water instead of the old four requests and 64 KiB. Its transfer interval
from metadata to first piece remained about 0.2 seconds in the two immediate
discovery runs; the slower run retained two productive connections and still
completed.

The first three-pair 50% screen completed only 1/3 for RSTorrent and 3/3 for
libtorrent, so it did not meet the 2/3 functional screen. The productive pair
is strong evidence for the window owner: RSTorrent reached 50% in 28.14
seconds versus libtorrent's 27.98 seconds, a 1.006x ratio, with 529 verified
pieces, a target of 521 requests, 8.68 MiB high-water, and exact cleanup.

The two misses retained only 90 and 101 verified pieces after 300 seconds.
They ended with two unchoked peers, 712 or 772 outstanding requests, aggregate
targets of 713 or 777, 11.68 or 12.66 MiB high-water, and no peer classified
stalled. Their connected peers had previously delivered about 26 MiB and
grown targets near 500 before ceasing useful output. This rejects the static
window as the remaining cause and identifies the pinned adaptive
request-response timeout/snubbing behavior as the next narrow owner.

The second checkpoint records a bounded twenty-sample request-response
estimator per connection. Before samples, the configured 60-second deadline
remains unchanged. Once useful blocks establish response timing, inactivity
uses libtorrent's average-plus-deviation form, rounded and clamped from two
seconds through the configured ceiling. A detected stall releases that
generation's whole ordinary window, reduces it to one probe, and permits
healthy peers to own the missing blocks; stale responses remain safe. Pure
tests prove the estimator, configured sub-two-second fixture ceiling, full
window release, reservations, and probe recovery. A real two-peer loopback
case serves one burst then withholds every later request; another peer
completes exact publication under the sampled deadline while the unsampled
ceiling is ten seconds.

Content diagnostics now retain current peer-registry totals, eligible,
dialing, backed-off, and failure-limited counts separately from the metadata
snapshot, plus the min/max adaptive timeout. The next live screen can
therefore distinguish exhausted discovery from stalled connections without
exposing endpoints.

At the second checkpoint, formatting and warning-denying clippy pass across
the workspace. The workspace test suite passes with 218 tests, three changing
public-network tests ignored, and no failures. The controlled mixed-peer and
paired publication gates also remain green.

A clean owner-only three-run 50% screen from commit `6c636ab` reached the
milestone once in 24.09 seconds and timed out twice at 180 seconds. The misses
verified 90 and 244 pieces (8.5% and 23.1%). Their current content registries
contained only nine and four candidates: two connected in each run, zero
eligible, and seven backed off in one versus one backed off and one dialing in
the other. The latter was again receiving about 4.45 MiB/s at termination.
The adaptive timeout was two seconds on every sampled productive connection;
one slow-start peer without a response sample retained the configured
60-second ceiling as designed.

This retains two distinct findings. Once a capable peer exists, RSTorrent now
reaches the reference's 24--28 second 50% range. It does not yet find and keep
enough alternatives reliably: live RSTorrent runs retained only four to nine
content candidates and two connections, whereas the paired libtorrent runs
had 16--22 connected peers at the same milestone. Pinned source identifies
initial tracker-operation breadth and connection boost as a separate owner:
libtorrent queues all not-yet-working magnet tracker tiers during its initial
announce pass and immediately attempts up to thirty returned peers. Tactical
`021` owns that peer-source boundary rather than expanding request-window
policy without evidence.

## Non-Goals And Next Boundary

- No product UI, Tauri launch, visible browser, Android device, incoming
  listener, upload/seeding, NAT traversal, or new discovery protocol.
- No endgame duplicates, core cancel message, piece hash-failure retry,
  contributor reputation, rarest-first claim, streaming priority, or durable
  resume change.
- No session-wide multi-torrent bandwidth or memory allocator.
- No unbounded queue chosen solely to improve one public benchmark and no CI
  threshold on a changing swarm.

The next tactical owns DL-C07 through DL-C09 and full verified publication:
bounded duplicate requests, cancel/late-loser behavior, hash reset and source
attribution, then 95%, 99%, and completion cohorts.

## Escalation And Stopping Condition

Ordinary refactoring inside the swarm/request owner, conservative limit
selection within the bounds above, new adversarial tests, comparator
diagnostics, and source-derived fixes at this boundary do not require human
input. Stop only if evidence requires materially different product behavior,
a persistence or public compatibility change, an external dependency/license
change, visible-device interaction, or work outside this tactical's owner.

The stopping condition is met through the explicitly permitted classified
external boundary. The adaptive request owner and expanded bounded work set
pass pure and scripted hostile cases, controlled libtorrent publication
remains exact, first-piece evidence remains functional, healthy-peer 50%
speed matches the reference range, and the failing 50% screen retains a
current peer-registry classification. Tactical `021` owns that discovery and
working-set boundary before endgame/integrity work resumes.
