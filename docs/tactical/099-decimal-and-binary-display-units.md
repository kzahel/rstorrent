# Decimal And Binary Display Units

Status: Complete (2026-08-06).

Topics: `web-ui-design`, `desktop-inspection-surface`

## Motivation

The shared React product currently formats byte counts and rates with binary
scaling and IEC suffixes such as `KiB`, `MiB`, and `GiB`. Those labels are
precise, but they add visual noise to the ordinary Library, Transfers, and
Workbench experience. Consumer torrent clients more commonly use the shorter
`kB`, `MB`, and `GB` vocabulary.

The accepted product direction is to make decimal units the fresh-install
default while retaining binary units as a browser-local presentation choice:

- **Decimal** uses powers of 1000 and `kB`, `MB`, `GB`, `TB`, and `PB`.
- **Binary** uses powers of 1024 and `KiB`, `MiB`, `GiB`, `TiB`, and `PiB`.

This is a display preference only. The application service and engine continue
to expose exact byte counts and bytes-per-second rates without selecting or
persisting presentation units.

## Stable Scenarios

- A fresh browser presentation displays ordinary sizes and rates in Decimal
  units, including `kB/s`, `MB/s`, and `GB/s`.
- Settings exposes exactly Decimal and Binary data-unit choices, with Decimal
  selected by default.
- Changing the choice immediately reformats every currently visible size,
  total, progress-byte, and rate value without reconnecting, changing leased
  views, or issuing an application command.
- The selected choice survives reload together with Interface size and Color
  theme.
- Existing version-1 and version-2 appearance records preserve their valid
  fields and acquire Decimal during migration.
- Malformed, unknown, future-version, and inaccessible browser storage use
  safe independent defaults and do not prevent startup or live changes.
- Binary remains semantically exact: 1024 bytes is `1.0 KiB` and 1,048,576
  bytes is `1.0 MiB` under the existing precision policy.
- Decimal uses actual decimal magnitudes rather than merely relabeling binary
  values: 1000 bytes is `1.0 kB` and 1,000,000 bytes is `1.0 MB`.
- Exact engineering and protocol copy such as `16 KiB` block geometry or a
  `64 MiB` parser limit remains IEC text and does not change with the
  presentation preference.

## Scope

- Add a typed browser-local Data units preference with exactly `decimal` and
  `binary` serialized values and Decimal as the default.
- Extend the existing versioned appearance record rather than adding a second
  storage key or application setting.
- Migrate known version-1 and version-2 appearance records without losing
  valid Interface size or Color theme values.
- Add an accessible Data units radio group to the existing Appearance section
  of the shared Settings sheet.
- Apply the selected unit system to all generic byte-count and byte-rate
  formatting in the shared React product, including Library, Transfers,
  Workbench summaries and tables, Files, Peers, Disk, DHT, Speed totals, chart
  labels, and the global session rates.
- Preserve exact integer-string formatting beyond JavaScript's safe integer
  range while making it obey the selected unit system.
- Rename the current `formatDecimalBytes` concept so its name describes its
  decimal-string input representation rather than colliding with the new
  Decimal unit-system vocabulary.
- Add deterministic formatter, preference, store, component, browser,
  responsive, and accessibility evidence.
- Update the owning topics and this tactical with actual implementation
  evidence when the slice lands.

## Non-goals

- Changing engine counters, application DTOs, generated contracts, view-set
  schemas, database state, or torrent persistence.
- Synchronizing the preference between browsers, Tauri webviews, browser
  extension presentations, profiles, or devices.
- Adding the preference to Android Compose, the experimental Android
  bootstrap, CLI reports, scripts, benchmarks, or documentation prose.
- Adopting JSTorrent's hybrid convention of powers of 1024 labeled `KB`, `MB`,
  and `GB`.
- Rewriting fixed technical limits, recorded diagnostics, demo log prose,
  protocol terminology, or implementation comments according to a UI
  preference.
- Changing the existing precision, significant-digit, unavailable-value,
  zero-rate, or percentage policy except where selecting a different unit base
  necessarily changes the scaled value.
- Locale-sensitive decimal separators, localized unit names, user-authored
  unit labels, bit rates, compact no-space forms, or automatic operating-system
  unit detection.
- Adding a formatting or internationalization dependency.

## Reference Dossier

No BitTorrent specification governs presentation units. The reference work is
therefore client/API boundary and product-behavior evidence rather than a
protocol oracle.

Pinned libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` (`v2.0.13`):

- `include/libtorrent/torrent_status.hpp` exposes totals as integer byte
  counts and explicitly defines transfer rates as bytes per second. It exposes
  no unit-display policy or preference.
- `bindings/python/src/torrent_status.cpp` carries those raw numeric fields
  through the Python binding without formatting them.
- `examples/print.cpp::add_suffix_float` is presentation-owned example code. It
  divides by 1000 and emits `kB`, `MB`, `GB`, `TB`, and `PB`.
- `examples/torrent_view.cpp` applies that helper to totals and rates, while
  `bindings/python/simple_client.py` independently divides rates by 1000 and
  labels them `kB/s`. The examples are not a uniform public formatting API,
  but they demonstrate that compact Decimal presentation belongs in clients.

Local JSTorrent revision
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/ui/src/utils/format.ts` divides by 1024 and emits `KB`, `MB`, `GB`,
  and `TB` throughout the web/desktop product.
- `android/app/src/main/java/com/jstorrent/app/util/Formatters.kt` uses the same
  binary arithmetic and conventional labels in Android.
- Neither presentation exposes a unit-system setting.

Current RSTorrent sources:

- `clients/web/src/inspection/format.ts` owns the shared number-backed and
  exact decimal-string-backed byte formatters plus rate composition. Both use
  powers of 1024 and IEC suffixes today.
- The formatter is consumed across ten shared React component modules; the
  engine and application boundary provide raw values rather than display
  strings.
- `clients/web/src/inspection/appearance.ts` owns the version-2 browser-local
  Interface size and Color theme record, validation, defaults, migration, and
  optional-storage boundary.
- `clients/web/src/inspection/state.ts` owns live presentation state and saves
  one complete appearance snapshot after either existing preference changes.
- `clients/web/src/inspection/components/AppearanceSettingsSection.tsx` owns
  the two existing accessible Appearance radio groups.
- `clients/web/src/inspection/torrentFile.ts`,
  `clients/web/src/inspection/components/DiskPanel.tsx`, and named demo log
  fixtures include literal IEC text describing exact limits or engine
  geometry. Those strings are technical content, not generic formatted data.

RSTorrent independently authors the implementation. No libtorrent or
JSTorrent source, fixture, or asset is copied.

## Accepted Design

The user-facing setting is **Data units**:

| Choice | Scale and suffixes | Intent |
| --- | --- | --- |
| Decimal | 1000; `kB`, `MB`, `GB`, `TB`, `PB` | Short conventional consumer display; fresh-install default |
| Binary | 1024; `KiB`, `MiB`, `GiB`, `TiB`, `PiB` | Explicit powers-of-1024 display |

`B` remains the base suffix in both modes. The formatter selects the largest
unit whose divisor does not exceed the nonnegative value, capped by the final
supported suffix. Rates append `/s` after formatting the byte magnitude.
Existing precision rules remain in force independently of the selected base.

The appearance record advances to version 3:

```json
{
  "version": 3,
  "interfaceSize": "standard",
  "colorTheme": "auto",
  "dataUnits": "decimal"
}
```

Version 1 preserves a valid Interface size, gains Auto theme, and gains
Decimal units. Version 2 preserves valid Interface size and Color theme fields
and gains Decimal units. Version 3 validates all three fields independently;
an invalid Data units field falls back only that field to Decimal. Unknown
future versions use the complete current default rather than guessing their
shape.

The Data units choice is initialized with the rest of presentation state and
is passed explicitly into pure formatting behavior. It must not be an ambient
mutable module global: multiple application stores and tests may coexist in
one JavaScript realm. The implementation may use an explicit formatter value,
a small React context/hook, or another bounded local mechanism, but components
must not obtain the choice from engine state or repeatedly read browser
storage.

The current `formatDecimalBytes` name describes a decimal-encoded integer
string used to preserve exact large counters. With Decimal now naming base-1000
presentation, that name becomes misleading. Rename it to an exact-input name
such as `formatExactBytes` while retaining arbitrary-precision integer
arithmetic and the existing visible precision policy. Internal final naming is
an ordinary implementation choice.

Literal IEC strings continue to mean exact binary geometry and limits. The
preference affects values routed through generic byte/rate formatting; it does
not search and replace source strings or rewrite diagnostic records.

## Contracts And Invariants

- `DataUnits` accepts exactly `decimal` and `binary`; arbitrary persisted input
  never selects a divisor or suffix array.
- Decimal is the code default, fresh-install default, migration default, and
  invalid-field fallback.
- Decimal uses powers of exactly 1000 and lowercase `k` in `kB`; Binary uses
  powers of exactly 1024 and IEC suffixes. Neither mode silently uses the
  other's divisors.
- Raw byte counts and bytes-per-second rates remain authoritative across the
  Rust/application boundary. Formatted strings never flow back into commands,
  sorting, filtering, charts, progress calculations, or persistence.
- Table sorting and chart geometry continue to use raw numeric or exact
  integer values, not localized or formatted text.
- Number-backed and exact integer-string-backed paths use the same unit
  thresholds and suffix vocabulary. This slice retains their existing
  precision behavior rather than silently redefining rounding policy.
- Exact integer-string inputs remain arbitrary-precision through unit
  selection and scaling; values above `Number.MAX_SAFE_INTEGER` are not
  converted through `number` merely for display.
- Changing any appearance field saves Interface size, Color theme, and Data
  units together from the same current store snapshot, so one change cannot
  erase another.
- A live unit change alters presentation only. It does not change desired Rust
  views, view leases, transport traffic, torrent selection, active tabs,
  virtual-table identity, or application commands.
- No new task, timer, listener, network operation, cancellation path, or
  external dependency is introduced.
- Browser storage remains optional. Read or write failure cannot prevent
  startup or an in-memory unit change.

## Ownership And Data Flow

```text
versioned browser-local appearance preference
  -> appearance parser/default/migration
  -> per-application Zustand presentation.dataUnits
       -> Settings > Appearance > Data units radio action
            -> immediate store update
            -> best-effort complete version-3 appearance save
       -> byte/rate display components
            -> pure formatter(selected units, raw bytes)
            -> Decimal or Binary text

Rust engine/application views -> raw bytes and bytes/second ----------------^
```

The appearance module owns the enum, default, record version, parsing,
migration, and serialization. The per-application store owns the live choice.
The Settings section owns only accessible selection UI. The formatter owns
unit selection and text production. Existing component calculations retain
their current owners.

There are no background tasks and therefore no new cancellation or termination
map. Browser storage access remains synchronous, best-effort, and bounded to
one small record on a user preference change.

## Shape-Changing Edge Cases

- Decimal thresholds around 999/1000 bytes, 999,999/1,000,000 bytes, and the
  corresponding higher boundaries;
- Binary thresholds around 1023/1024 bytes, 1,048,575/1,048,576 bytes, and the
  corresponding higher boundaries;
- `0 B`, null/unavailable byte counts, and zero/unavailable rates under both
  choices;
- values at which the existing presentation changes from one decimal place to
  an integer display;
- exact decimal-string values at, below, and above `Number.MAX_SAFE_INTEGER`,
  including `TB`/`TiB` and `PB`/`PiB` selection;
- consistent unit selection for exactly representable inputs through both
  formatter paths;
- switching units while Library cards, virtual transfer/peer/file tables,
  Disk details, and Speed charts are mounted;
- changing Data units after Interface size or Color theme and vice versa;
- migration from every valid version-1 Interface size and every valid
  version-2 Interface size/Color theme combination;
- independently invalid version-3 fields, malformed JSON, future versions,
  missing storage, denied reads, and throwing writes;
- desktop and phone Settings layouts with the additional radio group; and
- technical literal IEC copy remaining stable while adjacent generic values
  follow the preference.

## Implementation Order

1. Add the typed unit system and pure number/exact-input formatter coverage,
   including threshold, consistent unit selection, arbitrary-precision, and
   rate cases.
2. Advance the appearance record to version 3 with version-1/version-2
   migration, independent field validation, complete-record serialization,
   and storage-failure coverage.
3. Add Data units to presentation state and one typed action that updates live
   state and persists the complete current appearance snapshot.
4. Add the accessible Decimal/Binary radio group to Appearance and prove
   immediate selection, persistence, focus behavior, and reload restoration.
5. Thread the selected units through every generic byte/rate display, rename
   the ambiguous exact-input formatter, and update deterministic expectations
   without altering literal technical IEC content.
6. Exercise representative Decimal and Binary Library, Transfers, Workbench,
   Files, Disk, DHT, and Speed surfaces, including live switching with mounted
   virtualized content.
7. Run responsive and accessibility checks, the complete frontend gates, and
   update the owning topics, tactical index, and this document with actual
   evidence.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Pure formatter | Decimal/Binary threshold tables, suffix casing, rates, current precision transitions in both input paths, consistent unit selection, and arbitrary-precision values |
| Preference | defaults, version-1/version-2 migration, version-3 round trip, independent invalid fields, malformed/future records, and throwing storage |
| Store/component | immediate live action, complete-record persistence in either change order, accessible radio names/state, Escape/focus behavior, and reload restoration |
| Product surfaces | representative sizes, totals, file progress, peer totals/rates, global rates, Disk/DHT counters, and Speed labels under both choices |
| Sorting/calculation | raw-value ordering and chart geometry remain identical across a display-only unit switch |
| Responsive/accessibility | desktop and phone Settings/application layouts, keyboard operation, visible focus, and no serious or critical automated findings |
| Frontend | formatting, TypeScript, Vitest, production build/CSP check, and proportional Playwright coverage |
| Repository | documentation links and focused source searches confirm no generated contract, Rust engine, Android, or technical-literal policy change |

## Implementation Outcome

The shared React presentation now owns one typed `DataUnits` value with
exactly `decimal` and `binary`. The browser-local appearance record is version
3; version-1 and version-2 records preserve every independently valid prior
field and acquire Decimal, while invalid current fields fall back
independently. All appearance actions persist one complete size, theme, and
unit snapshot, and denied storage still permits live in-memory changes.

The pure formatter accepts the selected unit system explicitly. Its
number-backed and arbitrary-precision integer-string-backed paths share the
same thresholds and suffix tables while retaining their prior rounding and
truncation behavior. The latter is now named `formatExactBytes`. Decimal uses
base 1000 through `PB`; Binary uses base 1024 through `PiB`; rates compose the
same magnitude with `/s`.

Settings exposes an accessible Data units group. The per-application Zustand
choice is threaded through session rates, Library, Transfers, General,
Files, Peers, Disk, DHT, and Speed summaries, tables, chart axes, samples, and
totals. Sorting, table identity, chart geometry, view leases, commands, and
raw application values remain independent of the formatted text. Literal
`64 MiB`, `16 KiB`, and diagnostic IEC strings remain technical copy.

The complete browser run also exposed an existing serious light-theme
contrast failure in the removal dialog. Its warning/error text now uses the
existing `danger-strong` text token rather than the lighter decorative danger
token; the isolated failing check and the complete suite then passed.

## Validation Evidence

The following ran from `clients/web` on 2026-08-06:

- Prettier checked every changed frontend source and test file successfully.
- `npm run typecheck` passed.
- `npm test` passed 31 Vitest files with 200 tests; 2 files and 2 tests remain
  intentionally skipped by their existing environment gates.
- `npm run build` passed the Vite production build and CSP bundle check. The
  existing large-chunk advisory remained non-fatal; both JavaScript bundles
  passed the no-eval, no-Function-constructor, and no-CommonJS-require check.
- `npx playwright test tests/inspection-demo.spec.ts` passed all 20 scenarios
  in headless Chrome. The new scenario covers Decimal default, Binary live
  switching, persistence, Library, Transfers, General, Files, Disk, DHT,
  Speed, stable transfer order and canvas geometry, phone Settings layout,
  fixed IEC copy, and no serious or critical axe findings.
- Focused source searches found no obsolete `formatDecimalBytes` caller and
  left only the intended technical IEC strings plus Binary setting/test copy.
  No generated contract, Rust engine, Android, persistence database, or
  application DTO changed.

No public swarm, libtorrent process, visible Tauri window, Android build,
emulator, physical device, generated-contract refresh, or external network
access ran, as none is required for this presentation-only slice.

No public swarm, libtorrent process, Rust interoperability run, visible Tauri
window, Android build, emulator, physical device, generated-contract refresh,
or network access is required. The deterministic demo surface exercises the
same React formatting and Settings implementation used by browser and Tauri
presentations.

## Stopping Condition

This slice is complete when Decimal is the fresh and migrated default;
Settings exposes persistent Decimal and Binary choices; every generic shared
React byte and rate display changes immediately and consistently; Decimal uses
base 1000 with `kB/MB/GB` while Binary uses base 1024 with
`KiB/MiB/GiB`; exact large values remain precise; sorting, calculations,
engine/application contracts, and literal technical IEC content remain
unchanged; representative responsive/accessibility and complete frontend
evidence passes; and the owning docs record what actually ran.

## Next-Slice Boundary

Android, CLI, extension-specific preferences, cross-presentation preference
synchronization, locale-aware number formatting, and any future consolidated
presentation-settings architecture remain separate decisions. This tactical
does not imply them.

## Escalation Contract

The version-3 browser-local migration, typed presentation action, pure
formatter refactor, shared Settings control, component plumbing, deterministic
tests, headless browser evidence, and proportional documentation updates are
authorized implementation work. Ordinary internal naming and component-local
refactoring do not require direction.

Stop for direction if evidence requires an engine or generated-contract
setting, cross-client synchronization, Android/CLI scope, a third unit policy,
localization or formatting dependency, changed fixed-limit/diagnostic wording,
or a precision policy change beyond the consequences of the selected base.
