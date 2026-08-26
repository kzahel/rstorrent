# RSTorrent for ChromeOS Linux

This crate owns the bounded Crostini launcher and per-user package adapter from
Tactical 167. It does not own the torrent engine or web application. The
installed static systemd user service executes the packaged
`rstorrent-gateway`, which serves the matching `clients/web` production bundle
and application API at `http://penguin.linux.test:3030`.

`rstorrent-crostini launch` maps a real X11 window before starting the static
service, validates the exact gateway health identity on loopback, and opens the
local `/launch-chromeos` handoff page. That page wakes the pinned JSTorrent Beta
extension, which reuses the tab for the backend-served React UI.

Build an architecture-specific source package on Linux with:

```bash
./scripts/build-crostini-package.sh
```

After extracting the archive inside Crostini, run `./install.sh`. Installation
requires no sudo, does not enable the service, and does not change user
lingering. `rstorrent-crostini uninstall` preserves the profile and downloads;
`rstorrent-crostini uninstall --purge` additionally removes only the Crostini
profile.

The release-ready website bootstrap and two-architecture signed-manifest
contract are documented in [`docs/crostini-release.md`](../../docs/crostini-release.md).
There is no public Crostini release yet, so the one-command path is not a
current availability claim.
