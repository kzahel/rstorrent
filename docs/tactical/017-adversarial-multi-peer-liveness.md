# Tactical 017: Adversarial Multi-Peer Liveness

Status: In progress

Topics: `peer-lifecycle`, `download-correctness`

## Motivation And Desired Outcome

RSTorrent can discover many peers but currently installs only one live content
connection. One peer that lacks a wanted piece, remains choked, withholds a
request while sending unrelated messages, disconnects, or becomes stale can
therefore terminate or strand otherwise valid work. A public DHT metadata
attempt found many peer values without completing, and
OBS-2026-07-31-001 recorded a real torrent stalled near displayed completion.

This tactical replaces the one-connection and piece-local request boundary
with one bounded torrent-owned connection set and request scheduler. It is
complete only when the adversarial scenarios named below prove content
liveness, exact ownership, resource bounds, cancellation, and verified
publication end to end.

The implementation remains first-party, in-process, and headless. It is not a
general peer-management framework and does not add product UI.

## Dependencies And Reference Survey

- [`../topics/peer-lifecycle.md`](../topics/peer-lifecycle.md)
- [`../topics/download-correctness.md`](../topics/download-correctness.md)
- [`../topics/dht-discovery.md`](../topics/dht-discovery.md)
- Tactical `010` peer registry and generation-guarded dial state
- Tactical `012` typed bounded diagnostics
- Tactical `014` scheduled tracker discovery
- Tactical `016` session DHT discovery
- BEP 3 peer messages and the existing bounded peer-wire codec

Pinned Rasterbar libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` is the primary completeness and
edge-case oracle. The required survey includes:

- `src/torrent.cpp` and `include/libtorrent/aux_/torrent.hpp` for torrent-owned
  peer sets, picker coordination, connection pressure, and completion;
- `src/peer_connection.cpp` and `include/libtorrent/peer_connection.hpp` for
  connection-scoped request queues, choke/disconnect cleanup, request timing,
  snubbing, handshake/inactivity deadlines, and late responses;
- `src/bt_peer_connection.cpp` for bounded peer-wire request, piece, choke,
  bitfield, `have`, and later cancel behavior;
- `src/piece_picker.cpp` and `include/libtorrent/piece_picker.hpp` for block
  states, peer attribution, request abort, timeout, partial-piece preference,
  and the boundary to deferred endgame behavior;
- `src/peer_list.cpp`, `src/torrent_peer.cpp`, and `test/test_peer_list.cpp` for
  connection-independent records, candidates, capacity, failure, and
  replacement; and
- focused picker, peer-list, swarm, disconnect, and request-queue tests.

Relevant observed reference behavior includes a 60-second no-sample request
timeout that adapts with RTT evidence, a distinct 10-second handshake timeout,
request-queue cleanup on choke, torrent/picker ownership of aborted work, and
capacity-sensitive disconnection of uninteresting peers. Exact constants and
class boundaries are not copied.

The first-party JSTorrent sibling is correlated implementation and failure
evidence, not an independent oracle. Review `connection-manager.ts`,
`piece-requester.ts`, `active-piece.ts`, `swarm.ts`, request-pipeline tests,
connection-limit tests, and snubbing tests. Useful evidence includes counting
pending plus established connections, clearing only one peer's requests on
choke, per-attempt request timestamps, adaptive timeout experiments, and
dropping slow peers only when alternatives exist. RSTorrent does not inherit
JSTorrent's daemon boundary, event architecture, picker breadth, or defaults.

## Scenario Scope

This tactical owns complete evidence for:

- DL-C02: split availability, including a final piece only Peer B has;
- DL-C03: disconnect with outstanding and writing requests;
- DL-C04: choke releases only that connection's assignments;
- DL-C05: request expiry despite timely unrelated peer messages;
- DL-C06: a tracker or DHT peer discovered after content starts becomes useful;
- DL-C20: all established slots are permanently choked and a useful candidate
  arrives;
- DL-C21: all established peers lack the remaining wanted piece;
- DL-C22: pending dials accept TCP but do not complete handshake;
- DL-C23: an expired request is reassigned and the old generation responds;
- DL-C24: no useful replacement exists, so the torrent waits without churn or
  a false blocked state; and
- DL-C25: hostile observation and connection churn holds every declared bound.

The tactical may add adversarial cases implied by the invariants without human
approval. A newly discovered case becomes required in-slice when it changes
ownership, cancellation, integrity, compatibility, or a declared resource
bound. Policy-only optimization cases may be recorded for the measured-policy
slice.

## Ownership And Dependency Map

```text
PeerRegistry (connection-independent records and dial generations)
        |
        v
pure engine::swarm state
  connection generations, availability/choke facts,
  blocks, request attempts, deadlines, reservations,
  deterministic scheduler decisions and snapshots
        |
        v
torrent supervisor
  discovery intake, dial filling/replacement, connection task owner,
  storage writes, verification, checkpoints, cancellation and joins
        |
        +---- bounded commands ----> one task per peer socket
        |                             socket, decoder, write queue,
        |<---- bounded events ------- protocol and payload events
        |
        +---- authoritative writes --> existing storage owners
```

- Pure swarm state must not contain Tokio, sockets, filesystems, task handles,
  channels, or platform adapters. It remains in `rstorrent-engine` unless a
  concrete dependency or reuse problem justifies a crate extraction.
- `PeerRegistry` remains the only observation merger and dial-generation
  authority. Trackers and DHT do not open sockets directly.
- One torrent supervisor owns the connection set, scheduler, storage calls,
  checkpoints, and child-task collection.
- A connection task owns exactly one socket generation and its decoder. It may
  emit bounded typed events but cannot mutate torrent block or have state.
- Storage acceptance and complete-piece verification remain the only route to
  durable have state and publication.
- Every child task has cancellation, a bounded queue, and an observable join.
  Drop is not the normal shutdown path.

## State And Transition Contract

Required stable identities and values include:

- connection generation tied to one successful `DialAttempt`;
- `BlockKey { piece, begin, length }` derived from trusted layout;
- request attempt containing block, connection generation, issued time, and
  disposition;
- dispositions at least requested, writing, received, expired, choked,
  disconnected, superseded, write-failed, and cancelled;
- connection state containing validated availability, choke state, last useful
  progress, and outstanding request count; and
- scheduler deadline/reason facts for request expiry, slot replacement, dial
  eligibility, discovery, and waiting.

Ordinary scheduling permits one active request attempt per block. The state
shape may retain bounded earlier attempts so a response to an expired or
closed generation is classified without mutating current ownership. A valid
late block is harmless: it may satisfy a currently reassigned missing block or
be discarded as redundant, but it cannot release another block's reservation
or fail the torrent merely because its original attempt expired.

Choke, disconnect, request expiry, connection replacement, pause, and shutdown
terminate that generation's active attempts exactly once. Storage writing
keeps its reservation until acceptance or failure even if the originating
socket closes. A stale event cannot affect a newer connection generation for
the same endpoint.

## Initial Resource And Timing Bounds

The implementer may tighten these values when reference study or adversarial
evidence justifies it, and must record the final values. Increasing them beyond
the stated ceiling requires updating this tactical with measured memory and
task impact; it does not require human feedback while the owner and product
behavior remain unchanged.

| Resource | Initial value or ceiling |
| --- | --- |
| Peer records | existing 1,000 per torrent |
| Established peer connections | 8 per torrent |
| Pending outbound dials | 3 per torrent |
| Total peer socket tasks | established plus pending, at most 11 |
| Active request attempts per peer | 4 before measured tuning |
| Active request attempts session-wide | payload allowance divided by block size, never less restrictive than that allowance |
| Active pieces | 4; prefer existing partial work before new pieces |
| Request block | existing maximum 16 KiB |
| Global payload reservation | existing configured bounded allowance |
| Connection event queue | 64 per torrent |
| Connection command queue | 16 per established peer |
| Retained terminal attempts | 4 per block until piece verification/reset |
| Request timeout without RTT policy | 60 seconds, tested through an explicit clock |
| Unproductive connection grace | 60 seconds and only actionable under capacity pressure |
| Handshake and socket operation deadlines | existing `NetworkConfig` bounds |
| Scheduler work | bounded by configured active peers, pieces, and attempts per pass |

No peer may reserve the whole global allowance while another useful unchoked
peer has schedulable work. The first deterministic policy gives each useful
peer one request opportunity before filling remaining per-peer windows.

## Dial, Retention, And Replacement Policy

- Fill at most the pending-dial bound and recheck network policy immediately
  before every connect.
- A TCP connection does not consume an established slot until the handshake
  succeeds; pending plus established sockets remain under the total task cap.
- Metadata may be obtained from any one bounded successful peer. Multi-source
  metadata block assembly is not required, but parallel dial/handshake work
  must cancel or hand off cleanly and compatible peers may remain for content.
- Protect peers with unique wanted-piece availability, active accepted writes,
  or recent useful payload.
- Under full established capacity and with an eligible alternative, prefer
  replacing protocol-failed, unavailable-for-wanted-work, long-choked, or
  request-stalled peers, in that order with deterministic ties.
- Do not evict solely for low instantaneous speed in this slice.
- Without an eligible alternative, retain plausible connections, discovery,
  and retry deadlines instead of cycling sockets.
- Replaced peers transition through ordinary generation-checked close and
  backoff accounting. Policy replacement is distinct from protocol failure.

## Shape-Changing Edge Cases Required In-Slice

- simultaneous dial completions at the final slot;
- late success/failure from an obsolete dial generation;
- peer ID duplication across endpoints without allowing one callback to own
  another endpoint's connection;
- bitfield followed by `have`, omitted bitfield, malformed availability, and a
  peer whose useful pieces become complete;
- choke or disconnect during requested, received-but-not-written, and writing
  states;
- expiry racing a block event, a storage acknowledgment, pause, or shutdown;
- a late valid response after reassignment and an unsolicited never-requested
  block;
- event-queue saturation by non-payload and unsolicited payload messages;
- slow storage with every peer ready to send;
- full choked or irrelevant connection sets with and without replacements;
- discovery arrival while dial slots, connection slots, or request allowance
  are full;
- connection churn at registry and retained-history capacity;
- private metadata learned while DHT-only dials or connections are active;
- cancellation before and after each task/socket/storage ownership transfer;
  and
- checked generation/counter exhaustion and time arithmetic.

## Implementation Stages And Intermediate Gates

### Stage 1: adversarial harness and pure state

- Add scripted peer roles for split availability, permanent choke, unrelated
  keepalives, withholding, delayed blocks, handshake silence, disconnect, and
  late discovery.
- Add pure connection, block, attempt, clock, accounting, and scheduler state.
- Prove all limits, stale transitions, one-peer clearing, fair initial request
  distribution, replacement choice, and waiting deadlines without sockets.
- Keep all existing peer-registry and one-piece tests green.

### Stage 2: supervised connection set

- Replace `PeerSession::connection: Option<_>` with the bounded owner and
  generation-indexed connection tasks.
- Fill pending dials concurrently, admit successful handshakes atomically, and
  join failures/cancellation.
- Continue tracker and DHT intake while connections are live.
- Prove slot races, handshake silence, full-capacity replacement, no-alternative
  waiting, and exact task cleanup with scripted sockets.

### Stage 3: torrent-owned content requests

- Move availability/choke handling to connection facts and block assignment to
  the pure scheduler.
- Route request commands and peer events through bounded queues.
- Keep storage writes, verification, selective mapping, checkpoints, and
  publication authoritative in the supervisor.
- Pass DL-C02 through DL-C06 and DL-C20 through DL-C25 for ordinary and
  selective multi-file content.

### Stage 4: observability and independent evidence

- Add bounded typed scheduler facts needed to classify a stall: connection and
  dial counts, useful/unchoked counts, block dispositions, oldest request age,
  next expiry/replacement/discovery deadline, and why no request can issue.
- Update generated contracts only if shared snapshot types change; do not add
  UI presentation.
- Prove ordinary completion and at least one mixed scripted/libtorrent
  multi-peer scenario against pinned libtorrent 2.0.13.
- Attempt one bounded headless public smoke after controlled evidence; retain
  an honest timeout without making it a CI gate.

Each stage ends with formatting, focused clippy/tests, and `git diff --check`.
Commit coherent reviewable slices rather than leaving the ownership rewrite as
one final commit.

## Validation Matrix

### Pure deterministic

- every named state transition, disposition, stale generation, counter, time,
  capacity, replacement, fairness, and payload-accounting invariant;
- generated adversarial event sequences or property tests where they add
  coverage without hiding the failing transition; and
- architecture checks that pure state has no runtime or platform types.

### Scripted runtime

- all scoped DL scenarios with bounded virtual/test timing where possible;
- real loopback fragmentation, backpressure, concurrent dials, connection
  events, storage delay/failure, cancellation, and task/socket cleanup;
- tracker and DHT late-discovery variants; and
- application shutdown and private-metadata transition with active peers.

### Controlled interoperability

- existing libtorrent metadata/content directions remain green;
- RSTorrent completes with a scripted adverse peer and an ordinary libtorrent
  peer both connected; and
- verified bytes, piece hashes, publication, useful peers, limits, elapsed
  milestones, and cleanup are reported headlessly.

### Product and platform gates

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace --no-fail-fast`;
- locked relevant `tests/interop` scenarios;
- web dependency/build and no-launch desktop build;
- Android Rust cross-build for x86_64 and arm64-v8a; and
- generated-contract drift checks if contract types change.

No visible Tauri, Chrome, emulator, or physical-device interaction is required
or authorized by this tactical.

## Non-Goals

- No BEP 6 fast extension, BEP 11 PEX, incoming peer listener, upload/seeding,
  uTP, NAT traversal, or hole punching.
- No endgame duplicate scheduling or core cancel message in the ordinary path;
  those are Tactical 018 unless state-shape evidence requires codec preparation.
- No automatic hash-failure retry, contributor reputation, or ban policy.
- No rarest-first or speed-scored picker claim; deterministic partial-first
  eligible selection is sufficient.
- No persistent peer cache or session schema migration.
- No dynamic VPN/metered policy, bandwidth scheduler, or global multi-torrent
  connection budget.
- No product UI additions and no new remote control surface.
- No public speed threshold.

## Autonomous Execution And Escalation

The implementer is authorized to complete all stages without further approval,
including proportionate in-scope refactoring, adding adversarial cases implied
by the invariants, choosing internal modules and names, tightening limits,
fixing bugs exposed at the same ownership boundary, changing generated types
when required for truthful snapshots, running bounded headless public smokes,
updating owning topics, and committing coherent slices.

Do not stop for an ordinary failing test, internal architecture mismatch with a
reference, conservative numeric choice within the ceilings, public-swarm
timeout, or an implementation discovery already covered by the owner and
invariants. Investigate, fix, record, and continue.

Stop for human direction only if evidence requires materially different
product behavior, a persistence or public compatibility change, a new external
dependency or license posture, destructive handling of user data, visible or
physical-device interaction, or expansion into a stated non-goal whose absence
prevents the stopping condition.

## Stopping Condition

This tactical is complete when DL-C02 through DL-C06 and DL-C20 through DL-C25
pass through one bounded torrent-owned connection and request owner; the
scripted matrix proves split availability, permanent choke, request silence,
disconnect, expiry/reassignment, late generations, slot replacement,
no-alternative waiting, churn bounds, cancellation, and exact joins; a
controlled libtorrent peer participates in verified multi-peer completion; all
existing storage/resume/tracker/DHT evidence and product build gates remain
green; and bounded typed state can classify rather than merely display a
future near-completion stall.

Endgame duplicates, cancel messages, automatic hash retry/reputation, and
measured picker/connection tuning remain explicit subsequent tacticals.
