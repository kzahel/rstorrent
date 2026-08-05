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
countdowns, and a permanent tracker-recovery scenario. Tacticals `044`--`045`
add the global Disk pipeline and selected-torrent bounded Canvas Pieces
overview, including responsive and large-torrent fixtures. Tactical `047` adds
the global Settings sheet and versioned Compact, Standard, and Spacious
interface sizes, makes readable Standard the default, and keeps virtual-table
geometry coherent with presentation sizing. Tactical `048` replaces the
legacy Tauri entry with the same React inspection application using
acknowledged in-process leased-view streaming. Tactical `060` now gives the
ordinary browser one multiplexed WebSocket for all calls and view sets,
retains HTTP only through the explicit `transport=http` loopback diagnostic
query, deletes the superseded direct-DOM gateway entry, and makes the modern
named demo the no-mode root. Tactical `049`
completes the accepted
global Logs design: a dedicated virtualized chronological console with
structured expansion, separate capture and display filters, bottom-follow/
new-record behavior, local clear, and no sorting or persistence. Tactical `050`
adds Auto, Light, and Dark to the shared Appearance settings, safely migrates
the size-only preference, and applies persisted theme before React content.
Tactical `051` replaces ambiguous peer flag strings with typed Rust semantics,
one exhaustive React glyph/label table, full cell accessibility, and a
separate sortable-header help control containing a compact sectioned
glyph/name legend without explanatory prose.
Tactical `056` completes the existing Peers Client column: the Rust
application projection supplies a bounded client/version label derived from
the handshake peer ID, while React retains only nullable display, title, and
sorting behavior and shows an em dash for unknown evidence.
Tactical `055` implements the accepted product information architecture:
responsive Library, Transfers, and Workbench destinations; contextual
sidebars; shared bounded multi-selection; a clean transfer queue; a truthful
torrent-backed content grid; and collection-only application-view leasing
outside Workbench. The detailed surface remains intact as Workbench, as
recorded in
[`application-interface-direction.md`](application-interface-direction.md).
Tactical `058` replaces persistent torrent checkboxes with a discoverable
selection mode, separates the current torrent from batch command targets, and
applies the same interaction locally to Files. It initially staged disabled
file actions without claiming runtime mutation support.
Tactical `059` restores a permanently reserved checkbox column specifically on
actionable tables and adds sorted Shift-range selection without recoupling the
current row and batch command set. Tactical `063` activates that Files-local
selection with exactly `Normal` and `Skip`, and keeps Add limited to root plus
one checked-by-default start-content option. The accepted successor interaction
contract now lives in [`table-interaction.md`](table-interaction.md): it names
the singular detail-owning row as active, names checked command targets as the
batch selection, makes row focus follow active navigation, and adds
Shift+Arrow plus platform select-all as pending implementation work.
Tactical `071` adds a presentation-only **Copy magnet link** More action for
exactly one selected torrent. It synthesizes the canonical v1 URI from the
already projected info hash, reports actual clipboard success or failure, and
does not claim byte-for-byte preservation of the submitted source URI.
Tactical `077` replaces the four component-local popup implementations with a
shared portalled overlay layer. File actions, torrent More and its submenu,
table Columns, and column help now share collision-aware positioning,
dismissal, focus restoration, keyboard semantics, layering, and responsive
bounds while retaining visible actions and existing targeting policy.
Tactical `083` makes empty Add synchronously open a hidden single-file
`.torrent` chooser in the same shared browser/Tauri toolbar. Nonempty input
retains magnet validation. The existing root/start dialog owns options while
the component retains only the browser `File`; bytes are read when the add
begins and no filename, path, digest, or progress percentage enters
presentation state.
Tactical `085` unifies selection-scoped toolbar, More, and row-context actions.
Transfers and Workbench expose full-selection Start, Pause, Force recheck,
Copy magnet links, Archive, Restore, and coordinated Remove; Files exposes the
same Normal/Skip policy from More and its row context menu. One application-
lifetime torrent owner keeps sequential progress and multi-remove state alive
across destination changes.

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

## Terminology

Unqualified **UI** and **web UI** mean this mature shared React product
application, whether it is hosted in a browser or embedded by Tauri. Tactical
`060` retired and removed the earlier direct-DOM proof, so it is not a second
current web UI. The platform-specific Compose presentation is named
explicitly as the **Android**, **Compose**, or **Android UI**. The Astro
`website/` tree is the project website, not this product surface.

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

Tactical `077` accepts exact `react-aria-components` `1.20.0` as the focused
unstyled overlay and menu dependency. Direct library composition remains
inside one local wrapper rather than becoming feature-component policy.
RSTorrent still owns styling, appearance tokens, labels, command eligibility,
and tests; React Aria owns the difficult portal, positioning, menu, submenu,
focus, and dismissal mechanics. Do not add a parallel overlay package or the
styled React Spectrum system without new measured evidence.

## Information Hierarchy

Preserve JSTorrent's recognizable torrent detail hierarchy: overview,
trackers, peers, swarm, files, and pieces remain torrent-scoped views. Disk is
a session concern because its pressure and scheduling capacity are shared even
when rows retain torrent attribution. Logs, transfer speeds, DHT, Disk, search,
settings, and other session or product concerns use an identifiable right-side
global group. The accepted storage and piece split lives in
[`disk-and-piece-inspection`](disk-and-piece-inspection.md).

The accepted detail-tab sequence is complete. Tactical
[`064`](../tactical/064-registry-backed-swarm-inspection.md) now makes Swarm a
bounded, virtualized table over retained registry records while Peers remains
active connections. One central tab vocabulary owns torrent/session scope,
and the `swarm-lifecycle` plus 1,000-row fixtures cover state legibility,
responsive layout, accessibility, and bounded rendering. Completed Tactical
[`065`](../tactical/065-dht-observatory.md) makes DHT a
session observatory led by a static shared-prefix-depth distribution with
mirrored replacement occupancy, freshness, a truthful deeper-band summary, and
bounded lookup-convergence rows. An optional presentation toggle shows the
literal 160-slot engine array using the same observation, making the normalized
encoding teachable and screenshots diagnostic without introducing chart
inspection as required interaction. No force graph, globe, or raw node table
enters the slice. Its permanent fixture and browser evidence cover lifecycle,
sparse and ordinary occupancy, active convergence, malformed and rate-limited
traffic, a nonzero deeper tail, stale delivery, terminal state, both encodings,
narrow layout, and light/dark accessibility.
Completed Tactical [`066`](../tactical/066-smooth-session-speed-history.md)
makes Speed a session-owned range-selected history rendered by local high-DPI
Canvas code.
Exact received, staged-write, and verified payload lead the chart; coarse
rollups persist separately from correctness-critical session state, RAF smooths
and pans only between exact live anchors, and no general chart dependency is
selected.

Use three top-level application destinations rather than making one sidebar
carry product navigation, torrent filtering, and media organization:

- Library is content-centric media browsing and eventual playback;
- Transfers is the clean operational torrent queue; and
- Workbench preserves the current dense torrent table, detailed tabs, and
  global diagnostic surfaces as a first-class traditional interface.

The accepted responsibilities, continuity rules, media-truth boundary, and
local mockup record live in
[`application-interface-direction.md`](application-interface-direction.md).
The sidebar is contextual to the active destination. Torrent lifecycle filters
such as active, downloading, seeding, completed, paused, and errors therefore
belong to Transfers and Workbench rather than being described as a media
library.

The resulting conceptual hierarchy is:

1. application and session;
2. Library, Transfers, or Workbench destination;
3. destination-specific category, filter, and organization;
4. media or torrent collection and selection;
5. selected item and its contextual or Workbench detail views; and
6. progressive detail for a peer, file, tracker, piece, or other selected row
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

Anchored action menus and popover dialogs render through a body portal so
application and table overflow cannot clip them. They keep an 8-pixel viewport
boundary, flip and shift with available space, inherit root theme and
interface-size tokens, and stay below modal dialogs. At narrow phone widths a
nested menu divides usable width with its parent rather than rendering off
screen or covering the parent. Escape restores the relevant trigger; Tab
continues through document focus order. The first outside tap dismisses only
and does not also activate the obscured control. The shared trigger supports
desktop context invocation. Tactical `085` binds it only to actionable torrent
and file rows; visible toolbar/More actions remain the primary touch path and
touch/pen long press remains additive selection.

The continuing actionable-table interaction contract lives in
[`table-interaction.md`](table-interaction.md). Tactical `069` implements its
current-within-selection model: one singular current row owns detail and is
always checked, while every checked row is an action target. Row-body and bare
keyboard navigation collapse to one current row; checkbox, modifier, range,
and select-all gestures build the selection. Actionable tables retain visible
checkboxes; read-only inspection tables do not gain batch behavior without row
actions. Tactical `070` makes an error-bearing torrent status a nested
explanatory control without disturbing those row gestures. Hover exposes its
bounded error, keyboard and assistive technology receive the same context, and
activation establishes a singleton current row before opening and focusing the
General error detail. Tactical `085` adds right-click, Context Menu key, and
Shift+F10: a selected row preserves the complete checked set, while an
unselected row becomes the singleton target before the menu opens.

Torrent-detail tab selection is a paint-only state change. Labels retain the
same font metrics, the underline is out of layout flow, and every tab has the
same fixed footprint for the selected interface size. Navigation labels do not
carry counts; the corresponding detail content owns row totals and summaries.
The divider before Disk continues to distinguish torrent-scoped from
session-scoped views. Selecting or evicting a detail view therefore cannot move
neighboring tabs, while constrained layouts retain one horizontally scrolling
row rather than shrinking or wrapping the tabs.

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

Browser application-view delivery uses bounded periodic JSON snapshots and
keyed diffs through one leased view set; Tauri now streams the same batches.
The UI architecture does not depend on full-state replacement or broad React
context updates. A later frame-coalescing policy may paint latest-state stream
changes on animation frames without changing view semantics. Binary encoding
is a separable measured codec optimization, not automatically a new
application API.

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

Tactical `047` establishes the first global presentation preference. The
user-facing vocabulary is **Interface size** with Compact, Standard, and
Spacious presets; Standard is the fresh-install and invalid-storage fallback.
The versioned value is owned by the per-application Zustand store and persisted
best-effort in browser-local storage. Semantic CSS variables change type,
controls, icons, tabs, menus, dialogs, and spacing without whole-document zoom.
The same typed preset definition supplies virtual-table header and row CSS plus
absolute-row and keyboard-scroll arithmetic. Compact preserves useful desktop
density but not illegibly small icons, while coarse-pointer controls retain
44-pixel targets.

Tactical `050` extends that same versioned appearance owner instead of adding a
competing setting. Version 2 persists Interface size with **Color theme** as
Auto, Light, or Dark; version-1 sizes migrate intact with Auto. Auto follows
live `prefers-color-scheme` changes in CSS, while explicit choices override the
system. The validated root attribute is installed before the inspection
bundle's dynamic React import and synchronized from presentation state after
live changes. The since-retired direct-DOM UI retained its Dark-only browser
declaration and did not interpret the React appearance record while both entry
paths still existed.

Tactical `055` adds a separate versioned navigation preference for the active
Library, Transfers, or Workbench destination and each destination's local
filter. Transfers is the fresh and invalid-storage fallback. The preference
contains presentation values only and tolerates absent, malformed, future, or
denied storage. Tactical `058` keeps the current torrent and explicit batch
selection as separate ephemeral application-store concepts. Missing torrents
are pruned without inventing a replacement after the user clears or loses the
current row, and neither concept is persisted.

Archive state, user labels, and other organization intended to survive a new
webview or appear in another client are durable application data. Torrent
activity, queue state, storage, and content retention remain separate engine
or application concepts. The API design must make those distinctions explicit
before controls are implemented.

Download roots, the default root, and the choice attached to a torrent are
also durable application state rather than browser-local preferences. The
shared React surface requests a platform-owned picker capability and selects
established root identities; it never persists an ambient path or browser
directory handle. The accepted root, start-content, and metadata-backed file
selection flow lives in [`download-roots.md`](download-roots.md).
Metadata-only intake uses durable paused application intent; the Files tab
remains the only file-selection surface.

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

Tactical `058` adds a Files-local current row and explicit multi-selection
using the shared table interaction. Tactical `063` activates its More menu
with exactly `Normal` and `Skip` for the current row or selected range. The
live adapter sends bounded semantic file indices, displays command results,
and waits for the authoritative Files view to change the priority column. Demo
scenarios retain disabled actions with an explicit reason rather than
pretending to mutate engine state.

Tactical `077` replaces the component-local Files and torrent action menus,
table Columns popover, and column-help overlay with one locally styled React
Aria Components layer. It retains action and table state in the feature
owners, while the shared layer owns body-portalled rendering, collision-aware
placement and sizing, focus, dismissal, nested-menu behavior, and portal-safe
theme/interface metrics. Tactical `085` now uses that context-trigger
capability on production actionable rows, through one named trigger outside
the ARIA grid. Table touch long press still selects, and read-only tables retain
ordinary browser context behavior.

The Add dialog now has one checked-by-default **Start downloading files when
metadata is available** checkbox. Clearing it acquires metadata without
creating content artifacts and directs the user to the Files tab; no file tree
or second modal is introduced. Hiding add options continues to use the usable
default root and starts content normally. Tactical `083` reuses this exact
dialog for a chosen `.torrent`. Empty or over-64-MiB files fail locally; a
read or adapter failure remains visible, and a dialog-owned file can be
retried without selecting it again. Chooser cancellation is a no-op, and the
hidden input resets after selection so the same file can be chosen later.

Column visibility, widths, sort, and live-sort preference persist per table in
a versioned browser-local setting. Resize separators work by pointer and
keyboard. Wide, compact, and phone evidence keeps the active tab visible,
closes the phone drawer fully, and leaves horizontal scrolling available for
explicit extra columns. Serious and critical axe findings are empty.

Tactical `047` adds wide Compact, Standard, and Spacious geometry assertions,
reload persistence, desktop and phone Settings-sheet focus containment, and
serious/critical axe checks. Standard uses 36-pixel controls and table rows,
Compact retains 30-pixel desktop controls and 32-pixel rows, and Spacious uses
44-pixel controls and 42-pixel rows. The existing 2,001-torrent and
10,001-peer scenario remains bounded at 824 DOM elements after the new default;
this is sampled development evidence rather than a browser-wide ceiling.

Tactical `050` adds browser assertions for Auto under both system schemes and
through a live media change, explicit Light under system Dark, explicit Dark
under system Light, complete size/theme reload persistence, and the persisted
attribute already present at first React content. Explicit Light and Dark
Settings/application scans have no serious or critical axe findings. The new
dark scan also moved the demo strip and primary action from hard-coded light
colors to palette-specific semantic tokens after exposing contrast failures.

Tactical `055` adds deterministic navigation, storage-denial, selection
repair, destination-local filter, view-leasing, command, and component tests.
The headless browser suite covers wide and phone destination navigation,
contextual drawers, Library-to-Workbench handoff, shared selection, keyboard
operation, and empty serious/critical axe findings. Its 2,000-torrent scenario
keeps Workbench and Transfers below 100 rendered rows, keeps Library below 100
rendered cards, and retains fewer than 2,000 total DOM elements after changing
destinations. These are bounded development assertions rather than a general
browser performance guarantee.

Tactical `083` adds deterministic empty/nonempty Add, pointer/keyboard chooser,
cancel, advisory accept, same-file reset, default/chosen root, start-content,
post-success preference, busy, size, read-failure, retry, and byte-intent
coverage. Its production headless run observes the actual file chooser and
one binary frame through the ordinary WebSocket, no semantic HTTP requests,
the visible imported row, no payload artifacts for metadata-only intake, and
no serious/critical axe findings.

Tactical `058` adds pure state and component coverage for independent current
and batch torrent selection, modifier and keyboard entry, touch long press and
movement cancellation, empty-space clearing, shared Transfers/Workbench mode,
and Files-local selection. Browser evidence covers the explicit desktop path,
a phone-sized long press, disabled Files actions, wide/compact/phone Files
layouts, and empty serious/critical axe findings. The 4,096-row scenario still
renders fewer than 100 table rows; one sampled run retained 665 DOM elements,
52,586,655 bytes of JavaScript heap, and a 41 ms scenario update.

Tactical `059` adds component evidence for permanently stable selection-column
geometry, inactive checkbox entry, forward and reverse Shift ranges, range
shrinking, checkbox Shift-click, sorted-order resolution, and mode exit. The
application and browser suites exercise the same range behavior in Transfers,
Workbench continuity, and the 4,096-row Files surface while retaining bounded
virtual rendering and empty serious/critical axe findings.

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
while selecting every view. It asserts label-only tabs, equal footprints at
every interface size, the divider before Disk, and horizontal overflow at
phone width. Narrow layouts may scroll to reveal a clipped active tab, but the
tab boxes themselves do not reflow.

Tactical `087` removes the redundant configured-tracker and connected-peer
badges and establishes 88-, 100-, and 112-pixel tab footprints for Compact,
Standard, and Spacious. Headless Chrome confirms equal, stable geometry at
wide, compact-window, and phone widths, including the real phone
list-to-detail path and horizontal overflow. The representative 1,440- and
390-pixel strips were visually inspected with the torrent/session divider
intact and no label clipping or wrapping.

Tactical `049` replaces the generic Logs table with a dedicated ordered
console. Opening Logs does not require a selected torrent. Capture profile,
category prefixes, and pinned torrent scope alter the desired diagnostics
view; text, severity, category, and torrent display filters operate over the
already retained local history. Expanded entries show deliberately typed
context and provide copy actions. Local clear advances a sequence watermark,
and scrolling away from the bottom exposes a new-record action without
stopping bounded ingestion.

The permanent 10,000-record scenario retains 2,048 semantic records and
renders only 24 record elements in the sampled wide viewport. Wide and phone
screens pass serious/critical accessibility checks, retain keyboard-operable
controls, and do not make the continuously changing feed an `aria-live`
stream. Console state remains one application-lifetime presentation concern;
no diagnostic history or filters are written to browser storage.

Tactical `071` adds deterministic component and browser coverage for the
selection-aware copy action. The browser grants clipboard permission, reads
back the exact canonical URI, checks restored More focus and disabled
multi-selection behavior, and finds no serious or critical Axe violations in
the open menu. Tactical `085` supersedes only the prior multi-selection
disablement: it reads back one exact newline-delimited value for every selected
torrent in stable order. No submitted-source URI or durable clipboard state is
added.

Tactical `077` adds deterministic overlay evidence at 320x568, 390x844,
456x1024, 920x720, and 1440x900, at every trigger corner and after live
resize. Pointer, touch, keyboard, nested, context, dismissal, lifecycle, all
density/theme combinations, and open-overlay serious/critical Axe checks pass.
The full browser gate passes 28 deterministic cases with five live opt-in
cases skipped. Open File actions, Columns, help, and shared menu/submenu
screenshots were visually inspected at representative wide and phone sizes in
Light and Dark. The accepted dependency adds 153.44 kB minified / 48.21 kB
gzip to the production bootstrap while removing about 0.43 kB of component
CSS; `npm audit --omit=dev` reports no vulnerabilities and the CSP scan stays
clean.

Tactical `085` adds pure action-policy and component coverage for exact action
order/grouping/placement, whole-selection availability, sequential command
continuation, bounded failure feedback, plural removal retry, and shared file
priority rendering. The complete deterministic browser gate passes 29 cases
with seven live opt-in cases skipped; it covers Transfers, Workbench, Files,
pointer and keyboard context entry, plural clipboard readback, collision,
focus return, 4,096-file virtualization, and empty serious/critical Axe
findings. Two focused cases pass again against the production Vite preview,
and the build/CSP scan remains clean.

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
4. Files is complete in Tactical `041`, the schedule-backed Trackers view is
   complete in Tactical `043`, and the structured ordered Logs console is
   complete in Tactical `049`.
5. Connect remaining detail views according to debugging and product value,
   keeping unsupported scaffolds truthful.
6. Tauri streaming is complete in Tactical `048`. Accepted Tactical `060`
   owns the bounded multiplexed browser WebSocket and diagnostic HTTP
   comparison. Measure browser update volume and decode/reduce/render cost
   before adding binary encoding or more specialized rendering paths.

This is continuing direction, not an active tactical or permission to build
all stages as one slice.

## Deliberately Open Decisions

- the default wide-screen detail position and user-selectable layout modes;
- inbox, category, label, and queue semantics; archive and removal retention
  semantics are implemented by Tactical `040`;
- whether later navigation complexity justifies a router or icon dependency;
- exact columns, row-detail interactions, and further table customization
  outside the non-sortable Logs console;
- the first supported browser and phone access posture beyond headless and
  local desktop use; and
- the thresholds that would justify a binary codec.

Tactical `033` completed the headless transport-independent view-set
foundation. Tactical `034` completed the first visible React and demo-scenario
foundation. Tactical `035` completed the live torrent/peer adapter, exact
active-row membership, local endpoint posture, semantic view selection, and
suspension recovery. The implemented view-set limits and remaining delivery
choices are recorded in
[`application-view-api.md`](application-view-api.md).
