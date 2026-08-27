# Topics

Focused, living records of continuing concerns live here.

Prefer the smallest coherent topic whose status, decisions, evidence, gaps, and
next work benefit from continuity across sessions or commits. A topic can own a
product decision, contract, recurring problem, implementation campaign, status
question, or investigation. Split topics when their decisions can evolve
independently.

Create or update a topic when:

- work spans multiple tacticals or commits;
- important decisions or invariants need to survive the current session;
- new evidence changes the direction;
- current status is otherwise difficult to answer; or
- the user explicitly requests a living topic.

Do not create a topic for every small standalone change.

New topics should normally contain:

- a crisp scope;
- a `Topic: <slug>` line matching the filename;
- an honest status;
- the decisions and invariants currently in force;
- relevant evidence, known gaps, and recommended next work; and
- links to implementing tacticals when they exist.

Architecture and reference docs own durable system shape and external facts.
Topics own the current truth for a continuing concern. Tactical docs under
`../tactical/` own bounded implementation slices and execution records.

## Current Topics

- [`beta-release-readiness.md`](beta-release-readiness.md): authoritative beta
  gap ledger, platform release lanes, CI/updater/distribution gates, deliberate
  MVP deferrals, and ordered release tacticals.
- [`product-direction.md`](product-direction.md): initial product motivation,
  first-party Rust engine decision, platform posture, non-goals, open choices,
  and recommended bring-up sequence.
- [`product-surfaces-and-migration.md`](product-surfaces-and-migration.md):
  backend/presentation separation, desktop extension use, ChromeOS Android and
  Crostini choices, launch handoff, backend isolation, and later best-effort
  JSTorrent graduation.
- [`runtime-configurations-and-headless-deployment.md`](runtime-configurations-and-headless-deployment.md):
  visible, background, windowless, and headless runtime compositions; explicit
  Linux service, listener, origin, authentication, and reverse-proxy policy;
  and the separation of backend availability from UI and seeding lifetime.
- [`product-state-and-feedback.md`](product-state-and-feedback.md):
  installation identity, local usage summaries, prompt campaign state,
  lifecycle/version facts, and explicit user-submitted diagnostic context.
- [`capability-readiness.md`](capability-readiness.md): master engine and product
  scoreboard, evidence vocabulary, priority policy, and bounded current queue.
- [`code-organization-and-refactoring.md`](code-organization-and-refactoring.md):
  living module, crate, test-placement, source-pressure, and likely refactor
  boundary snapshot.
- [`download-correctness.md`](download-correctness.md): completion, integrity,
  request ownership, recovery invariants, observed incidents, and executable
  scenario ledger.
- [`protocol-support.md`](protocol-support.md): precise BEP support claims,
  deliberate limits, interoperability evidence, and protocol sequencing.
- [`bittorrent-v2-and-hybrid.md`](bittorrent-v2-and-hybrid.md): accepted BEP
  52 source dossier, implemented pure-v2/hybrid consumption campaign,
  identity/integrity/storage direction, and remaining Partial-claim gaps.
- [`dht-discovery.md`](dht-discovery.md): integrated session-owned dual-stack
  DHT routing, lookup, private-torrent policy, bounded warm restart, versioned
  hybrid lanes, evidence, and named participation gaps.
- [`performance-and-live-evidence.md`](performance-and-live-evidence.md):
  headless libtorrent comparison, public-smoke classification, performance
  measurement, and artifact-safety policy.
- [`public-torrent-testing.md`](public-torrent-testing.md): dated official
  public-torrent catalog, distinct protocol roles, refresh policy, and bounded
  privacy-preserving live-run contract.
- [`storage-throughput-architecture.md`](storage-throughput-architecture.md):
  proposed maximum-throughput receive-to-storage pipeline, positional I/O,
  write/hash joins, part-file coordination, batched durability, and
  session/root scheduling.
- [`android-saf-storage.md`](android-saf-storage.md): persisted Android tree
  capabilities, dynamic document acquisition, shared session-wide descriptor
  pooling, lazy part storage, and the Kotlin namespace/Rust payload boundary.
- [`oracle-driven-engine-campaign.md`](oracle-driven-engine-campaign.md):
  source-first libtorrent-oracle runbook, parity gates, autonomous restart
  checkpoint, milestone sequence, and transition to measured BEP breadth.
- [`client-persistence.md`](client-persistence.md): SQLite-backed client state,
  verified metadata and resume invariants, cross-platform storage-root
  identity, and the application-service boundary above the torrent engine.
- [`download-roots.md`](download-roots.md): user-selected payload roots,
  first-add and default-root UX, platform capability ownership, desktop/WebUI
  picker behavior, and the deferred JSTorrent-like file-selection flow.
- [`application-control.md`](application-control.md): shared semantic commands,
  responses, snapshots, revisions, and the boundary between in-process
  application control and future transports.
- [`settings-mutation-and-draft-consistency.md`](settings-mutation-and-draft-consistency.md):
  typed partial settings updates, atomic merge/validation, command-to-view
  convergence, and client draft ownership under complete live updates.
- [`application-view-api.md`](application-view-api.md): leased view sets,
  named snapshots and diffs, cursor recovery, polling and streaming delivery,
  generated TypeScript/schema, and provisional remote routes.
- [`application-connection-architecture.md`](application-connection-architecture.md):
  one typed application API over HTTP, multiplexed WebSocket and Tauri IPC,
  resumable view attachments, and future opaque encrypted relay layering.
- [`http-file-serving-and-streaming.md`](http-file-serving-and-streaming.md):
  capability-authorized HTTP reads of verified torrent files, ephemeral
  loopback and existing-gateway port policy, and the separate future
  incomplete-file streaming scheduler boundary.
- [`remote-access-authentication.md`](remote-access-authentication.md):
  owner username/passphrase E2E access, SRP and OPAQUE background, host and
  device identity, hardware-backed degradation, clone and active-proxy threat
  scenarios, and pre-implementation security research gates.
- [`client-view-delivery-policy.md`](client-view-delivery-policy.md):
  client-selected real-time, balanced, low-bandwidth and background view
  cadence, lifecycle policy, observer cost, and required evidence.
- [`client-surfaces.md`](client-surfaces.md): shared browser/Tauri web
  presentation, Android Compose adaptation, generated client types, reactive
  view delivery, and platform lifecycle boundaries.
- [`desktop-inspection-surface.md`](desktop-inspection-surface.md): accepted
  pivot to a JSTorrent-derived detailed desktop/web product and debugging
  surface, an intentional Android presentation split, and the API questions
  that remain open before implementation.
- [`application-interface-direction.md`](application-interface-direction.md):
  accepted Library, Transfers, and Workbench product destinations, contextual
  sidebar and inspector roles, media-library boundaries, and the preserved
  traditional advanced interface.
- [`web-ui-design.md`](web-ui-design.md): fresh React and CSS Modules web
  presentation, JSTorrent-inspired information hierarchy, category layer,
  Zustand state ownership, adaptive master/detail navigation, accessibility,
  and virtualized scale direction.
- [`table-interaction.md`](table-interaction.md): shared active-row,
  batch-selection, keyboard-focus, master/detail, range, and select-all
  behavior for actionable virtual tables.
- [`disk-and-piece-inspection.md`](disk-and-piece-inspection.md): global
  storage-pipeline pressure and piece-level work inspection together with the
  selected-torrent compact Canvas piece overview.
- [`peer-lifecycle.md`](peer-lifecycle.md): peer observations, bounded records,
  derived dial eligibility, connection attempts, adversarial multi-peer
  ownership, slot replacement, and the discovery-to-swarm boundary.
- [`utp-transport-campaign.md`](utp-transport-campaign.md): adaptive BEP 29
  investigation and implementation campaign, source/license choices, UDP and
  peer-owner boundaries, evidence ladder, WAN strategy, and human review gates.
- [`incoming-reachability-and-seeding.md`](incoming-reachability-and-seeding.md):
  campaign direction from a session-owned peer listener through verified
  upload, truthful advertisement, gateway mapping, settings, product status,
  and external reachability evidence.
- [`tracker-discovery.md`](tracker-discovery.md): tracker URL and announce
  lifecycle, bounded results, scheduling direction, and the
  tracker-to-peer-observation boundary.
