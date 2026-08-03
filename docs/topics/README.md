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

- [`product-direction.md`](product-direction.md): initial product motivation,
  first-party Rust engine decision, platform posture, non-goals, open choices,
  and recommended bring-up sequence.
- [`product-surfaces-and-migration.md`](product-surfaces-and-migration.md):
  backend/presentation separation, desktop extension use, ChromeOS Android and
  Crostini choices, launch handoff, backend isolation, and manual JSTorrent
  import.
- [`capability-readiness.md`](capability-readiness.md): master engine and product
  scoreboard, evidence vocabulary, priority policy, and bounded current queue.
- [`download-correctness.md`](download-correctness.md): completion, integrity,
  request ownership, recovery invariants, observed incidents, and executable
  scenario ledger.
- [`protocol-support.md`](protocol-support.md): precise BEP support claims,
  deliberate limits, interoperability evidence, and protocol sequencing.
- [`dht-discovery.md`](dht-discovery.md): integrated session-owned IPv4 DHT
  routing, lookup, private-torrent policy, bounded warm restart, evidence, and
  named address-family and participation gaps.
- [`performance-and-live-evidence.md`](performance-and-live-evidence.md):
  headless libtorrent comparison, public-smoke classification, performance
  measurement, and artifact-safety policy.
- [`storage-throughput-architecture.md`](storage-throughput-architecture.md):
  proposed maximum-throughput receive-to-storage pipeline, positional I/O,
  write/hash joins, part-file coordination, batched durability, and
  session/root scheduling.
- [`oracle-driven-engine-campaign.md`](oracle-driven-engine-campaign.md):
  source-first libtorrent-oracle runbook, parity gates, autonomous restart
  checkpoint, milestone sequence, and transition to measured BEP breadth.
- [`client-persistence.md`](client-persistence.md): SQLite-backed client state,
  verified metadata and resume invariants, cross-platform storage-root
  identity, and the application-service boundary above the torrent engine.
- [`application-control.md`](application-control.md): shared semantic commands,
  responses, snapshots, revisions, and the boundary between in-process
  application control and future transports.
- [`application-view-api.md`](application-view-api.md): leased view sets,
  named snapshots and diffs, cursor recovery, polling and streaming delivery,
  generated TypeScript/schema, and provisional remote routes.
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
- [`disk-and-piece-inspection.md`](disk-and-piece-inspection.md): global
  storage-pipeline pressure and piece-level work inspection together with the
  selected-torrent compact Canvas piece overview.
- [`peer-lifecycle.md`](peer-lifecycle.md): peer observations, bounded records,
  derived dial eligibility, connection attempts, adversarial multi-peer
  ownership, slot replacement, and the discovery-to-swarm boundary.
- [`tracker-discovery.md`](tracker-discovery.md): tracker URL and announce
  lifecycle, bounded results, scheduling direction, and the
  tracker-to-peer-observation boundary.
