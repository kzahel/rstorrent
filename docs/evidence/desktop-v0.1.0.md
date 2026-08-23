# Desktop v0.1.0 Release Evidence

Status: **Published and bounded macOS launch smoke passed on 2026-08-23.**

This is the acceptance record for RSTorrent's first public incubation desktop
release. It proves the signed publication pipeline, public artifacts, updater
metadata, and one installed Apple-silicon launch. It does not prove an
installed cross-version update or installed Windows/Linux behavior.

## Release Identity

- Version/tag: `0.1.0` / `desktop-v0.1.0`
- Commit: `768d7de3f5fabcdea4bc1619b127247d61df9ef9`
- Application identifier: `com.jstorrent.rstorrent`
- Public release:
  <https://github.com/kzahel/rstorrent/releases/tag/desktop-v0.1.0>
- Ordinary CI:
  <https://github.com/kzahel/rstorrent/actions/runs/32656028035>
- Tagged release workflow:
  <https://github.com/kzahel/rstorrent/actions/runs/32656926123>
- Publication decision: publish after the source gate, all five serialized
  signed package legs, and the sole complete-draft finalizer passed.

## Published Artifacts

The release contains `latest.json`, `SHA256SUMS`, and these 12 packages/update
archives:

| Target | Public artifacts | Hosted assertions |
| --- | --- | --- |
| macOS arm64 | `RSTorrent_0.1.0_aarch64.dmg`, `RSTorrent_aarch64.app.tar.gz` | Developer ID signing, Gatekeeper assessment, notarization, stapling, updater signature |
| macOS x86_64 | `RSTorrent_0.1.0_x64.dmg`, `RSTorrent_x64.app.tar.gz` | Developer ID signing, Gatekeeper assessment, notarization, stapling, updater signature |
| Windows x86_64 | `RSTorrent_0.1.0_x64-setup.exe`, `RSTorrent_0.1.0_x64_en-US.msi` | valid Authenticode signature and expected publisher on both installers; NSIS updater signature |
| Linux x86_64 | `RSTorrent_0.1.0_amd64.AppImage`, `RSTorrent_0.1.0_amd64.deb`, `RSTorrent-0.1.0-1.x86_64.rpm` | complete package matrix and updater signatures |
| Linux arm64 | `RSTorrent_0.1.0_aarch64.AppImage`, `RSTorrent_0.1.0_arm64.deb`, `RSTorrent-0.1.0-1.aarch64.rpm` | complete package matrix and updater signatures |

All 13 entries named by the published `SHA256SUMS` were downloaded from the
public release and passed `shasum -a 256 -c SHA256SUMS`. The GitHub asset
digest for the smoke-tested arm64 DMG and its checksum-file value are both
`21909219b0e70508a37c6b4ad11318d3e83e4372c7b7a6aeb6a907e9f3f3c4d8`.

The published `latest.json` reports version `0.1.0`, contains the five required
default updater keys plus package-specific aliases, gives every record a
nonempty signature, and uses only immutable URLs under the exact
`desktop-v0.1.0` GitHub release.

## Production Update Route

The production service passed both current-version and older-version probes
with a syntactically valid private UUID and `X-Check-Reason: manual`:

| Target/architecture | Current `0.1.0` | Older `0.0.0` result |
| --- | --- | --- |
| `darwin/aarch64` | HTTP 204 | HTTP 200, signed `RSTorrent_aarch64.app.tar.gz` metadata |
| `darwin/x86_64` | HTTP 204 | HTTP 200, signed `RSTorrent_x64.app.tar.gz` metadata |
| `windows/x86_64` | HTTP 204 | HTTP 200, signed `RSTorrent_0.1.0_x64-setup.exe` metadata |
| `linux/x86_64` | HTTP 204 | HTTP 200, signed `RSTorrent_0.1.0_amd64.AppImage` metadata |
| `linux/aarch64` | HTTP 204 | HTTP 200, signed `RSTorrent_0.1.0_aarch64.AppImage` metadata |

Every HTTP 200 body reported version `0.1.0`, a nonempty updater signature,
and an immutable URL in the exact public release.

## Installed macOS arm64 Smoke

The public arm64 DMG was mounted read-only on the maintainer's Apple-silicon
macOS machine. No prior `/Applications/RSTorrent.app` or
`com.jstorrent.rstorrent` application-config directory existed.

1. `codesign --verify --deep --strict` passed on the mounted and installed app.
2. `spctl --assess --type execute` accepted both copies with source
   `Notarized Developer ID`; `xcrun stapler validate` passed.
3. The mounted plist reported identifier `com.jstorrent.rstorrent`, short and
   bundle version `0.1.0`, and executable `rstorrent-desktop`.
4. The app was copied from the DMG to `/Applications/RSTorrent.app` and
   launched through Launch Services without a source checkout or development
   server.
5. The installed process remained healthy for the 12-second observation
   window, crossing the five-second startup-check schedule, at 108,768 KiB RSS.
6. Native updater initialization created `cfu-id` as a valid private UUIDv4.
7. An application-ID-directed quit terminated the process within ten seconds.
8. The smoke-created app and config directory were moved to Trash under unique
   `RSTorrent-smoke-*` names; no pre-existing user state was changed.

## Deliberate Limits And Next Gate

- Windows and Linux packages were built and validated on native hosted
  runners but were not installed or launched on external machines in this
  campaign.
- This first release cannot prove an old-to-new installed update. The next
  desktop version must update an exact installed `0.1.0` through the production
  route and relaunch into the new version on every supported updater target.
- The macOS smoke proved launch and updater initialization, not the complete
  beta torrent cohort, clean-machine permissions, file picker behavior,
  uninstall policy, rollback, or state migration.
- Android closed testing and iOS TestFlight remain independent release lanes.
