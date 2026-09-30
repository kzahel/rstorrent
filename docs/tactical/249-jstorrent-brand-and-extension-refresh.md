# Tactical 249: JSTorrent Brand And Extension Refresh

Status: **Active, 2026-09-30.**

Topics: [product-surfaces-and-migration](../topics/product-surfaces-and-migration.md),
[web-ui-design](../topics/web-ui-design.md),
[client-surfaces](../topics/client-surfaces.md),
[localization](../topics/localization.md),
[desktop-jstorrent-replacement](../topics/desktop-jstorrent-replacement.md),
[android-jstorrent-replacement](../topics/android-jstorrent-replacement.md).

## Accepted Direction

The next production generation replaces JSTorrent in place through its existing
update channels. Desktop retains its application identity and Tauri updater
trust root; Android updates the existing Play app with package/signing continuity;
the extension updates the existing production Web Store item using this repo's
implementation. Store updates may arrive independently under Tactical 248.
Rebrand the successor as JSTorrent before that cutover, retaining its established
icons and familiar torrent-list, detail, folder and native-app workflows.

This supersedes the earlier foreseeable independent RSTorrent product direction.
The repository, Rust crates and internal protocol names may remain RSTorrent.
Separate incubation/test package identities and update routes remain during
qualification. No publication, credential transfer or production release is
part of this slice.

## Scope And Stopping Condition

Restore original JSTorrent desktop, Android and extension icon assets with exact
source/license provenance. Use JSTorrent in desktop/shared React and Android
presentation copy and titles. Refresh the extension popup with familiar blue
accents, light/dark support, concise app-oriented connection guidance, and
accessible focus/status behavior. Keep the shared application's established
information architecture and user preferences.

Stop after source/catalog checks, web typecheck/unit coverage, packaged extension
validation, Android resource/build validation, and isolated browser visual and
accessibility checks of popup variants and the shared library. Record evidence
and remaining production gates before committing.

## Non-goals And Invariants

No engine, application DTO, catalog schema, importer, pairing, grant, transport,
permission, telemetry or lifecycle changes. No broad crate/path/key replacement.
No iOS or website rename in this desktop/Android/extension slice. No store upload,
release tag, signing-secret use or update-route switch. Desktop incubation bundle
names and the extension's test Web Store ID remain explicitly separate from the
production replacement until a qualified production packaging slice.

Existing controller/service owners and cancellation paths remain unchanged.
Assets are static local resources; CSP gains no remote sources or executable
content. Import only the precise icon family needed for existing first-party
branding, under the sibling JSTorrent MIT license. Carry its copyright/permission
text into packaged notices as well as source attribution.

## Follow-up Gates

- Production desktop package name/identifier, public updater key and route,
  native-host/browser origin registration, signed installed upgrade and OS
  launch/file/magnet continuity.
- Existing Android Play package, signing continuity and increasing versionCode;
  API 26/27 disposition and physical ChromeOS installed upgrade/grant retention.
- Existing production extension ID/public key, backend origin admission,
  identity-preserving extension update and existing launch links.
- Focused shared library/detail/settings design review against JSTorrent, plus
  migration outcomes and recovery feedback; preserve density and familiar actions.

## Evidence

Pending implementation and qualification.
