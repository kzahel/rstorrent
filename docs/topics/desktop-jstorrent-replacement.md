# Desktop JSTorrent Replacement

Topic: `desktop-jstorrent-replacement`

Tactical [237](../tactical/237-macos-windows-package-recovery.md) completes
macOS/Windows selected beta replacement, rollback and native registration repair,
joining Linux's 236 matrix. [238](../tactical/238-signed-desktop-channel-recovery.md)
proves native signed 0.2.301 → 0.2.501 updating and return-to-Stable without
downgrade on macOS/Windows; that published cohort predates extension control.
[239](../tactical/239-desktop-suspension-browser-recovery.md) proves same-runtime
browser restart and independently verified 32-MiB completion on both platforms,
plus macOS VM suspend/resume. Native OS sleep remains unqualified. Linux's new
238/239 checks stop at the inherited screen lock; no authentication bypass.
Owned guest state is cleaned up and claims released. Next: repeat those Linux
checks in an unlocked session, then qualify signed current-source delivery
when separately available. Broader endurance and importer work remain separate;
no importer has started.

Tactical [`236`](../tactical/236-desktop-compatibility-refusal-and-recovery.md)
qualifies selected Linux beta old/new extension/runtime pairs, an already-open
successor UI across replacement, credential rotation and same-schema rollback.
Incompatibility offers terminal guidance and attach-only recovery. This is an
initial Rehearsal C checkpoint, extended to macOS/Windows in 237. Original
legacy pairs, signed current-source delivery and ChromeOS shipment gates remain
open.

Tactical [`234`](../tactical/234-desktop-user-intent-and-background-lifecycle.md)
owns the accepted desktop lifetime follow-up: retain the tray/status-bar icon
while the runtime runs, including idle/paused states; browser closure only
detaches. Fresh toolbar, Start, or OS torrent-input intent may relaunch after
Quit; page restoration, worker wake and retries only attach. Tray Open and
OS magnet/file delivery always use the native window. No preferred surface,
browser-profile routing, icon-hiding or automatic completion shutdown is added.
Installed macOS/Windows/Linux debug builds pass the bounded lifecycle
checkpoint: browser closure during a verified transfer, 64 reload/worker-stop
cycles, actual tab discard, explicit relaunch and passive no-resurrection.
Desktop disconnect retires only that client's abandoned view sets before
admitting a replacement; native views and other transports retain their owners.
Broader endurance and installer/update compatibility remain open.

Tactical [232](../tactical/232-desktop-extension-control.md) records installed
Linux, Windows and macOS extension-control checkpoints: native picking,
controlled bytes, one runtime/library, two-view convergence, registration repair
and joined Quit in claimed guests. Tactical
[233](../tactical/233-legacy-desktop-fixture-cohort.md) separately records a
pinned released legacy fixture cohort on Linux/Windows only. Seven generated
profiles per fixture platform cover conflicting roots/intent, partial/complete
bytes, pending magnets and selective Unicode content, with independent closed
SQLite/identity/piece validation. Union rules remain proposed importer design;
no importer, production identity change or personal migration has run.

Status: **Three-platform installed control checkpoint complete, 2026-09-29;
migration campaign active.** Tactical
[`231`](../tactical/231-jstorrent-migration-working-campaign.md) tracks broader
browser lifecycle/endurance and update compatibility, separately from bounded
importer design, production identity and rollout gates.

## Outcome And Scope

Replace JSTorrent's desktop implementation through its existing product
identity and updater trust, preserve useful torrent intent and payload, retain
an effective support/reporting journey, and coordinate the browser extension's
change from engine owner to native-backend presentation.

The existing [product migration topic](product-surfaces-and-migration.md) owns
the accepted backend/presentation architecture. This topic owns the desktop
replacement campaign, source cohorts, acceptance gates, and rollout questions.
[Android replacement](android-jstorrent-replacement.md) remains independent,
but a shared production extension update must preserve its supported journeys.
RSTorrent incubation and JSTorrent production identities remain separate
until an explicitly selected graduation candidate.

Preserve the extension-first experience: open the familiar browser UI, see the
library, choose folders, manage torrents and report problems. Native desktop
and Android views attach to the same respective backend. Users should not need
to understand the move from a JavaScript engine to a native engine. This is
workflow continuity rather than a promise of identical pixels or every legacy
feature; missing behavior requires an explicit disposition.

Follow-up direction on 2026-09-28 selects **one desktop library**, with no
product-level profile creation, selection or switching. Migration combines
all discovered legacy desktop profiles belonging to the current OS user into
that library. Browser profiles and extension installations are connections to
the desktop runtime, not separate data owners. This deliberately retires the
legacy multi-profile experience while preserving its useful torrent data.

## Source Survey

Surveyed RSTorrent HEAD `7a4d7730920f84ed0c02374506b0cd4af1f26f2d`, JSTorrent
HEAD `25e4b701433fd815398ba89526546f5e4f072e3f`, and Machine Control HEAD
`f7dbe902080a793592b2f8c1efe3485fd2162efc`. Working trees contain unrelated
changes; this is a source survey, not qualification of immutable packages.
JSTorrent's checked-in desktop version is `0.2.1` and extension version is
`1.1.1`; those values do not prove which versions the installed audience uses.
Pin released source/artifact pairs before building the migration fixture set.

Inspected JSTorrent paths relative to that checkout:

| Paths | Finding and consequence |
| --- | --- |
| `desktop/README.md`, `extension/README.md`, `docs/contracts/native-host-contract.md` | The foreground web UI owns the TypeScript engine. The host owns profile bootstrap, roots and KV, and starts the IO daemon. Existing profile takeover is real ownership transfer, unlike the intended detachable successor views. |
| `desktop/common/src/lib.rs`, `desktop/host/src/{main,rpc,kv_store}.rs` | `jstorrent-native/rpc-info.json` holds profile/root discovery; `profiles/<id>/data.db` holds a SQLite `kv(key,value)` table in WAL mode. Discovery also contains runtime tokens and endpoints: it is sensitive input, not a support attachment. |
| `packages/client/src/host/{host-channel-session-store,tauri-channel,host-channel-config-hub}.ts` | Session keys have a `session:` prefix; settings use `config:`. JSON is serialized in the host store; binary metainfo/info dictionaries are base64 strings inside JSON values. An importer must decode the actual layers, not guess from key names. |
| `packages/engine/src/core/session-persistence.ts` | The version-2 index preserves info hash, source, magnet and added time. Per-torrent state includes root key, active/stopped intent, queue position, force-active, file selection and pre-metadata magnet selection, alongside progress claims and counters. |
| `extension/src/sw.ts`, `extension/src/lib/kv-handlers.ts` | Desktop KV routes through the native host. Extension-only preferences/authentication stay in browser storage, and a browser-local fallback exists. A desktop DB import does not cover every historical browser-only source. |
| `packages/client/src/hooks/useSystemBridge.ts` | Bug reporting opens JSTorrent's feedback page with environment/backend/version/status, engine context and daemon uptime; usage metrics are extension-only on this path. Preserving this journey requires more than a generic issue link. |
| `desktop/host/tests/profile_scenarios.rs`, `packages/engine/test/core/session-persistence.test.ts` | Existing real-host/profile and deterministic persistence harnesses provide starting points for source fixture generation; they were inspected, not executed in this survey. |

RSTorrent already has direct final-path storage and conservative checking,
typed application commands and recoverable views, background desktop lifetime,
external torrent intake, notifications, product-state controls, and packaged
extension React assets used by ChromeOS Android. Relevant owners are
[direct storage](direct-filesystem-storage.md),
[persistence](client-persistence.md),
[connections](application-connection-architecture.md), and
[runtime compositions](runtime-configurations-and-headless-deployment.md).

Two gaps matter immediately:

- Tactical `166`'s desktop native host only identifies compatibility and
  launches RSTorrent. It does not connect the desktop extension to the Rust
  application. The existing production extension still uses the legacy host.
- Tactical `216`'s support JSON is intentionally below 2 KiB and contains only
  allowlisted build/update facts. It does not capture migration, engine or root
  failures. Tactical `208`'s richer feedback transmission remains gated on
  hosted-page qualification. Local diagnostics and remote transmission are
  separate capabilities.

Use the current [release ledger](beta-release-readiness.md), especially its
Tactical `230` checkpoint, rather than older README package summaries.
Installed cross-channel replacement/relaunch evidence remains open there.
That evidence is also distinct from replacing an installed JSTorrent package.

## Proposed First Data Contract

Import the union of all discovered legacy desktop profiles into one fresh
successor library from consistent, retained source snapshots. Do not modify
legacy databases in place or reuse their schemas as RSTorrent's store. There
is no source-profile chooser and no successor multi-profile support. Inventory
every source and report unreadable or unsupported profiles rather than silently
omitting them. This scope does not merge other OS users, machines, Android or
Crostini libraries.

Union means one logical torrent per validated torrent identity, not one row
per legacy profile and not deduplication by display name. Preserve all distinct
torrents and useful root bindings. Legacy profile/root keys are source-local:
resolve them with source provenance before assigning successor identifiers.
Provenance is migration/recovery bookkeeping, not a new runtime profile model.

Exact conflict policy is an importer-design checkpoint. Matching duplicates
can coalesce automatically; the same torrent at different payload locations
must preserve those files and surface a location conflict, without copying,
deleting or silently choosing by modification time. Hold ambiguous records for
repair in the single library. Conflicting file selection, run intent, queue
order, settings and historical counters need deterministic documented rules;
do not sum duplicate history or enable broader activity/transmission implicitly.
Keep one resulting settings set. Ordinary nonconflicting imports should require
no profile-related decision from the user. Retry keys must account for every
source contributing to a merged record.

| Legacy data | Proposed treatment |
| --- | --- |
| Torrent file, original magnet, cached info dictionary | Validate bounds, encoding, metainfo and hash agreement; preserve useful source/tracker intent. Report unsupported or incomplete records individually. |
| Root reference and path | Resolve within each source profile, then map into the single library's root registry. Validate actual destination layout/access and require repair for missing/removable or conflicting locations. Never silently redirect to a new default. |
| Active/stopped intent, Normal/Skip selection, pending magnet selection | Preserve intent, initially hold imported work from downloading/seeding, and release only after ownership handoff and ordinary checking. Define pending-metadata behavior explicitly. |
| Added time and queue ordering | Preserve where representable, with deterministic normalization and an explicit disposition otherwise. |
| Settings | Closed mapping for equivalent rate, queue, network, background and notification policy; report changed or unsupported semantics. Do not copy arbitrary settings wholesale. |
| Piece bitfields and completion flags | Never establish verified content. Check retained bytes through the Rust engine. |
| Upload/download history, ratio policy, force-active | Explicit decision needed. Historical values must not silently change seeding-goal admission or masquerade as newly measured engine counters. |
| Tokens, peer cache, DHT state, runtime ports/PIDs, browser pairings | Do not import authority or transient engine state. Establish new connections and authorization. |
| Installation ID, statistics preference and counters | Separate product-state decision. Preserve the user's privacy choice; no implicit widening of transmission or reuse of browser pairing/telemetry authority. |
| Search plugins, browser-local preferences, old packaged-app data | Inventory and give a visible disposition; not an implicit requirement to migrate every historical architecture. |

Preserve payload in place when exact path compatibility is proven. Test
single-file and folder roots, partially downloaded and oversized files,
cross-file pieces with skipped neighbors, Unicode/case collisions, symlinks,
unavailable disks and duplicate identities. Foreign partial-file formats are
not trusted resume evidence; quantify any bounded redownload needed.

Proposed recovery model: discover -> snapshot -> validate/preview -> import
held records -> verify -> explicit activation. Persist enough phase and
per-record outcome information for retries to be idempotent. Partial success
must be visible, with counts for imported, needs attention and skipped records.
Bound source size, decoded metainfo, record counts, allocation, diagnostic
retention and concurrent check work before implementation.

Quiesce the legacy engine and its writers before the actual handoff. A SQLite
file copy without its live WAL is not a consistent backup. Choose a supported
snapshot procedure and fence restarts: unrelated profile locks do not prevent
two different engines from writing the same payload paths. Retaining a legacy
database alone does not make rollback safe after successor downloads or
deletions. A rollback procedure must stop the new writer and recheck payload
under the restored owner; test that explicitly.

## Extension Transition

Target: one desktop Rust application/profile/engine, with an embedded Tauri
view and a full extension view attaching to that same owner. Opening either
view must not perform normal profile takeover or create another engine.
Every authorized browser profile/extension connection attaches to this same
desktop library. Backend/profile identity may remain internal protocol facts
for detecting stale or wrong connections; they do not expose a profile picker
or permit an extension to create or select another desktop data owner.

Launch routing is explicit rather than preference-driven: clicking the extension
opens its browser view and ensures the runtime is running in the background;
ordinary desktop launch opens its native window. Both may coexist. There is no
preferred-UI setting or takeover prompt. Desktop bootstrap should authenticate
automatically through the approved installed native integration, without
routine code-entry pairing. Automatic reconnect only attaches; it cannot launch
a stopped runtime or undo explicit Quit. Tactical `232` owns the protected
bootstrap design and first library/control checkpoint.

Desktop extension control is the selected first implementation focus, before
the full migration rehearsal. This supersedes the initial standalone-first
recommendation. The first rehearsal uses fresh test state; the subsequent
legacy-state and installed-update rehearsals use that proven control path.
A launcher-only extension is not the intended successor desktop experience.

ChromeOS Android already implements this ownership pattern in Tactical `194`:
the extension hosts React and sends semantic commands while the Android service
owns the engine, profile and SAF grants. Compose is another view. Desktop should
reuse the presentation and application contract with its own authenticated
bootstrap and OS capabilities. Crostini remains a distinct backend with its
existing backend-served UI handoff. Browser media and other platform differences
must stay visible in the parity ledger rather than being assumed equivalent.

The desktop transport tactical must settle native-host discovery/bootstrap,
same-user admission and exact extension identity, protocol/version negotiation,
credential scope/revocation, bounded frames and queues, and cold windowless
launch followed by optional webview creation. Reuse the semantic application
contract and proven extension presentation; do not recreate the raw IO daemon.
Compare native-messaging delivery with an authenticated local control channel
before selecting the transport. ChromeOS Android's transport is useful evidence,
not proof of desktop authentication or lifecycle behavior.

Desktop and store updates arrive independently. Exercise all four combinations:

| Extension | Desktop | Required disposition |
| --- | --- | --- |
| Legacy | Legacy | Continues working during the transition. |
| Successor-capable | Legacy | Bounded legacy compatibility or clear upgrade path, selected before rollout. |
| Legacy | Successor | Safe upgrade/handoff guidance; never silently start a legacy writer against migrated payload. |
| Successor-capable | Successor | One native owner, both views converge, detach/reconnect and cold launch work. |

Also test already-open legacy UI pages, dormant service workers, multiple
browser profiles, native-host registration repair, cancelled updates and
rollback. A production extension rollout cannot accidentally replace the
ChromeOS Android/Crostini journey merely because desktop is ready. Inventory
magnet interception, context menus, local torrent intake and other browser
integrations separately from rendering the torrent table.

## Support And Observability Gate

Before inviting a migration cohort, provide a report action from both ordinary
settings and failed-migration/startup recovery. A broken application service
must not be required to explain why that service could not open its profile.

Proposed bounded local report: exact build/package/channel, source format and
version when known, migration phase and outcome counts, closed failure reasons,
recheck progress/duration, root-health categories, backend/control versions,
owner conflicts, interrupted-recovery state and diagnostic loss counts.
Use a migration-attempt correlation value scoped to the local operation; do
not turn it into another installation tracking ID. Keep richer paths, torrent
names/hashes, tracker URLs and raw errors out of the default export.

Preserve preview/copy/download and the familiar voluntary feedback journey.
Design any optional recent structured-event attachment with explicit bounds,
redaction and preview; logs are not the application state API. Automatic
uploads, a third-party crash SDK and continuous analytics are not assumed.
Decide separately what startup crash evidence can be retained locally.

Maintain a migration issue ledger with reproducible source fixture, phase,
failure category, repair action and tested resolution. Local per-import counts
are immediately useful; aggregate rollout metrics require an explicit
disclosure/collection decision. Existing updater counts cannot establish a
successful migration.

## Proposed Work Sequence And Acceptance

1. **Desktop extension control.** Tactical `232` first proves one native owner
   through cold launch, two simultaneous views, detach, reconnect, Quit, native
   picker, controlled transfer and connection recovery, without legacy import.
2. **Inventory and fixture rehearsal.** Pin supported released JSTorrent
   artifacts and source formats, generate controlled legacy profiles through
   their normal writers, and produce a read-only discovery/preview report.
   Cover desktop-only and extension-owned profiles. Stop with a field mapping,
   unsupported-feature dispositions and reproducible fixtures, without any
   production state change.
3. **Safe import and support.** Implement the bounded snapshot/import/recovery
   slice and useful local migration reporting. Prove retry, corruption,
   interrupted phases, source preservation, unavailable roots, cancellation,
   held activation, exact hashes and source-offline re-seeding. Any engine or
   persistence changes require the source-first pinned-libtorrent code/test
   review and proportional Android semantics/build evidence in that tactical.
4. **Installed JSTorrent replacement rehearsal.** On macOS, Windows and Linux,
   install an actual old package, create realistic state, replace through the
   intended updater/package path, reopen, report a controlled failure, repair,
   restart and rehearse rollback. Qualify branding, app identifiers, signing,
   updater trust, native-host registration, file associations and cleanup as
   one installed product. A renamed development binary is insufficient.
5. **Opt-in cohort, then production decision.** Choose a candidate, support
   baseline, source-version coverage, soak duration, failure thresholds and
   rollout stop mechanism from the rehearsals. Freeze the compatibility
   promise before migrating production users. Incubation's disposable-state
   policy must not silently govern their newly imported catalog.

Each step needs its own bounded tactical before implementation. This draft
does not promise every legacy feature or select a public version/date. No push,
store upload, production update-route change or signing-key movement follows
from planning authorization.

Tactical [`231`](../tactical/231-jstorrent-migration-working-campaign.md) owns
the detailed fresh-control, legacy-library and installed-update rehearsal
scripts, fixture matrix, decision log and evidence tracker.

## Machine Control Evidence And Test Ownership

Use `~/code/machine-control/bin/machine-control` and the applicable platform
guides. Keep fixture generation, migration assertions and bounded reports in
RSTorrent; Machine Control owns target lifecycle, claims, transport and native
UI control. Query capabilities before assuming isolated workspaces or rollback
snapshots exist. Keep target identities and credentials in private inventory.

Read-only checks on 2026-09-28: `targets` resolves desktop adapters; macOS
`target doctor` reports ready administration, unlocked desktop, semantics,
capture and input. Windows reports unresolved private target classification
and unavailable administration/desktop; Linux reports unknown power and
unavailable administration/desktop. These are current-route observations, not
product failures or claims that other controllers cannot reach those systems.
No VM was booted, repaired, claimed, installed into or modified by this survey.

The later same-day alternate-controller survey resolves native Linux and
Windows VM routes with both guests powered off, plus a Linux builder with Rust,
Node, GTK/WebKit dependencies and retained build cache. The recommended fast loop
is native Linux builds and a matching claimed Linux VM, conditional on guest
readiness; the previously ready macOS VM remains a fallback and separate gate.
This supersedes the initial macOS-first recommendation without claiming the
alternate guests are already ready. Tactical `232` owns build/test placement.
Resolve each route's readiness before its campaign. Retain inherited power
state and release claims/workspaces with joined test-process cleanup. Use
deterministic fixtures and command-driven checks first; native UI evidence is
needed for installers, associations, picker, tray, feedback and launch routing.
Use a separately identified test browser for automated extension fixtures.

No migration test ran in this initial survey. Documentation validation is
`git diff --check`; the read-only Machine Control results above are readiness
evidence only.

## Discussion Decisions

- Desktop extension control is selected first; its exact transport and
  lifecycle decisions are tracked in Tactical `232`.
- All legacy desktop profiles are combined into one library. Which released
  source formats are supported first, and what deterministic rules settle
  duplicate locations and conflicting settings/intent?
- Which legacy features/settings/history must carry forward, and which may
  be reset with visible explanation? Include ratio goals and privacy choices.
- Should the first user cohort be an opt-in JSTorrent update lane or a separate
  RSTorrent import rehearsal? Same-payload coexistence needs a writer handoff
  in either case.
- What support baseline and rollback promise begins with the first migrated
  production user?
