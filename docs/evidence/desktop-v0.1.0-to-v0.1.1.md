# Desktop 0.1.0 To 0.1.1 Update Evidence

Date: 2026-08-24

Status: the exact public macOS arm64 and Linux arm64 installed-update paths
pass. The Windows x86_64 NSIS updater also replaces and relaunches correctly
after selecting the supported automatic-loopback listener profile, but a fresh
default-profile installation cannot start because local-network address
selection falls back to `0.0.0.0` and the product rejects it. Linux x86_64
remains open. Maintainer direction deliberately omits an installed Intel macOS
campaign. This record does not close Tactical `158` or `UPD-005`.

## Release Identity

- Older version/tag/build: `0.1.0`, `desktop-v0.1.0`, and
  `768d7de3f5fabcdea4bc1619b127247d61df9ef9`.
- New version/tag/build: `0.1.1`, `desktop-v0.1.1`, and
  `2a9ab871847893ed809bf042406ab95487b9d645`.
- Public release:
  <https://github.com/kzahel/rstorrent/releases/tag/desktop-v0.1.1>.
- Credential-free presubmit:
  <https://github.com/kzahel/rstorrent/actions/runs/32660657596>.
- Signed tagged workflow:
  <https://github.com/kzahel/rstorrent/actions/runs/32661616090>.

The presubmit passed all seven jobs covering Rust/interop, web
unit/build/deterministic E2E, desktop macOS/Linux/Windows, Android
dual-ABI/lint/test, and iOS simulator/archive. The tagged workflow passed its
release-source gate, all five signed package jobs, and the sole publication
finalizer before making the release public.

## Public Assets And Production Routes

The public release contains 14 assets. All 13 files named by `SHA256SUMS`
were downloaded and passed independent SHA-256 verification. `latest.json`
reports `0.1.1`, contains 15 platform/package keys including all five required
default updater keys, gives each required record a nonempty signature, and
uses immutable URLs under the exact `desktop-v0.1.1` release.

Production checks used a syntactically valid disposable UUID and
`X-Check-Reason: manual`; the private installed identifier was never printed.

| Target/architecture | Current `0.1.1` | Installed `0.1.0` result |
| --- | --- | --- |
| `darwin/aarch64` | HTTP 204 | HTTP 200, signed `RSTorrent_aarch64.app.tar.gz` metadata for `0.1.1` |
| `darwin/x86_64` | HTTP 204 | HTTP 200, signed `RSTorrent_x64.app.tar.gz` metadata for `0.1.1` |
| `windows/x86_64` | HTTP 204 | HTTP 200, signed `RSTorrent_0.1.1_x64-setup.exe` metadata |
| `linux/x86_64` | HTTP 204 | HTTP 200, signed `RSTorrent_0.1.1_amd64.AppImage` metadata |
| `linux/aarch64` | HTTP 204 | HTTP 200, signed `RSTorrent_0.1.1_aarch64.AppImage` metadata |

Each HTTP 200 body had version `0.1.1`, a nonempty updater signature, and an
immutable exact-release URL. These route checks establish server coverage;
macOS arm64, Windows x86_64 with the stated profile boundary, and Linux arm64
have installed replacement evidence below.

## Installed macOS Arm64 Update

The test ran outside the source checkout in a claimed, isolated
Machine Control/Tart macOS workspace. The source appliance remained stopped;
the copy-on-write workspace was receipt-bound and configured for discard on
release. Ordinary target-native guest commands, Accessibility semantics,
capture, and input were used. Host/Tart-window input was forbidden.

1. The exact public `RSTorrent_0.1.0_aarch64.dmg` was downloaded in the guest
   and installed into the guest user's Applications directory. The installed
   bundle reported `com.jstorrent.rstorrent` and `0.1.0`, and passed strict
   code-signature and Gatekeeper assessment.
2. The installed app launched, reached its in-process service, created a valid
   private UUIDv4 `cfu-id` with mode `0600`, and displayed the automatic
   startup notice that `RSTorrent 0.1.1 is available`.
3. **Review update** opened About & updates. The page showed the `0.1.1`
   release notes and the running `0.1.0` build
   `768d7de3f5fabcdea4bc1619b127247d61df9ef9`.
4. A first-run macOS Local Network permission sheet was dismissed with
   **Don't Allow** because LAN discovery was not needed for the update. The
   production update check had already succeeded.
5. The explicit **Install and restart** action downloaded the signed arm64
   updater archive, replaced the app, terminated the old process, and
   relaunched one process from the updated bundle.
6. The relaunched UI reported version `0.1.1`, build
   `2a9ab871847893ed809bf042406ab95487b9d645`, target
   `aarch64-apple-darwin`, package `macOS app`, and automatic updates enabled.
   A manual follow-up check reported `Version 0.1.1 is the newest compatible
   release.`
7. The updated bundle's short version and bundle version were both `0.1.1`.
   The identifier remained `com.jstorrent.rstorrent`; strict code-signature,
   Gatekeeper, and stapled-notarization validation passed.
8. The post-relaunch `cfu-id` was byte-for-byte equal to the private baseline
   and remained mode `0600`. Its value was not logged or retained as evidence.
9. RSTorrent quit normally. Machine Control stopped and discarded the exact
   isolated workspace, released its claim, reported zero remaining temporary
   workspaces, and left the source appliance powered off.

The workspace's initial launchd job was accepted but remained at zero runs
until one bounded kickstart of that exact claimed job. Tart then reached full
power, administration, Aqua, resident, semantic, capture, and input readiness.
This is a Machine Control launch-path issue to diagnose separately; it did not
alter the app, release assets, updater route, or installed-update result.

## Installed Windows X86_64 Update

The test ran in a claimed, isolated Machine Control Windows workspace. The
source appliance remained stopped, the disposable overlay was receipt-bound,
and PowerShell, WinApp/semantic inspection, resident capture, and resident
input were used without host-window input.

1. The exact public `RSTorrent_0.1.0_x64-setup.exe` was downloaded in the
   guest. It was 10,065,472 bytes with SHA-256
   `e038f1662bedc496156ce8008b2b7f5eacc728ea5c6b06aae0695aaa046622a8`,
   and Authenticode validation reported the expected publisher.
2. The NSIS UI installed the per-user application into the standard local
   application-data location and launched it. A fresh default profile then
   failed before presentation with `Unable to start RSTorrent: local-network
   listener address is invalid`.
3. The appliance had a non-loopback IPv4 address. A direct OS probe showed
   that connecting an unbound UDP socket to `239.255.255.250:1900` selected
   `127.0.0.1`; `select_local_network_ipv4` rejects loopback, listener setup
   falls back to `0.0.0.0`, and the application validator rejects that address.
   This is a real default Windows startup blocker in both public versions, not
   an updater failure.
4. After preserving that result, the disposable profile's supported listener
   setting alone was changed to `automatic_loopback`. RSTorrent started,
   connected, retained its original private updater ID, and automatically
   displayed `RSTorrent 0.1.1 is available`.
5. About & updates showed version `0.1.0`, build
   `768d7de3f5fabcdea4bc1619b127247d61df9ef9`, target
   `x86_64-pc-windows-msvc`, package `Windows NSIS`, the public release notes,
   and **Install and restart**.
6. The explicit install action terminated the old process, completed without
   a lingering installer, and relaunched one process. The installed executable
   and per-user uninstall record both reported `0.1.1`; Authenticode remained
   valid with the expected publisher.
7. The relaunched UI reported build
   `2a9ab871847893ed809bf042406ab95487b9d645`, the same target/package, and
   automatic updates enabled. A manual follow-up check reported
   `Version 0.1.1 is the newest compatible release.`
8. The post-relaunch private `cfu-id` remained a valid UUIDv4 and was
   byte-for-byte equal to its baseline. Its value was never printed or
   retained as evidence.
9. The app was terminated and the exact isolated workspace was discarded.
   Machine Control reported zero temporary workspaces, an available claim,
   and the source appliance powered off.

This proves signed NSIS update replacement and relaunch under an existing
supported loopback-listener configuration. It does not qualify a clean default
Windows installation until the listener-selection defect is fixed and the
same campaign passes without altering application state.

## Installed Linux Arm64 Update

The test ran in a claimed, isolated Machine Control Ubuntu GNOME workspace.
The source appliance remained stopped, the disposable overlay was
receipt-bound, and guest commands, AT-SPI inspection, resident capture, and
resident input were used. Outer virtualization-window input was prohibited.

1. The exact public `RSTorrent_0.1.0_aarch64.AppImage` was downloaded into a
   stable user-writable Applications directory. It was 90,159,624 bytes with
   SHA-256
   `e039b0eb9ac3916dcca376c7623bbf7fcbc25fe8be883454bfd71de1639f5651`
   and executable mode `0755`.
2. The AppImage launched, reached its in-process service, created a valid
   private UUIDv4 `cfu-id` with mode `0600`, and automatically displayed
   `RSTorrent 0.1.1 is available`.
3. About & updates showed version `0.1.0`, build
   `768d7de3f5fabcdea4bc1619b127247d61df9ef9`, target
   `aarch64-unknown-linux-gnu`, package `Linux AppImage`, the public release
   notes, and **Install and restart**.
4. The explicit install action terminated the old process, replaced the
   AppImage, and relaunched exactly one new process. The installed file became
   the exact public `0.1.1` asset: 90,151,432 bytes, executable mode `0755`,
   and SHA-256
   `6e6ea5ec648a9a9c5de19c38c2ddb379bb75658c80798caf4dead68fe85c06c8`.
5. The relaunched UI reported version `0.1.1`, build
   `2a9ab871847893ed809bf042406ab95487b9d645`, the same target/package, and
   automatic updates enabled. A manual follow-up check reported
   `Version 0.1.1 is the newest compatible release.`
6. The post-relaunch `cfu-id` was byte-for-byte equal to the private baseline
   and remained a valid mode-`0600` UUIDv4. Its value was never printed or
   retained as evidence.
7. The app was terminated and the exact isolated workspace was discarded.
   Machine Control reported zero temporary workspaces, an available claim,
   and the source appliance powered off.

## Remaining Boundaries

- Intel macOS installed-update testing is deliberately omitted by maintainer
  direction. Its signed/notarized package and production route remain
  automated, but installed behavior is not a beta-readiness claim.
- Windows x86_64 must fix the fresh default local-network listener startup
  failure and repeat the installed campaign without changing profile state.
- No installed Linux x86_64 AppImage update has run because the available
  isolated Linux testbed is arm64.
- MSI, DEB, and RPM are deliberately manual/package-manager channels and are
  not self-replacement targets.
- The test proves application identity and updater-ID continuity. It does not
  substitute for the broader persistence-schema, beta torrent cohort,
  rollback, crash-during-update, or uninstall-retention campaigns.
