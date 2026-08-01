# Tactical 035: Live Torrent And Peer Inspection Projection

Status: ready for implementation; no implementation has started.

## Motivation And Desired Outcome

Tactical `033` establishes bounded leased application view sets and a
recoverable polling client. Tactical `034` establishes the responsive React,
Zustand, CSS Modules, virtual-table, and named-demo inspection surface. The
visible application still cannot consume truthful live torrent or peer rows,
and the engine currently exposes related peer facts through three owners with
partially overlapping snapshot nouns.

Connect the first useful live inspection cross-section without turning the UI
projection into another engine authority. Establish strict peer vocabulary,
one coherent connection-lifecycle observation, truthful torrent summaries,
semantic frontend view selection, self-expiring Rust view sets, and frontend
recovery after browser suspension. Peers become the first detailed live table:
every currently active connection generation appears from transport setup
through joined teardown, then disappears. The future Swarm table remains the
complete peer-registry projection and is deliberately not conflated with
Peers.

The stopping result is the new React application running headlessly against a
real loopback application service. Scripted peers prove every active lifecycle
phase and removal, a controlled pinned libtorrent peer proves a real transfer,
and browser suspension longer than the server lease proves self-destruction
and fresh-snapshot recovery. No Tauri window, visible browser, Android UI, or
public swarm is used.

## Dependencies And Owning Topics

- [`../topics/application-view-api.md`](../topics/application-view-api.md)
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md)
- [`../topics/desktop-inspection-surface.md`](../topics/desktop-inspection-surface.md)
- [`../topics/peer-lifecycle.md`](../topics/peer-lifecycle.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../topics/performance-and-live-evidence.md`](../topics/performance-and-live-evidence.md)
- [`../topics/capability-readiness.md`](../topics/capability-readiness.md)
- [`../engineering-principles.md`](../engineering-principles.md)
- [`033-headless-view-set-foundation.md`](033-headless-view-set-foundation.md)
- [`034-responsive-demo-inspection-ui.md`](034-responsive-demo-inspection-ui.md)

The implementation must preserve Tactical `033`'s generated contract, cursor,
queue, reset, owner-isolation, and legacy-adapter evidence and Tactical `034`'s
named demo scenarios, responsive behavior, accessibility baseline, and bounded
virtual DOM.

## Reference Survey

The survey is pinned to Rasterbar libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` and JSTorrent sibling revision
`9895410beeed6aff554053769bd006a3fbd373ef`.

### Normative protocol material

- `reference/bittorrent.org/beps/bep_0003.rst` supplies the peer-wire
  handshake and ordinary peer-message boundary.
- `reference/bittorrent.org/beps/bep_0010.rst` supplies extension-handshake
  capability context used by the existing metadata path.

This tactical does not add or change a peer-wire message. The specifications
matter because the projection must distinguish transport establishment from
the point at which a valid BitTorrent handshake makes peer identity and
protocol capabilities knowable.

### Pinned libtorrent source and tests

- `include/libtorrent/peer_info.hpp::peer_info` distinguishes connecting,
  handshake, outgoing, source, transport, choke, rate, queue, timeout,
  availability, and identity facts. Its `connecting` and `handshake` flags are
  evidence that a peer inspection surface must include pre-handshake
  connections rather than only content-scheduler members.
- `src/peer_connection.cpp::peer_connection::get_peer_info` projects one live
  connection's current rates, payload totals, request queues, timeouts,
  availability, endpoint, source, transport, buffers, and lifecycle flags.
- `src/peer_list.cpp::peer_list::new_connection`, `update_peer_port`, and the
  connection-close paths keep endpoint records separate from live connection
  state and handle incoming endpoints, duplicate connections, and cleanup.
- `test/test_peer_list.cpp` cases `update_peer_port`,
  `update_peer_port_collide`, `self_connection`, `double_connection`,
  `double_connection_loose`, `double_connection_random`,
  `double_connection_win`, and `incoming_size_limit` supply edge cases for
  future incoming ownership and the present data shape.

Adopted behavior is the separation of connection-independent peer records
from active connection rows, explicit pre-handshake visibility, distinct
direction and transport, stable connection-lifetime identity, and removal
after lifecycle cleanup. RSTorrent does not copy libtorrent's bit flags,
classes, raw-pointer ownership, single-thread model, source layout, or exact
field set.

### JSTorrent product reference

- `packages/ui/src/tables/PeerTable.tsx` supplies the user-facing precedent
  for state, endpoint, client, source, progress, rates, totals, choke flags,
  and pending requests.
- `packages/ui/src/tables/SwarmTable.tsx` explicitly distinguishes all known
  `SwarmPeer` records, including idle, failed, and banned records, from the
  currently active Peer table.
- `packages/engine/src/core/torrent.ts` and
  `packages/engine/src/core/swarm.ts` supply product-history vocabulary and
  reveal where a display aggregate can become coupled to mutable engine
  objects.

RSTorrent adopts the Peers-versus-Swarm information distinction. It does not
copy JSTorrent's table source, mutable object graph, first-source-wins policy,
React bridge, or styling.

## Strict Vocabulary And Row Membership

The existing vocabulary in `peer-lifecycle.md` remains authoritative. This
tactical adds these presentation-facing distinctions:

| Term | Meaning | Peers table | Future Swarm table |
| --- | --- | --- | --- |
| Peer record | Connection-independent accumulated endpoint evidence and history owned by `PeerRegistry` | Referenced when known | One row for every retained record |
| Connection generation | One uniquely identified active transport/protocol lifecycle | One row while active | Reflected as current record activity |
| Dial attempt | Outgoing transition from an eligible record into connection work | Same connection row, `transport_connecting` | Attempt/history facts on its record |
| Incoming intake | Accepted transport before a BitTorrent handshake identifies or routes the peer fully | Same connection row, beginning at `protocol_handshaking` | Creates or strengthens a record only through registry policy |
| Content scheduler membership | A handshaken connection installed for piece availability and requests | Fields enrich the existing connection row | Not a separate peer identity |

**Peers** contains every active connection generation: outgoing transport
connects, incoming accepted sockets, BitTorrent handshakes, connected peers,
and disconnecting peers whose tasks or owned resources have not finished
cleanup. A terminal history row is not retained here. Once the generation is
fully removed from its task owner, registry connection phase, and content
scheduler, the keyed patch removes it.

**Swarm** will contain every retained peer record, including eligible, idle,
backed-off, failure-limited, banned, dialing, and connected records. That view
is a later tactical. Diagnostics may retain bounded connection history, but
neither diagnostics nor the Swarm view keeps a terminal connection row alive
in Peers.

Use the following connection dimensions instead of a compound UI state:

- `direction`: `incoming` or `outgoing`;
- `transport`: `tcp` or `utp`; this slice emits only `tcp`, but the contract
  and reducer accept the future transport without inventing a new peer noun;
- `lifecycle`: `transport_connecting`, `protocol_handshaking`, `connected`, or
  `disconnecting`; and
- orthogonal current facts such as choke, interest, availability, metadata
  capability, request window, stall state, and close reason.

An incoming TCP socket begins at `protocol_handshaking`; it does not pretend
to perform an outgoing transport connect. A connection ID exists before the
BitTorrent handshake. Peer ID, client name, extension support, torrent routing,
and registry record ID remain `null` until their owning transition knows them.
No `disconnected` lifecycle variant is used in the current-state collection.

Incoming listening and uTP execution are non-goals, but their lifecycle shape
is a required compatibility case. Adding either later must populate the same
connection rows and must not require another active-peer collection.

## Engine Ownership And Refactor Boundary

The current engine distributes related state across:

- `PeerRegistry`, which owns endpoint observations, record phases, source
  flags, dial attempts, history, eligibility, and integrity evidence;
- `PeerSocketSet`, which owns pending connection work, established peer tasks,
  bounded command/event queues, cancellation, and joins; and
- `SwarmState`, which owns connected content availability, choke state,
  request assignments, transfer windows, rates, and usefulness.

These remain legitimate subowners. The defect is that membership transitions
and diagnostic projections coordinate them ad hoc and expose overlapping
`ConnectionActivitySnapshot` and `ContentPeerActivitySnapshot` nouns.

Install one torrent-owned peer-runtime coordinator for connection-generation
membership and lifecycle transitions. The exact internal type name is chosen
after the source survey; `TorrentPeerRuntime` is descriptive, not mandated.
It coordinates registry, connection-task, and content-scheduler transitions
without absorbing their independent invariants or introducing a trait layer.

Add one task-free engine observation keyed by connection generation. It is
assembled coherently through the coordinator and contains only owned current
facts. The application projection maps this engine observation into portable
DTOs; it does not query three owners independently, parse diagnostic strings,
or become a fourth lifecycle authority. The existing diagnostic snapshot and
comparison harness may map from the same observation so the duplicate content
connection snapshot can be retired.

The refactor must make these transitions explicit and exact:

1. reserve a connection-generation identity before outgoing work or incoming
   handshake work begins;
2. install and advance transport/protocol lifecycle only for that generation;
3. attach registry identity, negotiated peer identity, and content scheduler
   facts when they become known;
4. begin disconnecting once, with a stable reason and no new request work;
5. cancel and join the task, release scheduler/request ownership, update the
   registry only for the matching generation, then emit row removal; and
6. ignore or classify every stale completion from an older generation.

No protocol, runtime, socket, Tokio, storage, view, Serde, or frontend type may
leak into `PeerRegistry`'s deterministic state transitions or `SwarmState`'s
request invariants.

## Rust Application Projections

Add named `torrent_peers` and refine the existing torrent-list and selected
torrent-summary views. Rust Serde shapes remain the wire authority and produce
TypeScript and JSON Schema through the existing generator.

### Torrent summary

The initial live frontend summary contains only values with a current owner:

- torrent identity and metadata display name when available;
- lifecycle, storage, and progress assessment;
- metadata availability, content size when known, piece count, and verified
  piece count;
- requested, received, stored, verified, and published byte counts where the
  engine/application can distinguish them;
- current payload download/upload rates where an owner samples them;
- active connection and known peer-record counts;
- current error; and
- capability/status metadata for values the current implementation cannot
  supply.

Demo-only archive, added time, upload behavior, ETA, queue, and category fields
must not be fabricated by the live adapter. The frontend model is adjusted so
its demo adapter can provide these values while a live ready view can mark a
field unavailable or unsupported truthfully.

### Active peer row

The first row includes, when owned:

- connection ID, torrent ID, optional peer-record ID, direction, transport,
  lifecycle, and lifecycle age;
- bounded remote endpoint and optional local endpoint under the privacy policy;
- source flags, optional peer ID, optional parsed client name, extension and
  `ut_metadata` capability;
- interest/choke directions, piece/availability counts, and whether wanted
  work is available;
- payload and protocol rates/totals;
- pending/target requests, queued payload bytes, oldest request age, request
  timeout, request-window phase, and stall/usefulness facts;
- metadata-acquisition stage before content handoff; and
- optional disconnect reason while cleanup remains active.

Rows are complete keyed upserts. A field-mask diff is not introduced in this
slice. Strings, peer IDs, endpoints, source sets, and errors are length-bounded
before entering retained application state. Peer payload, bitfields, request
payload, file bytes, and unbounded event history never cross the boundary.

### Privacy

Endpoints are useful primary debugging data and may be shown in an explicitly
local live inspection view. They must not be added to diagnostic strings,
committed fixtures, screenshot baselines, or retained test reports. Scripted
and controlled evidence uses loopback or documentation addresses. Peer IDs and
client strings are bounded and treated as untrusted display text. A future
remote product requires a separate authorization and redaction policy.

## Missing, Null, Unsupported, And Stale Semantics

Do not use a JavaScript `Symbol` or overloaded absence.

At view level the frontend distinguishes:

- `not_requested`: the responsive/navigation view set does not include it;
- `loading`: requested, but its first coherent snapshot is not installed;
- `ready`: a current validated value exists;
- `unavailable`: the capability exists but its current owner cannot produce a
  value, with a bounded reason;
- `unsupported`: the running service does not implement the view; and
- `stale`: a previously ready value is retained visibly while transport/view
  recovery is in progress.

Within a ready row, a concrete value is known and `null` means the field is
supported but currently has no value, such as peer identity before handshake.
Capability metadata represents unsupported fields or views. A missing
required property/`undefined` is a schema-evolution, validation, or programming
error rather than a normal product state. The UI may render several of these
as an empty cell, but reducers, selectors, accessibility text, and tests retain
their semantic distinction.

## Semantic Frontend View Selection

Extend `InspectionApplication` with a semantic desired-view operation. The
frontend port owns names such as library, selected torrent summary, selected
peers, and logs; it does not expose generated Rust `ViewSpec` values to React.
The live adapter maps the semantic set into one Rust view set, while the demo
adapter honors the same selection and eviction behavior.

The desired set follows actual presentation:

- wide list plus peer detail: torrent list, selected summary, and selected
  peers;
- phone library: torrent list only;
- phone torrent detail: selected summary and only the active detail view;
- compact layout: only the projections visible in its current split/focus;
- global diagnostics: diagnostics plus the minimum navigation context; and
- changed selection or tab: remove no-longer-visible detail views and install
  the new ones atomically.

The torrent list is not permanently retained. A phone/detail-only client and
a future Android detail surface can request only the selected torrent summary
and active detail. Removing a semantic view evicts its materialized data after
the ordered `view_removed` transition; browser-local selection/navigation
context may remain.

React components continue to use narrow Zustand selectors. The controller,
not components or Zustand, owns the async `setViews` call, coalesces rapid
responsive/navigation changes, cancels obsolete requests, and ensures one
desired set wins.

## Self-Expiring View Sets

Tactical `033`'s five-minute idle lease and resource limits remain. Its
current implementation only prunes expired sets opportunistically during a
later view-set operation. Replace that behavior with prompt independent
self-destruction.

One application-owned lease reaper serves all view sets. It owns one bounded
timer, a cancellation signal, and an observable join; do not spawn one task per
view set. At least every five seconds it removes every set whose five-minute
lease elapsed, marks it closed, releases snapshots, pending updates and replay
state, and wakes a waiting consumer. Application shutdown cancels and joins
the reaper before returning. `Drop` remains a cancellation fallback, not the
normal join path.

Rename the semantic timestamp to `last_client_activity`. It is refreshed only
when the owner sends an accepted view-set operation: open, update desired
views, or begin `next_updates`. Engine publication, queue coalescing, wakeups,
response creation, replay retention, and an already-running poll loop do not
refresh it. A maximum 20-second long poll is therefore safe beneath the
five-minute lease. Producer activity can fill or reset an abandoned bounded
queue, but it can never keep the abandoned resource alive.

Expiry is owner-observable as unavailable/expired without revealing whether
another owner possesses the ID. Capacity is reclaimed no later than the lease
plus one reaper interval. Explicit close remains best effort for normal tabs;
the lease owns crash, network loss, process interruption, and suspended-tab
cleanup.

## Browser Suspension And Fresh-Epoch Recovery

A browser may suspend timers and network work for longer than the Rust lease.
Correctness must not depend on `visibilitychange`, `pageshow`, `online`, or a
timer firing while suspended.

When polling resumes and the old set is unknown, closed, or expired, the
controller:

1. marks previously materialized requested views `stale` and reports
   `reconnecting` without presenting them as current;
2. aborts any obsolete poll and permits only one reopen attempt;
3. opens a new Rust view set from the latest semantic desired views;
4. validates and reduces its coherent initial snapshots as a new view-set
   identity and epoch;
5. atomically replaces the stale materialization, including evicting views no
   longer requested; and
6. resumes one polling loop with bounded exponential retry.

No patch from the new set may be applied to the old materialization. A failed
snapshot or listener/store application does not acknowledge its cursor.
Visibility, page-show, and network-online signals may request an immediate
poll and improve latency, but they are hints only. Repeated recovery remains
bounded and cancellable; controller close aborts polling/reopen/backoff and
does not wait for the server lease.

## Loopback Development Mode

Add an explicit unauthenticated development mode for initial local browser
bring-up. It is acceptable only when all of these are enforced:

- the listener binds an OS-assigned loopback address/port and refuses a
  non-loopback bind;
- the UI/API use one same-origin development path or an exact configured
  loopback Origin; arbitrary CORS is not enabled;
- request, response, view-set, queue, and owner limits remain unchanged;
- each browser instance still receives an opaque owner identity and cannot
  use another owner's view set;
- harnesses use temporary profiles and storage roots; and
- the mode is explicit and cannot silently become a production/LAN default.

The existing bearer-authenticated loopback mode remains available. The view
set ID is not a credential in either mode. Production remote access, LAN
binding, accounts, pairing, TLS, relay, and deployment remain separate work.

## Owner, Task, Cancellation, And Data-Flow Map

```text
PeerRegistry ---------+
                      |
PeerSocketSet --------+--> torrent peer-runtime coordinator
                      |      lifecycle transitions + coherent observation
Content scheduler ----+                       |
                                              v
                                  task-free engine observations
                                              |
                                  application projection mapper
                                              |
ViewHub source models + single lease-reaper task
                                              |
                                  bounded leased view set
                                              |
                           generated/validated UpdateBatch
                                              |
                               live InspectionApplication
                                              |
                            controller -> Zustand -> React
```

| Owner | Mutable state/tasks | Cancellation and termination |
| --- | --- | --- |
| Peer runtime coordinator | Connection-generation membership and cross-subowner transitions | Torrent cancellation closes generations and awaits the existing socket/task owner |
| Peer registry | Peer records, observations, eligibility, attempt/history/integrity facts | Task-free; torrent owner drops it after all matching transitions finish |
| Peer connection-task owner | Pending/active sockets, queues, cancellation, task joins | Existing torrent cancellation plus exact per-generation join |
| Content scheduler | Availability, choke, requests, payload allowance, utility | Task-free transitions; generation removal releases all request ownership |
| View hub | Current portable projection models and bounded fan-out | Application lifetime; closes every set on shutdown |
| View-set lease reaper | One timer for the registry | Application cancellation and awaited join |
| Live frontend adapter | Rust-view to inspection-model mapping | Owned and closed by `InspectionController` |
| `ViewController` | Desired views, view-set identity/epoch/cursor, poll/reopen/backoff | Abort plus observable close; no handles in Zustand |

Dependency direction remains engine state -> session projection -> transport
adapter -> frontend model -> Zustand/React. No dependency points back inward.

## Initial Resource And Cadence Bounds

Retain Tactical `033`'s 32 application / 8 owner view-set limits, 16 views per
set, 16--512 KiB queue, 256 KiB default, 512 KiB response, 20-second maximum
wait, 60-second maximum delivery interval, one unacknowledged batch, and
five-minute idle lease.

This tactical adds:

- one lease-reaper task and timer per application service, with a reaper
  interval no greater than five seconds;
- peer rows bounded by the existing per-torrent pending plus established
  connection limits, not by registry size;
- no terminal peer-row history in the current-state view;
- complete peer-row upserts coalesced to the latest value within a requested
  250--500 ms active-view interval;
- bounded display strings and errors using existing diagnostic/string limits
  or tighter field-specific limits;
- no full peer bitfield, request list, payload, or socket buffer in a row; and
- no more than one desired-view update, poll, or reopen operation in flight per
  controller.

If one peer snapshot exceeds the retained queue/response bound, opening fails
explicitly or resets with an unavailable reason; it never truncates a row
silently. Existing connection limits make that condition a deterministic
testable bound.

## Known Performance Debt And Measurement

The current source contains observed but unmeasured costs:

- individual activity/progress changes clone the complete `ViewHub` torrent
  `BTreeMap` while holding its central mutex;
- torrent models clone nested strings, ranges, and active-piece vectors;
- view-set queue accounting serializes updates to JSON before the transport
  serializes the response;
- retained replay batches are cloned when returned;
- the TypeScript view reducer clones the whole view record per batch and the
  torrent-list reducer rebuilds arrays; and
- the inspection reducer clones a torrent's complete peer-row record for a
  peer patch.

These are not yet proven bottlenecks. The tactical nevertheless removes the
full torrent-map clone on a single targeted publication because the new peer
cadence would multiply that known broad critical section. Publish diffs from
the changed source model or an equivalently narrow owner while preserving
coherent snapshots and legacy subscribers.

Record at 30 established peers and in the existing 2,000-torrent/10,000-demo-
peer frontend fixture where applicable:

- projection work and `ViewHub` lock hold time;
- rows and encoded bytes per snapshot/update;
- queue and replay high-water bytes plus reset count;
- JSON accounting/encoding time and allocation evidence available without a
  new profiler dependency;
- TypeScript validation, reduction, selector notification, and render time;
- rendered row/DOM count, browser long tasks, and JavaScript heap; and
- task/view-set counts before expiry and after reaping.

The measurements decide later row field masks, sharding, streaming, or binary
encoding. This tactical does not introduce those optimizations speculatively.

## Shape-Changing And Adversarial Cases

The common path includes:

- outgoing TCP connect, transport success, BitTorrent handshake, content
  handoff, disconnecting, joined cleanup, and keyed removal;
- conceptual/scripted incoming intake beginning before a BitTorrent handshake,
  with identity and routing fields initially `null`;
- future-uTP contract fixture using the same lifecycle vocabulary without
  advertising runtime support;
- failure in transport connect, handshake timeout, invalid handshake,
  extension negotiation, connected content work, and cancellation;
- stale dial/task completion after the endpoint has a newer generation;
- rapid reconnect with a new connection ID while the old row is removed;
- metadata peer that never enters content scheduling and content peer that
  inherits the same connection row identity;
- disconnecting row retained until task, registry, scheduler, request, and
  payload cleanup are exact;
- current Peers empty while Swarm/registry candidates remain nonempty;
- source-rich 30-pending plus 30-established pressure without exceeding row,
  queue, response, or DOM bounds;
- torrent selection and responsive layout changes that stop retaining the
  torrent list or old peer table;
- unsupported, unavailable, loading, empty, stale, and ready-empty states;
- lease expiry without any later client operation, including engine activity
  continuing after the browser becomes silent;
- suspension beyond the lease, fresh-set recovery, and atomic stale-state
  replacement;
- close during poll, reopen, desired-view update, and retry backoff; and
- hostile peer ID, client name, endpoint, error, and additive wire fields.

## Staged Implementation And Gates

### Stage 1: lifecycle owner and vocabulary

Map current `PeerRegistry`, `PeerSocketSet`, `SwarmState`, metadata worker, and
diagnostic snapshot transitions. Install the single peer-runtime coordinator
and coherent task-free observation; retire duplicate connection snapshot
nouns where the common observation replaces them.

Gate: deterministic and scripted engine tests cover every current outbound
phase, stale generations, metadata/content handoff, disconnecting until exact
join, row removal, and all existing registry/request/resource invariants.

### Stage 2: Rust projections and lease ownership

Add `torrent_peers`, refine truthful torrent summaries, generate portable
contracts/schema, replace broad targeted-publication cloning, and install the
single joined lease reaper with client-silence semantics.

Gate: session tests cover snapshots, complete-row upserts, removals, bounds,
null/capability semantics, independent selected torrents, producer activity
that does not renew a lease, prompt expiry without another operation, waiting
poll wakeup, capacity recovery, and joined application shutdown.

### Stage 3: semantic frontend adapter and recovery

Add semantic inspection-view selection, per-view statuses, the live adapter,
responsive desired-set updates, store eviction, and suspension/reopen behavior.
Keep the named demo adapter conformant and its scenarios deterministic.

Gate: pure TypeScript and jsdom tests cover view selection by layout/route,
no always-retained torrent list, null versus unsupported, stale presentation,
single-flight reopen, fresh-snapshot replacement, close during every async
state, narrow selectors, and existing scale/reference-preservation claims.

### Stage 4: headless runtime and browser evidence

Run scripted phase peers and a controlled pinned libtorrent transfer through
the loopback development gateway and the production React build. Drive wide,
compact, and phone navigation without opening a visible browser. Force a short
test-configured lease or virtual lease clock to simulate tab suspension.

Gate: browser assertions observe real torrent and peer rows, active phases,
rates/requests, removal, a verified controlled transfer, stale/reconnecting
state, server-side set destruction, new view-set identity/epoch, and coherent
recovery. Accessibility and bounded-DOM gates remain green; screenshots use
loopback or redacted endpoints.

### Stage 5: documentation and regression

Record the actual owner/refactor shape, fields, limits, measurements, evidence,
known gaps, and next inspection view. Update generated artifacts and all owning
topics, then run the full workspace/frontend gates.

## Validation Matrix

### Pure and deterministic

```bash
source ~/.profile
cargo test -p rstorrent-engine peer
cargo test -p rstorrent-engine swarm
cargo test -p rstorrent-session view_set
cargo test -p rstorrent-session peer_view
npm test --prefix clients/web
npm run typecheck --prefix clients/web
```

Focused test filters may follow actual module names. The tactical record must
state the exact commands that ran.

### Generated and workspace gates

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm ci --prefix clients/web
npm run generate --prefix clients/web
git diff --exit-code -- \
  clients/web/src/api/generated/v1.ts \
  clients/web/src/api/generated/v1.schema.json \
  clients/web/src/fixtures/
npm run typecheck --prefix clients/web
npm test --prefix clients/web
npm run build --prefix clients/web
npm run test:e2e --prefix clients/web
```

### Scripted and controlled interoperability

Extend or add headless harnesses that:

- hold connections in transport-connect, handshake, connected, and
  disconnecting phases and assert exact row identity/removal;
- run the real React application through the unauthenticated loopback
  development mode with a temporary profile;
- suspend client operations past a shortened test lease while engine updates
  continue, then verify set destruction and recovery; and
- complete and hash-check one controlled libtorrent-seeded torrent while live
  peer rows remain observable.

No public swarm, visible Tauri app, visible Chrome, Android build/UI, emulator,
or physical device is required or authorized by this tactical.

## Invariants

- Peer records, connection generations, dial attempts, metadata workers,
  content scheduler members, and UI rows are never synonyms.
- The Peers collection contains all and only active connection generations.
- One connection ID is allocated before handshake and survives metadata-to-
  content handoff; reconnect creates a new ID.
- Incoming/outgoing direction and TCP/uTP transport are orthogonal to
  lifecycle.
- A disconnecting row is removed only after exact owned-resource cleanup.
- A projection cannot mutate registry, task, scheduler, request, or protocol
  state and cannot become a lifecycle authority.
- The Swarm view will derive from peer records; Peers does not retain idle,
  failed, banned, or historical-only records.
- Only accepted client operations renew a view-set lease. Engine activity
  never renews it.
- Every abandoned view set destroys itself within one reaper interval after
  lease expiry and wakes its consumer.
- Recovery after expiry begins from a new coherent snapshot/epoch, never a
  patch over stale state.
- The materialized store contains only currently desired views; the torrent
  list is not globally mandatory.
- `null`, unsupported, unavailable, stale, empty, missing, and not requested
  retain distinct meanings.
- View-set identifiers do not authenticate callers, including in local dev
  mode.
- Current view, queue, task, string, response, DOM, and payload bounds remain
  independent of hostile discovery volume.
- Structured state is not inferred from logs, and logs are not command
  receipts.
- No visible product client is launched by routine validation.

## Non-Goals

- incoming listener, accept routing, advertised-port correction, NAT mapping,
  UPnP/NAT-PMP, payload upload, or seeding;
- uTP sockets or uTP protocol implementation;
- the Swarm, tracker, file, piece, disk, speed, DHT, or full logs live tab;
- Android contract or UI migration;
- replacing the default Tauri/live entry before this headless adapter is
  proven;
- public/LAN remote access or production unauthenticated control;
- archive, labels, removal, deletion, queue, ETA, or category product
  semantics;
- streaming, `requestAnimationFrame` delivery, WebSocket view streams, Tauri
  Channels, CBOR, another binary codec, or field-mask row diffs;
- generalized peer-policy, picker, storage, throughput, or BEP campaign work;
  and
- copying reference source, assets, fixtures, or table implementation.

## Escalation Contract

Once implementation is authorized, proceed without routine approval for the
recorded owner refactor, generated contract changes, frontend model changes,
explicit loopback development mode, controlled libtorrent/scripted fixtures,
headless Chrome, temporary profiles, redacted screenshots, same-boundary bug
fixes, and bounded commits.

Stop for direction if evidence requires implementing incoming listening or
uTP, changing durable persistence or product command semantics, creating a
production remote security contract, exposing non-loopback unauthenticated
control, modifying Android or the visible desktop lifecycle, adding a broad
framework/dependency with product tradeoffs, copying reference material, or
resuming the engine-parity campaign outside this projection/refactor boundary.

## Stopping Condition And Next Boundary

This tactical is complete when the engine has one coherent active-connection
observation and strict vocabulary, Rust exposes truthful bounded torrent and
peer views, abandoned view sets self-destruct on client silence, the frontend
requests only currently relevant semantic views and recovers from suspension
through a fresh atomic snapshot, scripted and controlled libtorrent runs are
visible through the real React surface headlessly, performance/resource high
waters are recorded, all generated/frontend/workspace gates pass, owning
topics record actual evidence and gaps, and the working tree is committed and
clean.

The next slice should be selected from real inspection use. The likely choices
are the categorized live Logs view or the registry-backed Swarm view; neither
is implicitly authorized by completing this tactical.
