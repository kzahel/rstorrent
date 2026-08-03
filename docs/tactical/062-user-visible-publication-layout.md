# Tactical 062: User-Visible Publication Layout

Status: Complete on 2026-08-03.

Topic: `download-roots`

## Motivation And Stopping Condition

Tactical 061 made a user-selected download root authoritative but deliberately
retained the bring-up layout `<root>/<info-hash>`. That leaves ordinary
multi-file downloads in an opaque directory even though BEP 3, pinned
libtorrent, JSTorrent, and the accepted product topic all treat `info.name` as
the recognizable root directory.

This tactical stops when a newly accepted multi-file magnet:

- durably binds its verified safe metainfo name as its publication component;
- stages and resumes through full-info-hash-owned hidden artifacts;
- publishes as `<selected root>/<torrent name>/...` on path storage;
- supplies the same name to platform-capability publication;
- projects the named path to the Files view;
- removes exactly the named path publication and hash-owned internal
  artifacts;
- fails closed on an existing final destination without merging, overwriting,
  or suffixing it; and
- has deterministic store, storage, application restart, collision, and
  cleanup evidence plus the normal Rust workspace baseline.

## Scope

- Multi-file torrents only. Resumable product execution already requires
  multi-file metainfo; single-file publication remains a later joined slice.
- A schema revision that stores the verified publication component in the
  same transaction as exact raw info, piece geometry, and empty have state.
- One engine-owned path plan containing the recognizable final directory and
  the hidden staging and part paths derived from the full v1 info hash.
- Path-backed create, resume, recheck, publication, view projection, removal,
  and restart behavior.
- Platform descriptor planning/removal consistency with the same persisted
  name; platform adapters retain their existing descriptor and capability
  ownership.
- Explicit destination-conflict behavior and adversarial name/path checks.

## Non-Goals

- Moving, importing, or silently adopting existing hash-named development
  payloads. RSTorrent is unreleased and development state may be recreated.
- A user-visible or hidden preference to publish by info hash. No current
  product or reference behavior justifies that policy surface.
- Automatic suffixes, overwrite, directory merge, or same-length trust when a
  recognizable destination already exists.
- The later **use existing data and recheck** action.
- Single-file durable resume/publication, staged magnet intake, `.torrent`
  intake, file selection UI, relocation, dynamic priorities, or multi-torrent
  scheduling.
- New Linux, Windows, Android, or ChromeOS picker work; live public-swarm,
  device, or performance evidence.

## Inputs And Reference Findings

The continuing owners are:

- [`../topics/download-roots.md`](../topics/download-roots.md) for selected
  root identity, recognizable publication, and collision policy;
- [`../topics/client-persistence.md`](../topics/client-persistence.md) for
  exact verified metadata, conservative restart, and durable artifact
  identity;
- [`../topics/application-control.md`](../topics/application-control.md) for
  semantic commands and platform-capability boundaries; and
- [`../topics/storage-throughput-architecture.md`](../topics/storage-throughput-architecture.md)
  for storage ownership, publication fences, and joined cleanup.

Normative BEP 3 at `reference/bittorrent.org/beps/bep_0003.rst` says `name` is
the suggested file or directory name and, for a multi-file torrent, is the
directory containing the `files[*].path` tree.

The pinned libtorrent checkout is
`7d7fc38fac61177fa5e02148f791b2f65250b09d` (`v2.0.13`):

- `include/libtorrent/add_torrent_params.hpp` makes `save_path` the containing
  location, not a per-torrent hash-directory policy;
- `include/libtorrent/file_storage.hpp::set_name` documents the multi-file
  torrent name as the root directory;
- `src/file_storage.cpp::file_path` appends `m_name` before the inner path;
- `src/torrent_info.cpp` sanitizes the metainfo name and uses the info hash
  only when sanitization produces an empty name;
- `src/mmap_storage.cpp` separately names the internal part file from the full
  info hash; and
- `test/test_file_storage.cpp::setup_test_storage` proves the named root in
  resolved paths.

The sibling JSTorrent checkout at
`9895410beeed6aff554053769bd006a3fbd373ef` follows the same product behavior:

- `packages/engine/src/core/torrent-parser.ts` prefixes every multi-file path
  with the sanitized torrent root name;
- `packages/engine/src/core/torrent-path-sanitizer.ts` falls back to the info
  hash only when the root sanitizes to empty;
- `packages/engine/test/core/torrent-parser.test.ts` covers Unicode names,
  traversal sanitization, and the empty-name hash fallback; and
- `packages/engine/src/core/parts-file.ts` keeps part-state identity
  hash-based.

Neither reference exposes a normal setting that replaces the recognizable
multi-file root with the info hash. Libtorrent permits caller-driven remapping
or custom storage, but that is not its default publication policy.

## Current Boundary And Concrete Refactor

`ApplicationService::start_if_possible` currently passes
`root.join(torrent_id)` before magnet metadata is available. `SelectiveStorage`
treats that value as the complete publication root and derives staging and
part siblings from it. The Files view and removal independently reconstruct
the same hash path. These duplicated derivations make the bring-up policy part
of several owners.

Introduce one engine-owned immutable path plan:

```text
TorrentStoragePaths {
    output:  <root>/<verified metainfo name>,
    staging: <root>/.<full info hash>.rstorrent-staging,
    part:    <root>/.<full info hash>.rstorrent-parts,
}
```

The resumable magnet boundary receives the selected root rather than a
pre-metadata final path. Once metadata is verified, the engine constructs the
plan from that metainfo. The session persists the same verified name and uses
the shared planner for restart, Files projection, and managed deletion. Direct
storage tests may retain explicit arbitrary paths where they are testing the
storage primitive rather than product publication policy.

## Durable State And Compatibility

Schema version 6 adds nullable `publication_name` and a bounded
`managed_artifacts` ownership state to `torrents`. `record_metadata` writes
`raw_info`, `publication_name`, piece count, and empty have state atomically.
Restart re-parses and re-hashes the raw info and requires the stored component
to equal the verified `Metainfo::name` before resolving any path.

For path storage, `managed_artifacts` is distinct from the visible
storage/repair state. It advances from `none` to `staging` only after the
engine successfully creates or reopens its exact hash-owned artifacts, and to
`published` after an exact final tree is reopened or publication completes. A
collision before either boundary therefore cannot make an unrelated
destination eligible for later managed deletion. Repair/error transitions do
not erase established ownership. Platform capability cleanup retains its
existing adapter-owned document plan; this tactical only makes that plan
require the same durable verified name.

Version-5 rows are structurally upgraded without guessing that their physical
hash-named artifacts have already moved. A legacy row with metadata but no
publication name is retained with `legacy` artifact ownership for explicit
removal and otherwise enters `needs_repair`; managed deletion continues to
derive its old hash-named artifacts. This is bounded pre-release compatibility,
not automatic relocation. Re-adding the torrent after removal establishes the
new layout.

## Owners, Tasks, Cancellation, And Dependency Direction

- `rstorrent-protocol` continues to parse hostile metainfo and admits `name`
  only as one bounded safe component. It knows nothing about roots or I/O.
- `rstorrent-engine` owns the pure final/staging/part path plan and uses it for
  path-backed storage creation and resume. Existing storage workers,
  cancellation, checkpoint, and publication joins remain unchanged.
- `rstorrent-session` owns the schema, atomic metadata/name checkpoint,
  selected-root lookup, restart consistency check, Files projection, and
  removal generation.
- Android and other platform adapters continue to own capabilities and
  provider mutation. Descriptor plans already carry the verified torrent
  name; the session verifies that it matches the durable publication name.
- Presentations receive the corrected Files path through existing views. No
  new command, path-bearing wire field, task, or background owner is added.

Dependencies continue inward: session may call the engine's pure path planner;
the engine does not depend on SQLite, application commands, platform
capabilities, or presentation state.

## Invariants And Failure Policy

- Only hash-verified and bounded raw info may choose the publication name.
- The final name is one relative component and remains beneath the selected
  root. Inner paths retain the existing hostile-metainfo validation.
- The durable publication name and reparsed metadata name must agree exactly.
- Staging and part artifacts use the complete info hash and cannot alias the
  final path for the same torrent.
- Create and publish both reject an existing final destination. No bytes are
  merged, overwritten, or accepted as verified because of name or length.
- Resume opens only an exact final-or-staging plus part-file generation and
  conservatively rechecks every durable have claim.
- Deletion derives only the retained torrent's final, staging, and part paths;
  symlinks are unlinked rather than followed, absent artifacts are success,
  and the selected root and unrelated siblings remain.
- Path-managed deletion removes the recognizable final directory only after
  durable `published` ownership. A destination that caused a path
  create/publication conflict remains untouched when the torrent record is
  removed.
- Same-name torrents sharing a root are allowed in the catalog but the later
  one cannot create or publish content until the destination conflict is
  explicitly resolved.
- Metadata/checkpoint failure, storage conflict, pause, removal, and shutdown
  retain their existing joined task termination paths.

## Validation Plan

### Pure/store

- fresh schema and schema-5 structural upgrade;
- metadata atomically records the exact publication name;
- storage ownership survives repair/error projection and prevents cleanup of
  a colliding unowned destination;
- a mismatched or absent durable name cannot resume as trusted storage;
- legacy rows remain removable through their hash-named artifact plan; and
- Unicode and maximum-bounded valid names round-trip exactly.

### Engine/storage

- the pure planner produces named final and full-hash hidden paths;
- final and internal paths are distinct and below the supplied root;
- fresh staging, publication, and published/staging resume use the explicit
  plan; and
- pre-existing named output, hash staging, or hash part artifacts fail closed.

### Application

- a path-backed multi-file torrent resolves its engine output and Files base
  to the persisted name;
- restart reopens named staging and published content;
- delete-managed removes named output plus hash-owned internal artifacts and
  preserves siblings;
- same-name/pre-existing destination conflicts become visible repair state;
  and
- platform storage/removal plans agree with the durable name.

### Baseline

Run:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

No generated client contract should change because publication identity stays
behind the existing semantic view and platform-operation boundaries. If the
implementation disproves that assumption, regenerate and validate every
affected client surface before completion.

## Completion Record

Completed on 2026-08-03.

- `rstorrent-engine` now owns and exports the exact named-final/hash-internal
  path plan. Resumable path storage creates and reopens that plan, retains the
  old explicit-output primitive for diagnostic callers, and rejects unsafe or
  internal-artifact-shaped publication components.
- `rstorrent-session` schema version `6` atomically stores the verified
  publication name and a bounded managed-artifact ownership state. Restart,
  Files projection, platform planning, and removal all require or derive that
  durable identity. Version-5 rows remain legacy-owned and removable but are
  not silently relocated or resumed under a guessed name.
- A pre-existing named destination fails as `needs_repair`. Because ownership
  remains `none`, a later delete-managed command preserves that destination;
  staging-owned cleanup removes only the torrent's full-hash internal paths.
- Deterministic coverage proves Unicode and boundary-length planning, reserved
  component rejection, fresh/staging/published storage, schema migration,
  missing-name repair, Files projection, named and legacy cleanup, and
  collision preservation.
- No generated client contract changed: publication identity remains behind
  the existing semantic Files and platform-operation boundaries.

Validation passed:

```text
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

The workspace test run passed 374 tests; the three explicitly opt-in live
network tests remained ignored.
