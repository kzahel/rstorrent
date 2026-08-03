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
acknowledged in-process leased-view streaming; HTTP polling remains the
browser/headless delivery adapter. Tactical `049` completes the accepted
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
applies the same interaction locally to Files. Files now exposes disabled
Download and Skip actions without claiming runtime mutation support.

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

Table multi-selection is an explicit mode rather than permanent checkbox
chrome. A visible Select control and keyboard Space make the mode discoverable;
Command/Control-click and a bounded touch/pen long press are accelerators. Only
the mode shows checkboxes and select-all, row activation then toggles checks,
and Done or Escape clears the batch set. Outside the mode, row activation owns
one current item, commands target that item, and empty table space can clear it.

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
live changes. The legacy UI retains its existing Dark-only browser declaration
and does not interpret the React appearance record.

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
using the shared table interaction. The visible More menu contains Download and
Skip download, but both remain disabled with an unavailable reason because no
runtime file-selection command exists. This is intentional UI staging, not an
optimistic command or a support claim.

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

Tactical `058` adds pure state and component coverage for independent current
and batch torrent selection, modifier and keyboard entry, touch long press and
movement cancellation, empty-space clearing, shared Transfers/Workbench mode,
and Files-local selection. Browser evidence covers the explicit desktop path,
a phone-sized long press, disabled Files actions, wide/compact/phone Files
layouts, and empty serious/critical axe findings. The 4,096-row scenario still
renders fewer than 100 table rows; one sampled run retained 665 DOM elements,
52,586,655 bytes of JavaScript heap, and a 41 ms scenario update.

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
6. Tauri streaming is complete in Tactical `048`. Measure browser update
   volume and decode/reduce/render cost before adding WebSocket delivery,
   binary encoding, or more specialized rendering paths.

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
- the thresholds and transport shape that would justify low-latency streaming
  or a binary codec.

Tactical `033` completed the headless transport-independent view-set
foundation. Tactical `034` completed the first visible React and demo-scenario
foundation. Tactical `035` completed the live torrent/peer adapter, exact
active-row membership, local endpoint posture, semantic view selection, and
suspension recovery. The implemented view-set limits and remaining delivery
choices are recorded in
[`application-view-api.md`](application-view-api.md).
