# Tactical 216: Local Support Diagnostics And Release Baseline

Status: **Complete locally (2026-09-12); hosted/package qualification and
explicit support declaration remain separate.** Authorized release-readiness campaign.

Topics: `beta-release-readiness`, `client-surfaces`, `web-ui-design`,
`application-view-api`, `localization`, `client-persistence`,
`product-state-and-feedback`, `capability-readiness`

## Scope And Stopping Condition

Provide a bounded, previewed local diagnostics copy/download in the existing
shared React desktop/headless About settings, with known issues, public report
and truthful privacy/recovery guidance. Prepare a concrete first-supported-
release baseline checklist and compatibility matrix; select no version and
make no current incubation compatibility promise. Stop with hostile-input,
clipboard/download and responsive accessibility evidence, updated owning
records, and a substantial commit.

## Invariants, Ownership And Non-Goals

The existing updater snapshot supplies only version/build/target/package and
closed updater state/operation. A pure serializer copies an explicit allowlist
and replaces invalid values with `unknown`, bounded below 2 KiB. Never export
raw exceptions, logs, file paths, URLs, torrent data, peers, installation IDs,
credentials, counts or arbitrary object fields. Capture an immutable preview
only on request; copying/downloading uses precisely those bytes. No network
request, automatic upload, new telemetry, engine task or Rust/API change.
The component owns preview and clipboard status; object URLs have immediate
revocation and no retained file handle. Public issue navigation carries no
report content. Existing Tactical 208 transmission gates remain off.

This is presentation for the current desktop beta lane and shared browser
host About surface. Android and iOS retain their existing build/privacy
presentation; native export UI is a separate presentation decision before
claiming those public support lanes. No engine semantics or generated ABI
changes are implied by this client-only export.

The baseline proposal names the version-selection decision, exact candidate
and on-disk contract inventory, forward/reopen/rollback/corruption/payload
matrix and platform evidence. It does not invent a migration reader, freeze
schema 26 for a future version, enable mobile distribution, or publish pages.


## Execution And Validation

The pure `buildDiagnostics` serializer emits schema 1 and fixed product,
version/build, target/package, privacy-model and updater phase/operation
fields. Values use closed sets or bounded version/build patterns; unknown
values become `unknown`. Unknown properties and raw error messages are never
serialized. Preview is a captured string, not a live object, and a generation
fence prevents a delayed clipboard write from marking a refreshed preview as
copied. Clipboard denial selects the exact preview and presents generic manual
copy/download guidance. Download revokes its object URL in the next task.

About includes no-context GitHub issue/report links and the repository's
support/privacy/recovery guide. Thirteen new English catalog messages pass
all localization gates; the existing update privacy sentences also gain their
missing separating space. No Rust DTO, generated application contract,
statistics preference or hosted-context transmission gate changes.

Validation:

- Web typecheck, full unit suite **386 passed / 2 skipped**, production build
  and CSP (12 JavaScript bundles without eval/Function/CommonJS) pass.
- Five new deterministic/component cases cover hostile and oversized inputs,
  unknown fields/raw errors, frozen preview, clipboard denial and stale-write
  completion. Existing About tests continue to pass.
- Four owned Playwright Chromium cases at **320 / 1440 pixels**, light/dark,
  prove clipboard and downloaded bytes exactly equal the preview, output below
  2 KiB, zero retained object URLs, no external requests, no horizontal overflow
  and no serious/critical Axe findings. Download artifacts are deleted. Phone
  and wide captures were visually inspected. The user's primary browser is
  never launched. Vite emits its existing optional remote-Wasm prebundle
  warning; it does not affect these passing component routes.
- The current unsigned macOS ARM64 Tauri app builds with the support UI and
  embedded dependency notices. Its installed-tree notice/content inspector
  and macOS package validator pass. This is a local source fixture, not a
  signed release or OS-level clipboard/picker qualification.
- Native Android/iOS export UI is explicitly not claimed. Their existing
  English catalogs still validate; no native/runtime build is required for
  this presentation-only change.

`docs/release-baseline.md` records the concrete candidate contract inventory,
forward/rollback/reset/payload matrix, signed and hosted evidence prerequisites
and explicit version/support decision. `docs/user-support.md` distinguishes
new source UI from public `0.1.3`, documents its actual Windows reset failure,
firewall choice, GNOME tray limitation, public report privacy, and bounded
payload-safe recovery. No current schema is frozen for a future version.
