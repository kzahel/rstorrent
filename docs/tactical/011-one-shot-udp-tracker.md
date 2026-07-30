# Tactical 011: One-Shot UDP Tracker Discovery

Status: completed on 2026-07-30.

## Motivation And Outcome

Tactical `010` established the peer observations, bounded registry, selector,
dial attempts, failure history, and same-connection magnet path needed to
accept new discovery sources cleanly. Ordinary magnets still cannot enter
that path without an explicit `x.pe` hint.

Add the smallest complete BEP 15 discovery slice. A tracker-only v1 magnet
retains one bounded loopback UDP tracker URL. The engine performs one
correlated connect exchange and one started announce, validates a bounded
compact response, emits tracker observations into the peer registry, and
downloads verified multi-block metadata plus hash-verified content from the
discovered libtorrent seed.

The outcome proves the tracker-to-registry architecture and a public-style
magnet shape without adding periodic policy, public-network support, or
multi-peer transfer.

## Dependencies And References

- [`../topics/tracker-discovery.md`](../topics/tracker-discovery.md)
- [`../topics/peer-lifecycle.md`](../topics/peer-lifecycle.md)
- [`../topics/product-direction.md`](../topics/product-direction.md)
- [`../engineering-principles.md`](../engineering-principles.md)
- [`006-magnet-metadata-peer-hint.md`](006-magnet-metadata-peer-hint.md)
- [`010-peer-registry-magnet-failover.md`](010-peer-registry-magnet-failover.md)
- BEP 15: UDP Tracker Protocol for BitTorrent
- Rasterbar libtorrent `v2.0.13` UDP tracker client, tracker manager, and
  loopback tracker tests
- Current local JSTorrent UDP tracker, tracker manager, announce-statistics,
  and simple-tracker tests
- [`../test-torrents.md`](../test-torrents.md) for representative
  tracker-only public magnet shapes

No source or fixture is copied. Protocol tests and controlled tracker peers
are independently authored from BEP 15 packet behavior.

## Reference Findings

- BEP 15 uses a 16-byte connect request/response before the 98-byte IPv4-form
  announce request. All integers use network byte order.
- Transaction IDs correlate responses. Connect and announce use distinct
  transactions; error action `3` may answer either operation.
- Connect responses are at least 16 bytes. Announce responses are at least
  20 bytes followed by six-byte IPv4 or eighteen-byte IPv6 compact peers,
  selected by the UDP packet address family.
- Connection IDs are reusable by a client for 60 seconds and accepted by a
  tracker for 120 seconds. This one-shot operation does not live long enough
  to need a cache.
- BEP 15 recommends exponential retransmission beginning at 15 seconds.
  Libtorrent combines receive and completion deadlines and tries alternate
  resolved endpoints. Full retransmission is deferred here, but every
  operation and alternate endpoint remains bounded by the outer diagnostic
  deadline.
- Libtorrent asks for 200 peers by default and reports 16 KiB left when
  magnet metadata has not established torrent size. JSTorrent instead uses a
  much larger unknown-left sentinel. RSTorrent follows libtorrent's smaller,
  deployed behavior.
- Libtorrent ignores packets from unexpected sources, stale transactions, and
  packets shorter than a response header, while treating a malformed response
  correlated to the active operation as a tracker failure.

## Scope

### Magnet tracker values

Extend the pure magnet value with at most 32 distinct UDP tracker URLs:

- `tr` remains an ordinary bounded magnet query parameter;
- the scheme is ASCII case-insensitive `udp`;
- an explicit nonzero port is required;
- hostnames, canonical IPv4, and bracketed IPv6 follow the established
  bounded peer-host rules;
- credentials, fragments, queries, ambiguous IPv6, and non-ASCII input are
  rejected;
- an empty path, `/`, or conventional `/announce` is accepted and normalized;
- malformed and unsupported tracker parameters are ignored individually;
- repeated equivalent URLs deduplicate without changing first-seen order; and
- the valid supported-tracker count, not attacker-supplied invalid entries,
  controls the 32-item limit.

The value stores host and port rather than a runtime socket address. DNS and
the loopback diagnostic policy remain outside protocol state.

### Pure BEP 15 codec

Add a runtime-independent UDP tracker module with:

- connect request encoding and connect/error response parsing;
- started announce request encoding;
- IPv4 and IPv6 announce/error response parsing;
- explicit transaction, expected-action, minimum-length, stride, and
  peer-count validation;
- a maximum of 200 compact peers and a maximum 512-byte UTF-8 error message;
- checked integer and slice handling without panics on arbitrary datagrams;
  and
- exact fixed request sizes with future connect-response bytes tolerated as
  BEP 15 requires.

The parser returns compact socket endpoints but does not decide whether they
are valid peer records.

### One-shot runtime owner

Extend the diagnostic peer session with an ordered bounded UDP-tracker queue.
When no peer is eligible:

1. Resolve the next tracker URL.
2. Discard non-loopback results under the existing diagnostic restriction.
3. Try each remaining resolved address sequentially.
4. Bind and connect one address-family-matched Tokio UDP socket.
5. Send a connect request with a supplied random nonzero transaction.
6. Ignore undersized and stale-transaction packets until a fixed bounded
   response deadline.
7. Send one started announce with a new transaction, stable random key,
   `num_want = 200`, `left = 16 KiB`, zero transfer counters, zero IP, and
   port zero because this diagnostic owns no incoming listener.
8. Reject an oversized or malformed correlated response.
9. Translate valid, loopback compact endpoints into tracker observations.
10. Return to the existing selector and dial lifecycle.

Tracker URL, address, or protocol failure advances to the next resolved
address or tracker. A response with zero usable peers also advances. Explicit
peer hints remain observations in the same registry and may succeed without
causing tracker traffic.

Rename the diagnostic magnet entry points that say `with_peer_hint`; tracker
support makes that suffix incorrect. This repository has no public
compatibility commitment, and all in-tree callers move atomically.

### Controlled evidence

Add a scripted loopback UDP tracker that independently inspects RSTorrent's
connect and announce packets and returns:

- stale-transaction packets before correlated responses;
- invalid and duplicate compact endpoints;
- an unreachable loopback endpoint first; and
- one live scripted metadata/content seed.

A tracker-only magnet must retain only tracker-sourced valid peers, fail over
from the unreachable record, keep the successful socket across metadata and
content, verify exact content, and terminate the UDP tracker and TCP peer
tasks.

Add an interoperability harness where a small independent Python UDP tracker
returns a controlled libtorrent `2.0.13` seed. RSTorrent receives no `x.pe` or
metainfo bytes out of band. Exact info hash, multi-block metadata, content,
request fields, payload hash, process termination, and cleanup are required.

## Contracts And Invariants

- Tracker URLs and packets are hostile bounded protocol input.
- Trackers produce observations and never dial peers or own peer records.
- Tracker counts and claimed seed/leecher status do not establish peer facts.
- One UDP operation owns one socket and has an observable terminal result.
- Only packets from the connected tracker endpoint can enter parsing.
- Stale transactions cannot complete or mutate the current operation.
- A malformed correlated packet cannot add a partial peer set.
- Response allocation is bounded independently from UDP datagram size and
  swarm size.
- Invalid, non-loopback, and duplicate peer endpoints cannot bypass registry
  validation or capacity.
- Tracker failure cannot corrupt existing peer records or prevent another
  source from being selected.
- Existing metadata authorization, payload memory, storage, hashing,
  publication, cancellation, and cleanup invariants remain authoritative.

## Nasty Cases Required Up Front

- empty, malformed, unsupported, duplicate, overlong, credentialed,
  query-bearing, fragmented, missing-port, zero-port, bracketless IPv6, and
  non-ASCII tracker URLs;
- supported tracker-count exhaustion mixed with unlimited invalid `tr`
  parameters under the existing total query bound;
- exact request byte layout and boundary integer values;
- response lengths from zero through each required header boundary;
- stale transaction, unexpected action, tracker error, invalid UTF-8 error,
  oversized error, future connect trailing bytes, and announce bad stride;
- zero peers, exactly 200 peers, and a 201st peer without an oversized
  allocation;
- IPv4 and IPv6 compact peers;
- zero-port, unspecified, multicast, non-loopback, and duplicate endpoints;
- DNS failure, only non-loopback resolution, alternate resolved-address
  failure, UDP response timeout, and all trackers exhausted;
- an explicit hint succeeding without a tracker announce;
- unreachable tracker-discovered peer followed by a successful peer; and
- timeout or cancellation while waiting for tracker traffic without a leaked
  socket or task.

## Non-Goals

- BEP 15 retransmission, connection-ID caching, scrape, authentication, or
  BEP 41 extensions
- interval scheduling, reannounce, stopped/completed events, tiers, failure
  backoff, tracker persistence, or a permanent tracker manager
- HTTP, HTTPS, WebSocket, or proxy tracker transports
- public tracker or peer networking in the supported diagnostic
- incoming peer listening or advertising a nonzero listen port
- DHT, PEX, LSD, NAT traversal, uTP, or WebSeeds
- simultaneous tracker operations, dials, or peer connections
- content-transfer failover or multi-peer piece scheduling
- application tracker snapshots, settings, or remote-control surface
- claims that the public catalog's current tracker or swarm health is stable

## Validation

Run:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv run --project tests/interop \
  python tests/interop/udp_tracker_magnet.py --runs 3
uv run --project tests/interop \
  python tests/interop/magnet_metadata.py --runs 1
python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

Run the established Android target checks if the dependency graph changes.
Focused development runs may select tracker codec, magnet parser, and scripted
engine tests. The execution record must distinguish them from the final
baseline.

## Stopping Condition

Stop when a tracker-only loopback magnet performs a bounded BEP 15 exchange,
turns only validated compact results into tracker observations, fails over
from the first unavailable peer, obtains verified multi-block metadata and
hash-verified content from the next peer over one connection, passes three
controlled libtorrent-seed runs, leaves no socket/task/artifact behind, and
the topic and execution record state exact support, evidence, and deferrals.

## Execution Record

Completed on 2026-07-30.

### Landed behavior

- Bounded magnet parsing now retains up to 32 distinct normalized UDP tracker
  endpoints. It accepts explicit-port hostname, IPv4, and bracketed IPv6
  forms with an empty path or `/announce`, while independently ignoring
  malformed and unsupported `tr` values.
- Durable session canonicalization preserves normalized peer hints and UDP
  trackers. A tracker-only source therefore remains usable after passing
  through the existing SQLite torrent catalog.
- Added a runtime-independent BEP 15 codec with exact connect and announce
  request encoding, correlated connect/error and IPv4/IPv6 announce response
  parsing, checked response geometry, a 200-peer ceiling, and bounded tracker
  error text.
- `DiagnosticPeerSession` now owns an ordered tracker queue and creates its
  random nonzero announce key only when tracker discovery is actually needed.
  Explicit peer hints remain lazy-first and do not cause tracker traffic when
  one succeeds.
- When peer selection is empty, the runtime resolves the next tracker,
  retains only loopback addresses, tries each address sequentially, and owns
  one connected family-matched UDP socket through connect and started
  announce. Separate nonzero transactions, exact source filtering by the
  connected socket, stale-packet handling, correlated-malformation failure, a
  fixed response deadline, and a maximum-size receive buffer bound the
  operation.
- A successful response turns only valid loopback compact endpoints into
  `PeerSource::Tracker` observations. Deduplication, capacity, selection,
  failure history, TCP dialing, metadata acquisition, and same-connection
  content transfer remain owned by the existing peer lifecycle.
- Generic magnet entry points replaced the obsolete `with_peer_hint` names
  atomically across the engine, session service, CLI, and Android adapter.
  Android failure classification now distinguishes tracker peer, timeout, and
  runtime-entropy failures without adding tracker fields to product views.
- Added an independent Python UDP tracker interoperability harness which
  supplies a tracker-only magnet to RSTorrent and returns a controlled
  libtorrent seed without out-of-band peer or metainfo data.

### Evidence

Pure protocol tests cover exact request fields, response lengths at every
header boundary, stale transactions, unexpected actions, bounded tracker
errors, future connect bytes, invalid compact strides, IPv4 and IPv6 parsing,
and the exact 200/201-peer boundary. Magnet tests cover URL normalization,
deduplication, every rejected URL class, invalid entries mixed with the valid
capacity, and the 32/33-tracker boundary.

The scripted engine path first receives a correlated tracker rejection, then
advances to a second tracker. That tracker independently inspects the connect
and announce bytes, sends undersized and stale packets, and reports a
zero-port peer, an unreachable peer, a duplicate reachable peer, and a
non-loopback peer. The registry retains exactly the two usable tracker
records, records one connect failure, and completes verified metadata and
content through the live record. Separate evidence confirms that successful
explicit hints cause no tracker traffic and that both outer timeout and
explicit cancellation release the UDP socket without leaving output or
staging files.

Three controlled libtorrent `2.0.13.0` runs each:

- issued exactly one 16-byte connect and one 98-byte announce request;
- acquired the 26,686-byte info dictionary in two metadata blocks;
- matched info hash
  `a962f460b83861cfb5faa1d7ad7da9c3f3cc2fc4`;
- verified all three content pieces and the exact 40,000-byte payload; and
- terminated the tracker, peer, and client with cleanup reported as `ok`.

The established direct-hint libtorrent baseline also remained green with the
same metadata, content, and cleanup evidence.

### Validation run

The following completed successfully:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo ndk -t x86_64 -t arm64-v8a -P 28 \
  build --release -p rstorrent-session --lib
uv run --project tests/interop \
  python tests/interop/udp_tracker_magnet.py --runs 3
uv run --project tests/interop \
  python tests/interop/magnet_metadata.py --runs 1
python3 scripts/references.py status
cargo tree --workspace --locked
git diff --check
```

The final workspace run passed 40 engine-library tests, 50 protocol tests plus
the architecture test, 23 session-library tests, and all other workspace,
binary, gateway, Android-adapter, and documentation tests. Both established
Android Rust targets cross-compiled with the new direct `getrandom 0.3.4`
dependency at the API 28 floor. All pinned reference checkouts matched their
recorded revisions.

### Deliberate limits and next boundary

This remains a diagnostic, loopback-only, one-shot operation. It does not
retransmit, cache connection IDs, schedule reannounces, honor tiers, advertise
an incoming port, or support public, HTTP-family, authenticated, or proxied
trackers. Tracker interval and swarm counts are parsed but do not establish
policy or peer facts. There is still only one live TCP peer, no content
failover after metadata handoff, and no multi-peer piece ownership.

The next peer-focused slice should establish a small bounded set of live peer
connections and explicit content-request ownership/failover before optimizing
selection or adding broad discovery scheduling. The next tracker-specific
slice, when evidence requires it, should introduce a per-torrent tracker
record and scheduled announce lifecycle rather than extending this one-shot
owner piecemeal.
