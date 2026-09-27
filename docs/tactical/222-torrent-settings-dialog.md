# Torrent Settings Dialog

Status: Complete (2026-09-27).

Topics: `web-ui-design`, `table-interaction`,
`settings-mutation-and-draft-consistency`.

## Goal and scope

Move the per-torrent peer transfer limits out of the Workbench General detail
pane. Open a compact Torrent settings dialog for one torrent from its row
context menu or the shared More actions menu. The current upload and download
limit fields are the dialog's first settings. Keep typed sparse patches,
draft/revision convergence, validation, and saved/error feedback.

The shared action is disabled for zero or multiple targets. Context targeting
continues to follow the table's existing selection rules. Cancel or Escape
discards the draft and returns focus to the invoking row or toolbar control.
The dialog remains usable across desktop and phone widths.

No engine, API, persistence, Android, or global limit change is in scope. No
dependency or background task is added.

## Validation and stopping condition

Component tests cover context and More entry, exact target, sparse/combined
patches under live row updates, multiple-selection availability, and focus
return. A focused browser test covers wide and phone layout, keyboard and
accessibility behavior. Web typecheck, tests, build, localization check, and
diff check pass. Update owning topics after implementation.

## Implemented result and evidence

- The shared torrent action list presents **Torrent settings** in both row
  context and More menus. Zero or multiple targets disable it; a selected row
  retains the table's existing exact-target semantics.
- The dialog owns the existing per-torrent upload/download draft and sparse
  `update_torrent_settings` patch. Workbench General no longer displays the
  transfer-limit card. Cancel and Escape return focus to the row or More
  trigger; the dialog remains mounted across live row updates.
- Component tests cover action order, single-target availability, both menu
  entry points, exact target, focus return, combined patch, and a dirty draft
  through 24 complete row updates. The full web suite passed: 390 tests,
  two skipped.
- Focused Playwright passed the context action case and the dialog case at
  wide, compact, and phone widths. The phone dialog capture was inspected;
  Axe found no serious or critical violations while the modal was open.
- Web typecheck, production build with CSP check, localization check, and
  `git diff --check` passed. No Rust contract changed, so generation and
  engine tests were outside this frontend presentation slice.
