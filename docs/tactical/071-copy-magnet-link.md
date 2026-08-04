# Tactical 071: Copy Magnet Link

Status: Complete.

Topics: `web-ui-design`, `client-surfaces`, `table-interaction`

## Motivation And Outcome

The torrent toolbar's More menu currently contains only curated test-torrent
intake. Add a selection-aware **Copy magnet link** action so a user can copy a
portable v1 magnet for one selected torrent from the same menu.

The stopping result is a keyboard, pointer, and assistive-technology operable
menu item that is enabled for exactly one selected torrent, copies a bounded
canonical `magnet:?xt=urn:btih:<info-hash>` URI, closes the menu, restores
focus, and reports success or clipboard failure through the existing polite
toolbar status.

## Existing Source Semantics

RSTorrent does not retain the submitted magnet URI byte-for-byte. The session
store parses it and persists a canonical URI containing the v1 identity plus
supported explicit peer hints and UDP tracker endpoints. Parameter spelling,
ordering, display-name fields, unsupported fields, and equivalent encodings
are therefore not preserved exactly.

This slice deliberately synthesizes the shareable link from the trusted
torrent-summary info hash already present in the UI. That is truthful for
current magnet intake and will also remain correct when future `.torrent`
intake supplies the same v1 identity. Exact original-source export, retained
tracker/display-name reconstruction, and `.torrent` ingestion remain separate
application and persistence decisions.

## Dependencies And References

- [`038-curated-test-torrent-menu.md`](038-curated-test-torrent-menu.md)
- [`069-current-within-table-selection.md`](069-current-within-table-selection.md)
- [`../topics/web-ui-design.md`](../topics/web-ui-design.md)
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md)
- [`../topics/table-interaction.md`](../topics/table-interaction.md)

This is a presentation-only convenience over an already projected v1
identity. It changes no BitTorrent protocol, engine state, application command,
view contract, persistence format, or platform adapter, so no normative BEP or
pinned libtorrent survey is required.

## Scope And Invariants

- Add **Copy magnet link** to the existing torrent-toolbar More menu.
- Enable it only when the complete checked selection contains exactly one
  current torrent. Multi-selection never chooses an implicit member.
- Build exactly `magnet:?xt=urn:btih:<info-hash>` from that row. Do not include
  volatile peers, tracker observations, a UI-derived name, or hidden source
  fields.
- Use the platform clipboard API only after explicit user activation. Treat a
  missing or rejected clipboard capability as failure rather than claiming a
  copy occurred.
- Close on activation, preserve the menu's keyboard ownership, and restore
  focus to More after the asynchronous copy attempt settles.
- Report **Magnet link copied** on success and one bounded actionable failure
  through the toolbar's existing polite status output.
- Keep Add test torrent available without a selected torrent and while copy is
  disabled. Keyboard traversal skips disabled menu items.
- Add no dependency, background task, durable state, or generated contract.

## Non-Goals

- exact byte-for-byte original magnet retention or export;
- copying one link per row in a multi-selection;
- including display name, trackers, peer hints, web seeds, or private source
  material in the synthesized URI;
- `.torrent` file intake, `.torrent` export, Share-sheet integration, QR codes,
  or Android Compose presentation;
- changing selection, current-row, archive, removal, or add behavior.

## Validation

- Component tests assert the exact copied URI, success feedback, menu closure,
  focus restoration, disabled zero/multiple-selection behavior, and a rejected
  clipboard write.
- Existing More-menu keyboard tests retain nested-submenu traversal and prove
  disabled items do not trap initial focus.
- A deterministic browser test copies the current torrent's link through the
  production UI, reads the granted clipboard, and retains accessibility and
  bounded-layout checks in proportion to the change.
- Run TypeScript checking, Vitest, the production build/CSP scan, focused
  Playwright, and `git diff --check`.

## Stopping Condition

The shared browser/Tauri React surface can copy the selected torrent's
canonical v1 magnet through More with truthful feedback and accessible menu
behavior; deterministic tests and proportionate web gates pass; owning docs
record the implemented limitation and evidence.

## Implemented Result

- Added **Copy magnet link** as the first item in the torrent toolbar's More
  menu. It is available for exactly one selected torrent and deliberately
  disabled for an empty or multi-row selection.
- The presentation synthesizes the bounded v1 URI directly from the selected
  `TorrentRow.infoHash`. It does not add source fields to every library row or
  change the application command, view, database, or generated client
  contracts.
- Clipboard writes happen only after explicit activation. Success reports
  **Magnet link copied**; a missing or rejected clipboard API reports
  **Could not copy magnet link: ...** without claiming success.
- The menu closes before the asynchronous write and restores focus to More
  afterward. Root-menu keyboard traversal skips disabled items, preserving
  direct entry into Add test torrent when live intake has no selection.
- Named demo scenarios expose the truthful local copy action but continue to
  hide the live-only curated test-torrent submenu. Live mode retains that
  submenu even with no selected torrent.

## Evidence

- `npm run typecheck --prefix clients/web`: pass.
- `npm test --prefix clients/web -- --run`: 135 passed, 2 expected opt-in
  skips.
- `npm run build --prefix clients/web`: pass, including the production CSP
  scan of both JavaScript bundles.
- Focused production-bundle Playwright copy test: pass. It granted explicit
  clipboard read/write permission, copied the exact expected URI, read it
  back, checked menu closure/focus restoration and multi-selection disabling,
  and found no serious or critical Axe violations while the menu was open.
- Full deterministic Playwright run: the new test and 16 other cases passed;
  five opt-in live cases skipped. The pre-existing phone Swarm summary
  `scrollable-region-focusable` finding was the only failure, matching the
  deferral already recorded by Tactical `068` and unrelated to this slice.
- `git diff --check`: pass.

The stopping condition is met. Exact original-source retention and richer
magnet reconstruction remain explicitly outside this presentation-only slice.
