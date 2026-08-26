# Tactical 166: Desktop Native Bootstrap And Extension Scaffold

Status: **Implementation in progress and the sole Now on 2026-08-26.**
Maintainer direction temporarily yields signed-release Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) to this bounded
foundation. Tactical `158` remains active and resumes after this slice; this
does not publish or replace a signed desktop candidate.

Topics: `product-surfaces-and-migration`, `client-surfaces`,
`beta-release-readiness`

Dependencies: the maintained packaged Tauri desktop product, its
single-instance lifecycle from Tactical
[`162`](162-desktop-single-instance-and-tray-lifecycle.md), Chrome Manifest V3
and native-messaging contracts, and the maintained JSTorrent checkout at
revision `9598770baecb1164a00ba5d41f7e7c11bfb78828` as product history.

## Decision And Desired Outcome

Establish the smallest safe desktop extension/bootstrap foundation before the
beta extension has a Chrome Web Store identity. The result is:

- a store-uploadable, self-contained Manifest V3 **JSTorrent Beta** scaffold;
- one narrowly typed native-messaging executable named
  `com.jstorrent.rstorrent.native` that can report compatibility and request
  launch of the installed RSTorrent desktop application;
- repeatable desktop sidecar construction and first-launch/per-user native-host
  registration infrastructure; and
- exact documentation and validation for the store-identity handoff.

This slice does not settle the eventual detached React control surface,
headless application-service topology, simultaneous Tauri/extension ownership,
JSTorrent production migration, or Crostini design. It creates a testable seam
for those later decisions without putting peer, file, profile, or torrent
authority in the extension or bootstrap host.

The first store seed intentionally omits the extension manifest `key`. Chrome's
documented flow assigns the Web Store item ID and exposes its public key after
the ZIP is uploaded as a draft. The maintainer then supplies that ID and public
key in a follow-up. Only that follow-up may pin the extension development ID
and add its exact `chrome-extension://.../` origin to the installed native-host
manifest. Wildcards and placeholder origins are forbidden.

## Scope And Stopping Condition

This tactical owns:

1. a plain HTML/CSS/JavaScript Manifest V3 popup and service worker with only
   the `nativeMessaging` permission, no content scripts, host permissions,
   remote code, analytics, or torrent data path;
2. popup status plus explicit **Open RSTorrent** behavior over a small
   `hello`/`launch` native-messaging request set, including truthful guidance
   when the host is absent or the desktop app has not repaired registration;
3. a deterministic ZIP builder that packages an exact reviewed allowlist and
   produces the initial Chrome Web Store upload artifact;
4. a Rust stdio native host with bounded framing, typed requests and responses,
   caller-origin syntax validation, compatibility/capability reporting, and a
   launch request that reports only whether the request was accepted;
5. Tauri sidecar build/embedding plus per-user Chrome registration repair on
   desktop launch, package hooks where required, and uninstall behavior that
   removes only RSTorrent's own registration;
6. exact allow-list support for the existing JSTorrent production extension
   ID `dbokmlpefliilbjldladbimlcfgbolhk` from the first desktop package, while
   never registering or replacing legacy host `com.jstorrent.native`;
7. deterministic protocol, extension-package, registration-manifest, and
   proportional native package/build tests; and
8. documentation of the draft upload, item-ID/public-key return, unpacked-ID
   comparison, and subsequent exact-origin integration checkpoint.

The first implementation checkpoint stops when the reviewed ZIP is ready for
the maintainer to upload and the native host, registration writers, and
desktop sidecar/package gates pass locally. The tactical remains in progress
across the external store-identity wait. It completes after the returned beta
ID/public key are committed, both exact allowed origins are validated, and a
real Chrome-to-host `hello` plus desktop launch request passes on a supported
desktop installation. Publishing the extension is not required.

## Non-Goals

- Crostini packaging, a linger service, ChromeOS Launcher handoff, localhost
  routing, Android remote control, or physical ChromeOS evidence.
- A full React control surface, browser-hosted application views, torrent
  commands, profile selection, authentication, persistent native connection,
  REST/WebSocket gateway, or remote backend.
- Moving the existing in-process Tauri application service into a daemon or
  deciding headless/window/tray ownership.
- Modifying or publishing the current JSTorrent extension, taking over
  `com.jstorrent.native`, production-brand migration, legacy-state migration,
  or changing the JSTorrent desktop identifier/updater trust root.
- Automatically installing a browser extension, publishing a Web Store item,
  adding external users, changing store metadata, or using developer-account
  credentials.
- A macOS PKG solely for registration. The normal signed/notarized DMG remains
  the installation format; per-user registration is repaired on first launch.
- Signing, notarization, tags, releases, release-route mutation, or a new
  public desktop candidate.

## Native Bootstrap Contract And Bounds

The host uses Chrome native messaging exactly: UTF-8 JSON preceded by one
native-endian unsigned 32-bit length on stdin/stdout. Stdout contains protocol
frames only; bounded diagnostics use stderr. Windows stdin/stdout are switched
to binary mode before framing.

RSTorrent accepts at most 64 KiB per incoming JSON frame even though Chrome's
browser-to-host ceiling is larger. It processes one request at a time and
writes at most one response per request. A request ID is required, is at most
64 UTF-8 bytes, and is echoed exactly. Protocol version `1` initially supports:

- `hello`: returns host/product version, minimum/current protocol versions,
  caller origin, and the exact `launch_desktop` capability; and
- `launch`: validates compatibility, requests the already packaged desktop
  executable through a platform-specific launcher, and reports `requested`
  only after process creation succeeds.

Unknown operations, unsupported versions, malformed JSON, invalid request IDs,
invalid caller origins, and launch failures produce bounded typed errors when
a safe response can be formed. Oversized or truncated frames terminate without
allocation beyond the declared bound. EOF is normal cancellation and must
terminate promptly. The host holds no mutable torrent/profile state and starts
no listener or long-lived task.

Chrome's installed host manifest is the authorization boundary. Its
`allowed_origins` contains exact origins only. Passing an origin argument on
the command line provides an auditable caller identity but does not replace
Chrome's manifest enforcement. Direct host tests may supply a syntactically
valid test extension origin.

## Registration And Package Ownership

```text
Chrome extension popup
  -> MV3 service worker
  -> Chrome native-messaging authorization and process launch
  -> rstorrent-native-host (one bounded request process)
  -> platform launcher
  -> existing RSTorrent desktop single-instance owner

RSTorrent desktop startup
  -> pure native-host manifest builder
  -> per-user Chrome-family registration writer/registry adapter
  -> repairable registration pointing at the packaged sidecar
```

The extension worker owns no persistence in this slice. Chrome owns native-host
process creation and pipe closure. The host exits on request completion/EOF;
the launched desktop product retains its existing single-instance, window,
tray, update, and joined-shutdown owners.

Registration is per user and replace/repair safe. It must not create browser
profile roots for browsers that are not installed/configured. macOS and Linux
manifests contain absolute executable paths in each supported browser-specific
directory. Windows uses only RSTorrent's HKCU native-messaging-host key and an
absolute manifest path. AppImage's ephemeral mount cannot be a durable target,
so its first launch copies the tiny host to a stable app-owned user location
before writing registration. Uninstall/package hooks may remove exact
RSTorrent-owned entries but must not delete shared directories or legacy
JSTorrent registration.

## References And Adopted Lessons

Chrome's official native-messaging contract defines the host-name grammar,
exact `allowed_origins`, platform registration locations, caller-origin
argument, native-endian framing, stdout discipline, and browser process launch.
Chrome's Manifest V3 guidance requires executable code to ship in the
extension. Its manifest `key` guidance defines the draft-upload/public-key
procedure used for this tactical's identity checkpoint.

The maintained JSTorrent revision was inspected at:

- `desktop/manifests/com.jstorrent.native.json.template` for exact-origin
  registration history;
- `desktop/tauri-app/src-tauri/src/native_host.rs` for first-launch repair and
  platform placement history;
- `desktop/tauri-app/src-tauri/src/bin/native-host.rs` and its tests for EOF,
  framed request/response, handshake, and diagnostic behavior; and
- `extension/src/lib/native-connection.ts` for the existing production
  extension's hard-coded legacy-host dependency.

RSTorrent adopts small framed compatibility/launch operations, exact response
IDs, EOF exit, first-launch repair, and package validation. It intentionally
does not copy JSTorrent's profile store, daemon orchestration, takeover flow,
legacy host name, or native-host source. Merely allowing the production
extension's origin does not make it compatible: that extension must later be
changed to probe RSTorrent's distinct host deliberately.

The sibling `web-server-chrome` revision
`66a8c0ee95494f5b8632f7a2424a36e2da7495dd` informed only repeatable Tauri
sidecar naming/build mechanics. Its Crostini controller and service topology
remain outside this tactical. No reference source, fixture, test data, or asset
is copied.

This is platform/product integration rather than BitTorrent engine or protocol
work, so no pinned libtorrent completeness-oracle pass is applicable.

## Staged Implementation And Validation

1. **Decision gate:** land this tactical, queue reconciliation, and reference
   record before implementation.
2. **Host gate:** implement pure decode/dispatch/encode and launch selection;
   prove valid hello/launch, unsupported versions/operations, malformed and
   oversized input, response bounds, EOF, stderr/stdout separation, and child
   process behavior.
3. **Extension gate:** implement the MV3 scaffold and exact-allowlist ZIP;
   validate JSON, local-only code/assets, permissions, CSP-compatible markup,
   archive paths, size, and absence of secrets/build residue.
4. **Desktop gate:** embed the target-triple sidecar, implement pure manifest
   generation and platform registration, then pass Rust tests plus native
   desktop build/package validators on the available host.
5. **Store identity gate:** provide the ZIP to the maintainer; after the item
   ID/public key return, pin `key`, add the exact beta origin, compare unpacked
   and dashboard IDs, and run an installed Chrome `hello`/launch smoke.
6. **Closeout gate:** record exact commands/evidence, reconcile the topics and
   readiness row, and return the sole Now to Tactical `158` unless maintainer
   direction selects another bounded slice.

The default proportional baseline is:

```bash
source ~/.profile
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
npm run package:extension
npm run validate:extension
npm run validate:desktop-package -- --bundle <available-package>
```

Platform-specific registry/package assertions run where applicable. Store
upload, visible Chrome interaction, signing, and installed cross-platform
campaigns are explicit gates rather than silently inferred from source tests.

## Escalation Contract

Ordinary internal naming, pure-module extraction, test fixtures authored from
the public protocol, target-triple build plumbing, conservative error variants,
and package-validator changes are in scope. Stop for maintainer input if the
implementation would require a persistent service/listener, new remote
transport, profile/torrent authority, automatic extension installation, a
macOS PKG, legacy host takeover, production extension edit, store publication,
signing/release activity, or any architecture decision listed as a non-goal.
The expected beta item-ID/public-key wait is an external checkpoint, not
authority to invent a placeholder identity.

## Implementation Checkpoint: Store Identity Pending

The first implementation checkpoint is complete on 2026-08-26:

- commit `0f366a1` adds the independent Rust native host, 64 KiB framed
  protocol, lazy bounded launch configuration, Windows binary stdio, pure
  request/error tests, and real child-process EOF/stdout/stderr tests;
- commit `542984b` adds the seven-file Manifest V3 seed, native-only popup and
  worker, path-free setup guidance, permission/local-code/archive validators,
  deterministic ZIP builder, and store-identity handoff documentation;
- commit `5f25ffc` adds content-versioned first-launch registration, exact
  production JSTorrent origin, Chrome/Chrome for Testing/Chromium default
  locations, Windows HKCU registration and NSIS cleanup, stable AppImage launch
  targeting, explicit target-triple Tauri package/release overlays, and hosted
  package-placement gates; and
- ordinary Cargo tests do not require a generated sidecar. The developer
  launcher prepares a debug host, while only explicit Tauri package overlays
  prepare and embed release hosts.

The exact seed artifact is
`target/extension/jstorrent-beta-0.1.0.zip`, SHA-256
`2088a9ac45e1de8e507e6a34305d1a471d286c32ee40a208e5832e6150d248cb`.
Its allowlist contains only `manifest.json`, two existing RSTorrent PNG icons,
the popup HTML/CSS/JavaScript, and the service worker. It has no manifest
`key`, beta origin, host permissions, content scripts, remote code, network
request, dependency tree, documentation, build script, or secret.

Recorded local evidence:

- `cargo test -p rstorrent-native-host`: 9 tests pass, including two real
  child-process cases;
- `cargo test -p rstorrent-desktop`: 40 tests pass, including five registration
  and stable-copy cases;
- matching focused Clippy with `-D warnings` passes;
- full `cargo fmt --all -- --check`, workspace Clippy with `-D warnings`, and
  `cargo test --workspace` pass with only the repository's declared opt-in
  ignored tests;
- `npm test --prefix clients/extension`: three worker tests plus the source
  validator pass;
- two consecutive extension packages produced the same SHA-256 above;
- the release validator and its 17 package/ownership drift tests pass;
- shared-web typecheck, 279 unit tests with two declared skips, production
  build, and CSP scan pass;
- `actionlint` 1.7.9 accepts the changed CI and release workflows; and
- an exact unsigned macOS arm64 `RSTorrent.app` built through
  `tauri.package.conf.json`, contains executable arm64 desktop/host binaries,
  passes activation/sidecar validation, and its packaged host completes a real
  framed `hello` with no stderr or trailing stdout bytes.

No visible application, real browser connection, Web Store action, signed
package, Windows/Linux hosted package, publish, tag, release, or Crostini work
was performed. The tactical remains the sole **Now**, waiting for the
maintainer to upload the seed as a draft and return the dashboard item ID plus
single-line public key. The next in-scope change pins that public `key`, proves
the unpacked ID matches, adds only the exact beta origin beside the production
origin, and runs the installed Chrome `hello`/launch smoke. It does not decide
or begin the later control surface.
