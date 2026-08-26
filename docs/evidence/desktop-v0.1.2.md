# Desktop v0.1.2 Release And macOS Spot Evidence

Date: 2026-08-26

Status: **Published, complete signed-package workflow passed, and a bounded
macOS arm64 launch/native-host spot check passed.** This record does not prove
an installed `0.1.1`-to-`0.1.2` update or installed Windows/Linux behavior.

## Release Identity

- Version/tag: `0.1.2` / `desktop-v0.1.2`
- Commit: `788e953d1ed578c238beccbbc224907b0d9dc95c`
- Application identifier: `com.jstorrent.rstorrent`
- Public release:
  <https://github.com/kzahel/rstorrent/releases/tag/desktop-v0.1.2>
- Tagged release workflow:
  <https://github.com/kzahel/rstorrent/actions/runs/32959820514>
- Publication decision: publish only after the source gate, all five
  serialized signed-package jobs, and the complete-draft finalizer passed.

The final workflow passed its release configuration, extension package,
formatting, desktop Clippy/tests, web typecheck/tests/build, signed package,
package metadata, checksum, and finalization gates. Release preparation did
not run a duplicate local build or test matrix; CI was the requested
validation owner.

## Published Artifacts

The public release contains `latest.json`, `SHA256SUMS`, and these 12 package
or updater artifacts:

| Target | Public artifacts | Hosted assertions |
| --- | --- | --- |
| macOS arm64 | `RSTorrent_0.1.2_aarch64.dmg`, `RSTorrent_aarch64.app.tar.gz` | Developer ID signing, Gatekeeper assessment, notarization, stapling, updater signature |
| macOS x86_64 | `RSTorrent_0.1.2_x64.dmg`, `RSTorrent_x64.app.tar.gz` | Developer ID signing, Gatekeeper assessment, notarization, stapling, updater signature |
| Windows x86_64 | `RSTorrent_0.1.2_x64-setup.exe`, `RSTorrent_0.1.2_x64_en-US.msi` | expected-publisher Authenticode on both installers and installed activation-registry validation |
| Linux x86_64 | `RSTorrent_0.1.2_amd64.AppImage`, `RSTorrent_0.1.2_amd64.deb`, `RSTorrent-0.1.2-1.x86_64.rpm` | complete package matrix, activation metadata in every extracted package, updater signatures |
| Linux arm64 | `RSTorrent_0.1.2_aarch64.AppImage`, `RSTorrent_0.1.2_arm64.deb`, `RSTorrent-0.1.2-1.aarch64.rpm` | complete package matrix, activation metadata in every extracted package, updater signatures |

The public Apple-silicon DMG asset digest and the independently downloaded
`SHA256SUMS` value are both
`bf0d29ac1b4b5a7d8724cabd073c9933418970aef37446be0d8056fc3db98751`.
The tag, public release target, workflow head, and local annotated tag all
resolve to the exact commit above.

## Pre-publication Repairs

No failed attempt published a release. Two early runs stopped in the source
gate while exposing a Linux-only AppImage-path type mismatch and the matching
release-validator drift. Run
[`32953423351`](https://github.com/kzahel/rstorrent/actions/runs/32953423351)
then passed source checks, both macOS package jobs, and Windows packaging, but
its two Linux jobs found a verifier defect after successfully building their
AppImage, DEB, and RPM bundles: `bsdtar` extracted the Debian outer `ar`
container rather than its filesystem payload. The unpublished draft was
deleted, the verifier changed to `dpkg-deb -x`, and the tag was recreated only
at the corrected commit. The final run proves that correction on both Linux
architectures.

## macOS arm64 Public-Artifact Spot Check

The exact public DMG was tested on the claimed Machine Control macOS appliance,
which began powered off and reported macOS `26.2` build `25C56` on `arm64`.

1. The host download and the guest copy both produced the exact published
   SHA-256 above. `hdiutil` verified the read-only disk image while mounting
   it.
2. `codesign --verify --deep --strict` passed for the mounted app and its
   packaged `rstorrent-native-host`. Gatekeeper accepted it with source
   `Notarized Developer ID`, and `xcrun stapler validate` passed.
3. The mounted plist reported identifier `com.jstorrent.rstorrent`, short and
   bundle version `0.1.2`, executable `rstorrent-desktop`, minimum macOS
   `13.0`, `magnet:` handling, and the `.torrent` document type.
4. A uniquely named copy in the guest user's Applications directory was
   registered and launched through the common Machine Control application
   API. The returned application state and an independent guest process check
   both found the exact copied executable running. A native Accessibility
   snapshot found its visible `RSTorrent` window.
5. First launch installed the versioned
   `com.jstorrent.rstorrent.native` host and a Chrome manifest. The manifest
   used `stdio`, named the versioned `0.1.2` stable host, and allowed exactly
   the production JSTorrent extension
   `dbokmlpefliilbjldladbimlcfgbolhk` plus the provisional RSTorrent/JSTorrent
   Beta extension `gcgoepclopkgijmclmlheafaglmbjlcc`. Its launch config
   targeted the exact test app.
6. The app quit and the process disappeared. The test app was unregistered
   and removed, its task-created browser manifest and native-host directory
   were removed, the DMG was detached, and host/guest temporary downloads were
   deleted. Pre-existing RSTorrent profile state was preserved. The appliance
   returned to powered off and the claim was released.

## Deliberate Limits And Next Gate

- This is a release/package/launch spot check, not the strengthened
  `UPD-005` cross-version campaign. It did not install `0.1.1`, retain an
  incomplete torrent, or update that state to `0.1.2`.
- Windows signed packaging and activation-registry validation passed in CI,
  but clean default-profile launch, firewall-consent characterization, and
  installed update evidence remain open.
- Linux package construction and extracted activation metadata pass on both
  native architectures; installed Linux x86_64 behavior remains open.
- Installed Intel macOS testing remains deliberately omitted. The extension
  messaging/cold-launch boundary is recorded separately by Tactical `166`;
  Crostini topology remains undecided.
