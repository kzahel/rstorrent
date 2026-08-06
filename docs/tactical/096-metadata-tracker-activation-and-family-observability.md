# Tactical 096: Metadata Tracker Activation And Family Observability

Status: Complete on 2026-08-06. Metadata-only acquisition now activates the
session discovery owner only while its application task is live, and tracker
rows expose a bounded last-successful connection-family value. Controlled and
official Ubuntu evidence passes without creating payload artifacts.

Topics: `tracker-discovery`, `application-control`, `application-view-api`,
`public-torrent-testing`, `capability-readiness`

Dependencies: completed Tactical
[`095`](095-bounded-http-https-tracker-transport.md) owns the transport,
schedule, resource ceilings, unauthenticated-HTTPS policy, and controlled
IPv4/IPv6 evidence. This slice changes application activation and observation;
it does not reopen HTTP parsing or TLS policy.

## Motivation And Decision

The first official Ubuntu smoke proved that a normal running add can announce
to both retained HTTPS trackers and hash-verify metadata. The same magnet with
`start_content=false` ran the external metadata owner for 180 seconds while
both tracker rows remained inactive with zero attempts. Discovery registration
currently copies durable content-running intent even while the application has
an active metadata-acquisition task.

Separate current metadata-discovery activity from durable content-running
intent. A fresh paused magnet may activate trackers and DHT only while its
owned metadata task is live. It must deactivate discovery after metadata
success or terminal failure, and pause, removal, replacement, and shutdown
must keep their existing cancellation and join behavior. Content storage must
remain unopened until explicit resume.

Also retain the address family of the last successful physical tracker
announce as a bounded enum. Project it through the existing tracker view and
show it as an optional advanced table column. Do not expose a tracker socket
address, DNS answer, source address, peer address, interface, or hostname
through this field. The value is volatile and is not persisted.

## Stopping Condition

This tactical is complete when:

1. a controlled tracker-only magnet added with `start_content=false`
   announces, receives its only metadata peer, hash-verifies metadata, remains
   paused, and creates no payload, staging, or part-file artifact;
2. discovery activation is derived from an actually owned metadata task, not
   merely absent metadata, so startup failure and terminal task failure do not
   leave an unconsumed tracker or DHT owner running;
3. metadata success, pause, removal, replacement, and shutdown deactivate or
   stop tracker work through the existing bounded owner with no late mutation;
4. successful UDP, HTTP, and HTTPS announces retain only `ipv4` or `ipv6` as
   the last connection family, while inactive or never-successful rows expose
   no family;
5. engine snapshots, session views, generated schema/TypeScript/UniFFI,
   validation, reducers, demo data, and the advanced tracker table agree on
   the optional field;
6. controlled IPv4 and IPv6 tracker tests prove the field from the actual
   transport path rather than URL spelling or returned peer family; and
7. the metadata-only Ubuntu smoke is repeated after deterministic closure.
   A Debian IPv6 public announce may run only when the host has a routed IPv6
   path; lack of such a path is recorded rather than bypassed with a product
   setting or a stale literal address.

## Scope And Invariants

- `ApplicationService` owns whether its one active operation is acquiring
  metadata. Discovery reconciliation may combine that volatile fact with
  durable `desired_running`, but the store does not gain another intent bit.
- The initial catalog reconciliation before task start may remain inactive.
  The post-start reconciliation must activate discovery once the task handle
  is installed. The task supervisor's terminal advertisement reconciliation
  must restore the durable paused state after success or failure.
- DHT and trackers use the same registration activity fact. Private-torrent
  gating, tracker tiers, operation ceilings, retry policy, and port selection
  remain unchanged.
- The schedule record owns the last successful family because it already owns
  last success, interval, peers, and swarm counts. Failures do not invent or
  replace it. Replacement creates a fresh record and restart does not restore
  it.
- UDP reports the family of the connected destination that supplied the
  accepted announce response. HTTP(S) reports the family selected before the
  request and checked against reqwest's remote address. Returned `peers` or
  `peers6` never determine this value.
- Presentation labels the field `Family`, renders `IPv4`, `IPv6`, or an em
  dash, and keeps the column hidden by default. No user control for forcing a
  family is added.

## Owner, Task, And Dependency Map

```text
ApplicationService active task + durable resume
  -> DiscoveryAdvertisementRegistration.desired_running
  -> one session-owned advertisement service
       -> UDP or HTTP(S) physical announce
       -> TrackerAnnounceOutcome { connection_family }
       -> TrackerSchedule last-success record
       -> TrackerRuntimeSnapshot
  -> session TrackerView
  -> generated contracts and optional web Family column
```

The application depends inward on the existing discovery handle. The pure
schedule depends only on the transport-neutral family enum, not on sockets,
reqwest, Tokio, or application state. Presentation depends on the projected
view and does not infer family from a URL.

## Resource And Security Boundaries

Tactical `095`'s eight-operation session ceiling, DNS limits, request/body
limits, timeouts, peer caps, redaction, and cancellation rules remain exact.
Metadata-only activation consumes the same existing operation and peer
budgets. The new enum adds constant per-row state and no history.

HTTPS remains encrypted but unauthenticated. Connection-family visibility is
diagnostic routing evidence, not server identity, reachability, listener, or
BEP 7 proof. Live summaries must not retain raw peer or tracker IP addresses.

## Reference Dossier

No new wire behavior is introduced. Tactical `095`'s pinned BEP, libtorrent,
rqbit, JSTorrent, reqwest, and compression dossier remains controlling. The
relevant pinned libtorrent paths remain
`reference/libtorrent/src/http_tracker_connection.cpp`,
`reference/libtorrent/src/http_connection.cpp`, and
`reference/libtorrent/src/tracker_manager.cpp`; their tests establish that
physical connection selection and returned peer families are distinct facts.
RSTorrent keeps its existing owner shape and does not copy reference source.

The first-party behavior reference remains
`../jstorrent/packages/engine/src/tracker/http-tracker.ts` and
`../jstorrent/packages/engine/src/utils/minimal-http-client.ts`. Its connection
metadata is useful vocabulary, but this slice exposes only an enum and does
not inherit remote-address or minimal-client architecture.

## Validation

- Pure schedule tests cover unset, IPv4, IPv6, success replacement, failure
  retention, replacement reset, and inactive snapshots.
- Scripted UDP and HTTP(S) tests assert the family returned by the actual
  successful destination, including IPv6 literal/AAAA-only behavior.
- An application test performs tracker-only metadata acquisition with paused
  content and verifies started/stopped lifecycle plus absent content files.
- Session and web tests cover serialization, validation, mapping, diffing,
  demo rows, and optional table rendering.
- Run formatting, workspace clippy, workspace tests, generated-contract drift,
  web typecheck/tests/build, and relevant Android generation/cross-build gates
  in proportion to the contract change.
- Repeat the opt-in Ubuntu metadata-only smoke in a clean temporary profile,
  then remove all downloaded metainfo, databases, logs, and payload roots.

## Non-Goals And Deferrals

- Authenticated HTTPS certificate and hostname validation remains its own
  security tactical.
- Full BEP 7, IPv6 listeners, pinholes, per-family advertised endpoints,
  simultaneous announces, and a product family preference remain deferred.
- Persistent route history, DNS answers, remote socket addresses, interface
  names, peer-family summaries, and public-tracker reliability claims are not
  added.
- The public torrent catalog is a living testing topic, not a default CI suite
  and not an availability promise by third-party projects.

## Implementation And Evidence

`ApplicationService` now combines durable content-running intent with the
presence of its owned metadata-acquisition task when reconciling discovery.
Installing that task activates the existing tracker/DHT registration; its
supervised terminal path restores durable paused intent. No new persistent
state, task, or queue was introduced.

The transport-neutral tracker schedule now retains
`TrackerConnectionFamily::Ipv4` or `Ipv6` from the accepted physical announce.
UDP derives it from the connected response source, while HTTP(S) derives it
from the selected request destination. Failures retain the last successful
value; new/replaced rows begin unset. Session views, generated schema,
TypeScript, UniFFI, runtime validation, demo/live mappings, and the hidden-by-
default React `Family` column carry only that enum and never an address.

Deterministic and controlled evidence includes:

- schedule retention/replacement coverage for the last successful family;
- UDP loopback and HTTP AAAA-only assertions against the physical transport;
- an application tracker-only metadata test whose paused add receives its
  only peer from HTTP, observes started and stopped, verifies metadata, remains
  paused, and creates no payload, staging, or part-file artifact; and
- application separation between an IPv4 tracker connection and an IPv6-only
  returned peer.

The repeated opt-in Ubuntu 24.04.4 metadata-only run used a fresh temporary
profile and the exact official torrent info hash
`62a4d9e139f3315f8716bcccca0cc984a9809da1`. Metadata verified in 150.736
seconds. Both `torrent.ubuntu.com` and `ipv6.torrent.ubuntu.com` completed a
started and stopped HTTPS announce, ended inactive with two attempts, and
reported IPv4 as their last successful connection family. The latter name was
dual-stack and therefore did not constitute routed-IPv6 proof. Content storage
remained unopened: the run found zero payload files and cleanup removed its
temporary state.

A direct IPv6 connectivity probe reached Debian's dual-stack tracker host on
port 6969 but intentionally sent no valid announce and received no HTTP
response. It is route evidence only, not tracker-protocol evidence; a bounded
application announce on a native IPv6 host remains future public breadth.

Closure validation passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace`, including 307 passing engine tests with six
  ignored and 156 passing session tests with one ignored;
- generated-contract refresh, web type checking, 178 passing web tests with
  two skipped, and the production Vite/CSP build; and
- Android API 28 x86_64 and arm64-v8a Rust cross-builds, UniFFI Kotlin
  generation, debug APK assembly, and the debug JVM suite.

The first workspace run also reproduced a pre-existing timing weakness in the
incoming metadata-plus-payload vertical: its one-second no-request timer could
win while the test waited for the upload projection, and a queued keepalive
could precede EOF after unregister. The test now gives its own observation a
five-second lifecycle budget and drains bounded protocol bytes until close.
It passed ten consecutive isolated repetitions and the final workspace run;
production timeout behavior did not change.
