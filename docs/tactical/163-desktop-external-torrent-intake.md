# Tactical 163: Desktop External Torrent Intake

Status: **Planned — Next (2026-08-24).** Desktop release/updater Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) remains the sole **Now**.

Topics: `beta-release-readiness`, `client-surfaces`,
`product-surfaces-and-migration`

Dependencies: completed v1 torrent-byte intake Tactical
[`081`](081-v1-torrent-byte-intake.md), shared chooser Tactical
[`083`](083-shared-torrent-file-picker.md), packaged root-picker Tactical
[`161`](161-packaged-desktop-folder-picker.md), desktop lifecycle Tactical
[`162`](162-desktop-single-instance-and-tray-lifecycle.md), and the maintained
Tauri/React product.

## Decision And Desired Outcome

Make the installed RSTorrent desktop application a real operating-system
handler for `magnet:` links and local `.torrent` files on macOS, Windows, and
Linux. An activation while RSTorrent is stopped starts exactly one application
owner. An activation while it is visible or hidden behind the tray reaches the
same owner, restores the main window, and enters the existing download-root
and Add-options workflow exactly once.

This is RSTorrent incubation behavior under the current
`com.jstorrent.rstorrent` identity. It does not adopt JSTorrent's application
identity, updater key, protocol aliases, profile, extension routing, or legacy
state. It also does not introduce a helper process, native host, local RPC
server, or separate IO daemon. The Tauri shell remains the OS-integration
owner and the first-party Rust application service remains in-process.

## Scope And Stopping Condition

This tactical owns:

1. packaged `.torrent` file-association metadata for
   `application/x-bittorrent` and packaged `magnet:` scheme registration;
2. one platform-normalized activation parser for cold launch, macOS open
   events, and Windows/Linux second-instance arguments;
3. one bounded shell-owned pending-intake queue that survives frontend event
   races and presents activations sequentially;
4. restoration of the existing main window for every accepted activation;
5. delivery of magnets into the existing semantic `add_magnet` operation and
   local files into the existing bounded `add_torrent_bytes` operation after
   the ordinary root/start-options decision;
6. visible, privacy-preserving feedback for invalid, inaccessible, empty,
   oversized, overflowed, cancelled, duplicate, and successfully added input;
7. deterministic Rust and React coverage, unchanged browser behavior, hosted
   desktop package gates, and release-configuration validation; and
8. installed macOS arm64, Windows x86_64, and Linux arm64 proof for cold,
   visible, and tray-hidden activation. Linux x86_64 remains a native package
   gate, and installed Intel macOS remains deliberately omitted.

The tactical stops when an installed package on each required test platform
can receive one controlled magnet and one local `.torrent` through the OS,
show the existing Add flow, add each accepted source exactly once to the same
application service, retain one process throughout warm activation, and
report bounded failures without logging or exposing the supplied URI or path.

## Non-Goals

- Remote HTTP/HTTPS `.torrent` URL fetching.
- Drag and drop, clipboard monitoring, watched folders, RSS, search, or batch
  import UI.
- Becoming the `jstorrent:` handler or routing to a browser extension.
- Silently adopting JSTorrent identifiers, updater trust, associations,
  profiles, or migration behavior.
- Automatic replacement of a user's chosen handler outside the normal
  installer/operating-system association mechanisms.
- Changing magnet, metainfo, duplicate-torrent, storage-root, or download-start
  semantics in the application service.
- Android intent filters or iOS document/URL intake; those first-party clients
  retain their own platform owners.
- Start-at-login, crash restart, window-position persistence, or tray transfer
  statistics.

## Reference Inspection

### Current JSTorrent desktop

The maintained JSTorrent checkout at `~/code/jstorrent` is the product and
platform reference. The following exact paths were inspected on 2026-08-24:

- `desktop/tauri-app/src-tauri/tauri.conf.json` declares a `.torrent`
  `fileAssociations` entry with `application/x-bittorrent` and desktop deep-link
  schemes.
- `desktop/tauri-app/src-tauri/Cargo.toml` uses
  `tauri-plugin-deep-link`; its lock resolves version `2.4.9` alongside
  `tauri-plugin-single-instance` `2.4.3`.
- `desktop/tauri-app/src-tauri/src/lib.rs`, especially `DeepLinkState`,
  `deep_link_event`, `torrent_file_event`, `handle_deep_link_routed`, the
  first-registered single-instance callback, startup `get_current` handling,
  runtime `on_open_url` handling, and Windows/Linux `register_all`, proves the
  useful cross-platform shape: collect input before the frontend is ready,
  forward later launches to the established process, and restore its window.
- `desktop/README.md` records that current magnet and torrent-file handling
  lives in the Tauri application rather than a separate link-handler binary.
- `archive/legacy-app/manifest.json`, `archive/legacy-app/background.js`, and
  `archive/legacy-app/js/client.js` preserve the older Chrome App lessons:
  launch data may arrive before the visible client, multiple launch items must
  be sequenced, and MIME type alone is insufficient because some platforms
  report `.torrent` files as generic octet streams.

RSTorrent adopts the bundle registration, early single-instance ownership,
cold/warm event collection, pending-delivery, and window-restoration lessons.
It deliberately does not adopt JSTorrent's extension-versus-desktop routing,
`jstorrent:` fallback URL, base64 file event, sidecar topology, or full-input
diagnostic logging. JSTorrent's direct `std::fs::read` has no source-size
guard; RSTorrent instead retains the already accepted 64-MiB explicit-source
limit and reads at most one byte beyond it before rejecting input.

### Tauri platform contract

The implementation must confirm the exact dependency versions against the
official [Tauri v2 deep-link documentation](https://v2.tauri.app/plugin/deep-linking/)
and [bundle configuration reference](https://v2.tauri.app/reference/config/)
at implementation time. The current documented desktop shape is:

- declare static schemes under `plugins.deep-link.desktop.schemes`;
- keep the single-instance plugin first and enable its `deep-link` feature;
- use `get_current` for cold configured-scheme activation and `on_open_url` for
  runtime URL activation;
- treat Windows/Linux URL activation as untrusted second-process arguments;
- use bundle `fileAssociations` for installed file handlers; and
- use bounded runtime registration on Linux AppImage where installed desktop
  integration is otherwise absent, while macOS registration remains bundle
  metadata only.

The installed package, not a development executable alone, is the evidence
target. Tests must inspect generated macOS, Windows, and Linux association
metadata before exercising real OS dispatch.

## Product And Security Contracts

- Only an exact ASCII-case-insensitive `magnet:` scheme and a local file whose
  final extension is ASCII-case-insensitive `.torrent` are recognized.
  `http:`, `https:`, network-authority `file:` URLs, unknown schemes, switches,
  and ordinary unrelated launch arguments are not intake.
- Magnet input is at most the existing `MAX_MAGNET_LENGTH` of 16 KiB. The
  application service remains the authority for parsing BEP parameters,
  v1/v2/hybrid identity, and duplicate behavior.
- A raw path or local `file:` URL is converted with platform-aware URL/path
  APIs, not prefix stripping. Paths remain native shell values and never cross
  the generated application contract or enter logs, diagnostics, persistence,
  DOM attributes, or error strings.
- One activation item is at most 64 KiB of encoded path/URL representation.
  One event admits at most eight items, and the pending queue retains at most
  eight items. Overflow is counted with a saturating bounded counter and shown
  generically; input contents are never echoed.
- The shell stores only the bounded magnet or local path until the item is
  accepted or cancelled. It does not read a `.torrent` while merely queued.
  File reads require a regular readable source, reject zero bytes, and read at
  most `MAX_TORRENT_SOURCE_BYTES + 1` before the existing raw-byte intake.
- A file may change between activation and user confirmation. Its bytes are
  untrusted at read time and the existing metainfo parser and source digest
  remain authoritative; shell metadata is never treated as verified content.
- There is no ambient-path application command. The UI receives an opaque
  activation ID and a bounded display kind. A Tauri-only command consumes the
  ID, performs the bounded read, and calls the same factored Rust
  `add_torrent_bytes` helper used by the chooser path.
- An activation is consumed only after a terminal success, duplicate result,
  explicit cancellation, or non-retryable source failure. A transient read or
  root-choice failure remains retryable without enqueuing a second copy.
- The frontend processes one external activation at a time. Later activations
  remain ordered and cannot replace an open Add-options dialog.
- Events carry only a generation/signal. A pull command returns pending
  descriptors after the listener is installed, preventing cold-start and
  webview-recreation races in the same manner as the tray update generation.
- Accepted input restores and focuses the existing window even while Run in
  Background is enabled. It never creates a second engine, profile service,
  listener set, media server, tray, or updater owner.
- No code path logs a complete magnet, file URL, path, filename, metainfo
  bytes, or query string. Diagnostics may record only input kind, bounded
  reason category, queue counts, source byte count after read, and a generated
  activation ID.
- RSTorrent registers only under its current bundle identity. The tactical
  must not delete, rewrite, or claim JSTorrent's registry keys, desktop files,
  LaunchServices identity, updater route, or state.

## Ownership And Dependency Direction

```text
OS install metadata / URL or file activation
  -> Tauri deep-link, RunEvent, or single-instance adapter
  -> desktop activation parser and bounded pending queue
  -> generation-only frontend notification
  -> existing React Add/root/options presentation
  -> Tauri semantic magnet dispatch OR opaque file-token command
  -> existing ApplicationService add_magnet/add_torrent_bytes owner
  -> normal catalog/view update
```

The activation parser and queue are pure desktop-shell modules independent of
Tauri where practical. The Tauri adapter depends inward on them. They may use
plain `PathBuf`, `Url`, IDs, enums, and bounded counters; they do not depend on
React, application views, async runtime tasks, sockets, the filesystem reader,
or application-service state.

One `DesktopActivationState` owns the queue, generation, overflow count, and
terminal consumption. It creates no background task. The existing
single-instance owner and macOS run loop are the only OS event sources. The
React bootstrap owns one listener and cancels it when the webview is destroyed.
The existing application service owns all torrent identity, persistence,
networking, storage, and shutdown behavior.

The implementation should factor the current raw IPC byte-add body so chooser
bytes and external-file bytes share request validation, one-upload admission,
service locking, response mapping, and immediate view publication. It must not
add an external-path operation to the generated application API.

## Platform Delivery Plan

### Common shell and UI

1. Add the pinned compatible deep-link plugin and enable the official
   single-instance deep-link integration while retaining single-instance as
   the first plugin.
2. Add `magnet` static scheme and `.torrent` bundle association metadata.
3. Implement pure classification, local-file URL conversion, per-event bounds,
   queue admission, ordered acknowledgement, and overflow accounting.
4. Feed cold-start activations into the queue before the main React client can
   request them. Subscribe to warm URL/open events and parse only the
   platform-authoritative file/argument lane so one OS action cannot be
   delivered twice.
5. Add a Tauri-only external-intake bridge that subscribes before pulling the
   current generation and hands one item at a time to `TorrentActions`.
6. Reuse magnet validation and the existing Add-options modal. Extend the
   pending source union with an opaque external-file item, then use a
   Tauri-only application-client method to consume/read/add that token after
   root/start choices.
7. Restore the window on accepted or visibly rejected input. Preserve the
   ordinary restore-only behavior for an unrecognized second launch.

### macOS

- Verify generated `CFBundleURLTypes` for `magnet` and document-type/UTI
  metadata for `.torrent`/`application/x-bittorrent`.
- Use bundled static registration only. Consume configured URL events through
  the deep-link plugin and local file opens through the authoritative Tauri
  `RunEvent::Opened` lane, with an exact-one guard if a plugin version exposes
  the same file URL through both.
- Prove installed arm64 cold and warm activation with `open`; do not require
  Intel installed testing.

### Windows

- Verify NSIS association/protocol registry output points to the installed
  per-user executable with correct quoting and no console process.
- Let configured magnets use deep-link/single-instance integration. Parse only
  bounded `.torrent` arguments from the single-instance callback and cold
  process arguments; switches and unrelated arguments retain restore-only
  behavior.
- Prove Explorer/Shell activation on a clean per-user install without granting
  unrelated firewall consent or editing JSTorrent registry ownership.

### Linux

- Verify generated desktop metadata includes
  `x-scheme-handler/magnet` and `application/x-bittorrent` and that installed
  DEB/RPM dispatch is well formed.
- For AppImage, use the documented bounded runtime registration/integration
  path needed to keep the absolute executable handler current. Record the
  resulting default-handler behavior rather than assuming desktop integration.
- Prove `xdg-open` cold and warm activation on the available arm64 desktop;
  retain x86_64 as a native build/package gate unless a matching installed
  testbed is available.

## Validation

### Deterministic Rust

- exact/case variants of `magnet:` and `.torrent` classify; lookalike schemes,
  extensions, remote file authorities, switches, directories, and unrelated
  arguments do not;
- percent-encoded local file URLs, spaces, Unicode, Windows drive paths, and
  platform separators convert without manual prefix slicing;
- 16-KiB magnets and 64-KiB activation representations admit exactly;
  one-byte-over values reject without retained contents;
- eight-item event/queue boundaries, FIFO order, saturation, cancellation,
  retry, terminal consumption, and generation changes are exact;
- duplicate observations of one platform event yield one queued activation,
  while a later intentional repeat remains eligible for normal duplicate-
  torrent handling;
- file reads cover absent, denied, directory, empty, exact 64 MiB, one byte
  over, short-read/growth, and replacement between activation and acceptance;
  no path or magnet appears in formatted errors or diagnostics; and
- chooser and external files share one upload permit, request validation, and
  application-service add helper.

### React and adapter

- activation before bootstrap, after listener installation, while an options
  dialog is open, and after webview reconstruction is neither lost nor
  duplicated;
- magnet and opaque file items reuse required-root, default-root, start-paused,
  show-options, skip-options, cancellation, retry, success, duplicate, and
  reveal-added-row behavior;
- queue overflow and inaccessible/empty/oversized inputs show bounded generic
  feedback;
- browser and demo builds expose no OS association behavior and retain their
  existing chooser/magnet tests; and
- test fixtures prove no path or complete magnet enters rendered markup,
  stored settings, application commands for file input, or console output.

### Package and installed acceptance

Hosted CI retains the existing credential-free eight-job floor and adds static
package assertions for the configured scheme, MIME type, extension, command
quoting, and required plugin integration. Retained opt-in packages support the
installed campaign.

For each installed required platform, use one tiny deterministic valid
`.torrent` and one bounded controlled magnet. Prove:

1. app stopped -> OS activation -> one process -> visible Add flow -> one row;
2. app visible -> OS activation -> same process/window -> one additional row;
3. app hidden in tray -> OS activation -> same process restored -> one row;
4. cancellation adds nothing and the next queued activation remains available;
5. inaccessible, empty, oversized, and malformed sources produce visible
   bounded feedback without a crash, second owner, or leaked source text;
6. a repeated already-added source follows the existing duplicate result and
   does not create a second catalog owner; and
7. tray Quit still joins the application service and leaves zero RSTorrent
   processes after the campaign.

The installed campaign records package identity/version, architecture,
artifact size and SHA-256, OS association query results, process identity
before/after warm activation, observed catalog outcome, and exact cleanup. It
removes only campaign-created files, associations/integration, profiles, and
artifacts, restores inherited machine state, releases every machine-control
claim, and returns caller-started-off machines to power off.

## Documentation Closure

Completion updates:

- this tactical with commits, package metadata, exact tests, installed
  artifacts, platform observations, cleanup, and deliberate deferrals;
- [`../topics/client-surfaces.md`](../topics/client-surfaces.md) with actual
  shell/UI behavior and evidence;
- [`../topics/beta-release-readiness.md`](../topics/beta-release-readiness.md)
  with the desktop-handler gate;
- [`../topics/capability-readiness.md`](../topics/capability-readiness.md) with
  current/next queue reconciliation; and
- changelog and maintainer instructions for installed handler testing.

## Commit Slices

1. Land this tactical and readiness-queue reconciliation.
2. Add bundle/plugin configuration, pure activation parsing/queueing, and
   deterministic Rust tests.
3. Add the Tauri external-intake adapter, shared byte-add refactor, React Add
   integration, and focused adapter/UI tests.
4. Add package validators and hosted retained artifacts; pass the local and
   credential-free matrix.
5. Run installed macOS arm64, Windows x86_64, and Linux arm64 campaigns; repair
   platform defects without weakening bounds.
6. Reconcile docs and evidence, commit, push, and require the exact hosted run
   to be green before marking the tactical complete.

## Completion Record

Not started.
