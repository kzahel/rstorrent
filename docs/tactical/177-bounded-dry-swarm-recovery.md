# Tactical 177: Bounded Dry-Swarm Recovery

Status: **Complete.** Explicit user direction on 2026-08-27 temporarily
yielded durable High file-priority Tactical `176` and release/updater Tactical
`158` to this bounded peer-lifecycle correction. Every stopping-condition gate
passes, and Tactical `176` resumes as the sole **Now**.

Topics: `oracle-driven-engine-campaign`, `capability-readiness`,
`peer-lifecycle`, `download-correctness`, `application-view-api`

Dependencies: completed multi-peer Tactical
[`017`](017-adversarial-multi-peer-liveness.md) supplies failure/backoff and
no-alternative waiting; completed registry inspection Tactical
[`064`](064-registry-backed-swarm-inspection.md) supplies truthful retained
eligibility; completed long-lived runtime Tactical
[`086`](086-long-lived-torrent-peer-runtime.md) supplies the single shared
registry/runtime owner; completed session admission Tactical
[`114`](114-session-wide-concurrent-torrent-admission.md) supplies fair
outbound turns and the session connection ceiling.

## Motivation And Decision

An observed running public torrent had remaining wanted data and continuing
discovery but no activity. Its retained Swarm state initially contained 31
records, all at the fixed three-failure ceiling, with zero eligible, dialing,
or connected peers and zero useful payload. A later DHT result added four new
records; those moved through ordinary attempts and backoff while the original
31 remained permanently excluded. The hard ceiling prevented destructive
reconnect churn, but it also turned plausible transient endpoint failures into
permanent process-lifetime exclusions even when no installed alternative could
advance the torrent.

Retain the ordinary three-failure ceiling and add two distinct escape valves:

1. A fresh tracker observation of an existing failed endpoint decrements its
   consecutive failure count by exactly one. It does not alter total failures,
   the last failure classification, or the already scheduled retry deadline.
   DHT, PEX, cache, magnet, manual, local-discovery, and incoming observations
   do not rehabilitate failure history.
2. A content swarm with no connected, incoming, dialing, ordinarily eligible,
   or ordinarily backed-off peer may select one failure-limited endpoint whose
   own retry deadline passed and whose last failure is plausibly transient. A
   torrent-wide probe cadence permits only one such attempt at 5, 10, 20, 40,
   then at most every 60 minutes. Any successful outgoing handshake resets the
   cadence.

The dry-swarm probe is a liveness mechanism, not removal of the failure limit.
Normal selection, Swarm classification, and source rehabilitation remain
independently inspectable.

## Stable Scenarios

1. **DSR-001 tracker rehabilitation.** An idle endpoint at the configured
   failure ceiling remains failure-limited after DHT or PEX refresh. One
   tracker refresh decrements consecutive failures by one, preserves its
   cumulative history and third-failure retry deadline, reports backed-off
   until that deadline, and then receives one ordinary attempt.
2. **DSR-002 ordinary ceiling.** Without a trusted refresh or the exact dry
   preconditions, three consecutive failures remain excluded from ordinary
   selection. New eligible or normally backed-off candidates always precede a
   failure-limited probe.
3. **DSR-003 bounded dry admission.** With wanted work, live discovery, zero
   connected/incoming/pending peers, and only expired failure-limited records,
   one transient-failure endpoint becomes a probe. The attempt uses the
   existing peer, connection-generation, outbound-turn, socket, handshake,
   transport, and session/torrent capacity owners.
4. **DSR-004 cadence and capacity.** A dry swarm owns at most one pending probe
   and cannot cycle immediately across a full registry. Consecutive probe
   admissions schedule torrent-wide delays of 5, 10, 20, 40, and 60 minutes;
   later delays remain capped at 60 minutes and time arithmetic saturates.
5. **DSR-005 recovery.** A successful probe follows the ordinary handshake,
   duplicate-admission, scheduling, payload, close, and cleanup path. It resets
   both that endpoint's consecutive failure history and the torrent-wide probe
   cadence; normal multi-peer filling may then resume.
6. **DSR-006 exclusions.** Banned, non-connectable, active, corrupt/known-bad,
   address-family-disallowed, self, duplicate-peer-ID, and protocol-failed
   records are never dry probes. Only `Connect`, `Handshake`, and
   `RemoteClosed` terminal failures are transient for this policy.
7. **DSR-007 observability and termination.** Swarm continues to classify an
   idle candidate as failure-limited until the admitted attempt moves it to
   dialing. A structured `dry_swarm_probe_started` diagnostic records the
   exceptional admission and its next cadence without exposing the endpoint.
   Pause, replacement, failure, and shutdown retain exact generation-fenced
   cleanup and no new background task survives the torrent owner.

## Reference Dossier

### Protocol semantics

No BEP standardizes local endpoint retry ceilings or dry-swarm recovery. This
is bounded connection policy and changes no tracker, DHT, PEX, peer-wire,
metainfo, storage, or integrity format.

### Pinned libtorrent oracle

The exact pin is rasterbar libtorrent `2.0.13` commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d` from
`reference/pins.toml`. The source and tests inspected were:

- `src/settings_pack.cpp`: defaults `max_failcount = 3`,
  `min_reconnect_time = 60`, and `torrent_connect_boost = 30`;
- `src/peer_list.cpp::is_connect_candidate`: a peer at the maximum failure
  count is not a normal candidate;
- `src/peer_list.cpp::find_connect_candidates`: retry delay grows as
  `(failcount + 1) * min_reconnect_time`;
- `src/peer_list.cpp::update_peer`: only an exact tracker-source update is
  trusted to decrement a nonzero failure count and allow another try;
- `src/torrent.cpp::do_connect_boost`: the boost spends startup connection
  capacity immediately after early discovery; it is not a no-progress or
  dry-swarm override; and
- `test/test_peer_list.cpp::set_max_failcount`: changing the maximum removes
  peers at the ceiling from the normal candidate count.

Libtorrent therefore has the same hard normal ceiling and no reserved hail-
mary slot. RSTorrent adopts its tracker-source rehabilitation behavior but
independently adds the more conservative one-at-a-time dry-swarm mechanism
because the observed product state had continuing discovery and no remaining
normal action. RSTorrent does not copy libtorrent's candidate cache, raw
pointer ownership, startup boost, ranking, or session tick architecture.

### JSTorrent product history

The local JSTorrent checkout was inspected at exact revision
`0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`:

- `packages/engine/src/core/peer-selector.ts` excludes connected, connecting,
  banned, and backed-off peers but has no permanent connection-failure ceiling;
  its exponential retry reaches a five-minute cap;
- `packages/engine/src/core/swarm.ts::resetBackoffState` clears non-banned
  failure state when a torrent starts; and
- `packages/engine/test/core/swarm.test.ts` covers exclusion during backoff,
  increasing quick-disconnect delay, expiry recovery, and failed-attempt
  separation.

RSTorrent retains the stricter ordinary ceiling, restart-independent volatile
records, deterministic selection, and explicit integrity bans. It adopts only
the product lesson that transient connection failures must retain some bounded
automatic recovery path.

## Owner, Task, Cancellation, And Dependency Map

```text
tracker/DHT/PEX/manual observations
              |
              v
task-free PeerRegistry
  source merge + ordinary eligibility + trusted tracker rehabilitation
              |
              +--> normal PeerSelector -------------------+
              |                                           |
              +--> failure-limited probe selector         |
                                                          v
task-free per-operation DrySwarmProbeState --> TorrentPeerCoordinator
  next admission + capped cadence              existing outbound turn
                                                  |
                                                  v
                         existing TorrentPeerState / PeerRuntime
                         attempt + connection generation + cancellation
                                                  |
                                                  v
                         existing PeerSocketSet / content supervisor
```

`PeerRegistry` remains the sole owner of endpoint history and both selection
classifications. `DrySwarmProbeState` owns only a counter and next-admission
deadline inside the existing content-operation coordinator. It adds no task,
timer, socket, channel, persistence, client command, or dependency. The
existing content supervisor's maintenance wake evaluates the deadline.

Every admitted probe is an ordinary outgoing connection generation. The
existing torrent cancellation token, socket set, peer budget, session fair
turn, transport subattempt, handshake deadline, duplicate admission, and
joined cleanup remain its owners. A success resets probe cadence; a failed or
cancelled attempt follows existing exact settlement. Dropping the coordinator
drops the two scalar probe fields, so pause/restart cannot retain an orphaned
deadline or task.

Dependency direction remains runtime-independent peer history/selection and
probe cadence -> torrent runtime coordinator -> existing socket/protocol
workers -> session diagnostic projection. Protocol code does not depend on
Tokio, sockets, clocks, application views, or retry policy.

## Invariants And Resource Bounds

- Ordinary selection still rejects every record at or above the configured
  consecutive-failure ceiling.
- Tracker rehabilitation decrements at most once per received endpoint
  observation and never below zero; it preserves `total_failures`,
  `last_failure`, and `retry_at`.
- Dry selection never mutates the registry. Admission revalidates the endpoint
  and generation under the registry owner before creating runtime state.
- A probe requires zero established outgoing peers, zero incoming content
  peers, zero pending dials, zero ordinary eligible records, and zero ordinary
  backed-off records. It consumes one existing pending-dial slot and one
  existing session outbound turn.
- At most one probe is admitted per content-supervisor pass. Its global cadence
  begins at five minutes, doubles with saturation, and caps at one hour.
- Candidate order is deterministic: prior successful connection, more
  independent sources, tracker provenance, fresher observation, older last
  dial, lower failure count, then stable record ID.
- Failure types with definite local/protocol/identity or integrity meaning do
  not become transient through dry policy. Existing bans remain absolute.
- Registry capacity remains 1,000 records; established and pending defaults
  remain 30 each; no history, queue, task, event, or allocation bound grows.
- Swarm snapshots remain truthful ordinary classifications. The diagnostic
  event contains record ID, failure count, probe ordinal, and next delay, but
  no endpoint, peer ID, source text, magnet, path, or payload.

## Implementation And Validation Sequence

1. Add exact tracker-source rehabilitation to `PeerRegistry::observe` with
   deterministic at-limit, non-tracker, retry-deadline, active, and history
   tests; commit the independent mechanism.
2. Add runtime-independent dry candidate selection and capped cadence state,
   with transient/excluded failure, deterministic rank, expiry, saturation,
   success-reset, and stale-admission tests.
3. Compose one probe into content dial filling beneath normal candidates and
   all existing capacity/admission owners. Add scripted runtime evidence for
   three failures, no churn before expiry, one exceptional attempt, successful
   handshake/content progress, and exact cancellation/cleanup.
4. Add structured session diagnostics without changing generated application
   commands or Swarm schema. Update peer lifecycle, download scenario, campaign
   checkpoint, readiness queue, and evidence.
5. Run focused tests, `cargo fmt --all -- --check`,
   `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and the
   maintained Android Rust ABI builds because the common engine behavior ships
   in-process there. Generated TypeScript/UniFFI and presentation gates are
   inapplicable unless implementation changes their boundary.

## Stopping Condition

This tactical completes when DSR-001 through DSR-007 pass through pure and
scripted runtime evidence; normal selection remains hard-limited; tracker
rehabilitation and dry probing are separately observable; a dry swarm can make
one bounded recovery attempt without retry waves; success reaches the ordinary
content path; pause/failure/shutdown clean up exactly; proportional repository
and Android engine gates pass; and every owning topic records the landed
behavior and remaining limits.

## Implementation Evidence

Stage 1 is complete. `PeerRegistry::observe` now applies the pinned
libtorrent-compatible one-step rehabilitation only to an existing exact
tracker observation. The deterministic transition reaches the ordinary
three-failure ceiling through real attempt identities, proves DHT refresh is
inert, preserves all cumulative and deadline history across tracker refresh,
reports ordinary backoff until the original deadline, and then exposes one
ordinary candidate. Repository formatting and the focused `rstorrent-engine`
tracker-rehabilitation test pass.

Stages 2 through 4 are implemented. Runtime-independent selection waits for
the endpoint's ordinary retry deadline, admits only connect, handshake, or
remote-close failures, excludes protocol/identity/integrity cases, ranks
credible records deterministically, and revalidates the candidate under the
registry owner. The task-free torrent cadence produces exact 5, 10, 20, 40,
and capped 60-minute delays with saturating time/counters and success reset.
The content coordinator requires every ordinary connection action plus the
normal backoff cohort to be exhausted, then spends one existing fair outbound
turn and pending slot. Its structured event maps to the endpoint-free
`dry_swarm_probe_started` session diagnostic.

The scripted vertical closes three accepted TCP sockets during handshake,
observes exactly three ordinary attempts, admits exactly one probe after the
third ordinary retry deadline, completes and verifies the same endpoint on
attempt four, publishes exact bytes, resets endpoint failure and torrent probe
state, and terminates with zero dialing, connected, or failure-limited record
counts. The 26 peer/long-lived-owner tests, three focused dry-swarm tests,
formatting, and warning-denying clippy for `rstorrent-engine` and
`rstorrent-session` pass.

Closure validation on 2026-08-27 passes:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace -- -D warnings`;
- `cargo test --workspace`, including 586 passed engine tests with 11 declared
  opt-in/maximum tests ignored and 257 passed session tests with two declared
  ignored; and
- `clients/android/build.sh`, including locked x86_64 and arm64-v8a release
  Rust builds, Kotlin UniFFI generation, debug APK assembly, and JVM unit
  tests. Existing Android deprecation warnings remain unrelated.

No generated application type changed, so TypeScript/schema regeneration and
web presentation gates are inapplicable. No installed service, live public
swarm, external machine, release, or network policy was mutated.

## Non-Goals

- Removing or raising the ordinary failure ceiling, releasing all failed peers,
  retrying multiple dry candidates concurrently, or matching JSTorrent's
  indefinite five-minute retry cap.
- Dry probing while a normal peer is eligible/backed off or any content peer is
  connected, incoming, or dialing; metadata-acquisition dry probing; automatic
  torrent restart; or treating discovery traffic alone as payload progress.
- Changing tracker/DHT/PEX cadence, session/torrent connection limits,
  replacement, uTP suppression, MSE fallback, request windows, piece choice,
  integrity reputation, parole, persistence, or peer-record eviction.
- New user settings, client presentation, durable peer cache, service install
  or restart, live public-swarm performance claims, release publication, or
  mutation of the currently running installed service.
