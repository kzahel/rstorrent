# Code Organization And Refactoring

Status: Living guidance and repository snapshot, refreshed on 2026-08-09 at
source commit `f8f2671` after completed Tacticals
[`079`](../tactical/079-engine-driver-source-shape.md),
[`080`](../tactical/080-session-view-subsystem-boundaries.md), and the
feature-driven ownership work through completed
[`112`](../tactical/112-dual-stack-transport-and-ipv6-dht.md). The crate graph
remains appropriate. Current internal pressure is highest in the application
root and its test topology, the session store, and the role-specific peer
bootstrap paths exposed by MSE; the larger dual-family DHT actor is a concrete
cohesion watch point, while web semantic validation and selective-storage
internals remain lower-timed candidates. No standalone refactor is selected by
this topic.

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
splitting the 1,851-line hub merely because it remains the largest child; it
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

Tactical
[`097`](../tactical/097-live-client-settings-and-replaceable-session-generations.md)
then moved the coupled session-network lifetime out of `ApplicationService`.
The private `SessionNetworkRuntime` owns one latest-value reconciler and the
stable incoming, UDP/DHT, discovery, advertised-endpoint, admission,
scheduling, and accounting state around replaceable transport and
reachability generations. `ApplicationService` still owns persistence,
storage roots, torrent runtimes, commands, and views. This is the right
boundary; the later dual-family growth to 2,192 lines remains under the same
one-task owner and should not trigger a size-driven split.

Completed Tactical
[`098`](../tactical/098-authenticated-https-tracker-platform-trust.md) settled
its known insertion point in that session-network owner and the
existing focused HTTP-tracker runtime. The session owner submits one added
domain through its existing reconciler; the engine advertisement owner
replaces one passive client pair; and Android owns its process bootstrap. No
TLS framework, generic settings callback system, extra task, or new crate was
needed.

Feature work through Tacticals `099`--`110` grew the application and store
around display preferences, source-preserving intake, authentication,
selection-independent checking, and serialized file intent without changing
their durable owners or the crate graph. The additions strengthen the existing
callback/test-topology and schema/migration candidates; they do not justify a
generic application coordinator or persistence layer.

Completed Tactical
[`111`](../tactical/111-mse-peer-stream-encryption.md) retained protocol/runtime
direction while adding one pure `rstorrent-protocol::mse` state-machine
subsystem, one focused engine DH-work owner, and no new long-lived task. Its
role-specific runtime integration nearly doubled the outgoing
`peer_socket.rs` connect/handshake portion and added a substantial pre-
admission plain/MSE bootstrap path to `incoming.rs`. Those are now concrete
private-module seams. They are not evidence for a shared async handshake
framework: initiator downgrade memory and responder torrent lookup, policy
failure, cancellation, and accounting remain deliberately different.

Completed Tactical
[`112`](../tactical/112-dual-stack-transport-and-ipv6-dht.md) exercised the
existing `session_socket`, `session_udp`, `SessionNetworkRuntime`, and single
DHT actor boundaries with a second family. It preserved one UDP owner, one DHT
actor, one command route, one observation owner, and one latest-value session
reconciler while independently replacing, failing, and disabling each family.
The feature therefore rejects the earlier hypothesis that dual stack required
a prerequisite network-owner split. It did make the DHT actor a 3,044-line
production owner with a 1,033-line inline test module, which warrants an
explicit cohesion review if the next DHT behavior introduces independently
changing policy or fixtures; size alone still does not identify that seam.

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

The following snapshot uses the source tree at `f8f2671` on 2026-08-09, 137
commits after the prior `6ffaeff` snapshot and 16 commits after the preceding
`0b25152` refresh. Production and test counts are
approximate physical lines. For Rust files with one trailing `#[cfg(test)]`
module, the marker separates the two; child test files are counted separately.
Files with interspersed test-only helpers are intentionally less exact.
Touches are path appearances across the most recent 200 repository commits and
are only convergence evidence. Moves and mechanical extractions inflate churn,
while a new file can have low touch count despite substantial ownership.

| Boundary | Approximate production lines | Approximate test lines | Touches in 200 commits | Current assessment |
| --- | ---: | ---: | ---: | --- |
| Engine driver facade plus `control` and `storage_pipeline` | about 10,700 across three owners | 8,459 child-test lines | 25 on the facade | Tactical `079` still supplies useful owners. `DownloadControl` has grown around checker and disk/resource observation, while direct-engine discovery remains a conditional facade seam. No umbrella split is justified. |
| `SelectiveStorage` | about 3,816 | about 1,986 | 2 | Shared immutable artifact geometry remains a concrete one-way-dependency seam, but current feature churn is low and no active tactical changes storage shape. |
| `SwarmState` | about 3,331 | about 1,766 | 9 | `piece_picker` owns independently changing activation policy. Retain the remaining deterministic transition owner until another policy separates. |
| Incoming and outgoing peer bootstrap | about 4,466 across `incoming.rs` and `peer_socket.rs` before their test modules | about 1,986 | 22 incoming, 8 outgoing | Strong new engine source-shape candidate after MSE. Pre-stream handshake/policy/accounting can become role-specific private children while listener/admission/upload and peer-set/task owners remain in place. Do not unify the two roles behind a generic async runner. |
| DHT actor | about 3,044 | about 1,033 | 6 | Tactical `112` retained one actor around two independently bounded family nodes, one command route, and one observation owner. The growth is material, but no duplicated lifecycle or cross-family state leak appeared. Review again when a DHT policy changes independently; do not split by family or file size alone. |
| Session view subsystem | 7,352 across the facade and eight child owners | 2,473 across six child files | 9 on `hub.rs` | The Tactical `080` shape remains healthy. New ETA behavior landed in its own pure child without restoring bidirectional knowledge. |
| `ApplicationService` | about 4,583 | about 5,819 inline | 50 | Strongest general convergence and navigation pressure. The lifecycle owner remains legitimate, but callback adapters and unrelated fixture families have concrete private seams. |
| `SessionNetworkRuntime` | 2,192 | application-level fixtures live in `application.rs` | 13 | Tactical `112` reused one reconciler for independent family sockets, DHT, endpoint state, and cancellation without a second task or channel. It remains cohesive; Tactical `113` adds pinhole state to the separate reachability coordinator rather than motivating a transport split. |
| `SessionStore` | about 5,485 | about 3,192 | 24 | Strong persistence candidate. Schema 16, complete DDL, historical data migrations, settings migrations, and independently changing mutations/decoders remain under one connection owner. |
| Gateway HTTP, application WebSocket, and first-run web authentication | about 4,306 across four owners | about 2,572 | 6 on `lib.rs` | The transport split remains useful, and authentication landed in focused policy/HTTP modules. Large integration tests have navigation pressure, but the recent owner shape should settle. |
| Web `LiveApplication` | 1,610 | 1,171 in its test file | 13 | Connection/view intent and pure mapping/transition behavior remain separable; no reconnection or store rewrite is implied. |
| Web semantic validation | 2,276 | 888 in its test file | 13 | Strongest web-only candidate. Connection frames, settings, DHT, torrent/file/tracker/peer/swarm views, diagnostics, pieces, and disk semantics still share one hand-authored module. |
| Web `VirtualTable` | 1,332 | 613 in its test file | 3 | Action policy is outside it; pure sorting/configuration/range selection remain extractable, but current churn is low. |
| Android diagnostic plus product services | 1,756 across two Kotlin services | focused product reducer tests elsewhere | 1 legacy, 3 product | The 794-line legacy and 962-line product paths remain simultaneously packaged. Resolving them is product graduation, not file-size cleanup. |

The snapshot is a map, not a ranked size queue. The application and store
remain strongest because they combine sustained growth with distinct helper
and fixture families. MSE creates a new peer-bootstrap candidate from a
specific before/after responsibility split, not from line count alone.
`SelectiveStorage` remains concrete but moves down on timing because only two
of the last 200 commits touched it. The session-network, view, gateway-auth,
and DHT owners have now passed their dual-family feature test and should remain
intact unless later work exposes an independently changing policy, dependency,
or lifecycle defect.

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

These are independently selectable stories, ordered by current evidence
rather than by a promise to execute them in sequence.

### 1. Application Callback Adapters And Test Topology

Keep `ApplicationService` as the owner of the store, storage roots, torrent
catalog and runtimes, command effects, view hub, and ordered application
shutdown. Tactical `097` has already removed session-network composition; an
umbrella application coordinator or command framework would obscure the
remaining real owner.

The concrete subordinate boundary is the engine-to-session callback bridge.
`StoreCheckpointSink` and `ViewActivitySink` occupy about 906 production
lines in `application.rs`. They translate checkpoint, metadata, piece,
storage, tracker, peer, and diagnostic events into the existing store and
view owners; they do not own the application lifecycle. Move them behind one
or two private child modules while preserving their existing concrete sink
interfaces and error behavior. Durable view-state construction is a related
pure mapping seam but should move only if it produces a one-way dependency,
not merely to increase the extracted line count.

The approximately 5,673-line inline test module is independent evidence for
the same boundary. It now contains distinct HTTP/HTTPS tracker fixtures,
session-network/settings generations, DHT lifetime, command/view delivery,
recheck and resume, selection-independent checking, file-priority lifecycle,
managed removal, publication, and incoming-seeding families. A first gate can
move those unchanged into `application/tests/` child modules with shared
private support, preserving names and visibility. Session-network integration
cases belong in a named application-test child because they deliberately
exercise `ApplicationService`; they should not be rewritten as white-box tests
of the network owner.

This is the strongest current standalone source-shape story. It must not move
store, torrent-runtime, network, or view ownership into the callback modules,
and it must not combine removal, platform-storage, and command dispatch into
one broad rewrite.

### 2. Session Store Schema And Domain Internals

Keep `SessionStore` as the sole owner of its SQLite connection and the
transactions on that connection. The private settings-persistence boundary
from Tactical `084` demonstrates the appropriate pattern: focused functions
borrow a connection or transaction, while the store retains commit ordering,
revisions, receipts, resource-limit translation, and its public facade.

The first coherent extraction is schema creation and migration. `store.rs`
now carries schema version 16, the complete initial DDL, multiple table
families, and historical data/settings migrations through v15 before its
storage-root readers and command mutations. Move schema constants, migration-
only observations, and migration functions to a private child with exact
version, transaction, rollback, corruption, filesystem-fact, and ephemeral-
profile tests. Do not introduce a migration framework or make migrations own
the connection. Tactical `112` has now consumed schema 16 and planned Tactical
`114` adds queue facts, making this the most strategically timed standalone
persistence extraction if the authoritative feature queue is explicitly
paused for structural work.

Later store stories should remain separate unless a feature proves they
change together:

- source/intake and torrent command mutations;
- resume, have, checkpoint, publication, and repair mutations;
- storage-root and removal mutations; and
- snapshot, resume, tracker, selection, and removal row decoding.

Those families may become private functions over `&Connection` or
`&Transaction`; they do not justify repositories, async persistence traits,
per-table objects, or a second database authority. The schema/migration slice
is the most bounded starting point.

### 3. Peer Stream Bootstrap Boundaries

Tactical `111` made two role-specific seams concrete. In `peer_socket.rs`,
outgoing TCP connection, plain/MSE negotiation, bounded downgrade memory,
handshake accounting, and raw pre-framing IO precede the independent peer-task
and `PeerSocketSet` owner. In `incoming.rs`, plain-versus-MSE classification,
responder action execution, provisional torrent lookup, and handshake response
precede duplicate admission and the independent metadata/upload peer loop.

A bounded source-shape tactical may move those pre-stream paths into private
role-specific children while preserving the current `PeerConnection` and
`ReceivedIncomingHandshake` results, error classes, byte accounting, DH owner,
deadlines, cancellation, and terminal observations. The incoming child should
receive a narrow concrete context for policy, lookup, sinks, and DH work rather
than import the whole listener/upload `Shared` owner.

Do not unify initiator and responder behind a generic async trait or action
runner merely because both drive the same sans-IO protocol enum. Their
downgrade, lookup, cancellation, and failure contracts differ materially. Do
not select this extraction solely to preserve Tactical `111` file sizes.
Tactical `112` exercised plaintext and MSE IPv6 cancellation at the existing
role boundaries without impeding family ownership or focused tests, so the
candidate remains a navigation/cohesion story rather than a correctness
prerequisite.

### 4. Web Semantic Boundaries

The web candidates are independent and should not be bundled automatically.
The strongest is `validation.ts`: 2,276 lines and 13 recent touches now place
API connection frames, settings, DHT, torrent/file/tracker/peer/swarm views,
diagnostics, pieces, and disk pipelines behind one hand-authored semantic
validator. Split those domains into private modules behind the same public
decode facade and common bounded primitives. Generated JSON Schema remains
the structural gate; the extraction must preserve every additional semantic
bound and hostile-input test byte-for-byte in meaning. No validation
framework change is implied.

`LiveApplication` has a second clear seam: its class owns client connection,
commands, desired views, and lifecycle through the first 592 lines, while the
rest of the now 1,610-line file maps and transitions generated view values into
product models. Move that pure mapping layer only when the next projection
changes it or when a standalone web refactor is selected; do not change store
or reconnection semantics at the same time.

`VirtualTable` is lower priority. Tactical `085` has removed feature action
policy, leaving a bounded opportunity to extract pure sorting, persisted
configuration, and range-selection algorithms while React retains focus,
measurement, resize, virtualization, and rendering. Only three of the last 200
commits touched it; do not adopt a data grid or generic state framework merely
to reduce the component.

### 5. Selective Storage Geometry And Write-Side Internals

`SelectiveStorage` correctly remains the authority for selection routes,
part-file state, verified state, materialization, and publication transitions.
Completed Tactical `078` now gives upload its own immutable `SeedContent` read
owner, so the old sequencing blocker is gone. That owner still imports
`PublicationShape`, `SelectiveStorageError`, and torrent path derivation from
`selective_storage`, revealing that immutable artifact geometry is shared
while it remains housed under the write-side coordinator.

The most useful first seam is therefore storage artifact geometry and path
derivation, with existing public re-exports and error behavior preserved.
After that dependency is one-way, a dedicated write-side tactical may assess
these already-visible private seams:

- backing references and bounded lease acquisition;
- immutable write and hash plans plus their blocking execution;
- descriptor and platform preparation and validation; and
- path publication and namespace durability.

Keep `SelectiveStorage` as the state-transition coordinator. Do not turn the
children into services, make upload depend on write-side state, or split all
five concerns mechanically in one pass. This remains the leading storage-
specific engine candidate when engine work next changes storage, not the
default engine cleanup while dual-stack transport is active.

### 6. Direct-Engine Discovery Compatibility Boundary

The driver facade is now 6,379 physical lines including interspersed test
support and received 25 touches in the current window. Its private
`TrackerManager`, direct DHT retry path, content
discovery tasks, and metadata coordinator are now separable from the
application path, which uses long-lived external discovery. A private driver
child could make that standalone-engine compatibility owner explicit while
leaving the facade responsible for public entry points and top-level download
orchestration.

This is conditional, not a recommendation to delete or unify the two
lifetimes. The direct path still supports focused engine APIs and tests, and
it intentionally lacks the application owner's HTTP/HTTPS tracker transport.
Select this story only when another direct-engine discovery or metadata
change would otherwise expand the facade, or when the product decides the
compatibility surface can change. Preserve public paths and do not make the
session advertisement owner a dependency of the download driver.

### 7. Android Product Graduation

The Android application still lives under
`experiments/android-engine-bootstrap`. The 794-line legacy `EngineService`
has one touch in the current 200-commit window, while the 962-line
`ProductEngineService` has three and remains the active Compose/application
path; both remain in the manifest, and `MainActivity` can still invoke both.
Tactical `098` initializes platform trust before either service constructs
native network owners, and Tactical `111`'s successful physical product-MSE
profile further validates the product path without resolving the dual
packaging.

Graduation is a product/repository decision, not a response to Kotlin file
size. It needs its own tactical after the durable Android location is
accepted. Preserve Compose, SAF, foreground-service, and generated
Rust/Kotlin contracts while removing or isolating the diagnostic path. Do not
mix that decision into an engine, TLS, or storage refactor.

## Watch List And Deliberate Non-Work

- **Session network and dual-stack DHT:** Tactical `097` created one cohesive
  runtime owner, and later settings reused its reconciler without adding a
  second task or channel. Tactical `112` successfully exercised its transport-
  generation and single-DHT-actor boundaries with a second family. Do not
  split transport, mapping, DHT, discovery, or settings reconciliation by
  family. Revisit the DHT actor only when new behavior owns an independent
  policy or cannot be tested without unrelated runtime setup; Tactical `113`
  belongs to the already separate reachability coordinator.
- **Download control and future session admission:** `DownloadControl` now
  spans cancellation, checker state, peer/metadata diagnostics, storage
  pressure, and resource accounting. Planned Tactical `114` explicitly moves
  memory, disk/hash, and connection authority above individual torrents.
  Let that feature identify the retained per-download observation boundary;
  do not pre-invent a generic resource manager or split coherent counters from
  the transitions that update them.
- **Swarm state:** its scheduling, request generations, piece bookkeeping, and
  storage completion remain one deterministic invariant set after the
  independently changing activation policy moved to `piece_picker`. Consider
  another extraction only when a policy changes independently or tests
  require unrelated setup.
- **Session views:** Tactical `080` remains healthy after subsequent settings,
  DHT, tracker, ETA, and product-view additions. The 2,090-line hub is still
  one coordinator, not a size-driven candidate; new ETA behavior already owns
  an independent pure child.
- **Gateway:** HTTP routes, the multiplexed application WebSocket, and first-
  run web authentication are separate owners. The large private HTTP
  integration tests could be categorized for navigation, but metrics,
  registry, connection pump, attachment, writer, and authentication should
  move only when transport work creates an independent lifecycle or test seam.
- **Crate graph:** no candidate currently earns a new crate. Private module
  extraction should be the default.
- **Entry-point documentation:** tactical checkpoints should link the
  authoritative queue instead of copying it. Reconcile stale campaign or
  direction checkpoints in their owning topics; do not turn documentation
  drift into a source-refactor justification here.

## Near-Term Recommendation

Do not open a repository-wide umbrella refactor or delay authoritative
Tactical `113` for a speculative network or DHT split. Tactical `112` already
proved family independence through the current `SessionNetworkRuntime`,
socket, UDP, incoming, peer-connect, and DHT owners. Tactical `113` has a
different concrete seam: shared bounded gateway discovery feeding independent
IPv4-mapping and IPv6-firewall clients inside the existing reachability
coordinator. Let that feature own the small protocol-boundary refactor already
specified by its Gate 1.

If the authoritative queue is explicitly paused for one standalone structural
tactical, the strongest general story remains application callback adapters
plus categorized tests. The most strategically timed bounded alternative is
the `SessionStore` schema/migration extraction because schema 16 is landed and
planned queue facts are next. Before Tactical `114`, revisit the application
boundary because that feature replaces its single-active-torrent core. DHT
actor extraction is not promoted above those candidates: its second family
increased size but did not reveal a second owner. Web validation and immutable
storage geometry remain independent lower-timed candidates. Do not combine
any of these merely to amortize validation.

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

- **2026-08-09:** Refreshed the repository snapshot at source commit
  `f8f2671`, 16 commits after `0b25152` and after Tactical `112` completed.
  Dual-stack transport preserved one session-network reconciler and one UDP
  owner; independent DHT nodes remained cohesive under one actor, command
  route, and observation owner. DHT production/test size grew materially and
  is now a watch point, but no lifecycle or dependency seam justifies a split.
  Application callback/test topology and schema/migration ownership remain
  the strongest standalone candidates; neither displaces Tactical `113`.
- **2026-08-09:** Refreshed the repository snapshot at source commit
  `0b25152`, 121 commits after the prior baseline and after Tactical `111`'s
  physical graduation. Application callback/test topology and store
  schema/migrations remain the strongest general candidates. MSE exposes a new
  role-specific peer-bootstrap seam, while low recent storage churn moves
  selective-storage cleanup down on timing. The session-network, view,
  gateway-auth, and DHT owners remain coherent; no crate split or prerequisite
  refactor displaces Tactical `112`.
- **2026-08-09:** Completed Tactical `111`. Pure MSE remains in protocol, one
  engine DH owner bounds blocking work, and role-specific handshake execution
  stays inside existing connection generations without a new long-lived task.
  The physical Pixel 7a gate did not expose a lifecycle or resource-owner
  defect. It did make outgoing and incoming pre-stream bootstrap the next
  concrete engine source-shape candidate if dual-stack work creates pressure.
- **2026-08-06:** Completed Tactical `098`. The existing session reconciler
  gained one tracker-authentication domain; the engine advertisement owner
  retained one replaceable passive client pair; and the Android adapter gained
  one process bootstrap. In-flight operations retain old pairs through `Arc`
  ownership without a generation task or cache. No generic TLS abstraction,
  callback registry, separate settings owner, or crate split was justified.
- **2026-08-06:** Refreshed the repository snapshot at `6ffaeff`, 101 commits
  after the prior baseline. Sustained application and persistence growth moves
  the application callback/test topology and session-store schema boundary
  ahead of size-only candidates. Web semantic validation is the strongest
  web-only story. Completed seeding makes immutable storage geometry the first
  concrete `SelectiveStorage` seam, while the new session-network owner waits
  for planned Tactical `098` before reassessment. No standalone refactor or
  crate split was selected.
- **2026-08-06:** Completed Tactical `097`. One private session-network owner
  now reconciles all five client settings live while peer, upload, DHT,
  discovery, and endpoint state retain their real long-lived owners.
  Candidate transport and reachability generations remain concrete children;
  persistence, torrent runtimes, views, adapters, and the crate graph retain
  their prior boundaries.
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
