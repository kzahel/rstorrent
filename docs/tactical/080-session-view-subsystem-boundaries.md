# Tactical 080: Session View Subsystem Boundaries

Status: Completed on 2026-08-04. The behavior-preserving source-shape
refactor, categorized private tests, full consumer validation, and execution
record are complete. No Rust API, generated contract, resource policy, lock,
queue, or task ownership changed.

Topics: `code-organization-and-refactoring`, `application-view-api`,
`application-connection-architecture`, `client-view-delivery-policy`

Dependencies: completed Tacticals
[`033`](033-headless-view-set-foundation.md),
[`048`](048-unified-view-delivery-and-tauri-migration.md), and
[`060`](060-multiplexed-application-websocket.md) establish the semantic view
contract, leased delivery owner, and transport equivalence that this refactor
must preserve. Completed Tactical
[`079`](079-engine-driver-source-shape.md) supplies the tested source-shape
and private-test extraction method.

## Decision And Motivation

Refactor the `rstorrent-session` view subsystem around its existing owners
before more product projections and delivery policy accumulate there.

This slice was selected because the current two-file shape has a concrete
dependency problem rather than only large files:

- `views.rs` defines public view DTOs, current projection models, `ViewHub`,
  legacy subscriptions, patch construction, patch coalescing, and range
  algorithms, while importing `ViewSetInner`, `ViewSetUpdate`, and view-set
  limits from `view_sets.rs`;
- `view_sets.rs` defines public view-set DTOs, the leased accumulator, cursor
  and replay behavior, lease reaping, and validation, while importing private
  `HubState`, `ViewHub`, patch helpers, snapshots, and subscription contracts
  from `views.rs`;
- `view_sets.rs` implements public methods directly on `ViewHub` even though
  the concrete hub state lives in `views.rs`;
- `HubState` stores `ViewSetInner`, while a `ViewSet` stores a weak reference
  to `HubState` so reset can reconstruct snapshots; and
- both files retain several unrelated private test families inline.

The current behavior is mature and deliberately shared by HTTP polling,
multiplexed WebSocket delivery, Tauri Channels, generated TypeScript/schema,
and the legacy single-subscription compatibility surface. The refactor must
make that ownership easier to follow without redesigning it.

The selected outcome is one private view subsystem with deliberate public
facades and one-way internal dependencies. `ViewHub` remains the current
projection and registry coordinator. A legacy `ViewSubscription` remains one
bounded queue owner. A leased view set remains one bounded cursor, replay,
reset, cadence, and lease owner. Pure mapping, diff, coalescing, and range
logic move behind explicit internal seams.

## Reconciled Baseline

The planning baseline is commit `570158d` on 2026-08-04. No production source
changed between the earlier `db4a092` source snapshot and this baseline.

| File | Production lines | Test lines | Total lines | Relevant contents |
| --- | ---: | ---: | ---: | --- |
| `crates/rstorrent-session/src/views.rs` | 4,468 | 1,145 | 5,613 | Contract DTOs, projection models, hub state, legacy subscriptions, patches, coalescing, and ranges. |
| `crates/rstorrent-session/src/view_sets.rs` | 1,400 | 809 | 2,209 | View-set contract, leased accumulator, replay/reset, validation, reaper, and `ViewHub` methods. |
| Combined | 5,868 | 1,954 | 7,822 | One mature semantic subsystem with two-way private implementation knowledge. |

`cargo test -p rstorrent-session --lib -- --list` reports 119 tests and zero
benchmarks. Eighteen tests currently live under `views::tests` and fifteen
under `view_sets::tests`, for 33 focused tests. The listing command completed
successfully; no implementation test suite was run for this documentation-only
slice.

The generated v1 artifacts are byte-stable baseline gates:

| Artifact | SHA-256 at `570158d` |
| --- | --- |
| `clients/web/src/api/generated/v1.ts` | `61391084663698b4c7be0ae7bc98fe55005a55c47036e76a3a4f03a95f84d531` |
| `clients/web/src/api/generated/v1.schema.json` | `0e7769f536f23fa7afb62b29ebd3702f4e1a2c1849d9b90b4eb3aa4638918d80` |
| `clients/web/src/fixtures/reactive-trace.json` | `89763804a6e004d3bc69d199034e8b7a153099c8cbf89d8a47da0189e3f6d677` |
| `clients/web/src/fixtures/view-set-trace.json` | `88119d6482527addbd08d81199dc714eaa8167c0b7c379efd6c47172db8a4ffe` |

The crate-root exports in `rstorrent-session/src/lib.rs` are the public Rust
surface. Gateway, Android, and desktop consumers import those root names, not
the private module paths. The refactor must retain those paths and the
Serde/`schemars`/`ts-rs`/UniFFI derivations that make the portable contract.

## Desired Outcome And Stopping Condition

The tactical stops when all of the following are true:

- public view and view-set names retain their existing
  `rstorrent_session::*` paths, visibility, derives, serialization, schema,
  and semantics;
- `views.rs` and `view_sets.rs` no longer import each other's private concrete
  state in both directions;
- the leased view-set implementation no longer implements methods on
  `ViewHub` from a sibling module;
- `HubState`, subscriber state, and view-set accumulator state each have one
  identifiable implementation owner;
- lower-level queue, diff, coalescing, and range modules do not import
  `HubState` or application, gateway, transport, or platform owners;
- hub coordination supplies snapshots and patches to delivery owners without
  making those owners inspect the hub's current projection maps;
- the existing single-subscription and leased-view-set APIs retain their
  distinct limits, continuity models, and close behavior;
- no task, timer, channel, queue, allocation bound, lock, cadence, cursor,
  epoch, lease, or reset policy changes;
- all 33 focused test cases and all 119 session library tests remain present,
  except for mechanical module-path changes caused by categorized private
  test files;
- the four generated artifacts reproduce byte-for-byte at their baseline
  hashes;
- before/after source counts and the final internal dependency map are
  recorded as navigation evidence; and
- the complete validation matrix passes without launching a visible product
  client or using a public swarm.

An approximate target layout is:

```text
rstorrent-session/src/
  views.rs                    private subsystem facade and public re-exports
  views/
    contract.rs               portable view and view-set DTOs and limits
    model.rs                  current projection models and mapping
    diff.rs                   snapshot/patch construction and coalescing
    ranges.rs                 pure canonical range operations
    hub.rs                    ViewHub coordination and owned registries
    subscription.rs           legacy bounded subscription queue
    view_set.rs               leased accumulator, cursor, replay and lease
    tests/
      mod.rs
      contract.rs
      projection.rs
      subscription.rs
      view_set.rs
      support.rs
```

Exact filenames and whether a small compatibility facade remains at
`view_sets.rs` may change after imports and fixtures are moved. Do not create
empty categories, one-file-per-type layout, or a facade that merely forwards
every private implementation detail.

## Target Dependency Direction

Higher-level coordination may depend inward on lower-level values and
operations:

```text
hub coordinator
  |-> current projection models -> portable contract values
  |-> snapshot/patch diff       -> contract + pure range operations
  |-> subscription accumulator  -> contract + patch coalescing
  `-> view-set accumulator      -> contract + patch coalescing
```

More precisely:

- portable contract values depend on Serde/schema/generation support and
  other portable session DTOs, not on mutexes, `Notify`, task handles, or hub
  state;
- range and patch-coalescing logic is deterministic and independently
  testable;
- projection models may depend on engine snapshots and the existing file,
  tracker, speed, DHT, diagnostic, and control models;
- `ViewHub` owns current projection state, revision, live weak subscription
  handles, live leased view-set handles, and speed-interest notification;
- each delivery accumulator owns its queue, byte accounting, notification,
  close state, and continuity state without reading the hub's private maps;
  and
- snapshot reconstruction and registry mutation happen through the hub
  coordinator or an equally narrow outer operation, not by teaching a lower
  delivery module the `HubState` representation.

The final implementation may place the public `ViewSet` handle beside the hub
coordinator if that is the simplest way to preserve reset-from-current-state
without a module cycle. Do not introduce a trait, callback framework, actor,
or erased service interface solely to make the diagram literal.

## Owner, Task, Lock, And Cancellation Map

| Owner | Retained state and behavior | Lifetime and termination |
| --- | --- | --- |
| `ViewHub` / hub coordinator | Current torrent, storage, disk, DHT, speed, diagnostics, subscription registry, view-set registry, revision and projection publication | One `ApplicationService`; application shutdown closes view sets, wakes waiters, and releases projection state. |
| Legacy subscriber | One selector/projection, epoch/sequence, bounded queue, cadence deadline, reset state and `Notify` | Explicit close, last-handle drop, hub drop, or application shutdown. It owns no task. |
| Leased view-set accumulator | Owner identity, requested specs, epoch/cursors, one unacknowledged batch, pending/coalesced updates, limits, lease time, one-consumer guard and `Notify` | Explicit close, owner close, lease expiry, application shutdown, or last handle after registry removal. It owns no task. |
| View-set lease reaper | Cancellation token and the sole periodic expiry task | Owned by `ApplicationService`; shutdown cancels and awaits the task before hub teardown. |
| HTTP/WebSocket/Tauri adapters | Existing wait, attachment, acknowledgement and transport tasks | Unchanged and outside this source move; their existing cancellation and joins remain authoritative. |

The current lock domains remain the hub mutex, each legacy subscription queue
mutex, each leased view-set state mutex, the speed-history mutex, and existing
diagnostic internals. Preserve the established outer-to-inner order used to
publish or reset. Do not await while holding these locks, add a lock, call
unknown client code under a lock, or create a path that can re-enter the hub
while a hub guard is live. Lock poisoning must retain the current typed
internal-error behavior.

## Stable Contracts And Invariants

### Portable contract

- `VIEW_CONTRACT_VERSION` remains `2` and `API_VERSION` remains `1`.
- All existing enums, tagged variants, field names, optionality, decimal
  string representations, default behavior, and generated declarations stay
  identical.
- The crate-root export surface and feature-gated UniFFI derivations remain
  intact.
- Unknown additive object fields and closed-variant behavior remain owned by
  the existing generated/schema and semantic validation boundary.

### Legacy subscriptions

- Queue limits remain 4 KiB through 4 MiB and maximum cadence remains 60
  seconds.
- Sequence, base revision, revision, overflow reset, resync, independent
  queues, diagnostic ordering, delivery spacing, close wakeup, and last-handle
  cleanup preserve their exact meanings.
- This compatibility surface is not deleted merely because leased view sets
  are the mature product path.

### Leased view sets

- Limits remain 32 live sets per application, 8 per owner, 16 views per set,
  64-byte view IDs, a 16--512 KiB steady queue with 256 KiB default, a 16 MiB
  snapshot ceiling, 20-second maximum wait, five-minute lease, five-second
  default reaper interval, and 60-second maximum delivery interval.
- One emitted batch remains unacknowledged and replayable until the exact
  resulting cursor is supplied. Only one consumer may drain a set.
- Epoch, base cursor, cursor, and durable revision remain distinct. Cursor
  mismatch and overflow rotate through explicit reset and coherent snapshots.
- Client operations renew the lease; engine publication, queue wakeups,
  response construction, and transport heartbeat do not.
- Owner isolation, unpredictable ID generation, explicit close, expiry,
  shutdown wakeup, and per-view cadence retain current behavior.

### Projection and patch truth

- Torrent list/summary, piece activity, Disk, DHT, Speed, Peers, Swarm, Files,
  Trackers, and Diagnostics retain their exact snapshot, keyed patch,
  replacement, empty/unavailable, and terminal-removal semantics.
- Later removal and later upsert precedence remain exact for coalesced keyed
  collections. Ordered Diagnostics remain bounded and never become a
  latest-value collection.
- Canonical ranges remain sorted, nonoverlapping, half-open, overflow-safe,
  and do not expand into per-piece indices.
- Projection interest and speed-clock interest remain derived from live
  subscribers and view sets without a new observer or task.

## Scope

- Record the exact public root exports, focused test list, generated hashes,
  source counts, and current cross-module imports before moving code.
- Move the 33 existing focused tests into private categorized child modules,
  preserving assertions, ignored status, fixtures, and test-only visibility.
- Extract portable contract values and limits without changing their derives,
  serialization order, generated output, or public root paths.
- Extract pure range and patch/diff operations and retain deterministic tests
  at that boundary.
- Give current projection models and snapshot construction a private owner.
- Give the legacy subscription queue and leased view-set accumulator separate
  private implementation homes.
- Move all `ViewHub` methods and hub-registry coordination to the hub owner so
  no sibling delivery module extends the hub from outside.
- Remove the two-way private-state imports while preserving reset snapshots,
  speed-interest notification, diagnostic interest, expiry, and close.
- Add concise module documentation naming each owner's invariant,
  dependencies, synchronization, and absence or ownership of tasks.
- Update this tactical and the living refactor topic with final layout,
  counts, dependency direction, validation, and any rejected seam.

Small same-boundary cleanup is allowed for moved imports, obsolete
qualification, duplicated test support, and private names that become
misleading after extraction. It must not become semantic cleanup.

## Test Placement Contract

- Contract and pure algorithm tests live beside their private modules or in a
  focused child test module when they share contract fixtures.
- Hub/projection tests cover coherent snapshots and typed patches without
  transport setup.
- Subscription tests cover independent queues, cadence, overflow, resync,
  diagnostics, and close.
- View-set tests cover validation, owner isolation, replay, acknowledgement,
  reset, snapshot ceiling, cadence, expiry, reaper shutdown, and waiter
  wakeup.
- Shared `ServiceSnapshot`, torrent, peer, tracker, disk, and large-file
  builders remain test-only and move to `support` only when used by multiple
  families.
- Gateway and client interoperability tests remain outside the session crate
  because they prove behavior through public APIs.

Moving tests is a navigation step, not permission to merge scenarios, weaken
exact assertions, convert deterministic coordination to sleeps, or expose
private implementation publicly.

## Non-Goals

- A new view, field, patch, error variant, contract/API version, capability,
  generated type, schema change, or compatibility promise.
- Changing interval-only `ViewSpec` updates to avoid snapshots. That known
  delivery-policy behavior remains a separate semantic tactical.
- Snapshot reconstruction optimization, finer-grained patches, retained
  history, binary encoding, compression, or Tactical `057` performance work.
- HTTP, WebSocket, Tauri, relay, authentication, reconnect, transport
  scheduler, reducer, Zustand, or React changes.
- A new queue, task, channel, lock, timeout, cadence, lease, resource default,
  dependency, crate, trait hierarchy, actor, repository, or framework.
- Removing the legacy `ViewSubscription` API or changing adapter call sites
  merely to make the refactor smaller.
- Refactoring `ApplicationService`, `SessionStore`, engine views, web
  `LiveApplication`, TypeScript validation, or `VirtualTable`.
- Tactical `077` overlay implementation or Tactical `078` incoming seeding.
- A hard file-size gate, one-file-per-type layout, public-swarm evidence,
  visible client launch, emulator, or physical-device run.

## Completed Implementation

The implementation landed in independently green commits:

- `22544d3` moved the 33 focused tests out of the two production owners;
- `0c149ea` consolidated the leased view-set implementation under the private
  view subsystem;
- `50b76b0` extracted the contract, projection model, deterministic diff and
  range logic, and the two delivery accumulators while removing the concrete
  hub/view-set dependency cycle;
- `15b39f9` made `views.rs` a deliberate facade, gave coordination one
  `hub.rs` owner, and categorized the focused tests; and
- `beb8d2f` corrected the controlled gateway harnesses to wait for semantic
  publication and verify the accepted metainfo-root path rather than the
  retired hash-named staging path.

The final source layout is:

```text
rstorrent-session/src/
  views.rs                         45-line private facade
  views/
    contract.rs                 1,314 lines: portable DTOs and limits
    model.rs                    1,136 lines: projection state and mapping
    diff.rs                       749 lines: snapshot diffs and coalescing
    ranges.rs                     110 lines: canonical range operations
    hub.rs                      1,706 lines: mutable coordination/registries
    subscription.rs              279 lines: legacy bounded accumulator
    view_set.rs                  680 lines: leased bounded accumulator
    tests/
      mod.rs                       6 lines: categorized test root
      projection.rs              614 lines
      ranges.rs                   16 lines
      subscription.rs            335 lines
      support.rs                 188 lines
      view_set.rs                806 lines
```

The eight production/facade files contain 6,019 physical lines and the six
private test files contain 1,965 physical lines, for 7,984 total. The baseline
was 5,868 production plus 1,954 test lines, or 7,822 total. The modest physical
increase is explicit module documentation, imports, visibility seams, and test
roots; line reduction was not an objective. The important result is that the
former 5,613- and 2,209-line mixed owners no longer exist.

All 18 legacy focused tests now live under projection, range, or subscription
families. All 15 leased-view tests retain their leaf names under the
`view_set` owner. `cargo test -p rstorrent-session --lib -- --list` still
reports exactly 119 tests and zero benchmarks.

The final dependency direction is:

```text
views facade
  |-> portable contract
  |-> pure ranges
  |-> projection model -> contract + ranges + engine/session observations
  |-> diff             -> contract + model + ranges
  |-> subscription     -> contract + diff coalescing
  |-> view_set         -> contract + diff coalescing + spec validation
  `-> hub              -> every inward owner above
```

`hub.rs` is the only owner of `HubState`, every `ViewHub` method, the public
`ViewSubscription` and `ViewSet` handles, registry mutation, reset snapshot
reconstruction, speed-interest wakeups, and the cancellable lease-reaper task.
`subscription.rs` and `view_set.rs` own their mutex-protected queue and
continuity state but import neither `HubState` nor `ViewHub`. `diff.rs` and
`ranges.rs` own no shared state or task. `contract.rs` contains no mutex,
notification, task handle, runtime owner, or transport/platform dependency.

The public `ViewSet` handle deliberately remains with the hub coordinator.
That preserves reset-from-current-state without adding a trait, callback,
actor, or lower-layer hub dependency. No empty `contract` test category was
created: generated-byte gates and existing validation cases already exercise
that seam, while the categorized test files reflect actual fixture families.

## Validation Evidence

- `cargo test -p rstorrent-session --lib -- --list` reports 119 tests and zero
  benchmarks; `cargo test -p rstorrent-session --lib` passes all 119.
- `npm run generate --prefix clients/web` reproduces the four baseline
  artifacts byte-for-byte at the recorded SHA-256 values, and
  `git diff --exit-code -- clients/web/src/api/generated
  clients/web/src/fixtures` is clean.
- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`,
  `cargo test --workspace`, and `git diff --check` pass.
- `npm run typecheck --prefix clients/web`, `npm test --prefix clients/web`,
  and `npm run build --prefix clients/web` pass.
- The raw cross-target Cargo command found the installed Rust target but not
  the NDK compiler shim. The repository's established
  `cargo ndk -t x86_64 -t arm64-v8a -P 28 check -p rstorrent-android --lib`
  command then passed both `x86_64-linux-android` and
  `aarch64-linux-android` with the installed NDK.
- `gateway_view_set_surface.py` passes against pinned libtorrent `2.0.13.0`
  with eight batches, exact 40,000 requested/received/stored bytes, exact
  payload SHA-1, explicit view-set close, joined gateway shutdown, and clean
  temporary cleanup.
- `gateway_application_connection_surface.py` passes against the same pinned
  libtorrent with eight semantic updates, exact payload SHA-1, one accepted
  connection, seven acknowledged view batches, zero stream errors, joined
  shutdown, and clean temporary cleanup.

No public swarm, visible client, browser window, Tauri window, emulator, or
physical device was used.

## Implementation Sequence And Intermediate Gates

The implementation followed these independently green gates:

1. **Baseline and dependency gate.** Reconcile the starting commit, record
   exact counts, exports, hashes, test names, feature derives, lock order, and
   the current two-way imports. Run the focused session tests.
2. **Private test-layout gate.** Move only the 33 view and view-set tests into
   categorized child modules. Preserve their leaf names and assertions, keep
   production source unchanged, and confirm 119 session tests still list and
   pass.
3. **Contract and pure-logic gate.** Extract DTOs, limits, range operations,
   patch construction, and coalescing. Regenerate the v1 contract and require
   byte-identical artifacts before moving mutex-owned state.
4. **Projection and subscription gate.** Extract current models, snapshot
   construction, legacy subscriber state, queue accounting, resync, and
   diagnostic delivery. Run projection, range, coalescing, cadence, overflow,
   and independent-subscriber tests.
5. **Leased view-set and hub gate.** Move the accumulator, cursor/replay/reset
   state, owner validation, and lease reaper. Consolidate `ViewHub` methods in
   the hub owner and eliminate the two-way concrete-state imports. Run every
   view-set lifecycle, replay, reset, owner, expiry, and shutdown test.
6. **Boundary audit and full evidence.** Narrow visibility, document modules,
   inspect the final import graph and lock order, run generated, Rust,
   frontend, adapter, Android compile, and controlled loopback evidence, then
   record final counts and update the living topic.

Each gate must compile and pass before the next. Do not combine a mechanical
move, behavior change, and test rewrite in one slice to make failures
ambiguous. Reasonable commits may land at each green gate.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Mechanical shape | Before/after production and test counts; 33 focused tests and 119 session tests retained; unchanged crate-root exports and derives; no production visibility widening; one-way private import review. |
| Contract drift | `npm run generate:contract` reproduces all four baseline SHA-256 hashes and leaves generated TypeScript, schema, and fixtures unchanged. |
| Deterministic session | Contract validation, all projection snapshots and patches, range operations, patch coalescing, legacy queue overflow/resync, view-set replay/reset/cadence/owner/expiry/reaper/shutdown. |
| Rust adapters | Gateway HTTP and WebSocket application-connection tests, desktop acknowledged delivery tests, Android host tests, and complete workspace tests. |
| Frontend consumers | Typecheck, Vitest reducer/validation/controller/HTTP/WebSocket/Tauri suites, production build, and generated-validator drift. |
| Platform compile | `rstorrent-android` checks for the established `aarch64-linux-android` and `x86_64-linux-android` targets so moved derives and exports remain portable. |
| Controlled loopback | Existing gateway polling view-set and multiplexed application-connection harnesses retain exact cursor/application traces, verified payload hash, joined shutdown, and cleanup. |
| Repository | `cargo fmt --all -- --check`, warning-denying workspace Clippy, workspace tests, and `git diff --check`. |

Representative commands are:

```bash
source ~/.profile
cargo test -p rstorrent-session --lib
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo check -p rstorrent-android --target aarch64-linux-android
cargo check -p rstorrent-android --target x86_64-linux-android
npm run generate --prefix clients/web
git diff --exit-code -- clients/web/src/api/generated clients/web/src/fixtures
npm run typecheck --prefix clients/web
npm test --prefix clients/web
npm run build --prefix clients/web
uv run --project tests/interop --locked \
  python tests/interop/gateway_view_set_surface.py
uv run --project tests/interop --locked \
  python tests/interop/gateway_application_connection_surface.py
git diff --check
```

No public network, visible browser, Tauri window, emulator, or physical device
is required because this tactical changes no corresponding product behavior.

## Escalation And Handoff

Implementation may choose exact private module and test filenames, move
closely coupled private values with their owner, keep a narrow compatibility
facade, use private or `pub(crate)` visibility, and make mechanical
same-boundary cleanup without further direction.

Stop for maintainer direction if the refactor requires changing generated
bytes, public root paths, serialization/schema/UniFFI shape, a queue or
resource bound, cursor/reset/lease/cadence semantics, task or lock ownership,
transport behavior, error meaning, a dependency or crate, the legacy
subscription compatibility surface, or any non-goal above. A failing existing
test is evidence to reconcile, not permission to weaken the contract.

After this tactical completes, return to the accepted feature choice rather
than automatically continuing a size queue. Tactical `078` remains the
planned feature-driven next boundary. Selective-storage internals remain the
leading engine-only refactor candidate to reassess after immutable seeding
reads establish their actual seam.
