# Interface Size Settings

Status: Complete (2026-08-02).

Topics: `web-ui-design`, `desktop-inspection-surface`

## Motivation

The shared web inspection surface has no global settings entry point. Its
default presentation is also smaller than the intended ordinary desktop
experience: toolbar labels are approximately 11--12 CSS pixels, controls are
about 30 pixels high, the More chevron is approximately 9 pixels, and Archive,
Start, Pause, and Remove use font-dependent Unicode glyphs. Several actions are
difficult to parse even when their text remains readable.

The current sizes are distributed across component CSS while virtual table row
and header geometry is duplicated as numeric TypeScript constants. Increasing
only the root font size or applying browser-style zoom would leave scroll math,
responsive behavior, focus geometry, and some fixed-height controls
inconsistent. This slice establishes an explicit browser-local appearance
preference and one coordinated semantic sizing vocabulary.

## Stable Scenarios

- A fresh application starts at the larger Standard interface size.
- Compact, Standard, and Spacious change typography, controls, spacing, tabs,
  menus, dialogs, and virtualized rows together without reload.
- The selected size survives reload in one versioned browser-local preference;
  unavailable, malformed, or future-version storage falls back to Standard.
- Changing size does not overlap virtual rows, corrupt keyboard scroll
  positioning, materialize unbounded DOM, or alter engine/application views.
- A global header gear opens an accessible right-side Settings sheet, Escape
  and its close action dismiss it, and focus returns to the gear.
- The Settings sheet fills a phone viewport while the gear remains available
  after nonessential header statistics collapse.
- Toolbar action and disclosure icons remain legible in every size, including
  Compact, and retain visible text labels where they have them today.

## Scope

- Add the first global Settings entry point in the application header and an
  Appearance section with an Interface size radio group.
- Define `compact`, `standard`, and `spacious` as the stable browser-local
  presentation vocabulary, with Standard as the default.
- Persist and validate the preference independently from engine snapshots,
  torrent state, and durable cross-client application data.
- Replace scattered small typography, control, icon, toolbar, table, tab,
  menu, dialog, and spacing values with semantic application-level tokens.
- Make virtual-table row/header metrics derive from the same selected preset
  used for rendered geometry.
- Replace the action toolbar's font glyphs and More disclosure glyph with a
  small first-party inline SVG icon set; add the Settings gear from that set.
- Retain useful coarse-pointer targets even when Compact is selected.
- Add deterministic store/component tests and representative headless browser
  geometry, accessibility, persistence, responsive, and visual evidence.
- Update the owning web UI topic and tactical index.

## Non-goals

- Engine settings, bandwidth policy, storage settings, theme selection,
  synchronization between clients, or Android settings/UI changes.
- A continuous scale slider, arbitrary percentage zoom, browser zoom control,
  `transform: scale()`, or CSS `zoom`.
- A router, standalone settings page, settings API, design-system dependency,
  or third-party icon package.
- Redesigning information hierarchy, table columns, user-resized column widths,
  detail-pane sizing, responsive breakpoints, or phone navigation.
- Removing dense desktop operation: Compact remains a supported first-class
  presentation, but illegible icons are not part of its contract.

## Reference Dossier

This is presentation-only work and does not change BitTorrent behavior, so no
protocol or libtorrent source governs it.

Local JSTorrent revision `9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/ui/src/hooks/useAppSettings.ts` applies a named UI-scale attribute
  and keeps presentation settings outside torrent state.
- `packages/ui/src/styles.css` defines named typography, row, spacing, button,
  and icon metrics and deliberately defaults above its historical base size.
- `packages/ui/src/tables/VirtualTable.solid.tsx` makes table geometry react to
  scale changes rather than treating CSS as visual-only.
- `packages/client/src/components/SettingsOverlay.tsx` exposes scale from a
  global settings surface.

RSTorrent adopts the product lessons that the readable preset should be the
default, presentation sizing should be named and persistent, and virtualized
geometry must change coherently. It deliberately uses three semantic presets,
does not apply whole-document zoom, keeps preferences per web application
instance, and independently authors its React, Zustand, CSS, dialog, icon, and
test implementation. No source or asset is copied.

## Accepted Design

The user-facing setting is **Interface size**:

| Preset | Intent | Approximate toolbar/control/table metrics |
| --- | --- | --- |
| Compact | maximum useful desktop density | 12 px text, 30--32 px controls, 32 px rows |
| Standard | balanced default | 13--14 px text, 34--36 px controls, 36 px rows |
| Spacious | larger text and targets | 15--16 px text, 40--44 px controls, 42 px rows |

The application root exposes `data-interface-size`. Semantic CSS custom
properties own typography, icon size, control height, table geometry, toolbar
height, and recurring spacing. Components retain colocated layout/state CSS.
Virtual table code receives explicit numeric metrics from the same TypeScript
preset definition so absolute row offsets and keyboard scrolling match CSS.

The gear is a labeled icon-only button at the far right of the global header.
It opens an application-modal right sheet approximately 360 pixels wide on
desktop and full-width on phones. Changes apply and persist immediately; there
is no Save action. The sheet has an explicit close button, traps focus, closes
with Escape or backdrop activation, and restores focus to the gear.

## Invariants And Bounds

- `InterfaceSize` has exactly three accepted serialized values. Unknown input
  never reaches a CSS attribute or metric lookup.
- The versioned preference contains presentation data only and remains
  optional when local storage is denied or throws.
- Standard is the code, storage-fallback, and visual default.
- One selected preset supplies every virtual row/header number and its
  corresponding CSS height. A density transition cannot retain stale scroll
  geometry.
- User-resized column widths remain stable CSS-pixel preferences across size
  changes; this slice does not silently rewrite their stored values.
- Every rendered action icon uses fixed view-box SVG geometry with currentColor
  and is hidden from assistive technology when adjacent text supplies the
  accessible name.
- The icon-only Settings and close buttons have explicit accessible names.
- Compact does not reduce coarse-pointer interactive targets below 44 CSS
  pixels. Browser zoom and larger text remain independently usable.
- No interface-size transition changes desired Rust views, commands, torrent
  selection, active detail tab, or materialized row identity.
- Existing bounded overscan remains unchanged; larger rows should render the
  same or fewer visible DOM rows.

## Ownership And Data Flow

```text
versioned browser-local appearance preference
  -> createInspectionStore initial presentation
  -> Settings radio action / setInterfaceSize
       -> validate named value
       -> update this application store instance
       -> best-effort persist immediately
  -> App data-interface-size + semantic CSS tokens
  -> VirtualTable preset metrics
       -> header height, row height, offsets, keyboard scroll, visible count
```

The Zustand application instance owns the current preference. A small
appearance module owns accepted values, defaults, metrics, and serialization.
Settings UI owns only open/closed dialog state and dispatches the typed store
action. Engine adapters, the generated application contract, and the view
controller do not participate.

## Shape-Changing Edge Cases

- first launch with no storage;
- malformed JSON, unknown enum value, wrong version, and storage exceptions;
- density change while a table is scrolled or a row has keyboard focus;
- density change while the detail tab strip is horizontally scrolled;
- Settings opened at wide, compact-window, phone, and coarse-pointer layouts;
- Tab, Shift+Tab, Escape, close button, and backdrop dialog dismissal;
- changing size while the Settings sheet itself is open;
- toolbar wrapping at phone width and long live command status text;
- Compact and Spacious under browser zoom and system dark mode; and
- large logical torrent, peer, file, piece-work, and log collections.

## Implementation Order

1. Add the typed preset/metric/persistence module and store state/actions with
   deterministic default, round-trip, invalid-input, and unavailable-storage
   tests.
2. Add the global gear and accessible responsive Settings sheet with live
   Interface size selection and focus restoration.
3. Establish semantic global size tokens and migrate the shell, navigation,
   toolbar, detail tabs, tables, panels, menus, and dialogs to them.
4. Feed preset table metrics into absolute row rendering and keyboard scroll
   math, then prove runtime switching retains bounded correct geometry.
5. Replace toolbar/disclosure glyphs with independently authored inline SVG
   icons and retain semantic button labels.
6. Run component and representative browser coverage, inspect captured Standard,
   Compact, Spacious, and phone results, and correct clipping or regressions.
7. Record exact evidence, update the owning topic and tactical index, run the
   proportional repository gates, and commit the complete slice.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Preference | default, valid round trip, malformed/future values, and throwing storage |
| Store/component | live selection, data attribute, SVG icons, dialog keyboard/focus behavior, and persistence |
| Virtual table | row/header heights, offsets, keyboard scroll, and bounded DOM at all presets |
| Responsive browser | wide Standard, Compact, Spacious, and phone Settings/toolbar geometry |
| Accessibility | semantic dialog/radio group, labeled icon buttons, focus containment/restoration, and no serious/critical axe findings |
| Repository | frontend formatting, TypeScript, unit tests, production build, and headless browser suite |

No public swarm, live engine, visible Tauri client, Android build, emulator, or
physical device is required. The demo adapter exercises the exact shared React
surface without external traffic.

## Stopping Condition

This slice is complete when the header exposes an accessible Settings sheet;
Standard is the larger default; all three interface sizes apply immediately
and persist safely; all affected typography, controls, icons, and virtual table
geometry change coherently without clipped, overlapping, or unbounded content;
representative wide/phone accessibility and visual evidence passes; the owning
topic and tactical index record the result; and the work is committed with a
clean tree.

## Escalation Contract

The browser-local preference, Settings sheet, first-party SVG icons, semantic
token refactor, virtual-table metric plumbing, responsive corrections, tests,
and topic/tactical updates are authorized. Stop for direction if implementation
requires a new dependency, engine or generated-contract setting, cross-client
persistence, router, changing durable application data, removing a supported
layout, or launching a visible/physical product client.

## Implementation And Evidence

The inspection store now initializes one typed `InterfaceSize` from the
versioned `rstorrent.presentation.appearance` browser-local record. Missing,
malformed, unknown, future-version, and throwing storage all select Standard.
The typed action persists Compact, Standard, or Spacious immediately while the
controller's optional storage boundary keeps per-application tests isolated.
Engine snapshots, application commands, desired Rust views, and generated
contracts are unchanged.

The global header now retains a labeled Settings gear after session connection
status and after all nonessential phone header content collapses. It opens a
right-side application-modal Settings sheet with an Appearance radio group,
immediate live application, explicit close action, Escape and backdrop
dismissal, focus containment, and focus restoration. At 390 pixels the sheet
fills the viewport; at desktop sizes it remains a bounded 23-rem sheet. No
router, settings service, icon dependency, or Save transaction was added.

Semantic application tokens now own recurring caption, small, body, heading,
display, icon, control, header, toolbar, tab, and spacing metrics across the
shell, sidebar, demo controls, command toolbar, detail tabs, virtual tables,
disk/piece surfaces, menus, and dialogs. Standard is visibly larger than the
old presentation. Compact retains approximately the old control/row density
with readable icons; Spacious raises controls to 44 pixels and rows to 42
pixels. Coarse-pointer layouts retain 44-pixel controls independently from the
visual preset.

The generic virtual table accepts the named preset explicitly. Its one typed
metric lookup sets the CSS header/row variables and drives viewport slicing,
canvas height, absolute row offsets, keyboard scrolling, and focus visibility.
Runtime switching therefore cannot leave a 32-pixel visual row with 36-pixel
scroll arithmetic. Existing per-table column widths remain untouched.

The menu, Add, Start, Pause, Archive/Restore, Remove, hamburger, close, and gear
artwork now use a small independently authored inline SVG set with one
view-box/stroke vocabulary and no external asset or dependency. Adjacent text
continues to provide action names; icon-only actions have explicit labels.
Compact keeps 16-pixel action artwork, Standard uses 18 pixels, and Spacious
uses 20 pixels.

Deterministic evidence covers default/fallback parsing, every valid round trip,
denied storage, store restoration, dialog focus containment/restoration, live
root-attribute changes, and Compact/Spacious absolute table geometry. Headless
Chrome asserts 30/36/44-pixel controls, 32/36/42-pixel rows, readable icon
bounds, reload restoration, the full-width phone sheet, and empty serious or
critical axe findings at desktop and phone sizes. Captured light-mode Standard,
Compact, Spacious, wide, and phone results were inspected; the action toolbar,
gear, option labels, tabs, and table rows remain readable without overlap.

Validation run on 2026-08-02:

```text
source ~/.profile
cd clients/web
npm run typecheck
npm test
npm run build
npm run test:e2e
cd ../..
cargo fmt --all -- --check
```

TypeScript passed; 75 frontend tests passed and two environment-specific tests
remained skipped. The production Vite build passed. Ten headless Chrome demo
tests passed and the three opt-in live-engine browser cases remained skipped.
The large scenario retained 824 DOM elements, approximately 33.6 MiB sampled
JavaScript heap, and bounded virtual rows; the file scenario retained 678 DOM
elements and approximately 65.3 MiB. Rust formatting remained clean. No public
network, visible Tauri client, Android build, emulator, physical device, new
dependency, or generated-contract change was used.
