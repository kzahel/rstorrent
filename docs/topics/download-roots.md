# Download Roots And Add Options

Topic: `download-roots`

Status: Product behavior accepted in maintainer discussion on 2026-08-03.
Desktop and the manual WebUI still install one implicit app-data-backed root
named `downloads`, and the live add path still selects that root and every file
without asking. Android already proves a user-selected persisted SAF root, but
general root management is not implemented. The first implementation slice
should establish user-selected roots and default-root behavior; staged magnet
metadata and file selection are explicitly deferred to a later slice.

## Scope

This topic owns the product meaning and user experience of download roots:

- how a first usable root is acquired;
- how roots are named, persisted, selected, defaulted, checked, repaired, and
  removed;
- how an add flow chooses a root without exposing ambient paths on the
  application command boundary;
- which behavior is shared across desktop, the local browser presentation, and
  Android despite different platform capabilities;
- what belongs in application-private storage versus a user-visible content
  location; and
- the intended user-visible publication layout beneath a root.

It complements:

- [`client-persistence.md`](client-persistence.md), which owns stable root
  identity, SQLite records, verified resume, and cross-platform portability;
- [`application-control.md`](application-control.md), which owns semantic
  commands and the platform-capability boundary;
- [`product-surfaces-and-migration.md`](product-surfaces-and-migration.md),
  which owns backend/profile isolation and later JSTorrent import root
  remapping;
- [`web-ui-design.md`](web-ui-design.md), which owns the shared React
  presentation and browser-local versus durable state; and
- [`storage-throughput-architecture.md`](storage-throughput-architecture.md),
  which owns content I/O, staging, hashing, durability, and root-level resource
  scheduling.

This topic does not implement torrent relocation, automatic content import,
dynamic file priorities, fast resume, browser filesystem I/O, or the later
staged magnet file-selection flow.

## Terms And Ownership

A **download root** is a long-lived, user-visible base location into which the
application may publish torrent content. It is not the application-data
directory, a torrent-specific final path, or an arbitrary path supplied by a
presentation.

A root has a stable opaque **root ID** used by the application service,
database, commands, and torrent records. The ID is not an absolute path, SAF
URI, descriptor number, browser handle, or hash whose identity changes when a
locator is repaired. A root also has bounded display metadata and an explicit
availability or permission state.

The ownership split is:

- the application service and profile database own the root registry, stable
  root IDs, default root, availability visible to application views, and each
  torrent's selected root;
- the platform adapter owns acquisition and reopening of the operating-system
  locator or capability, such as a desktop path, future macOS persistent access
  token, or Android SAF tree grant;
- the engine owns safe relative torrent layout, staging, verification,
  publication, and content I/O beneath an already resolved capability; and
- the presentation requests semantic root selection and displays established
  roots, but does not become filesystem authority.

The default root and root registry are durable profile/application state. They
are shared by every presentation attached to the same backend and are not
browser-local preferences. A setting that skips add options also changes add
behavior and therefore belongs to the profile rather than one browser tab.

## Accepted User Experience

### Fresh profile and first add

A fresh product profile has no implicit payload root. Application-private
storage contains the profile database, settings, exact metadata, and bounded
application-owned support state, but not ordinary torrent payload.

Do not ask for a folder merely because the application launched. On the first
user-initiated torrent add:

1. present the add options;
2. explain that a download folder is required;
3. offer **Choose folder...** in that flow;
4. acquire the folder through the local platform adapter;
5. register it as a stable root and make this first root the default; and
6. start content only after the add is confirmed with a usable root.

If the picker is cancelled or permission is denied, retain the user's add
input while the dialog remains useful, but do not create payload or report a
successful download start.

There is no automatic product fallback to an app-data `downloads` directory.
There is also no later automatic move from hidden storage. Moving partial or
complete content requires joined torrent ownership, crash-safe rename or
copy, cross-volume handling, conservative recheck, rollback, and exact cleanup
and is a separate feature.

### Default root and per-add choice

The product follows the current JSTorrent behavior:

- **Show options when adding torrents** defaults on.
- The add dialog preselects the current default root and initially selects all
  files once file selection exists.
- The first configured root becomes the default automatically.
- Selecting a different established root for one torrent does not silently
  change the default.
- Making a later root the default is an explicit Settings action or a clearly
  labeled per-add choice.
- **Don't show again** is an accessible shortcut for turning off the add-option
  preference. Settings can turn it back on.
- When add options are off, an ordinary add uses the available default root
  and all files. A missing, unavailable, or permission-lost default overrides
  that preference and requires root selection or repair.

Changing the default affects future torrents only. Every accepted torrent is
pinned to the root selected for that add. It is never silently redirected
because another root becomes the default.

### Root settings and repair

Settings should expose a **Downloads** or **Storage** section with:

- the current default root;
- registered roots with useful labels, local location display where policy
  permits it, availability, permission state, and free space when supported;
- **Add folder...**, **Make default**, **Repair/Re-select**, and bounded removal
  actions; and
- an explanation that changing the default does not move existing torrents.

Repair updates the platform locator or capability behind the same stable root
identity after appropriate validation. It does not rewrite every torrent to a
new root ID or trust prior verified state without the normal storage checks.

A root referenced by retained torrents cannot be removed as if it were unused.
The product must require those torrents to be removed, retained in an explicit
unavailable state, or deliberately relocated by a later feature. Removing an
unreferenced default root either requires choosing a replacement or leaves the
profile with no usable default; it does not silently remap existing torrents.

An unavailable root is a missing prerequisite, not evidence that torrent
metadata, the database, or verified-piece state is corrupt. Preserve user
intent and expose an actionable waiting/repair state.

## Current Root-Selection Slice

The first root tactical should solve the immediate product problem without
absorbing the later magnet file-selection state machine:

- persist and project established roots plus one default root;
- acquire, register, select, and repair a desktop root through a local platform
  capability;
- make the behavior available from both Tauri and `./scripts/webui`;
- require root choice on the first add and use the selected/default root for
  all files;
- add the root controls and truthful unavailable states to the shared React
  surface;
- keep Android's user-selected SAF root behavior aligned without requiring a
  multi-root Android redesign in the same slice; and
- remove the implicit app-data payload root from ordinary interactive product
  behavior.

This slice may continue to add every file because runtime initial file
selection is not yet implemented. It must describe that limit honestly and
must not fabricate a file chooser whose choices cannot reach durable engine
state.

## Deferred Staged Magnet And File Selection

The complete JSTorrent-like add experience is accepted direction but belongs
to a later tactical. A magnet does not reveal its file list until verified
metadata arrives, while the current RSTorrent `add_magnet` command requires a
root and skip list immediately. Implementing the final flow therefore needs a
durable or explicitly owned pending-add/configuration transition, not only a
new React dialog.

The later desired flow is:

1. user submits a magnet or opens a `.torrent` file;
2. a magnet may acquire and verify metadata while content storage remains
   disabled;
3. the add dialog displays a loading state and then the bounded file list;
4. the user chooses a root and wanted files;
5. one confirmation durably records root, initial selection, and running
   intent before content starts; and
6. cancel removes the pending intake and joins any metadata owner without
   leaving payload artifacts.

`.torrent` input already has metadata and can display files immediately once
that intake exists. The later tactical must define duplicate-add behavior,
pending-intake restart semantics, cancellation, queueing, and file-count/UI
bounds. Dynamic priority changes after content starts remain separable work.

## Desktop, WebUI, And Remote Boundaries

The shared React surface expresses **choose a download folder** as a platform
operation. It never sends an ambient path inside `add_magnet`, another portable
application command, or browser-local persistence.

- Tauri asks its native platform adapter to open the folder picker and install
  the resulting root capability.
- The local `./scripts/webui` backend may expose the same trusted local
  operation over its authenticated or explicitly development-scoped
  connection. The gateway process, not the browser sandbox, resolves and owns
  the filesystem capability.
- A future remote or relayed presentation may select an already established
  root. It cannot register a path on the backend merely by submitting a string.
  If that transport cannot safely invoke a local picker, it directs the user
  to the native product to add or repair a root.

Do not use the browser File System Access API as an engine storage backend and
do not move piece payload through the web presentation. Browser directory
handles cannot supply the native Rust engine's durable cross-platform storage
authority and would violate the existing hot-path boundary.

Interactive `./scripts/webui` should not silently create or select
`.local/webui/downloads`. Keep an explicit environment/CLI root injection for
developer workflows, controlled harnesses, and headless automation. Such a
root is visibly preconfigured rather than inferred as product consent.
Automated tests continue to use isolated temporary roots and never open a
visible picker.

## Platform Behavior

### macOS and other desktop platforms

On a fresh macOS profile, start the folder picker at Home rather than probing
or defaulting to Downloads. After the user has established roots, start at the
most recent usable root when the platform can do so without an unrelated
permission prompt. This follows the current JSTorrent behavior and keeps any
Downloads access request in the context of the user's explicit choice.

The platform adapter must detect denial and lost access on restart. A plain
path may be sufficient for the initial unsandboxed desktop build, but the root
model must permit a future platform-specific persistent capability without
changing portable commands or torrent records.

Windows and Linux use their native folder picker and path capability while
preserving the same first-root, default, repair, and per-torrent binding
semantics.

### Android and ChromeOS Android

Android continues to use a persisted SAF tree selected through the system
picker. SAF URI and grant state stay in the platform adapter; SQLite and
portable application values retain the stable root ID. A revoked grant leaves
the torrent waiting for repair rather than selecting app-private storage.

The existing one-root Android product is a valid initial presentation limit.
General multi-root Settings parity with desktop may land later, but it must not
change the shared root identity and per-torrent binding semantics.

## User-Visible Publication Layout

A chosen root is intended to contain recognizable content rather than a
permanent hash-named product layout:

- a single-file torrent publishes as `<root>/<filename>`; and
- a multi-file torrent publishes as `<root>/<torrent name>/...` using its safe
  metainfo-relative tree.

Engine-owned staging, part files, and other incomplete artifacts may use
bounded hidden names containing the info hash so ownership and cleanup remain
unambiguous. They must remain beneath the selected root unless a later
explicit storage design says otherwise.

Never overwrite or merge an existing final destination implicitly. The
initial product may report a visible destination conflict and require another
choice. A later **use existing data and recheck** action may deliberately adopt
matching content through the ordinary integrity path. Automatic suffixing,
blind overwrite, and treating same-length files as verified are not accepted
fallbacks.

The current `<root>/<info-hash>` publication shape is bring-up behavior. A
root tactical must state whether user-visible naming is included or remains a
bounded follow-up; root selection alone must not be described as the completed
download-destination product if hash-only names remain.

## JSTorrent Product Cheat Sheet

JSTorrent is the first-party behavior and terminology reference for this
topic. The sibling checkout is declared in [`../../reference/pins.toml`](../../reference/pins.toml)
as `../jstorrent`, normally available at `~/code/jstorrent`. This topic was
written after inspecting sibling commit
`9895410beeed6aff554053769bd006a3fbd373ef` on 2026-08-03.

Before implementing a root or add-options tactical, inspect the then-current
sibling `main` and record any changed behavior. These paths are the useful
starting map, relative to the JSTorrent repository root:

| Concern | JSTorrent path or symbol | Lesson to retain |
| --- | --- | --- |
| Default add preference | `packages/engine/src/config/config-schema.ts::showFileSelection` | Showing file/location options defaults on. |
| User-add policy | `packages/client/src/utils/add-torrent-options.ts::getUserAddTorrentOptions` | User adds honor the preference; restore and diagnostic paths do not pretend to be user adds. |
| Pending metadata and confirmation | `packages/client/src/AppContent.tsx` file-selection queue and confirm handlers | A pending magnet may fetch metadata, but root and file choice precede content activity. |
| Combined dialog | `packages/ui/src/components/FileSelectionModal.tsx` | Default root, roots, free space, loading metadata, files, summary, cancel, download, and Don't show again belong in one flow. |
| First root and default | `packages/client/src/App.tsx` root setup handlers | The first usable root becomes default; later changes are explicit. |
| Root settings | `packages/client/src/components/SettingsOverlay.tsx` | Roots can be added, defaulted, removed with warning, and paired with the add-options preference. |
| Root/default synchronization | `packages/client/src/engine-manager/daemon-engine-manager.ts` | Root inventory, persisted default, live engine state, and platform host state must be reconciled deliberately. |
| Per-torrent root resolution | `packages/engine/src/storage/storage-root-manager.ts` | A torrent-specific root overrides the default; missing roots fail explicitly. |
| Root in resume state | `packages/engine/src/core/session-persistence.ts` | Each torrent's chosen storage key survives restart. |
| Desktop folder picker | `desktop/host/src/folder_picker.rs` | Start at a recent usable root or Home on macOS, deduplicate roots, and return a stable registered root. |
| Android root persistence | `android/app/src/main/java/com/jstorrent/app/storage/RootStore.kt` | Persist SAF roots outside the engine and resolve opaque keys through the platform owner. |
| Android add/file choice | `android/app/src/main/java/com/jstorrent/app/ui/dialogs/FileSelectionDialog.kt` | Use the same default-root and initial-file-choice semantics in a platform-appropriate UI. |
| Android first-root setup | `android/app/src/main/java/com/jstorrent/app/AddRootActivity.kt` and `ui/screens/StorageSettingsScreen.kt` | The first root becomes default and later root management remains explicit. |

Use JSTorrent as a UX and failure-history reference, not an architecture or
source donor. RSTorrent does not adopt its extension engine, native-host/IO
daemon topology, ConfigHub persistence shape, mutable engine objects, path-
derived root identity, or mixed React/Compose implementation. No JSTorrent
source, fixture, or asset is imported by this topic.

## Invariants

- Ordinary product payload never defaults to application-private storage.
- Content I/O cannot begin without one usable, durably selected root.
- The first selected root becomes default; a one-torrent override does not.
- Default-root changes affect future adds only.
- A torrent refers to a stable root ID, never an ambient path or descriptor.
- Platform locator loss becomes an actionable unavailable state, not silent
  remapping or false corruption.
- Presentations cannot grant native filesystem authority by sending strings.
- Root removal, repair, publication, and later relocation never silently move,
  overwrite, merge, or trust content.
- Interactive and headless add paths distinguish user consent from explicit
  developer/test root injection.
- The first root slice may download all files, but it cannot claim the deferred
  metadata-backed file-selection UX is implemented.

## Recommended Next Work

When root implementation is authorized, create one bounded tactical for the
root registry/default, local picker capability, Tauri/WebUI presentation,
first-add requirement, truthful all-files behavior, root repair, and removal
of the implicit app-data product root. It must read this topic plus the
persistence, application-control, client-surface, and web-UI topics it names.

Do not include the pending magnet metadata/file-selection transition in that
tactical. Open a later tactical from the deferred flow above after root
selection is established and the command/state ownership can be designed from
the verified metadata boundary.
