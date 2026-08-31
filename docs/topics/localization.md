# Localization

Topic: `localization`

Status: **Foundation implementation active as of 2026-08-31.** Tactical
[`204`](../tactical/204-cross-product-localization-foundation.md) owns the
first complete extraction and qualification across the shared React product,
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

## Translation Workflow

Run the repository checker before and after catalog work:

```bash
node scripts/check-localization.mjs
```

The completed Tactical 204 workflow also provides platform extraction,
pseudo-localization, and catalog checks. A future translation cohort follows
this sequence:

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

Tactical 204 records exact catalog counts, FormatJS pins and licenses,
artifact deltas, pseudo-locale matrices, installed/simulator results, and the
final extraction workflow. A separate tactical is required to choose and ship
the first real non-English cohort. Website, store, marketing, and support-copy
localization remain outside this owner until explicitly planned.
