# Native x86_64 Installed Desktop Acceptance, 2026-09-12

## Exact Public Inputs

The production route currently serves desktop 0.1.3, not the 0.1.2 described
by the previous readiness checkpoint. Public release
[`desktop-v0.1.3`](https://github.com/kzahel/rstorrent/releases/tag/desktop-v0.1.3)
is commit `5e9d5441d12d72812dc438e6638eca972e43408e`; signed workflow
[`33297105275`](https://github.com/kzahel/rstorrent/actions/runs/33297105275)
passed on 2026-08-30. No route, release, tag or published artifact was changed
during this campaign.

| Asset | Independently checked SHA-256 |
| --- | --- |
| 0.1.1 Windows x64 NSIS | `7f034e481afabfee136f6b4b44a96a92997440fe2ac9942d164b7dd610bfca8e` |
| 0.1.3 Windows x64 NSIS | `0146db6833a657b3aee49fbff19b8067510994e42efebfcc6545beb555dc6fdf` |
| 0.1.1 Linux amd64 AppImage | `ebbc60f83cb65f4fdf4e360a8995313fd9bc4301666469685bbc985085dbdc4f` |
| 0.1.3 Linux amd64 AppImage | `5a008d6b8007b50ebedb9bd32da418eb47cb889cf62eb43c2bba4a5655bfabd5` |

Hashes match each public `SHA256SUMS`. Both Windows installers have valid
Authenticode signatures from Kyle Graehl. The separately downloaded 0.1.2
installers also passed hashes/signatures, but the live updater selected 0.1.3;
they are not the destination evidence.

## Windows 11 x86_64

Use a new disposable native x86_64 Windows appliance overlay, ordinary user
installation, and untouched fresh listener defaults. Public 0.1.1 launches
and presents the actual Windows Security public/private network prompt.
Choose Cancel; the resulting exact-app rules are inbound **Block/Public**.
No Allow rule or global firewall change is made. The product remains usable
and the production updater offers 0.1.3.

Actual **Review update → Install and restart** replaces the executable and
updates HKCU uninstall metadata to 0.1.3 with a valid expected-publisher
signature. The automatic relaunch exits 101: the old catalog reset calls
`File::open(directory).sync_all()` and receives Windows error 5. Repeating a
normal installed launch reproduces this before a usable window. This is a
failed signed update qualification, not a pass inferred from replacement.
Tactical 215 repairs the same defect in current source.

With the stopped application's private profile moved aside inside the
disposable appliance, the unchanged signed 0.1.3 launches from fresh state.
Its actual About panel shows version 0.1.3, build `5e9d5441d12d`, target
`x86_64-pc-windows-msvc`, Windows NSIS. No loopback workaround is used; the
native process listens on its normal wildcard port 6881. Native folder cancel
and choose succeed. The selected folder is the default after tray Quit and
normal installed relaunch. Tray Quit reaches zero product processes. A
payload sentinel created for the fresh-profile/uninstall portion survives
joined Quit and silent NSIS uninstall. Uninstall removes the installed
executable and HKCU uninstall metadata and leaves zero product processes;
no pre-update Windows sentinel existed, so this run does not
claim a pre/post-update payload comparison.

Default support guidance: Cancel is a valid choice and leaves outbound
connections usable; unsolicited incoming connections can remain blocked.
Users who want incoming peers on a trusted private network may explicitly
allow this exact app for that network. This test does not authorize or prove
public-network allowance or remote incoming reachability.

## Ubuntu 24.04 x86_64, GNOME 46 / Wayland

The public 0.1.1 AppImage launches with ordinary FUSE and default environment.
Actual Review update/Install and restart replaces that same user-writable
file and relaunches one product process. Its new hash exactly matches the
public 0.1.3 AppImage; About exposes the destination build.

Native GTK folder Cancel returns without adding a root. Choose adds the
selected folder as the default, and a normal relaunch preserves it. The
session database independently agrees with the displayed default. The
payload sentinel remains byte-identical across update, relaunch and removal.
Invoke the installed app's own **Quit RSTorrent** action through its published
D-Bus menu, observe zero product processes, relaunch, and repeat joined Quit.
Removing the owned AppImage leaves the payload sentinel unchanged.

The appliance's first-login welcome service initially timed out and ended its
desktop session. Only the disposable copy received its normal completed-
welcome marker and display-manager restart before product testing. The
bundled AppIndicator extension reports enabled but inactive, so this run
proves the actual native Quit handler, not a visible GNOME tray menu. Visible
tray affordances on this desktop configuration remain an explicit limitation.
No WebKit/GTK environment workaround was used despite renderer/module warnings.

## Limits And Cleanup

These are bounded package, updater, picker and lifecycle tests. They do not
prove the current unshipped source, supported persistence compatibility,
public-swarm throughput, store distribution or every desktop environment.
Linux was gracefully shut down and its disposable overlay discarded. The
earlier local Windows appliance that never reached guest readiness was also
gracefully shut down and discarded. Native Windows also shut down cleanly
and its overlay was discarded after successful uninstall and sentinel checks.
Tactical 215 records the passing current-source native tests. No owned guest
or product process remains.
