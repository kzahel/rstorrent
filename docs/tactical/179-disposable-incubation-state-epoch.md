# Tactical 179: Disposable Incubation State Epoch

Status: **Complete (2026-08-27).** Schema 21 is the fresh current catalog;
recognized schemas `1..=20` reset before startup. Compatibility-only DHT-v1,
desktop-settings-v1/v2, and browser-appearance-v1/v2 readers are removed.
Every declared gate passes, and Tactical `176` resumes as the sole **Now** for
its existing macOS-only iOS compile; desktop release Tactical `158` remains
next.

Topics: `client-persistence`, `dht-discovery`, `client-surfaces`,
`web-ui-design`, `beta-release-readiness`, `capability-readiness`,
`oracle-driven-engine-campaign`

Dependencies: completed clean-reset Tactical
[`143`](143-dual-identity-and-persistence-foundation.md) supplies the bounded,
crash-convergent application-private reset owner; completed Tacticals
[`162`](162-desktop-single-instance-and-tray-lifecycle.md),
[`164`](164-desktop-completion-and-attention-notifications.md), and
[`165`](165-cross-platform-active-download-sleep-inhibition.md) supply the
current desktop-shell settings record; completed web appearance Tacticals
`047`, `050`, and `099` supply the current browser-local record.

## Decision And Desired Outcome

Apply the disposable-incubation policy immediately instead of retaining code
whose only purpose is to carry unsupported preview state forward. Establish
schema 21 as a fresh catalog epoch. Every recognized earlier catalog version
`1..=20` resets before application work begins; none migrates into schema 21.

Remove the other bounded old-format readers that are still present solely for
incubation continuity:

- DHT snapshot version 1 and its single legacy IPv4 node-ID projection;
- desktop-shell settings versions 1 and 2; and
- browser appearance versions 1 and 2.

The current formats remain versioned, bounded, and fail closed. Unsupported
desktop-shell records are atomically repaired to current defaults. Unsupported
or malformed browser-local appearance records use current defaults. An
unsupported DHT snapshot is rejected for cold bootstrap. These are resets, not
best-effort field migrations.

## Stable Scenarios

1. **DSE-001 fresh/current catalog.** A missing catalog creates schema 21 and
   a schema-21 catalog reopens unchanged.
2. **DSE-002 recognized reset.** Representative schemas 19 and 20, plus an
   older recognized schema, converge to one empty schema-21 catalog with one
   truthful reset report and no retained torrent, root, setting, source,
   receipt, DHT, selection, priority, or verification state.
3. **DSE-003 payload safety.** Reset removes only `session.db` and its exact
   SQLite sidecars. User-selected roots, published content, partial artifacts,
   platform grants, metrics, and installation identity are not modified or
   adopted as verified state.
4. **DSE-004 interrupted and hostile reset.** Existing marker recovery still
   converges after interruption. Busy, symlinked, non-regular, malformed,
   unversioned nonempty, future, and marker-mismatch inputs fail closed without
   touching external payload.
5. **DSE-005 current DHT only.** Current version-2 address-keyed identities and
   bounded warm nodes round trip. Version 1 and unknown versions are rejected,
   and warm nodes in a current snapshot still precede cold routers.
6. **DSE-006 current desktop settings only.** Version 3 round trips exactly.
   Versions 1 and 2, malformed, oversized, unknown, and structurally invalid
   records repair to version-3 defaults with a bounded diagnostic.
7. **DSE-007 current appearance only.** Version 3 validates and round trips.
   Versions 1 and 2, malformed, future, and denied storage return current
   defaults without throwing.
8. **DSE-008 first-party composition.** Linux Rust/web gates and both Android
   native builds pass. No generated application boundary changes.

## Scope And Stopping Condition

This tactical owns the schema-21 reset boundary, removal of the schema-19 to
20 migration, removal of DHT snapshot-v1 restoration, removal of desktop
settings-v1/v2 migration, removal of web appearance-v1/v2 migration, focused
tests, proportional first-party builds, and reconciliation of the owning
topics and release ledger.

It stops when DSE-001 through DSE-008 pass, searches find no removed reader or
schema-migration symbols, the full repository validation baseline passes, and
the reset diagnostic and documentation state exactly what is discarded and
what is preserved.

## Invariants And Resource Bounds

- Catalog preparation completes synchronously before any application task,
  socket, storage handle, or platform descriptor starts.
- Only the three fixed application-private basenames `session.db`,
  `session.db-wal`, and `session.db-shm` are reset targets.
- Reset never enumerates, deletes, moves, hashes, verifies, or adopts anything
  under a user-selected path or platform storage root.
- The reset report remains one bounded row and says
  `external_payload_modified = false`; schema 21 accepts recognized prior
  versions through 20 in that row.
- Current catalog, DHT, shell, and appearance inputs retain their existing
  bounds and exact validation. Unknown future versions do not downgrade.
- DHT warm-node samples remain capped at 64 per family and address-keyed
  identities at eight per family. Removing the legacy node ID adds no state.
- Desktop settings retain the 4 KiB input ceiling and atomic replacement.
  Browser presentation persistence remains best effort and task free.
- No application command, generated TypeScript/JSON Schema/UniFFI contract,
  torrent protocol behavior, engine resource ceiling, or payload layout
  changes.

## Owner, Task, Cancellation, And Dependency Map

```text
profile startup
  -> profile_reset inspects, exclusively locks, and resets fixed private files
  -> SessionStore creates or validates schema 21
  -> application records and acknowledges one bounded reset diagnostic

session DHT store
  -> current DhtSnapshot version 2
  -> DHT actor validates before startup
  -> current warm candidates or cold-bootstrap fallback

Tauri application lifetime
  -> current desktop-shell version 3 or atomic default repair

React appearance owner
  -> current browser-local version 3 or in-memory defaults
```

The catalog and preference changes are synchronous and introduce no task or
cancellation owner. The existing DHT actor remains the sole owner of runtime
routing state and retains its joined cancellation path. Dependency direction
remains shell/web -> application/session -> engine -> protocol values; SQLite,
Tauri, browser storage, and async runtime types do not move inward.

## Reference And Compatibility Dossier

No BitTorrent wire behavior changes, so there is no normative BEP or
libtorrent state transition to adopt. The relevant repository evidence is the
clean-reset contract and hostile/crash cases in Tactical `143`, the current
version-2 DHT snapshot and fast-restart contract in `dht-discovery`, the
desktop atomic-repair contract in Tactical `162`, and the best-effort
appearance owner in `web-ui-design`.

The removed readers are local RSTorrent incubation formats, not interoperable
protocols. Libtorrent resume compatibility, JSTorrent import, peer-wire legacy
support, v1 torrent support, hybrid routing, old-artifact non-adoption, and
receipt replay within the current schema are therefore unrelated and remain
unchanged.

## Edge And Failure Cases

- Schema 19 and schema 20 both reset; neither reaches the store validator as a
  compatible current catalog.
- A reset marker may record any recognized version through 20. A marker beside
  a matching partially removed catalog converges; a disagreement fails.
- A completed schema-21 catalog plus a stale matching marker validates its
  committed report before the marker is removed.
- Unknown future catalog versions still return `UnsupportedSchema` and are not
  deleted. A nonempty version-0 or malformed database remains unsafe.
- A rejected DHT snapshot causes the existing cold-bootstrap warning/fallback;
  invalid nodes and identities in the current version retain their existing
  bounded validation.
- Desktop versions 1 and 2 do not preserve any old choice. Repair writes exact
  current defaults or reports that defaults are active if persistence fails.
- Browser versions 1 and 2 do not preserve size or theme. Current version-3
  fields still validate independently so one malformed field does not discard
  other valid current fields.

## Implementation And Validation Sequence

1. Record this contract, make Tactical `179` the sole **Now**, and pause
   Tacticals `176` and `158` without changing their remaining gates.
2. Advance the catalog to schema 21, remove the compatible-schema constant and
   migration branch, extend reset/report recognition through schema 20, and
   replace retention tests with exact schema-19/schema-20 reset evidence.
3. Remove DHT snapshot-v1 state and restoration, preserving current identity,
   warm-node ordering, validation, and cold fallback tests.
4. Remove desktop v1/v2 and browser v1/v2 readers; replace migration tests with
   default-reset tests while preserving exact current round trips.
5. Run focused session, engine, desktop, and web tests; then formatting,
   warning-denying workspace Clippy, workspace tests, web typecheck/tests, and
   both maintained Android ABI builds.
6. Reconcile implementation evidence and owning topics. Return Tactical `176`
   to **Now** for its existing macOS-only gate, then Tactical `158`.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Session/reset | fresh/reopen schema 21; schemas 18, 19, and 20 reset; one-shot report; marker recovery; future/unversioned/malformed/busy/symlink failures; payload sentinel unchanged |
| DHT | current snapshot validation and store round trip; version 1/unknown rejection; current warm-before-cold bootstrap; address/family-exact identity selection |
| Desktop | current version-3 round trip; versions 1/2 default repair; malformed/oversized/unknown/denied-write behavior; atomic setting mutations |
| Web | current version-3 combinations and independent validation; versions 1/2 default; malformed/future/denied storage; DOM application |
| Platform | desktop crate tests, web typecheck/tests, Android x86_64 and arm64 build/APK/unit-test gate |
| Repository | `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, stale-reader search, and clean diff check |

Controlled peer interoperability, public swarms, installed updater runs, and
physical-device mutation are not required because this slice changes no wire,
payload, generated-client, or OS integration behavior.

## Implementation And Evidence

Schema 21 now creates the existing opaque-owner, dual-identity, source,
settings, selection, sparse High-priority, verification, removal, and current
DHT tables only from fresh state. The schema-19-to-20 branch and its compatible
version constant are gone. Catalog preparation recognizes every earlier
version through 20 only as a reset source, retains the exclusive pre-task lock,
fixed-basename removal, marker checksum, and one-shot report, and permits the
fresh report to name versions 1 through 20. The startup diagnostic now names
the disposable-incubation epoch.

The reset suite proves representative schemas 18, 19, and 20 become empty
schema-21 catalogs, with a file inside the configured payload root unchanged.
It also proves marker recovery after database removal, beside the same reset
source, beside a committed current catalog, and after an older epoch committed
but had not removed its marker. Malformed, nonempty unversioned, symlinked,
busy, and future catalogs retain fail-closed behavior.

DHT snapshot version 2 is now the only accepted shape. The unused singleton
legacy node-ID column and projection are removed; address-keyed IPv4/IPv6
identities and bounded warm nodes still round trip, version 1 rejects, and the
current warm-before-cold bootstrap test passes. Desktop shell settings accept
only exact version 3 and atomically repair versions 1 and 2 to current defaults.
Browser appearance accepts only version 3 and resets versions 1 and 2 to
Standard, Auto, and Decimal. No generated application contract changed.

Validation run on 2026-08-27:

- `cargo fmt --all -- --check` passes;
- `cargo clippy --workspace -- -D warnings` passes;
- `cargo test --workspace` passes, including 586 engine tests with 11 declared
  ignores, 257 session tests with two declared ignores, and all 40 desktop
  tests;
- `NODE_OPTIONS=--no-webstorage npm run test --prefix clients/web` passes 293
  tests with two declared skips;
- `npm run typecheck --prefix clients/web` passes;
- `clients/android/build.sh` passes locked x86_64 and arm64 Rust builds,
  Kotlin UniFFI generation, JVM unit tests, and debug APK assembly; and
- stale-reader searches and `git diff --check` pass. The full workspace also
  builds/tests the unchanged iOS Rust crate on Linux; no Swift or Xcode source
  changed in this tactical.

## Non-Goals And Next Boundary

- Do not delete or clean old staging, part, materialization, published, SAF,
  selected-root, or user payload objects.
- Do not remove legacy BitTorrent v1/hybrid peer behavior, compatibility-safe
  artifact recognition, current receipt replay, current version validation,
  or conservative recovery checks merely because their names contain
  `legacy`, `old`, or `version`.
- Do not prune persisted enum vocabulary such as canonicalized source
  provenance or legacy managed-artifact presentation in this slice; that is a
  separate model/API audit and may require generated-client changes.
- Do not choose the first supported version, promise forward compatibility,
  publish a release, alter updater routes/keys, or change product identifiers.
- Do not resume or expand Tactical `176`, `158`, or `153` inside this slice.

After completion, Tactical `176` returns to the sole **Now** for its unchanged
macOS-only iOS compile. Tactical `158` then closes signed installed-update
evidence under the disposable-state contract. The future supported-release
boundary remains a separate explicit declaration.

## Escalation Contract

Stop for direction before deleting external payload, preserving a selected old
format rather than applying the approved reset, changing a generated/public
application contract, changing peer-wire behavior, adding a dependency with a
meaningful tradeoff, mutating an external machine, or publishing anything.

Internal renaming, focused refactoring, additional hostile reset cases,
updating exact current-format fixtures, and fixing failures at these same
owners are implementation choices within this tactical.
