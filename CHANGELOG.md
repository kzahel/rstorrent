# Changelog

All notable desktop incubation changes are recorded here. RSTorrent follows
three-component versions, and desktop release tags use
`desktop-v<version>`.

## [Unreleased]

## [0.1.3] - 2026-08-30

- Add the owner-only remote-access preview for desktop, including Settings
  enrollment, durable browser resume, authorized-session administration, and
  a security audit log. The hosted controller and opaque relay cannot read
  authenticated application traffic.
- Keep remote access an unsupported, unadvertised incubation capability for
  manual owner testing. It controls a running desktop product host and does
  not provide Android remote control.
- Write torrent content directly to final paths, adopt existing payload
  through a bounded full recheck, and preserve unrelated files during
  recovery.
- Add responsive Library media details and playback, restore torrent sizes,
  and make High file priority durable.
- Improve metadata startup and dry-swarm recovery while bounding peer
  attempts, probe pacing, and retained transfer accounting.
- Reduce browser connection bandwidth with view-aware coalescing, sparse row
  patches, compact preparation progress, and incremental speed history.
- Replace settings mutation with revisioned typed patches so desktop, web,
  and Android drafts converge on authoritative state.
- Clarify that every `0.1.x` package is an unsupported incubation build. Its
  application-owned state and application contracts may be reset or replaced;
  no compatibility baseline begins until a future version is explicitly
  declared the first supported beta or release.
- Start a fresh schema-21 session catalog, resetting recognized schemas 1
  through 20 without touching user payload, and remove old DHT, desktop-shell,
  and browser-appearance compatibility readers.

## [0.1.2] - 2026-08-26

- Repair fresh-profile Windows listener selection and add the packaged native
  download-root picker.
- Register installed desktop packages for bounded `magnet:` and local
  `.torrent` activation, restore the existing window, and reuse the ordinary
  download-root and Add-options workflow without exposing source paths to the
  webview contract.
- Reject non-regular external `.torrent` paths before opening them and retain
  platform-native file URL handling across Windows, macOS, and Linux.
- Enforce one desktop product lifetime with default-on close-to-tray,
  persisted **Run in Background**, visible manual update checks, and joined
  close/Quit/restart shutdown.
- Add branded desktop tray/window integration and suppress the unwanted
  console window from release Windows launches.
- Add native completion and repair-attention notifications plus default-on
  active-download sleep inhibition with joined cleanup.
- Package the bounded RSTorrent native-messaging bootstrap, authorize the
  production JSTorrent and provisional JSTorrent Beta extension origins, and
  repair per-user Chrome registration on desktop launch.
- Extend credential-free desktop presubmit packaging to native Linux arm64
  while retaining the Linux x86_64 package gate.

## [0.1.1] - 2026-08-23

- Updater-validation release with no engine, protocol, or persistence changes.
- Reused the `0.1.0` application identity, updater trust root, production
  route, and application state as updater-validation evidence, without making
  that incubation state a supported compatibility baseline.

## [0.1.0] - 2026-08-23

- First signed incubation desktop packages for macOS, Windows, and Linux.
- First-party Rust BitTorrent engine embedded directly in the desktop app.
- Shared React Library, Transfers, and Workbench product interface.
- Explicit, signed in-app updates for supported self-replacing packages.
