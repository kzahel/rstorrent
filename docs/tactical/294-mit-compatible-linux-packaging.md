# Tactical 294: MIT-compatible Linux packaging

Status: Active, 2026-10-10. User-directed beneath finish-line259/293.
Owners: product-direction, beta-release-readiness and desktop-jstorrent-replacement.

## Scope and stopping condition

Retain MIT and remove the GPL-only runtime library dependencies confirmed in
both frozen AppImages. Prefer a small native build change that preserves the
existing Linux tray and WebKit application. Prepare and verify local inputs
before requiring a new signed Linux candidate. Existing2b83 receipts remain
exact-byte historical evidence.

Stop the implementation slice at verified native source builds, accurate custom
source/patch provenance, package guards and a focused Linux runtime check.
Original-signed packages, source delivery and publication remain under259/293.
If preserving the tray needs a new backend, substantial fork or new dependency,
present that tradeoff before expanding best-effort Linux scope.

Non-goals: engine/JNI/API changes, WebKit replacement, older-distro backports,
DEB/RPM delivery, iOS, changing MIT to GPL, signatures or publication.

## Sources and bounded implementation

Separate062 verifies174 x64/173 ARM component hashes and173/172 ELF dependency
sets. The TIFF loader requires TIFF, which requires GPL-2+ JBIG. The tray's
frozen libappindicator-sys0.9.0 loader selects Ayatana appindicator, which requires
GPL-3 indicator. These are library dependencies rather than GPL source tools.

Exact descriptor-bound libayatana-appindicator0.5.93 source is inspected:
`CMakeLists.txt` declares the indicator dependency; `src/app-indicator.c` includes
`indicator-desktop-shortcuts.h` and calls its desktop-shortcut API. That file
declares LGPL2.1/LGPL3 alternatives. Investigate disabling the unused desktop-file
shortcut path while preserving ABI, tray status/icon/menu and normal controls.
The current CMake has no such build option; no existing flag is claimed.

Rebuild exact selected TIFF with optional JBIG disabled. Inspect original Debian
changes and compiler/configuration paths before choosing flags. GPL build tools
are acceptable; do not confuse them with GPL-only linked runtime code.

Use original archives and independently authored bounded patches. Record source
and patch hashes, flags and outputs; preserve original copyrights and LGPL
source/patch obligations. Attribution must identify custom builds rather than
falsely identifying unchanged distro binaries. Never remove notices to hide a
dependency or delete a required library without rebuilding its consumer.

## Validation, ownership and next action

Prepare source/patch/compiler checks before booting a testbed. Machine Control
owns transport, claim, power and lifecycle; this repo owns build recipes, ELF/ABI
checks, packaging and assertions. Bound and observably terminate the runner,
clean only owned roots/mounts, power off and release on every exit. Headless
builds use no product profiles; the later tray smoke owns its product scope.

Verify changed libraries' provenance, required exported ABI, DT_NEEDED without
targeted GPL libraries and native closure. Check ordinary TIFF formats and tray
menus/events. Add meaningful packaging refusals so the dependencies cannot
silently return. Do not reject GPL text from unrelated tools/tests in a notice.

Next: inspect exact source packages, prepare the smallest patch/build recipes
and verifier, then run one bounded x64 prototype before changing production
packaging. ARM and signed scope remain explicit. The MIT decision resolves the
license-choice question; it does not authorize push or publication.
