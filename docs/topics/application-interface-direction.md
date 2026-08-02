# Application Interface Direction

Topic: `application-interface-direction`

Status: The desktop/web product-navigation direction is accepted from the
maintainer-selected mockup. Library, Transfers, and Workbench are distinct
top-level destinations. Workbench preserves the current dense inspection
surface as a first-class interface rather than hiding it behind an advanced
setting. No implementation tactical is active, and media catalog, metadata,
and playback behavior remain unimplemented.

## Purpose And Scope

RSTorrent's detailed interface is intentionally useful as a debugging and
inspection surface, but that density should not define every product task.
The application also needs a content-oriented experience that can eventually
serve the role explored by the sibling `playsvideo` project, plus a clean place
for ordinary torrent control.

This topic owns the desktop/web product information architecture:

- the top-level Library, Transfers, and Workbench destinations;
- the responsibility and vocabulary of each destination;
- the role of contextual sidebars and inspectors;
- continuity of selection and presentation state between destinations; and
- the boundary between accepted presentation direction and media behavior
  that still requires design and implementation evidence.

[`web-ui-design.md`](web-ui-design.md) continues to own React, CSS Modules,
responsive behavior, accessibility, rendering, and browser-local presentation
state. [`desktop-inspection-surface.md`](desktop-inspection-surface.md) owns why
detailed torrent observation is a product and maintainer requirement.
[`client-surfaces.md`](client-surfaces.md) owns the browser and Tauri hosts and
their platform lifecycle.

## Accepted Application Shape

Use the existing top application bar for primary navigation:

```text
Library   Transfers   Workbench
```

These are destinations, not merely different row sizes or arrangements of one
torrent list.

### Library

Library is content-centric. Its primary objects are recognizable media and
files that a person may want to browse, play, reveal, or organize. Artwork,
title, duration, resolution, watched state, and playback readiness are more
important here than tracker, peer, and piece mechanics.

The Library sidebar filters media concepts such as all media, movies, recently
added, ready to play, watched, and unwatched. It does not relabel torrent
status filters as a media library.

The mockup is directional, not evidence that playback is ready. RSTorrent does
not yet own a media catalog, metadata/artwork acquisition, watched history,
playback-oriented scheduling, or the future verified-range HTTP playback data
plane described in [`client-surfaces.md`](client-surfaces.md). The product must
not display content as playable until the responsible owner can establish that
truthfully.

### Transfers

Transfers is the clean operational queue. It emphasizes torrent name, state,
progress, rate, common commands, and multi-selection without requiring a dense
detail pane. Its sidebar uses torrent lifecycle filters such as all,
downloading, seeding, complete, paused, errors, and archived.

Transfers is not a reduced-capability replacement for Workbench. It is the
ordinary control surface for people who do not currently need protocol,
storage, or diagnostic detail.

### Workbench

Workbench preserves and extends the current detailed React interface. Its
recognizable default is the traditional torrent-client arrangement:

```text
full torrent table
──────────────────────── resizable splitter
selected-torrent detail tabs and global diagnostic views
```

The table retains sortable columns, multi-selection, transfer commands, and
high-density status. The lower surface retains General, Trackers, Peers,
Swarm, Files, Pieces, and other torrent-scoped views. Global Disk, Logs, DHT,
Speed, and related diagnostic surfaces also belong in Workbench rather than
requiring Activity to be a separate top-level destination.

Workbench is not a hidden "advanced mode" that transforms the rest of the
application. It is visible in primary navigation, may be a user's persistent
home, and can retain its own density, columns, splitter position, active tab,
and other presentation preferences.

## Sidebar And Inspector Semantics

Primary navigation belongs in the top application bar. The left sidebar is
contextual to the selected destination:

- Library filters media and viewing state;
- Transfers filters torrent lifecycle and organization; and
- Workbench filters the torrent working set while leaving detailed inspection
  in its table and lower pane.

A lightweight contextual inspector may complement Library or Transfers, but it
does not replace Workbench. When present, it is toggled by a normal pressed
toolbar button with `aria-pressed` semantics, visually analogous to a latched
pane button. It is not represented as a checkbox. Its placement and exact
scope remain open.

The Workbench detail pane remains a resizable part of the workbench layout.
Whether it can also be hidden is a local Workbench presentation choice, not the
application-level switch between clean and advanced experiences.

## Shared Selection And Continuity

All three destinations operate on the same application truth. They must not
create separate torrent identities, command meanings, or materialized engine
replicas merely because they present different tasks.

The presentation should preserve useful continuity:

- a selected media item can expose its source torrent and open it in Workbench;
- a selected torrent remains selected when moving between Transfers and
  Workbench when it is still visible;
- multi-selection remains available in Transfers and Workbench;
- destination-specific scroll, filter, columns, density, splitter, and tab
  state may be remembered independently; and
- returning to a destination restores its useful local context rather than
  resetting the whole application.

The default fresh-install destination and last-destination restoration policy
remain open. Power users must be able to live in Workbench without repeatedly
opting into it.

## Adaptive And Platform Direction

Wide desktop space uses the top bar for labeled primary navigation and leaves
the contextual sidebar available. At narrow widths, primary destinations may
collapse to recognizable icons, the sidebar may become a horizontal filter row
or drawer, and Workbench may stack the torrent table above its detail surface.
The destination model remains the same rather than becoming a separate mobile
web application.

This direction applies to the shared desktop/browser presentation. It does not
require the Android Compose client to adopt the same primary navigation,
Library cards, or Workbench density.

## Local Mockup Record

Exploratory and selected mockups are retained locally under the gitignored
`mockups/web-ui-direction/` directory. They are intentionally not repository
history and may be absent from another checkout.

The local sequence is:

1. `01-view-modes.html` and `01-*.png`: Finder-like Cards, Transfers, and
   Inspect presentations of one filtered collection.
2. `02-inspector-layouts.html` and `02-*.png`: independent Cards/Table and
   hidden/right/bottom inspector choices, including multi-selection.
3. `03-library-transfers-activity.html` and `03-*.png`: the first genuinely
   distinct Library, Transfers, and Activity destinations plus a pressed
   inspector button.
4. `04-library-transfers-workbench.html` and `04-*.png`: the accepted direction,
   replacing Activity with the first-class traditional Workbench.

The standalone HTML files retain their local interactions. PNG files capture
representative wide, alternate, and phone arrangements. The whole `mockups/`
tree is ignored so screenshots and generated preview wrappers cannot enter a
commit accidentally.

## Invariants

- Library, Transfers, and Workbench are distinct product destinations, not
  names for cosmetic table modes.
- Workbench preserves the current dense inspection capability as a first-class
  product surface.
- A clean default experience must not require removing advanced information or
  making it undiscoverable.
- A media item is never presented as playable unless application and storage
  owners can support that claim with verified content behavior.
- Presentation differences do not create separate command or engine truths.
- The contextual sidebar does not carry application-level navigation and local
  filtering as one ambiguous hierarchy.
- A pane toggle uses pressed-button semantics rather than a checkbox.
- Desktop/web direction does not imply automatic Android presentation parity.

## Non-Goals

- Implement the three destinations without a bounded tactical.
- Adopt `playsvideo` source, persistence, or metadata contracts implicitly.
- Select an external media metadata or artwork provider.
- Claim streaming playback, verified-range serving, or media readiness.
- Turn Workbench into a hidden preference or remove its detailed views.
- Add a remote daemon, playback server, or new transport as a presentation
  shortcut.

## Open Decisions

- The fresh-install destination and last-destination restoration policy.
- The durable media-catalog owner and relationship between torrents, files,
  playable media, and user organization.
- Metadata, artwork, watched-state, privacy, cache, and offline behavior.
- Playback application choice and the eventual verified-range data plane.
- Whether Library includes incomplete but not-yet-playable media, and how that
  state is communicated without a false readiness claim.
- The exact scope and placement of lightweight Library and Transfers
  inspectors.
- Workbench's default splitter position and whether its lower pane can be
  hidden.
- The exact home for global Disk, Logs, DHT, Speed, and future diagnostic
  surfaces inside Workbench.
- URL, history, keyboard shortcut, and accessibility semantics for primary
  destination changes.

## Recommended Next Work

Before changing the application shell, open one bounded tactical that:

1. preserves the current interface intact as Workbench;
2. adds the top-level destination and contextual-sidebar state model without
   duplicating application views;
3. proves selection, focus, responsive navigation, and preference restoration;
4. keeps Library and Transfers truthful if their first states are incomplete;
   and
5. records which additional application projections are actually required
   before a useful Library or clean Transfers surface can graduate.

Media catalog and playback work should follow a separate source- and
edge-case-driven tactical because it introduces integrity, storage,
persistence, metadata, privacy, and platform-lifecycle questions beyond shell
navigation.
