# Joined Pause Peer Cleanup

Status: Complete (2026-08-02).

Topics: `peer-lifecycle`, `application-control`, `application-view-api`,
`download-correctness`

## Motivation

Pausing a live torrent can leave connected peer rows in the Peers view after
the pause command succeeds. The rows are not retained history: Peers is the
current connection-generation collection, and a terminal generation must be
removed after its socket, worker, registry, scheduler, request, and payload
owners finish cleanup.

The metadata and content supervisors already observe `DownloadControl`
cancellation and perform explicit joined cleanup. Session-facing download
wrappers also race those same supervisors against the same cancellation token
with biased `tokio::select!` branches. When pause cancels the token, the outer
branch may win and drop the supervisor future before its cleanup branch runs.
The application then observes a terminal task while `DownloadControl` and
`ViewHub` still contain the last connected rows.

This slice makes a successful pause receipt mean that the engine supervisor
has joined its owned work and published the final empty peer observation. It
fixes the owner boundary rather than clearing rows speculatively in the
application or hiding them in React.

## Stable Scenarios

- Pause during a connected metadata attempt emits `disconnecting`, joins the
  socket and metadata worker owners, then emits an empty peer collection
  before the download future returns `Cancelled`.
- Pause during connected content work does the same after request, scheduler,
  socket-set, discovery, and storage-pipeline cleanup.
- The session pause command awaits that terminal result; its current Peers
  snapshot is empty, the active-peer count and transfer rate are zero, and a
  waiting view-set poll receives keyed removals.
- Cancellation before an owner is constructed remains bounded and does not
  create a phantom peer row or detached tracker/DHT task.
- Cleanup failure remains an error and must not be represented as successful
  joined pause.

## Scope

- Remove wrapper-level cancellation races around metadata and content
  operations that already own cooperative cancellation and cleanup.
- Preserve cancellation checks inside the actual metadata/content supervisors,
  storage safe-cancel boundary, peer workers, tracker workers, and DHT work.
- Make terminal owner state explicit and testable at public download-operation
  boundaries.
- Add deterministic loopback regressions for metadata and content cancellation,
  final peer observations, joined scripted peers, zero active work, and session
  view cleanup after pause.
- Correct the affected living correctness and lifecycle evidence.

## Non-goals

- Retaining peer history in the Peers table or adding a disconnected lifecycle
  state.
- Clearing Peers immediately when durable desired state changes to paused.
- A React-only filter, timeout, tombstone, or periodic stale-row reaper.
- Changing peer selection, scheduling, wire protocol, trackers, DHT policy,
  storage layout, pause vocabulary, or durable command ordering.
- Public swarm traffic, visible product launch, Android UI work, schema or
  generated-contract changes, or a new dependency.

## Reference Dossier

There is no BitTorrent wire-state change in this slice, so no BEP adds a
normative transition. The relevant oracle is task and resource teardown.

Pinned libtorrent `2.0.13` revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `src/torrent.cpp::torrent::abort()` first prevents new peer work, stops
  announcing, disconnects all peers, immediately removes peer ownership, and
  asserts that the connection collection is empty before asynchronous disk
  shutdown continues.
- `src/session_impl.cpp::remove_torrent()` detaches session ownership and then
  calls `abort()`; `remove_torrent_impl()` does not synthesize peer display
  state.
- `test/test_remove_torrent.cpp` covers active mid-download teardown, repeated
  removal, eventual invalidation, and an auto-management teardown race.

RSTorrent adopts the explicit no-new-work, disconnect, joined-removal, then
terminal-observation ordering. It retains its own Tokio owners and does not
copy libtorrent structures or asynchronous disk model.

Local JSTorrent revision
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/engine/src/core/torrent.ts::stopNetwork()` cancels queued and
  in-flight connection work, marks connecting peers, and closes every connected
  peer even when network state was already inactive.
- `Torrent::destroy()` disables new network work before destroying connection
  management, clears active pieces, destroys trackers, closes peers, clears
  swarm state, and awaits storage close.

RSTorrent adopts the product expectation that stopped networking has no live
peer rows while making completion stronger through explicit awaited Rust task
joins. No source or fixture is copied.

## Invariants And Bounds

- Peers contains all and only active connection generations. A generation may
  remain `disconnecting` only while one of its owned resources is still being
  cleaned up.
- The supervisor that creates a socket set, metadata worker, content worker,
  discovery task, or storage pipeline owns its cancellation and join.
- A wrapper may request cancellation but must not drop a running owner future
  as its cancellation mechanism.
- The terminal download result is published only after every constructed child
  owner has reached its observable join path.
- The final peer observation is an empty replacement emitted after runtime,
  registry-connection, scheduler, request, and payload cleanup.
- A successful application pause receipt is later than the terminal download
  result and therefore later than the final peer observation.
- Existing resource ceilings remain unchanged: at most 30 pending dials, 30
  established content peers, the existing metadata cohort bound, and the
  configured request, payload, storage-job, and view queue limits.
- Cancellation remains idempotent. A token already cancelled before operation
  startup produces bounded terminal state without constructing avoidable work.
- A cleanup error is retained as an error; terminal assertions never convert it
  to success or claim unverified cleanup.

## Owner, Cancellation, And Data Flow

```text
ApplicationService pause
  -> persist desired paused state
  -> DownloadControl::cancel_when_safe()
  -> await torrent task
       -> metadata/content supervisor observes cancellation
       -> mark generations disconnecting
       -> cancel + join socket/worker/discovery/storage owners
       -> remove scheduler/registry/runtime membership
       -> DownloadControl emits PeerConnections([])
       -> wrapper joins tracker owner and returns Cancelled
  -> ViewActivitySink has already installed the empty peer replacement
  -> refresh/poll publishes peer removals and zero current aggregates
  -> pause receipt succeeds
```

`PeerRuntime` remains the engine membership authority. `DownloadControl` owns
the task-free observation cache and activity delivery. `ViewHub` owns the
portable current projection. Neither `ApplicationService` nor the web reducer
may invent lifecycle removal before the engine owner reports it.

The only wrapper-level early cancellation check permitted is before owner
construction. Once construction begins, any asynchronous constructor must
either be cancellation-aware and clean its own partial owners or be awaited to
a bounded handoff before returning. Magnet peer-hint DNS resolution creates no
child task; tracker construction happens after it and is followed by explicit
tracker shutdown.

## Shape-Changing Edge Cases

- token cancelled before peer coordinator creation;
- cancellation simultaneous with transport connect or handshake completion;
- cancellation while a metadata worker owns the selected connection;
- cancellation between metadata verification and content handoff;
- cancellation with pending and established content peers together;
- cancellation with queued storage writes or a hash operation;
- tracker or DHT discovery active while peer cleanup begins;
- cleanup error after a primary cancellation result;
- repeated pause/cancel and durable refresh during the transient
  `disconnecting` phase; and
- replacement connection generations for the same endpoint.

## Implementation Order

1. Record the source survey and reproduce the wrapper/supervisor ownership
   inversion with focused engine tests.
2. Replace outer cancellation races with cooperative awaits, retaining early
   checks only where no owner exists and retaining internal supervisor selects.
3. Add a terminal-state check at public operation boundaries and preserve
   primary plus cleanup failures accurately.
4. Prove metadata and content cancellation emit connected, disconnecting, and
   final empty observations only after scripted peers have closed.
5. Prove application pause waits for that final observation and publishes
   removals, zero active-peer count, and zero rate through the normal view path.
6. Update owning topics and this evidence record, run proportional and full
   repository gates, and commit cleanly.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure engine state | terminal peer observations are empty; request, payload, and storage pending gauges are zero |
| Metadata runtime | loopback stalled metadata peer reaches connected state, cancellation joins it, and final event is empty |
| Content runtime | loopback connected content peer plus storage owner cancel and join before terminal result |
| Application/view | pause receipt follows peer removals; current Peers is empty and active count/rate are zero |
| Existing lifecycle | queued writes, hashing, full pending cohorts, trackers, restart, removal, and shutdown regressions remain green |
| Repository | Rust format, Clippy with warnings denied, workspace tests, and any directly affected generated checks |

No public interoperability smoke, visible browser, Android build, emulator, or
physical device is required: the defect is below those surfaces and
deterministic loopback coverage exercises the real owner path.

## Stopping Condition

This slice is complete when deterministic metadata and content pause cases
cannot return before child owners join and the final empty peer observation is
installed; the application pause receipt observes empty Peers, zero current
peer aggregates, and keyed removals through the normal leased view contract;
the affected living topics record the evidence; all proportional repository
gates pass; and the work is committed with a clean tree.

## Escalation Contract

Wrapper/supervisor cancellation refactoring, terminal invariant checks,
same-boundary cleanup fixes, deterministic fixtures, and topic/test updates are
authorized. Stop for direction if evidence requires changing durable pause
ordering, retaining peer history, weakening joined-pause semantics, adding an
external dependency, changing a wire or persistence contract, launching a
visible/physical client, using a public swarm, or expanding into unrelated
peer/scheduler/storage policy.

## Implementation And Evidence

All public/session download operation families now perform an early cancelled
check before constructing avoidable work, then directly await the metadata or
content supervisor that owns cleanup. The duplicate biased wrapper selects
around peer-coordinator construction and running supervisors are gone.
`TorrentPeerCoordinator::from_magnet()` checks cancellation before and after
bounded hint resolution so it cannot start a tracker after cancellation.

The metadata supervisor now also selects cancellation while it has no active
peer cohort and is waiting for the first tracker or DHT discovery result. This
was exposed by the full suite: cooperative ownership made an existing silent
tracker test wait indefinitely because that initial discovery branch had
previously relied on the unsafe outer wrapper race. The supervisor-local branch
restores prompt cancellation while leaving tracker shutdown with its owner.

Every public terminal boundary checks the shared `DownloadControl` observation
before applying its existing defensive payload clear. A result with live peer
connections, metadata dials/workers, storage jobs, outstanding request bytes,
or buffered payload bytes becomes a cleanup error; an existing primary failure
is retained alongside that error. Tracker shutdown failure is likewise retained
beside a primary operation failure. The application records these failures and
propagates them through pause rather than accepting them as joined success.
This diagnoses an invariant failure instead of hiding peer rows or converting
incomplete cleanup to success.

Deterministic evidence now includes:

- a stalled BEP 9 peer that reaches connected work, receives a metadata
  request, then observes cancellation; the event sequence includes connected,
  disconnecting, and a final empty replacement, with zero metadata dials and
  workers at return;
- the existing selective diagnostic cancellation case now waits for a real
  protocol-handshaking row, proves `disconnecting` and final empty events,
  asserts zero peer/request/storage owners, and joins the scripted socket
  instead of aborting the fixture; and
- an application service resumed from verified multi-file metadata holds a
  real choked content connection, pauses through the ordinary command, and
  proves the peer sees EOF before the pause receipt. Its already-open leased
  view set then receives the exact keyed removal and a summary with zero active
  peers and zero payload rate; and
- a synthesized terminal owner-cleanup failure is durably recorded and
  returned as an application task error, proving pause cannot accept that
  outcome as joined success.

Validation run on 2026-08-02:

```text
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

All gates passed: 313 tests passed and the three opt-in public-network tests
remained ignored. No generated contract changed, and no public network,
visible client, emulator, or physical device was used. The owning topics and
DL-C15 ledger now record the closing evidence. Directly dropping a public
download future remains outside the application pause contract; in-product
pause and shutdown use `DownloadControl` cancellation and awaited task joins.
