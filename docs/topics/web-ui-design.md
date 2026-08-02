# Web UI Design

Topic: `web-ui-design`

Status: Product, presentation, application-view, and client-store direction
accepted. Tactical `033` implements the generated contract, polling client,
lifecycle controller, and pure reducer. Tactical `034` implements the fresh
React/Zustand/CSS Modules application, adaptive inspection hierarchy, virtual
tables, and permanent named demo adapter. Tactical `035` connects stable Rust
torrent and active-peer projections through semantic responsive view
selection while retaining the demo adapter and recovering cleanly after
browser suspension. Tactical `036` adds the production-built manual live
browser launcher. Tactical `037` adds accessible, responsive magnet intake to
the live toolbar and routes it through the semantic application command while
retaining the permanent demo adapter. Tactical `038` adds a keyboard, pointer,
touch, and phone-safe More > Add test torrent submenu backed by the recorded
WebTorrent catalog and the same add path. Tactical `040` adds live archive and
an accessible removal dialog whose managed-data option is unchecked by
default. The docked detail inspector is now bounded and resizable by pointer,
touch, or keyboard. Tactical `041` adds the first live Files surface, exact
sorting, persistent table columns and widths, and a 4,096-row named scenario.
Tactical `043` adds the responsive live Trackers table, local deadline
countdowns, and a permanent tracker-recovery scenario. The Tauri entry remains
legacy.

## Purpose

The shared browser and Tauri presentation should become RSTorrent's detailed
product and inspection surface without inheriting JSTorrent's frontend
architecture. It must remain useful as a dense desktop torrent client while
also adapting gracefully to small desktop windows, touch input, tablets, and
phone-sized browser viewports.

This topic owns the web presentation technology, information hierarchy,
adaptive navigation, styling, accessibility, and performance direction. The
broader reason for prioritizing the surface remains in
[`desktop-inspection-surface.md`](desktop-inspection-surface.md). The accepted
semantic application-view, view-set, polling, streaming, codec, and generated
contract direction lives in
[`application-view-api.md`](application-view-api.md).

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

Use the stable Zustand v5 release available when the implementation tactical
opens and pin the exact version in the lockfile. Use its vanilla store as the
application-state owner and its React bindings for narrow selectors. No
routing, design-system, or data-grid library is selected by this topic. Each
dependency must solve a concrete need and preserve strict typing, headless
testability, accessibility, and bounded rendering.

## Information Hierarchy

Preserve JSTorrent's recognizable torrent detail hierarchy: overview,
trackers, peers, swarm, files, and pieces remain torrent-scoped views. Disk is
a session concern because its pressure and scheduling capacity are shared even
when rows retain torrent attribution. Logs, transfer speeds, DHT, Disk, search,
settings, and other session or product concerns use an identifiable right-side
global group. The accepted storage and piece split lives in
[`disk-and-piece-inspection`](disk-and-piece-inspection.md).

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

The implemented horizontal separator retains its size in per-application
presentation state, exposes range semantics to assistive technology, supports
Up/Down and Home/End keys, and accepts pointer or touch dragging. Its bounded
25--80% range protects useful space for both collection and detail content.
The preference is not yet persisted across application reloads.

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

Torrent-detail tab selection is a paint-only state change. Labels retain the
same font metrics, the underline is out of layout flow, and bounded count
badges keep fixed inline geometry. Peers and configured-tracker counts come
from the selected torrent summary, not the detail collection that is requested
only while its tab is visible. Selecting or evicting a detail view therefore
cannot move neighboring tabs or make its navigation count appear only while
selected.

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

The first application-view delivery uses bounded periodic JSON snapshots and
keyed diffs through one leased view set. The UI architecture must not depend on
full-state replacement or broad React context updates. A later low-latency
latest-state stream may coalesce changes and paint them on animation frames
without changing view semantics. Binary encoding is a separable measured codec
optimization, not automatically a new application API.

Ordered events, diagnostics, command results, and errors must not silently use
latest-value conflation intended for current state. The exact delivery and
recovery rules live in
[`application-view-api.md`](application-view-api.md).

## Zustand Store And Controller

Create one Zustand vanilla store per web application instance and provide it
to React through context plus typed `useStore` selectors. Do not use a module
global hook as the application authority. Per-instance construction supports
isolated tests, more than one client in a process, and explicit installation of
the transport and platform adapters.

The store is a materialized local copy of the named Rust views currently of
interest, not a complete engine mirror. Keep projection state keyed by the
client's stable `view_id`; normalize large collection views into ordered IDs
and rows keyed by stable identity. Do not force partial torrent-list,
torrent-detail, peer, and other projections into one universal entity whose
field presence becomes ambiguous.

`InspectionApplication` exposes semantic desired views rather than generated
Rust `ViewSpec` values. The live adapter maps them to one leased Rust view set;
the demo adapter obeys the same request and eviction behavior. Responsive
navigation determines the set: a phone library retains the torrent list, a
phone detail retains the selected summary and active detail only, and a wide
split may retain list, summary, and peers together. The torrent list is not a
mandatory global replica.

Each materialized view distinguishes not requested, loading, ready,
unavailable, unsupported, and stale. Within ready values, `null` means a
supported field currently has no value; missing required fields are validation
or programming failures. Transport recovery may retain prior values visibly
as stale, but a fresh view-set snapshot atomically replaces them before new
patches apply.

Keep the layers explicit:

```text
ApplicationClient
        |
ViewController
  view-set ID, cursor, polling/stream task, retry and cancellation
        |
validated UpdateBatch
        |
pure applyUpdateBatch reducer
        |
Zustand vanilla store
        |
React selectors and virtualized components
```

Sockets, Tauri Channels, promises, polling loops, abort controllers, and task
handles do not live in Zustand. One `ViewController` owns those lifecycles and
applies each valid update batch as one atomic store operation. Snapshot and
patch reduction remains pure TypeScript so it can be tested without React or
Zustand and reused by a headless CLI.

Reducers preserve references for unchanged view containers and rows. A
virtualized table selects ordered row IDs while each visible row selects its
own value, preventing an unrelated row update from rerendering every visible
row. Compound selectors use shallow equality only where needed; broad whole-
store subscriptions are not the default.

Do not persist the materialized engine replica. A reload recovers from fresh
Rust snapshots. Persist only explicit presentation preferences such as theme,
density, columns, and layout. Avoid Immer initially so patch behavior and
structural sharing remain visible. Development tooling may expose one named
action per update batch, but high-frequency row changes must not flood a
production store or an unbounded devtools history.

Zustand and immutable keyed containers are the initial simplicity choice, not
a performance claim. Synthetic scale tests measure cloning, reduction,
selector notification, rendering, and memory before introducing entity
sharding, mutable versioned containers, or another store abstraction.

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

Tactical `034` establishes the first concrete evidence: deterministic wide,
compact, and phone screenshots, keyboard and pointer browser paths, serious
and critical axe checks, and bounded rendering of 2,000 torrents plus 10,000
peers. The sampled large scenario retained 840 total DOM elements, used about
29.3 MiB of JavaScript heap, rendered initially in 247 ms, applied a simulated
ten-second update in 50 ms, and recorded no browser long tasks. These numbers
are development smoke evidence rather than general performance guarantees.

Synthetic scale fixtures should cover thousands of torrents, peers, and files
without public network traffic. Record visible-row count, update and render
latency, long-task or missed-frame evidence, and memory high-water when a slice
changes table or store behavior. Real torrents remain useful spot checks after
deterministic presentation evidence passes.

Tactical `035` adds the first live-product evidence through the same
components. A production Vite build in headless Chrome observed a loopback
libtorrent peer from active work through verified completion and keyed row
removal at wide, compact, and phone viewports. It also held browser update
operations beyond a shortened server lease: the header exposed reconnecting,
the peer row remained visibly stale, and a fresh view-set snapshot restored
connected state. Serious and critical axe findings were empty. The demo scale
measurements above remain the larger rendering-pressure evidence; one live
peer is interoperability evidence, not a scale profile.

Tactical `040` preserves the same independent frontend path for lifecycle
controls. The demo adapter deterministically archives and removes rows; the
live adapter maps those intents to generated Rust commands. Removal restores
focus after Escape or completion, retains the dialog and error after command
failure, and displays an irreversible warning only when managed-data deletion
is selected.

Tactical `041` makes Files a live responsive detail rather than a placeholder.
The default table shows Name, Folder, Normal/Skip selection, Size, Progress,
Done, and Verified; Type, Index, torrent offset, piece span, and absolute
Storage Path are optional columns. Padding remains available in the typed
projection but is hidden with an explicit count. Exact decimal counters use
`BigInt` comparison, null sorts last in both directions, zero remains visible,
and semantic enum order is explicit. Live update sorting is opt-in so changing
rates or progress do not make rows jump while being inspected.

Column visibility, widths, sort, and live-sort preference persist per table in
a versioned browser-local setting. Resize separators work by pointer and
keyboard. Wide, compact, and phone evidence keeps the active tab visible,
closes the phone drawer fully, and leaves horizontal scrolling available for
explicit extra columns. Serious and critical axe findings are empty.

The deterministic 4,096-row scenario hides one padding row and rendered 690
DOM elements with 66,468,705 bytes of sampled JavaScript heap. A complete
ten-second scenario update and paint took 55 ms. Its hash-failure timeline
regresses only unverified Done bytes while Verified remains monotonic, then
recovers. These are bounded development observations, not browser-wide
performance guarantees.

Tactical `042` removes the live adapter's permanent hash-only label. Before
metadata, the library and General view retain `Torrent <hash-prefix>`; after
verified metainfo is durably recorded, the shared torrent row automatically
uses its bounded name. No view selection, table identity, or local preference
changes during that transition.

The detail-tab regression suite records each tab's layout offset and width
while selecting every view. It also keeps peer and tracker badges visible from
the stable summary throughout those view-set changes; narrow layouts may
scroll to reveal a clipped active tab, but the tab boxes themselves do not
reflow.

## Likely Sequencing

1. The accepted view-set contract, generated TypeScript/schema, polling
   client, and pure reducer described in
   [`application-view-api.md`](application-view-api.md) are implemented and
   headlessly validated by Tactical `033`.
2. The React/CSS Modules shell, adaptive navigation, Zustand/virtual-table
   foundation, deterministic named demo adapter, and accessibility baseline
   are complete in Tactical `034`.
3. Tactical `035` defines stable Rust torrent and active-peer inspection
   projections, self-expiring leases, semantic view selection, suspension
   recovery, and the live frontend adapter. This step is complete.
4. Files is complete in Tactical `041`, and the schedule-backed Trackers view
   is complete in Tactical `043`. Integrate the existing categorized logger
   into the global diagnostics area or add the registry-backed Swarm view
   according to immediate debugging value; Peers, Files, and Trackers are the
   first detailed live engine views.
5. Connect remaining detail views according to debugging and product value,
   keeping unsupported scaffolds truthful.
6. Measure before adding frame-speed streaming, binary encoding, or more
   specialized rendering paths.

This is continuing direction, not an active tactical or permission to build
all stages as one slice.

## Deliberately Open Decisions

- the default wide-screen detail position and user-selectable layout modes;
- inbox, category, label, and queue semantics; archive and removal retention
  semantics are implemented by Tactical `040`;
- whether later navigation complexity justifies a router or icon dependency;
- exact columns, row-detail interactions, table customization, and density
  presets;
- how session-scoped diagnostics coexist visually with torrent-scoped tabs;
- the first supported browser and phone access posture beyond headless and
  local desktop use; and
- the thresholds and transport shape that would justify low-latency streaming
  or a binary codec.

Tactical `033` completed the headless transport-independent view-set
foundation. Tactical `034` completed the first visible React and demo-scenario
foundation. Tactical `035` completed the live torrent/peer adapter, exact
active-row membership, local endpoint posture, semantic view selection, and
suspension recovery. The implemented view-set limits and remaining delivery
choices are recorded in
[`application-view-api.md`](application-view-api.md).
