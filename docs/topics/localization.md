# Localization

Topic: `localization`

Status: **Foundation complete as of 2026-09-01.** Tactical
[`204`](../tactical/204-cross-product-localization-foundation.md) completed
English extraction and qualification across the shared React product, Tauri,
Android, and iOS. English is the only supported locale. No multilingual
product claim is currently authorized.

## Scope And Owners

This topic owns supported-locale policy, fallback, terminology, translation
provenance, review expectations, and the repeatable catalog workflow. Product
copy stays in the platform catalog that presents it:

- React and Tauri use the packaged React message catalog and locale owner;
- Android Compose and platform integration use Android string and quantity
  resources; and
- iOS SwiftUI and platform integration use an Xcode String Catalog.

The typed Rust application contract remains language-neutral. It carries
semantic facts and stable error codes, never a selected locale, translated
sentence, or platform message key.

Machine-readable policy lives under `localization/`:

- `locales.json` is the authoritative shipping/test locale manifest and
  catalog path map;
- `glossary.json` owns shared product terminology and disambiguation;
- `provenance.json` records catalog source, license, and review state; and
- `source-classification.json` distinguishes presentation copy from user,
  protocol, persistence, diagnostic, fixture, and test text.

## Current Policy

- `en` is both the source locale and ultimate fallback. English source is
  maintained as normal product copy, not imported translation.
- `en-XA` and `ar-XB` are generated test-only pseudo-locales. They must never
  appear in a release locale manifest or product language picker.
- Locale follows browser/operating-system preferences. The application has no
  stored or synchronized language setting.
- Stable keys describe semantic intent rather than copying English or encoding
  screen position. User values are interpolation data, never keys or format
  templates.
- Product numbers, dates, times, durations, lists, plurals, and percentages use
  locale-aware platform formatting. Protocol values, hashes, endpoints,
  persisted values, exact byte geometry, and machine-readable timestamps keep
  their canonical forms.
- Rich text is exceptional. React messages may receive only explicitly
  supplied safe elements; platform catalogs do not embed arbitrary markup.
- Product UI may show a bounded verbatim technical detail beside a localized
  summary. Logs and diagnostic records remain stable technical evidence.

## Catalog Workflow

Run the repository checker before and after catalog work:

```bash
node scripts/check-localization.mjs
```

For React, edit semantic English source messages under
`clients/web/src/localization/messages`, regenerate the compiled AST catalog,
and run the source/catalog checker:

```bash
npm run generate:localization --prefix clients/web
npm run typecheck --prefix clients/web
node scripts/check-localization.mjs
```

The ordinary, companion, and remote builds all compile the same packaged
catalog. `VITE_RSTORRENT_ENABLE_PSEUDO_LOCALES=1` enables generated `en-XA`
and `ar-XB` only for tests; production builds cannot select them.

For Android, edit
`clients/android/app/src/main/res/values/strings.xml` and use quantities for
count-dependent copy. Android's debug build generates platform pseudo-locales;
release resources advertise only English. Run from the repository root:

```bash
clients/android/build.sh
clients/android/gradlew -p clients/android lintDebug \
  assembleDebugAndroidTest assembleRelease
clients/android/scripts/run-localization-matrix.py
```

The matrix creates and removes its own Pixel 6 API 28 and Pixel Tablet API 35
AVDs. It must not target an arbitrary attached ADB device.

For iOS, edit `clients/ios/App/Localization/Localizable.xcstrings` or
`clients/ios/App/Localization/InfoPlist.xcstrings`, preserve translator
comments and placeholder/plural shape, regenerate the Xcode project, and run
from the repository root:

```bash
clients/ios/scripts/generate-project.sh
clients/ios/scripts/check-localization-roundtrip.sh
clients/ios/scripts/run-localization-matrix.sh
clients/ios/scripts/archive.sh --unsigned /absolute/task/path/RSTorrent.xcarchive
```

The round-trip command uses Xcode to export English as `en.xcloc`, rejects any
extraction-time rewrite, imports into an isolated copy, and compares both
catalogs byte-for-byte. The simulator matrix creates and removes its own
iPhone and iPad. To hand a real locale to a translator, use Xcode's
`-exportLocalizations`/`-importLocalizations` flow and retain the reviewed
`.xcloc` provenance outside the product runtime; do not add translation
service code.

## Translation Cohort Workflow

A future translation cohort follows this sequence:

1. Select a BCP 47 locale from audience evidence, maintenance capacity,
   supported fonts/input/layout, and an identified native reviewer.
2. Add a provenance row naming the exact author/source, license, locale, and
   review state. Do not copy JSTorrent or third-party translations solely from
   their repository-level license.
3. Export the native platform catalogs with translator comments and screenshot
   context. Translation tools receive no runtime access and no private product
   data.
4. Import into a branch, run placeholder/plural/catalog validation, and review
   every message in context with a native speaker.
5. Run long-text, RTL where applicable, accessibility, platform lifecycle,
   build, and installed/simulator evidence before adding the locale to
   `supportedLocales` or platform release metadata.
6. Update support/store/release disclosure only after the same locale is
   actually packaged in the relevant product.

If a locale later loses complete coverage or native review, remove it from the
release manifest before removing its catalog. Never leave a selectable locale
that falls through to raw keys or materially incomplete English.

## Evidence And Open Work

The currently checked catalogs contain 1,234 React messages, 17 Tauri-native
messages, 414 Android strings plus 7 quantities, 172 iOS product messages,
and 6 iOS Info.plist messages. Tactical 204 records the foundation's exact
FormatJS pins and licenses, artifact deltas, API 28/35 and iPhone/iPad pseudo
matrices, archive evidence, and repository gates. Completed Tactical
[`206`](../tactical/206-android-jstorrent-feedback-handoff.md) adds three
context-commented Android feedback strings; the checker, lint, Android build,
focused API-35 route, and physical ChromeOS presentation pass without adding
a production locale.

Implementation-complete Tactical
[`207`](../tactical/207-android-safe-reset-and-clear-data.md) adds the Android
reset/clear confirmation, progress, failure, retry, and explicit keep-
remaining-files copy to this existing English catalog with translator comments.
The 421-resource checker, lint, all 20 Compose navigation cases on owned API
28/35 AVDs, and both service reset/recovery cases on those APIs pass. It does
not select a production locale; the tactical's destructive and physical
qualification gates remain independent of this catalog evidence.

A separate tactical is required to choose and ship the first real
non-English cohort. Android replacement localization remains open until that
cohort passes provenance, native review, lifecycle, layout, accessibility,
and release disclosure. Website, store, marketing, and support-copy
localization remain outside this owner until explicitly planned.
