# Tactical 230: Desktop Stable and Latest Update Channels

Status: Active. Source, production channel registry, and the first signed
Latest release are deployed; installed acceptance remains open.

Topics: `beta-release-readiness`, `client-surfaces`, `product-state-and-feedback`,
`product-surfaces-and-migration`, `web-ui-design`.

## Decision and scope

Give installed RSTorrent desktop users an explicit Stable or Latest update
choice. Stable retains the existing `desktop-v*` release and update route.
Latest uses signed `desktop-latest-v*` prereleases from verified `main` source,
with the same updater key, application identity, package matrix, and explicit
install/relaunch policy. Follow the proven
`desktop-release-kit/contract/desktop-update-channels-v1.md` contract and the
existing `simple-app-update-server` channel registry. The server advertises
channels, confirms explicit requests, and selects candidates numerically.

The source slice includes the product descriptor, a nightly selector and
reusable signed workflow, persisted native channel choice, UI selection,
candidate invalidation, and local source tests. The first signed Latest
publication, production server deployment, and installed old-to-new campaign
are separate operational acceptance steps and are not inferred from source
tests.

## Non-goals

- Changing Android, iOS, headless, Crostini, or JSTorrent release channels.
- Changing the updater key, product identifier, user profile, or torrent state.
- Automatic installation, downgrade, or reclassification of an existing
  prerelease as Stable.
- Publishing a signed release or deploying the update service in this source
  change.

## Invariants and limits

- Missing or invalid saved selection means Stable. Old clients and requests
  without `channel` continue to resolve Stable.
- Latest requires successful channel discovery and explicit server confirmation;
  an older server that ignores the query cannot masquerade as Latest.
- Check-only `X-CFU-Id` and reason headers remain on the product update route;
  the native candidate clears them before fetching public GitHub assets.
- A channel switch immediately removes the prior candidate and invalidates
  outstanding checks. Selection and installation are mutually exclusive.
- Returning to Stable never downgrades a newer installed build. The UI reports
  that Stable must catch up.
- Nightly selects an exact successful `main` CI commit, skips unchanged packaged
  source unless forced, and uses a numeric version train within native package
  limits. A failed or incomplete draft stays private.
- Signing, notarization, Authenticode, five updater targets, checksums, and
  exact release validation remain mandatory for both channels.

## Ownership and evidence

The desktop native adapter owns the persisted choice, endpoint selection, and
retained signed candidate. The React updater controller owns presentation,
check scheduling, and dismissal. The release workflow owns immutable build
identity and draft finalization. No engine or generated application contract
changes. Validate deterministic version selection, channel switching and stale
responses, server confirmation, release configuration, and full web/Rust gates.
Hosted package evidence and installed package evidence are recorded after an
authorized publication.

## Stopping condition

Source behavior and local gates pass; the production registry, first signed
Latest release, and all five update routes are proven. Installed
Stable-to-Latest selection, replacement/relaunch, and return-to-Stable without
downgrade remain the final acceptance gate.

## Source evidence and remaining gates

- Native channel preference, legacy fallback, explicit response confirmation,
  and Stable catch-up tests pass in the focused desktop library suite (5 tests),
  including an isolated temporary source snapshot after unrelated work made
  the shared tree temporarily uncompilable.
- Focused web updater/controller/presentation tests pass (12 tests), and
  `npm run typecheck --prefix clients/web` passes. In an isolated snapshot of
  the staged commit, the full web suite passes (395 tests; 2 skipped). The
  current shared worktree separately has 34 failures in concurrently edited
  navigation/state scenarios (`App.test.tsx` and `state.test.ts`).
- Nightly identity, exact successful CI selection, package-input filtering,
  release input routing, version application, release configuration, and
  complete draft validator tests pass (23 tests). `actionlint` passes the
  nightly, signed release, and CI workflows.
- `cargo fmt --all -- --check`,
  `cargo clippy -p rstorrent-desktop --no-default-features -- -D warnings`,
  and `cargo test -p rstorrent-desktop --no-default-features` pass in the
  current shared tree (50 desktop library tests). Full workspace Rust gates
  remain outside this focused source check.
- On 2026-09-27, commit `1b13a550` was pushed and fast-forwarded into Remy's
  clean RSTorrent checkout. The product-owned descriptor remains symlinked from
  `simple-app-update-server/products.d`; the service was restarted and active.
  Public `/rstorrent/channels` returns Stable and Latest (HTTP 200).
- The first two signed publication attempts remained private after release
  workflow failures. The corrected source commit `30523da4` passed main CI
  [`36329145890`](https://github.com/kzahel/rstorrent/actions/runs/36329145890).
  Nightly run
  [`36331465234`](https://github.com/kzahel/rstorrent/actions/runs/36331465234)
  passed its source gate, five signed package jobs, and sole finalizer. It
  published prerelease `desktop-latest-v0.2.301` from that exact commit.
- Independent public release and production route checks found all 15 updater
  platform keys, 13 checksummed assets, HTTP 200 Latest `0.2.301` metadata on
  all five Tauri targets, HTTP 204 for current `0.2.301`, and continued Stable
  `0.1.4` behavior with and without a channel query. See
  [`desktop-latest-v0.2.301`](../evidence/desktop-latest-v0.2.301.md).
  Installed cross-channel acceptance remains open.
