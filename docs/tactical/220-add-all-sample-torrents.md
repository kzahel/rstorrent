# Add All Sample Torrents

Status: Complete on 2026-09-27.

Topics: `web-ui-design`, `application-interface-direction`.

## Motivation And Stopping Condition

The live More > Add test torrent submenu exposes five recorded public magnets
one at a time. Add an explicit **Add all sample torrents** item in that submenu.
One activation should submit every current catalog entry through the existing
semantic add path, preserve any unfinished magnet draft, and report the outcome
of every entry. The slice ends when component tests prove direct and
options-dialog adds, browser tests verify submenu access, the owning topic is
current, and web validation passes.

This user-directed slice supersedes Tactical `038`'s earlier non-goal of
automatic all-catalog intake. The action remains explicit in the submenu.

## Scope And Invariants

- Use `WEBTORRENT_TEST_TORRENTS` in its recorded order. No second list or
  constructed batch protocol command is introduced.
- Use one available default root or one shared Add options dialog choice for
  all five. Cancel before confirmation submits none. Apply file-selection and
  start-content settings uniformly; save the "don't show again" preference
  once after the batch.
- Submit one `add_magnet` command at a time. Continue after individual failures
  and report counts of newly added, already present, and failed entries, with
  bounded failure detail. Duplicate adds retain the existing idempotent server
  semantics.
- Hold the existing frontend add busy guard for the whole batch. Keep the More
  and Add controls disabled during submission and preserve the typed draft.
- The action is explicit and limited to the five public catalog entries. It
  changes no protocol, engine, storage, persistence, generated contract, or
  Android behavior.

## Ownership And Validation

`MoreActionsMenu` renders the action. `TorrentActions` owns the root/options
choice and cancellation before submit. The application-lifetime
`TorrentActionProvider` owns the single batch runner and progress status, so
navigation between Transfers and Workbench does not detach the operation. It
checks its mount state between commands and stops submitting on application
unmount. The application command remains the owner of each durable add.

Test ordered commands, draft preservation, duplicate and partial-failure
summary, busy behavior across destination changes, one dialog choice and
cancellation, and keyboard or pointer submenu behavior. Run web typecheck,
tests, focused Playwright overlay checks, production build, and
`git diff --check`. Public-swarm activity is not a validation gate.

This is frontend composition of existing commands and catalog data. The
protocol source survey from Tactical `038` remains applicable; no new engine
behavior calls for a fresh libtorrent survey.

## Implemented Result And Evidence

- The submenu leads with **Add all sample torrents**, followed by a separator
  and the existing five entries. The named demo adapter does not expose it.
- One root/options choice applies to five ordered `add_magnet` calls. The
  application-lifetime runner retains the busy state and progress through
  Transfers/Workbench navigation, continues after per-item failures, and
  distinguishes duplicate no-ops from new adds in the final status.
- The Add options dialog names the batch, supports cancellation before any
  command, and saves the existing preference once after the batch. The typed
  magnet draft remains untouched by the batch action.
- `npm run typecheck --prefix clients/web` passed.
- `npm test --prefix clients/web` passed: 389 tests, two skipped. Component
  tests prove catalog order, one in-flight command, navigation continuity,
  draft preservation, duplicate and failure counts, dialog cancellation,
  shared root/start policy, and one preference write.
- `npm run build --prefix clients/web` and its CSP bundle check passed. Vite
  retained its existing large-chunk advisory.
- Two focused Playwright overlay tests passed against an isolated Vite server,
  covering phone submenu focus and viewport placement plus pointer closure.
  No public-swarm add was run.
- `git diff --check` passed.
