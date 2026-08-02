# Color Theme Settings

Status: Complete (2026-08-02).

Topics: `web-ui-design`, `desktop-inspection-surface`

## Motivation

The React inspection surface already has complete light and dark token palettes,
but the active palette is controlled only by the operating-system preference.
Users need an explicit Auto, Light, or Dark choice in the existing Settings
surface. The choice must apply immediately, survive reload, honor subsequent
system changes while Auto is selected, and avoid a light flash when a persisted
Dark preference starts.

The existing browser-local appearance record contains only Interface size at
version 1. Adding theme must preserve that preference rather than creating two
settings records that can overwrite one another or discarding an existing size
selection during migration.

## Stable Scenarios

- A fresh application starts in Auto and follows the current system color
  scheme.
- Auto reacts when the system changes between light and dark without a reload.
- Explicit Light stays light under a dark system preference, and explicit Dark
  stays dark under a light system preference.
- Theme changes apply immediately from Settings and survive reload alongside
  the selected Interface size.
- An existing version-1 appearance record retains its valid Interface size and
  gains Auto during migration.
- Malformed, unknown, future-version, and inaccessible storage use safe field
  defaults without preventing the application from starting.
- The browser advertises both schemes for Auto and the matching single scheme
  for an explicit choice so built-in controls match the initial palette.

## Scope

- Add typed `auto`, `light`, and `dark` color-theme values, with Auto as the
  default and serialized spelling.
- Replace size-only serialization with one version-2 appearance preference that
  owns Interface size and Color theme together.
- Read version-1 records as a compatibility migration, preserving valid size
  and defaulting theme to Auto.
- Apply the stored theme attribute before loading the inspection application,
  then keep it synchronized with the React store during live changes.
- Extend the existing Appearance settings with an accessible Color theme radio
  group and immediate application.
- Make the CSS palette obey explicit Light/Dark and use
  `prefers-color-scheme` only for Auto.
- Add deterministic persistence, migration, component, browser, visual, and
  accessibility evidence.
- Update the owning topic and tactical index.

## Non-goals

- Custom colors, user-authored palettes, high-contrast mode, contrast sliders,
  per-torrent themes, or a theme marketplace.
- Synchronizing presentation preferences between browsers or clients.
- Changing engine state, application contracts, Tauri commands, Android UI, or
  legacy web UI behavior.
- Adding a theme framework, CSS-in-JS library, external dependency, or native
  operating-system preference API.
- Reworking the existing light or dark visual language beyond corrections
  required for equivalent readability and accessibility.

## Reference Dossier

This work changes presentation only. No BitTorrent specification, libtorrent
source, protocol fixture, or interoperability oracle governs it.

Current RSTorrent sources:

- `clients/web/src/inspection/appearance.ts` owns the version-1 Interface size
  record and storage-failure boundary.
- `clients/web/src/inspection/state.ts` owns live presentation state and
  immediate persistence.
- `clients/web/src/inspection/global.css` has complete light defaults and a dark
  palette currently selected by an unconditional system media query.
- `clients/web/src/main.ts` selects the inspection or legacy entry point through
  dynamic imports, providing a pre-React point for stored-theme application.
- `clients/web/src/inspection/components/SettingsDialog.tsx` owns the existing
  accessible Appearance radio UI and focus containment.

Local JSTorrent revision `9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/ui/src/hooks/useAppSettings.ts` defines a system/light/dark theme
  preference and applies it separately from torrent state.
- `packages/client/src/components/SettingsOverlay.tsx` presents theme alongside
  other browser-local appearance settings.

RSTorrent adopts the product boundary and three-state behavior, names the
automatic choice **Auto**, and independently authors its TypeScript, React,
CSS, migration, and tests. No source or asset is copied.

## Accepted Design

The user-facing setting is **Color theme** with these choices:

| Choice | Behavior |
| --- | --- |
| Auto | Follow `prefers-color-scheme`, including changes while running |
| Light | Always use the light token palette |
| Dark | Always use the dark token palette |

The document root carries `data-color-theme="auto|light|dark"`. Light tokens
remain the CSS defaults. A dark system media query targets only Auto (and an
absent attribute as a defensive startup fallback); an explicit Dark selector
uses the same dark token values outside the media query. CSS therefore owns
live system observation without a JavaScript listener or lifecycle task.

The version-2 browser-local value is one object:

```json
{"version":2,"interfaceSize":"standard","colorTheme":"auto"}
```

Parsing validates the two fields independently. A valid version-1 size is
migrated in memory with Auto and is rewritten as version 2 on the next user
change. Unknown or future versions use all defaults. Persistence remains
best-effort and presentation-only.

The inspection path reads and applies the stored theme synchronously before its
dynamic React import. The application store reads the same validated value,
and the App keeps the root attribute synchronized after any live action. The
legacy entry point does not receive or interpret this setting.

## Invariants And Bounds

- `ColorTheme` has exactly three accepted serialized values; arbitrary storage
  input never reaches the root attribute.
- Auto is the code, storage-fallback, migration, and visual default.
- Interface size and theme are always saved together from one current store
  snapshot, so changing either cannot erase the other.
- Version-1 compatibility accepts only its known version and a valid size;
  future records are not guessed at or partially interpreted.
- Storage denial, malformed JSON, and write exceptions never prevent startup or
  a live in-memory setting change.
- Explicit Light and Dark are independent of system media changes. Auto has no
  JavaScript event listener, task, timer, or cancellation requirement.
- The theme attribute changes presentation only: desired Rust views, torrent
  selection, detail tabs, table scroll state, and commands remain unchanged.
- Both palettes retain visible focus, status meaning beyond color alone, and no
  serious or critical automated accessibility findings in representative
  desktop and phone layouts.

## Ownership And Data Flow

```text
versioned browser-local appearance preference
  -> main inspection bootstrap -> validated root theme attribute before React
  -> createInspectionStore -> interfaceSize + colorTheme
       -> Settings radio action
            -> update current store snapshot
            -> best-effort save both appearance fields
       -> App root-attribute synchronization
            -> CSS explicit Light/Dark selector
            -> CSS Auto system media query
```

The appearance module owns enum validation, defaults, migration, serialization,
and the small DOM attribute operation. The Zustand application instance owns
the current typed values. Settings dispatches actions but does not access
storage or system media. CSS owns palette resolution and live Auto changes.
There are no new background tasks or cancellation paths.

## Shape-Changing Edge Cases

- no record, valid version-1 record, valid version-2 record, malformed JSON,
  invalid fields, unknown/future version, and throwing read/write storage;
- selecting theme before and after changing Interface size, then reloading;
- persisted Dark at initial inspection bootstrap;
- system dark at first Auto load and both live system transitions;
- explicit Light under system dark and explicit Dark under system light;
- Settings focus trapping with the additional radio group;
- desktop and phone sheets at Compact, Standard, and Spacious sizes; and
- native form controls, focus rings, menus, tables, logs, and status colors in
  both effective palettes.

## Implementation Order

1. Add unified typed appearance parsing/serialization, version-1 migration,
   theme application, and unit coverage.
2. Extend presentation state and actions so either setting persists a complete
   current version-2 record.
3. Apply stored theme before inspection bootstrap and synchronize the root
   attribute from App state.
4. Add Color theme controls to the existing Settings Appearance section.
5. Gate dark CSS tokens by explicit Dark or Auto system preference and declare
   both browser color schemes.
6. Add component and browser coverage for migration, live switching, system
   changes, persistence, focus, responsive layout, and accessibility; inspect
   representative captures.
7. Record exact evidence, update the topic and index, run proportional frontend
   and repository gates, and commit the complete slice.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Preference | defaults, version-1 migration, every valid theme, independent field validation, future/malformed values, and throwing storage |
| Store/component | immediate root attribute, complete-record persistence, reload restoration, and Settings radio/focus behavior |
| Browser behavior | Auto under both media values and live media changes; explicit override in the opposite system scheme |
| Responsive visual | representative Light/Dark desktop and phone Settings/application captures without clipping or overlap |
| Accessibility | labeled theme group, keyboard access, visible focus, and no serious/critical axe findings in both palettes |
| Repository | frontend formatting, TypeScript, unit tests, production build, browser suite, and proportional workspace gates |

No live swarm, engine process, visible Tauri window, Android build, emulator, or
physical device is required. The deterministic demo surface exercises the same
React and CSS implementation used by live browser and Tauri delivery.

## Stopping Condition

This slice is complete when Settings exposes Auto, Light, and Dark; Auto tracks
system changes; explicit choices override them; the choice applies immediately
and starts without an avoidable wrong-theme React paint; version-1 Interface
size is preserved; both appearance settings round-trip together; representative
browser/accessibility evidence passes; the owning docs record actual evidence;
and the work is committed with a clean tree.

## Escalation Contract

The preference migration, root attribute, Settings controls, CSS selector
changes, tests, captured headless evidence, and topic/tactical updates are
authorized. Stop for direction if evidence requires a new dependency, custom
palette system, cross-client synchronization, durable application-contract
change, legacy UI rewrite, native platform work, or visible/physical client
launch.

## Implementation And Evidence

The browser-local appearance record is now version 2 and contains both the
typed Interface size and Color theme. The reader migrates a valid version-1
size to Auto, validates version-2 fields independently, rejects future or
malformed records, and treats unavailable storage as optional. Either store
action saves one complete current snapshot, so changing size after theme or
theme after size cannot reset the other field.

Auto, Light, and Dark now appear as an immediately applied radio group above
Interface size in the existing Appearance sheet. The Zustand presentation
state owns the typed selection. App synchronizes it to `data-color-theme` on
the document root with a layout effect; dialog open, focus trap, Escape, close,
focus restoration, and full-width phone behavior remain unchanged.

The inspection branch in `main.ts` validates and applies the stored attribute
before its dynamic React import, and changes the document color-scheme metadata
to Light/Dark support for Auto or the matching scheme for an explicit choice.
The legacy entry path retains its prior Dark declaration and receives no
appearance attribute. Headless instrumentation observed persisted Dark already
present when React inserted the first application content after reload.

Light remains the base palette. Explicit Dark selects the dark tokens outside
media queries; only Auto (plus an absent-attribute defensive fallback) selects
them through `prefers-color-scheme`. Consequently live operating-system media
changes require no JavaScript observer, timer, task, or cleanup. Explicit Light
under system Dark and explicit Dark under system Light remain stable.

First-time serious contrast evidence in explicit Dark exposed the demo
scenario strip's hard-coded light foregrounds and the bright primary-action
fill. The strip now uses semantic theme tokens for its background, border,
labels, badge, controls, clock, and message. A separate high-contrast action
fill preserves the brighter accent used for progress and focus semantics.
Both explicit palettes now have no serious or critical axe findings in the
representative Settings/application view.

Deterministic tests cover defaults, all nine accepted size/theme combinations,
version-1 migration, independent field fallback, future and malformed records,
throwing storage, root application, complete store persistence, component
selection, and reload restoration. Headless Chrome proves Auto at both system
schemes and through a live media transition, both opposite-system explicit
overrides, complete local-storage content, pre-content persisted theme,
Settings focus behavior, desktop and phone geometry, and empty serious or
critical axe results. Auto-Dark, explicit Light, explicit Dark, Standard,
Compact, Spacious, and phone captures were inspected without clipping,
overlap, illegible icons, or incoherent palette application.

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

TypeScript passed. All 91 frontend tests passed and two environment-specific
tests remained skipped. The production Vite build passed. Twelve deterministic
headless Chrome tests passed and the three opt-in live-engine cases remained
skipped. Rust formatting remained clean. No public network, live engine,
visible browser or Tauri client, Android build, emulator, physical device, new
dependency, or generated-contract change was used.
