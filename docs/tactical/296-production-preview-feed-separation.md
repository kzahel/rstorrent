# Tactical 296: Production and Preview Feed Separation

Status: Active, 2026-10-10. Beneath shipment295 and finish-line259.
Owners: product-surfaces-and-migration and beta-release-readiness.

## Scope and stopping condition

Both product descriptors currently select desktop-v releases in the same
repository. Publishing production0.3.0 would offer its original-JSTorrent-root
packages to an incubation installation with a different updater key. Add an
optional exact required release asset to the update server's channel rule.
Production requires JSTorrent_x64.app.tar.gz; preview requires
RSTorrent_x64.app.tar.gz. Apply it before version ordering and notes collection.
Production Latest has no eligible release until its own package family is
published; do not offer the existing preview Latest package.

Keep identifiers, hostnames, tag prefixes, channels and client trust roots.
Package signatures remain authenticated by the client; the marker is a release
selection guard, not cryptographic verification. No engine or client change,
new rollout system, signing change or speculative Linux work.

## Invariants and evidence

Inspect sibling server's channels/products/GitHub selection and persisted
cache namespaces. Reject empty, oversized, path-like or non-string markers.
A marked rule gets an isolated cache and notes path, including Stable, so old
unqualified fallback cannot bypass the guard. Legacy unmarked products retain
existing behavior. Exercise mixed same-prefix production/preview releases,
Latest with no eligible release, notes separation and cache isolation.

Run server typecheck, tests and build; validate both repository descriptors.
Commit and push before scoped service deployment. Preserve exact prior private
config and service revision for rollback. Read actual public selectors before
and after; preview remains0.1.4/0.2.801 and production remains legacy until the
separate approved cutover. Stop when selectors are separated and live checks
prove no production/preview cross-offer.
