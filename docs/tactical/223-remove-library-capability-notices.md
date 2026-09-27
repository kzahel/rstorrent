# Remove Library Capability Notices

Status: Complete (2026-09-27).

Topics: `web-ui-design`, `application-interface-direction`.

## Scope and stopping condition

Remove implementation-status copy from the Library sidebar and empty state.
Keep the Library filters, counts, empty-state result message, and documented
capability boundaries. Delete unused styles and localization entries. No
command, state, API, engine, or platform behavior changes.

Run web typecheck, tests, localization validation, and `git diff --check`.
The slice ends when Library no longer displays those capability notices and
the owning topics reflect the presentation change.

## Evidence

- Removed the Library sidebar's quoted capability notice and the matching
  implementation-status line in the Library empty state. The filters, counts,
  and empty-state result message remain.
- Deleted both unused localization entries and the now-unused sidebar style.
- `npm run typecheck --prefix clients/web` passed.
- `npm test --prefix clients/web` passed: 390 tests, two skipped.
- `npm run check:localization --prefix clients/web` and `git diff --check`
  passed. No browser interaction or application command behavior changed.
