# Tactical 038: Curated Test Torrent Menu

Status: complete.

## Motivation And Outcome

The live inspection toolbar can paste a magnet, but routine interactive
testing still requires finding and copying a long public URI. Add a familiar
More actions menu with a nested Add test torrent submenu. Each shortcut must
submit one of the five recorded WebTorrent free-torrent magnets through the
same semantic add path as typed input.

The stopping result is an accessible pointer, keyboard, and touch-friendly
menu containing Big Buck Bunny, Cosmos Laundromat, Sintel, Tears of Steel, and
WIRED CD shortcuts, with exact catalog drift protection and headless visual
evidence in the production live surface.

## Dependencies And References

- [`037-live-magnet-toolbar-intake.md`](037-live-magnet-toolbar-intake.md)
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../../tests/live/torrents.json`](../../tests/live/torrents.json), the
  machine-readable owner of the retained public magnets and provenance.
- [`../test-torrents.md`](../test-torrents.md), the human-readable provenance,
  licensing caution, variability, and evidence policy.
- JSTorrent sibling revision `9895410beeed6aff554053769bd006a3fbd373ef`,
  `packages/client/src/AppContent.tsx::moreMenuItems` and
  `packages/ui/src/components/DropdownMenu.tsx`.

Retain JSTorrent's familiar More trigger and small menu density. Improve the
current flat developer-only list into the requested submenu and do not copy
its inline styles, mutable hover styling, or incomplete keyboard behavior.

## Scope And Invariants

- Add More after the live torrent controls. It remains available without a
  selected torrent because test-torrent intake is not selection-dependent.
- Add one nested Add test torrent item containing exactly the five catalog
  entries, with concise labels and the full canonical magnets.
- Keep the recorded JSON catalog authoritative. A deterministic test must fail
  if the checked-in frontend projection differs in identity, name, ordering,
  or magnet bytes.
- Reuse the same guarded `add_magnet` frontend path, generated live-adapter
  mapping, application validation, storage root, busy policy, status output,
  and success/error semantics as typed input. Do not create a debug command or
  bypass durable application control.
- A shortcut must not overwrite or clear a magnet draft in the adjacent text
  input. Disable the More trigger while any add is in flight.
- The menu owns identifiable open/submenu state and no background task. Close
  on selection, outside pointer action, Escape, or Tab. Restore sensible focus
  for keyboard closure and support Arrow Up/Down, Home/End, Right into the
  submenu, and Left back to its parent.
- Use semantic button/menu roles, `aria-haspopup`, `aria-expanded`, visible
  focus, CSS Modules, bounded DOM, and a phone layout that nests rather than
  opening off-screen.
- Public swarm health is not implied by the shortcut. Failure remains visible
  application feedback and does not change capability claims.

## Non-Goals

- automatically adding all test torrents;
- executing a variable public download as a deterministic gate;
- synthetic size fixtures, Ubuntu, arbitrary user-defined shortcuts, recent
  items, or persisted menu customization;
- selection-based recheck, removal, copy, share, or other future More actions;
- changing the one-active-download application policy;
- `.torrent` file selection, Tauri migration, Android UI, or categorized Logs.

## Validation

- A catalog test compares the frontend projection byte-for-byte with
  `tests/live/torrents.json`.
- Component tests drive the nested menu with keyboard and pointer input,
  inspect focus/expanded state, and assert the exact semantic magnet command.
- The controlled production browser/libtorrent proof opens the nested menu,
  captures it at wide and phone width, checks accessibility and closure, then
  continues its deterministic transfer without adding a public torrent.
- Run TypeScript, Vitest, production build, ordinary and controlled
  Playwright, generated drift, formatting, clippy, and workspace tests.

## Stopping Condition

The live UI exposes all five exact catalog magnets beneath More > Add test
torrent, every shortcut uses the ordinary guarded add path, responsive and
accessible interaction has automated evidence, documentation is current, and
the committed tree is clean.

## Implemented Result

- Added More after the live Start/Pause controls and a nested Add test torrent
  submenu with Big Buck Bunny, Cosmos Laundromat, Sintel, Tears of Steel, and
  WIRED CD. Named demo scenarios remain deterministic and do not expose live
  shortcuts.
- Added an independently typed projection of the recorded catalog. It builds
  the exact retained magnets from bounded static fields, and its test compares
  name, identity, ordering, and every magnet byte with
  `tests/live/torrents.json`.
- Kept test adds on the existing semantic `add_magnet` path. Typed and shortcut
  intake now share one synchronous in-flight guard; the shortcut preserves an
  unfinished text-input draft, closes while adding, reports application
  success or failure, and restores focus only after More becomes usable.
- Added explicit menu ownership with outside-pointer, Tab, Escape, arrow,
  Home/End, submenu-entry, and submenu-return behavior. The desktop submenu
  flies toward available space; at phone width it nests inside the root menu
  instead of leaving the viewport.
- The production controlled browser proof opens the menu by keyboard at wide
  width and by pointer at phone width. It does not select a variable public
  torrent, then continues the existing deterministic libtorrent-seeded
  transfer and cleanup proof.

## Evidence

- `npm run typecheck --prefix clients/web`: pass.
- `npm test --prefix clients/web -- --run`: 38 passed, 2 skipped opt-in cases.
- `npm run build --prefix clients/web`: pass against the production bundle.
- `npm run test:e2e --prefix clients/web` against an isolated Vite host: 4
  ordinary browser cases passed; the opt-in controlled live case skipped.
- `uv run --project tests/interop --locked python
  tests/interop/browser_peer_inspection_surface.py --screenshot-dir
  target/headless-evidence/t038-test-torrent-menu`: pass. The production bundle
  showed both menus, found all five items, passed the open-menu accessibility
  scan with no serious or critical findings, then completed and hash-verified
  the three-piece controlled payload, recovered its view set, joined gateway
  shutdown, and cleaned its profile. Wide and phone menu captures are retained
  as ignored local evidence.
- `npm run generate --prefix clients/web`: pass with no generated drift.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace -- -D warnings`: pass.
- `cargo test --workspace`: 278 passed, 3 ignored live-network cases.

The functional stopping condition is met. Public-torrent completion remains
variable opt-in evidence; other More actions, `.torrent` file selection,
Tauri migration, and categorized Logs remain separate slices.
