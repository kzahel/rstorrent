# Tactical 206: Android JSTorrent Feedback Handoff

Status: **Ready for implementation as of 2026-09-01.** Maintainer direction
selects the current JSTorrent Android feedback handoff exactly: one external
browser navigation to the existing JSTorrent feedback page with the same four
environment fields. No application code changes have landed under this
tactical yet.

Topics: `android-jstorrent-replacement`, `product-state-and-feedback`,
`client-surfaces`, `localization`, `capability-readiness`

Dependencies: completed Android product Tactical
[`117`](117-jstorrent-shaped-android-product-ui.md), completed Android
lifecycle Tactical [`200`](200-android-product-background-lifecycle.md), and
completed localization foundation Tactical
[`204`](204-cross-product-localization-foundation.md). The existing
`https://jstorrent.com/feedback.html` page remains an externally hosted
product dependency rather than code or content duplicated into RSTorrent.

## Product Outcome

Android Advanced Settings exposes **Report Bug / Send Feedback** with the
current JSTorrent supporting copy, **Help us improve JSTorrent**. Activating
the row opens the device's external browser at:

```text
https://jstorrent.com/feedback.html
    ?platform=android
    &v=<application version name>
    &android=<Android release>
    &device=<URI-encoded manufacturer and model>
```

The destination page remains the sole feedback presentation and recipient
owner. It displays the supplied system information, embeds the existing Google
Form with that information prefilled, and separately offers **Open an issue on
GitHub**. RSTorrent does not choose between those recipients, submit either
form, proxy content, or learn whether the user continues after navigation.

This is deliberate JSTorrent replacement parity, not a new RSTorrent support
backend. The current incubation identity may use the established JSTorrent
route because the implementation is intended to replace JSTorrent when the
separate graduation gates pass; this tactical does not rename, migrate,
publish, or graduate the application.

## Exact Behavior Contract

1. The base URL is exactly `https://jstorrent.com/feedback.html`; Android does
   not link directly to Google Forms or GitHub.
2. The query contains exactly `platform`, `v`, `android`, and `device`.
   `platform` is the literal `android`; the other values come from
   `BuildConfig.VERSION_NAME`, `Build.VERSION.RELEASE`, and
   `Build.MANUFACTURER + " " + Build.MODEL` at click time.
3. Query values use normal URI component encoding. A final URL above 2 KiB is
   rejected rather than truncated, partially opened, or replaced with another
   destination.
4. Android launches one `Intent.ACTION_VIEW` external-browser intent. There is
   no WebView, Custom Tab, Sharesheet, clipboard handoff, local report preview,
   file attachment, or application-owned HTTP request.
5. Failure to construct the strict HTTPS URL or resolve/start a browser is a
   bounded localized user-visible failure. It does not crash or change engine,
   torrent, service, profile, or settings state.
6. Leaving the activity for the browser uses the existing Android foreground,
   background-download, companion-interaction, and joined service-lifetime
   policy. Feedback navigation acquires no new lease and starts no task.

## Privacy Decision And Data Boundary

Opening the page necessarily transmits the four query values before any Google
Form or GitHub submission. The hosted page may disclose them through ordinary
browser history, request logs, referrers, its Google Forms iframe, and its
existing Cloudflare analytics. Maintainer direction explicitly accepts this
narrow behavior because it is the current JSTorrent replacement flow.

The exception is closed to these non-stable environment facts:

- literal platform `android`;
- application version name;
- Android release version; and
- device manufacturer and model.

The URL must never contain an installation, profile, report, peer, torrent, or
client-instance identifier; days-since-first-use or usage counters; torrent
names, hashes, magnets, paths, roots, trackers, peers, endpoints, settings,
logs, errors, diagnostics, lifecycle state, or user-entered prose. No cookie,
token, credential, report identifier, or durable feedback state is created by
the Android application. The general explicit-preview and explicit-submit
policy in `product-state-and-feedback` remains authoritative for any future
richer support report.

## Scope

- Add a small Android-owned pure URL builder with the exact base, closed key
  set, injected environment values for tests, strict HTTPS/host/path checks,
  and the 2-KiB final bound.
- Add an Android platform launcher that constructs the URL at click time and
  starts the external `ACTION_VIEW` intent with bounded failure feedback.
- Thread one presentation callback from `MainActivity` through `ProductApp`
  into the existing Advanced Settings route. Tests may inject the callback;
  Compose never reads Android build properties or starts an activity itself.
- Replace no existing unavailable row. Add the localized report action beside
  the current disabled Search plugins and Reset engine settings rows.
- Add English Android resources with translator context under the completed
  localization catalog workflow. English remains the only production locale;
  pseudo-locales remain test-only.
- Record the exact external dependency and installed evidence in the owning
  topics when implementation completes.

## Non-Goals

- A new feedback site, Google Form, GitHub repository, issue template, email
  address, server endpoint, API, or authentication mechanism.
- A local diagnostic export, log bundle, screenshot, attachment, support code,
  report ID, usage metric, product-state database, telemetry, analytics, or
  automatic submission.
- Reset settings, clear metadata/profile state, delete payload, forget storage
  roots, or modify installation identity. Those destructive operations remain
  separate work.
- React, desktop, extension, headless, or iOS feedback UI. Their eventual
  support entry points need separate product decisions rather than inheriting
  an Android callback.
- Mirroring or embedding `feedback.html` in this repository, pinning the
  Google Form ID in Android code, or treating the external page as an
  application availability dependency.
- Changing the RSTorrent incubation package identity or claiming JSTorrent
  graduation, replacement readiness, support readiness, or publication.

## Owner, Lifecycle, And Resource Map

```text
Advanced Settings row
    -> injected ProductApp callback
        -> MainActivity Android platform launcher
            -> strict bounded feedback URL
                -> external ACTION_VIEW browser activity
                    -> jstorrent.com page
                        |-> embedded Google Form
                        `-> optional GitHub issue link
```

- Compose owns only the localized row and click event.
- `MainActivity` owns Android build facts, intent construction, launch failure,
  and the ordinary foreground transition.
- The external browser, website, Google, and GitHub own all work after the
  successful activity handoff. RSTorrent owns no callback, polling, retry, or
  completion state.
- The operation creates one URL and one intent, retains neither, opens no
  application socket, changes no Rust/UniFFI/generated contract, and adds no
  task, queue, database row, preference, permission, or service lease.

## JSTorrent Reference Record

JSTorrent commit `25e4b701433fd815398ba89526546f5e4f072e3f` was
inspected:

- `android/app/src/main/java/com/jstorrent/app/ui/screens/AdvancedSettingsScreen.kt`
  renders the support section and `ReportBugButton`, builds the exact four-
  parameter URL, and starts a plain `Intent.ACTION_VIEW`;
- `android/app/src/main/res/values/strings.xml` owns the English label and
  supporting copy;
- `website/public/feedback.html` displays URL-provided system information,
  pre-fills Google Form field `entry.2074805438`, and offers
  `https://github.com/kzahel/jstorrent/issues/new`; and
- `extension/CHANGELOG.md` records the intentional move of **Report Bug** from
  direct GitHub navigation to `feedback.html`.

RSTorrent adopts this observable product behavior and external destination.
No Kotlin, HTML, JavaScript, test fixture, or translated string is copied from
the reference repository.

## Validation

- Pure Android unit tests prove the exact base scheme/host/path, key set,
  values, encoding of spaces and non-ASCII device text, absence of every
  prohibited key, deterministic 2-KiB rejection, and no mutation of the input
  values.
- Compose instrumentation navigates Library -> Settings -> Advanced, observes
  the localized support row, clicks it once, and proves exactly one injected
  callback without launching the real browser.
- Android platform tests prove the final `ACTION_VIEW` intent carries the
  expected HTTPS URI and that missing-handler/launch failures remain
  user-visible and non-fatal.
- `node scripts/check-localization.mjs`, `clients/android/build.sh`, Android
  lint, `testDebugUnitTest`, and `assembleDebugAndroidTest` pass.
- A focused API 35 AVD test proves the installed Settings route and browser
  intent handoff. One physical ChromeOS Android spot check opens the exact
  feedback page, visibly shows the four system-information fields, exposes the
  embedded Google Form and GitHub choice, then returns to the unchanged
  RSTorrent activity. It submits neither recipient and leaves no application
  task, preference, profile mutation, or feedback artifact.

## Stopping Condition

This tactical is complete when Android Advanced Settings exposes the localized
JSTorrent feedback action, one click builds only the exact bounded four-field
URL and hands it to the external browser, failure is non-fatal, existing
application/service lifecycle remains unchanged, prohibited context cannot
enter the URL, deterministic/build/AVD evidence passes, the physical ChromeOS
spot check confirms the live page and both recipient choices without
submitting, owning topics record exact evidence, and the completed slice is
committed.
