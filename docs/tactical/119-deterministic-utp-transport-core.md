# Tactical 119: Deterministic uTP Transport Core

Status: In progress on 2026-08-10 as the single authoritative **Now** after
human acceptance of Tactical `118`'s independently authored Rust
recommendation.

Topics: `utp-transport-campaign`, `capability-readiness`,
`oracle-driven-engine-campaign`, `protocol-support`, `peer-lifecycle`

Dependencies: completed Tactical
[`118`](118-utp-implementation-decision-spike.md) owns the exact source,
license, platform, ownership, and forced-uTP oracle dossier. Completed
Tacticals [`112`](112-dual-stack-transport-and-ipv6-dht.md) and
[`111`](111-mse-peer-stream-encryption.md) own the future shared-UDP generation
and ordered-stream/MSE boundaries. This tactical does not modify either
runtime owner.

## Decision And Desired Outcome

Implement an independently authored, runtime-free uTP v1 protocol component
inside `rstorrent-protocol`. It must turn hostile datagrams and explicit
monotonic time into bounded deterministic state transitions without Tokio,
sockets, tasks, channels, filesystem access, entropy, peer-wire behavior, or
platform code.

The slice owns enough transport reliability to make the difficult state shape
real before congestion or runtime integration:

1. exact v1 header and extension decoding/encoding;
2. explicit wrapping sequence and timestamp arithmetic;
3. connection IDs and initiating/accepting handshake transitions;
4. bounded receive ordering, duplicate handling, SACK generation, FIN, and
   RESET state;
5. bounded sent-packet ownership, cumulative/SACK acknowledgement, duplicate-
   ACK and three-later-ACK loss signals, Karn-safe RTT sampling, and RTO timer
   intents; and
6. exact resource snapshots and terminal cleanup.

This is not a minimal happy-path codec. Malformed extension chains, sequence
wrap, stale/future ACKs, reorder pressure, duplicate packets, missing data
around FIN, retransmission sampling, timeout backoff, and limit rollback are
part of the initial state shape.

## Stopping Condition

The tactical is complete only when:

1. the pure packet codec validates the complete datagram before returning a
   borrowed view and encoding enforces the same limits;
2. the connection, receive, and send state pass deterministic common-path,
   adversarial, wrap, loss, timeout, teardown, and resource-bound tests;
3. malformed or over-limit input makes no partial state change;
4. every stored payload byte and packet is visible through a snapshot, and
   reset/terminal drain returns those counters to zero;
5. the full Rust formatting, clippy, and workspace test baseline passes; and
6. the owning campaign, readiness queue, and protocol-support truth are
   reconciled without claiming runtime or BEP 29 support.

## Exact Resource And Work Bounds

- One decoded datagram is at most 65,535 bytes. Decoding borrows the input and
  performs no payload copy.
- One packet has at most eight linked extensions. Each extension length is the
  wire `u8`; a SACK is 4--252 bytes and a multiple of four. A second SACK is
  rejected as ambiguous. Unknown extensions within the chain bound are
  retained and skipped safely.
- One connection stores at most 64 out-of-order receive packets and 1 MiB of
  out-of-order payload. Its generated SACK is at most eight bytes because only
  the accepted 64-packet reorder horizon is advertised.
- One connection stores at most 1,024 sent sequence-bearing packets and 1 MiB
  of their payload. The caller supplies already segmented payload; this slice
  owns no additional unsent user-data queue.
- One input can release at most the current packet plus all 64 reorder packets.
  All acknowledgement and loss scans are bounded by the 1,024-packet send
  ledger; extension work is bounded by eight headers and 252 SACK bytes.
- A packet has at most eight recorded transmissions. Initial RTO is one second,
  the BEP floor is 500 ms, and exponential timeout intent saturates at 60
  seconds. Time is a caller-supplied monotonic `u64` microsecond value.
- This slice constructs one connection at a time and therefore owns no
  endpoint map or half-open collection. The first runtime tactical must set
  session/per-endpoint half-open and established-connection limits before
  accepting UDP traffic.

Every rejected limit reports the actual and maximum values. Checked updates
reserve or validate all required capacity before mutating sequence,
acknowledgement, lifecycle, or byte counters.

## Invariants And State Shape

- `rstorrent-protocol` remains `#![forbid(unsafe_code)]` and gains no
  dependency.
- Header fields are network byte order. Only version 1 and packet types DATA,
  FIN, STATE, RESET, and SYN are accepted.
- DATA must carry payload. SYN, STATE, and RESET must not. For deployed
  compatibility, FIN payload is accepted as the final ordered payload and is
  counted explicitly; this differs from BEP 29's payload-free FIN and is
  independently tested.
- A wrapping sequence comparison reports the exact half-range ambiguity rather
  than inventing a total order. Connection windows are far below that range;
  ambiguous or out-of-window input is ignored without mutation.
- The caller supplies initial local sequence numbers. Tests use BEP 29's value
  one where normative behavior matters; a future entropy-owning runtime may
  use a random value as libtorrent does without putting randomness in the core.
- STATE carries the next local sequence number but does not consume it,
  following BEP 29's packet-type rule and deployed libtorrent behavior rather
  than the contradictory increment shown in the BEP setup diagram.
- Cumulative ACKs never advance beyond the highest sequence-bearing packet
  sent. Impossible future ACKs and excessively stale ACKs are ignored, matching
  the oracle's injection-resistant behavior.
- RTT samples come only from packets transmitted exactly once. ACK progress
  resets timeout backoff; retransmitted packets cannot contaminate the
  estimator.
- Three duplicate STATE acknowledgements or three acknowledged later packets
  produce a loss signal at most once per outstanding packet until it is
  explicitly retransmitted.
- Receiving RESET or performing terminal cleanup releases all send and receive
  payload ownership. No old state can mutate a new connection object.

## Owner And Dependency Direction

The implementation is split only where invariants differ:

- `utp::packet` owns the hostile wire boundary and borrowed extension/payload
  views;
- `utp::sequence` owns wrapping arithmetic;
- `utp::receive` owns receive order, buffered payload, SACK, FIN, and delivery;
- `utp::send` owns sent payload, ACK/loss accounting, RTT, and timer intent;
  and
- `utp::connection` owns IDs, handshake/lifecycle composition, and delegates
  reliability state inward.

No protocol module depends on an engine type. The later runtime may depend on
these outputs, the session UDP owner, and Tokio; dependency direction never
reverses.

## Source-First Record

The source and license dossier in Tactical `118` remains authoritative. This
slice rechecked the exact relevant behavior in:

- managed BEP 29's v1 header, extension/SACK, packet-type, connection setup,
  loss, timeout, packet-size, and congestion sections;
- pinned libtorrent
  `utp_socket_impl::{send_syn,send_pkt,incoming_packet,parse_sack,ack_packet,
  packet_timeout,tick}` and the receive/outgoing buffer fields in
  `utp_stream.hpp`;
- pinned libtorrent `test/test_utp.cpp` wrapping and forced-uTP cases plus the
  hostile entry shape in `fuzzers/src/utp.cpp`;
- standalone libutp's wrapping buffers, parser, SACK, timeout, and callback
  pump as a secondary behavior check; and
- librqbit-utp's raw codec, `SeqNr`, recovery, RTT estimator, receive assembly,
  send segments, and shutdown tests as a secondary edge-case inventory.

No reference source, constants table, fixture, or test vector is copied. Tests
are independently authored from public wire behavior and the edge cases named
above.

## Validation

Focused deterministic tests cover at least:

- exact header bytes, every packet type, truncation at every boundary,
  unsupported version/type, packet-size limit, extension truncation/count,
  unknown extensions, duplicate/invalid SACK, and SACK bit ordering;
- sequence/timestamp wrap, exact half-range ambiguity, connection-ID wrap, and
  initiator/acceptor handshake IDs and acknowledgement;
- in-order, duplicate, reordered, far-future, over-packet, and over-byte receive
  behavior; contiguous release; SACK generation; FIN before missing data; FIN
  payload compatibility; RESET; and terminal zero ownership;
- sent packet/byte bounds and rollback, ACK wrap, stale/future ACKs, cumulative
  and selective ACKs, duplicate-ACK and SACK loss signals, one-shot loss,
  retransmit reset, Karn filtering, RTT/RTO updates, exponential saturation,
  and attempt exhaustion; and
- state snapshots before and after every failure or terminal transition.

Run:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

No Python oracle, client, Android, WAN, or physical-device run is required
because this slice has no runtime or application path. The retained forced-uTP
oracle remains the later runtime acceptance peer.

## Non-Goals

- No LEDBAT, congestion window, pacing, slow start, bandwidth fairness,
  receive-window policy, packet sizing, Nagle policy, path-MTU discovery, UDP
  send/receive, retransmission execution, delayed-ACK scheduling, or impaired-
  network simulator.
- No endpoint/socket manager, shared-UDP classification, task, channel,
  cancellation token, generation replacement, peer stream, BitTorrent
  handshake, MSE composition, DHT change, or application setting.
- No TCP/uTP selection, racing, fallback, advertisement, port mapping, IPv6
  pinhole, WAN, `pimom`, public swarm, or support claim.
- No source copying, mechanical translation, vendoring, FFI, dependency, or
  third-party notice change.

## Escalation

Ordinary internal representation, pure-test, and bounded-state decisions
inside this tactical proceed autonomously. Stop for direction before adding a
dependency, accepting foreign source, changing the shared UDP/runtime owner,
moving LEDBAT or real networking into this slice, or weakening a recorded
resource bound to obtain a passing test.

## Execution Record

In progress. Implementation commits and exact validation evidence will be
appended as the bounded components land.
