# Tactical 180: Typed Settings Patches And Draft Convergence

Status: **Complete (2026-08-27).** Typed resource patches replace both old
settings commands without compatibility aliases. Rust, browser/Tauri,
Android, and generated Swift/Kotlin boundaries agree; complete cloned updates,
reset/reconnect, failure, replay, and live listener handover evidence pass.
The unavailable macOS-only Swift simulator/archive compile remains Tactical
`176`'s host gate, and Tactical `176` resumes as the sole **Now**.

Topics:
[`settings-mutation-and-draft-consistency`](../topics/settings-mutation-and-draft-consistency.md),
[`application-control`](../topics/application-control.md),
[`application-view-api`](../topics/application-view-api.md),
[`client-persistence`](../topics/client-persistence.md),
[`client-view-delivery-policy`](../topics/client-view-delivery-policy.md),
[`client-surfaces`](../topics/client-surfaces.md),
[`web-ui-design`](../topics/web-ui-design.md), and
[`capability-readiness`](../topics/capability-readiness.md).

## Motivation And Desired Outcome

An active torrent publishes unrelated activity frequently enough that the web
transfer-limit editor repeatedly receives a newly allocated authoritative
limits value. Its current synchronization effect copies that value over the
local form draft, so unchecking a limit appears to work and then immediately
reverts. The same ownership pattern exists in the global client-settings
editor. Stopping the live update source hides the problem rather than fixing
it.

The command API also exposes two inconsistent mutation shapes: client settings
require resending the complete `ClientSettings`, while torrent upload and
download limits are forced into one pair-specific command. The desired result
is one typed partial-update contract per settings resource and a reusable
client edit model that remains correct under complete cloned view updates,
command/view delay, failure, stale revisions, and reconnect resets.

At the stopping condition:

- callers can update any non-empty subset of declared client settings in one
  request;
- callers can update torrent upload, download, or both limits in one request;
- supplied values validate and persist atomically after merge with current
  durable state, while omitted values remain unchanged;
- browser, Tauri, Android, generated TypeScript, JSON Schema, Kotlin/UniFFI,
  and Swift/UniFFI boundaries agree on the contract;
- web and Android settings editors preserve dirty and submitted fields until a
  correlated receipt and sufficiently new authoritative view converge; and
- all acceptance tests still pass when complete settings/rows are republished
  continuously with fresh object identities.

## Dependencies And Entry Gate

This slice builds on completed Tacticals `007`, `008`, `033`, `034`, `048`,
`060`, `084`, `097`, and `134`: semantic commands, generated contracts,
durable request receipts, leased views, client state ownership, settings
persistence/reconciliation, and hierarchical transfer-limit enforcement are
already present.

Before starting implementation:

1. Reconcile the working tree and complete the current sole-Now Tactical
   `179`, because it changes the application-state schema and removes
   disposable compatibility readers near this slice's persistence boundary.
2. Confirm that no other queued tactical is changing `ClientSettings`, the
   command union, response envelopes, generated bindings, or the same web and
   Android settings components.
3. Re-run the deterministic active-update characterization and record the
   exact pre-fix failure. Do not rely only on a frozen `autoplay=0` demo.

There is no BitTorrent protocol behavior or engine scheduling algorithm in
scope, so no BEP or libtorrent source survey is required. The normative inputs
for this application/client slice are the owning topics above, the generated
contract, current persistence tests, and the existing web/Android state-owner
patterns. Inspect the exact current implementations and tests before
finalizing reducer transitions; record their paths in the implementation
evidence.

## Stable Scenario Subset

This tactical must make all scenarios `SM-001` through `SM-010` in the owning
topic pass. The following executable cases are mandatory:

1. **Active torrent edit:** with active progress/peer publication continuing,
   toggle only the download limit off, wait across multiple updates, and save.
   The draft remains visible, the emitted patch contains only download, and
   the authoritative view converges to Unlimited.
2. **Independent and combined directions:** update only upload, only download,
   and both in one torrent patch. The first two preserve the other direction;
   the combined case produces one transaction/revision.
3. **Global settings edit:** change one client setting while complete runtime
   settings values are repeatedly republished. Unedited fields follow newer
   authority; the edited field remains local until convergence.
4. **Cross-field transaction:** patch listener policy and preferred port
   together through a combination that is valid only as a merged candidate.
   Validate the final candidate, not intermediate field order. An invalid
   combination persists neither field.
5. **No-op and replay:** a semantically unchanged patch leaves the revision
   unchanged; exact request replay returns the prior receipt and causes no
   second reconciliation.
6. **Stale revision and failure:** a stale expected revision, same-field
   authority change after editing began, validation error, injected store
   failure, and transport failure leave durable state unchanged and preserve
   the draft with a bounded actionable state.
7. **Delayed projection:** deliver older and unrelated full updates after the
   command succeeds but before the accepted revision appears. Submitted values
   remain pending. A matching update at or beyond the accepted revision clears
   only the submitted overlay.
8. **Edit while pending:** edit a submitted field again before the first value
   converges. Confirmation of the captured submission must not erase the newer
   local edit.
9. **Reset and identity:** replace the view set with a fresh snapshot while a
   draft or accepted mutation exists; preserve same-resource overlays by the
   normal revision rules. Switching or removing the torrent must not leak the
   draft into another resource.
10. **Adversarial identity churn:** repeat semantically equal configured values
    through separately allocated objects/records at the highest existing
    deterministic delivery cadence. No draft transition may depend on
    reference identity.

## Contract And Persistence Changes

Add closed generated records `TorrentSettingsPatch` and
`ClientSettingsPatch`. Every current top-level setting has its own typed
optional patch member. Add semantic commands:

```text
update_torrent_settings { torrent_id, patch }
update_client_settings  { patch }
```

Omission means unchanged. Reject an empty patch and unknown fields. Explicit
domain values perform resets, including `TransferRateLimit::Unlimited`; null
does not acquire an implicit clear meaning. Retain the request envelope's
request ID and optional expected revision.

For each command, the application service must:

1. resolve an exact durable request replay before new mutation;
2. reject a stale expected revision before mutation;
3. load the current durable resource and merge every supplied property into a
   candidate value;
4. validate the complete candidate and its cross-field invariants once;
5. persist the candidate, request receipt, and revision outcome with the
   existing transaction guarantees;
6. keep the revision unchanged for a semantic no-op; and
7. after commit or exact replay, reconcile only affected runtime domains using
   the current stable owners.

Both transfer directions remain independently optional. Supplying both gives
the caller atomic grouping; the server does not force the pair into every
mutation.

Supersede `SetClientSettings` and `SetTorrentTransferLimits`. Update all
first-party producers, fixtures, diagnostics, and tests in the same change;
do not keep a compatibility-only alias for the unsupported disposable
incubation contract. Explicit user direction requires no alias, adapter,
parallel endpoint, protocol-version bridge, or generated compatibility shape.
The unfrozen internal v1 envelope remains current while its command union is
replaced in place; the first supported API baseline has not been declared.

The successful adapter result must expose at least the correlated request ID
and resulting durable revision. It may reuse the current `ResponseEnvelope`;
removing its complete `ServiceSnapshot` is not required here. Revisions remain
opaque decimal strings across JavaScript and generated foreign bindings.

## Client Draft State Machine

Implement one small pure, value-semantic draft reducer/model per client
language and reuse it across the torrent and global settings editors. Shared
generated patch types are mandatory; identical presentation code is not.

The model tracks the resource key, latest authoritative values/revision, each
dirty field's authoritative edit base, typed local field overlays, one captured
in-flight patch, edits made after capture, accepted revision, and one bounded
failure/conflict. It supports these transitions:

- authority initializes a pristine resource and updates clean fields;
- editing adds or removes a field overlay according to semantic equality;
- a same-field authority change away from the edit base marks conflict without
  erasing or silently rebasing the overlay;
- submit captures the current non-empty patch and starts one request;
- success changes the captured fields to awaiting-view at the accepted
  revision;
- matching sufficiently new authority clears the captured overlay only when
  no newer local edit supersedes it;
- stale, validation, persistence, and transport errors retain overlays;
- retry uses a new request ID and a newly captured current patch;
- reset/discard explicitly removes overlays; and
- resource-key change or authoritative removal terminates the old editor state
  without reusing it for the new resource.

The web implementation keeps promises, abort/cancellation, and transport
handles in the existing application/inspection controller. Zustand may expose
serializable authoritative and pending facts but does not own promises or a
second engine replica. React effects may notify the reducer of changed
authority; they may not unconditionally copy authority into draft state.

Android keeps request coroutine ownership in the existing service/view-model
boundary and feeds the same semantic transitions to Compose. Recomposition and
new Kotlin record identity do not reset a draft. If no Apple settings editor
exists, generated Swift compilation and boundary construction are sufficient;
do not invent a new Apple presentation surface in this tactical.

Each editor permits at most one in-flight settings request. A person may edit
again while that request is pending, but a second submit waits until it settles.
The model must distinguish the newer edits from the captured patch. Drafts
remain ephemeral and bounded by the closed patch fields.

## Owner, Task, Cancellation, And Data Flow

```text
presentation input
    -> per-resource typed draft reducer
    -> non-empty patch captured with authoritative revision
    -> existing client application/controller request task
    -> application service validation + store transaction + receipt
    -> existing settings/runtime reconcilers
    -> authoritative view batch with durable revision
    -> client view reducer/store
    -> draft convergence reducer
    -> presentation applied/pending/conflict state
```

- The application service and store own merge, validation, commit, receipt,
  replay, and revision. Patch records remain runtime independent.
- Existing session bandwidth, listener/network, scheduler, security, and
  per-torrent bandwidth owners apply resulting intent. Add no new long-lived
  task and do not replace peer, torrent, listener, or discovery generations for
  an unrelated property.
- Existing client controller/request scopes own transport cancellation and
  termination. Component unmount cancels observation/request work according to
  current adapter policy; it does not mutate durable settings implicitly.
- View producers remain free to send complete rows and fresh reset snapshots.
  The draft reducer is the only owner allowed to combine authority with local
  overlays.

## Resource Bounds And Edge Cases

- Patch cardinality is statically bounded by two torrent properties and the
  current closed `ClientSettings` fields. No strings, paths, maps, recursive
  merge values, or unbounded arrays select properties.
- Existing command payload and error-message bounds remain in force. Reject an
  empty or structurally invalid patch before store work.
- One editor holds at most one authoritative value, one overlay per declared
  field, one captured request, and one bounded error/conflict. Do not retain
  request or snapshot history.
- Merge and validate before persistence. Include minimum transfer rates,
  listener/port constraints, connection and slot bounds, active-download
  bounds, and every existing cross-field rule.
- A no-op may return the current durable revision and immediately converge if
  the local authority already matches it. It must still participate in durable
  request replay under the existing receipt contract.
- Unknown torrent, authoritatively removed resource, malformed decimal
  revision, duplicate request ID with different payload, store rollback,
  runtime degraded state, view reset, and out-of-order pre-acceptance view
  batches need explicit cases.
- Durable convergence is not the same as runtime application. Preserve current
  typed `applying`/`applied`/`degraded` projections and do not claim an
  effective network change from the durable receipt alone.

## Implementation Sequence And Gates

1. **Characterize ownership failure.** Add deterministic web tests that fail
   under continuous complete-row publication and a corresponding client-
   settings case. Add pure state-transition tables for delayed receipt/view,
   reset, conflict, and edit-while-pending. Gate: failures reproduce without a
   headed/manual client.
2. **Land pure patch contracts.** Add patch records, validation helpers,
   command variants, schema fixtures, and merge tests independent of runtime,
   filesystem, network, and async tasks. Gate: all subset, empty, no-op, and
   cross-field cases pass.
3. **Land transactional service behavior.** Wire commands through receipt,
   SQLite/in-memory stores, revision, replay, rollback, and existing
   reconcilers. Gate: deterministic service tests prove atomicity and restart
   durability where applicable.
4. **Regenerate every boundary.** Update JSON Schema, TypeScript, gateway,
   Tauri, UniFFI/Kotlin, Swift, fixtures, and all internal callers. Remove the
   superseded variants in the same stage. Gate: stale command names are absent
   outside completed historical docs and generated/platform builds compile.
5. **Adopt client draft ownership.** Implement the pure reducer/model and move
   web torrent/global settings plus Android settings controls to patches and
   revision-aware convergence. Gate: SM-001 through SM-010 pass in pure and
   component/integration tests with fresh object identity.
6. **Run proportional product evidence.** Exercise browser and Tauri adapters,
   Android Compose/service behavior, reconnect/reset, and failure injection.
   Record observed receipt-to-view convergence latency without changing
   cadence. Gate: all matrices below pass and no client relies on sparse
   delivery.
7. **Reconcile documentation.** Update the owning topics, generated-contract
   inventory, tactical evidence, and readiness queue. Record measured delivery
   cost as input to a separate optimization tactical; do not implement field
   masks opportunistically here.

## Validation Matrix

### Pure contract and reducer state

- Rust patch serialization/deserialization, unknown/empty rejection, merge,
  complete-candidate validation, semantic no-op, and independent/combined
  transfer directions.
- Request fingerprint/replay and expected-revision tests for every new command.
- Web and Android reducer tables for dirty, clean-field authority, submit,
  delayed acknowledgement, delayed view, no-op convergence, failure, conflict,
  reset, identity change, removal, and newer edit over an older submission.
- Generated schema and TypeScript validator fixtures for valid subsets and
  malformed values, including decimal revision handling.

### Scripted application and persistence

- In-memory and SQLite success, rollback, exact replay, different-payload
  request-ID conflict, stale revision, restart, and no-op revision cases.
- Runtime reconciliation observations proving only affected domains are woken
  or reconfigured and unrelated long-lived owners retain identity.
- Complete cloned torrent/client settings publications before acknowledgement,
  between receipt and convergence, and after a reset.

### Client and transport integration

- `npm run typecheck --prefix clients/web`
- `npm run test --prefix clients/web`
- deterministic browser E2E with active updates enabled, covering the torrent
  transfer editor and global client settings;
- browser HTTP/WebSocket and in-process Tauri command receipt/revision
  propagation, including reconnect/reset;
- Android JVM reducer/service tests and Compose interaction tests for both
  settings editors that currently exist; and
- generated UniFFI construction/round-trip tests for Kotlin and Swift.

### Platform builds

- the repository Rust format, clippy, and workspace-test baseline;
- web production build plus deterministic browser suite;
- Tauri native compile on the presubmit hosts affected by generated types;
- Android dual-ABI debug build and configured emulator smoke; and
- iOS simulator/archive compile for the regenerated Swift boundary when the
  current cross-platform testbed is available. No physical iOS interaction is
  required.

### Interoperability and live evidence

No public swarm, independent BitTorrent client, WAN, or throughput run is
required. The failure depends on first-party command/view timing, so
deterministic high-cadence local publication is the controlled evidence.

Record the exact commands, host/platform matrix, pre-fix failure, post-fix
results, and receipt-to-view latency observations in this tactical before
marking it complete.

## Implementation And Evidence

The application contract now has closed `ClientSettingsPatch` and
`TorrentSettingsPatch` records plus `update_client_settings` and
`update_torrent_settings` command variants. The old whole-client and forced-
pair variants are absent from generated TypeScript, JSON Schema, UniFFI
construction, adapters, and first-party producers. The only remaining old
wire names outside historical documentation are deliberate negative decoder
fixtures proving that those variants reject.

`crates/rstorrent-session/src/settings/contract.rs`, `control.rs`, `store.rs`,
and `application.rs` own closed-field validation, merge-then-validate,
expected revision, atomic persistence, semantic no-op, durable request replay,
and affected-domain runtime reconciliation. Store/service tests cover upload-
only, download-only, combined, cross-field validity, empty/unknown input,
rollback, replay, stale revision, and unchanged revision outcomes.

The web implementation in `clients/web/src/inspection/settings-draft.ts` and
`use-settings-draft.ts` is one pure value-semantic reducer/hook reused by the
global client-settings and selected-torrent editors. It records edit bases,
overlays, one captured submission, accepted revision, newer edits, conflicts,
and one bounded failure. The live, demo, HTTP/WebSocket, and Tauri adapters
submit only the generated non-empty patch with the current opaque durable
revision. The pre-fix characterization reproduced the reported failure: one
unrelated freshly allocated active-torrent row restored the limited download
value after the checkbox was cleared. Post-fix component coverage retains the
cleared draft across 24 complete cloned row updates; the global editor retains
its dirty field across 25 complete cloned runtime updates.

Android implements the same transitions in
`SettingsDraftModel.kt`, keeps coroutine/request ownership in
`ProductEngineService`, and applies sparse generated patches from both current
Compose settings editors. JVM tables and Compose interaction tests cover 24
fresh torrent rows, global updates, failure, conflict, reset, resource change,
and captured-submit convergence. iOS has no settings editor to migrate, so no
new Apple presentation was invented. Fresh generated Swift inspection proved
both patch records and both new command cases are present and both old cases
are absent.

SM-001 through SM-010 are covered across the Rust merge/store tests, the seven
web reducer tables, web component/demo autoplay cases, Android reducer and
Compose cases, generated-schema fixtures, and the controlled restart script.
The latter establishes a peer connection at a one-byte/s gate, changes the
fixed loopback listener while the torrent remains active, reconnects the
libtorrent oracle to the replacement endpoint, and then releases a bounded
256 KiB/s transfer. It also persists/reopens settings, injects a bind conflict,
recovers in the same generation, restarts again, and verifies joined cleanup.

## Validation Record

Validation run on Linux on 2026-08-27:

- `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, and
  `cargo test --workspace` pass. The workspace run includes 597 engine tests
  and 260 session tests plus every other workspace and doc-test target; only
  declared opt-in tests are ignored.
- `cargo check -p rstorrent-desktop` and
  `cargo build -p rstorrent-ios --release` pass on the host.
- `npm run generate --prefix clients/web`,
  `npm run typecheck --prefix clients/web`, and
  `npm run build --prefix clients/web` pass. The production build reports only
  its existing large-bundle warning and passes the CSP gate.
- `NODE_OPTIONS='--localstorage-file=/tmp/rstorrent-t180-vitest-localstorage.json'
  npm run test --prefix clients/web` passes 306 tests in 46 files with two
  declared skips. `npm run test:e2e --prefix clients/web` passes 34 cases with
  12 opt-in live cases skipped. The two active-settings autoplay cases pass
  separately.
- `clients/android/build.sh` produces the dual-ABI debug APK with arm64-v8a and
  x86_64 native libraries. `lintDebug testDebugUnitTest
  assembleDebugAndroidTest` passes, and `connectedDebugAndroidTest` passes all
  four tests on the configured headless `jstorrent-tablet` AVD, including both
  new settings interactions.
- fresh Swift generation and inspection pass on Linux. This host has no
  `swift`, `xcodegen`, Apple Rust targets, or Xcode, so the proportional iOS
  simulator/archive compile was unavailable; no physical-device interaction
  was required.
- `uv run --project tests/interop --locked python
  tests/interop/client_settings_restart.py` passes. It verifies exactly
  8,388,608 payload bytes and SHA-1
  `c995f4a05e42222e94d1133536701d6edd70dbc6`, one live listener handover,
  persisted restart, injected bind failure and recovery, zero terminal owners,
  bounded storage/peer high-water marks, joined shutdown, cleanup, and zero
  serious/critical Axe findings. Receipt to applied authoritative listener
  view measured 24.1 ms in this run.

## Delivery-Cost Observation And Deferral

The existing producer already omits `client_settings` from an unrelated
torrent-list patch; global settings are not blindly streamed on every active
update. A changing torrent is still cloned and encoded as one complete
`TorrentView`, including semantically unchanged transfer limits. Compact JSON
measurements from the checked-in one-torrent view-set trace are 3,220 bytes for
the initial batch, 1,157 bytes for its one-row update batch, 3,215 bytes for a
reset batch, 915 bytes for the changed row, 64 bytes for its transfer-limits
object, and 2,003 bytes for the complete client-settings runtime snapshot.

Source inspection shows one complete Rust row clone/serialization and one
client row/map reconstruction plus reducer/store notification per delivered
active-row batch. The adversarial tests deliberately force 24 torrent and 25
client-settings notifications with fresh identities; neither causes draft
loss. Resets still replace the complete snapshot and converge by resource key
and durable revision.

These observations do not justify inventing a sparse wire shape in this
correctness slice: the settings-specific repeated portion in the representative
active row is 64 bytes, and unchanged global settings are already suppressed.
The broader full-row allocation/render cost remains a real measurement target
under `client-view-delivery-policy` and Tactical `057`. A later bounded
optimization tactical should be opened only with representative multi-torrent
allocation, notification, reset-rate, and render evidence; SM-010 remains its
correctness gate.

## Non-Goals

- Field masks, JSON Patch, sparse row delivery, splitting stable and volatile
  projections, binary encoding, or changing view delivery cadence.
- Removing the complete `ServiceSnapshot` from every successful response; a
  later command-response optimization may do so with adapter evidence.
- Persisting drafts, automatically merging conflicts, multi-user editing, or
  introducing per-resource revision streams.
- Turning lifecycle actions, queue operations, file-priority collections, or
  other behavior-specific commands into generic properties.
- Redesigning runtime bandwidth enforcement, admission scheduling, listener
  ownership, encryption, IPv6, tracker trust, or port mapping.
- A new native Apple settings UI, remote daemon, socket proxy, or stable public
  remote API promise.
- General performance optimization without measurements attributable to the
  application-view path.

## Escalation Contract

Ordinary naming/module choices, generated-type repairs, test seams, reducer
representation, and same-boundary refactors are authorized. Tightening a
declared bound or fixing an adjacent draft overwrite exposed by the mandatory
stress cases is also in scope.

Stop for direction if implementation evidence requires a stable API-version
compatibility promise despite the explicit unsupported-incubation policy, a
new durable draft or per-resource revision model, a new dependency, a database
migration beyond the post-Tactical-179 baseline, visible physical-device
interaction not already authorized above, or a change to accepted runtime/
product semantics outside settings mutation.

## Stopping Condition And Next Slice

Stop when the typed patch commands replace the old settings mutation shapes,
all current first-party clients and generated boundaries use them, SM-001
through SM-010 pass under complete high-frequency updates and recovery resets,
and the validation matrix is recorded with no settings draft overwrite.

Do not continue into delivery optimization. Record current bytes, allocation,
reducer-notification, reset, and convergence observations. If they show a
material cost, create a separate tactical choosing among structural sharing,
incremental snapshot materialization, projection-specific field masks, or
stable/volatile projection separation while retaining complete reset
snapshots and the transport-shape-independent client tests.
