# Desktop Inspection Surface

Topic: `desktop-inspection-surface`

Status: Strategic direction and application-view architecture accepted. The
headless view-set, polling client, generated contract, and pure reducer are
implemented by Tactical `033`. Tactical `034` implements the fresh responsive
frontend, virtual torrent and peer tables, and named deterministic demo
application. Tactical `035` completes the first live bounded cross-section:
truthful torrent summaries and active peers flow from one coherent engine
observation through leased semantic views into the responsive React surface,
with headless recovery and controlled libtorrent evidence. Tactical `036`
adds the one-command production browser launcher needed for maintainer visual
confirmation before the Tauri entry changes.

## Purpose

RSTorrent now has a roughly functional downloader, structured diagnostics,
headless interoperability harnesses, and enough live behavior to expose
correctness and performance problems. The current development loop is still
too indirect: bounded runs and terminal reports are useful evidence, but they
do not replace watching a torrent evolve and building a mental model from its
peers, requests, pieces, discovery, storage, and logs in real time.

Detailed torrent-client views are not merely internal tooling. Experienced
users expect peer, tracker, piece, file, and transfer details, and those views
also form the primary debugging surface for maintainers. The desktop product
should embrace that dual role.

This topic owns that strategic direction. The API and client-state owners are
linked above; this topic does not duplicate their schema, sampling policy,
table definition, or implementation tactical.

## Direction

Pause the engine completeness and libtorrent-parity campaign after completed
Tactical `032`. Preserve its evidence and restart point, but do not open its
next engine tactical while the desktop inspection surface is being discussed
and established.

Use the existing JSTorrent desktop interface as the primary product and
interaction reference. The fresh React presentation described in
[`web-ui-design.md`](web-ui-design.md) preserves its recognizable information
hierarchy and detailed views while adding category navigation, adaptive
master/detail presentation, touch usability, and an accessibility baseline.
Views may initially contain truthful unavailable states while their RSTorrent
data feeds do not exist.

The existing categorized logger naturally belongs in the logging view. The
peer view is the first new live inspection priority because it gives a direct
picture of connection utility, request ownership, choke state, throughput,
and churn during ordinary interactive testing. Other detailed views should be
connected according to observed debugging and product value rather than a
precommitted completeness order.

Desktop and browser-hosted web builds continue to share one web presentation.
The Tauri application remains the ordinary interactive desktop product, while
the authenticated loopback gateway remains the headless automation seam for
the same components. This direction does not make a local socket service or
remote daemon part of the desktop architecture.

During the transition, `./scripts/webui` is the explicit manual host for the
new live React surface and `./scripts/desktop` still hosts the legacy surface.
Once the maintainer confirms the browser path, the next bounded client slice
should adapt Tauri's in-process commands/views to `InspectionApplication`
rather than make the gateway a desktop dependency. The following inspection
slice should prioritize the categorized Logs experience, studying JSTorrent's
tab while deciding which existing legacy presentation behavior is worth
migrating rather than copying wholesale.

## Platform Split

Desktop/web and Android presentation are allowed to diverge from this point.
The desktop/web product is the dense, detailed inspection surface. Android
continues as a platform-appropriate Compose product and is not required to
mirror desktop tabs, tables, or diagnostic density.

The shared Rust engine, application semantics, integrity rules, and durable
session state remain common. A desktop inspection capability does not imply an
Android screen, and an Android lifecycle or platform feature does not imply a
desktop interaction. Cross-platform presentation parity is now an explicit
choice for each product feature rather than a default requirement.

The initial desktop inspection work should not modify the Android UI. Android
remains supported; it is intentionally behind the desktop inspection surface
while that surface becomes useful for engine development.

## JSTorrent Reference Use

JSTorrent is a first-party product-history and interface reference. Reuse its
overall information architecture, layout lessons, interaction patterns,
visual language, and terminology through a fresh implementation. Do not
transplant its mixed React/Solid component tree or silently import the old
engine state model, controller topology, transport assumptions, or stringly
typed runtime coupling.

Any source or assets copied into this repository must still record the exact
origin and revision, preserve applicable license and attribution coverage, and
identify intentional adaptation. The reference supplies a proven product
surface, not the RSTorrent application-view architecture.

## Principles

- The inspection surface is a real product surface, not a hidden developer
  console that may ignore lifecycle, bounds, accessibility, or correctness.
- A view must distinguish genuinely empty state from unavailable,
  unsupported, disconnected, stale, or overflowed data.
- Structured state, commands, events, and logs retain separate meanings. UI
  code must not scrape log strings to infer engine state or control behavior.
- High-detail observation must remain bounded and must not put peer payloads,
  piece payloads, or storage buffers on the application boundary.
- Existing torrent add, control, persistence, and lifecycle behavior must
  remain usable while the detailed shell is introduced.
- The shared web application remains headlessly exercisable without launching
  or focusing the Tauri window. Interactive desktop testing and automated
  validation use different hosts for the same presentation.
- Empty scaffolding is acceptable when it is honest. Controls that cannot act
  must not appear to succeed.

## Deliberately Open Design

The view-set, snapshot/diff, polling-to-streaming, generated-type, and Zustand
architecture is accepted. Tactical `035` now implements the first live torrent
and active-peer field set, Peers-versus-Swarm membership, and local endpoint
privacy posture. The remaining inspection design includes:

- the division between later swarm, piece, tracker, file, storage,
  protocol-message, and history views;
- sorting, filtering, selection, and row-detail semantics beyond the first
  active-peer table;
- concrete update cadence, history retention, overflow, and memory bounds;
- which endpoint, peer-client, protocol, and failure details are appropriate
  for local display or exported diagnostics;
- the exact JSTorrent revision, components, tab inventory, columns, actions,
  or visual adaptations to reuse; or
- whether a future remote-control product consumes the same detailed views.

Those choices should follow inspection of the current JSTorrent UI, the
existing RSTorrent application and diagnostic contracts, and concrete
interactive debugging needs. They should not be inferred from the public
comparator schema or prematurely fixed by this strategy document.

## Relationship To Engine Work

The oracle-driven campaign is paused, not abandoned. Its comparator, source
discipline, correctness ledger, and retained storage bottleneck remain valid.
Once the inspection surface supports useful real-time observation, maintainers
can decide whether to resume the recorded storage-execution candidate, select
a different engine owner from interactive evidence, or fill another detail
view first.

Tactical `033` completed the bounded view-set contract and headless client
foundation described in
[`application-view-api.md`](application-view-api.md). Tactical `034` built
the responsive presentation and permanent named scenario adapter first, so
the stable peer projection and its adversarial fixtures can connect to a
tested frontend model rather than defining component architecture indirectly.
Tactical `035` now completes that connection. The production React build is
driven headlessly against a controlled peer, displays active request state,
recovers after its old server view expires, verifies the downloaded payload,
and removes the connection row after joined cleanup without launching Tauri or
a visible browser.

The surface is therefore useful for live peer observation, but it is not yet a
complete debugging console. The existing categorized diagnostics feed is the
broadest next view candidate; a registry-backed Swarm table is the deeper
peer-lifecycle candidate. Selection remains an explicit next tactical based on
real inspection use.
