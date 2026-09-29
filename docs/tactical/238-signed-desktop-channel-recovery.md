# Tactical 238: Signed Desktop Channel Recovery

Status: **Planned, 2026-09-29.** Campaign 231; follow-up to 230/237.
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

Published Latest 0.2.401 is source 80b04468, before desktop extension control.
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
