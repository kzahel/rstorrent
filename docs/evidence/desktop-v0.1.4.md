# Desktop 0.1.4 Candidate And Installed Acceptance

## Candidate Before Publication

Annotated `desktop-v0.1.4` tag commit:
`691143b6db1c0269404c0babf2c019a38d3b290b` on `main`.
Version-preparation commit:
`ca9bcbe6cf87b00939d49540127763fc94a6f453`. Desktop Cargo, both Tauri
configurations, shared web package and lockfile all declare `0.1.4`.
The application identifier remains `com.jstorrent.rstorrent`; its distinct
updater key and production route remain unchanged. This is an unsupported
incubation release, not a selected persistence baseline or beta declaration.

Signed workflow [`36262994983`](https://github.com/kzahel/rstorrent/actions/runs/36262994983)
passed from that tag. It produced Developer ID signed/notarized macOS arm64
and x86_64 apps/DMGs, Windows x86_64 Authenticode NSIS/MSI, and Linux x86_64
and arm64 AppImage/DEB/RPM packages. Its sole finalizer validated the
five-platform `latest.json`, updater signatures, and `SHA256SUMS` before
publication. The public package claims below come from the tagged workflow
and independent live-route checks, not local source tests.

The first unpublished tag attempt
[`36250244821`](https://github.com/kzahel/rstorrent/actions/runs/36250244821)
passed source and both macOS package legs but failed both Ubuntu 22.04 Linux
legs before packaging: its default Python 3.10 cannot import `tomllib` in the
GLib provenance check. The run was canceled; its draft and original tag were
removed. The corrected tag commit pins Python 3.12 for Linux release checks.
Local `actionlint`, the GLib check, release-tool tests, and release validator
pass after that change. No asset from the failed draft was published.

The second unpublished attempt
[`36253275926`](https://github.com/kzahel/rstorrent/actions/runs/36253275926)
passed source checks, both signed macOS legs, and the Windows x86_64 signing,
installed activation registry, and packaging gates. Both Linux legs passed
the Python and optimized GLib checks but failed in Tauri's AppImage bundling
step with only `failed to run linuxdeploy` in its default error output. The
finalizer refused publication. Its draft and tag were removed. Commit
`8563e9b9` adds Linux-only verbose Tauri output to reveal the underlying
linuxdeploy failure; local `actionlint` and `git diff --check` pass. No asset
from the second failed draft was published.

The third unpublished attempt
[`36257893690`](https://github.com/kzahel/rstorrent/actions/runs/36257893690)
passed source checks and both signed macOS legs. Verbose output identified
the exact Linux x86_64 and arm64 failure in the pre-signing AppImage notice
hook: Ubuntu 22.04 lacks the `GPL-2.0` common-license path referenced by a
package copyright, while its `base-files` package provides the canonical
`GPL-2` text. The already-failed run was canceled before its redundant
Windows leg; the prior attempt passed that leg. Its draft and tag were
removed, with no asset published. Commit `691143b6` resolves only explicit
GPL/LGPL version-name aliases when the named path is absent, preserving
containment and unknown-license failures. All 18 distribution review tests
pass locally, including the alias and escape cases.

Local preflight passes: release configuration and 15 release-tool tests;
web typecheck and 386 unit tests; desktop `cargo check`, strict clippy,
44 unit tests, and workspace formatting; `git diff --check`. Hosted main
candidate CI run [`36248531712`](https://github.com/kzahel/rstorrent/actions/runs/36248531712)
passes all ten jobs at the version-preparation commit. Corrected-tag main CI
run [`36253262895`](https://github.com/kzahel/rstorrent/actions/runs/36253262895)
also passes all ten jobs after the Linux release-workflow Python setup.
Diagnostic-tag main CI run
[`36256544499`](https://github.com/kzahel/rstorrent/actions/runs/36256544499)
passes all ten jobs at commit `8563e9b9`. Current-tag main CI run
[`36262149413`](https://github.com/kzahel/rstorrent/actions/runs/36262149413)
also passes all ten jobs.
An exact local Cargo/web/website dependency audit passes the release-ready
review, including the GLib source-provenance check and expiring warning review.

Applicable open release gates: `QA-003`, `QA-005`, `DESK-001`, and `UPD-005`.
The decisive installed case is a native Windows x86_64 fresh-default
`0.1.1`-to-`0.1.4` update through the production route with explicit install,
automatic reset/relaunch, exact version/build and signature, and preservation
of a pre-update payload sentinel. The native Windows appliance is now ready:
the exact public `0.1.1` NSIS installer matches its published SHA-256
`7f034e481afabfee136f6b4b44a96a92997440fe2ac9942d164b7dd610bfca8e`
and expected Authenticode signer, installs per-user, and starts under untouched
listener defaults. The Windows Security prompt was canceled, yielding only
scoped inbound Public block rules. A pre-update Documents payload sentinel
has SHA-256 `8bbc4a3b5efd8ff9a1502d7f3b76334dcba20245f0b524c2d740d11fc6680e0d`.
The old build's Add folder control reports no usable picker starting directory,
so the sentinel is outside its selected-root catalog; it still tests whether
the reset touches unrelated payload files. Linux x86_64 already passed the
exact public `0.1.1`-to-`0.1.3` AppImage path; macOS arm64 and Linux arm64
previously passed
`0.1.0`-to-`0.1.1`. Maintainer direction omits Intel macOS installed testing.
Before `0.1.4` publication, the production Windows x86_64 route still returns
HTTP 200 for `0.1.1` with signed `0.1.3` NSIS metadata; its switch to the
new version must be independently rechecked afterward.

Known limits before installed acceptance: public `0.1.3` Windows replacement
succeeds but its old-catalog reset exits with Windows error 5; current source
repairs that path. The Linux GNOME testbed's inactive AppIndicator prevents a
visible-tray claim. Native-platform notice/source-delivery review and the
broader installed common cohort remain open. The repository's medium GLib
alert is version based and does not recognize the qualified source backport;
release audit still checks that exact patch and the reviewed warning set.
Remote access and direct file save remain owner-only incubation previews.

Publication decision: the maintainer authorized a new signed desktop release.
Publish `0.1.4` only if candidate CI and all tagged workflow gates pass;
record the public assets and installed result below afterward. A failing gate
does not authorize declaring updater or beta readiness.

## Published Package And Installed Result

The finalizer published
[`desktop-v0.1.4`](https://github.com/kzahel/rstorrent/releases/tag/desktop-v0.1.4)
after the source gate and all five signed package jobs passed. The public
release contains 13 hashed assets plus `SHA256SUMS`; `latest.json` has all 15
required default and package-specific keys. Independent public reads found
version `0.1.4` and exact checksums including Windows NSIS
`7e4e88563bd3fbda9ecfb0b9283c795f1bfdea80c494bb9b55caf9488d73249a`,
Linux x86_64 AppImage
`0ae0b15544a520889ca794b25e9b2206df2530bda01c4349f3c9d16165e66585`,
and Linux arm64 AppImage
`217adebfa19cfcaf56e679be1699593d4b5f9d1c7f9199ad0daf5471d5824432`.
The production updater returned HTTP 200 with signed `0.1.4` metadata for
older `0.1.1` clients on all five default target/architecture routes and
HTTP 204 for current `0.1.4` clients. Windows x86_64 pointed to the exact
public 0.1.4 NSIS asset; the other routes pointed to their corresponding
signed updater archives.

On the native Windows 11 x86_64 appliance, the public, expected-publisher
signed `0.1.1` per-user NSIS install launched under untouched listener
defaults. Its installed executable reported version `0.1.1`, with one
running process and a valid Kyle Graehl Authenticode signature. The app's
actual **Review update** view offered `0.1.4`; explicit **Install and
restart** replaced the executable and automatically relaunched one process.
The new About view reported version `0.1.4`, build `691143b6db1c`, target
`x86_64-pc-windows-msvc`, and Windows NSIS; it reported the current version
rather than another available update. The installed executable's version was
`0.1.4`, its Authenticode status remained Valid with the expected signer,
and HKCU uninstall metadata reported `0.1.4`. The old process ID changed
from 8760 to 5116 with one current process, so replacement was followed by
an actual relaunch. The new first-run usage-statistics dialog appeared after
the old private-profile reset; statistics were disabled for this appliance
and the later Privacy & feedback view confirmed the preference unchecked.
The pre-update Documents sentinel retained SHA-256
`8bbc4a3b5efd8ff9a1502d7f3b76334dcba20245f0b524c2d740d11fc6680e0d`.
The test appliance was cleanly shut down and its exclusive claim released.

This closes the native Windows old-catalog reset/relaunch failure observed
with public `0.1.3`. It does not prove retention of disposable `0.1.x`
application-private state. The sentinel was outside the selected-root
catalog because the old build could not obtain a usable folder-picker
starting directory, so this test proves the reset left that unrelated
payload file untouched but does not prove preservation of a selected root.
The broader installed cohort, macOS arm64 and Linux arm64 pre-update payload
checks, Linux GNOME visible tray affordance, native notice/source-delivery
clearance, and the first supported persistence baseline remain open.
