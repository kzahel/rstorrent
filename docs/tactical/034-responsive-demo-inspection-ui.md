# Tactical 034: Responsive Demo Inspection UI

Status: active.

## Motivation And Desired Outcome

Tactical `033` proves the application view-set and TypeScript polling
boundary, but the visible web client is still a provisional direct-DOM proof.
The next inspection surface should be designed, tested, and discussed as a
real frontend before new Rust peer fields dictate its component structure.

Build a fresh React inspection application behind an explicit named-demo URL.
It consumes a deterministic application adapter rather than engine objects,
can simulate changing torrents and peers, and remains fully drivable in a
headless browser. The result should let maintainers request a precise scenario,
reproduce the same state, and exchange wide, compact, and phone screenshots
without launching Tauri or touching the network.

The existing Tauri and WebSocket product proof remains the default non-demo
entry point during this slice. Connecting the new surface to Rust is a later
tactical rather than an implicit partial migration.

## Dependencies And References

- [`../topics/web-ui-design.md`](../topics/web-ui-design.md)
- [`../topics/desktop-inspection-surface.md`](../topics/desktop-inspection-surface.md)
- [`../topics/application-view-api.md`](../topics/application-view-api.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../engineering-principles.md`](../engineering-principles.md)
- [`033-headless-view-set-foundation.md`](033-headless-view-set-foundation.md)
- JSTorrent product reference at sibling commit
  `9895410beeed6aff554053769bd006a3fbd373ef`, especially
  `packages/client/src/AppContent.tsx`,
  `packages/ui/src/components/DetailPane.tsx`, and its torrent, peer, and
  virtual-table components.

The JSTorrent reference supplies terminology, dense list/detail hierarchy,
tab inventory, and useful peer columns. Its React/Solid bridge, mutable engine
objects, inline-style architecture, frame polling, and table implementation
are not copied.

The package versions selected when this tactical opened are React and
React DOM `19.2.8`, Zustand `5.0.14`, Testing Library React `16.3.2`,
Testing Library user-event `14.6.1`, jsdom `30.0.1`, Playwright `1.62.1`, and
axe-playwright `4.12.1`. Runtime packages are MIT. Playwright is Apache-2.0;
the dev-only axe integration is MPL-2.0. No source, asset, or fixture is
imported from these references.

No BitTorrent protocol or engine transition changes in this tactical, so no
BEP or pinned libtorrent source dossier applies.

## Scope

### Fresh application and presentation boundary

Add a strict-TypeScript React entry with:

- one Zustand vanilla store per application instance;
- a typed frontend inspection model independent from generated Rust DTOs;
- an `InspectionApplication` port for snapshots, keyed patches, commands, and
  lifecycle;
- one controller that applies adapter updates atomically and owns adapter
  termination outside Zustand;
- React context and narrow selectors rather than a module-global store;
- CSS Modules for component layout and states, with one global token and
  normalization sheet; and
- an explicit demo query route while the existing client remains the default
  for Tauri and non-demo browser use.

### Responsive inspection hierarchy

Implement one adaptive application/session, category, torrent-list,
selected-torrent, and detail hierarchy:

- wide view: persistent category sidebar, torrent table, and docked detail;
- compact view: collapsible category navigation and a useful list/detail
  split;
- phone view: torrent list and full-screen detail with a clear back action;
- visible global transfer status and demo identity;
- torrent actions implemented truthfully by the demo adapter; and
- General, Trackers, Peers, Swarm, Files, Pieces, Disk, Logs, Speed, and DHT
  tabs, with Peers, General, and Logs receiving the first useful data and
  other tabs reporting explicit demo/unavailable state.

Navigation, selection, sort, and scenario state must remain understandable
under keyboard and touch input. The app does not need a router dependency for
this first local query-selected mode.

### Lazy table foundation

Add an independently authored fixed-row virtual grid suitable for torrents
and peers. It must provide:

- stable row identity;
- sortable semantic column buttons and announced sort direction;
- roving keyboard focus, arrow/Home/End navigation, and Enter/Space
  selection;
- horizontal scrolling without allocating offscreen cells;
- bounded overscan and DOM row count independent from collection size;
- row and column counts for assistive technology; and
- deterministic scale evidence with 2,000 torrents and 10,000 peers.

Column resizing, reordering, persistence, variable-height rows, and a generic
data-grid framework remain later measured work.

### Named deterministic demo scenarios

Provide a reusable catalog with stable identifiers, generated data, a
controllable monotonic clock, and URL selection:

- `healthy-download`: metadata, transfer, and completion progression;
- `stalled-metadata`: candidates exist but metadata does not advance;
- `tracker-recovery`: an announce failure, scheduled retry, and recovery;
- `endgame`: near-complete transfer, duplicate requests, and completion;
- `large-swarm`: 2,000 torrents and 10,000 peers for store/table pressure;
- `disk-error`: a storage failure and actionable stopped state; and
- `empty-library`: a truthful empty product state.

Demo mode supports play/pause, bounded time advance, reset, scenario switch,
torrent pause/resume, archive/unarchive, and adding one generated demo
transfer. Scenario state is presentation evidence, not a claim about torrent
engine policy or correctness.

Headless tests may freeze the clock and seek to a named offset through query
parameters so screenshots and assertions are repeatable. Ordinary demo mode
may run the same clock automatically.

## Owner, Task, Cancellation, And Data-Flow Map

```text
named ScenarioDefinition (pure deterministic data)
               |
               v
       DemoApplication
  clock + command overlays + update emission
               |
               v
      InspectionController
  one subscription + explicit close
               |
               v
     Zustand vanilla store
 normalized rows + presentation state
               |
               v
 React selectors -> responsive shell / virtual grids
```

`ScenarioDefinition` and frontend reducers remain independent from React,
DOM, timers, Tauri, sockets, and generated API DTOs. `DemoApplication` owns at
most one interval and clears it on close. `InspectionController` owns one
adapter subscription and closes it during React teardown. Zustand contains no
timer, abort controller, promise, socket, or task handle.

The later Rust adapter will translate validated view-set snapshots and patches
into the same frontend inspection updates. It does not require components to
consume transport DTOs directly.

## Initial Resource Bounds

- one demo clock interval per application, no faster than 250 milliseconds;
- one atomic frontend update per demo tick;
- at most 256 retained demo log rows, with an explicit dropped count;
- at most 2,000 torrent rows and 10,000 peer rows in retained named scenarios;
- fixed table row height and 8-row overscan on each side;
- no more than 100 rendered body rows per virtual grid at supported test
  viewports;
- URL scenario IDs and numeric offsets validated against the finite catalog
  and a bounded 24-hour scenario clock; and
- demo command strings and generated labels bounded in the frontend model.

## Shape-Changing And Adversarial Cases

The common path includes:

- empty, active, complete, paused, archived, and error torrent states;
- metadata without a known content size;
- zero rates and unavailable ETA versus actual numeric zero;
- peer connecting, useful, choked, stalled, and disconnected lifecycles;
- stable connection IDs and a reconnect represented as a distinct row;
- torrent and peer keyed upserts plus explicit removals;
- selected torrent removed or excluded by the active category;
- scenario reset while a detail row is selected;
- long names, client names, endpoints, and diagnostic summaries without
  unbounded layout growth;
- thousands of rows with a bounded DOM;
- reduced-motion, zoom-friendly sizing, narrow width, coarse-pointer
  operation, and keyboard-only navigation; and
- explicit unsupported/unavailable detail states rather than fabricated
  empty engine views.

## Staged Implementation And Gates

### Stage 1: application model and demo owner

Add pure models, normalized reducer/store, controller, named scenarios, clock,
and command behavior.

Gate: deterministic tests cover scenario identity, clock seeking, completion,
tracker recovery, command overlays, keyed removal/reconnect, scale counts,
store reference preservation, and exact shutdown.

### Stage 2: React shell and tables

Add the adaptive category/list/detail shell, CSS Modules, virtual torrent and
peer tables, first useful General and Logs surfaces, and explicit remaining
tabs.

Gate: jsdom component tests cover category and torrent selection, tab and
back navigation, sort and keyboard operation, scenario controls, empty/error
states, and bounded rendered rows for the scale scenario.

### Stage 3: headless browser evidence

Drive frozen named scenarios through the built Vite application in headless
Chrome. Capture wide, compact, and phone screenshots, exercise pointer and
keyboard paths, and run an automated accessibility scan.

Gate: the production build passes; browser tests find no serious or critical
accessibility violations; screenshots are visually inspected; and the app
does not launch Tauri, use the public Internet, or bind an application server
beyond the temporary Vite test host.

### Stage 4: documentation and regression

Record the actual dependency inventory, named scenarios, measurements,
screenshots, known limitations, and next adapter boundary. Keep the legacy
client, generated API, reducers, and controller tests green.

## Validation Matrix

```bash
npm ci --prefix clients/web
npm run typecheck --prefix clients/web
npm test --prefix clients/web
npm run build --prefix clients/web
npm run test:e2e --prefix clients/web

source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

The full Rust gates protect the unchanged embedded client and generated
application boundary. No controlled libtorrent run, public swarm, Android
build, emulator, physical device, visible browser, or visible Tauri client is
required or authorized.

## Invariants

- Components never import generated Rust DTOs, Tauri globals, WebSocket
  frames, or engine objects.
- Demo and later real adapters terminate at one frontend inspection model.
- Demo data is explicitly labeled and cannot be confused with engine truth.
- Demo progression is deterministic from scenario, clock, and command state.
- Scenario simulation represents observable state and does not implement
  BitTorrent scheduling or integrity policy.
- Large logical collections do not create proportional DOM collections.
- Empty, unsupported, stale, and error states remain distinguishable.
- React owns presentation composition, not adapter tasks or application truth.
- The existing live client remains available and unchanged outside the
  explicit demo route during this tactical.
- Routine evidence remains headless and does not disturb the user's desktop.

## Non-Goals

- stable Rust torrent or peer projection fields;
- connecting React/Zustand to `ViewController`, HTTP polling, Tauri commands,
  or Channels;
- replacing or deleting the existing direct-DOM product proof;
- Android UI changes;
- archive, removal, labels, queueing, or file-priority product semantics;
- production remote access, authentication, routing, or deployment;
- streaming, animation-frame batching, CBOR, or other transport optimization;
- general table column customization or a third-party data-grid dependency;
- exact visual reproduction of JSTorrent; and
- engine, protocol, discovery, storage, persistence, or performance changes.

## Escalation Contract

Proceed without routine approval for the recorded frontend and dev-test
dependencies after license review, internal TypeScript/CSS refactoring,
fixture generation, temporary loopback Vite hosting, headless Chrome, captured
screenshots, and bounded commits.

Stop for direction if the work requires replacing the live client by default,
changing generated Rust API semantics, adding a production listener or remote
security model, persisting new product state, launching a visible client,
modifying Android, copying reference source/assets, or introducing a table or
design framework with a broader product contract.

## Stopping Condition

This tactical is complete when the named demo URLs render a production-quality
adaptive inspection shell, deterministic scenarios and commands are drivable
through one frontend application port, 2,000 torrents and 10,000 peers retain
a bounded virtual DOM, unit/component/browser/accessibility gates pass, wide,
compact, and phone screenshots have been inspected and shared, the existing
live entry remains green, owning topics record the new truth and adapter gap,
and the working tree is committed and clean.

The next slice should connect stable Rust peer and torrent projections to the
frontend inspection model through the implemented view-set controller while
keeping the demo adapter as a permanent development and reproduction tool.
