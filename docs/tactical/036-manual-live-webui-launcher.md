# Tactical 036: Manual Live Web UI Launcher

Status: complete; awaiting maintainer confirmation before Tauri migration.

## Motivation And Outcome

The new React inspection surface is proven through a controlled headless
gateway harness, but a maintainer cannot launch the same live surface in a
normal browser with one command. `./scripts/desktop` still selects the legacy
Tauri presentation. Add `./scripts/webui` as the manual bridge before changing
the desktop entry.

The launcher builds the locked production web assets and gateway, starts an
online application service plus a production web preview on loopback, opens
the exact live URL in the normal browser, and keeps ownership in the invoking
terminal. One `Ctrl+C` must gracefully stop and join both servers. A no-open
mode provides non-disruptive lifecycle validation.

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
- Bind control and web listeners only to loopback. The gateway keeps its exact
  Origin and opaque owner checks.
- Select `NetworkPolicy::Online` for real torrent egress by default while
  allowing the existing policy environment override for controlled checks.
- Keep one persistent, isolated local profile beneath a documented ignored
  data root. Do not share or concurrently open the Tauri profile.
- Install locked web dependencies when needed, build production web assets and
  the gateway, then serve the production build rather than a development-only
  bundle.
- Print the live URL and data root before opening the browser. Support macOS
  `open`, Linux `xdg-open`, and `--no-open`.
- Own exact gateway and web-preview process IDs. Signal graceful shutdown,
  wait for both children, escalate only for a child that refuses a bounded
  join, and remove only the launcher's exact temporary runtime files.
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

`scripts/webui` reuses the locked dependency check from the desktop launcher,
builds the production Vite bundle and gateway binary, and serves the bundle
through Vite preview. The gateway remains the explicit ephemeral-port,
unauthenticated-loopback development adapter with one exact Origin and online
engine egress. Its printed address is encoded into the `live` query before the
normal browser opener runs.

The default data root is `.local/webui`; `RSTORRENT_WEBUI_DATA_ROOT`,
`RSTORRENT_WEBUI_PORT`, and the existing `RSTORRENT_NETWORK_POLICY` provide
bounded maintainer/test overrides. `--no-open` suppresses only browser opening.
Exact web and gateway PIDs are signaled and awaited by the exit trap, with
bounded TERM/KILL escalation only if a child refuses graceful shutdown.

The automated lifecycle run used a temporary data root, loopback-only engine
policy, and `--no-open`. It fetched the printed production URL, observed the
generated asset entry, fetched authenticated-owner hello from the gateway,
and confirmed `torrent_peers` plus the advertised 300,000 ms lease. Sending
the PTY's `Ctrl+C` printed `RSTorrent web UI stopped`; both listener ports were
then closed and the wrapper removed the temporary validation data. No browser
or Tauri window was opened.

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

The implementation stopping condition is met. Tauri remains unchanged pending
explicit maintainer confirmation from a normal `./scripts/webui` run.
