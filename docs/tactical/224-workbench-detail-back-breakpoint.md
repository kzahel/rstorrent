# Workbench Detail Back Breakpoint

Status: Complete (2026-09-27).

Topics: `web-ui-design`, `application-interface-direction`.

## Scope and stopping condition

Workbench changes to a list-or-detail layout at 760 px, but the detail
header's return-to-list button appears only at 700 px. Align those
presentation breakpoints so every width that hides the torrent collection
shows a way back. Keep current row activation, selection, commands, and
detail state unchanged; the broader action interaction remains a separate
product discussion.

Check the transition at 700, 701, 760, and 761 px, including resizing while
detail is open and returning to the list. Run web typecheck, tests, a focused
browser check, and `git diff --check`. No API, engine, Android, or background
task change is involved.

## Evidence

- DetailPane now shows its Torrents return control through 760 px, matching
  the width where Workbench hides the collection. Other detail layout rules
  retain their 700 px threshold.
- Focused Playwright passed direct list-to-detail and back navigation at
  700, 701, 730, and 760 px. It also passed resizing an open detail across
  the 760/761 px boundary, plus the existing 390 px phone navigation case.
- `npm run typecheck --prefix clients/web`, `npm test --prefix clients/web`
  (390 passed, two skipped), and `git diff --check` passed.
- No command behavior, selection semantics, or API contract changed.
