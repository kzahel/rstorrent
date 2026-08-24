# Desktop 0.1.0 To 0.1.1 Update Evidence

Date: 2026-08-24

Status: the exact public macOS arm64 installed-update path passes. macOS
x86_64, Windows x86_64 NSIS, Linux x86_64 AppImage, and Linux arm64 AppImage
remain open and this record does not close Tactical `158` or `UPD-005`.

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
only the macOS arm64 row has installed replacement evidence below.

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

## Remaining Boundaries

- No installed macOS x86_64 update has run on an Intel testbed.
- No installed Windows x86_64 per-user NSIS update has run.
- No installed Linux x86_64 or arm64 AppImage update has run.
- MSI, DEB, and RPM are deliberately manual/package-manager channels and are
  not self-replacement targets.
- The test proves application identity and updater-ID continuity. It does not
  substitute for the broader persistence-schema, beta torrent cohort,
  rollback, crash-during-update, or uninstall-retention campaigns.
