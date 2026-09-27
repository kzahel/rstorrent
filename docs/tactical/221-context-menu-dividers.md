# Context Menu Dividers

Status: Complete on 2026-09-27.

Topics: `web-ui-design`, `table-interaction`.

## Motivation And Outcome

Torrent and file action menus repeat action meaning with headings such as
Transfer, Sharing, Open, and Priority. The headings add height and noise without
clarifying the commands. Replace visible section headings with the existing
subtle divider between nonempty action groups in row context menus and their
shared More-menu presentations.

This user-directed presentation change supersedes Tactical `085`'s menu
heading treatment while retaining its stable action order and shared action
definitions.

## Scope And Invariants

- Preserve exact action labels, icons, order, availability, target sets,
  callbacks, and toolbar/context parity.
- Render exactly one divider between adjacent nonempty groups. No leading,
  trailing, or doubled dividers when a conditional group is absent.
- Keep the shared overlay primitives available; flatten only the torrent and
  file action renderers that currently emit category headings.
- Maintain keyboard, pointer, phone viewport, and accessibility behavior.
- No engine, application command, generated contract, storage, Android, or
  protocol change. No dependency or background task is added.

## Validation And Stopping Condition

Component tests assert order, conditional divider counts, and absence of
category headings. Focused Playwright checks exercise torrent/file context
menus and phone placement. Web typecheck, tests, build, and `git diff --check`
pass. Update the owning topics with the simplified presentation.

## Implemented Result And Evidence

- Torrent and file action renderers now emit flat action items with one
  existing low-contrast divider between nonempty groups. Shared action
  definitions, labels, order, enablement, targeting, and callbacks are intact.
  The overlay section primitive remains available for menus that need it.
- Component tests cover the nine torrent actions with three dividers, the
  conditional file menus with one divider, and absence of the former headings.
- Two focused Playwright cases passed for torrent/file context menus and the
  wide Files surface. The context-menu case also passed serious/critical Axe
  scans. Temporary wide captures were inspected for both menus and removed.
- `npm run typecheck --prefix clients/web` passed.
- `npm test --prefix clients/web` passed: 389 tests, two skipped.
- `npm run build --prefix clients/web` passed with its CSP bundle check and the
  existing large-chunk advisory.
- `npm run check:localization --prefix clients/web` and `git diff --check`
  passed. The removed file-menu headings' unused message keys were deleted.
