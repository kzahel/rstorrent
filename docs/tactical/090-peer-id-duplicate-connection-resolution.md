# Tactical 090: Peer-ID Duplicate Connection Resolution

Status: Complete on 2026-08-06. Commits `6ea2576` through `d5b55cd` install
the task-free admission owner, enforce its decision in every socket owner,
prove controlled crossed-connection convergence, and publish typed product
diagnostics.

Topics: `peer-lifecycle`, `download-correctness`, `protocol-support`,
`capability-readiness`

Dependencies: completed Tacticals
[`017`](017-adversarial-multi-peer-liveness.md),
[`082`](082-bounded-multi-peer-upload-ownership.md), and
[`086`](086-long-lived-torrent-peer-runtime.md) establish bounded outgoing and
incoming connection generations, exact request/upload cleanup, and one
long-lived per-torrent peer owner. Completed Tactical
[`091`](091-availability-ranked-piece-activation.md) supplies the ranked piece
activation prerequisite recorded by the campaign sequence.

## Decision And Motivation

Resolve duplicate live connections by the remote handshake peer ID after the
handshake is validated and before the connection becomes eligible for content
or upload work.

RSTorrent currently records a peer ID on each connection observation but
indexes live authority by endpoint, peer record, and internal connection
generation. Simultaneous incoming and outgoing connections can therefore keep
two sockets to the same remote client, consume two connection or upload slots,
and split request and cleanup state. PEX would increase the number of alternate
endpoints and address-family duplicates that reach this boundary.

Peer IDs are connection claims, not authenticated durable identities. This
tactical uses equality only to choose one live socket. It does not merge peer
records, endpoints, discovery provenance, retry history, integrity reputation,
or bans by peer ID.

This tactical owns stable correctness scenario DL-C29 in
[`download-correctness`](../topics/download-correctness.md).

## Stopping Condition

This tactical is complete when all of the following hold:

1. one torrent-owned index admits at most one established connection for a
   nonlocal remote peer ID under the default policy;
2. simultaneous crossed connections make the same deterministic winner choice
   at both clients, while repeated same-direction connections retain one
   stable winner without oscillation;
3. a connection reporting the application peer ID is rejected as a
   self-connection before content or upload work begins;
4. closing the loser releases its request attempts, upload grant, queues,
   descriptor charge, runtime observation, peer-record generation, and task
   exactly once without disturbing the winner;
5. endpoint records and accumulated source provenance remain separate and a
   spoofed peer ID cannot transfer reputation, bans, or retry history;
6. deterministic crossed, same-direction, self, stale-cleanup, IPv4/IPv6, and
   capacity-saturation cases pass; and
7. controlled RSTorrent/libtorrent tests in both initiation directions pass
   with exact terminal owner counts.

## Scope

- Add a task-free per-torrent live peer-ID index beside the existing
  connection-generation owner.
- Perform duplicate admission after info-hash and peer-ID validation but before
  the connection enters request scheduling or receives an upload grant.
- Use the pinned libtorrent-compatible crossed-connection rule: the endpoint
  with the lexicographically greater local peer ID is the side permitted to
  initiate. This makes both clients close the same physical connection.
- For two locally incoming or two locally outgoing connections with the same
  peer ID, keep the already admitted generation and reject the later one.
- Fence removal by connection generation so a late loser task cannot erase a
  newer winner's index entry.
- Emit typed close reasons for self and duplicate peer-ID decisions and retain
  enough bounded observation to diagnose which direction won.
- Apply the same decision to IPv4 and IPv6 connections while retaining their
  endpoint records independently.

## Non-Goals

- IP-address deduplication, one-connection-per-NAT policy, or treating multiple
  peers behind one address as duplicates.
- Authentication of peer IDs, durable client identity, Sybil resistance, or
  identity-based access control.
- Merging tracker, DHT, magnet-hint, incoming, or future PEX records solely
  because they present the same peer ID.
- Persisting peer IDs, integrity reputation, connection history, or bans.
- Connection scoring, rarest-first selection, snub/parole policy, BEP 6, PEX,
  BEP 40, or application presentation beyond truthful diagnostic close state.

## Normative And Reference Dossier

The v1 handshake in BEP 3 supplies the 20-byte peer ID but does not make it an
authenticated identity. The implementation must preserve that limitation.

Pinned libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

- `reference/libtorrent/src/bt_peer_connection.cpp`, around the handshake
  peer-ID admission path, rejects self-connections and uses local/remote peer-ID
  ordering to resolve crossed connections;
- `reference/libtorrent/src/peer_list.cpp` owns endpoint and direction-related
  duplicate handling separately from peer-ID equality;
- `reference/libtorrent/include/libtorrent/settings_pack.hpp` defaults
  `allow_multiple_connections_per_pid` to false; and
- `reference/libtorrent/test/test_peer_list.cpp` covers self, double incoming,
  and both winners of simultaneous connection races.

Adopt the deterministic agreement rule and the separation between live
connection identity and endpoint records. Do not copy libtorrent's class
layout, pointer ownership, settings surface, or peer-record representation.

The first-party JSTorrent checkout was also inspected. Its
`packages/engine/src/core/swarm.ts` retains a `peerIdIndex` that groups endpoint
keys for inspection and cleanup. It is useful evidence that address and peer-ID
indexes serve different purposes, but it does not supply the complete
crossed-connection eviction contract required here.

No source, fixture, or test data is imported from either reference.

## Owner And Transition Map

```text
validated handshake
       |
       v
torrent peer runtime
  compare local/remote peer IDs
  consult generation-fenced live peer-ID index
       |
       +-- admit winner --> ordinary content/upload registration
       |
       `-- reject loser --> typed close --> joined task cleanup
```

- The task-free peer runtime owns the peer-ID index and admission decision.
- Socket tasks report the validated handshake and obey the returned admission
  decision; they do not inspect sibling sockets or mutate the index directly.
- The torrent supervisor continues to own request, upload, registry, budget,
  cancellation, and task joins.
- Protocol values remain independent of Tokio, sockets, task handles, and
  application views.

Required transitions are `unidentified -> identified candidate -> admitted`
or `unidentified -> identified candidate -> rejected`. A rejected generation
must never become briefly schedulable. Winner removal deletes the index entry
only when both peer ID and connection generation still match.

## Resource And Correctness Invariants

- The index contains at most one entry per admitted live connection and is
  therefore bounded by the existing per-torrent connection ceiling.
- The index adds no background task, timer, queue, retry loop, or durable state.
- A duplicate decision never changes piece, block, request, upload, or
  integrity state until ordinary loser cleanup runs through its existing
  owner.
- A stale close cannot remove a newer generation's entry or decrement its
  resource charges.
- Peer ID comparison uses the exact 20 handshake bytes; client-name parsing is
  display-only and never policy input.
- Multiple endpoints with no simultaneous live connection remain eligible.
- Incoming and outgoing capacity, upload slots, and descriptor accounting stay
  exact through the race.

## Implementation Gates

1. Add pure admission-state cases, including the exact crossed-connection
   truth table and generation-fenced removal.
2. Integrate the decision at the shared post-handshake boundary for outgoing
   and routed incoming sockets.
3. Prove loser cleanup with outstanding download requests, queued upload work,
   an upload grant, and saturated descriptor/connection budgets.
4. Run controlled paired RSTorrent and pinned libtorrent races in both local
   peer-ID orderings.
5. Update the owning topics, readiness row, and evidence record without
   claiming authenticated peer identity.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | self ID, first admission, same-direction duplicate, both crossed-direction orderings, stale removal, reconnect, IPv4/IPv6 endpoints |
| Scripted runtime | simultaneous handshakes, loser with requests, loser with upload grant/read, capacity saturation, cancellation during admission, exact joins |
| Controlled interoperability | RSTorrent and pinned libtorrent each initiate simultaneously; both peer-ID orderings retain one usable connection and complete verified payload |
| Resource evidence | peer-ID index, live connections, descriptors, request reservations, upload slots, child tasks, and terminal counts remain within existing bounds |
| Product surfaces | no new UI; existing Peers/Swarm state shows only the admitted generation and a bounded typed diagnostic explains loser closure |

No public swarm, visible client, schema migration, dependency, or physical
device is required.

## Escalation And Next Boundary

Ordinary refactoring at the shared handshake/admission boundary, additional
race cases, and tightening generation checks are authorized only when this
tactical is explicitly selected for implementation. Stop for direction if
evidence requires multiple live connections per peer ID as product policy,
durable identity/reputation merging, or a compatibility exception for a named
client family.

The next planned slice is the BEP 6 request lifecycle in Tactical
[`093`](093-bep6-fast-request-lifecycle.md). Peer-ID resolution is also a
prerequisite for planned bounded PEX in Tactical `094`.

## Implementation Record

The torrent peer runtime now owns one exact-20-byte peer-ID index beside its
connection-generation map. Admission occurs only after a validated handshake.
Self IDs are rejected; a same-direction candidate loses to the established
generation; and crossed directions retain outgoing only on the endpoint whose
local peer ID is lexicographically greater. Removal checks both peer ID and
connection generation.

Every incoming, content-download, and metadata-download socket reports the
validated ID through that owner before it becomes schedulable. A torrent peer
handle also owns generation-keyed cancellation tokens, so eviction closes the
losing socket immediately and then lets the existing request, upload,
descriptor, registry, budget, and joined-task owners perform their ordinary
exact cleanup. Endpoint records, sources, retry history, integrity state, and
reputation remain keyed independently and are never merged by peer ID.

`PeerFailure::SelfConnection` and `PeerFailure::DuplicatePeerId` project as
the generated `self_connection` and `duplicate_peer_id` disconnect reasons.
No setting, durable identity, UI policy, background task, dependency, or
schema version was added.

## Closing Evidence

- Pure runtime cases cover first admission, self rejection, same-direction
  stability, both crossed orderings in both arrival orders, IPv4 and IPv6
  endpoints, reconnect, and stale generation-fenced removal.
- Scripted incoming and outgoing owners prove post-handshake admission before
  upload grants or request scheduling, immediate loser socket closure, typed
  terminal observation, winner continuity, cancellation, and exact zero
  request/upload/connection ownership. The duplicate-incoming case reaches
  its configured two-connection saturation ceiling and returns to one winner.
- `tests/interop/peer_id_duplicate.py` drives simultaneous crossed TCP
  connections through the production incoming and outgoing owners against
  pinned libtorrent `2.0.13.0`. Opposite admission orders make the outgoing
  winner evict an active incoming uploader and the incoming winner evict an
  outgoing generation only after a decoded wire trace proves a content
  request. Both exact peer-ID orderings converge to the agreed direction,
  retain one libtorrent peer, report one distinct typed loser, reach a
  connection high-water of two, verify SHA-1
  `0bfc6cebc1fde20c5325ae7d89d5da5e720bc096`, and terminate with zero pending,
  established, and torrent connection generations.
- Fresh generated TypeScript, JSON Schema, and validators pass 200 web tests
  plus type checking. Session UniFFI and Android Rust binding checks accept the
  expanded closed enum.
- `cargo fmt --all -- --check`, warning-denying workspace Clippy, and the full
  workspace tests pass. The controlled proof opens no public swarm or visible
  product client and removes its temporary fixture.
