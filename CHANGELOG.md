# Changelog

All notable desktop beta changes are recorded here. RSTorrent follows stable
three-component versions, and desktop release tags use `desktop-v<version>`.

## [Unreleased]

- Enforce one desktop product lifetime with default-on close-to-tray,
  persisted **Run in Background**, visible manual update checks, and joined
  close/Quit/restart shutdown.
- Add branded desktop tray/window integration and suppress the unwanted
  console window from release Windows launches.
- Extend credential-free desktop presubmit packaging to native Linux arm64
  while retaining the Linux x86_64 package gate.

## [0.1.1] - 2026-08-23

- Updater-validation release with no engine, protocol, or persistence changes.
- Retains the `0.1.0` application identity, updater trust root, production
  route, and compatible application state.

## [0.1.0] - 2026-08-23

- First signed incubation-beta desktop packages for macOS, Windows, and Linux.
- First-party Rust BitTorrent engine embedded directly in the desktop app.
- Shared React Library, Transfers, and Workbench product interface.
- Explicit, signed in-app updates for supported self-replacing packages.
