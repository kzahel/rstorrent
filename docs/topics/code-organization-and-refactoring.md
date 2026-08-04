# Code Organization And Refactoring

Status: Living guidance and repository snapshot, refreshed on 2026-08-04
after Tacticals [`079`](../tactical/079-engine-driver-source-shape.md) and
[`075`](../tactical/075-ephemeral-application-state.md).

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

Planned Tactical [`078`](../tactical/078-local-single-peer-tcp-seeding.md)
already specifies the next feature-driven module boundaries: direction-neutral
peer I/O, incoming admission, runtime-independent upload state, immutable seed
content, and session-owned seeding eligibility. Its implementation should add
those owners rather than enlarge `driver.rs`, `selective_storage.rs`, or
`application.rs`.

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

The following snapshot uses the tree at `db4a092` on 2026-08-04. Production
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
| Session `views` plus `view_sets` | 4,468 + 1,400 | 1,145 + 809 | 30 + 17 | Strongest standalone refactor candidate because the two modules know each other's private implementation. |
| `ApplicationService` | 3,225 | 2,728 | 50 | Legitimate service owner with several subordinate sinks, cleanup paths, and projections. Feature-driven extraction is preferable. |
| `SessionStore` | 3,330 | 1,669 | 21 | Legitimate SQLite owner combining connection policy, schema, migrations, domain reads, and mutations. |
| Gateway `lib.rs` | 944 | 1,473 | 18 | Production boundary is still manageable; inline integration fixtures dominate physical size. |
| Web `LiveApplication` | 1,391 | 892 in its test file | 21 | Connection orchestration and pure contract-to-product mapping are beginning to diverge. |
| Web semantic validation | 1,701 | 624 in its test file | 22 | Several contract domains share one hand-authored validation module. |
| Web `VirtualTable` | 1,250 | 598 in its test file | 10 | React rendering, persisted configuration, sorting, selection, resizing, and virtualization meet in one component. |

The snapshot is a map, not a ranked size queue. For example, the session view
subsystem ranks above several larger files because it has an observable
dependency-direction problem, while `SwarmState` remains locally testable and
owns one tightly coupled deterministic state machine.

## Most Likely Focused Opportunities

### 1. Session View And View-Set Boundaries

This is the best current standalone refactor.

`views.rs` contains public contract values, projection models, hub state,
subscription queues, activity mapping, snapshot and patch construction, and
range-diff algorithms. `view_sets.rs` contains the public leased view-set
contract and delivery owner, but imports private `HubState` and patch helpers;
in the other direction, `views.rs` stores `ViewSetInner`, imports
`ViewSetUpdate`, and reads view-set constants. `view_sets.rs` also implements
methods directly on `ViewHub`. This is concrete two-way implementation
knowledge, not merely a large file.

A focused tactical should preserve generated contracts and public re-exports
while separating approximately:

- serializable view contract values;
- hub and projection models;
- individual subscription queue ownership;
- leased view-set delivery;
- snapshot/patch and collection-diff construction; and
- pure range algorithms.

The exact filenames should follow the ownership map discovered during the
tactical. The acceptance criterion is one-way internal dependencies and
unchanged contract/evidence, not matching this sketch.

### 2. Selective Storage Internals

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

### 3. Application Service And Session Store Internals

Keep `ApplicationService` as the lifecycle owner and `SessionStore` as the
transaction and SQLite-connection owner. Do not replace them with service,
repository, or persistence trait hierarchies.

Likely application seams are managed-artifact cleanup, checkpoint and
activity sinks, durable view-state construction, and feature-specific
lifecycle reconciliation such as incoming seeding. Likely store seams are
connection policy, schema/migrations, row decoding, and domain mutation
families expressed as private functions over the one owned connection.

These should normally be extracted by the feature that changes them. Incoming
seeding will test application lifecycle placement; future `.torrent` source
retention or schema work will test the store boundary. If independent churn
continues after those features, use one focused tactical per owner rather than
combining application and persistence into an umbrella rewrite.

### 4. Web Contract Mapping And Reusable Algorithms

The web opportunities are related by product churn but should not be bundled
automatically:

- move pure `ViewSnapshot`/patch-to-product mapping out of `LiveApplication`
  so transport and reconnection ownership can be tested separately;
- divide `validation.ts` by semantic contract domain while retaining one
  public validation facade and exact hostile-input limits; and
- extract VirtualTable's pure persisted-configuration, sorting, and selection
  algorithms while leaving React focus, measurement, and rendering ownership
  in the component.

Tactical [`077`](../tactical/077-shared-overlay-menu-system.md) has removed the
shared overlay concern from this list. Choose among the remaining seams based
on the next web feature's pressure; do not adopt a data-grid or validation
framework merely to reduce file length.

### 5. Android Product Graduation

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

There should not be one umbrella refactor tactical.

- If the next work is a dedicated structural improvement, create a focused
  tactical for the session view/view-set subsystem.
- If the next work is the already planned seeding capability, implement
  Tactical `078` directly; its owner map already includes the bounded
  extractions the feature needs.
- Reassess selective storage immediately after `078` exposes immutable seed
  reads. It remains the leading engine-only refactor candidate.

This ordering keeps refactoring evidence-driven while still preventing known
two-way dependencies and mixed owners from accumulating silently.

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

- **2026-08-04:** Created the living topic from the repository-wide review
  recorded in Tactical `079`, refreshed after its completed extraction and
  Tactical `075`, and ranked session views ahead of selective storage for a
  standalone refactor because the view subsystem has concrete bidirectional
  implementation knowledge. Tactical `078` remains the preferred path when
  feature implementation, rather than dedicated refactoring, is next.
