# Tactical 238: Signed Desktop Channel Recovery

Status: **Active; Linux installed checks blocked, 2026-09-29.** Campaign 231; follow-up to 230/237.
Topics: `beta-release-readiness`, `client-surfaces`, `desktop-jstorrent-replacement`.

## Scope And Evidence

Qualify selected existing signed Stable/Latest packages through installed
native update UI on macOS arm64, Windows x86_64 and Linux x86_64 where the
package lane supports it. Freeze published artifact hashes and source, verify
platform trust/signatures, select Latest explicitly, install/relaunch and
return to Stable without downgrade. Prove retained controlled external bytes;
record the package's catalog policy separately. Keep existing production
routes/keys read-only. No push, workflow dispatch, publication, tag, release,
key change, importer or personal migration.

Initial inventory included Latest 0.2.401 (80b04468); live channel discovery
offers 0.2.501 (7a4d7730), also before desktop extension control.
Therefore its installed updater evidence cannot qualify signed delivery of
232–237. Record incompatibility honestly and do not replace the newer beta's
controlled profile with the older published cohort. Use a separate fresh
controlled signed-cohort library and restore the beta cohort afterward.

## Ownership, Limits And Stopping Condition

Native update owner retains one signed candidate, invalidates it on channel
change and rejects unsupported package lanes. Existing joined restart owns
shutdown; no background updater/task added. Read official Tauri updater docs,
230 and the release runbook, current release assets and native source before
finalizing cases. Builder release source/tests precede installed evidence.

Stop after available signed platform checks, cancel/failure outcomes supported
by the existing UI, exact evidence and inherited-state cleanup. A missing
signed current-source candidate is an explicit remaining gate, not authority
to publish one. Guest policy or unavailable power states are recorded as
limitations. Retain no test credentials, servers, registrations or claims.

## macOS Signed Channel Evidence

Use a separate fresh signed-cohort profile/root; move the newer controlled beta
profile/app aside while stopped. Never open it with the older signed cohort.
The published 0.2.301 DMG SHA-256 is
`de3af97e66257001c77f4022e7fe3e43f8f1be68288bcb714e800ab49ee17139`
(source `30523da4b5bef2b3609d8491585a651733557c1f`). Read-only builder inspection
verifies arm64, minimum macOS 13.0, code signature and Gatekeeper acceptance.
Guest is macOS 26.6.2 arm64. A native folder dialog selects the controlled root.
Usage reporting is opted out through the native UI.

Selecting Latest in native About & updates discovers **0.2.501**, not the
initially inventoried 0.2.401. Re-inventory the already-published release:
source `7a4d7730920f84ed0c02374506b0cd4af1f26f2d`, published 2026-09-28.
`latest.json` SHA-256 is
`4b01c16c085869d20b327bd1c6e99a230953eec23c65aa7b01230887a24b5276`;
aarch64 updater archive is
`ddeac9ded764e2a0b7862e03b39142994a70e973a90420acddab0ba6c67720fa`.
The native **Install and restart** action downloads/verifies/installs and
relaunches 0.2.501 through the unchanged signed updater route. Its executable
SHA-256 `ee3f94cc839486f4f403f9e621877ae72918283cca4d4472c0e1bd4aa3bfe6f1`
matches the independently downloaded published archive. `codesign --verify
--deep --strict` passes; native About identifies the exact source/target.

The native Downloads view retains the controlled root; its external sentinel
retains SHA-256
`8981c074565ee7493567c1fa4a8d2913ba442bd308e3cddb20abd159c8b46974`.
Switching to Stable displays **Waiting for Stable to catch up**, preserves
0.2.501 and offers no downgrade. Native select interaction must retain current
guest focus while committing its menu selection; reactivating the application
between arrow/Enter can dismiss the popup without changing the channel.

This is successful published-cohort signed updating, not signed delivery of
232–239. No cancellation control is exposed during installation; cancellation
or interrupted replacement is not claimed from this happy-path run. Channel
candidate invalidation remains separately covered by 230's scripted tests.
No production route, identity, key, release or workflow was changed.

Real native Quit and owned browser shutdown precede cleanup. Owned app bundles,
profiles, manifests, browser support roots and payload are removed. The saved
LaunchServices dictionary is unchanged. The guest is returned to its initial
suspended state and its exclusive claim is released. Windows evidence follows; Linux remains unqualified.

## Windows Signed Channel Evidence

The 0.2.301 NSIS installer SHA-256
`3eaba45249c02f35aa5cee1d0da13464a5a548c05fd39b4f7cecaeda7f47c9bf`
matches its published digest and `Get-AuthenticodeSignature` reports
**Signature verified**. Install into a separate fresh signed-cohort profile,
opt out of usage reporting, and select a fresh root through the native folder
UI. Select Latest, observe 0.2.501, and invoke **Install and restart**.
The single relaunched installed executable reports 0.2.501 and verifies its
Authenticode signature. Published 0.2.501 NSIS SHA-256 is
`17c43294a3ea6a100c743e8761ffb8a0021c2d153acc24c93c85963c08f5f9e9`.
Native Downloads retains the root and the independently read external sentinel
matches the macOS sentinel hash above. Stable selection displays **Waiting for
Stable to catch up** and keeps 0.2.501. No route/key/publication change.

Common Windows desktop snapshots may be rooted at the full desktop; depth 20
can omit nested About controls. Enumerate the actual RSTorrent HWND and use the
supported target-resident `testbed -- control` snapshot with that HWND, then
invoke its generation-bound references. No host browser/UI or global input is
used. Signature/version/hash and filesystem receipts are independent oracles.

Final native tray Quit precedes restoration: all 319 inherited file hashes,
eight registry keys/absence states and 1,482 builder source paths verify.
Task-owned installation, profile and browser artifacts are removed; caches are
retained. Windows is returned to its initial off state and its claim released.
Linux signed execution remains blocked by the inherited screen lock, not a
passed lane. See 239 for the compositor recovery, lock boundary and cleanup.
Its task-only AppImage/FUSE preparation does not establish updater execution.
Next: claim an unlocked Linux guest, use a fresh native-selected signed-cohort
root, and repeat native Latest install/relaunch and Stable catch-up checks.
Do not publish a current-source candidate under this tactical.
