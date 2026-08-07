# Tactical 036: Manual Live Web UI Launcher

Status: Complete; launcher topology superseded by Tactical `109`.

## Motivation And Outcome

This tactical introduced `./scripts/webui` as the one-command normal-browser
bridge to the React inspection surface. Tactical `109` retains that outcome
while replacing the original two-listener bootstrap topology.

The launcher now builds the locked production web assets and gateway, starts
one online application service on a stable loopback origin, and lets that
gateway serve both the UI and application transport. It opens the plain root
URL and keeps ownership in the invoking terminal. One `Ctrl+C` gracefully
stops and joins the process. A no-open mode provides non-disruptive lifecycle
validation.

## Dependencies And Owning Topics

- [`035-live-peer-inspection-projection.md`](035-live-peer-inspection-projection.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../topics/desktop-inspection-surface.md`](../topics/desktop-inspection-surface.md)
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md)

This is launcher and client-lifecycle work. It changes no engine protocol and
therefore requires no new BEP or libtorrent behavior survey.

## Scope And Invariants

- Use the implemented `LiveApplication` and explicit unauthenticated loopback
  development gateway; do not create another API or product daemon.
- Bind the one hosted application listener only to loopback. The gateway keeps
  its exact Origin and opaque owner checks.
- Select `NetworkPolicy::Online` for real torrent egress by default while
  allowing the existing policy environment override for controlled checks.
- Keep one persistent, isolated local profile beneath a documented ignored
  data root. Do not share or concurrently open the Tauri profile.
- Install locked web dependencies when needed, build production web assets and
  the gateway, then serve the production build rather than a development-only
  bundle.
- Print the stable root URL and data root before opening the browser. Support
  macOS `open`, Linux `xdg-open`, and `--no-open`.
- Own the exact gateway process ID. Signal graceful shutdown, wait for the
  child, escalate only if it refuses a bounded join, and remove only the
  launcher's exact temporary runtime files.
- A browser tab may remain open after server shutdown and must recover on the
  next launch; the launcher does not control or close the user's browser.

## Non-Goals And Next Gate

This slice does not switch Tauri to the React inspection surface, add an
add-magnet control, migrate legacy controls or logs, change Android, expose a
LAN listener, or establish production remote authentication. The live React
surface currently supports inspection plus pause/resume and truthfully retains
the existing unsupported scaffolds.

After automated no-open lifecycle evidence and maintainer-visible confirmation
of `./scripts/webui`, stop for direction. A later tactical may switch Tauri to
the new application adapter and should then decide whether the categorized
logger is migrated or redesigned using JSTorrent's Logs view as the product
reference.

## Validation

- Shell syntax and repository formatting checks.
- Start `./scripts/webui --no-open` with an isolated temporary data root and
  loopback-only engine policy.
- Fetch the printed UI URL and verify it serves the production application.
- Send `SIGINT`, require clean zero-resource shutdown, verify both listener
  ports close, and remove the temporary data root.
- Do not open a visible browser during automated validation. The maintainer
  owns the subsequent normal-browser confirmation.

## Implementation And Evidence

`scripts/webui` reuses the locked dependency check from the desktop launcher
and builds the production Vite bundle with its same-origin hosted default plus
the gateway binary. One fixed-port, unauthenticated-loopback development
gateway serves the immutable bundle, health route, HTTP API, and application
WebSocket with one exact Origin and online engine egress. The browser opener
receives only that origin's plain root URL.

The default data root is `.local/webui`; `RSTORRENT_WEBUI_DATA_ROOT`,
`RSTORRENT_WEBUI_PORT`, and the existing `RSTORRENT_NETWORK_POLICY` provide
bounded maintainer/test overrides. `--no-open` suppresses only browser opening.
The exact gateway PID is signaled and awaited by the exit trap, with bounded
TERM/KILL escalation only if it refuses graceful shutdown.

Tactical `109` revalidated the launcher with a temporary data root, port
`44177`, online engine policy, and `--no-open` while leaving the active
`4177` process untouched. The root and health routes came from the gateway,
an owner-bearing application hello succeeded, and headless Chrome remained at
the root URL while opening exactly one same-origin application WebSocket.
Sending the PTY's `Ctrl+C` printed `RSTorrent web UI stopped`, closed the
listener, and joined the gateway. The temporary profile was removed.

Validation commands also include:

```text
bash -n scripts/webui
./scripts/webui --help
npm run typecheck --prefix clients/web
npm test --prefix clients/web -- --run
npm run build --prefix clients/web
git diff --check
```

`shellcheck` was unavailable on this host and therefore is not claimed as
executed evidence.

## Stopping Condition

The implementation stopping condition is met. Tactical `109` owns the current
stable same-origin launcher contract; Tauri remains unchanged.
