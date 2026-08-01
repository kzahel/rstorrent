# Tactical 033: Headless View-Set Foundation

Status: active.

## Motivation And Desired Outcome

The detailed desktop inspection direction now has an accepted application
boundary, but the repository implements only Tactical `008`'s independent
push subscriptions. Building React tables directly on that proof would make
the UI coordinate several stream identities, couple recovery to WebSocket or
Tauri delivery, and leave polling and forward-compatible runtime validation
unproven.

Establish one leased view set for the named projections relevant to a client.
Prove its snapshots, keyed diffs, cursor replay, acknowledgement, bounds,
reset, and lifecycle through authenticated JSON polling and a pure TypeScript
client before React or the peer table depends on it.

The stopping result is one controlled libtorrent-seeded magnet added through
the real application command API and observed by a headless TypeScript driver
through metadata, piece activity, verified publication, and joined shutdown.
No Tauri window, browser window, Android UI, or public swarm is used.

## Dependencies And References

- [`../topics/application-view-api.md`](../topics/application-view-api.md)
- [`../topics/application-control.md`](../topics/application-control.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md)
- [`../topics/desktop-inspection-surface.md`](../topics/desktop-inspection-surface.md)
- [`../engineering-principles.md`](../engineering-principles.md)
- [`008-reactive-multi-surface-control.md`](008-reactive-multi-surface-control.md)
- The existing controlled gateway/libtorrent proof in
  `tests/interop/gateway_reactive_surface.py`
- The current Rust owners in `rstorrent-session::views` and
  `rstorrent-session::ApplicationService`
- The current loopback adapter in `rstorrent-gateway`

No BitTorrent protocol transition changes in this tactical, so no new BEP or
libtorrent engine-source dossier applies. Pinned libtorrent `2.0.13.0` remains
the independent controlled peer, using the existing fixture and process
owners rather than serving as an application-API design reference.

Application API references inspected before this tactical:

- qBittorrent WebUI API 5.0 `sync/maindata`: `rid`, `full_update`, keyed
  torrent updates, and `torrents_removed` establish a mature recoverable pull
  precedent.
- Transmission RPC specification: requested projections, object results, and
  explicit recently removed IDs inform the collection shape; its positional
  table encoding is deliberately not adopted.
- `ts-rs` `12.0.1`, already locked with Serde compatibility, remains the
  TypeScript declaration generator.
- `schemars` generates JSON Schema from the same Rust DTOs; an established
  standards-compliant TypeScript validator may be added after exact version,
  license, and lockfile review. Generated structural validation allows unknown
  object fields; handwritten validation retains cross-field and resource
  invariants.

Reference URLs and deliberate differences live in
[`../references.md`](../references.md). No reference source or fixture is
copied by this tactical.

## Scope

### Rust semantic contract

Add portable, Serde- and `ts-rs`-derived values for:

- API capabilities and effective resource limits;
- client-selected named view specifications;
- view-set opening options and response;
- per-view snapshot, patch, removal, and reset updates;
- atomic update batches with view-set ID, epoch, base cursor, resulting
  cursor, and durable revision; and
- structured view-set errors suitable for transport mapping.

Retain canonical decimal strings for epochs, cursors, revisions, timestamps,
and unbounded counters. Unknown additive object fields are forward-compatible;
unknown closed control or patch variants remain errors.

The initial view kinds adapt existing truthful owners only:

- torrent-list summary;
- selected-torrent summary;
- selected-torrent piece activity; and
- bounded diagnostics.

Session capabilities may be returned by `hello`, but a new peer projection is
the next tactical and is not fabricated here.

### Leased view-set owner

Extend the application-owned view hub with a task-free view-set registry.
Each view set owns:

- an opaque 128-bit randomly generated identifier;
- transport-installed owner identity that is never accepted from a wire DTO;
- one epoch and opaque cursor sequence;
- at most 16 unique client-selected view IDs;
- the active view specifications and effective delivery intervals;
- one whole-set bounded pending accumulator;
- at most one emitted but unacknowledged batch for exact replay;
- queue high-water and reset counts;
- last activity and bounded idle lease; and
- explicit close state plus wakeup for pending pull operations.

The hub remains the source of coherent projection snapshots and patches. It
fans each state edge into legacy Tactical `008` subscribers and view sets
without making either consumer the application authority. Existing Android,
Tauri, and WebSocket subscription contracts remain working during this slice.

Opening returns the initial coherent snapshots as the first acknowledged
cursor position. `next_updates(after)` treats `after` as acknowledgement of a
previously applied batch. Repeating the same unacknowledged cursor replays the
same batch. A stale, unknown, or incompatible cursor returns an explicit
reset, never a silent tail jump. Updating desired views places added
snapshots and ordered `view_removed` values into the same feed.

### Pull and transport adapter

Expose the semantic operations through `ApplicationService` without holding
its Tokio mutex across a wait. The first remote mapping is:

```text
GET    /api/v1/hello
POST   /api/v1/commands
POST   /api/v1/view-sets
PUT    /api/v1/view-sets/{id}/views
GET    /api/v1/view-sets/{id}/updates?after=...&wait_ms=...
DELETE /api/v1/view-sets/{id}
```

The existing `/control` WebSocket proof remains available. New HTTP routes
require the configured loopback credential in a standard bearer header and
enforce the configured `Origin` before accessing service state. Authentication
context installs the view-set owner; no request body can claim another owner.

JSON request and response bodies, query strings, IDs, view counts, waits, and
errors are bounded before mutation. The gateway stays loopback-only and does
not become a production remote server. Browser CORS UX and WebSocket view-set
streaming are later adapters; the headless TypeScript driver supplies the
allowed origin explicitly.

### Generated TypeScript and headless client

Move generated v1 declarations behind a stable `src/api` barrel without
forcing the provisional direct-DOM UI to migrate in this tactical. Generate a
matching JSON Schema deterministically from Rust. Replace handwritten
structural enumeration for the new view-set envelopes with schema-backed
validation while preserving small handwritten semantic bounds. Fix the
currently demonstrated storage-state drift through generation rather than
another duplicated list.

Add:

- a JSON codec boundary;
- an authenticated polling `ApplicationClient` implementation;
- a lifecycle-owning `ViewController` without React or Zustand;
- pure view-set batch reduction keyed by `view_id`; and
- a headless TypeScript integration driver/test.

The controller keeps one poll in flight, acknowledges only after successful
validation and reduction, immediately polls after commands or view changes,
and closes its remote view set. Sockets, timers, and abort handles do not enter
the reducer state.

### Controlled interoperability

Adapt the existing temporary-profile libtorrent gateway harness to use the
polling client. It must:

1. open a torrent-list view;
2. add the controlled magnet through `/api/v1/commands`;
3. observe the keyed torrent upsert;
4. add selected summary and piece-activity views;
5. observe positive requested, received, and stored activity;
6. reach three verified pieces and `complete`;
7. verify the exact published payload SHA-1;
8. close the view set; and
9. interrupt and join the gateway and libtorrent owners with temporary
   profile, payload, and process cleanup.

This is view-contract interoperability evidence, not a throughput result.

## Owner, Dependency, And Data-Flow Map

```text
engine/application edges
        |
        v
ApplicationService -> ViewHub source models
                           |
              +------------+-------------+
              |                          |
       legacy subscribers          leased ViewSet
                                         |
                                  bounded UpdateBatch
                                         |
             +---------------------------+------------------+
             |                                              |
        HTTP polling                                future stream adapter
             |
      schema + semantic validation
             |
        pure TS reducer
             |
       headless controller
```

Protocol and engine crates do not depend on application views, Axum,
TypeScript, schemas, or clients. `rstorrent-session` owns semantic values and
task-free view-set transitions. `rstorrent-gateway` owns HTTP authentication,
framing, waits, and cancellation. The TypeScript controller owns polling and
abort lifecycle. The pure reducer depends only on generated DTOs.

## Initial Resource Bounds

- maximum live view sets per application service: 32;
- maximum live view sets per authenticated gateway owner: 8;
- maximum views per set: 16;
- maximum client view ID: 64 UTF-8 bytes, restricted to a conservative ASCII
  identifier alphabet;
- whole-set requested queue: 16 KiB through 512 KiB, default 256 KiB;
- maximum emitted HTTP update body: 512 KiB;
- maximum delivery interval: 60 seconds;
- maximum long-poll wait: 20 seconds;
- idle lease: 5 minutes, refreshed by successful view-set operations;
- maximum JSON request body: 64 KiB; and
- one unacknowledged batch per view set, with later state remaining in the
  bounded coalescing accumulator.

The implementation may tighten these values if an existing coherent snapshot
or deterministic hostile case demonstrates a safer bound. Raising them or
adding unbounded retained history requires topic evidence.

## Shape-Changing Edge Cases

The common path must include:

- duplicate and invalid client view IDs;
- invalid projection combinations and diagnostic filters;
- initial snapshot larger than the requested queue;
- independent view sets at different speeds;
- repeated pull after a lost HTTP response;
- acknowledgement of the emitted cursor followed by accumulated next state;
- old, future, malformed, wrong-epoch, and wrong-view-set cursors;
- view removal followed by reuse of the same ID and mandatory new snapshot;
- row upsert followed by removal and later distinct upsert;
- state coalescing versus lossless ordered diagnostics;
- overflow with explicit reset and fresh-snapshot convergence;
- close while a long poll waits;
- idle expiry and lazy registry pruning;
- application shutdown waking and closing all view sets;
- unknown owner access rejected without revealing resource existence;
- unknown additive JSON fields accepted; and
- unknown tagged variants, malformed decimals, oversized bodies, and excess
  resources rejected before mutation.

## Staged Implementation And Gates

### Stage 1: semantic owner

Add contract values, task-free view-set state, fan-out, lifecycle operations,
and deterministic tests. Preserve the legacy subscription suite.

Gate: focused `rstorrent-session` tests cover opening, replay, acknowledgement,
coalescing, reset, specification changes, owners, expiry, close, and shutdown.

### Stage 2: generated boundary

Add deterministic JSON Schema and TypeScript output, structural validation,
pure reducer, cross-language fixture, and drift checks.

Gate: generation produces no uncommitted diff; TypeScript type checking and
unit tests cover snapshots, patches, removal, reset, replay, unknown fields,
and semantic rejection.

### Stage 3: authenticated polling

Add bounded HTTP routes, service operations, wait cancellation, polling
client, and controller. Keep `/control` regressions green.

Gate: Rust gateway tests cover auth/origin, owner isolation, body/query
bounds, open/update/pull/delete, lost-response replay, expiry, and shutdown.
The headless TypeScript integration test drives the real gateway.

### Stage 4: controlled download

Run the polling client against a temporary real application service and
controlled libtorrent seed through exact publication and cleanup.

Gate: the retained trace contains list upsert, live piece activity, three
verified pieces, complete state, exact SHA-1, explicit view-set close, joined
gateway shutdown, and no retained temporary artifacts.

## Validation Matrix

### Pure and deterministic

```bash
source ~/.profile
cargo test -p rstorrent-session view_set
cargo test -p rstorrent-gateway view_set
npm test --prefix clients/web
npm run typecheck --prefix clients/web
```

### Workspace and generated artifacts

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
```

Exact generated paths may retain a temporary compatibility re-export for the
existing UI. The tactical record must state the final paths actually checked.

### Controlled interoperability

```bash
source ~/.profile
uv run --project tests/interop --locked \
  python tests/interop/gateway_view_set_surface.py
```

No public live run, visible desktop client, browser window, emulator, or
physical device is authorized or required by this tactical.

## Invariants

- A view-set identifier never authenticates its caller.
- One application service and profile remain command and durable-state
  authority.
- A client never applies a patch without a matching view set, epoch, and base
  cursor.
- Writing a response does not acknowledge it; only a later client cursor does.
- Repeating an unacknowledged cursor is safe and deterministic.
- Every overflow or expired continuity path becomes an explicit reset.
- Coalescing preserves final current state; ordered diagnostics report loss.
- Queue and response memory remain bounded independently of swarm, torrent,
  piece, peer, file, and event counts.
- Closing or expiring a view set wakes waiters and releases its state.
- No task or socket handle enters the task-free semantic owner or reducer.
- Legacy Tauri, Android, and WebSocket subscribers remain functional.
- JSON and future binary codecs decode to the same semantic DTOs.
- Generated declarations and schema originate from Rust Serde shapes.
- Unknown additive object fields do not break v1; unknown control variants do
  not silently pass.
- No peer, piece, file, or storage payload crosses the application boundary.

## Non-Goals

- React, Zustand, CSS Modules, routing, virtualization, or visible UI changes;
- the stable peer inspection projection;
- WebSocket view-set streaming or Tauri Channel migration;
- Android contract or presentation migration;
- public/LAN remote access, accounts, pairing, TLS, relay, or CORS product UX;
- CBOR or another binary codec;
- torrent removal, content deletion, archive, labels, or queue semantics;
- arbitrary field queries or generic JSON Patch;
- public swarm, speed, CPU, RSS, or rendering claims; and
- engine scheduling, storage, discovery, protocol, or persistence changes
  except a same-boundary bug exposed by the controlled view evidence.

Collection row removal is proven with deterministic source replacement. It
does not require adding a destructive torrent command.

## Escalation Contract

Proceed without routine approval for internal refactoring of `views`, adding
the recorded generation and validation dependencies after license review,
updating generated artifacts and lockfiles, adding loopback HTTP routes,
running controlled libtorrent fixtures, cleaning temporary artifacts, and
committing bounded checkpoints.

Stop for direction if implementation requires a stable public compatibility
promise, production remote authentication, a destructive torrent/content
operation, an Android or visible desktop change, a new process architecture,
an incompatible persistence migration, or engine/protocol behavior outside
the same-boundary controlled regression.

## Stopping Condition

This tactical is complete when all four stages pass, the controlled TypeScript
polling client observes the exact libtorrent-seeded download from add through
verified publication and joined cleanup, generated Rust/TypeScript/schema
artifacts are deterministic, the legacy subscription clients remain green,
the owning topics record actual evidence and remaining gaps, and the working
tree is committed and clean.

The next slice is the bounded peer inspection projection, followed by the
React/Zustand/virtualized-table foundation.

## Implementation Record

### Checkpoint 1: semantic owner

Implemented the task-free leased view-set owner in `rstorrent-session` and
adapted it to the existing coherent `ViewHub` models. The application service
now exposes owner-scoped open, replace, lookup, and close operations and closes
all sets before joined shutdown. Existing Tactical `008` subscribers and new
view sets receive the same durable, activity, and diagnostic publication
edges.

The implementation tightened the requested queue ceiling from the tactical's
provisional 4 MiB to 512 KiB. This makes the retained queue ceiling match the
maximum eventual HTTP update response rather than allowing an accumulator that
cannot be emitted through the selected adapter. The initial 16 KiB minimum and
256 KiB default remain unchanged.

Added `schemars` `1.2.2` under its MIT license after registry review. It is
derived on the semantic DTO graph in this checkpoint; deterministic schema
emission and validation are Stage 2 work. The generated lock additions are
`schemars_derive` `1.2.2` and `serde_derive_internals` `0.30.0`.

Deterministic evidence at this checkpoint:

- 11 focused view-set tests pass, covering validation, initial snapshots,
  exact replay until acknowledgement, accumulated next state, independent
  clients, atomic view replacement/removal, owner isolation, cursor mismatch,
  overflow reset with epoch rotation and fresh snapshots, lease cleanup,
  explicit close wakeup, and application-shutdown wakeup;
- `cargo clippy -p rstorrent-session --tests -- -D warnings` passes; and
- the legacy subscription implementation remains in the same publication
  paths and is retained for the workspace regression gate.

At Checkpoint 1, Stage 2 through Stage 4 remained pending.

### Checkpoint 2: generated boundary and pure reducer

Extended the deterministic exporter to emit:

- `clients/web/src/api/generated/v1.ts` from `ts-rs`;
- `clients/web/src/api/generated/v1.schema.json` from `schemars`;
- the retained legacy reactive fixture; and
- `clients/web/src/fixtures/view-set-trace.json` from Rust DTO values.

The handwritten `src/api/index.ts` is the stable import barrel. Existing web
code now imports through it, and the old `src/generated/contract.ts` output is
removed. Re-running `npm run generate` after generation left all four outputs
unchanged.

Added Ajv `8.20.0` under its MIT license after registry and lockfile review.
It validates generated structural shapes while existing focused TypeScript
checks continue to own canonical decimals, collection/range bounds, and
cross-field invariants. Generated schemas leave additive object properties
open but reject unknown tagged variants and enums. A regression proves that
`prepared`, previously omitted from the handwritten storage-state validator,
is accepted through the generated `StorageState` definition; no replacement
storage-state list exists in TypeScript.

The new task-free reducer stores projections by client `view_id`, checks view
set, epoch, base cursor, and projection continuity, treats an already-applied
batch as idempotent, applies removal before later upsert, and clears stale
state on an explicit epoch reset. Runtime transport, polling, abort, and timer
ownership remain outside it.

Evidence at this checkpoint:

- `cargo clippy -p rstorrent-gateway --all-targets -- -D warnings` passes;
- `cargo test -p rstorrent-gateway --no-fail-fast` passes (2 tests);
- `npm run typecheck --prefix clients/web` passes;
- `npm test --prefix clients/web` passes (13 tests, 1 opt-in integration test
  skipped); and
- `npm run build --prefix clients/web` passes.

Stage 3 and Stage 4 remain pending.
