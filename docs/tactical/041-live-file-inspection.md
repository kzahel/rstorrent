# Live File Inspection

Status: Planned.

Topics: `application-view-api`, `web-ui-design`,
`desktop-inspection-surface`, `performance-and-live-evidence`

## Motivation

The detailed web inspection surface can show live torrent and peer state, but
its Files tab is still a placeholder. File geometry, selection, stored
progress, and verified progress are among the most useful ways to understand a
download: they reveal piece-boundary effects, hash failures, selective-storage
behavior, and whether bytes merely arrived or are trusted.

This is deliberately a Files-only slice. Pieces and Disk need separate product
and visualization decisions. File priority mutation, part-file management, and
file actions also carry enough storage and lifecycle behavior to deserve later
tacticals. This slice instead establishes one truthful read-only projection,
connects it through the existing leased view-set API, and makes the shared
virtual table reliable enough for live inspection.

## Accepted Product Decisions

- The Files view contains the complete local file catalog. It is not paginated
  or windowed at the API boundary under the current 4,096-file metainfo limit.
- The browser materializes the Files view only while it is visible. A compact
  or phone detail view does not retain the torrent list merely because a file
  view is active.
- `Done` and `Verified` are distinct byte counts. `Done` includes safely stored
  blocks in a piece that has not passed its hash yet and can therefore regress;
  `Verified` contains only hash-passed piece bytes.
- Padding is a metainfo file attribute, not a file selection or priority. The
  API retains the attribute for correct geometry, while the ordinary Files
  table hides synthetic padding rows.
- The current selection vocabulary is only `wanted` and `skipped`. The UI may
  present those conventional values as `Normal` and `Skip`; it must not imply
  that low or high priorities already exist.
- Filesystem-backed torrents expose enough unredacted local path information
  for an optional absolute Storage Path column. This column describes the
  intended managed path and does not claim that the file currently exists.
  Platform-capability storage such as Android SAF has no fabricated filesystem
  path and supplies `null` instead.
- The shared table supports correct typed sorting, explicit live-sort policy,
  user-controlled column visibility, and column resizing. Those presentation
  preferences persist per table. Multi-selection and file actions remain out
  of scope.

## Scope

- Add a runtime-independent file-geometry/progress projection derived from the
  existing metainfo layout, file selection, stored-block transitions, and
  durable verified-piece authority.
- Expose the complete file catalog through a new `torrent_files` view-set
  capability with coherent snapshots, keyed complete-row patches, explicit
  metadata-pending state, and generated TypeScript/JSON Schema/Kotlin types.
- Include stable file identity, path components, length and torrent offset,
  piece span, selection, padding, done bytes, and verified bytes.
- Expose a filesystem content base for path-backed storage. Derive individual
  absolute paths from that base and the validated metainfo path rather than
  repeating the same base in every wire row.
- Extend semantic frontend interest, pure validation/reduction, Zustand state,
  live and named-demo adapters, lease-expiry recovery, and view eviction for
  the Files detail tab.
- Implement the responsive, virtualized Files table with default and optional
  columns, truthful empty/loading/stale/unsupported states, and wide, compact,
  and phone layouts.
- Correct the shared virtual-table sorting behavior used by Files, Peers, and
  Torrents, including numeric decimal-u64 values, semantic enums, endpoints,
  null ordering, zero values, stable ties, and non-jittering live updates.
- Add persisted per-table column visibility, bounded resizing, sort state,
  live-sort preference, and reset behavior without adding a new UI framework.
- Measure the full legal catalog snapshot, reducer/store pressure, and
  virtualized rendering behavior. Adjust the bounded snapshot path so the
  existing 512 KiB steady-state queue does not truncate a valid catalog.
- Update the owning topics and this tactical with actual evidence when the
  implementation completes.

## Non-goals

- Pieces or Disk projections, tabs, or visualizations.
- Runtime file selection or priority commands; low/high priority; file
  reordering; priority propagation into the piece picker; or a file-selection
  modal.
- Multi-row selection, context menus, open/reveal/copy-path actions, streaming
  playback, per-file deletion, materialization commands, or part-file controls.
- A `Show padding` control. Padding rows remain in the semantic projection and
  are filtered from the initial UI.
- Filesystem existence, open-file, allocation, sparse-range, cache, read/write
  queue, or per-file I/O statistics. Those belong to later File actions or Disk
  work.
- Android Compose presentation. Shared Rust and generated Kotlin contracts
  must continue to build, but the Android UI remains unchanged.
- Arbitrarily large file catalogs, pagination, server-side sorting/filtering,
  a binary codec, frame-rate streaming, or field-level JSON patches.
- A path-redaction or remote-enterprise policy layer. The current local
  product surface exposes the user's own managed paths directly.
- BEP 52/v2 or hybrid torrent support. BEP 47 padding already accepted by the
  v1 parser is represented honestly without claiming broader metainfo support.

## Reference Review

### Specifications

- `reference/bittorrent.org/beps/bep_0003.rst` defines v1 single-file and
  multi-file ordering, length, and path semantics. File indices and offsets in
  this tactical preserve metainfo order exactly.
- `reference/bittorrent.org/beps/bep_0047.rst` defines the `p` file attribute.
  Padding files provide zero bytes for piece alignment and conventionally use
  `.pad/<length>` paths; aware clients need not request or write those bytes.
  RSTorrent already parses the attribute and treats padding as a layout target,
  not a selection state.

### Pinned libtorrent oracle

Pinned libtorrent `2.0.13` revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d`:

- `include/libtorrent/file_storage.hpp` owns ordered file geometry, offsets,
  sizes, paths, piece mapping, attributes, and padding identity.
- `include/libtorrent/torrent_handle.hpp::file_progress()` returns one complete
  vector in file order. Its accurate mode adds stored blocks in current
  download pieces and may regress; `piece_granularity` counts only hash-passed
  pieces and is cheaper and non-regressing during ordinary download.
- `src/torrent.cpp::file_progress()` returns zeroes before progress, file sizes
  for a seed, uses the retained hash-passed file-progress state, and adds
  non-requested block states from active pieces for accurate progress.
- `include/libtorrent/aux_/file_progress.hpp` and `src/file_progress.cpp` keep a
  lazy per-file cache updated from completed pieces, map piece bytes across file
  boundaries, and exclude pad bytes from physical total-on-disk accounting.
- `test/test_file_progress.cpp` covers zero-length files, cross-file piece
  mapping, sequential and reverse updates, and padding completion behavior.
- `test/test_checking.cpp` verifies file progress across checking and missing
  files. `test/test_torrent_info.cpp` covers padding attributes and paths.

RSTorrent adopts the semantic distinction and boundary cases, not
libtorrent's picker or alert architecture. Libtorrent exposes the full vector
rather than a paginated file-progress API; its configurable `max_piece_count`
is a load-time resource guard, not a UI pagination mechanism.

### JSTorrent product reference

Local JSTorrent `main` revision
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/ui/src/tables/FileTable.tsx` provides the useful default information
  hierarchy: filename, folder, extension, priority, size, progress, done, and
  optional index/full path.
- `packages/ui/src/tables/VirtualTable.solid.tsx`, `types.ts`, and
  `column-config.ts` demonstrate virtual rows, column visibility/order/width
  persistence, resize handles, stable row keys, and an optional live-sort mode.
- Its multi-selection, context menu, open/reveal, priority, watch, and deletion
  actions are intentionally deferred here.
- Its generic comparator still reduces mixed values to number subtraction or
  string comparison. RSTorrent does not copy that limitation; its comparator
  contract is typed and tested for the data actually displayed.

No source or fixture is copied.

## Vocabulary And Semantic Contract

- **File index** is the zero-based position in the verified metainfo file list.
  It is stable for the torrent lifetime and is the row identity, including for
  padding rows hidden by the initial UI.
- **Relative path** is the validated metainfo path beneath the torrent's
  managed content directory. Filename, folder, and extension are presentation
  derivations, not duplicated engine authorities.
- **Filesystem content base** is the absolute hash-named managed directory for
  path-backed storage. Joining it with validated path components yields the
  optional Storage Path value. It is `null` for SAF/capability storage.
- **Wanted/skipped** is current persisted selection intent. It is independent
  of padding and independent of how many boundary bytes happen to be present.
- **Done bytes** are bytes in a file's torrent range backed by a hash-verified
  piece or by a block whose storage write completed successfully in the
  current engine generation. Requested, received-but-not-stored, rejected, and
  failed-write bytes do not count.
- **Verified bytes** are bytes in the file's torrent range covered by durable
  hash-passed pieces. They are rebuilt from the verified-piece checkpoint on
  resume.
- **Progress** is a UI derivation of `done_bytes / length_bytes`. A zero-length
  non-padding file is complete without division. Verified progress remains a
  separate column/value and is never inferred from aggregate torrent percent.

Done is greater than or equal to Verified. Verifying a piece transfers its
unverified stored contribution into Verified without changing Done. A hash
failure removes that piece's unverified contribution from Done. Done may also
fall across conservative restart because unverified staging bytes are not
trusted resume state. Verified is monotonic during normal transfer but may be
cleared by an explicit checking/repair transition; no such command is added by
this tactical.

Skipped files remain visible and retain honest Done and Verified counters.
Boundary pieces can cause those counters to be nonzero even though the target
file is not selected or materialized. Padding bytes may participate in piece
geometry and progress accounting, but padding rows never claim physical disk
bytes and are hidden from the normal table.

## View Contract

Add `ViewSpec::TorrentFiles { view_id, torrent_id, delivery }` and advertise
`torrent_files` only when its full contract is available. The conceptual
generated payload is:

```text
FileView {
    file_id: decimal file-index string,
    file_index: u32,
    path: Vec<String>,
    length_bytes: decimal u64 string,
    torrent_offset_bytes: decimal u64 string,
    first_piece: Option<u32>,
    last_piece: Option<u32>,          // inclusive
    selection: wanted | skipped,
    padding: bool,
    done_bytes: decimal u64 string,
    verified_bytes: decimal u64 string,
}

ViewSnapshot::Files {
    torrent_id,
    state: metadata_pending | available | torrent_missing,
    filesystem_content_base: Option<String>,
    files: Vec<FileView>,
}

ViewPatch::Files {
    torrent_id,
    upsert: Vec<FileView>,
    removed: Vec<file_id>,
}
```

`files` is empty unless state is `available`; an available v1 torrent has at
least one metainfo row. The transition from metadata pending to available is a
fresh coherent snapshot because it installs static geometry and dynamic
progress together. Complete-row keyed upserts carry only files whose dynamic
values changed. A reset or lease recovery also starts with a complete snapshot.

All u64 byte values remain decimal strings on the wire and are parsed as
`bigint` for comparison. Formatting may convert bounded display units, but it
must not clamp identity or ordering to JavaScript's safe-integer range. `null`
is a supported value with no current value; a missing required field is a
validation failure. An older server without `torrent_files` produces the
existing explicit `unsupported` materialization rather than an empty table.

The TypeScript desired-view vocabulary extends detail interest with `files`
and adds `viewStatus.files`. Only the visible Files tab requests the collection.
Switching tabs removes and evicts it at the ordered `view_removed` boundary.
The controller retains no view-set identifier in browser storage; after lease
expiry, suspension, reload, or server restart it opens a fresh set and replaces
the stale catalog atomically.

## State Ownership And Data Flow

```text
verified metainfo + FileSelection + verified checkpoint
                         |
stored / verified / hash-failed block-piece transitions
                         v
runtime-independent FileProgress projection
                         |
ApplicationService -> ViewHub source model -> leased Files view
                         |
validated batch -> pure reducer -> selected Zustand Files state
                         |
responsive virtual Files table
```

- `Metainfo`/`TorrentLayout` remain the authority for file order, path,
  offsets, lengths, padding, and piece geometry.
- `SessionStore` remains the authority for verified metadata, storage-root
  identity, persisted selection, and durable verified-piece checkpoints.
- `SwarmState` and the storage supervisor remain the authorities for block
  lifecycle and successful storage completion. The Files projection observes
  those transitions; it does not become a scheduler or storage owner.
- A small runtime-independent file-progress state owns only derived per-file
  counters. It is rebuildable from layout, haves, and current stored-block
  state, has no socket/filesystem/task types, and is tested without Tokio.
- `ApplicationService` resolves the optional filesystem content base from its
  configured trusted root and torrent identity. Browser input never supplies
  a path.
- `ViewHub` publishes immutable catalog state and coalesced complete-row
  changes. It does not reconstruct progress from diagnostics or create a
  second persisted truth.
- The existing application-owned view-set reaper and TypeScript controller own
  lease and polling lifecycles. This slice adds no detached background task.
- Zustand retains only requested file views. React components own no transport
  work and do not persist the engine replica.

The implementation may place the derived tracker beside storage layout or the
swarm domain according to dependency direction. It must not make protocol
types depend on Tokio, channels, paths, view DTOs, or platform adapters. One
immutable catalog should be shared inside the application source model rather
than cloned once per progress event or once per subscribed view set.

## Shared Table Behavior

Every sortable column declares a semantic sort kind or comparator. The shared
ordering rules are:

- integer byte/count/time fields compare exactly, including decimal-u64
  strings through `bigint`;
- text uses locale-aware natural comparison;
- lifecycle, source, selection, and other enums use explicit semantic ranks;
- endpoints compare parsed address family/address/port when valid and use a
  deterministic text fallback otherwise;
- `null` and unavailable values sort last in both ascending and descending
  order;
- numeric zero is a known value and is never rendered or sorted as missing;
- equal values use the stable row ID ascending as a direction-independent tie;
  and
- sorting never mutates the source collection.

Live sort defaults off for changing tables. A header click or row-membership
change establishes a sorted order; subsequent value-only patches update cells
without continuously moving rows. The per-table `Live sort` preference opts
into reordering after each applicable patch. This makes peer rates and file
progress inspectable without preventing a user who wants a continuously
ranked view.

Presentation preferences use a versioned browser-local record per table:
visible column IDs, bounded widths, current sort, and live-sort mode. New
columns reconcile safely with older records, invalid/removed IDs are ignored,
and Reset restores current defaults. Column reordering is not part of this
slice.

The Files defaults are Filename, Folder, Ext, Size, Progress, Done, Verified,
Priority, and Pieces. Optional hidden columns are Index, Relative Path, and
Storage Path. Progress, sizes, and piece spans sort by raw values rather than
formatted strings. The virtual table retains a bounded visible DOM regardless
of catalog size.

Column menus, sort controls, and resize separators require keyboard and pointer
paths. Headers expose `aria-sort`; resize handles have names, value feedback,
minimum widths, and arrow-key operation. Responsive hiding remains distinct
from a user's visibility preference, and horizontal scrolling remains
available when explicitly shown columns do not fit.

## Resource Bounds

- Preserve `MAX_FILES = 4096`, `MAX_PATH_COMPONENTS = 32`, and the current
  1 MiB bencode/metadata input bounds. This tactical does not raise torrent
  admission limits.
- Return the full accepted catalog. Do not silently truncate, page, or label a
  partial list as complete.
- Start with a 16 MiB maximum coherent file-snapshot encoding. The implementer
  may raise this to at most 32 MiB only if a generated, valid worst-case
  metainfo fixture proves 16 MiB insufficient; record the measured encoded
  high-water and chosen bound.
- Keep the ordinary steady-state view-set diff backlog capped at 512 KiB. A
  large initial or reset snapshot must have a separately bounded retention and
  response path rather than silently making every view set retain 16 MiB of
  patches. Polling/gateway/client response readers enforce the same advertised
  snapshot bound.
- Coalesce repeated file-row changes to the newest complete row. The initial
  Files delivery policy is no faster than four batches per second; it does not
  emit one network frame per stored block.
- Do not clone the full catalog on each block, progress patch, reducer update,
  or React render. Record catalog source size, encoded snapshot size, retained
  Rust bytes where measurable, reducer duration, browser heap, visible DOM
  count, and long-task evidence for the 4,096-row fixture.
- Keep virtual-table overscan bounded by the existing table policy. A scale
  check must demonstrate that DOM node count does not grow linearly with the
  complete catalog.

If the current view accumulator cannot separate a large coherent snapshot
from its patch backlog, a focused refactor at that owner boundary is in scope.
Changing the wire codec or introducing pagination is not.

## Implementation Stages And Intermediate Gates

1. **Pure geometry and progress.** Implement file/piece overlap accounting and
   the rebuildable Done/Verified tracker. Gate on single/multi-file boundaries,
   zero length, padding, skips, final short piece, resume haves, storage failure,
   verification, and hash-failure regression before touching view transport.
2. **Rust view contract.** Add catalog source state, Files snapshot/patch
   coalescing, metadata transition, path base, lease/reset behavior, generated
   contracts, and the separate bounded snapshot path. Gate on deterministic
   ViewHub/view-set and encoding-size tests.
3. **Pure TypeScript model.** Add strict validation, exact decimal sorting,
   reducer state, semantic desired-view interest, eviction, and demo/live
   adapter mapping. Gate on Vitest without React or a server.
4. **Shared virtual-table correction.** Add typed comparators, null/zero/tie
   rules, optional live sort, persisted visibility and resize configuration,
   keyboard behavior, and Peers/Torrents regression coverage.
5. **Files presentation.** Add one deterministic `file-progress` demo scenario,
   the responsive table, honest materialization states, padding filtering, and
   the default/optional columns. Gate on component tests and headless wide,
   compact, phone, and 4,096-row browser evidence.
6. **Controlled live proof.** Use a headless application gateway and a
   controlled libtorrent multi-file seeder. Observe metadata pending, catalog
   arrival, stored progress, verified progress, completion, view removal, and
   lease-expiry recovery through the same UI adapter. Do not launch Tauri or a
   visible browser.
7. **Closure.** Regenerate checked-in contracts, run proportionate desktop and
   Android build gates, update living topics and this evidence record, remove
   temporary profiles/downloads/screenshots not intended as artifacts, commit,
   and leave a clean tree.

## Validation Matrix

### Pure Rust state

- single file and nested multi-file catalogs preserve metainfo order and exact
  offsets;
- piece overlap spans two or more files, including the final short piece;
- zero-length file progress is complete and never divides by zero;
- padding is unselectable, does not claim physical bytes, and does not create a
  path requirement;
- skipped-file boundary bytes can be Done/Verified without claiming the target
  is selected or materialized;
- received-but-not-stored and failed writes do not count as Done;
- stored unverified blocks count as Done, verification increases Verified
  without double-counting Done, and hash failure removes only that piece's
  unverified bytes;
- restart rebuilds Verified exactly from checkpoints and conservatively drops
  untrusted active Done bytes; and
- repair/recheck reset behavior cannot leave Verified above Done or length.

### View-set and transport

- metadata pending, available, torrent missing, unsupported, stale, reset, and
  recovered states remain distinct;
- the available snapshot contains every accepted file, stable IDs, exact
  decimal byte values, geometry, and selection;
- repeated row upserts coalesce, later removal wins, and snapshot recovery
  restores the full catalog after overflow or lease expiry;
- tab interest adds and removes only the Files view and evicts it after the
  ordered removal;
- filesystem base is absolute for a configured path root and `null` for a
  platform-capability root;
- a legal 4,096-file worst-case snapshot passes the selected encoding bound,
  while an over-bound response fails explicitly rather than truncating; and
- gateway and TypeScript readers accept the advertised snapshot maximum but
  retain ordinary request and patch limits.

### Frontend behavior

- exact numeric ordering covers values above `Number.MAX_SAFE_INTEGER`;
- null is last in both directions, zero remains visible, semantic enum and
  endpoint ordering are deterministic, and stable ties do not jitter;
- live-sort off preserves row order across value-only peer/file patches, while
  live-sort on reorders it;
- column visibility, bounded pointer and keyboard resizing, sorting, live sort,
  schema reconciliation, and reset survive reload in table-scoped settings;
- Files default and optional columns render raw semantics correctly, padding
  rows stay hidden, and Storage Path remains blank when unavailable;
- named demo progress can regress after a hash failure and then recover;
- keyboard navigation, focus, column menus, resize handles, and table labels
  pass serious/critical axe checks; and
- wide, compact, and phone screenshots plus the scale scenario show no clipping
  or bottom dead area and retain bounded rendered rows.

### Controlled interoperability and builds

- a controlled libtorrent seed serves an independently constructed nested
  multi-file torrent with at least one piece crossing file boundaries;
- the live headless web adapter observes metadata, time to first Done byte,
  time to first Verified byte, per-file completion, and full verified output;
- final materialized file lengths and hashes match the fixture;
- view lease expiry and reopen occur while the engine continues without losing
  or duplicating file progress;
- `cargo fmt --all -- --check`, warning-denying workspace Clippy, and workspace
  tests pass;
- generated web contracts are current; Vitest, typecheck, production build,
  and Playwright pass; and
- generated Kotlin contracts and the existing two-ABI Android build compile,
  without adding an Android Files screen or requiring a device.

Public-swarm traffic is not required for this presentation/projection slice.
Any optional spot check follows the live-evidence policy and is reported
separately from deterministic completion.

## Escalation Contract

In-scope implementation authority includes ordinary module extraction,
replacing duplicated progress derivation with the single rebuildable cache,
adjusting the file snapshot limit within the declared 16--32 MiB range,
changing generated contracts, adding deterministic fixtures and demo data,
fixing shared sort/render bugs at the table boundary, and updating owning
topics with evidence.

Stop for human direction if evidence requires pagination or a transport/codec
redesign; runtime file mutation or destructive file actions; a new persistence
schema; a filesystem/SAF behavior change beyond read-only projection; a new UI
or runtime dependency with material tradeoffs; visible application or physical
device interaction; or broader Pieces/Disk/Android presentation work.

## Stopping Condition

This slice is complete when the demo and live headless web application can
materialize the full selected torrent file catalog only while Files is visible;
show exact, independently meaningful Done and Verified progress through a
controlled multi-file completion and hash-failure/recovery case; recover after
view-set expiry; expose configurable virtualized columns with correct stable
sorting; meet the declared snapshot and browser scale bounds; keep shared Rust,
web, desktop, and Android build gates green; record actual evidence in this
tactical and the owning topics; commit the completed work; and leave a clean
tree without implementing Pieces, Disk, or file actions.
