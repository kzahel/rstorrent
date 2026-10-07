# Tactical 264: Hosted Website Dependency Qualification

Status: Active, 2026-10-08. Bounded follow-up to Tactical 259/263.

Topics: `beta-release-readiness`, `product-surfaces-and-migration`.

## Scope, invariants and stopping condition

Qualify the sibling JSTorrent hosted website's dependency graph against current
advisories. Preserve all existing major-version ranges, public routes, form and
analytics recipients, disabled richer-context gates and replacement source's
separate engine architecture. Do not repair unrelated retired-engine workspace
advisories under this slice or publish the website. Keep unrelated untracked
files, existing installed dependency links and personal environment intact.

Stop with inspected targeted website lock/manifest changes, fresh graph/version-
scoped advisory evidence, an isolated corrected-source website build, hosted
query/privacy security tests, responsive captures and a local commit. Other
workspace findings remain explicit; name-only graph matching is insufficient
because a different installed version may be unaffected. Preserve the cache
source-behavior assessment from 263; zero registry entries is bounded evidence.

## Source checkpoint and validation plan

Initial full-workspace audit reports 69 dependency paths, which is not a count
of website vulnerabilities. Inspect the selected website dependency versions
and exact npm advisory paths. Website currently permits Astro ^7.1.3 and existing
React/Solid integrations; selected updates remain within these ranges.
The initial normal update refuses installed public-hoist configuration before
mutation. A targeted lockfile-only update succeeds, avoiding replacement of the
user's existing workspace modules. Validate the modified graph in a task-owned
archive checkout before adopting it; no branch/worktree or inherited state reset.
