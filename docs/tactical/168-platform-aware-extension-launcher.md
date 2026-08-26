# Tactical 168: Platform-Aware Extension Launcher

Status: **Complete as of 2026-08-26.** The deterministic package and physical
ChromeOS presentation/link/handoff checks pass. Signed-release Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) resumes as the sole
**Now**.

Topics: `client-surfaces`, `product-surfaces-and-migration`,
`beta-release-readiness`

Dependencies: the exact JSTorrent Beta extension identity and desktop native
bootstrap from Tactical
[`166`](166-desktop-native-bootstrap-and-extension-scaffold.md), the completed
ChromeOS Linux handoff from Tactical
[`167`](167-chromeos-crostini-bundled-web-launcher.md), and the published
JSTorrent Android listing for package `com.jstorrent.app`.

## Desired Outcome

Make the small extension popup relevant to the platform on which it is opened:

- ChromeOS presents the two usable Chromebook choices, the published
  **JSTorrent for Android** app and **RSTorrent for ChromeOS Linux**;
- macOS, Windows, Linux, and OpenBSD present only the RSTorrent desktop native
  bootstrap; and
- an unknown platform or failed platform query falls back to showing both
  surfaces rather than silently hiding a recovery route.

The ChromeOS choice is honest about backend ownership. Android and ChromeOS
Linux are separate applications with separate torrent libraries and download
locations. The extension does not infer Play availability, Android-app
installation, or shared state.

## Scope And Stopping Condition

This tactical owns:

1. a pure, deterministic presentation decision around
   `chrome.runtime.getPlatformInfo()`;
2. a polished ChromeOS chooser that omits the irrelevant desktop native-host
   error, links only to the exact published Google Play listing, and retains
   the warm Crostini open/focus plus setup/recovery actions;
3. a desktop presentation that omits Chromebook-only controls and retains the
   existing typed native `hello` and `launch` behavior;
4. an explicit unknown/error fallback, accessible hidden-state transitions,
   exact-link validation, and no new permissions;
5. an extension version increment and deterministic reviewed ZIP; and
6. proportional real-Chrome evidence on the physical Chromebook, including
   the chooser, exact Play destination, and retained Crostini handoff.

The slice stops when deterministic platform/UI tests, source validation, two
byte-identical package runs, and the physical ChromeOS spot check pass. It
does not require publishing the extension or Android app.

## Security And Product Invariants

- Platform information chooses presentation only. It grants no authority and
  changes no backend, native-message, or external-message trust boundary.
- The Android action opens exactly
  `https://play.google.com/store/apps/details?id=com.jstorrent.app`. No caller,
  page, storage value, or remote response supplies a destination.
- The popup never says Google Play is enabled or the Android app is installed.
  It offers the published listing and lets ChromeOS/Google Play report actual
  availability.
- ChromeOS Linux retains the exact fixed origin, protocol, extension ID, and
  detachable backend ownership from Tactical `167`.
- Android and ChromeOS Linux copy explicitly says their libraries and download
  locations are separate.
- No host permission, content script, analytics, remote code, polling, Android
  intent bridge, new native operation, or new long-lived task is added.

## Reference Check

The current JSTorrent checkout at revision
`25e4b701433fd815398ba89526546f5e4f072e3f` was inspected at:

- `README.md` for the published Google Play package and ChromeOS product link;
  and
- `extension/src/lib/chromeos-bootstrap.ts` for the exact Android package and
  Play fallback used by the current product.

RSTorrent adopts only the maintainer-owned exact public-listing destination.
It deliberately does not adopt JSTorrent's Android companion pairing, forced
companion intent, polling, IO daemon, or extension engine topology. The live
Google Play page was independently checked on 2026-08-26 and identifies
**JSTorrent** by Graehl Arts at package `com.jstorrent.app`.

This is presentation and launch integration, not BitTorrent protocol or engine
work, so the pinned libtorrent oracle pass is inapplicable.

## Validation

The proportional source baseline is:

```bash
npm test --prefix clients/extension
npm run package --prefix clients/extension
```

Tests must cover ChromeOS, each desktop OS family, unknown/error fallback,
exact Play destination, hidden irrelevant controls, retained desktop messages,
and retained Crostini messages. The package validator must keep the reviewed
allowlist, exact store identity, and permission boundary.

The physical Chromebook check uses the authoritative `chromeos-testbed` and
`machine-control` path. It records semantic popup state, exact Play tab URL,
and successful warm Crostini UI focus/open without modifying or installing the
published Android application.

## Completion Record

Planning landed in `826a1ee`; implementation landed in `82f54f8`. Extension
version `0.3.0` packages as `jstorrent-beta-0.3.0.zip` with SHA-256
`96af7af3a64f4dfefeb73216d11f95d0f5742ddd7508f27842d4d1f3bce9ac28`.
Two package runs were byte-identical.

Fourteen extension tests and source/archive validation pass. They cover the
ChromeOS-only chooser, macOS/Windows/Linux/OpenBSD desktop-only presentation,
unknown/error recovery fallback, application of hidden state, the one exact
Play URL, the existing native host messages, and the exact Crostini handoff.
The reviewed ZIP adds only the local platform helper and unchanged packaged
asset families; manifest permissions remain exactly `nativeMessaging` and
`storage`.

The physical check used the same ChromeOS `16700.60.0` milestone 150 x86_64
Chromebook as Tactical `167`. The exact unpacked extension reloaded as version
`0.3.0`. Its semantic tree contained **Choose your Chromebook app**,
**JSTorrent**, **Open on Google Play**, **RSTorrent preview**, **Open ChromeOS
Linux UI**, and **Setup and recovery**, with no desktop section or native-host
error. The rendered 360-pixel surface showed both choice cards and the
separate-library note without clipping.

Selecting the Android action opened exactly
`https://play.google.com/store/apps/details?id=com.jstorrent.app`; the live page
identified **JSTorrent** by Graehl Arts and exposed Chromebook install UI. No
Android application was installed or inspected. After the registered Linux
Launcher warmed the existing clean Crostini package, closing its RSTorrent tab
and selecting **Open ChromeOS Linux UI** reopened the backend-served React
application at `http://penguin.linux.test:3030/` with the expected empty
library. The testbed doctor passed all ten required checks before and after.

Final cleanup closed every test tab, stopped `termina`, removed local staging
and screenshot evidence, and retained the reviewed extension deployment plus
the clean RSTorrent Crostini installation. No store upload, application
installation, tag, push, or release occurred.

## Non-Goals

- Detecting Google Play enablement, Android application installation, ARCVM
  state, app version, or compatibility.
- Installing, launching through an Android intent, pairing with, controlling,
  replacing, or modifying the current JSTorrent Android application.
- Sharing or migrating torrents, profiles, roots, payloads, or settings
  between Android and ChromeOS Linux.
- Polishing the full React product UI, changing desktop native-host behavior,
  waking a stopped Crostini VM from the extension, or adding production
  extension control.
- Chrome Web Store upload/publication, Play Console changes, public Crostini
  distribution, release tagging, or updater work.

## Escalation Contract

Ordinary popup markup, CSS, deterministic platform helpers, exact-link opening,
tests, versioning, and physical browser automation are in scope. Stop for
direction if the work would add permissions, inspect Android/Play state,
install or modify another application, introduce a remote service, change a
backend trust boundary, or publish any artifact.
