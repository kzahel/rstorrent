# Tactical 077: Shared Overlay And Menu System

Status: Planned from maintainer direction on 2026-08-04. Implementation has
not started.

Topics: `web-ui-design`

## Decision And Motivation

Replace the web client's component-local popup mechanics with one small,
RSTorrent-styled overlay system built on the unstyled React Aria Components
primitives. Action menus, nested action menus, arbitrary-content popovers, and
help popovers must share portalled rendering, collision-aware positioning,
dismissal, focus restoration, keyboard behavior, layering, and responsive
bounds.

The current surface has four independent implementations:

- `FileActionsMenu` installs document listeners, focuses its menu container,
  and absolutely aligns a fixed-width menu inside the Files summary.
- `MoreActionsMenu` separately implements document listeners, item focus,
  arrow navigation, and a fixed-direction nested menu.
- `VirtualTable` implements Columns as a locally positioned nonmodal dialog
  without outside-press dismissal.
- `VirtualTable` portals column help to `document.body`, but computes fixed
  coordinates and dismissal by hand.

This duplication has produced materially different behavior and a confirmed
phone failure. In a 456 CSS-pixel viewport with a summary equivalent to the
reported eight-file case, the Files More trigger occupied approximately
`x=78..149`, while its right-aligned 208-pixel menu occupied
`x=-59..149`. The `Normal` and `Skip` labels were entirely at negative x
coordinates even though the clipped right side of the menu background
remained visible.

A portal is necessary to escape application and table clipping, but is not a
positioning, focus, dismissal, or menu-semantic system by itself. This
tactical adopts one focused dependency rather than extending the handwritten
implementations independently.

## Desired Outcome And Stopping Condition

The tactical stops when:

- one local overlay layer owns the web client's action-menu and anchored
  popover behavior while retaining RSTorrent's existing visual language;
- file actions, torrent More and its submenu, table Columns, and column help
  all use that layer and no longer install their own document listeners,
  portals, or viewport-coordinate algorithms;
- every migrated overlay renders outside clipped application/table ancestors,
  remains within the usable viewport, updates while its anchor or viewport
  moves, and scrolls internally when available height is insufficient;
- action menus meet the accepted menu-button keyboard, focus, disabled-item,
  submenu, dismissal, and focus-return contract;
- Columns remains a popover dialog with interactive checkboxes rather than
  being mislabeled as an action menu, and outside press, Escape, focus, and
  trigger toggling behave consistently;
- portal content inherits the exact active color theme and Compact, Standard,
  or Spacious interface metrics;
- the shared action-menu trigger can use either ordinary press or desktop
  context-menu invocation without duplicating menu content, while no product
  row gains a new context-menu binding in this slice;
- open-menu browser evidence passes at representative wide, compact, and
  phone viewports, including the reported short-summary geometry; and
- the web typecheck, component tests, production build/CSP check, focused
  browser suite, accessibility scans, and owning-topic updates pass.

## Accepted Dependency

Pin `react-aria-components` `1.20.0` in `clients/web`. The package metadata
inspected on 2026-08-04 identifies the Adobe React Spectrum repository and an
Apache-2.0 license. Implementation must audit the exact lockfile graph and
update release notices when the repository's dependency policy requires it.
No source, styles, examples, or fixtures are copied from the project.

Use the unstyled React Aria Components package, not the styled React Spectrum
component system. RSTorrent continues to own DOM-adjacent wrappers, CSS
Modules, semantic colors, density, icons, labels, command policy, and tests.
Do not add `@floating-ui/react`, Radix, another overlay system, or a full UI
framework in parallel.

The dependency is accepted because it solves the concrete cross-cutting
interaction and accessibility boundary. If focused imports cannot be
tree-shaken, require unsafe DOM workarounds, conflict with the production CSP,
or cannot preserve the contracts below, stop and report measured evidence
before adding another library or forking its behavior.

## References And Existing Evidence

- The W3C WAI-ARIA Authoring Practices
  [menu-button](https://www.w3.org/WAI/ARIA/apg/patterns/menu-button/) and
  [menu](https://www.w3.org/WAI/ARIA/apg/patterns/menubar/) patterns define
  trigger semantics, focus entry, arrow/Home/End movement, submenu navigation,
  Tab behavior, Escape restoration, and menu item roles.
- React Aria's official [Menu](https://react-aria.adobe.com/Menu)
  documentation defines `MenuTrigger`, `Menu`, `MenuItem`, `SubmenuTrigger`,
  disabled items, and `trigger="contextMenu"` composition.
- React Aria's official [Popover](https://react-aria.adobe.com/Popover)
  documentation defines placement, flipping, positioning boundaries,
  automatic updates, maximum available height, outside-interaction filtering,
  and custom target rectangles for pointer coordinates.
- React Aria's official
  [PortalProvider](https://react-aria.adobe.com/PortalProvider) documentation
  confirms that popovers portal outside ordinary overflow and stacking
  ancestors. RSTorrent should use the default body portal unless evidence
  requires a dedicated root; custom portal placement is not an initial
  requirement.
- [Floating UI](https://floating-ui.com/docs/react) and the native
  [Popover API](https://developer.mozilla.org/en-US/docs/Web/API/Popover_API/Using)
  were considered. Floating UI provides strong low-level positioning but
  leaves more menu semantics and item interaction to the application. Native
  popovers provide top-layer and light dismissal behavior but not the complete
  menu, focus, submenu, and collision contract. Neither is added in this
  slice.
- Existing behavior and tests originate in Tacticals `038`, `047`, `050`,
  `051`, `058`, `063`, and `071`. Preserve their truthful command targeting,
  interface size, theme, peer-legend, table selection, live file selection,
  and clipboard outcomes while replacing overlay mechanics.

This is a frontend interaction and presentation slice. It changes no
BitTorrent protocol, engine, storage, application command, generated contract,
or transport behavior, so no normative BEP or pinned libtorrent survey is
required.

## Shared Widget Boundary

The implementation should expose a small local component surface equivalent
to these semantic roles; exact names may follow the existing frontend style:

- an action menu trigger and action menu content;
- action items, disabled items, separators, sections, and one nested submenu;
- a trigger mode that supports ordinary press or desktop context-menu
  invocation over the same action content;
- an anchored popover dialog for interactive content such as Columns; and
- an anchored help popover for explanatory content.

Do not expose React Aria throughout feature components when a local wrapper can
retain the invariant. Feature owners provide labels, enabled state, callbacks,
and content. The shared layer owns overlay mechanics and maps those values to
the library primitives.

Action-menu content contains actions and menu structure only. Columns remains
a dialog because it contains persistent checkboxes and a reset button. A
disabled-action explanation may be rendered as an inert labelled description
within the surrounding popover, but arbitrary interactive controls do not
become descendants of `role="menu"`.

## Portal, Appearance, And Layering Contract

React Aria overlays may use their default body portal. RSTorrent's color tokens
already live on `:root`, but interface metrics such as `--ui-font-small`,
`--ui-control-height`, and spacing currently live on `.app`. Move the global
interface-size attribute and its semantic metric variables to a root scope
that both the application and body-portalled overlays inherit. Apply the
stored interface size before first React content, as color theme already is,
so an initially open or quickly opened overlay cannot flash Standard metrics.

Do not copy computed styles from a trigger into each portal or maintain a
second density state. The versioned browser preference and the application
presentation store remain the one authority; the document-root attribute is
its presentation projection.

Define one documented overlay layer order for ordinary popovers, nested
popovers, and modal dialogs. An ordinary menu must appear above sticky table
headers and drawers but below an active modal dialog. Nested menus belong to
their root overlay tree rather than winning through unrelated large z-index
values.

Portalled React events still follow the React owner tree. Opening, selecting,
or dismissing an overlay from a table row must not accidentally activate the
row, change selection, or trigger an empty-table action.

## Positioning And Size Contract

- A More menu prefers below-end alignment to its trigger.
- A menu or popover flips above when the preferred side cannot fit and shifts
  along the cross axis to remain inside the usable viewport.
- Every edge keeps at least eight CSS pixels of breathing room in addition to
  applicable safe-area insets.
- Width is content driven but never exceeds the usable viewport minus those
  margins. Long labels wrap only when the widget contract allows it; action
  labels normally remain one line and the menu may choose a wider fitting
  placement.
- Height is capped by actual available space and overflow scrolls inside the
  overlay without making the page or table scroll to reach an item.
- Position updates while the anchor, scrolling ancestors, viewport, visual
  viewport, interface size, or overlay content changes.
- A nested menu prefers the conventional outward side, flips horizontally and
  vertically as required, and remains in the same dismissal/focus tree.
- If the anchor becomes disconnected, hidden by virtualization, or irrelevant
  because navigation changed, the overlay closes rather than retaining stale
  screen coordinates.
- A context-triggered menu anchors to its invocation point and applies the same
  shift, flip, size, and safe-margin policy.

No feature component may recover a fixed `right: 0`, hard-coded submenu side,
or one-time `getBoundingClientRect()` calculation as a second positioning
path.

## Interaction And Focus Contract

- Pointer, touch, Enter, and Space open an ordinary menu. Down Arrow opens and
  focuses the first available item; Up Arrow may open and focus the last.
- Focus moves into an opened action menu. Arrow keys, Home, End, typeahead,
  Enter, Space, nested-menu arrows, and disabled items follow the accepted
  WAI-ARIA/React Aria behavior rather than component-local variants.
- Escape closes only the innermost submenu first, then its root, and returns
  focus to the invoking trigger or context when it still exists.
- Tab and Shift-Tab leave and close an action menu with one predictable focus
  transition; no removed menu item may strand focus on `body`.
- An outside primary-pointer press closes the overlay. Interaction with the
  outside target is neither swallowed unexpectedly nor delivered twice.
- Pressing the trigger while its overlay is open closes it. Opening another
  root overlay closes the prior root through ordinary outside interaction.
- Selecting an ordinary action closes the complete menu tree immediately.
  The command continues through its existing async owner, feedback remains in
  the existing status surface, and focus returns when the origin remains
  mounted. Command failure does not reopen the menu or claim success.
- A popover dialog keeps interaction inside its own controls, closes by
  outside press, Escape, or trigger toggle, and restores focus consistently.
  Library-provided touch-screen-reader dismissal must remain available.
- Disabling or removing a trigger while its overlay is open closes the overlay
  without attempting to focus a disconnected element.

Outside-dismiss logic must consider the portalled overlay, its trigger, and
nested overlays as one interaction tree. The existing
`container.contains(event.target)` pattern is not valid after portalling and
must not survive in migrated widgets.

## Context-Menu Boundary

The shared action-menu wrapper must support React Aria's desktop context-menu
trigger mode, including pointer coordinates and keyboard context invocation,
and component/browser tests must prove that it uses the same items and
placement contract as an ordinary trigger.

Do not attach that mode to torrent, file, peer, tracker, or other product rows
in this tactical. A later product slice must decide whether invoking a context
menu on an unchecked row replaces selection, preserves a checked range, or
acts on current context. It must also define command availability.

Touch and pen long press remain table-selection gestures established by
Tactical `058`. This tactical must not also bind them to a context menu or
change their 500-millisecond, movement-cancellation, or synthetic-click
behavior. Visible More remains the touch discovery and action path.

## Ownership, Tasks, Bounds, And Dependency Direction

```text
document appearance projection
  -> root theme and interface-size tokens

feature trigger + action/content policy
  -> local RSTorrent overlay wrapper
       -> React Aria positioning, focus, dismissal, and portal mechanics
       -> body portal subtree while open
```

Each feature component owns only whether its controlled overlay is open when
external state must close it, plus its command-pending and feedback state.
React Aria owns transient placement, list-navigation, focus, and nested-overlay
coordination. Overlay state is neither durable application data nor browser
preference and does not enter Zustand unless a demonstrated cross-component
owner requires it.

Every root overlay and submenu is conditionally mounted. The initial wrapper
supports at most one open submenu per root and the one level currently needed
by torrent More. Available-height scrolling retains only the menu's bounded
item DOM; this tactical introduces no virtual menu, queue, background task,
timer loop, socket, storage write, or application command.

Automatic observers/listeners belong to the selected library and must be
released when an overlay closes or its owner unmounts. Do not add a parallel
global overlay registry unless a failing coordination scenario proves it is
needed. Record the production JavaScript bundle delta and exact installed
dependency graph; no second overlay dependency is permitted.

## Shape-Changing Edge Cases

- The eight-file phone summary, a trigger at each viewport corner, and a
  trigger inside an overflow-scrolling table all keep every visible menu item
  inside the usable viewport.
- Compact, Standard, and Spacious change an already open overlay's metrics and
  position without clipping or retaining stale measurements.
- Light, Dark, and Auto apply the same semantic colors inside a body portal,
  including an Auto system-theme change while open.
- Browser resize, phone orientation/visual-viewport change, page zoom, table
  horizontal/vertical scroll, and changing menu description content preserve
  or deliberately close a valid overlay.
- Navigating tabs, changing torrents, removing a selected torrent, hiding a
  responsive pane, or virtualizing away an anchor leaves no orphan portal.
- Clicking a second trigger while a root menu or submenu is open produces one
  final open root and one action at most.
- Escape in a submenu closes only that submenu; a second Escape closes the
  root and restores focus.
- A menu with its first item disabled focuses the next available item. A menu
  whose actions are all unavailable still exposes its truthful explanation
  and a working dismissal path.
- An asynchronous command may remove or disable the trigger before completion
  without throwing, reopening, or focusing a disconnected node.
- A menu or help popover opened from a row does not activate that row again
  when a portalled child emits React pointer/click events.
- Context invocation at all four viewport edges is clamped and dismissible;
  touch long press continues to select rather than opening it.
- A popover opened above a modal cannot escape the modal's interaction and
  layer boundary; an ordinary page overlay cannot cover an active modal.

## Scope

- Add the exact accepted frontend dependency and lockfile changes, then audit
  its resolved license graph and production build output.
- Make interface-size semantic variables and initial document projection
  available to body-portalled overlays without duplicating appearance state.
- Add the bounded local action-menu, submenu, context-trigger mode, popover
  dialog, and help-popover wrappers and their shared CSS.
- Migrate `FileActionsMenu`, including its selected-target and unavailable
  explanation behavior.
- Migrate `MoreActionsMenu`, including copy, disabled state, async focus
  restoration, curated test torrents, and its one nested submenu.
- Migrate VirtualTable Columns without changing table configuration semantics,
  and migrate column help without changing the peer-flag vocabulary.
- Remove superseded component-local listeners, manual focus traversal,
  absolute menu/submenu geometry, full-screen help layer, and manual portal
  code after equivalent behavior is proven.
- Add focused component, interaction, geometry, accessibility, screenshot,
  build, and CSP evidence and update this tactical plus `web-ui-design` with
  actual results.

## Non-Goals

- Adding a context menu to a production row, changing row/current/batch target
  policy, or adding new torrent/file/peer/tracker commands.
- Reassigning touch or pen long press, adding swipe actions, or replacing the
  visible More affordance.
- A command palette, menubar, select, combobox, autocomplete, date picker,
  tooltip campaign, toast system, or general design-system rewrite.
- Migrating Settings, Add Torrent, Remove Torrent, or other modal dialogs
  unless a same-boundary regression requires a small compatibility fix.
- Adopting React Spectrum styling, importing its starter-kit source or assets,
  or changing RSTorrent's colors, density names, icons, or product language.
- Direct use of the native Popover API, CSS anchor positioning, Floating UI,
  Radix, or another overlay package alongside React Aria Components.
- Rust, engine, session, gateway, generated API, transport, persistence,
  Android Compose, Tauri shell, public network, or physical-device changes.

## Implementation Sequence And Gates

1. Pin and audit the dependency, record the clean production bundle baseline,
   move the global interface-size projection to a portal-safe root scope, and
   prove first-render Compact/Standard/Spacious plus theme inheritance.
2. Build the shared action-menu wrapper and migrate File actions. Add the
   reported 456-pixel regression plus 320- and 390-pixel edge geometry before
   migrating another consumer.
3. Migrate torrent More and its submenu. Prove pointer, keyboard, disabled,
   async action, nested Escape, focus restoration, and collision behavior.
4. Build the popover-dialog/help variants and migrate Columns and column help.
   Prove outside dismissal, retained checkbox state, focus, peer legend, and
   layering with existing dialogs.
5. Exercise the same action-menu content through context-trigger mode in a
   focused harness without attaching new product behavior. Prove pointer-edge
   and keyboard-context invocation plus table long-press non-regression.
6. Remove superseded code, run the complete web validation matrix, record the
   production bundle delta and installed license evidence, update the owning
   topic and this execution record, and commit the completed slice.

Each phase must keep the production build usable. Do not leave both old and
new positioning/dismissal paths active for one migrated overlay.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Appearance | Stored initial and live Compact/Standard/Spacious plus Auto/Light/Dark apply identically inside the application and body portal. |
| Action semantics | Pointer/touch/Enter/Space/Arrow open, first/last focus, Arrow/Home/End/typeahead, disabled actions, selection, Escape, Tab, outside press, trigger toggle, and async focus return. |
| Nested menu | Pointer and keyboard open, collision flip/shift, one submenu at a time, inner then outer Escape, action close, and focus restoration. |
| Popover dialog/help | Columns checkbox/reset persistence, outside/Escape/trigger close, focus return, peer legend content, and no swallowed or duplicate outside action. |
| Geometry | Open overlays remain within an eight-pixel safe boundary at 320x568, 390x844, 456x1024, compact, 1440x900, all four trigger corners, scroll containers, resize, and content growth. |
| Lifecycle | Anchor removal, tab/torrent/navigation change, responsive pane hiding, virtualized unmount, trigger disable, and owner unmount remove every portal/listener/observer. |
| Context capability | Same action content opens by desktop context pointer and keyboard, clamps at edges, dismisses correctly, and does not alter touch long press or product targeting. |
| Accessibility | Roles/names/states, focus order, keyboard-only paths, touch-oriented paths, and open-overlay serious/critical Axe scans pass. |
| Visual | Open File actions, torrent submenu, Columns, and help screenshots are retained in Light and Dark at wide and phone viewports. |
| Build | `npm run typecheck`, `npm test`, `npm run build`, focused Playwright, full deterministic Playwright, CSP check, `git diff --check`, bundle delta, and resolved license inventory pass. |

Component and browser tests must assert geometry of the menu items or their
text, not merely that a menu node exists or has an accessible name. Phone
screenshots must be captured with each relevant overlay open.

Rust workspace, protocol interoperability, public swarms, visible Tauri,
Android builds, emulators, and physical devices are not relevant to this
frontend-only slice.

## Escalation And Next Boundary

Implementation may choose local component names, CSS Module organization,
controlled versus uncontrolled open state per consumer, exact React Aria
composition, and ordinary same-boundary refactoring without direction. The
accepted dependency install and lockfile update are authorized by this
tactical.

Stop if evidence requires a second overlay/UI dependency, a full styled UI
framework, a new product context-menu target policy, a touch-gesture change, a
modal-dialog redesign, a browser-support reduction, CSP weakening, copied
third-party source/assets, or a material expansion outside frontend overlay
behavior.

The next product boundary is deciding which rows, cards, or other contexts
should expose right-click actions and how invocation interacts with current
and batch selection. Broader tooltip, toast, command-palette, modal, and form
widget unification likewise remains evidence-driven follow-up rather than
implied work.
