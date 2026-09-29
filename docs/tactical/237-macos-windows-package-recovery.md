# Tactical 237: macOS And Windows Package Recovery

Status: **Active, 2026-09-29.** Campaign [231](231-jstorrent-migration-working-campaign.md).
Topics: `client-surfaces`, `application-connection-architecture`,
`desktop-jstorrent-replacement`, `beta-release-readiness`.

## Scope And Stopping Condition

Extend 236's selected beta matrix to installed macOS arm64 and Windows x86_64.
Preserve one fresh native-selected controlled library across old/new extension
and desktop pairs, an already-open page, joined rollback and registration repair.
Record exact artifacts, commands, source, architecture/runtime compatibility,
negative results and cleanup. Stop this slice after both platform matrices and
bounded repair pass or a concrete external blocker is recorded. Do not infer
signed automatic updating from local package replacement.

The authorized following slices are signed update-path qualification using
existing published artifacts, then bounded sleep/wake and browser-restart
recovery on macOS/Windows/Linux. Give each its own tactical before execution.
If current signed artifacts predate extension control, record that distinction;
no publication, push, tag, route/key/identity change or fabricated signed
candidate. Importer work and personal-data migration remain excluded.

## Invariants, Ownership And Bounds

One runtime/library; extension pages are detachable connections. Quit joins
owned work, passive reconnect never starts a runtime, explicit intent may.
Tray Open uses the native window. No browser-profile routing preference.
Only same-schema selected beta rollback is claimed. Preserve exact torrent/root
identity and intent; independently hash controlled bytes. Credentials stay in
page memory and must fail after runtime replacement. A missing registration
is repaired by normal desktop launch, never by weakening origin admission.

Reuse 236's bounded scripts and 232/234's platform-native UI/transfer harnesses.
Tests own their browser, fixture processes, scratch paths and exclusive claims.
Close/join before replacing executable files. Record inherited guest paths and
OS registrations before installation; restore them and initial power state.
No engine/protocol/Android contract change is planned. Any discovered runtime
fix needs a local owner/cancellation analysis and proportional builder tests
before installed reruns.

## Source Review And Ordered Evidence

Review 232/234/236, the desktop release runbook and 230, exact Tauri updater
and native-registration code, official [Tauri updater](https://v2.tauri.app/plugin/updater/)
and [Chrome native messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
contracts. Updater signatures remain mandatory; custom test-browser manifests
are explicit fixture setup, distinct from installed standard registration.

1. Freeze old packages and current extension; rebuild candidates incrementally.
   Run builder tests before guest installation, verify architecture/deployment
   target and compare installed hashes.
2. Doctor, claim and supported readiness; inventory inherited state. Use only a
   separately identified Chrome for Testing inside the claimed guest.
3. Native-select a fresh root; add/pause a controlled fixture. Exercise old/old,
   old/new with its document still open, new/new and new/old rollback. Observe
   unchanged library and bytes, new runtime identity, refused stale authority.
4. Delete only owned native registration/host after joined Quit; ordinary launch
   repairs it. Test terminal incompatibility and attach-only recovery.
5. Quit, check passive no-resurrection, reap owned browser/helpers, restore
   inherited installation/registration/state, remove scratch, park and release.

## Restart Checkpoint

Clean source `cee8ff04` includes Linux repair `26c50e2f` and final extension
compatibility handling. Retained macOS old executable matches 234's SHA-256
`e3a3fc17ce2aed822f118ffca61f784d886def7816bfbc341b27c2007b33ff8e`.
Current extension matches 236's `a21fb6e52ee720a83987d0b5094a60e1a338014c024cdaa977cbcadd47538533`.
macOS doctor reports an available suspended guest and unlocked host resume
precondition; no guest mutation yet. Recheck Windows availability independently.
