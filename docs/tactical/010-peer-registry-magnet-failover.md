# Tactical 010: Peer Registry Magnet Failover

Status: completed on 2026-07-30.

## Motivation And Outcome

Tactical `006` proved magnet metadata and content transfer from explicit
`x.pe` hints, but the runtime still resolves and dials hints through three
separate address loops. Those loops have no durable peer vocabulary, bounded
record owner, source accumulation, explicit dial transition, or reusable
selection seam for trackers.

Establish the first peer lifecycle without adding another discovery protocol.
Every resolved magnet hint becomes a `PeerObservation`, observations merge
into a bounded `PeerRegistry`, `PeerSelector` derives eligible endpoints, and
one identified `DialAttempt` may become the existing live peer transport,
renamed `PeerConnection`.

The concrete outcome starts from a magnet whose first loopback endpoint is
unreachable and whose second endpoint is a scripted metadata/content seed.
The failed record enters backoff, the selector advances to the second record,
and the same successful connection obtains verified metadata and one
hash-verified content piece.

## Dependencies And References

- [`../topics/peer-lifecycle.md`](../topics/peer-lifecycle.md)
- [`../topics/product-direction.md`](../topics/product-direction.md)
- [`../engineering-principles.md`](../engineering-principles.md)
- [`006-magnet-metadata-peer-hint.md`](006-magnet-metadata-peer-hint.md)
- Rasterbar libtorrent `v2.0.13` `torrent_peer`, `peer_list`, peer-source,
  candidate-selection, and connection-lifecycle behavior
- Current local JSTorrent `PeerAddress`, `SwarmPeer`, `Swarm`,
  `PeerSelector`, `ConnectionManager`, and `PeerConnection` decomposition

No source or fixture is copied. The implementation and tests are independently
authored against the established RSTorrent magnet diagnostic.

## Scope

### Runtime-independent peer state

Add a coherent engine peer module containing:

- validated `PeerEndpoint` values;
- `PeerSource` and accumulating `PeerSources`;
- `PeerObservation` with explicit advertised reachability;
- stable `PeerRecordId` and `DialAttemptId` values;
- `PeerRecord`, `PeerHistory`, `PeerPhase`, and bounded failure kinds;
- configurable bounded `PeerRegistry`;
- explicit observation add/merge/capacity outcomes;
- deterministic `PeerSelector`, `PeerSelectionContext`, derived
  `DialEligibility`, and temporary `DialCandidate`;
- begin, success, failure, and connection-close transitions guarded by the
  matching attempt identity; and
- deterministic reconnect backoff supplied with an explicit time input.

This state does not depend on Tokio, sockets, DNS, filesystems, channels,
tasks, wall-clock reads, or randomness.

### Runtime integration

Replace direct magnet-hint dialing with one diagnostic peer-session owner:

- resolve bounded `x.pe` host/port hints;
- discard non-loopback results under the existing diagnostic restriction;
- translate every usable resolved endpoint into a magnet-hint observation;
- merge and bound records before dialing;
- select and begin one attempt at a time;
- retain failure history and select another eligible record after connection,
  handshake, or metadata failure;
- associate the successful attempt with `PeerConnection`;
- retain the registry while that connection crosses from metadata to content;
  and
- explicitly close the connection lifecycle when the diagnostic finishes.

The explicit `.torrent --peer` input follows the same path as one manual
observation so socket creation no longer bypasses the peer lifecycle.

The live diagnostic still supports at most one connected peer at a time.
That limit is explicit and does not introduce multi-peer piece scheduling.

### Concrete evidence

Extend the existing scripted same-connection magnet test with:

1. a valid but unreachable loopback hint first;
2. a reachable scripted BEP 9 and piece peer second;
3. metadata verification;
4. same-connection piece transfer and hash verification;
5. exact published payload verification; and
6. peer-state assertions showing the first record's failure/backoff, the
   second record's successful connection, and records retained through close.

Add only focused deterministic state tests needed to establish source merging,
capacity/pruning, derived eligibility, backoff, and stale-attempt rejection.

## Contracts And Invariants

- Discovery produces observations; only the registry accumulates records.
- A peer record and a live peer connection are distinct lifetimes.
- Candidate status is derived and never separately persisted.
- Repeated endpoint observations merge their sources and can strengthen, but
  not weaken, advertised reachability.
- Peer-controlled discovery cannot exceed the configured record capacity.
- Dialing and connected records are not evicted for capacity.
- Every dial terminal transition matches its record and attempt generation.
- A failed endpoint cannot be selected again before its backoff or after its
  configured failure ceiling.
- DNS, Tokio, socket, and handshake failures translate into bounded peer
  history rather than entering deterministic peer state as arbitrary strings.
- Verified metadata and content behavior, storage cleanup, payload bounds,
  cancellation, and same-connection handoff retain their existing authority.
- The diagnostic loopback restriction remains in force.

## Nasty Cases Required Up Front

- duplicate endpoint observations from different sources;
- an initially non-connectable record later advertised as connectable;
- zero registry capacity and capacity exhaustion with only active records;
- pruning preference for unusable or failed idle records;
- dialing, connected, banned, backed-off, and failure-ceiling ineligibility;
- stale failure, success, or close callbacks from an older attempt;
- attempt and history counter overflow without wrapping;
- DNS failure and non-loopback-only hint results;
- connection refusal followed by a reachable endpoint;
- handshake or metadata failure followed by another candidate;
- no usable records and all discovered records exhausted; and
- explicit lifecycle close after both success and content failure.

## Non-Goals

- UDP, HTTP, or WebSocket tracker protocols
- tracker reannounce, scrape, tiers, or tracker persistence
- DHT, PEX, LSD, NAT traversal, uTP, or incoming peer service
- simultaneous dial attempts or live peer connections
- multi-peer piece availability, request ownership, endgame, or choking
- peer-ID indexing and deterministic duplicate-connection resolution
- confirmed seed facts, integrity trust, parole, or banning policy beyond the
  lifecycle state needed to make ineligibility explicit
- dynamic peer-cache persistence or product peer views
- public networking or removal of the loopback diagnostic restriction
- candidate-cache or large-swarm performance optimization

## Validation

Run:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv run --project tests/interop \
  python tests/interop/magnet_metadata.py --runs 1
python3 scripts/references.py status
git diff --check
```

Focused development runs may select the new peer-state tests and scripted
magnet failover test. The execution record must distinguish those from the
final workspace and interoperability baseline.

## Stopping Condition

Stop when all peer sockets in the explicit-peer and magnet diagnostic flow
originate from selected registry records; the unreachable-first,
reachable-second magnet downloads and verifies metadata and content over the
successful same connection; deterministic state evidence proves the declared
bounds and guarded transitions; existing workspace and controlled libtorrent
magnet evidence remain green; and this record states exact landed behavior,
validation, deliberate limits, and the next tracker boundary.

## Execution Record

Completed on 2026-07-30.

### Landed behavior

- Added a runtime-independent `peer` module with validated endpoints,
  accumulating discovery-source flags, explicit observations, stable record
  and attempt identities, connection-independent history, a bounded registry,
  deterministic selection, derived eligibility, linear reconnect backoff, and
  guarded dial/connection transitions.
- Registry configuration rejects zero capacity and zero failure ceilings.
  Endpoint, identifier, observation-order, dial-attempt, history, and backoff
  arithmetic fail with typed errors instead of wrapping.
- Repeated observations merge by endpoint, preserve every source, and can
  strengthen connectability. Capacity pressure only prunes idle records,
  prefers unusable or failed records, and does not discard dialing, connected,
  or banned records.
- Added one runtime-owned `DiagnosticPeerSession`. Both explicit
  `.torrent --peer` input and resolved magnet `x.pe` hints now become
  observations before any outbound socket is opened. The previous three
  independent address/dial loops were removed.
- A selected `DialAttempt` now owns the transition into `PeerConnection`.
  Connect and handshake failures update the matching record, metadata
  protocol failures close that attempt, and selection proceeds to another
  eligible record. Stale success, failure, and close callbacks cannot mutate a
  later lifecycle.
- A successful magnet connection remains installed while verified metadata
  is handed to content execution. No reconnect or second address list exists
  at that boundary. Completion and content-protocol failure explicitly close
  the matching registry connection state.
- Android failure classification treats peer-registry failures as peer
  failures without exposing registry internals through the generated control
  contract.

### Evidence

Four deterministic peer-state tests cover source merging and strengthened
reachability, selection and reconnect backoff, successful and failed
lifecycle transitions, stale callbacks, capacity and pruning, active and
banned retention, and checked identifier/history exhaustion.

The scripted magnet tests establish both failure classes before successful
handoff:

- an unreachable first loopback endpoint is retained with one connect failure
  and a future retry time while the second endpoint supplies verified metadata
  and exact hash-verified content over the same connection; and
- a reachable first endpoint that completes the BitTorrent handshake without
  extension support is closed and skipped before the second endpoint
  completes through the public magnet entry point.

The existing one-peer failure tests continue to prove that unsupported
extension capability, metadata disconnect, invalid premetadata state, and
timeout fail before publishing storage.

### Validation run

The following completed successfully:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p rstorrent-engine peer::tests --no-fail-fast
cargo test -p rstorrent-engine public_magnet_entry_uses_peer_registry_path
cargo test -p rstorrent-engine \
  magnet_registry_fails_over_and_hands_same_peer_to_content_download
uv run --project tests/interop \
  python tests/interop/magnet_metadata.py --runs 1
python3 scripts/references.py status
git diff --check
```

The final workspace run passed all crate, architecture, and documentation
tests, including 38 engine-library tests. The controlled libtorrent `2.0.13.0`
run transferred a 26,686-byte, two-block info dictionary and verified all
three content pieces and the exact 40,000-byte payload with cleanup reported
as `ok`. All pinned reference checkouts matched their recorded revisions.

### Deliberate limits and next boundary

The runtime owner remains diagnostic and loopback-only, holds at most one live
connection, and does not fail over during content transfer. Selection is a
deterministic first policy, not a mature tit-for-tat or performance policy.
There is no tracker scheduling, public networking, peer-ID duplicate
resolution, simultaneous dialing, piece ownership across peers, or persisted
peer cache.

The recommended tactical `011` is one bounded UDP tracker announce. It should
parse compact peers into `PeerObservation` values, feed this registry, and
exercise one public-style magnet through tracker discovery without adding
reannounce scheduling or multi-peer transfer.
