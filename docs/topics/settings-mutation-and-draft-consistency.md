# Settings Mutation And Draft Consistency

Topic: `settings-mutation-and-draft-consistency`

Status: Direction accepted 2026-08-27. Tactical
[`180`](../tactical/180-typed-settings-patches-and-draft-convergence.md) is
decision-complete and queued in **Later**; it does not displace the single
authoritative **Now**.

## Purpose And Scope

Settings are declarative properties of an application-owned resource. A
caller should be able to update one property or several related properties in
one request without first reconstructing and resending the whole resource.
The command boundary must preserve type safety, validation, request replay,
optimistic concurrency, atomic persistence, and runtime reconciliation.

Clients also need a shared ownership rule for settings drafts. An
authoritative view can update frequently for reasons unrelated to the field a
person is editing. A complete replacement snapshot, full-row upsert, reconnect
reset, or newly allocated but semantically equal object must not erase a dirty
or submitted value. Correctness cannot depend on the server omitting unchanged
fields or on a client retaining object identity.

This topic owns:

- typed partial mutation of torrent and client settings;
- validation and atomicity for patches containing one or more properties;
- command receipt and authoritative-view convergence semantics;
- dirty, submitted, failed, and conflicting client drafts across the web,
  Tauri, Android, and generated Apple boundary; and
- the boundary between correctness work and later measured view-delivery
  optimization.

[`application-control.md`](application-control.md) continues to own request
identity, replay, revision, and transport-neutral command semantics.
[`application-view-api.md`](application-view-api.md) continues to own
authoritative snapshots, typed patches, cursors, resets, and delivery.
[`client-persistence.md`](client-persistence.md) owns durable storage and
transaction behavior. This topic joins those contracts for editable settings;
it does not make a client draft authoritative or durable application state.

## Motivating Observation

During an active download, the web torrent inspector receives frequent peer
and progress updates. Those updates can produce a fresh complete torrent row
and therefore a fresh transfer-limits object even when neither limit changed.
The transfer-limit form copied every such authoritative object back into
component state. Unchecking a limit briefly changed the local draft, then the
next unrelated update restored the old checked value before the user could
save it. With the active update source stopped, the same edit remained dirty.

The global client-settings form has the same ownership hazard: it synchronizes
a complete local settings value from a live configured/runtime value. The
specific publication cadence differs, but the correctness rule is identical.

This is not evidence that complete view updates are invalid. Complete rows and
fresh reset snapshots are deliberate recovery tools. It is evidence that a
form needs an explicit draft overlay and convergence state instead of treating
every render-time value as permission to reset user-owned state.

## Stable Scenarios

These scenarios define the durable behavior. Tacticals may add narrower cases
but must not weaken them.

- **SM-001 — dirty field survives unrelated authority:** while a torrent or
  client setting is edited, any number of semantically unchanged complete
  values and unrelated changing view fields may arrive. The dirty value and
  dirty indication remain.
- **SM-002 — clean fields follow authority:** an authoritative change to a
  field without a local overlay appears immediately even when another field in
  the same resource is dirty.
- **SM-003 — one or many typed properties:** a request may supply exactly one
  property or several properties. Omitted properties remain unchanged. The
  server validates the merged result and commits all supplied properties or
  none.
- **SM-004 — independent transfer directions:** torrent upload and download
  limits may be changed independently. When both are supplied in one patch,
  they share one transaction and one resulting durable revision; they are not
  an intrinsically atomic pair in the model.
- **SM-005 — replay, no-op, and stale revision:** an exact request replay
  returns the durable receipt; a semantically unchanged patch succeeds without
  advancing the durable revision; a stale expected revision changes nothing
  and retains the client draft for review or retry.
- **SM-006 — receipt precedes projection:** after command acceptance, submitted
  fields remain visibly pending until an authoritative view at or beyond the
  accepted durable revision confirms the submitted values. An older or
  unrelated view cannot roll them back.
- **SM-007 — reset and reconnect:** a fresh snapshot or view-set replacement
  uses the same value-and-revision convergence rules. It may refresh clean
  fields but cannot silently erase dirty or pending fields for the same
  resource.
- **SM-008 — failure and conflict:** validation, persistence, transport, stale
  revision, a same-field authority change after editing began, or later
  authoritative disagreement leaves the user's values recoverable and exposes
  a bounded error or conflict state. It never reports an unconfirmed value as
  applied.
- **SM-009 — resource identity:** a draft is keyed by the canonical resource
  identity. Changing the selected torrent cannot apply or display the prior
  torrent's draft. If the edited resource is authoritatively removed, the
  editor terminates with an explicit missing-resource result.
- **SM-010 — transport-shape independence:** every scenario passes when the
  producer repeatedly sends complete cloned settings/rows. Sparse field
  delivery and structural sharing may improve cost but are never required for
  correctness.

## Accepted Command Shape

Settings use resource-specific typed patches, not one command per property, a
string/property bag, JSON Patch, or an untyped merge document. The conceptual
contract is:

```rust
struct TorrentSettingsPatch {
    upload_rate_limit: Option<TransferRateLimit>,
    download_rate_limit: Option<TransferRateLimit>,
}

struct ClientSettingsPatch {
    listener: Option<ListenerPolicy>,
    preferred_listen_port: Option<u16>,
    port_mapping: Option<PortMappingPolicy>,
    peer_connection_limit: Option<u32>,
    upload_slots: Option<u16>,
    active_downloads: Option<u16>,
    upload_rate_limit: Option<TransferRateLimit>,
    download_rate_limit: Option<TransferRateLimit>,
    encryption: Option<EncryptionPolicy>,
    ipv6_enabled: Option<bool>,
    tracker_https_server_authentication:
        Option<HttpsServerAuthenticationPolicy>,
}

enum Command {
    UpdateTorrentSettings {
        torrent_id: String,
        patch: TorrentSettingsPatch,
    },
    UpdateClientSettings {
        patch: ClientSettingsPatch,
    },
}
```

The concrete generated names may follow language conventions, but the
following semantics are fixed:

- omission means unchanged and at least one property is required;
- every property has its real contract type and unknown properties are
  rejected;
- a caller may supply any valid subset, including both transfer directions in
  one request;
- the service merges the patch with the current durable resource, validates
  the complete candidate including cross-field invariants, then persists it in
  one transaction;
- any validation or persistence failure changes no supplied property;
- a semantically unchanged candidate is a successful no-op and does not
  advance the durable revision;
- explicit domain values such as `TransferRateLimit::Unlimited` clear policy;
  omission is never overloaded to mean clear. A future nullable property must
  use a typed explicit value or operation rather than ambiguous nested
  `Option`s on the wire; and
- runtime reconcilers receive the resulting durable intent and disturb only
  the owners whose effective configuration changed.

Commands with lifecycle semantics remain commands: Pause, Resume, Force
recheck, removal, Download now, and queue movement do not become settings
properties. Large or behaviorally specialized collection changes such as file
priority ranges also retain their dedicated typed operations. A resource patch
is for bounded declarative properties whose combined validation and commit are
meaningful.

The existing internal `SetTorrentTransferLimits` and whole-value
`SetClientSettings` variants are superseded. Tactical 180 updates all
first-party producers and consumers together and does not retain aliases only
for disposable pre-support incubation compatibility. This is not a stable
public remote wire claim; the first supported contract baseline remains owned
by product/release direction.

## Receipt And Convergence Contract

The request envelope retains its unique request ID and optional expected
durable revision. A success exposes, rather than discards, the correlated
request ID and accepted/resulting durable revision as opaque decimal strings.
The current response may continue to carry its complete service snapshot while
the broader command-response optimization remains deferred. Clients must not
need that snapshot to decide whether a local draft is still pending.

The authoritative view remains the source of resulting application state. A
successful command receipt means the durable mutation was accepted; it does
not prove that every asynchronously reconciled runtime effect is already
applied. The settings editor therefore distinguishes acceptance from view
convergence:

1. Submit the current typed dirty-field patch with the latest authoritative
   durable revision available to that editor.
2. Preserve the submitted values while the request is in flight.
3. On success, remember the accepted revision and enter `awaiting_view`.
4. Converge a submitted field only when a view at or beyond that revision
   contains the submitted semantic value. A successful no-op may converge
   immediately when the current authoritative value already matches at the
   returned revision.
5. If a sufficiently new view disagrees, retain the local value and surface a
   conflict rather than silently choosing either side.

Runtime application state such as `applying`, `applied`, or `degraded` remains
separate. Durable view convergence can complete while the existing runtime
projection still reports asynchronous application or a typed degraded result.

## Client Draft Ownership

Each editable resource owns a value-semantic draft overlay, not a copied
authoritative object whose lifetime is tied to component renders. The reusable
state machine has these observable phases:

- `pristine`: no overlay; displayed values follow authority;
- `dirty`: one or more fields have local values that differ semantically from
  authority;
- `submitting`: one bounded request contains a captured patch;
- `awaiting_view`: the request succeeded and submitted fields wait for a view
  at or beyond its accepted revision;
- `failed`: the request failed and the draft remains available; and
- `conflicted`: a stale revision or sufficiently new disagreeing authority
  requires an explicit retry, reset, or edit.

The implementation may represent phases per field plus a resource-level
request phase; it need not force the whole editor into one enum. It must retain
enough data to distinguish:

- the latest authoritative value and revision;
- the authoritative base value from which each dirty field was edited;
- current local overlays by typed field;
- the exact patch captured for the one in-flight request;
- local edits made after that capture; and
- the accepted revision, bounded error, and conflict facts.

Incoming authority updates clean fields immediately. Dirty fields retain their
overlay. If authority changes a dirty field away from its edit base before
submission, the editor marks a conflict rather than silently rebasing and
overwriting that change. Submitted fields retain their captured value until
receipt/view convergence. If a person edits a submitted field again,
convergence of the old submitted value must not erase the newer edit. Semantic
equality, not object identity, determines dirtiness and convergence.

On the web, the pure draft reducer/hook owns serializable edit facts while the
existing application/controller layer owns requests, cancellation, and adapter
lifecycle. No promise, transport handle, or engine replica is added to the
Zustand store. Android follows the same reducer semantics in its state owner
and Compose observes them; recomposition is not synchronization authority.
Generated Apple bindings carry the same patch command even where a native
settings editor is not yet present.

Drafts are intentionally ephemeral. Closing an editor or navigating to a
different resource may discard an unsaved draft according to that surface's
existing interaction policy, but the draft cannot leak to the next identity.
Persisting drafts, cross-device edit sessions, and automatic conflict merging
are outside this topic's accepted baseline.

## Ownership And Bounds

- The application service owns patch validation, merge, durable transaction,
  receipt replay, revision advancement, and post-commit reconciliation.
- Existing settings/domain reconcilers own effective runtime application. A
  patch introduces no new background task or cancellation tree.
- Each client application controller owns command execution and transport
  cancellation. Each mounted settings resource owns at most one in-flight
  settings request and one bounded overlay over the statically declared patch
  fields.
- Patch size is bounded by the closed generated schema and the existing
  command-envelope limit. There is no caller-controlled property map or
  unbounded mutation history.
- Errors reuse the bounded typed response surface. The UI may retain one
  current failure/conflict per editor; production diagnostics do not accumulate
  an unbounded draft log.

## Delivery Efficiency Is A Follow-Up

The view system already suppresses a collection update when the complete row
is unchanged. When progress, rates, or peer activity change a torrent row, the
current typed upsert sends the complete row, including unchanged settings.
Adapters may then reconstruct broader snapshots and lose useful structural
sharing. Existing measurement Tactical
[`057`](../tactical/057-hardware-performance-baselines.md) characterizes
the broader view path. This behavior explains avoidable allocation and
delivery cost, but it does not excuse a client draft reset.

Tactical 180 deliberately keeps complete rows, full reset snapshots, existing
cadences, and the current response snapshot. After correctness and convergence
evidence land, a separate measured tactical may compare:

- preserving nested references for semantically unchanged values in reducers
  and adapters;
- incrementally materializing inspection projections instead of rebuilding a
  complete snapshot for every batch;
- projection-specific field masks or typed partial row upserts;
- separating low-churn configured settings from high-churn activity rows; and
- command-specific results that avoid returning a complete service snapshot.

That tactical must measure encoded bytes, allocations, reducer notifications,
snapshot/reset cost, convergence latency, and representative render work
before selecting a wire change. It must preserve coherent fresh snapshots,
cursor/epoch recovery, coalescing, and SM-010. Binary encoding and delivery
profile policy remain separately owned concerns.

## Recommended Direction

Execute Tactical 180 after the current release/state queue permits it. Treat
the complete-update stress cases as primary acceptance tests. Once the API and
all current editors use typed patches and explicit convergence, characterize
the remaining high-frequency delivery cost and create one bounded optimization
tactical only if the measurements justify it.
