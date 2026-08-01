# Web UI Design

Topic: `web-ui-design`

Status: Product and presentation direction accepted. The application-view API,
exact library inventory, and first implementation tactical remain pending
maintainer discussion.

## Purpose

The shared browser and Tauri presentation should become RSTorrent's detailed
product and inspection surface without inheriting JSTorrent's frontend
architecture. It must remain useful as a dense desktop torrent client while
also adapting gracefully to small desktop windows, touch input, tablets, and
phone-sized browser viewports.

This topic owns the web presentation technology, information hierarchy,
adaptive navigation, styling, accessibility, and performance direction. The
broader reason for prioritizing the surface remains in
[`desktop-inspection-surface.md`](desktop-inspection-surface.md). The semantic
application-view and transport contract remains deliberately open.

## Fresh Frontend

Build a fresh frontend rather than transplanting the current JSTorrent React
and Solid component tree. JSTorrent remains the primary product reference for
visual density, terminology, familiar actions, information hierarchy, and the
set of detailed views. Its mutable engine objects, mixed-framework rendering,
controller topology, and transport assumptions are not compatibility targets.

Use the stable React `Latest` release available when the implementation
tactical opens. React does not publish a conventional LTS channel. Pin the
selected stable release in the lockfile and do not adopt Canary, Experimental,
server components, or a full-stack framework without a demonstrated product
need. The shared client remains a strict-TypeScript, client-rendered Vite
application.

Component styling uses CSS Modules. Global CSS is limited to normalization,
design tokens, typography defaults, theme variables, and other genuinely
application-wide rules. Layout and component states belong in colocated
`*.module.css` files. Inline styles are reserved for measured or data-driven
geometry such as virtual-row transforms and column widths, preferably through
CSS custom properties.

No general state, routing, design-system, or data-grid library is selected by
this topic. Each dependency must solve a concrete need and preserve strict
typing, headless testability, accessibility, and bounded rendering.

## Information Hierarchy

Preserve JSTorrent's recognizable torrent detail hierarchy: overview,
trackers, peers, swarm, files, pieces, and disk activity remain torrent-scoped
views, while logs, transfer speeds, DHT, search, settings, and other session or
product concerns retain an identifiable global scope.

Add a library and categorization level above the torrent list. This may expose
derived views such as all, active, downloading, seeding, completed, paused,
and errors together with durable organizational state such as archived
torrents and, later, user labels. The sidebar is one presentation of this
level, not the only way to reach it.

The resulting conceptual hierarchy is:

1. application and session;
2. library category or organizational view;
3. torrent collection and selection;
4. selected torrent and its detail views; and
5. progressive detail for a peer, file, tracker, piece, or other selected row
   when useful.

Archive is not deletion. Its exact relationship to pause, queue, content
retention, and the default library view remains an application-state decision.
Durable categories and labels belong above browser-local presentation state,
even when Android does not initially expose them.

## Adaptive Navigation And Layout

Use one navigation and view model across sizes rather than separate desktop
and mobile-web applications. Layout responds primarily to available space and
input capability, not a platform or user-agent label.

On a wide display, the category sidebar may remain visible and the torrent
list may coexist with a docked, resizable detail inspector. A selected torrent
may also enter a focused detail mode when the user wants more room for peers,
files, pieces, or another dense view.

As space narrows, the sidebar becomes collapsible, a rail, or a drawer. The
torrent list and detail inspector may stop sharing the viewport. At phone
widths, selecting a torrent navigates to a full-size detail view with a clear
back action. Returning to the list preserves useful context such as category,
filter, sort, selection, and scroll position.

The same torrent and detail components should serve docked and focused modes.
Navigation state must remain understandable and restorable rather than being
encoded only in incidental component state. The exact URL and routing scheme,
default wide-screen inspector placement, and whether users can choose between
bottom and side docking remain open.

Touch is a first-class input. Primary actions cannot require hover, right
click, or a fine pointer. Context menus may complement visible or otherwise
discoverable actions but cannot be their only access path. Density may adapt
without removing information or making controls too small to operate.

The responsive web application does not replace the native Android Compose
product or create automatic UI parity with it. Phone usability applies to the
shared web surface itself, including small desktop windows and future browser
access.

## Accessibility

Accessibility is an initial correctness requirement. The new implementation
must not reproduce JSTorrent's dependence on generic `div` grids, color-only
state, hover-only discovery, or incomplete focus semantics.

- Use semantic elements and appropriate grid, table, tab, navigation, dialog,
  status, and live-region semantics.
- Make all navigation, selection, sorting, resizing alternatives, menus, and
  torrent commands operable by keyboard.
- Preserve and restore focus across dialogs, responsive layout changes, and
  list-to-detail navigation.
- Expose selection, sort order, current tab, progress, stale or unavailable
  data, errors, and disabled actions to assistive technology.
- Maintain useful contrast in all themes, never rely on color alone, honor
  reduced-motion preferences, and remain usable under browser zoom and larger
  text.
- Provide touch targets and spacing appropriate to coarse pointers without
  forcing desktop users into one permanently oversized density.

Automated semantic and accessibility checks, keyboard-driven browser tests,
and manual screen-reader spot checks should accompany the surface as it grows.
Passing an automated checker alone is not sufficient evidence.

## State And Rendering Direction

React owns presentation composition and lifecycle, not engine state. Rust
application views terminate at a typed client-side store. Components consume
stable application view values rather than engine objects, Tauri globals,
WebSocket frames, or log text.

Large torrent, peer, file, tracker, piece, and log collections use lazy or
virtualized rendering. Only visible rows plus bounded overscan should own DOM
nodes. Stable row identity, narrow subscriptions, and collection revisions
should prevent unrelated application changes from rerendering every logical
row. Sorting, filtering, formatting, and selection must remain measurable and
bounded for thousands of torrents and much larger file collections.

The first useful application-view delivery may use bounded periodic JSON
snapshots and keyed diffs. The UI architecture must not depend on full-state
replacement or broad React context updates. A later low-latency latest-state
stream may coalesce changes and paint them on animation frames without
changing view semantics. Binary encoding is a separable measured codec
optimization, not automatically a new application API.

Ordered events, diagnostics, command results, and errors must not silently use
latest-value conflation intended for current state. The API discussion owns
their exact delivery and recovery rules.

## State Ownership

Browser-local presentation preferences may include sidebar visibility and
width, density, theme, column configuration, inspector size, dock or focus
preference, last selected tab, and preserved navigation context.

Archive state, user labels, and other organization intended to survive a new
webview or appear in another client are durable application data. Torrent
activity, queue state, storage, and content retention remain separate engine
or application concepts. The API design must make those distinctions explicit
before controls are implemented.

## Validation Direction

Routine validation uses the authenticated headless browser host and does not
launch or focus the Tauri application. Each meaningful presentation slice
should retain screenshots at representative wide, compact, and phone-sized
viewports and exercise pointer, keyboard, and touch-oriented paths where
applicable.

Synthetic scale fixtures should cover thousands of torrents, peers, and files
without public network traffic. Record visible-row count, update and render
latency, long-task or missed-frame evidence, and memory high-water when a slice
changes table or store behavior. Real torrents remain useful spot checks after
deterministic presentation evidence passes.

## Likely Sequencing

1. Inventory the JSTorrent views and classify their data as current state,
   durable application state, history, ordered events, diagnostics, or
   commands. Agree on the initial application-view API.
2. Establish the React/CSS Modules shell, adaptive navigation, fixture-backed
   category/list/detail hierarchy, and accessibility baseline.
3. Establish the virtualized table and client-store foundation under
   synthetic large collections, then connect the torrent list.
4. Make peers the first detailed live engine view and integrate the existing
   categorized logger into the global diagnostics area.
5. Connect remaining detail views according to debugging and product value,
   keeping unsupported scaffolds truthful.
6. Measure before adding frame-speed streaming, binary encoding, or more
   specialized rendering paths.

This is continuing direction, not an active tactical or permission to build
all stages as one slice.

## Deliberately Open Decisions

- the initial snapshot, diff, polling, subscription, and resynchronization
  contract;
- the default wide-screen detail position and user-selectable layout modes;
- archive, inbox, category, label, pause, queue, and content-retention
  semantics;
- the router, normalized-store, virtualization, accessibility-test, and icon
  dependencies;
- exact columns, row-detail interactions, table customization, and density
  presets;
- how session-scoped diagnostics coexist visually with torrent-scoped tabs;
- the first supported browser and phone access posture beyond headless and
  local desktop use; and
- the thresholds and transport shape that would justify low-latency streaming
  or a binary codec.

No implementation tactical is active. The next design discussion should
classify the initial torrent-list and peer-view data and choose the minimum
recoverable application-view contract that can serve both periodic delivery
and a later low-latency stream.
