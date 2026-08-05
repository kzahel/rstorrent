# Tactical 093: BEP 6 Fast Request Lifecycle

Status: Planned. This document records a decision-complete candidate slice;
it does not authorize implementation or displace the current truthful tracker
and DHT advertisement work.

Topics: `protocol-support`, `download-correctness`, `peer-lifecycle`,
`incoming-reachability-and-seeding`, `capability-readiness`

Dependencies: completed Tacticals
[`017`](017-adversarial-multi-peer-liveness.md),
[`023`](023-strict-endgame-ownership.md),
[`078`](078-local-single-peer-tcp-seeding.md),
[`082`](082-bounded-multi-peer-upload-ownership.md), and
[`086`](086-long-lived-torrent-peer-runtime.md) establish exact request
attempts, cancel/late-response handling, bounded incoming upload, and shared
connection observations. Planned Tacticals
[`090`](090-peer-id-duplicate-connection-resolution.md) and
[`091`](091-availability-ranked-piece-activation.md) precede this slice in the
recorded campaign sequence but are not wire-protocol dependencies.

## Decision And Motivation

Implement the negotiated BEP 6 Fast Extension as one complete request-
lifecycle slice, with explicit rejection as the primary liveness outcome.

Adding message opcodes alone would be incorrect. When Fast is negotiated, each
request has exactly one terminal wire response: the corresponding `Piece` or
`Reject Request`, including cancellation races. `Choke` no longer implicitly
rejects pending requests. RSTorrent currently releases a download connection's
requests immediately on `Choke`, clears queued upload requests when revoking a
grant, and silently ignores new requests while choking. Those semantics must
remain unchanged for ordinary BEP 3 peers and change coherently for negotiated
Fast peers.

RSTorrent must not advertise the Fast reserved bit until all request, choke,
cancel, availability, allowed-fast, validation, and cleanup invariants in this
tactical are installed on both outgoing and incoming connections.

This tactical owns stable correctness scenario DL-C31 in
[`download-correctness`](../topics/download-correctness.md).

## Stopping Condition

This tactical is complete when all of the following hold:

1. Fast is enabled only when both handshakes set `reserved[7] & 0x04`, and all
   five Fast messages have strict bounded codecs and negotiated-state checks;
2. every valid Fast request received by the uploader produces exactly one
   matching `Piece` or `Reject Request` across choke, cancel, read completion,
   grant revocation, resource pressure, pause, and shutdown races;
3. a download request remains owned across `Choke` until an exact reject,
   piece, disconnect, or bounded timeout terminates it, and a reject releases
   and refills work immediately without waiting for timeout;
4. Have All, Have None, and Bitfield obey the exactly-one initial availability
   rule and integrate with existing availability accounting;
5. allowed-fast requests can proceed while choked only for the connection's
   bounded canonical set, while suggestions remain bounded advisory picker
   input and neither message is mistaken for availability;
6. unnegotiated, malformed, impossible, never-requested, duplicate, stale, and
   excessive Fast inputs have explicit protocol outcomes without leaking
   request or upload resources; and
7. deterministic, scripted race, and controlled bidirectional pinned-
   libtorrent evidence pass with exact request, response, byte, and terminal
   owner counts.

## Scope

### Wire and negotiation

- Add `Suggest Piece` (13), `Have All` (14), `Have None` (15), `Reject
  Request` (16), and `Allowed Fast` (17) protocol values and exact message
  lengths.
- Add a typed negotiated Fast capability derived from both handshakes rather
  than exposing raw reserved bytes to scheduler and upload policy.
- Reject Fast messages received without negotiation and invalid initial
  availability ordering through the existing typed protocol-close path.
- Carry the capability through outgoing downloads, incoming downloads, and
  incoming/outgoing upload relationships owned by the ordinary connection
  runtime.

### Download request lifecycle

- Preserve pending request attempts on negotiated Fast `Choke`; stop issuing
  ordinary new requests while choking but await exact reject/piece outcomes.
- Match `Reject Request` to the full piece/begin/length request identity and
  the current connection generation.
- On a valid rejection, terminate exactly that attempt, release its request
  and byte reservations, remove rejected suggestion/allowed-fast preference
  where applicable, and immediately make the block eligible elsewhere.
- Keep the existing bounded request timeout as a compatibility fallback for a
  broken Fast peer. Fast negotiation does not authorize an infinite wait.
- Close on a rejection for a request that was never sent. A late response for
  a retained terminal attempt follows one explicit bounded policy and cannot
  terminate a newer attempt.

### Upload request lifecycle

- When choking a Fast peer, enqueue `Choke` before rejects for all queued
  requests that will not be served, except eligible allowed-fast requests.
- A request received while choking is rejected unless its piece is currently
  allowed fast and resource policy accepts it.
- Give every accepted request a terminal-response state so cancellation,
  asynchronous read completion, grant revocation, and writer backpressure
  cannot produce zero or two responses.
- Resource limits may reject rather than serve a valid request. Egregious
  continued request flooding after choke may still close the connection under
  the existing bounded policy.

### Availability, suggestions, and allowed fast

- Emit exactly one of Bitfield, Have All, or Have None immediately after the
  handshake on Fast connections. Ordinary connections retain existing BEP 3
  behavior.
- Retain at most 32 unique received suggestions and 32 unique received
  allowed-fast indices per connection. Valid extra advisory messages may be
  ignored once full; duplicates do not grow state.
- Suggestions rank only eligible pieces after active/partial work and before
  ordinary equal-rarity ties. They cannot bypass file selection, availability,
  active memory, integrity, or request bounds.
- Generate the IPv4 allowed-fast set with BEP 6's canonical algorithm and
  initial `k = 10`, capped by piece count. BEP 6 defines no IPv6 generation;
  IPv6 Fast connections retain reject and availability semantics but do not
  emit an allowed-fast set in this slice.
- Allowed Fast never proves that the sender has a piece.

## Non-Goals

- Super-seeding, tit-for-tat download choking, upload ratio/time goals, or a
  new global upload policy.
- Predictive piece requests, streaming/deadline priority, or changing ordinary
  request-window targets.
- IPv6 allowed-fast algorithm invention or a protocol extension beyond the
  accepted BEP 6 text.
- General BEP 10 registry work, PEX, BEP 40, hole punching, or uTP.
- Advertising partial Fast support or promoting the protocol matrix before
  controlled independent interoperability passes.

## Normative And Reference Dossier

Pinned BEP revision `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`
was inspected at `reference/bittorrent.org/beps/bep_0006.rst`. Shape-changing
requirements include bilateral negotiation, exactly one piece/reject response,
choke no longer implying rejection, choke-before-reject ordering, the exact
initial availability choice, unnegotiated-message closure, and canonical
IPv4 allowed-fast generation.

Pinned libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

- `reference/libtorrent/src/bt_peer_connection.cpp` owns Fast message parsing,
  emission, negotiation checks, bitfield/have-all/have-none choice, and cancel
  behavior;
- `reference/libtorrent/src/peer_connection.cpp::incoming_reject_request`
  validates and removes an exact request before refilling the picker;
- `reference/libtorrent/test/test_fast_extension.cpp` covers rejected allowed-
  fast work, predictive request rejection, invalid suggestions, availability
  messages, and upload request validation; and
- `reference/libtorrent/include/libtorrent/settings_pack.hpp` records the
  reference allowed-fast set size and related bounds.

Adopt the complete lifecycle checklist and interoperability behavior. Do not
copy libtorrent's queue representation, predictive requests, plugin hooks,
alert system, class ownership, or unrelated settings.

The first-party JSTorrent checkout implements bilateral Fast negotiation for
Have All/Have None in
`packages/engine/src/core/torrent-peer-handler.ts`, but no complete reject-
request or allowed-fast lifecycle was found. This is useful product-history
evidence that availability-only Fast support is not the target. No source or
fixtures are imported.

## Owner, State, And Dependency Map

```text
peer-wire Fast values and deterministic codecs
                    |
validated bilateral handshake capability
                    |
        +-----------+-----------+
        v                       v
pure download swarm state     pure upload state
request attempts/rejections   terminal response per request
choke/timeout/refill          choke/reject/read/cancel order
        |                       |
        +-----------+-----------+
                    v
ordinary connection task writer and joined supervisor
```

- The protocol crate owns message values, exact lengths, and capability bits.
- Pure swarm state owns download attempt transitions, availability, suggestion
  preference, and bounded timeout fallback.
- Pure upload state owns request admission and exactly-one terminal-response
  decisions; asynchronous storage reads remain outer actions identified by
  generation.
- Connection tasks own ordered socket writes but cannot invent reject policy or
  release torrent request state directly.
- The torrent supervisor owns task cancellation, storage calls, accounting,
  and observable joins.

## Core Invariants And Bounds

- Negotiation is connection-generation local and never inferred from client
  name or a later extension handshake.
- A request has at most one active attempt per connection outside strict
  endgame and at most one terminal Fast response from the uploader.
- A reject releases exactly one matching request reservation. It cannot release
  a different block, newer generation, or already accepted storage write.
- `Choke` precedes generated rejects in the writer action order.
- Suggestions and allowed-fast sets are unique, piece-index validated, bounded
  to 32 retained values each, and cleared with the connection generation.
- Canonical generated allowed-fast output contains at most ten unique indices.
- Existing frame, event queue, request, read, writer-descriptor, payload, and
  connection limits remain authoritative; this tactical adds no task.
- Ordinary non-Fast peer behavior remains byte-for-byte and transition-for-
  transition compatible except for shared refactoring proven by regression
  tests.

## Implementation Gates

1. Add codecs, handshake capability, and strict negotiated-state validation
   independently from sockets.
2. Add downloader exact-reject ownership while retaining the current timeout
   fallback and non-Fast choke behavior.
3. Add uploader terminal-response state and ordered choke/reject behavior
   across asynchronous reads and cancellation.
4. Add availability choice, canonical allowed-fast generation, and bounded
   advisory state/picker integration.
5. Run the full deterministic and scripted race matrix before advertising the
   reserved bit in ordinary handshakes.
6. Prove both RSTorrent-to-libtorrent directions and update the BEP 6 claim
   only to the exact evidence-supported state.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Codec/state | all message lengths/opcodes, bilateral bit, unnegotiated messages, initial availability ordering, canonical published vectors, duplicate/excess advisory input |
| Download transitions | choke then reject, choke then piece, cancel races, never-sent reject, stale reject, timeout fallback, immediate reassignment, endgame losing attempt |
| Upload transitions | choke with queued and in-flight reads, allowed-fast exception, request while choked, cancel before/after read completion, resource rejection, pause/shutdown, exactly one wire response |
| Scripted runtime | fragmented/coalesced frames, writer backpressure, delayed storage, disconnect at every transition, bounded flooding, exact queues and joins |
| Controlled interoperability | RSTorrent leeches from and seeds to pinned libtorrent with Fast negotiated; explicit rejection avoids waiting for request timeout and exact payload verifies |
| Resource evidence | advisory sets, request attempts, upload terminal states, queued frames/bytes, descriptors, tasks, and terminal owner counts stay within declared bounds |

No public swarm, visible client, schema migration, dependency, or physical
device is required.

## Escalation And Next Boundary

Ordinary refactoring of protocol values and request states, additional race
cases, and tighter advisory bounds are authorized only when this tactical is
explicitly selected for implementation. Stop for direction if interoperability
requires advertising a knowingly partial Fast package, inventing IPv6
allowed-fast behavior, changing global upload policy, or weakening exact
request ownership.

The next planned protocol slice is
[`094`](094-bounded-bep11-peer-exchange.md). Predictive requests,
super-seeding, full snub behavior, and parole isolation remain separate work.
