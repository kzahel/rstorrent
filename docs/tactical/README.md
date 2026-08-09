# Implementation Tactical Docs

Bounded implementation plans and execution records live here.

Use zero-padded numeric filenames:

```text
000-first-slice.md
001-next-slice.md
```

Keep one coherent implementation slice per tactical. A tactical should be
small enough to have one falsifiable stopping condition while still producing
an end-to-end result. Parent sequencing documents are allowed when a campaign
needs them, but individual implementation work should still have bounded child
tacticals.

Create a tactical before substantial implementation. It should normally state:

- status;
- motivation and desired outcome;
- dependencies and references;
- scope;
- non-goals;
- contracts and invariants;
- implementation direction without unnecessary line-by-line prescription;
- exact validation and interoperability evidence; and
- the stopping condition or next-slice boundary.

Update the tactical as implementation reveals new facts. When complete, record
what landed, what validation actually ran, known gaps, and the recommended next
slice. Completed tacticals remain in place as execution records; living
direction belongs in `../topics/`.

## Decision-Complete Tacticals

A tactical intended for autonomous execution must settle enough direction that
ordinary implementation discoveries do not require repeated approval. In
addition to the fields above, record:

- the stable topic scenarios or observations that define the problem and the
  exact subset the tactical must make pass;
- the normative specifications and pinned reference source/tests that must be
  surveyed before finalizing state transitions;
- the owner, task, cancellation, dependency, and data-flow map, including
  which state must remain runtime independent;
- exact initial resource bounds, or a bounded range plus explicit authority to
  choose and tighten a conservative value from reference and test evidence;
- shape-changing edge cases that must land with the common path rather than be
  deferred into an incompatible architecture;
- the staged implementation order and the intermediate gates that keep a
  large slice diagnosable;
- a validation matrix separating pure state, scripted runtime, controlled
  interoperability, platform build, and opt-in live evidence;
- explicit non-goals and the next-slice boundary; and
- an escalation contract naming what does and does not require human input.

Unless a tactical says otherwise, in-scope implementation authority includes
ordinary refactoring, adding adversarial cases implied by its invariants,
choosing internal names and module layout, tightening declared limits, fixing
newly exposed bugs at the same ownership boundary, updating generated types,
and updating the tactical and owning topics with actual evidence. These are not
reasons to stop merely because the initial plan did not predict the exact code
edit.

Stop for human direction when evidence requires a materially different product
behavior, protocol-support claim, persistence or compatibility contract,
external dependency or license posture, destructive data action, visible or
physical-device interaction not already authorized, or a significant expansion
beyond the tactical's stated owner and non-goals. An ordinary test failure,
internal refactor, conservative bound choice, public-smoke timeout, or a
reference implementation whose architecture differs from RSTorrent is not by
itself an escalation.

Autonomy does not broaden permissions silently. A tactical that needs public
network access, fixture downloads, emulator/device use, schema migration,
generated-contract changes, or another externally visible action must state
that scope and its cleanup or compatibility rules explicitly.

## Current Tacticals

- [`000-first-verified-piece.md`](000-first-verified-piece.md): completed
  download and verification of one multi-block piece from a controlled
  libtorrent peer, establishing the pure protocol/runtime boundary.
- [`001-bounded-large-piece.md`](001-bounded-large-piece.md): completed
  block-granular staging and streamed verification of a 32 MiB piece under a
  256 KiB engine-owned payload allowance.
- [`002-selective-multi-file-storage.md`](002-selective-multi-file-storage.md):
  completed cross-file mapping, skipped-file part storage, mixed-source
  verification, durable reopen, and materialization through an edge-rich
  libtorrent fixture.
- [`003-android-storage-feasibility.md`](003-android-storage-feasibility.md):
  completed native descriptor, SAF, sparse-offset, reopen, cancellation,
  publication, filesystem, and allocation evidence in three runs each on an
  AVD, Chromebook ARCVM, physical Pixel 7a, and Moto X4 internal and removable
  exFAT storage.
- [`004-android-engine-bootstrap.md`](004-android-engine-bootstrap.md):
  completed in-process engine packaging behind UniFFI, foreground-service
  ownership, direct Rust networking, bounded app-private storage, cancellation,
  peer-failure, activity-recreation, and exact cleanup evidence on an AVD,
  Chromebook ARCVM, and Moto X4.
- [`005-saf-selective-storage.md`](005-saf-selective-storage.md): closed after
  proving descriptor-backed selective download, provider publication, forced
  restart verification, and exact cleanup on an AVD, Chromebook ARCVM, and
  Pixel 7a. Unavailable Moto rows and remaining provider-failure profiles are
  recorded as deferred rather than claimed.
- [`006-magnet-metadata-peer-hint.md`](006-magnet-metadata-peer-hint.md):
  completed bounded v1 magnet parsing, direct `x.pe` bootstrap, bidirectional
  BEP 9 metadata exchange, same-connection content download, and independent
  libtorrent evidence in both directions.
- [`007-durable-session-control.md`](007-durable-session-control.md):
  completed one transport-neutral application contract,
  profile-local SQLite authority, exact magnet metadata retention, durable
  verified-piece checkpoints, and conservative process-death resume.
- [`008-reactive-multi-surface-control.md`](008-reactive-multi-surface-control.md):
  completed recoverable bounded reactive views, generated TypeScript and
  Kotlin contracts, and controlled browser/WebSocket, Tauri/channel, and
  Android/UniFFI product threads.
- [`009-android-saf-session-storage.md`](009-android-saf-session-storage.md):
  completed durable Android SAF root identity, descriptor-backed restart,
  provider publication recovery, and controlled AVD and Pixel evidence.
- [`010-peer-registry-magnet-failover.md`](010-peer-registry-magnet-failover.md):
  completed bounded peer observations, records, deterministic selection,
  guarded dial lifecycle, connection and protocol failover, and
  same-connection magnet handoff.
- [`011-one-shot-udp-tracker.md`](011-one-shot-udp-tracker.md): completed
  bounded BEP 15 connect/announce exchange, tracker observations, session
  source retention, and tracker-only magnet metadata/content transfer from a
  controlled libtorrent seed.
- [`012-bounded-diagnostics-progress.md`](012-bounded-diagnostics-progress.md):
  completed prompt task-terminal supervision, active/waiting/blocked progress
  assessment, a bounded filtered typed diagnostic stream, equivalent
  web/Tauri and Android presentation, and isolated headless Chrome and
  no-window AVD evidence.
- [`013-explicit-live-network-policy.md`](013-explicit-live-network-policy.md):
  completed explicit offline, loopback-only, and online outbound policy,
  online desktop and Android product networking, loopback-isolated harnesses,
  offline progress, and bounded network-operation deadlines without a
  whole-download timeout.
- [`014-scheduled-udp-tracker-lifecycle.md`](014-scheduled-udp-tracker-lifecycle.md):
  completed supervised UDP tracker records, multi-tracker fallback, bounded
  retry and reannounce scheduling, loss recovery, token reuse, and equivalent
  waiting diagnostics on the web and Android surfaces.
- [`015-headless-live-comparison.md`](015-headless-live-comparison.md):
  complete; added the catalog-backed alternating comparator, controlled
  publication validation, deterministic result tests, and first paired
  metadata and full-download baselines.
- [`016-dht-discovery-foundation.md`](016-dht-discovery-foundation.md):
  complete; added the session-owned bounded IPv4 DHT participant, private
  gating, warm restart, peer integration, controlled libtorrent completion,
  and an honest public trackerless attempt.
- [`017-adversarial-multi-peer-liveness.md`](017-adversarial-multi-peer-liveness.md):
  complete; replaced the one-live-peer content boundary with a bounded
  torrent-owned connection set and request scheduler driven by adversarial
  liveness scenarios.
- [`018-inspectable-metadata-acquisition.md`](018-inspectable-metadata-acquisition.md):
  complete; added bounded peer-registry and BEP 9 acquisition snapshots,
  closed metadata-slot starvation, and classified tracker-only failure versus
  repeated public DHT metadata completion.
- [`019-torrent-owned-metadata-acquisition.md`](019-torrent-owned-metadata-acquisition.md):
  complete; replaced independent per-peer BEP 9 transfers with one bounded
  cross-peer block owner, added source-derived request pacing, and met the
  tracker, DHT functional, and catalog metadata gates.
- [`020-sustained-transfer-parity.md`](020-sustained-transfer-parity.md):
  complete; replaced the static four-request/four-piece transfer ceiling with
  a bounded source-derived per-connection feedback window and classified
  initial peer-source breadth as the remaining 50% boundary.
- [`021-initial-peer-working-set.md`](021-initial-peer-working-set.md): complete;
  adds bounded initial tracker-operation breadth, separates half-open and live
  peer capacity, adds per-peer diagnostics, and classifies a duplex peer-task
  deadlock as the next boundary.
- [`022-duplex-peer-task-liveness.md`](022-duplex-peer-task-liveness.md): complete;
  breaks command/event backpressure cycles without dropping or unbounding
  peer messages and passes 3/3 owner-only plus 3/3 paired 50% screens.
- [`023-strict-endgame-ownership.md`](023-strict-endgame-ownership.md): complete;
  adds strict bounded duplicate requests, first-response cancellation, harmless
  late payload, and exact public verified-publication evidence.
- [`024-piece-hash-failure-recovery.md`](024-piece-hash-failure-recovery.md):
  complete; resets a failed v1 piece, retains bounded contributor generations,
  and distinguishes known-bad from ambiguous peer evidence.
- [`025-bounded-async-content-storage.md`](025-bounded-async-content-storage.md):
  complete; separates bounded storage execution from peer-event progress with
  exact payload, hash, resume, and shutdown ownership, and records a negative
  localhost speed result.
- [`026-paired-peer-utility-timeline.md`](026-paired-peer-utility-timeline.md):
  complete; adds bounded endpoint-free time-series evidence for both
  comparator owners and classifies candidate supply plus half-open admission.
- [`027-expanded-half-open-working-set.md`](027-expanded-half-open-working-set.md):
  complete; expands the source-rich startup dial cohort from eight to 30 while
  preserving the separate 30-live-peer and fixed payload bounds.
- [`028-fair-content-supervisor-intake.md`](028-fair-content-supervisor-intake.md):
  complete; prevents continuous accepted-block storage from starving bounded
  discovery admission, dial refill, and safe peer-event service.
- [`029-coalesced-selective-piece-hashing.md`](029-coalesced-selective-piece-hashing.md):
  complete; removes redundant per-chunk async seeks from bounded multi-file
  piece verification and records a neutral controlled timing result.
- [`030-single-boundary-selective-hash-job.md`](030-single-boundary-selective-hash-job.md):
  complete; moves each common all-wanted piece hash behind one bounded blocking
  positional-I/O boundary and records neutral controlled/public results.
- [`031-storage-command-duration-evidence.md`](031-storage-command-duration-evidence.md):
  complete; separates bounded storage queue wait from per-kind write and hash
  service duration and attributes the live bottleneck to serialized writes.
- [`032-bounded-coalesced-write-batches.md`](032-bounded-coalesced-write-batches.md):
  complete; batches at most 16 already-admitted blocks and coalesces adjacent
  piece ranges while retaining per-block integrity and cancellation ownership.
  Physical writes fell by about 91%, but controlled wall time stayed neutral
  and public serialized storage service remained above 93%.
- [`033-headless-view-set-foundation.md`](033-headless-view-set-foundation.md):
  complete; establishes leased multi-view sets, recoverable authenticated
  polling, generated TypeScript and JSON Schema, a pure reducer and headless
  lifecycle client, and controlled libtorrent-seeded view evidence before
  React or peer-table work.
- [`034-responsive-demo-inspection-ui.md`](034-responsive-demo-inspection-ui.md):
  complete; establishes the fresh React, Zustand, CSS Modules, responsive
  inspection shell, virtual tables, deterministic named demo scenarios, and
  headless visual/accessibility/scale evidence before connecting new Rust
  peer projections.
- [`035-live-peer-inspection-projection.md`](035-live-peer-inspection-projection.md):
  complete; unifies active connection observation, exposes truthful bounded
  torrent/peer views, independently reaps silent view sets, connects semantic
  responsive views to the live React surface, and proves suspended-client
  recovery plus a verified libtorrent transfer entirely headlessly.
- [`036-manual-live-webui-launcher.md`](036-manual-live-webui-launcher.md):
  complete; adds one production-built live browser launcher with persistent
  isolated state, an exact loopback boundary, normal-browser opening, and
  joined `Ctrl+C` shutdown. Tauri migration awaits maintainer confirmation.
- [`037-live-magnet-toolbar-intake.md`](037-live-magnet-toolbar-intake.md):
  complete; adds JSTorrent-style adjacent magnet input and Add controls to the
  live React toolbar and moves the controlled transfer proof through that
  visible command path while reserving later `.torrent` file selection.
- [`038-curated-test-torrent-menu.md`](038-curated-test-torrent-menu.md):
  complete; adds an accessible More > Add test torrent submenu backed by the
  five recorded WebTorrent magnets and the ordinary guarded add path.
- [`039-generous-download-resource-pipelines.md`](039-generous-download-resource-pipelines.md):
  complete; replaces the prototype two-block payload ceiling with explicit,
  generous desktop and bounded Android request, buffering, and active-piece
  budgets.
- [`040-torrent-lifecycle-retention-actions.md`](040-torrent-lifecycle-retention-actions.md):
  complete; adds durable archive/restore and fenced keep-data or
  delete-managed removal through the semantic contract and web UI.
- [`041-live-file-inspection.md`](041-live-file-inspection.md): complete; adds a
  bounded complete live file projection with distinct Done and Verified
  progress, correct configurable virtual-table behavior, and controlled
  headless multi-file proof.
- [`042-metadata-display-name.md`](042-metadata-display-name.md): complete;
  replaces the live info-hash label with the verified durable metainfo name in
  library and General summaries while preserving the pre-metadata fallback.
- [`043-live-tracker-inspection.md`](043-live-tracker-inspection.md): complete;
  establishes one bounded authoritative tracker runtime projection, connects
  it to the leased API and responsive Trackers table, and proves the path with
  an isolated tracker-only browser transfer.
- [`044-global-disk-inspection.md`](044-global-disk-inspection.md): complete;
  establishes a session-scoped storage pipeline contract, pressure behavior,
  piece-level active work, and the global responsive Disk inspection surface.
- [`045-piece-map-visualization.md`](045-piece-map-visualization.md): complete;
  generalizes bounded active-piece diffs and implements a
  high-DPI, RAF-coalesced, read-only Canvas piece overview.
- [`046-joined-pause-peer-cleanup.md`](046-joined-pause-peer-cleanup.md): complete;
  makes pause await metadata/content owner cleanup and the final empty peer
  observation before its successful receipt.
- [`047-interface-size-settings.md`](047-interface-size-settings.md): complete;
  adds the global Settings surface, a readable Standard default, coordinated
  Compact/Standard/Spacious metrics, and legible first-party action icons.
- [`048-unified-view-delivery-and-tauri-migration.md`](048-unified-view-delivery-and-tauri-migration.md):
  complete; unifies leased view-set delivery across HTTP polling and
  acknowledged in-process Tauri streaming, then makes the React inspection
  surface the desktop default before categorized Logs work.
- [`049-structured-log-console.md`](049-structured-log-console.md): complete;
  replaces the sortable Logs scaffold with a global ordered diagnostic
  console, structured expandable context, producer capture interest, honest
  gap semantics, and bounded pull/stream delivery with headless live evidence.
- [`050-color-theme-settings.md`](050-color-theme-settings.md): complete; adds
  Auto/Light/Dark selection to the shared Settings surface with safe appearance
  migration, pre-React application, and system-responsive Auto behavior.
- [`051-typed-peer-flags-and-legend.md`](051-typed-peer-flags-and-legend.md):
  complete; replaces ambiguous peer flag strings with a typed Rust semantic
  set, one shared frontend vocabulary, and an accessible column-header legend.
- [`052-batched-durability-checkpoints.md`](052-batched-durability-checkpoints.md):
  complete; separates hash verification from bounded payload/SQLite durability
  epochs, moves checkpoint work off the content supervisor, and establishes a
  steady controlled storage profile for the throughput campaign.
- [`053-immutable-positional-storage-plans.md`](053-immutable-positional-storage-plans.md):
  complete; replaces mutable cursor I/O with retained, generation-checked
  positional write and hash plans across wanted and part-file storage.
- [`054-bounded-independent-storage-execution.md`](054-bounded-independent-storage-execution.md):
  complete; runs immutable write and hash jobs through separate bounded
  capacity, joins out-of-order results by exact piece generation, and retains
  a repeated 1/10 GiB libtorrent comparison as the first throughput gate.
- [`055-application-destinations.md`](055-application-destinations.md):
  complete; establishes Library, Transfers, and Workbench as responsive
  top-level destinations, adds truthful bounded clean views and shared
  multi-selection, and preserves the existing dense surface as Workbench.
- [`056-peer-client-identification.md`](056-peer-client-identification.md):
  complete; identifies common clients and versions from bounded handshake
  peer IDs and completes the existing active-peer Client projection.
- [`057-hardware-performance-baselines.md`](057-hardware-performance-baselines.md):
  complete; retains hardware-matched 1/10 GiB engine gates and paired
  per-view/adversarial application throughput evidence in local and CI tiers.
- [`058-contextual-table-selection.md`](058-contextual-table-selection.md):
  complete; replaces persistent torrent checkboxes with an explicit selection
  mode and applies the same UI-only interaction plus deferred actions to Files.
- [`059-actionable-table-range-selection.md`](059-actionable-table-range-selection.md):
  complete; keeps checkbox columns visible on actionable tables and adds
  sorted Shift-range selection across torrent and Files surfaces.
- [`060-multiplexed-application-websocket.md`](060-multiplexed-application-websocket.md):
  complete; makes one bounded multiplexed WebSocket the ordinary live-browser
  connection, retains HTTP only as an explicit loopback diagnostic, shares
  exact acknowledgement with Tauri, and retires the superseded `/control`
  protocol and direct-DOM frontend after migrating their useful evidence.
- [`061-user-selected-download-roots.md`](061-user-selected-download-roots.md):
  authorized for macOS; replaces implicit app-data payload storage with
  persisted user-selected roots, default/add-option policy, trusted local
  folder pickers, and shared add/Settings UX while deferring staged magnet file
  selection plus Linux and Windows native evidence.
- [`062-user-visible-publication-layout.md`](062-user-visible-publication-layout.md):
  complete; publishes completed downloads in a user-visible torrent-named
  layout while retaining private staging and exact restart behavior.
- [`063-live-file-selection.md`](063-live-file-selection.md): complete; adds
  durable live Normal/Skip file selection, metadata-only add intent, exact
  boundary-piece storage behavior, and safe joined generation replacement.
- [`064-registry-backed-swarm-inspection.md`](064-registry-backed-swarm-inspection.md):
  planned; turns the bounded peer registry into the torrent-scoped Swarm view
  while preserving Peers as active connection generations.
- [`065-dht-observatory.md`](065-dht-observatory.md): complete; adds the
  session-scoped DHT observatory with exact 160-bucket occupancy and freshness,
  bounded lookup convergence, and normalized/literal diagnostic encodings.
- [`066-smooth-session-speed-history.md`](066-smooth-session-speed-history.md):
  complete; retains bounded exact session byte history in live and durable
  tiers and renders a smooth hand-rolled high-DPI Canvas chart without a
  general chart dependency.
- [`067-dynamic-platform-file-acquisition.md`](067-dynamic-platform-file-acquisition.md):
  proposed; replaces Android's eager SAF descriptor manifest with bounded
  dynamic acquisition and one session-wide Rust file pool shared with path
  storage.
- [`068-active-and-batch-table-interaction.md`](068-active-and-batch-table-interaction.md):
  superseded; established synchronized active-row and batch-selection
  mechanics before product trial exposed ambiguous shared action scope.
- [`069-current-within-table-selection.md`](069-current-within-table-selection.md):
  complete; constrains current row to checked selection and makes every table
  action target that one explicit selection set.
- [`070-actionable-torrent-error-status.md`](070-actionable-torrent-error-status.md):
  complete; makes torrent error status explanatory on hover and focus, then
  routes activation to the focused General error detail.
- [`071-copy-magnet-link.md`](071-copy-magnet-link.md): complete; adds a
  singleton-selection More action that copies a canonical v1 magnet from the
  projected info hash with truthful clipboard feedback and accessible focus.
- [`072-derived-media-catalog.md`](072-derived-media-catalog.md): draft; adds a
  pure deterministic video/episode classifier, one rebuildable application
  catalog, a separately leased media projection, and a read-only virtualized
  Workbench Media tab without thumbnails or playback.
- [`073-unified-storage-and-complete-recheck.md`](073-unified-storage-and-complete-recheck.md):
  complete; removes the single-file storage fork, gives every v1 torrent one
  durable resume/publication path, and adds bounded full managed-storage and
  force recheck with controlled libtorrent fault evidence.
- [`074-context-specific-metainfo-limits.md`](074-context-specific-metainfo-limits.md):
  complete; separates generic bencode, BEP 9, durable-session, structural,
  and explicit-import parser limits, raises only the schema-owned durable
  piece/have checks, and records bounded allocation and unchanged controlled
  interoperability without adding `.torrent` product intake.
- [`075-ephemeral-application-state.md`](075-ephemeral-application-state.md):
  complete; adds explicit private bounded in-memory session and metrics state,
  typed page-cap exhaustion, a no-profile-path headless mode, and controlled
  no-file lifecycle evidence while leaving external payload storage and
  persistent source policy unchanged.
- [`076-authenticated-private-web-host.md`](076-authenticated-private-web-host.md):
  complete; adds bounded Basic-auth static, health, and application hosting,
  explicit same-origin production bootstrap, joined terminate handling, and
  externally owned exact-push deployment evidence without recording host
  identity or credentials in this repository.
- [`077-shared-overlay-menu-system.md`](077-shared-overlay-menu-system.md):
  planned; replaces component-local menu and popover mechanics with one
  portalled, collision-aware, accessible React Aria layer while reserving new
  product context-menu bindings for a later policy decision.
- [`078-local-single-peer-tcp-seeding.md`](078-local-single-peer-tcp-seeding.md):
  complete; adds one session-owned loopback TCP listener, generation-fenced
  torrent routing, verified metadata and payload upload, restartable seeding
  ownership, per-application peer identity, and controlled
  RSTorrent/libtorrent evidence for one peer.
- [`079-engine-driver-source-shape.md`](079-engine-driver-source-shape.md):
  complete; extracts download-control observation and bounded content-storage
  execution from the engine driver and divides its private test suite by owner
  without changing behavior, crate boundaries, or public API.
- [`080-session-view-subsystem-boundaries.md`](080-session-view-subsystem-boundaries.md):
  planned; separates portable view contracts, current projection models,
  legacy subscriptions, leased view-set delivery, diffs, and ranges behind
  one-way private dependencies without changing API or behavior.
- [`081-v1-torrent-byte-intake.md`](081-v1-torrent-byte-intake.md): planned;
  separates exact source provenance from operational metadata, adds bounded
  v1 outer-metainfo and tracker-tier persistence, adopts libtorrent-aligned
  large-v1 parser/geometry limits with scalable storage and paged catalog
  owners, and carries one semantic byte-intake operation through WebSocket,
  HTTP automation, and raw Tauri IPC without adding visible picker UX or
  chunking.
- [`082-bounded-multi-peer-upload-ownership.md`](082-bounded-multi-peer-upload-ownership.md):
  completed; replaces the one-peer upload proof with shared connection
  budgets, eight fair seed upload slots, adaptive bounded reads/writes, and
  exact physical payload accounting. Simultaneous two-RSTorrent/two-libtorrent
  clients independently verified four complete copies while all declared
  limits and joined cleanup held.
- [`083-shared-torrent-file-picker.md`](083-shared-torrent-file-picker.md):
  complete; adds one shared browser/Tauri torrent-file chooser, bounded binary
  intake through the active adapter, server-derived source identity, initial
  all-file selection, and headless WebSocket product evidence.
- [`084-persisted-client-connection-and-seeding-settings.md`](084-persisted-client-connection-and-seeding-settings.md):
  planned; defines a typed settings subsystem and one atomic persisted group
  for loopback listener policy, the global peer ceiling, and payload upload
  slots through startup enforcement, existing application transports and
  views, and the shared browser/Tauri Settings surface.
- [`085-unified-contextual-selection-actions.md`](085-unified-contextual-selection-actions.md):
  planned; unifies torrent and file action policy across visible toolbars,
  More, and row context menus, including whole-selection magnet copy, recheck,
  archive/restore, and confirmed sequential multi-torrent removal.
- [`086-long-lived-torrent-peer-runtime.md`](086-long-lived-torrent-peer-runtime.md):
  planned; extracts one application-generation per-torrent runtime and one
  shared engine peer-state owner, then proves the boundary by attaching
  incoming seed connections to the ordinary bounded Peers/Swarm lifecycle.
- [`087-uniform-detail-tab-footprints.md`](087-uniform-detail-tab-footprints.md):
  complete; removes redundant detail-tab count badges, gives every tab one
  interface-size-specific footprint, and retains the torrent/session divider
  plus horizontal overflow on constrained widths.
- [`088-upnp-mapped-external-tcp-seeding.md`](088-upnp-mapped-external-tcp-seeding.md):
  complete; adds bounded IPv4 UPnP IGD v2 mapping ownership and proves exact
  payload seeding from an independently controlled off-LAN peer followed by
  verified mapping deletion and failed reconnect.
- [`089-coordinated-session-listen-sockets.md`](089-coordinated-session-listen-sockets.md):
  complete; coordinates application-generation TCP and UDP bind policy,
  migrates DHT to one bounded session UDP owner, and reports actual transport
  endpoints separately before truthful peer advertisement.
- [`090-peer-id-duplicate-connection-resolution.md`](090-peer-id-duplicate-connection-resolution.md):
  planned; resolves simultaneous live connections by handshake peer ID with a
  deterministic cross-direction winner and generation-fenced exact cleanup,
  without treating peer ID as durable identity or merging endpoint records.
- [`091-availability-ranked-piece-activation.md`](091-availability-ranked-piece-activation.md):
  complete; retains partial-first work and unique-piece protection, adds exact
  live availability accounting and an independent active-piece count ceiling,
  and graduates an incrementally indexed rarest-first default through naive
  differential, maximum-geometry CPU/memory, and controlled libtorrent gates.
- [`092-truthful-tracker-and-dht-peer-advertisement.md`](092-truthful-tracker-and-dht-peer-advertisement.md):
  complete; replaces provisional tracker/DHT peer-port claims with one
  generation-fenced advertised endpoint and retains discovery plus
  advertisement across the long-lived torrent runtime.
- [`093-bep6-fast-request-lifecycle.md`](093-bep6-fast-request-lifecycle.md):
  planned; implements the complete negotiated Fast request lifecycle,
  including explicit rejection, exactly-one terminal responses, initial
  availability forms, and bounded suggestions and allowed-fast state.
- [`094-bounded-bep11-peer-exchange.md`](094-bounded-bep11-peer-exchange.md):
  planned; adds a bounded general BEP 10 negotiation map and bidirectional PEX
  with exact live add/drop events, private gating, source diversity, hostile
  input limits, and controlled two-hop evidence.
- [`095-bounded-http-https-tracker-transport.md`](095-bounded-http-https-tracker-transport.md):
  complete; adds bounded HTTP and encrypted-but-unauthenticated HTTPS tracker
  transport, compact/noncompact IPv4/IPv6 peer intake, family-correct
  advertisement, and controlled libtorrent plus Android product evidence.
- [`096-metadata-tracker-activation-and-family-observability.md`](096-metadata-tracker-activation-and-family-observability.md):
  complete; activates session-owned discovery only while a paused metadata
  task is actually live, projects the last successful tracker connection
  family without addresses, and passes controlled plus Ubuntu evidence.
- [`097-live-client-settings-and-replaceable-session-generations.md`](097-live-client-settings-and-replaceable-session-generations.md):
  complete; applies every existing client setting without restart through one
  stable session-network owner and replaceable TCP/UDP/reachability
  generations while preserving peers, DHT, discovery, accounting, exact
  transfer, and bounded terminal ownership.
- [`098-authenticated-https-tracker-platform-trust.md`](098-authenticated-https-tracker-platform-trust.md):
  complete; defaults HTTPS trackers to desktop and Android platform trust,
  retains one hidden live compatibility policy, atomically replaces only the
  bounded family-specific HTTP client pair, and passes controlled
  authenticated pinned-libtorrent interoperability plus desktop/AVD runtime
  evidence.
- [`099-decimal-and-binary-display-units.md`](099-decimal-and-binary-display-units.md):
  complete; makes Decimal `kB/MB/GB` the fresh and migrated browser default,
  retains persistent Binary `KiB/MiB/GiB` as an explicit choice, and updates
  every shared React byte/rate display without changing raw application data,
  sorting, chart geometry, or exact technical IEC copy.
- [`100-bep53-select-only-and-duplicate-add-feedback.md`](100-bep53-select-only-and-duplicate-add-feedback.md):
  complete; adds bounded BEP 53 select-only magnets and their monotonic
  duplicate-selection rule, makes ordinary duplicate adds successful no-ops,
  and consistently reveals the typed add target with accessible feedback.
- [`101-first-run-web-authentication.md`](101-first-run-web-authentication.md):
  complete; adds a communicated ten-minute loopback onboarding window,
  local-open or remembered-browser policy, four-digit authorization for new
  browser profiles, persistent bounded cookie sessions, explicit restart
  recovery, adaptive Web access session management, and retention of Basic,
  bearer, development, and in-process modes.
- [`102-ordinary-incoming-listener-settings.md`](102-ordinary-incoming-listener-settings.md):
  complete; makes ordinary automatic and fixed incoming modes bind all IPv4
  interfaces while retaining disabled, loopback, and preferred-candidate
  policies for controlled internal use.
- [`103-peer-transfer-and-recency-columns.md`](103-peer-transfer-and-recency-columns.md):
  complete; exposes already-owned peer upload rate, physical upload total,
  connected age, and last-received-payload age through sortable responsive
  columns without changing the generated application contract.
- [`104-selection-aware-torrent-eta.md`](104-selection-aware-torrent-eta.md):
  accepted; adds exact non-padding network-work geometry, constant-space
  torrent rate smoothing, and a typed selection-aware ETA owned by the Rust
  application view rather than the engine or React.
- [`105-fact-based-persistence-and-recheck-containment.md`](105-fact-based-persistence-and-recheck-containment.md):
  complete; replaces overlapping durable runtime/storage state with fact-based
  payload and verification authority, makes force recheck exclusive and
  restartable, migrates the observed published-content contradiction without
  payload mutation, and contains torrent-local recovery failures during
  profile open.
- [`106-live-transfer-rate-tab-title.md`](106-live-transfer-rate-tab-title.md):
  complete; keeps the shared browser/Tauri tab title current with exact
  session download and upload rates at a bounded one-second cadence while
  retaining the plain application title when idle or disconnected.
- [`107-source-aware-magnet-export.md`](107-source-aware-magnet-export.md):
  complete; preserves exact verified magnet sources and otherwise synthesizes
  bounded magnets from verified identity, name, and ordered tracker evidence
  through one explicit read-only application operation.
- [`108-serialized-torrent-control-and-observable-checking.md`](108-serialized-torrent-control-and-observable-checking.md):
  complete; replaces whole-generation file-selection restart with serialized
  torrent reconciliation and a bounded storage fence, makes checking
  selection-independent, and exposes exact checker progress while retaining
  conservative validation and reserving a later trusting fast-resume option.
- [`109-stable-same-origin-web-launch.md`](109-stable-same-origin-web-launch.md):
  complete; gives the manual browser launcher one stable same-origin hosted
  gateway, removes caller-selected live destinations, and proves reconnect
  through a process restart without changing the visible URL.
- [`110-atomic-download-now.md`](110-atomic-download-now.md):
  complete; adds one atomic wanted-plus-running application command, reconciles
  only current durable intent through the serialized torrent controller, and
  exposes it for skipped targets in the shared Files action menus.
- [`111-mse-peer-stream-encryption.md`](111-mse-peer-stream-encryption.md):
  complete; adds MSE/PE for TCP peer connections in both directions through a
  sans-IO protocol state machine and one four-value live client policy, while
  claiming compatibility rather than security. Controlled interop/performance,
  Android cross-build, API 34 AVD, and API 37 physical Pixel 7a product
  evidence pass with exact publication, bounded resources, full owner drain,
  and cleanup.
- [`112-dual-stack-transport-and-ipv6-dht.md`](112-dual-stack-transport-and-ipv6-dht.md):
  complete and graduated; gives the session one
  coordinated TCP/UDP socket pair per address family, runs a BEP 32 IPv6 DHT
  node with its own BEP 42 identity and routing table beside the IPv4 node,
  makes the reachable peer port a per-family fact for BEP 7 announcing, and
  gates every IPv6 path behind one persisted `ipv6_enabled` setting defaulting
  to enabled. Claims no IPv6 incoming reachability.
- [`113-ipv6-firewall-pinhole-and-incoming-reachability.md`](113-ipv6-firewall-pinhole-and-incoming-reachability.md):
  closed, evidence-limited; bounded UPnP IGD v2
  `WANIPv6FirewallControl:1` pinhole control beside the existing IPv4 mapping
  under one reachability coordinator,
  distinguishes listener, unfiltered-gateway, installed-pinhole, and observed
  incoming evidence, and stages an off-LAN IPv6 peer hash-verifying proof.
  Protocol, coordinator/product, and physical-harness commits pass their
  deterministic, scripted, generated-contract, web, and Android gates. The live
  negative control passes, but the observed gateway returns typed `606` to
  `AddPinhole`. Positive physical capability is recorded as unknown on the
  current hardware; the tactical remains ungraduated and no control-transport
  expansion is implied. The pinned oracle implements no pinhole support, so
  every deterministic test is independently authored.
- [`114-session-wide-concurrent-torrent-admission.md`](114-session-wide-concurrent-torrent-admission.md):
  complete session-wide concurrent download admission; schema 17 persists
  queue order and the default-three setting, one application owner admits exact
  generations, shared resources prevent per-torrent multiplication, controlled
  performance and 100/500-catalog scale gates pass, and a physical Pixel 7a
  proves the effective-two Android cap, promotion, exact payload, and cleanup.
- [`115-mse-policy-advertisement-and-peer-detail.md`](115-mse-policy-advertisement-and-peer-detail.md):
  complete bounded post-graduation follow-up; matches libtorrent's default
  plaintext-payload selection under compatibility-only `allow`, derives the
  HTTP tracker MSE capability announcement from live policy, and exposes the
  exact peer method through the existing quiet `E` presentation.
- [`116-platform-storage-coherence-and-ios-feasibility.md`](116-platform-storage-coherence-and-ios-feasibility.md):
  authoritative **Now**, planned and not started after Tactical `114`; closes
  path/Android SAF storage lifecycle fractures, makes Android a same-tactical
  engine parity gate, and requires a bounded physical-iPhone storage,
  networking, and lifecycle probe before fast resume or later engine feature
  breadth.

Tactical `015` completed the oracle campaign's headless measurement
foundation. Current prioritization and the compaction-safe restart
checkpoint live in [`../topics/capability-readiness.md`](../topics/capability-readiness.md)
and [`../topics/oracle-driven-engine-campaign.md`](../topics/oracle-driven-engine-campaign.md).
