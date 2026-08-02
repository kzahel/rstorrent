# Tactical 055: Application Destinations

Status: Complete on 2026-08-02.

Topics: `application-interface-direction`, `web-ui-design`,
`desktop-inspection-surface`, `application-view-api`, `client-surfaces`

## Motivation And Outcome

The shared React application is a capable torrent inspection surface, but it
still presents that one dense hierarchy as the whole product. The accepted
application direction instead gives content browsing, ordinary transfer
control, and deep torrent inspection distinct first-class destinations.

Implement the initial complete presentation shell with Library, Transfers,
and Workbench in the existing application header. Preserve the current
interface as Workbench, add a clean virtualized multi-select transfer queue,
and add a truthful content-card Library derived only from currently available
torrent facts. This slice establishes information architecture and continuity;
it does not pretend that media discovery or playback already exists.

## Stable Scenarios

- A fresh application opens Transfers. A later application instance restores
  the last selected destination and each destination's last local filter.
- The header exposes Library, Transfers, and Workbench as keyboard-operable
  primary navigation at wide, compact, and phone widths.
- Workbench retains the existing add and lifecycle toolbar, virtual torrent
  table, horizontal splitter, selected-torrent tabs, global diagnostics, and
  phone list-to-detail navigation.
- Transfers shows the same torrents through a simpler virtualized Name,
  Status, Progress, Rate, and Size queue without materializing detail views.
- Checkboxes and keyboard Space create one shared multi-selection across
  Transfers and Workbench. Row activation establishes one primary selection;
  Workbench activation also opens phone detail.
- Start, Pause, and a uniform Archive or Restore apply the existing semantic
  command once to every selected eligible torrent and report partial failure
  honestly. Removal remains single-torrent because multi-removal and mixed
  managed-data policy require a separate product decision.
- Select-all applies to the currently filtered virtual table rows and removal
  of torrents repairs both primary and multi-selection without stale IDs.
- Library presents torrent-backed content cards with names, actual transfer
  progress, size, availability wording, and an explicit Open in Workbench
  action. Generated gradients and initials are placeholders, not artwork.
- Library never displays Play, duration, resolution, watched state, media
  type, or readiness claims that the current application projections cannot
  establish.
- Contextual sidebars change their heading, labels, counts, and active filter
  with the destination. They never duplicate top-level application
  navigation.
- Library and Transfers request only the torrent collection projection.
  Workbench retains the current responsive selected-summary/detail leasing.
- The 2,001-torrent demo keeps both tables and the Library card grid bounded
  rather than creating one DOM subtree per logical item.

## Scope

- Add typed browser-local destination and destination-filter preferences with
  Transfers as the validated fresh-install fallback.
- Add shared primary and multi-selection state plus exact repair on snapshot
  and keyed removal updates.
- Add the top-level primary navigation and responsive header behavior without
  a routing dependency.
- Make the existing sidebar contextual and give Library a separate,
  presentation-only content filter vocabulary.
- Preserve the existing detailed application as Workbench.
- Add a virtualized Library card grid with truthful placeholders and an
  explicit source-torrent handoff to Workbench.
- Add a virtualized clean Transfers table and generic virtual-table
  multi-selection support reusable by Workbench.
- Make existing toolbar commands selection-aware where policy is already
  unambiguous.
- Update semantic desired-view selection so clean destinations do not lease
  torrent detail, peer, file, tracker, piece, disk, or log projections.
- Add store, reducer, component, responsive browser, accessibility, bounded
  rendering, persistence, and production-build evidence.
- Update the owning topics, readiness roll-up, and tactical index with the
  exact result.

## Non-Goals

- A media catalog, file-type detection, metadata matching, external artwork,
  thumbnail extraction, watched state, duration probing, playback, streaming,
  or playback-oriented piece priorities.
- Importing PlaysVideo source, routes, persistence, CSS, metadata contracts,
  playback engine, or assets.
- New Rust views, commands, generated-contract fields, engine behavior, or
  durable application data.
- Multi-torrent removal, mixed Archive/Restore in one action, or transactional
  batch-command semantics.
- URL routing/history, deep links, keyboard shortcut policy, search, labels,
  queues, or a lightweight Library/Transfers inspector.
- Persisting selected torrents, engine replicas, scroll position, or open
  dialogs.
- Android Compose changes, visible Tauri validation, public networking, or
  physical-device work.

## Reference Dossier

This is presentation-only work. No BitTorrent specification or pinned
libtorrent source governs its state transitions, and no external source or
asset is copied.

The accepted local mockup is
`mockups/web-ui-direction/04-library-transfers-workbench.html`. The entire
mockup tree remains gitignored and is a discussion record rather than a build
dependency.

Local PlaysVideo revision
`c94c3604d19303a285149dd2e90491dc57ee08cd` supplies product-history lessons:

- `app/src/components/CatalogEntry.tsx` uses deterministic initials when
  actual thumbnail or metadata artwork is absent and keeps playback wording
  tied to a real catalog/playback view.
- `app/src/components/CatalogListView.tsx` distinguishes content-oriented
  name, episode, duration and watch status from transport mechanics.
- `app/src/components/Sidebar.tsx` keeps catalog navigation and now-playing
  state separate from media rows.

RSTorrent adopts only the lesson that missing media enrichment needs an honest
fallback. It deliberately renders current torrent-backed content sources and
does not import PlaysVideo's media facts, persistence, links, or Play actions.

Local JSTorrent revision
`9895410beeed6aff554053769bd006a3fbd373ef` remains the traditional Workbench
reference through `packages/ui/src/tables/TorrentTable.tsx`,
`packages/ui/src/components/DetailPane.tsx`, and
`packages/client/src/AppContent.tsx`. RSTorrent preserves its already
independently authored table/detail hierarchy rather than copying source.

## State, Ownership, And Data Flow

```text
versioned browser-local navigation preference
  -> createInspectionStore presentation state
       destination
       library filter
       Transfers torrent filter
       Workbench torrent filter
  -> primary nav + contextual Sidebar
  -> LibraryView | TransfersView | existing Workbench

one materialized Rust torrent collection
  -> shared primary torrent ID + bounded selected ID list
  -> Library cards / Transfers table / Workbench table
  -> existing one-torrent semantic commands

destination + responsive Workbench navigation
  -> InspectionController desired views
  -> existing leased application view set
```

The per-application Zustand store owns destination, filters, selection, and
Workbench presentation state. A small navigation-preference module owns
validation and best-effort persistence. React components own only transient
form, menu, dialog, scroll, and virtual-viewport state. No task, transport, or
engine owner changes.

## Invariants And Bounds

- The destination enum contains exactly Library, Transfers, and Workbench;
  unknown or future persisted values fall back to Transfers.
- Navigation persistence contains presentation values only. Storage denial,
  malformed JSON, or a future version cannot prevent startup.
- Each destination has one independent filter value. Switching destinations
  does not reinterpret or overwrite another destination's filter.
- Selected IDs are unique and always refer to currently materialized torrents.
  The primary ID is either null or a member of that selected set.
- A normal row/card activation replaces selection with that torrent. Checkbox
  and keyboard Space toggle membership. Select-all is bounded by the current
  already-materialized filtered collection.
- A clean-destination selection never opens or leases Workbench detail.
  Explicit Open in Workbench selects the source and opens phone detail.
- Workbench's splitter, active detail tab, columns, density, and diagnostics
  retain their current owners and semantics.
- Multi-command dispatch uses existing one-torrent commands sequentially,
  reports completed and failed counts, and never claims atomic application.
- Removal remains disabled when more than one torrent is selected.
- Library card vocabulary derives only from `TorrentRow`; complete means the
  torrent reports complete, not that RSTorrent has proven browser playback.
- Card virtualization renders visible rows plus at most two overscan rows.
  Existing virtual table overscan remains unchanged.
- Primary navigation and pane controls retain semantic button, navigation,
  current-page, grid, progress, separator, and dialog behavior.

## Shape-Changing Edge Cases

- missing, malformed, old, or future navigation preference data and storage
  get/set exceptions;
- switching destinations while a sidebar drawer, Workbench phone detail,
  selected detail tab, or multi-selection is active;
- a selected torrent hidden by a destination filter without deleting shared
  selection truth;
- select-all followed by filter changes, snapshot replacement, keyed row
  removal, archive, or remove commands;
- zero, one, mixed-state, archived, removal-pending, and failed selections;
- a partial multi-command failure after earlier commands succeeded;
- metadata-pending torrents with null size and progress;
- missing or stale collection materialization and an empty live engine;
- 2,001 logical Library cards or transfer rows at every interface size;
- header wrapping, contextual drawer closure, and Workbench detail at 390
  pixels; and
- Light, Dark, system Auto, reduced motion, coarse pointer, browser zoom, and
  serious/critical accessibility scanning.

## Implementation Order

1. Add the navigation preference and destination-local state, selection
   transitions, reducer repair, and deterministic tests.
2. Add header primary navigation, contextual Sidebar behavior, and
   destination-aware view leasing while leaving Workbench structurally intact.
3. Add generic bounded multi-selection to `VirtualTable`, connect it to the
   existing Workbench table, and make toolbar commands selection-aware.
4. Add the clean Transfers table and toolbar composition through the same
   rows, selection, and commands.
5. Add the bounded truthful Library card grid and explicit Workbench handoff.
6. Adapt wide, compact, phone, coarse-pointer, empty, stale, and large-scenario
   presentation; correct semantics and focus behavior revealed by testing.
7. Run proportional frontend and repository gates, inspect deterministic
   screenshots, update the tactical and owning topics, and commit logical
   slices.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Preference/store | Fresh default, round trip, invalid/throwing storage, independent filters, selection toggle/replace/repair |
| Controller | Library/Transfers collection-only views and unchanged responsive Workbench detail views |
| Components | Primary navigation, contextual sidebars, truthful cards, shared multi-selection, select-all, single removal, command results, Workbench handoff |
| Bounded rendering | 2,001 torrents retain bounded transfer rows and Library cards at representative viewports |
| Responsive browser | Wide Library, Transfers and Workbench; compact navigation; phone destinations, filters and Workbench detail/back flow |
| Accessibility | Current-page primary nav, contextual labels, multi-select grids, progress semantics, keyboard flow, focus behavior, and empty serious/critical axe findings |
| Repository | TypeScript typecheck, frontend tests, production build, headless demo suite, and Rust formatting check |

No public swarm, live-engine run, visible Tauri application, Android build,
emulator, physical device, new dependency, generated-contract change, or
external asset is required.

## Stopping Condition

This slice is complete when Library, Transfers, and Workbench are genuine
responsive primary destinations; Workbench retains the current detailed
surface; sidebars and filters are contextual and independently restored;
Transfers and Workbench share bounded multi-selection; Library is useful and
truthful without media enrichment; clean destinations lease no details; the
large scenario remains bounded; unit, browser, accessibility, build, and
documentation evidence passes; and the changes are committed in reviewable
logical slices.

## Escalation Contract

The presentation preference, Zustand state, component extraction, primary
navigation, contextual sidebar, virtual table selection, Library/Transfers
components, sequential multi-command handling, responsive corrections, tests,
topic/readiness updates, and logical commits are authorized. Stop for human
direction if implementation requires new durable application data, a generated
contract or engine change, a media/playback claim, an external dependency or
asset, multi-remove policy, URL/router policy, Android changes, public network
access, or a visible/physical client.

## Implementation And Evidence

The implementation remains entirely inside the shared web presentation and
its living documentation:

- `navigation.ts` owns validated versioned destination and independent-filter
  persistence with Transfers as the fresh fallback.
- The per-instance store owns destination, primary selection, and shared
  multi-selection; snapshot and keyed-removal reduction repair selection
  without stale or duplicate torrent IDs.
- The header and contextual Sidebar expose the three destinations across wide
  and phone layouts. The prior interface is preserved as Workbench.
- `TransferTable` and the Workbench torrent table share generic virtual-table
  checkbox, select-all, and keyboard-Space selection. `TorrentActions`
  composes the existing add/remove UI and applies eligible Start, Pause, and
  uniform Archive or Restore commands sequentially. Removal remains enabled
  only for one selected torrent.
- `LibraryView` virtualizes torrent-backed cards with deterministic generated
  placeholders, actual summary facts, and an explicit source handoff to
  Workbench. It makes no media or playback claim.
- Destination-aware desired views retain only the torrent collection for
  Library and Transfers and preserve the prior responsive detail logic for
  Workbench.

Validation on 2026-08-02:

- `npm run typecheck` passes.
- `npm test` passes all 106 executed tests with 2 expected skips.
- `npm run build` passes the production Vite build.
- `npm run test:e2e` passes all 15 deterministic browser tests with 3
  live-engine tests deliberately skipped. Wide and phone destination flows
  have no serious or critical axe findings.
- The 2,000-torrent browser scenario retains fewer than 100 Workbench rows,
  100 Transfer rows, and 100 Library cards, with fewer than 2,000 total DOM
  elements after destination changes.
- Wide Transfers, Library, and Workbench plus phone Library screenshots were
  inspected from a temporary directory and were not added to the repository.
- `cargo fmt --all -- --check` and `git diff --check` pass.

No generated contract, Rust engine, Android, dependency, media asset, or
mockup image changed in this tactical.
