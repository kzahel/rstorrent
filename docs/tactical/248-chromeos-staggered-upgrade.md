# ChromeOS Staggered Extension And Android Upgrade

Status: Active, 2026-09-30.

## Scope And Stopping Condition

Implement the accepted four-pair contract in product-surfaces-and-migration:
new extension/old Android requires an update; old extension/new Android loses
control while standalone Android works; new/new attaches after local import.
Prove normal legacy extension writers persist a library that survives an
installed Android replacement, and exercise bounded mixed-version behavior.
Commit documentation first, then implementation and recorded test evidence.

Read owners: product-surfaces-and-migration, android-jstorrent-replacement,
client-surfaces, web-ui-design, application-view-api, android-saf-storage,
client-persistence, download-roots, android-lifetime-and-background-work,
localization, plus tacticals 194, 245, 246 and 247.

Non-goals: production publication/signing/branding, Crostini migration, old
raw-I/O compatibility server, modifying the legacy engine, older browser-local
library conversion, automatic token reuse, public swarms or personal-data
replacement. A preparatory legacy-extension messaging release is optional.

## Invariants, Bounds And Owners

Android service initialization owns import before engine/companion admission.
Existing atomic import, retained sources, payload checking and SAF bindings
remain authoritative. Latest destination owners win and duplicates are safe.
The extension owns only its bounded connection/presentation and aborts work
when canceled; mismatches never open old pairing, KV, socket or file channels.
Fixed ARC host and exact approved origins remain; legacy discovery reads only
unauthenticated status on the five existing legacy discovery ports. Each probe
has a two-second deadline and 64-KiB response ceiling, with no redirects,
credentials, arbitrary endpoint override or LAN fallback in shipping code.

The rehearsal owns a fresh emulator, disposable same-package signing key,
separately identified Playwright Chromium/profile, unchanged pinned legacy
extension build, loopback seed/tracker and exact fixtures. Join browser,
emulator and server processes and remove owned temporary state. Never select
an attached personal device or overwrite the installed physical library.

## Source Audit And Required Evidence

JSTorrent source pin 7b454be4410385f9c4f7f135cb6b16194a2b0409:
extension/src/sw.ts::handleKVMessageViaWebSocket and KV routing,
extension/src/lib/chromeos-bootstrap.ts discovery/pairing,
android/app/.../CompanionServerDepsImpl.kt KV provider,
android/quickjs-engine/.../SqliteKVStore.kt database and schema,
android/companion-server/.../NettyHttpServer.kt status fingerprint.
Remote KV routing first appears at 1f523446 (2026-02-01); browser-local fallback
exists when no remote handler is selected. Qualification is the pinned
connected companion case, not every historical source.

No engine/protocol/storage-format change is planned. Reuse 246/247's pinned
libtorrent oracle and independent fixtures; foreign completion bits remain
untrusted and the ordinary checker verifies final bytes.

Required evidence: strict legacy/current/incompatible/malformed discovery,
current-service preference, offline/cancellation and revoked-pairing handling;
real old extension settings/intake/session writer into old Android; retained
torrents, root/grants, pause/settings and independent hashes after installed
replacement; old endpoint retirement and new extension control/identity;
restart and cleanup. Record any unavailable physical/store gate accurately.

## Restart Checkpoint

Contract recorded. Implementation and installed extension-driven evidence next.
