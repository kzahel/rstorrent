# Tactical 064: Registry-Backed Swarm Inspection

Status: Planned; direction accepted on 2026-08-03. Implementation has not
begun.

Topics: `peer-lifecycle`, `application-view-api`, `web-ui-design`,
`desktop-inspection-surface`, `capability-readiness`

## Motivation

The Peers tab deliberately shows only active connection generations. That is
the correct lifecycle boundary, but it cannot explain why a known endpoint is
idle, backed off, failure-limited, banned, currently dialing, or no longer
connectable. The engine's bounded `PeerRegistry` already owns those facts for
up to 1,000 retained records. The empty Swarm tab should project that owner
instead of growing disconnected history inside the Peers view or reconstructing
selection state from diagnostics.

This slice makes Swarm the durable mental model for every currently retained
peer candidate while preserving Peers as the active-connection view. It is the
smallest missing detail tab because it needs no new networking policy and no
new peer owner.

## Desired Outcome And Stopping Condition

When a torrent is selected, Swarm shows every retained peer-registry record,
its source set, current eligibility, retry/failure history, and integrity
posture in one bounded responsive table. A controlled peer can move through
eligible, dialing, connected, backed-off, and eligible again without changing
row identity; Peers contains the row only while a connection generation is
active. Closing and reopening the leased view reconstructs the exact current
registry state.

The tactical stops when the immutable registry observation, named application
view, generated contracts, demo scenario, responsive presentation, and
controlled loopback lifecycle proof all pass. It does not change peer
selection, dialing, backoff, banning, or protocol-support claims.

## Dependencies And Sequence

- Tacticals `033`, `035`, `043`, `048`, and `060` provide the leased view-set,
  active-peer projection, table behavior, shared Tauri/web reducer, and
  multiplexed transport foundations.
- Tactical `063` is complete before this planned sequence begins.
- This is the first of the accepted missing-detail-tab tacticals. Tactical
  `065` follows it and reuses the session/torrent tab-scope cleanup; Tactical
  `066` follows `065`.

## Scope

- Add one task-free immutable observation of the existing `PeerRegistry`.
- Publish meaningful registry transitions through the existing engine activity
  boundary, including one terminal inactive state after joined cleanup.
- Retain one bounded per-torrent Swarm projection in the application view hub.
- Add capability `torrent_swarm`, `ViewSpec::TorrentSwarm`, coherent snapshots,
  keyed patches, generated TypeScript/schema/UniFFI/Kotlin contracts, strict
  browser decoding, and reducer coverage.
- Replace the Swarm scaffold with a summary strip and virtualized table.
- Centralize detail-tab scope metadata so torrent tabs and session tabs are
  selected from one vocabulary rather than hard-coded independently in the
  React pane and controller. This establishes the boundary needed by the later
  DHT and Speed tabs.
- Add a permanent deterministic scenario plus headless accessibility,
  responsive, scale, lease-recovery, and controlled live evidence.

## Non-Goals

- Changing peer admission, eviction, dialing order, backoff, failure limits,
  banning, parole, or integrity scoring.
- Persisting peer-registry records or presenting a historical connection log.
- Adding incoming listening, upload/seeding, uTP, PEX, local discovery, NAT
  traversal, or multi-torrent execution.
- Replacing or merging the active Peers tab.
- Per-row commands, peer details drawers, protocol-message capture, traffic
  history, or client-lifetime byte totals.
- A public remote-control privacy policy. The existing authenticated local
  application boundary remains; a future remote product must review endpoint
  exposure explicitly.
- Public-swarm traffic, a visible desktop launch, or Android UI work.

## Reference Dossier

### Protocol and product semantics

BEP 3 defines peers and peer-wire behavior but not a client-side retained-peer
registry or inspection table. Eligibility, retry, source attribution, and
integrity posture are explicit RSTorrent engine policy and must remain labeled
as such.

### Pinned libtorrent oracle

The required oracle remains libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d` from `reference/pins.toml`.
Implementation must re-inspect:

- `include/libtorrent/peer_info.hpp` for active-peer observation vocabulary;
- `include/libtorrent/torrent_peer.hpp` and `src/peer_list.cpp` for retained
  peer identity, source flags, fail counts, connection state, and pruning; and
- `test/test_peer_list.cpp` for duplicate discovery, reconnect/backoff,
  replacement, and bounded-list cases.

RSTorrent adopts useful observable distinctions, not libtorrent's object
layout, bit flags, connection manager, scoring rules, or storage policy.

### JSTorrent product history

Inspect local JSTorrent revision
`9895410beeed6aff554053769bd006a3fbd373ef`, especially:

- `packages/ui/src/tables/SwarmTable.tsx` for the established product
  distinction between Swarm and active Peers; and
- the engine peer/swarm model and tests reached from that component for source,
  connection, retry, and failure semantics.

JSTorrent informs labels and useful scenarios. No JavaScript source, fixture,
  table implementation, or implicit unbounded history is copied.

## Existing Boundary And Concrete Improvement

`PeerRegistry` is the sole owner of retained endpoint identity and currently
caps itself at 1,000 records. A `PeerRegistrySnapshot` already contains one
capture time, configured maximum, derived counts, and records with stable ID,
endpoint, sources, connectability, observation times, phase, dial eligibility,
history, and integrity. The content path currently publishes only aggregate
counts at a 100 ms observation cadence; the complete snapshot is not an
authoritative live application view.

The boundary improvement is to make that existing immutable snapshot a typed
activity input. The application maps it to presentation DTOs; neither the UI
nor view hub becomes a registry, timer, or peer-selection authority.

## Owner, Task, Cancellation, And Data Flow

```text
PeerRegistry (torrent content owner, maximum 1,000 records)
       |
       | immutable snapshot after semantic transition / deadline
       v
existing bounded engine activity sink
       |
       v
ViewHub torrent_swarm projection
       |
       | coherent snapshot or keyed patch
       v
leased view set -> strict browser adapter -> Zustand
       |
       v
summary strip + virtualized Swarm table
```

No new task, timer, channel owner, socket, or daemon is introduced. The content
owner emits the latest immutable state through its existing coalesced activity
path. It forces transitions that affect visible eligibility and forces a final
inactive observation only after its peer/socket/scheduler children are joined.
Application view interest changes delivery, not engine ownership or peer work.

The runtime-independent peer module continues to own record transitions and
snapshot derivation. Session/application code may depend inward on those
types; the peer module must not depend on view sets, JSON, React, or transports.

## View Contract

Add capability `torrent_swarm` and `ViewSpec::TorrentSwarm { view_id,
torrent_id, delivery }`. The conceptual generated contract is:

```text
SwarmAvailability = active | inactive | torrent_missing

SwarmPeerState =
  eligible | not_connectable | dialing | connected |
  backed_off | failure_limited | banned

SwarmPeerView {
  peer_record_id,
  remote_endpoint,
  sources[],
  connectable,
  state,
  first_observed_age_millis,
  last_observed_age_millis,
  retry_in_millis?,
  dial_attempts,
  dial_failures,
  last_dial_age_millis?,
  last_failure_age_millis?,
  last_failure_kind?,
  trust_points,
  hash_failures,
  valid_pieces,
  on_parole,
}

TorrentSwarmSnapshot {
  availability,
  captured_millis,
  maximum_records,
  counts,
  peers[],
}

TorrentSwarmPatch {
  availability?,
  captured_millis,
  maximum_records?,
  counts?,
  upsert[],
  removed[],
}
```

Exact names may follow existing generated-contract conventions, but the
semantics may not be collapsed into display strings. Source, state, and failure
kind are closed typed vocabularies. `peer_record_id` is the existing stable
registry identity and is the patch/table key. A connected Swarm record is not
an active-peer row join; connection details remain authoritative in Peers.

`active` with an empty collection means a valid empty registry. `inactive`,
`torrent_missing`, unsupported, disconnected, stale, reset, and overflow
remain distinct. Null means unavailable; it is never replaced with zero or an
empty string.

## Invariants And Resource Bounds

- A snapshot contains no more than the registry's existing 1,000-record hard
  maximum. The application and browser reject a larger collection.
- One endpoint has one retained registry record. Patch keys are stable across
  eligibility changes and are removed only when the registry evicts the record
  or the torrent projection becomes inactive.
- Aggregate counts equal the rows in the same captured snapshot. State mapping
  has one precedence order matching `DialEligibility`; a record cannot appear
  simultaneously eligible and backed off.
- `sources` is a deduplicated closed set capped by the engine's source
  vocabulary, initially no more than eight values.
- Ages and retry durations are derived from the same monotonic capture point.
  Wall-clock timestamps are not manufactured from engine `Instant` values.
- A retry deadline becoming eligible is a semantic transition even if no peer
  packet arrives. The owner must publish it through its existing wake/deadline
  path; the browser does not count down into a different authoritative state.
- Failure text is a typed bounded category plus optional bounded local detail,
  never an unbounded socket or log error chain.
- Snapshot/patch delivery is no faster than 100 ms and may coalesce intermediate
  observations. Forced lifecycle and eligibility transitions may bypass the
  cadence once.
- View lease closure drops browser/application replica interest but does not
  clear or mutate the engine registry.
- Endpoint display follows the accepted local detailed-inspection posture.
  Diagnostics export and future remote access do not inherit that authority
  automatically.

## Presentation Contract

- The title is **Swarm** and helper text says that it contains all retained
  candidates, while **Peers** contains active connection generations.
- The summary strip shows Total, Eligible, Dialing, Connected, Backed off,
  Failure limited, and Banned. Counts do not become filters in this slice.
- Default columns are Endpoint, State, Sources, Last seen, Attempts, Failures,
  Retry, Trust, and Parole. First seen, valid pieces, hash failures, and last
  failure are optional columns in the shared table preferences.
- Sorting is typed and stable. Existing column sizing, visibility, live-sort
  preference, virtualization, and narrow-width horizontal scrolling apply.
- Source and state use text plus restrained tokens; color is never the only
  distinction. Retry and age cells use accessible full values even when their
  visual labels are compact.
- Empty, unavailable, inactive, stale, reset, and transport-disconnected states
  use the shared inspection-state components. There are no fake rows and no
  permanently spinning state.
- The tab remains torrent-scoped and requires a selected torrent. Central tab
  metadata, rather than a hard-coded `disk`/`logs` exception, decides this.

## Stable Scenarios And Shape-Changing Cases

The permanent `swarm-lifecycle` scenario contains stable rows for eligible,
not-connectable, dialing, connected, backed-off, failure-limited, banned, and
multi-source records. It also exposes empty, inactive, stale, and overflow/reset
states without using timers for fixture truth.

The implementation and tests must cover:

1. duplicate discovery from tracker and DHT merges sources without changing ID;
2. an active connection appears in both views with different authoritative
   fields, then disappears from Peers while remaining idle in Swarm;
3. a failed dial moves to backed-off and later eligible at the engine deadline;
4. reaching the failure limit and explicit banning remain distinct;
5. integrity/parole changes update the same row;
6. registry-cap eviction produces one exact removal and count update;
7. pause, completion, failure, removal, and replacement generation cannot
   leave ghost rows; and
8. view-set reset and lease reopen reconstruct the complete bounded snapshot.

These cases land with the common path because they define identity, time,
membership, and lifecycle ownership.

## Staged Implementation And Gates

1. **Reference and pure projection.** Reconfirm the dossier, add immutable
   registry observation/mapping, and prove state precedence, age anchoring,
   duplicate sources, deadline transition, cap eviction, and terminal clear.
2. **Application contract.** Add the capability, view spec, snapshot/patch,
   bounds, generated artifacts, strict decoders, and reducer. Rust and contract
   tests must pass before presentation work.
3. **Scope cleanup and demo.** Centralize tab scope and add the permanent
   scenario. Existing torrent/session tab requests must remain exact.
4. **Presentation.** Build summary and virtual table with responsive and
   accessibility coverage. The 1,000-row fixture must stay bounded and usable.
5. **Controlled live proof.** Exercise one loopback peer through connect,
   disconnect/failure, backoff, and re-eligibility while comparing Swarm and
   Peers. Join the torrent owner and prove terminal cleanup.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure state | Registry snapshot/count/state precedence, source merge, age/retry anchoring, cap eviction, and terminal mapping tests. |
| Contract | Rust serialization/schema/type generation, invalid enum/oversize rejection, keyed patch/reset/lease reducer tests. |
| Scripted runtime | Deterministic lifecycle including backoff deadline and replacement-generation fencing. |
| Web UI | Component, accessibility, keyboard/table, narrow/standard/wide responsive, theme, stale/reset, and 1,000-row scale tests. |
| Controlled interoperability | Headless loopback peer lifecycle with exact payload integrity and joined cleanup; no visible client. |
| Platform | Rust workspace baseline, production web build, and proportional Tauri/Android generated-contract compilation. |
| Public live evidence | Not authorized or required. |

Run the repository Rust baseline and focused frontend lint/type/test/build
commands in proportion to the changed packages. Record exact commands and
results in this tactical when complete.

## Escalation Contract

Ordinary refactoring, internal naming, conservative tightening of limits,
generated-contract updates, test-only loopback controls, and fixes at this
observation boundary are authorized when the tactical is activated. Stop for
direction before changing registry policy or persistence, exposing endpoints
to a new remote surface, adding commands or a dependency, changing protocol
claims, or expanding into another discovery/transport capability.

## Next Boundary

Raw protocol messages, connection history, per-peer controls, and a combined
peer detail drawer remain later work. Tactical `065` next adds a session-wide
DHT observatory without turning its routing table into another peer registry.
