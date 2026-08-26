# RSTorrent for ChromeOS Linux changelog

## [Unreleased]

No unreleased ChromeOS Linux changes.

## [0.1.0]

- Add the bundled RSTorrent gateway and mature React control surface.
- Install per-user files and a static, disabled systemd user service without
  sudo or lingering.
- Add the ChromeOS Launcher handoff through the pinned JSTorrent Beta
  extension.
- Add the signed-manifest one-command bootstrap for native x86_64 and ARM64
  Crostini packages.

Known limitations: this preview has no in-app updater or rollback store. The
service starts on demand and does not run merely because the user logs in.
Android and ChromeOS Linux use separate application libraries and downloads.
