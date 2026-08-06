# Code Organization And Refactoring

Status: Living guidance and repository snapshot, refreshed on 2026-08-05
after completed Tacticals
[`079`](../tactical/079-engine-driver-source-shape.md) and
[`080`](../tactical/080-session-view-subsystem-boundaries.md), and after the
feature-driven settings seam completed by Tactical
[`084`](../tactical/084-persisted-client-connection-and-seeding-settings.md)
and selection-action seam completed by Tactical
[`085`](../tactical/085-unified-contextual-selection-actions.md), with the
concrete per-torrent lifetime seam now completed by Tactical
[`086`](../tactical/086-long-lived-torrent-peer-runtime.md), and the session
listen-socket/UDP waist completed by Tactical
[`089`](../tactical/089-coordinated-session-listen-sockets.md), and the next
feature-driven lifetime seam completed by Tactical
[`092`](../tactical/092-truthful-tracker-and-dht-peer-advertisement.md), with
the transport-specific HTTP runtime boundary completed by Tactical
[`095`](../tactical/095-bounded-http-https-tracker-transport.md). On
2026-08-06, planned Tactical
[`097`](../tactical/097-live-client-settings-and-replaceable-session-generations.md)
selected the concrete session-network lifetime seam required for live client
settings.

Topic: `code-organization-and-refactoring`

## Scope

This topic owns the continuing view of source shape: module cohesion, crate
boundaries, facade size, test placement, recurring convergence points, and
the most likely refactoring opportunities. It preserves the architectural
review that should outlive any one implementation tactical.

This is not a second feature queue and does not authorize every candidate.
[`capability-readiness`](capability-readiness.md) continues to own product and
engine priority. A material refactor gets a bounded tactical when selected;
small same-boundary cleanup may land in the tactical that exposes it.
Completed tacticals remain the execution record and are linked rather than
copied here.

## Current Direction

The workspace crate boundaries remain appropriate:

- `rstorrent-engine` depends inward on `rstorrent-protocol`;
- `rstorrent-session` owns application state and depends on the engine and
  protocol crates;
- the gateway, desktop shell, and Android adapter depend on the session
  boundary, with `rstorrent-platform` supplying narrow operating-system
  capabilities; and
- product adapters do not push sockets, storage execution, or payload hot
  paths back out of the engine.

No new `core`, `domain`, `storage`, `views`, or generic service crate is
currently justified. The engine and session `lib.rs` files are healthy
facades: they declare private modules and deliberately re-export public
contracts. The remaining pressure is mostly inside subsystems, not in the
workspace graph.

Tactical `079` completed the first deliberate source-shape refactor. The
download driver now retains top-level orchestration while private child
modules own download control and the bounded storage/checkpoint pipeline, and
its large tests are grouped under private child modules. The facade is still
large, but size alone is not evidence that the extraction should continue.

Tactical `075` subsequently added durable and ephemeral connection policies
through the existing `SessionStore` and speed-history owners without adding a
generic persistence layer. That result supports keeping concrete owners while
extracting only demonstrated internal seams.

Tactical `080` completed the session-view refactor. A 45-line private facade
now preserves every crate-root contract while child modules separately own
portable values, projection mapping, deterministic diffs and ranges, the
legacy accumulator, the leased accumulator, and hub coordination. The prior
two-way concrete dependency is gone: only `hub.rs` knows `HubState`, and
neither delivery accumulator imports or extends `ViewHub`. Do not continue
splitting the 1,706-line hub merely because it remains the largest child; it
now has one coherent coordinator/registry owner and focused lower seams.

Tacticals `078` and `082` implemented the feature-driven incoming and upload
owners without enlarging the download driver. Tactical
[`084`](../tactical/084-persisted-client-connection-and-seeding-settings.md)
completes the next concrete seam: one private session settings facade with
portable contract, borrowed-transaction persistence helpers, deterministic
runtime conversion, and focused tests. It preserves `ApplicationService` and
`SessionStore` ownership, moves the existing storage-settings DTO family
without public contract drift, and does not introduce a generic configuration
hub, repository layer, or new crate.

Tactical `085` resolves a demonstrated web convergence point without creating
a generic command framework. Pure torrent/file action policy and shared menu
renderers are separate from an application-lifetime torrent runner/removal
owner; toolbar and row-context presentations consume them. `VirtualTable`
retains only generic invocation, focus, selection, and virtualization
mechanics, while the existing overlay layer retains positioning and menu
semantics.

Tactical [`086`](../tactical/086-long-lived-torrent-peer-runtime.md) has landed
the selected feature-driven seam through all five gates. A task-free
engine torrent-peer state now retains the ordinary `PeerRegistry`,
`PeerRuntime`, checked connection IDs, and publication state. A private
session `TorrentRuntime` owns that handle, active-operation membership, and a
generation-fenced seed-registration slot across download completion and later
seeding. Routed incoming tasks attach to that state after handshake routing,
and the existing Peers/Swarm mapper consumes immutable observations rather
than reaching into their socket or scheduler owners. `ApplicationService`
retains catalog, global service, and single-active-torrent policy. This is
concrete ownership and deterministic-test pressure, not a file-size extraction
or an umbrella session rewrite.

Tactical [`088`](../tactical/088-upnp-mapped-external-tcp-seeding.md) follows
the same feature-driven split. Focused `rstorrent-engine::port_mapping::upnp`
owns bounded SSDP/HTTP/XML/SOAP protocol and gateway runtime without importing
session views or persistence. Private session `reachability` owns eligibility,
generation fencing, one task and mapping, renewal, status publication, and
joined cleanup. `ApplicationService` retains only construction and shutdown
order, while the existing view hub remains a task-free projection. The
physical gate did not expose a reason for a new crate or an umbrella session
coordinator refactor.

Tactical [`089`](../tactical/089-coordinated-session-listen-sockets.md) adds
two focused private engine modules without changing the crate graph.
`session_socket` owns task-free TCP/UDP candidate policy and bound-socket
handoff; `session_udp` owns the one receive task, bounded dispatch, and shared
send side. Incoming and DHT runtimes consume supplied transports, while
`ApplicationService` retains generation composition and ordered rollback and
shutdown. This is the concrete lifecycle split selected over an umbrella
session coordinator or new crate.

Tactical
[`092`](../tactical/092-truthful-tracker-and-dht-peer-advertisement.md)
completes the next concrete seam exposed by that work. One cohesive engine
`advertisement` module owns the session command table, pure tracker schedules,
bounded tracker operations, and DHT operation admission. The application
composes it once and long-lived `TorrentRuntime` generations supply small
registrations, counters, and peer-registry destinations. Application download
drivers disable their nested tracker/DHT discovery paths, so completed seeding
does not lose discovery or create a second product scheduler.

The retained direct-engine tracker manager and DHT retry helper still support
standalone engine APIs and their focused tests. They do not execute inside the
application service. Removing that compatibility path would change the public
engine entry points without improving the selected product lifetime, so it was
not folded into this refactor. No new crate was justified: protocol values and
deterministic tracker/DHT transitions remain inward, while the one session
task, Tokio handles, and cancellation stay at the engine runtime boundary.

Tactical
[`095`](../tactical/095-bounded-http-https-tracker-transport.md) extends that
boundary without turning it into a tracker framework. Pure magnet/catalog URL
authority validation stays in `rstorrent-protocol` without a `url`, reqwest,
Tokio, DNS, or socket dependency. The existing engine `tracker` module owns
transport-neutral schedules and outcomes; a cohesive private `http_tracker`
module owns reqwest clients, DNS, HTTP/TLS, redirects, gzip, request encoding,
and hostile response parsing. `advertisement` uses explicit endpoint-enum
dispatch under its existing task and operation budget. UDP token caching and
HTTP tracker-ID continuation remain transport-specific state.

The architecture dependency test caught and rejected an intermediate `url`
dependency from protocol before closure. The landed pure bounded parser keeps
dependency direction inward while reqwest and async-compression remain at the
engine runtime boundary. The content driver gained only a cancellation-owned
external-discovery lifetime fence needed to keep session observations live
through content startup. No new crate, generic transport trait, separate HTTP
manager, application socket owner, or product-control task was justified.

## Source-Organization Guidance

These are review prompts, not mechanical rules:

- A module primarily owns state, invariants, or a lifecycle. It is not a
  bucket for similarly named types.
- `lib.rs` and subsystem facade files normally explain a boundary, declare
  modules, and expose a deliberate API. Substantial implementation may remain
  when the subsystem is genuinely small and cohesive.
- Around 1,000 non-test lines prompts a cohesion review. Around 2,000 is a
  strong prompt to record why the owner remains together or to extract a
  demonstrated seam. Neither number is a CI limit or automatic split.
- Independent churn, unrelated mutable owners, bidirectional implementation
  knowledge, several fixture families, and behavior that cannot be tested
  without unrelated infrastructure are stronger evidence than line count.
- Prefer a private child module before a new crate. A crate must create a
  useful acyclic dependency, reuse, platform, security, feature-isolation,
  lifecycle, or testing boundary.
- Preserve public paths and behavior during a structural extraction. Use
  private or crate-local visibility rather than widening an API for moved
  tests.
- Keep compact value and transition tests beside a cohesive owner. Move a
  large private test body into categorized child modules when it has several
  fixture families. Use crate-level `tests/` for behavior expressible through
  the public API.
- Generated contracts, catalogs, fixtures, and naturally tabular declarations
  are judged by their owning workflow rather than ordinary hand-authored line
  prompts.

A refactor should name the concrete improvement: one owner becomes visible,
a dependency points inward, a task acquires an exact shutdown path, pure logic
becomes independently testable, duplicated policy gains one authority, or a
public facade becomes deliberate. Shorter files alone are not the outcome.

## Snapshot Method

The following snapshot uses the tree at `beb8d2f` on 2026-08-04. Production
and test counts are approximate physical lines. For Rust files with one
trailing `#[cfg(test)]` module, the marker separates the two; child test files
are counted separately. Touches are path appearances across the most recent
200 commits and are only convergence evidence. Moves and mechanical
extractions inflate churn, especially for the driver.

| Boundary | Approximate production lines | Approximate test lines | Touches in 200 commits | Current assessment |
| --- | ---: | ---: | ---: | --- |
| Engine driver facade plus `control` and `storage_pipeline` | 4,995 + 3,923 | 7,766 child tests | 78 on the facade | Recently improved. Monitor orchestration pressure; do not immediately continue splitting it. |
| `SelectiveStorage` | 3,616 | 1,860 | 23 | Strongest engine-side structural candidate; one valid owner contains several separable planning and platform concerns. |
| `SwarmState` | about 2,550 | about 1,109 | 17 | Large but still one deterministic transition owner. Extract only independently changing policy or bookkeeping. |
| Session view subsystem | 6,019 across facade and seven child owners | 1,965 across six child files | Structural move completed | Recently improved by Tactical `080`: one-way dependencies, one hub owner, two independent accumulators, deliberate facade. Monitor feature pressure; do not continue splitting by size. |
| `ApplicationService` | 3,225 | 2,728 | 50 | Legitimate service owner with several subordinate sinks, cleanup paths, and projections. Feature-driven extraction is preferable. |
| `SessionStore` | 3,330 | 1,669 | 21 | Legitimate SQLite owner combining connection policy, schema, migrations, domain reads, and mutations. |
| Gateway `lib.rs` | 944 | 1,473 | 18 | Production boundary is still manageable; inline integration fixtures dominate physical size. |
| Web `LiveApplication` | 1,391 | 892 in its test file | 21 | Connection orchestration and pure contract-to-product mapping are beginning to diverge. |
| Web semantic validation | 1,701 | 624 in its test file | 22 | Several contract domains share one hand-authored validation module. |
| Web `VirtualTable` | 1,319 | 613 in its test file | 13 | React rendering, persisted configuration, sorting, selection, resizing, virtualization, and generic context invocation meet in one component. Action policy remains outside it. |

The snapshot is a map, not a ranked size queue. The completed session-view
move is retained as a recent reference point, while `SwarmState` remains
locally testable and owns one tightly coupled deterministic state machine
despite its size.

## Recently Resolved Boundary

### Session View And View-Set Boundaries

Tactical [`080`](../tactical/080-session-view-subsystem-boundaries.md) is
complete. It resolved the concrete bidirectional implementation knowledge,
not just the original file sizes.

Before the tactical, `views.rs` and `view_sets.rs` imported each other's
private concrete state and the leased owner implemented methods on a sibling
`ViewHub`. Afterward, the 45-line facade preserves public paths, `hub.rs` owns
all coordination, and the lower delivery modules receive snapshots and
patches without reading hub maps.

The retained subsystem now separates:

- serializable view contract values;
- hub and projection models;
- individual subscription queue ownership;
- leased view-set delivery;
- snapshot/patch and collection-diff construction; and
- pure range algorithms.

The 33 focused cases and 119 session tests remain, generated artifacts are
byte-identical, Android consumers compile for both established targets, and
both controlled gateway delivery paths pass. Revisit this boundary only when
a new projection, delivery policy, or measured coordinator problem reveals a
specific seam.

## Most Likely Focused Opportunities

### 1. Selective Storage Internals

`SelectiveStorage` correctly remains the authority for selection routes,
part-file state, verified state, and publication transitions. Its file also
contains backing acquisition, immutable write and hash plans, descriptor and
platform validation, publication filesystem operations, materialization, and
path derivation. Those concerns already have distinct types and failure
fixtures, making private child modules plausible without replacing the owner.

The likely seams remain:

- storage backing and lease acquisition;
- immutable write/hash I/O plans and their execution;
- descriptor/platform preparation and validation;
- path publication and namespace durability; and
- artifact/path derivation helpers.

Sequence this with Tactical `078`, not against it. The seeding tactical should
create a conservative immutable `seed_content` read plan rather than making
`SelectiveStorage` a long-lived upload service. After that boundary lands,
refresh this snapshot and decide whether the remaining write-side separation
still merits its own tactical. A pre-`078` refactor is justified only if it is
needed to expose that read contract safely.

### 2. Application Service And Session Store Internals

Keep `ApplicationService` as the lifecycle owner and `SessionStore` as the
transaction and SQLite-connection owner. Do not replace them with service,
repository, or persistence trait hierarchies.

Likely application seams are managed-artifact cleanup, checkpoint and
activity sinks, durable view-state construction, and feature-specific
lifecycle reconciliation such as incoming seeding. Likely store seams are
connection policy, schema/migrations, row decoding, and domain mutation
families expressed as private functions over the one owned connection.

Tactical `086` selects the incoming-seeding seam narrowly: active-download
membership, seed registration, and the shared torrent peer handle become one
private per-torrent lifetime owner while `ApplicationService` retains global
admission and session services. It does not move persistence, DHT, tracker,
storage, settings, or view-set ownership merely because they also meet in the
application root.

These should normally be extracted by the feature that changes them. Incoming
seeding will test application lifecycle placement; future `.torrent` source
retention or schema work will test the store boundary. If independent churn
continues after those features, use one focused tactical per owner rather than
combining application and persistence into an umbrella rewrite.

### 3. Web Contract Mapping And Reusable Algorithms

The web opportunities are related by product churn but should not be bundled
automatically:

- move pure `ViewSnapshot`/patch-to-product mapping out of `LiveApplication`
  so transport and reconnection ownership can be tested separately;
- divide `validation.ts` by semantic contract domain while retaining one
  public validation facade and exact hostile-input limits; and
- extract VirtualTable's pure persisted-configuration, sorting, and selection
  algorithms while leaving React focus, measurement, and rendering ownership
  in the component.

Tactical [`077`](../tactical/077-shared-overlay-menu-system.md) owns the shared
overlay concern separately, while Tactical
[`085`](../tactical/085-unified-contextual-selection-actions.md) now owns the
selection-action policy and runner seam. Neither is included in this refactor
list. Choose among the remaining seams based on the next web feature's
pressure; do not adopt a data-grid or validation framework merely to reduce
file length.

### 4. Android Product Graduation

The Android application still lives under
`experiments/android-engine-bootstrap` and retains both the current
`ProductEngineService` application boundary and the older diagnostic
`EngineService`. Graduation is a product/repository boundary, not a response
to Kotlin file size. It needs its own tactical once the bootstrap is accepted
as the durable Android product location or a replacement location is chosen.

That tactical should preserve Compose, SAF, foreground-service, and generated
Rust/Kotlin contracts while removing or isolating the legacy diagnostic path.
It should not be mixed into an engine-only refactor or Tactical `078`, whose
first seeding slice deliberately excludes Android product work.

## Watch List And Deliberate Non-Work

- **Driver facade:** Tactical `079` established meaningful owners. Let future
  metadata, discovery, content-supervision, or publication work demonstrate a
  new seam before extracting more.
- **Swarm state:** its scheduling, request generations, piece bookkeeping, and
  storage completion currently form one deterministic invariant set. Consider
  extraction only when one policy changes independently or tests require
  unrelated setup.
- **Gateway:** moving the large private integration test body into categorized
  child tests would improve navigation, but the production facade alone does
  not justify a broad gateway redesign.
- **Crate graph:** no candidate currently earns a new crate. Private module
  extraction should be the default.
- **Entry-point documentation:** tactical checkpoints should link the
  authoritative queue instead of copying it. At this refresh,
  `capability-readiness` still names completed Tactical `075` as **Now**;
  reconcile that when the maintainer selects the next executable capability
  rather than inventing a queue in this topic.

## Near-Term Recommendation

There should not be one repository-wide umbrella refactor tactical. Tacticals
`084`, `086`, `088`, and `089` confirm that focused child modules work:
settings, per-torrent peer lifetime, reachability, coordinated bind policy,
and UDP receive ownership became independently testable while the store and
application retained their real owners.

Planned Tactical
[`097`](../tactical/097-live-client-settings-and-replaceable-session-generations.md)
now exposes concrete pressure one level above those children. The five
existing settings cannot apply live while TCP acceptance, UDP/DHT transport,
reachability, discovery, admission, upload scheduling, and shutdown are
assembled as immutable application-generation siblings. The selected private
`SessionNetworkRuntime` is therefore a cohesive feature-driven lifetime owner
with replaceable transport generations; it does not absorb persistence,
torrent catalogs, storage, views, or product adapters and does not justify a
new crate or generic service framework.

Selective storage remains the leading engine-only refactor candidate when a
feature next changes its large coordinator; size alone does not authorize the
work.

## Maintenance Contract

Refresh this topic:

- after a tactical materially changes a module, task, crate, public facade, or
  test-placement boundary;
- before choosing a standalone refactor tactical;
- when a listed candidate is resolved, rejected, or superseded by a feature
  owner; and
- periodically at a campaign transition or after roughly 25 to 50 substantive
  commits, rather than on every source edit.

Each refresh should record the date and baseline commit, rerun approximate
size and recent-touch evidence, inspect actual imports and owner/task
boundaries, reorder the candidates, and link any new tactical. Do not paste
implementation logs here. Do not turn approximate counts into CI gates. A
candidate stays listed only while a concrete ownership, dependency, testing,
lifecycle, or navigation problem remains.

## History

- **2026-08-06:** Planned Tactical `097` after live client settings exposed a
  concrete session lifetime mismatch. The selected private session-network
  owner keeps incoming peers, DHT, discovery, admission, scheduling, and
  accounting stable around replaceable TCP/UDP/reachability generations; the
  store, application, torrent runtimes, crate graph, and public engine
  architecture retain their existing responsibilities.
- **2026-08-05:** Completed Tactical `095`. A private engine HTTP tracker
  owner now meets the existing transport-neutral schedule through explicit
  enum dispatch; protocol retains pure catalog parsing, the long-lived
  advertisement task retains all concurrency and lifecycle authority, and the
  application adds only projection plus an external-discovery lifetime fence.
  The architecture gate removed an accidental outward `url` dependency before
  closure, and no new crate or tracker trait hierarchy was needed.
- **2026-08-05:** Completed Tactical `089`. Task-free coordinated bind policy
  and one bounded session UDP receive owner now sit below incoming and DHT
  runtimes; application generation composition remains explicit. The feature
  resolved the shared-socket pressure without a new crate, generic transport
  trait, or umbrella session coordinator.
- **2026-08-05:** Completed Tactical `088`. The feature established a focused
  engine UPnP boundary and private session reachability coordinator with one-
  way dependencies, explicit cancellation/join, task-free projection, and
  exact terminal ownership; no new crate or broader application refactor was
  required.
- **2026-08-05:** Planned Tactical `086` after incoming projection exposed a
  real lifetime mismatch: download-scoped ordinary peer state terminates while
  registration-owned completed seeding continues. The selected feature-driven
  seam is a private session per-torrent runtime plus a task-free engine peer
  owner, not a new crate or umbrella session rewrite.
- **2026-08-05:** Completed Tactical `085`. Pure action policy, shared
  renderers, and an application-lifetime sequential runner now sit outside
  `VirtualTable`; the table retains generic exact-target context invocation.
  The added mechanics increase its recorded size and touch count, so its pure
  configuration/sorting/selection extraction remains a concrete candidate,
  but no data-grid or command framework is justified.
- **2026-08-04:** Completed Tactical `080`. The session-view facade now points
  inward to explicit contract, model, diff, range, hub, subscription, and
  view-set owners; 33 focused tests are categorized, all generated bytes are
  stable, and the full Rust, web, Android, and controlled-loopback matrix is
  green. Selective storage becomes the leading standalone candidate, best
  reassessed after Tactical `078` establishes immutable seeding reads.
- **2026-08-04:** Selected the session view/view-set boundary and created
  Tactical `080` with byte-stable generated-contract, one-way dependency,
  owner/task/lock, test-layout, and adapter-regression gates. No implementation
  started.
- **2026-08-04:** Created the living topic from the repository-wide review
  recorded in Tactical `079`, refreshed after its completed extraction and
  Tactical `075`, and ranked session views ahead of selective storage for a
  standalone refactor because the view subsystem has concrete bidirectional
  implementation knowledge. Tactical `078` remains the preferred path when
  feature implementation, rather than dedicated refactoring, is next.
