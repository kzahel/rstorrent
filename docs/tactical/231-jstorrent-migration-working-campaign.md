# Tactical 231: JSTorrent Migration Working Campaign

Status: **Active planning, 2026-09-29.** This is the working tracker for
“ripping the Band-Aid off.” Maintainer direction selects desktop extension
control as the first implementation focus and user-experience continuity as
the North Star. No migration implementation or production rollout has run.

Topic: [`desktop-jstorrent-replacement`](../topics/desktop-jstorrent-replacement.md)

This parent campaign tracks decisions, rehearsals, evidence and follow-up
slices. Each implementation slice has its own bounded tactical. The living
topic owns the source survey and current migration contract; this document
owns the executable work queue and acceptance ledger.

## North Star

An existing JSTorrent user should still open the extension, see their library,
choose a folder, add and manage torrents, leave downloads running, and report
a problem through familiar journeys. Opening the desktop or Android native UI
should show the same backend state. The Rust engine replaces the internals;
users should not have to understand the new process topology.

Preserve useful behavior, not every pixel or legacy implementation detail.
Record unavoidable changes, unsupported features and recovery steps explicitly.
Do not force desktop-window use as the default replacement for the browser UI.

The deliberate exception to legacy workflow continuity is desktop profiles:
there is one desktop library, with no profile picker or multi-profile product.
Migration takes the union of all legacy desktop profiles for the current OS
user. Every authorized extension connection, including connections from
different browser profiles, sees that same library. Legacy source provenance
exists only to make merging, reporting and recovery reliable.

| Configuration | Successor engine/state owner | Extension's job | Native UI's job |
| --- | --- | --- | --- |
| Desktop | One desktop app process and profile | Packaged shared React UI, browser integration, bootstrap and semantic control | Another view of that same owner; OS picker, tray, notifications and update integration remain native |
| Chromebook Android | Android foreground service, Rust engine and SAF roots | Packaged shared React UI, launch/pairing, semantic control and requests for Android folder selection | Compose view, pairing approval, system picker and service lifecycle |
| Chromebook Linux | Separate Crostini backend/profile | Existing launch/focus/setup handoff to the backend-served React tab | Linux backend owns storage and lifetime; it is not the Android library |

Tactical `194` already implements the Chromebook Android ownership shape,
including two attached views and detached transfers. It does not supply desktop
control, legacy state import or all browser media capabilities. Browser
rendering and commands remain JavaScript; peer networking, hashing, scheduling,
SQLite and payload writes remain native. No engine runs in the extension.

## Decision Log

| ID | Decision | State |
| --- | --- | --- |
| D-01 | Familiar extension-first and native-app workflows guide migration acceptance. | Accepted 2026-09-28 |
| D-02 | Implement desktop extension control before the first full migration rehearsal. | Selected 2026-09-28; Tactical `232` plans the slice |
| D-03 | Extension and native views attach to one backend; changing views is not profile takeover. | Accepted architecture |
| D-04 | Reuse the mature React UI and semantic application contract across presentations. | Accepted direction; extract only concrete shared seams |
| D-05 | Preserve JSTorrent production identity/updater trust at graduation; keep incubation identities during development. | Accepted direction; candidate/package mechanics remain open |
| D-06 | Best-effort import preserves valuable intent and rechecks bytes; legacy completion claims are not trusted. | Accepted integrity rule; exact field/cohort mapping remains proposed |
| D-07 | Local diagnostics and bug-report continuity precede a migration cohort. | Required outcome; reporting implementation remains open |
| D-08 | Union all legacy desktop profiles into one library; no successor multi-profile support or source-profile chooser. | Accepted 2026-09-28; duplicate/conflict rules need importer design |
| D-09 | Explicit extension open starts/attaches in background; desktop launch shows its native window. Both views coexist without preferred-UI routing. | Accepted in follow-up discussion |
| D-10 | Automatic reconnect attaches only; explicit Quit must not be undone by browser retries. Desktop bootstrap avoids routine code-entry pairing. | Accepted; protected bootstrap implemented in 232 |

| D-11 | Retain tray/status-bar icon whenever desktop runtime runs, even idle/paused; closing browser only detaches. Native close follows its existing setting. | Accepted 2026-09-29; 234 |
| D-12 | Fresh toolbar/Start/magnet/file intent may relaunch after Quit; automatic restore/retry/worker wake may only attach. | Accepted 2026-09-29; 234 |
| D-13 | Tray Open and OS torrent inputs use native UI; no preferred surface or browser-profile routing. | Accepted 2026-09-29; 234 |

## Work Tracker

| ID | Work | State | Exit evidence / next action |
| --- | --- | --- | --- |
| M-01 | Desktop extension control | Linux/Windows/macOS installed checkpoint complete in [`232`](232-desktop-extension-control.md); full rehearsal partial | Protected bootstrap, native picker, controlled bytes, shared library, singleton and Quit pass. Bounded intent/tray/browser lifecycle passes in [234](234-desktop-user-intent-and-background-lifecycle.md), including 64 reload/worker-stop cycles per guest and prompt view cleanup. Linux Quit/launch overlap is repaired in [235](235-linux-quit-launch-handoff.md). Broader endurance remains separate. |
| M-02 | Legacy cohort inventory and fixtures | [233](233-legacy-desktop-fixture-cohort.md): bounded Linux/Windows v0.2.1 cohort complete | Seven profiles/eight records plus two labeled mutations per platform; closed snapshot/payload oracle passes. Broader releases, browser-store variants and macOS remain unqualified. |
| M-03 | Migration preview, union import and recovery | Not implemented | Consistent snapshots of all source profiles, identity/root mapping, explicit conflict outcomes, retry/crash recovery, held activation, exact recheck and preservation |
| M-04 | Support/report continuity | Required before user cohort | Failure-page report, bounded migration/backend context, familiar voluntary feedback journey and disclosure verification |
| M-05 | Installed desktop replacement rehearsal | Depends on M-01/03/04 | Old installed JSTorrent -> exact candidate -> restart/repair/rollback on macOS, Windows and Linux |
| M-06 | Extension update coordination | [236](236-desktop-compatibility-refusal-and-recovery.md)/[237](237-macos-windows-package-recovery.md): selected three-platform beta matrix passes | Terminal refusal, attach-only recovery, open-page replacement, credential rotation, repair and same-schema rollback pass. [238](238-signed-desktop-channel-recovery.md) passes macOS/Windows signed updating of an older published cohort. Linux signed checks, signed current-source delivery, original legacy pairs and ChromeOS shipment gates remain open. |
| M-07 | Opt-in cohort and graduation | Not scheduled | Explicit supported baseline, source coverage, observation window, stop thresholds and approved production operation |

No entry above implies feature completion from source presence alone. Keep
unrelated release and engine campaigns running under their own ownership.

## Rehearsal A: Extension Controls A Fresh Desktop Backend

This is the first end-to-end rehearsal, before any legacy state import.

1. Record source commit, artifact hashes, OS/architecture, extension identity,
   build versions and protocol capabilities. Use a disposable profile and
   independently generated single-file/multifile payloads with known hashes.
2. Through Machine Control, doctor and claim the selected target; acquire an
   isolated workspace when supported. Record inherited machine state. Install
   the test desktop package and the beta extension in a separately identified
   test browser. Do not use personal profiles or production download roots.
3. With the app stopped, open the extension. Prove one native backend becomes
   ready and the packaged React library opens without a forced desktop window.
   A repeated launch focuses/reuses the presentation rather than duplicating
   the backend or losing the selected library.
4. Choose a folder from the extension, exercise Cancel, then select a root.
   Add a magnet and a local `.torrent`, choose files, and complete a controlled
   transfer. Confirm the selected bytes independently and skipped-file behavior.
5. Open the native window. Confirm the same backend/profile/torrent identities.
   Pause/resume and change file selection/settings from each view; the other
   converges through the ordinary application contract.
   Attach from a second authorized browser profile and confirm the same library,
   without a profile selector, new catalog or takeover prompt.
6. Close the browser view, then exercise the configured desktop background
   behavior. Prove continued work under the chosen policy, reopen the extension,
   and recover current state. Separately test browser restart and service-worker
   suspension; neither should own engine lifetime.
7. Restart the app and simulate stale connection/bootstrap state, incompatible
   versions, picker interruption and connection loss during a command. Recovery
   must avoid duplicate adds, guessed command success and another engine owner.
8. Trigger an actionable connection failure and inspect/copy its bounded report.
   Explicit Quit closes listeners, subscriptions, picker requests and the engine.
   Capture process/handle/queue high-water evidence and verify cleanup.

The recommended fast loop is native Linux builds on a development controller
and installed testing in its matching Linux VM, after guest readiness passes.
The alternate-controller survey resolves Linux and Windows routes with both
guests off; the earlier macOS VM remains the ready fallback. Tactical `232`
records build/test placement. Repeat the installed gate across all three OSes;
a pass on one platform is a milestone, not cross-platform completion.

## Rehearsal B: A Real Legacy Library Crosses The Boundary

Build a fixture cohort through pinned old JSTorrent applications, not only by
handwriting the expected database schema:

| Fixture | Required observation |
| --- | --- |
| Empty and ordinary desktop-only profile | First-run behavior, settings disposition, no invented data |
| Extension-used and multiple legacy profiles with disjoint torrents | Automatic union into one library, every supported source accounted for, no profile-selection UI |
| Duplicate torrents across profiles, including different payload roots | One logical torrent per validated identity; retained payload copies and explicit location conflicts |
| Identical root keys in different profiles and conflicting settings/intent | Source-qualified root mapping, one settings set, deterministic dispositions and no implicit activity/privacy expansion |
| Complete single-file and multifile torrent | Source-offline verification and seeding; exact bytes retained |
| Partial transfer with skipped neighboring files | Selection preserved; verified reuse versus necessary redownload measured |
| Paused torrent and queued/rate-limited work | User intent retained without surprise download/seed activation |
| Magnet with and without cached metadata/selection | Hash/source validation and truthful pending state |
| Multiple roots, removed disk and permission denial | Exact root binding; actionable repair without relocation |
| Duplicate, malformed, oversized or missing source record | Bounded per-record outcome, no silent success or entire-library disappearance |
| Unicode/case-sensitive paths, symlinks and foreign partial state | Safe path semantics and explicit unsupported cases |

For each cohort:

1. Record original user-visible state, expected payload hashes, source versions
   and feature/settings dispositions for all source profiles. Stop all old
   writers and take consistent snapshots, including SQLite WAL semantics.
   Preserve those snapshots unchanged.
2. Run read-only discovery/preview. Compare imported/needs-attention/skipped
   counts and root mappings against the expected union fixture. Account for
   every source record as imported, coalesced, conflicting or skipped with a
   reason, and distinguish source counts from unique destination torrents.
   No engine activation or payload mutation belongs to preview.
3. Import into separate successor state, initially held. Inject interruption
   at snapshot completion, partial catalog commit, check start and activation
   handoff. Retry and restart must converge without duplicate records or loss.
4. Recheck with controlled peers offline; confirm correct byte reuse and the
   inability of legacy bitfields to hide deliberately corrupted content. Then
   permit controlled repair and verify exact final hashes.
5. Open the extension and native UI, confirm the same recovered library and
   test ordinary commands, folder repair, support reporting and restart.
6. Reopen the importer and vary source enumeration order to prove idempotence
   and deterministic union results. Exercise keep-data removal and
   uninstall/reinstall in fixtures without deleting unrelated content.
7. Rehearse rollback: stop/join the successor, restore the old app/profile
   authority, and recheck any payload the successor changed. Explicitly test
   stale legacy UI pages/relaunches attempting to become a second writer.

Importer bounds, snapshot protocol, writer fencing and crash checkpoints must
be finalized in its own tactical before this rehearsal can run. A retained
database or VM snapshot alone is not a rollback contract for external payload.

## Rehearsal C: The Installed Update And Mixed-Version Journey

Use actual old installed packages, including their original app identifiers,
host registration and updater configuration. Qualify the candidate through the
intended signed update path in isolated test infrastructure before production
route changes. Record explicit extension/browser update order and cover:

- old extension + old desktop;
- new extension + old desktop;
- old extension, including an already-open engine page, + new desktop;
- new extension + new desktop; and
- cancelled update, interrupted replacement, registration repair and rollback.

Verify cold launch, native associations, magnet/browser integration, tray,
folder picker, reporting, window/background policy and process cleanup. Track
each platform/package/architecture result separately. Re-run the existing
ChromeOS Android pairing/control journey and Crostini launcher journey before
shipping a shared extension change. Physical Android validation uses Machine
Control's ChromeOS guide and doctor, with project-owned deployment/assertions.

## Evidence And Stop Rules

Each run records fixture and artifact identity, actions, independently observed
outcomes, import/check counts, timings, resource peaks, recovery result and
cleanup. Keep bounded sanitized summaries in the repository; raw profiles,
tokens, personal paths and machine inventory stay out. Delete owned temporary
captures, payload and logs after extracting the needed evidence.

Data loss/corruption, two payload writers, wrong-profile attachment,
unauthorized control or unexplained reported success blocks progression.
Other missing legacy features require a named product disposition rather than
being silently ignored. Public-swarm tests, signing/release operations and
production routing remain separately scoped.

## Restart Checkpoint

- Completed: source survey and draft field mapping in the living topic;
  read-only Machine Control discovery/doctor checks; this campaign plan.
- Completed first control checkpoint: Tactical `232` implements one desktop
  owner/library, protected native bootstrap and authenticated shared React.
  Claimed Linux VM evidence proves cold/warm launch, singleton races/repeated
  clicks, native-root preparation, two-view pause/resume, invalid/stale token
  refusal and Quit without resurrection. Exact source/artifact hashes, tests,
  resource samples and cleanup are recorded in that tactical.
- Current: `232` installed control and `234` bounded desktop lifecycle pass;
  full campaign rehearsal remains partial.
- Installed Linux and Windows follow-up: native bootstrap, picker
  selection/Cancel/detach/Quit, exact private 32-MiB transfer, two-view
  convergence, source-offline restart and registration repair pass. Linux
  also proves transfer while detached. The macOS installed counterpart now passes
  with owned AppKit picking, same-UID bootstrap, serialized cold starts and
  joined menu/tray shutdown; exact commands and artifact hashes are in `232`.
- M-02 preparation: Tactical 233 pins released desktop/extension artifacts,
  generates seven legacy profiles on each guest, and verifies closed SQLite,
  torrent identities, bitfields and payload pieces independently. Its union
  outcomes remain a proposal for importer design, not implemented migration.
- Completed follow-ups: 237 extends selected beta package replacement/repair/
  rollback to macOS and Windows. 238 passes native signed 0.2.301 → 0.2.501
  update/relaunch and Stable catch-up on both; published source predates
  extension control, so it is not signed current-source delivery. 239 passes
  same-owner browser restart and independently verified 32-MiB completion on
  macOS/Windows, plus macOS VM suspend/resume. Native OS sleep is unavailable
  on those guests and remains unqualified.
- Next executable action: repeat 238/239's Linux checks in an unlocked claimed
  guest. A compositor crash recovered through supported maintenance/reboot;
  the inherited five-minute screen lock then blocked native-root preparation.
  No authentication bypass or Linux recovery/update success is claimed. All
  owned state is cleaned up, Linux/Windows are off, macOS is suspended and all
  claims released. Keep physical sleep, longer endurance and signed
  current-source delivery as separate gates.
- Rehearsal A: **partial; three-platform installed control and bounded
  lifecycle checkpoints complete**, with the 239 Linux follow-up still open.
  Rehearsal B: **not run**; C: **partial, selected three-platform beta
  compatibility and two-platform published signed-cohort updates**. No
  migration-ready or rollout-ready claim. Retained 233 fixtures support later
  importer discussion only; no importer or personal migration was started.
- Remaining campaign decisions: supported source formats, union-conflict rules,
  settings and historical counters, mixed-version bridge policy, first cohort
  delivery, compatibility baseline and rollback support duration.
