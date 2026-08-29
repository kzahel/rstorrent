# Download Root Acquisition Across Runtimes

Topic: `download-root-acquisition`

Status: Current behavior researched on 2026-08-29. Packaged desktop, local
browser gateway, Crostini, and Linux headless use distinct platform adapters
despite sharing the React storage UI. Windows packaged-desktop picker evidence
passes, while the local browser gateway has no Windows picker implementation.
Linux local-WebUI selection is a best-effort Zenity-then-KDialog operation;
Linux headless deliberately injects no native picker and currently exposes a
nonfunctional **Add folder...** action that returns HTTP `501`. Maintainer direction on
2026-08-29 accepts typed absolute server paths for the exact Linux headless
product, with server-side validation and durable root identity. That direction
is not implemented and has no tactical yet.

## Conclusion

The browser's operating system does not decide which folders can be selected.
The backend process owns payload I/O, so acquisition follows the backend
runtime and its session:

- packaged Tauri opens a native dialog in the desktop application's graphical
  session;
- a local browser gateway asks its same-machine backend to launch a native
  helper, not the browser;
- a browser controlling a remote Linux headless service cannot select a
  client-machine folder and must not cause the server to launch a dialog; and
- the exact Linux headless product should instead accept and validate an
  absolute path in the server's namespace, such as `/srv/media/torrents`.

An opaque root ID remains the portable application value after acquisition.
Neither a picker result nor a typed path belongs in `add_magnet`, torrent
records, relay records, or browser-local persistence.

## Scope And Ownership

This topic owns how a backend locator or platform capability is initially
acquired or repaired across product runtimes, including:

- which current picker implementation each operating system and runtime uses;
- whether a graphical session or helper program is required;
- what the user sees when no picker is available;
- the accepted typed-path behavior for Linux headless;
- path-validation and configuration-provenance requirements; and
- the evidence required before claiming a platform path-acquisition flow.

[`download-roots.md`](download-roots.md) continues to own stable root identity,
default and per-torrent selection, availability, repair semantics, and removal.
[`runtime-configurations-and-headless-deployment.md`](runtime-configurations-and-headless-deployment.md)
owns process, service-manager, listener, and deployment policy.
[`android-saf-storage.md`](android-saf-storage.md) owns Android SAF capability
lifetime. This topic does not move payload I/O into a browser, add a generic
filesystem API, or authorize a presentation to enumerate the backend machine.

## Current Behavior Matrix

| Product surface | Backend operating system and session | Current acquisition operation | Current user-visible result |
| --- | --- | --- | --- |
| Packaged Tauri desktop | macOS graphical login | `tauri-plugin-dialog` backed by the native macOS folder panel and parented to the invoking window | Native folder picker; choose and cancel flow through Tauri IPC |
| Packaged Tauri desktop | Windows graphical login | `tauri-plugin-dialog`/`rfd` backed by the Windows Common Item Dialog and parented to the invoking window | Native folder picker; installed choose, cancel, default, repair, and restart evidence passes |
| Packaged Tauri desktop | Linux graphical login | The exact dependency feature set selects the `rfd` GTK3 backend | Native GTK folder picker; no Zenity or KDialog runtime dependency; installed interaction is not yet claimed |
| Local `scripts/webui` | macOS graphical login | The gateway runs `/usr/bin/osascript` with AppleScript `choose folder` | Unparented native folder panel; source and focused tests pass, but the manual chooser/restart smoke remains open |
| Local `scripts/webui` | Linux graphical session | The gateway launches `zenity --file-selection --directory`; only command-not-found falls through to `kdialog --getexistingdirectory` | Best effort. Cancel is clean; no helper gives an actionable error; another helper failure is an error rather than fallback |
| Local gateway/browser | Windows | The native adapter is compiled as unsupported for every operating system except macOS and Linux | Clicking **Add folder...** reaches the HTTP platform route and returns `501` with “download folder picker is not implemented on this platform” |
| ChromeOS Linux/Crostini | Linux user service, ChromeOS browser presentation | Crostini injects the same Linux native helper adapter as local WebUI | `~/Downloads` is preconfigured, so first use does not require a picker. Additional selection is conditional on a usable Linux graphical session and Zenity or KDialog; exact installed interaction is unproved |
| Hosted Linux headless | Linux service with or without a display | `rstorrent-headless` uses the hosted gateway preparation that injects `UnavailableDownloadDirectoryPicker` | TOML supplies at least one root. The shared UI still renders native-picker add/repair controls, but those calls return `501` |
| Android/ChromeOS Android | Android activity | Android Storage Access Framework tree selection and a persisted URI grant | System tree picker; the platform adapter retains the capability and Rust uses an opaque root ID |
| iOS/iPadOS | First-party iOS application | System directory picker followed by a qualified security-scoped bookmark | System picker; accepted local roots retain opaque identity and platform-owned reopening state |

“Web UI on Windows” is ambiguous and must be qualified:

- a Windows browser controlling a Linux headless host operates on Linux server
  paths and cannot open a useful Windows folder chooser for that host;
- the maintained Windows Tauri product has a native picker; and
- a gateway process running locally on Windows has no RSTorrent picker today.
  `scripts/webui` is also a Bash-oriented local development launcher, not a
  maintained native Windows product launcher.

The WebSocket client does not change this split. Its platform operations
delegate to the same HTTP `/api/v1/platform/download-root` endpoint.

## How The Two Desktop Implementations Differ

### Packaged Tauri

`clients/desktop/src-tauri/src/lib.rs` handles `choose_download_root` through
Tauri's dialog plugin, sets the invoking application window as parent, starts
from the suggested directory, and passes the selected Rust `PathBuf` directly
to `ApplicationService`. JavaScript does not receive filesystem authority.

The exact dependency is `tauri-plugin-dialog = 2.7.2`, resolving to
`rfd 0.16.0`. Its selected Cargo features use GTK3 on Linux. On Windows, the
underlying Common Item Dialog supports directory selection through
`IFileDialog`/`FOS_PICKFOLDERS`; on macOS the native concept is
`NSOpenPanel.canChooseDirectories`. These are interactive desktop APIs, not
headless path-browsing services:

- [Microsoft Common Item Dialog folder selection](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/ne-shobjidl_core-fileopendialogoptions)
- [Apple `NSOpenPanel.canChooseDirectories`](https://developer.apple.com/documentation/appkit/nsopenpanel/canchoosedirectories)

### Local browser gateway

The shared React UI calls the gateway's platform endpoint. Local hosted and
Crostini preparation inject `NativeDownloadDirectoryPicker`; generic hosted
preparation injects the unavailable implementation. The adapter selection is
therefore an explicit runtime policy, not a guess based on the request's user
agent.

The current Linux adapter is intentionally small but not a general Linux
desktop integration layer. It relies on executable lookup, an interactive
display, and the session environment needed by the helper. It does not probe
DBus, XDG portals, `DISPLAY`, `WAYLAND_DISPLAY`, or helper usability before the
button is shown.

The XDG Desktop Portal FileChooser API is a plausible future interactive-Linux
backend: it has an asynchronous folder-selection request, a `directory` option,
and an optional parent window. It still requires a working graphical portal
session and backend, and a browser-hosted gateway does not own the browser
window needed for normal modal parenting. It is not a headless solution and is
not used by current RSTorrent code:

- [XDG Desktop Portal FileChooser](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html)

The browser File System Access API is also not a substitute. A browser call to
`showDirectoryPicker()` returns a handle to the browser client's local
filesystem after a user gesture. It cannot give a remote Rust backend a native
server path or move torrent payload I/O out of the engine:

- [File System Access specification](https://wicg.github.io/file-system-access/)

## Current Validation And Failure Semantics

### Interactive picker result

After either path-backed desktop implementation returns a path,
`ApplicationService`
canonicalizes it, requires it to be a directory, and requires `read_dir` to
succeed before registration or repair. The registry deduplicates the canonical
locator, enforces the 32-root bound, and retains a stable opaque root ID.

This is meaningful existence/readability validation, but it is not proof that
the service can create, rename, sync, and delete payload files. A listable
read-only directory can appear available until an actual write fails.

### Headless configuration

The current strict TOML configuration accepts one through 32 `storage_roots`.
Each path must be an absolute UTF-8 Linux path of at most 4096 bytes, without
NUL, line endings, `.` or `..` components, duplicate IDs or locators, symlink
components that already exist, or overlap with protected profile,
configuration, and release paths. A present target must be a directory.

A missing configured target is deliberately accepted. The service starts with
`PathRootStartupPolicy::PreserveUnavailable`, projects the exact display path
as **Unavailable — repair required**, and leaves affected torrents awaiting
storage. It does not create the missing directory or silently select another
root. Syntax and protected-path failures prevent startup and are reported in
the service logs; there is no standalone validate-only command.

This startup behavior should remain distinct from interactive typed-path
registration: preserved operator intent may be temporarily offline, while a
new path submitted through the UI should not be registered when it does not
exist.

## Accepted Linux Headless Direction

The exact `rstorrent-headless` presentation should replace native-picker
actions with a text field labeled **Path on this Linux server** and an example
such as `/srv/media/torrents`. The field accepts a Linux server path, not a path
on the browser machine.

The first implementation should provide typed add and repair for UI-managed
roots without building a server directory browser. A generic browser would
need bounded roots, pagination, symlink policy, mount handling, directory-name
disclosure rules, and a more complicated authorization surface. There is no
evidence that this complexity is preferable to a path field for the headless
operator use case.

### Authority boundary

Typed path registration is a headless platform operation, not a portable
application command:

- only the exact Linux headless runtime may advertise server-path entry;
- the backend validates and installs the locator, then returns the ordinary
  root snapshot;
- existing add flows continue to select the returned opaque root ID;
- the operation follows the deployment endpoint's storage-management
  authority and must not become anonymously reachable through a relay or a
  client-supplied product claim; and
- local browser, Tauri, Android, and iOS adapters cannot gain server-path
  authority merely by sending the same request shape.

The connection/bootstrap contract should advertise an acquisition mode such
as native picker, server path, or unavailable. React should render the
matching control before the user acts. Runtime identity already distinguishes
the exact headless and Crostini hosted products, but a capability remains the
authoritative control because product identity alone does not prove a usable
operation.

### Required validation before mutation

A new typed path must not enter the durable registry until the server reports
all applicable checks as successful:

1. Require an absolute UTF-8 Linux path. Reject `~`, environment variables,
   `file:` URLs, Windows drive paths, NUL or line endings, dot components, and
   values over the existing 4096-byte bound.
2. Require every component and the final target to exist at registration time.
   Reject symlink components, a non-directory target, and resolution errors.
3. Require the service identity to open and enumerate the directory.
4. Reject duplicate canonical locators, the existing 32-root bound, and
   overlaps with profile, configuration, release, or other explicitly
   application-owned state.
5. Return typed, actionable outcomes for invalid syntax, not found, not a
   directory, permission denied, unsafe symlink, protected overlap, duplicate,
   root limit, and internal I/O. At minimum, the UI must say **Path does not
   exist on the server** for the common missing-path case.
6. Do not create the directory automatically in the first slice. Directory
   creation has separate parent selection, permission, ownership, mode, and
   cleanup semantics.

Metadata and enumeration cannot prove writeability. The implementation
tactical should include a bounded Linux write-probe design before claiming
that a path is writable: create a uniquely named private probe beneath the
selected directory without following symlinks, exercise the minimum
create/write/sync/rename/delete operations required by storage, and remove it
before registration. A failed or incompletely cleaned probe must block
registration and report the exact stage. If the tactical declines this
temporary mutation, the UI may claim only **exists and is readable**, not
**ready for downloads**.

Validation is point-in-time. Mounts and permissions can change after success;
the existing unavailable-root and storage-failure states remain necessary.

### Configuration provenance

Headless TOML roots and UI-added roots currently have different authorities:

- each TOML root is reapplied by ID on every service start and can overwrite
  that database row's label and locator;
- a root installed through `ApplicationService` is stored in the profile
  database and survives restart when its ID is not owned by the TOML; and
- default-root changes are database state and are not overwritten merely
  because the TOML is read again.

The current snapshot does not expose that provenance. Consequently, removing
or repairing a TOML-owned root in the UI can appear successful and then revert
on restart. Before typed path repair is exposed, the view model must identify
configuration-owned roots. The first headless UI should label them
**Configured by the service**, disable remove/repair, and direct the operator
to edit the configuration and restart. Typed add, repair, and remove apply to
database-managed roots. Rewriting protected TOML from the browser is not part
of this direction.

## Windows And Linux Follow-Up Decisions

### Windows local browser gateway

Windows has a suitable folder-selection API, and JSTorrent's current native
host reference uses `rfd` on non-macOS systems. RSTorrent could add an `rfd`
or direct Common Item Dialog implementation when the gateway runs in an
interactive Windows user session. That would still be separate from Tauri and
would need its own lifetime, foreground, cancellation, and installed evidence.

Do not add the dependency only to make a theoretical source configuration
compile. First decide whether a native Windows local-WebUI launcher is a
maintained product surface. Until then, expose the capability as unavailable
and avoid presenting a button that can only return `501`.

### Linux local browser and Crostini

Zenity then KDialog remains an honest best-effort implementation, not a general
Linux picker guarantee. A follow-up tactical may evaluate an XDG portal
backend, but it must test GNOME/GTK, KDE, no portal backend, missing session
bus, cancellation, parentless modality, service-manager environment, and
Crostini specifically. Crostini's preconfigured `~/Downloads` should remain so
the product is usable without that optional interaction.

## Evidence

Current code and dependency evidence:

- `clients/desktop/src-tauri/src/lib.rs` owns the parented packaged-desktop
  command; `clients/desktop/src-tauri/Cargo.toml` pins the dialog plugin.
- `crates/rstorrent-platform/src/lib.rs` contains the macOS AppleScript,
  Linux Zenity/KDialog, and other-platform unsupported branches.
- `crates/rstorrent-gateway/src/lib.rs` selects native versus unavailable
  adapters and maps unsupported selection to HTTP `501`.
- `crates/rstorrent-headless/src/runtime.rs` selects generic hosted mode and
  preserves unavailable roots.
- `crates/rstorrent-headless/src/config.rs` owns strict configuration and
  protected-path validation.
- `crates/rstorrent-session/src/application.rs` owns selected-directory
  canonicalization, readability, and availability; `store.rs` owns durable
  registration and configuration reapplication.
- `clients/web/src/inspection/components/DownloadSettingsSection.tsx` currently
  renders one picker-shaped add/repair interface without acquisition-mode
  awareness.
- Tactical [`061`](../tactical/061-user-selected-download-roots.md) records
  real Ubuntu GNOME/Zenity local-WebUI choose, cancel, restart, repair, and
  missing-helper evidence.
- Tactical [`161`](../tactical/161-packaged-desktop-folder-picker.md) records
  installed Windows Tauri picker evidence and the remaining installed-Linux
  gate.
- Tacticals [`170`](../tactical/170-configured-linux-headless-service.md) and
  [`171`](../tactical/171-signed-headless-release-and-lan-service.md) record the
  configured headless service and real lifecycle evidence.
- Tactical [`178`](../tactical/178-crostini-storage-guidance.md) proves
  Crostini storage guidance and performance labels, not an installed helper
  picker interaction.

The 2026-08-29 Machine Control check found a usable Windows target but only a
locked interactive session and no Rust toolchain or prepared gateway binary
available to that session. No login, toolchain installation, or UI claim was
inferred; the target was returned to its prior powered-off state and its claim
was released. The
current Windows local-gateway result is therefore a source-derived unsupported
claim, while installed Windows Tauri evidence remains the live product claim.
A bounded Linux target readiness check did not complete, so this research
relies on Tactical `061` rather than adding a new Linux live claim.

The JSTorrent reference at `~/code/jstorrent/desktop/host/src/folder_picker.rs`
uses AppleScript on macOS and `rfd` elsewhere, and validates an externally
registered path for existence and directory type. It demonstrates a possible
Windows native-host implementation, not behavior present in RSTorrent's local
gateway.

## Next Bounded Work

The recommended next tactical is one headless path-acquisition slice with this
stopping condition:

- a typed and runtime-gated server-path operation exists for Linux headless;
- capability/provenance facts drive the React controls;
- missing, invalid, protected, duplicate, unreadable, and unwritable paths
  produce distinct evidence-backed outcomes without registering a root;
- a valid UI-managed root survives service restart under the same opaque ID;
- configuration-owned roots cannot be deceptively repaired or removed;
- local WebUI and Tauri retain their existing picker behavior; and
- the application command and relay schemas still carry only opaque root IDs.

Windows local-WebUI support, an XDG portal adapter, a generic server directory
browser, config-file rewriting, and automatic directory creation should remain
separate decisions unless evidence gathered during that tactical changes the
scope.
