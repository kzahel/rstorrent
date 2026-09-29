# Tactical 239: Desktop Suspension And Browser Recovery

Status: **Active, 2026-09-29.** Campaign 231; follows 234/237.
Topics: `client-surfaces`, `runtime-configurations-and-headless-deployment`,
`application-connection-architecture`, `desktop-jstorrent-replacement`.

## Scope And Stopping Condition

Use the final installed beta packages and fresh controlled roots on claimed
macOS/Windows/Linux guests. Close/restart the separate test browser during a
private bounded transfer, reopen its existing extension page attach-only and
prove the same runtime/library, tray and converged state. Verify complete
payload with an independent SHA-256 and authoritative piece counts.

Query real guest sleep/wake capabilities first. Exercise supported target
suspension/resume and actual guest sleep/wake only with a supported recovery
path. VM snapshot/pause, native OS sleep, browser closure and discarded-page
freezing are distinct evidence classes; never relabel one as another. No
controller sleep, outer input, unsupported hypervisor mutation or policy bypass.

Stop after the bounded available-platform recovery matrix, explicit Quit plus
browser restart without resurrection, fresh user-intent relaunch, resource
samples and cleanup. Native OS sleep unavailable in virtual hardware remains
a physical-platform qualification gap. No broader endurance or release claim.

## Contracts And Ordered Work

Keep 234's one runtime, persistent tray, attach-only passive reconnect and
native tray Open rules. Reuse its 32-MiB independently generated fixture and
bounded libtorrent seed; no public swarm, personal content or importer.
One task owns the seed, server, browser and any exact guest-access firewall
rule; close/join, delete owned artifacts and release claims at completion.
No engine/protocol algorithm or Android behavior change is planned.

Review 165's power policy, current connection/page cleanup, pinned keepawake
platform adapters and official Apple/Windows/Linux power documentation before
power experiments. Preserve settings and distinguish a user-forced suspension
from idle-sleep inhibition. Run scripts and affected builder tests before
installed experiments. A discovered engine fix requires the engine campaign
source/test oracle and Android parity analysis before implementation.

For efficiency, a platform's 239 cases may reuse 237's still-owned controlled
installation before cleanup; evidence remains under this separate tactical.

## Initial Platform Evidence

Reviewed pinned keepawake 0.6.1 `src/sys/{macos,windows}.rs`, Apple's installed
`pmset(1)` scheduling/sleep contract and IOKit `IOReturn.h`, official Windows
[SetThreadExecutionState](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate)
and Chrome [worker lifecycle](https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle).
Active downloading holds an idle-system-sleep assertion, not a system-sleep
or display assertion. No worker keepalive is introduced.

Windows 11 build 26200 virtual firmware reports every sleep/hibernate state
unavailable (`powercfg /a`); Machine Control also does not advertise provider
suspension there. macOS 26.6.2 advertises sleep, but root `pmset sleepnow`
returns `0xe00002e2` (`kIOReturnNotPermitted`). The exact task-owned scheduled
wake is cancelled afterward; no inherited wake event existed or was removed.
These are testbed limitations, not passing native sleep tests. Supported Tart
suspend/resume is exercised separately without claiming native OS wake events.

The new opt-in `verify-desktop-suspension-recovery.mjs` uses attach-only
bootstrap, bounded six-second replies and 180-second progress observations.
It records sanitized root/torrent identities, verifies the same runtime and
non-regressing piece counts after recovery, and independently hashes the
32-MiB controlled payload at completion. It accepts the existing paused
compatibility fixture beside the active transfer. Its first live record
found ambiguous text `connected` (connection health plus a live peer); scope
the wait to the connection div, leaving authenticated identity checks intact.

## macOS Transfer Recovery

The first VM pause run is **invalid as transfer-recovery evidence**: the
fixture bound libtorrent to port zero and captured that port once in its
tracker response. Host interface refresh reopened its listener on a new port,
while the tracker kept advertising the closed old port. The UI truthfully
showed the sole peer failure-limited after three failures; 119/512 pieces
remained verified. Reachable HTTP tracker and live runtime were insufficient
to establish a healthy seed. No engine change was made.

The fixture now selects one ephemeral port before session creation, uses that
fixed port with `listen_system_port_fallback=false` and
`max_retry_port_bind=0`, and checks the live listener on every tracker reply.
A missing listener returns HTTP 503 rather than a stale compact peer. Its
receipt includes listener/tracker ports. Reviewed pinned libtorrent
`7d7fc38f` `include/libtorrent/settings_pack.hpp` listen-interface and fallback
contracts and `src/session_impl.cpp` listener reopening. This changes only
independently authored test infrastructure, not product networking or Android.

Repeated with a new owned transfer in the same native-selected root. The
runner recorded 2/512 verified pieces, then common `target suspend` and
`target ensure-ready` preserved runtime identity and reached 41 pieces.
`verify-desktop-intent-lifecycle.mjs browser-close`, a real test-browser restart
with `--restore-last-session`, and attach-only recovery reached 69 pieces
with unchanged runtime/root/torrent identity and intent. Removing the temporary
64-KiB/s client limit completed all 512 pieces within the 180-second deadline.
Independent file read verified 33,554,432 bytes and SHA-256
`99080b09c925782f67975d36476f171ee4e8b367e2a893d07a88bd70028b3fe8`.
The seed retained its fixed listening port and an established guest connection
after resume. Native tray Show also opened the same library while the browser
was closed in the first run. One sample was 142,256 KiB RSS; this is a sample,
not an endurance bound. Joined Quit/browser restore checks and final cleanup
are still pending. Windows/Linux recovery remains pending.
