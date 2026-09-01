# Android JSTorrent Replacement Readiness

Topic: `android-jstorrent-replacement`

Status: **Active as of 2026-09-01.** This is the authoritative readiness and
feature-disposition ledger for eventually shipping the first-party Rust
Android product as a normal update to the current JSTorrent Android
application. It does not authorize a Google Play release, signing-key use,
production-extension publication, or implementation without bounded
tacticals.

The initial audit compares RSTorrent commit
`fe05d74a7f7aa9e508bb941dc758c8196a2c8864` with the local JSTorrent checkout
at `25e4b701433fd815398ba89526546f5e4f072e3f`. Re-audit both products before a
replacement candidate because this topic records a moving product boundary,
not permanent parity with that one revision.

Ready Tactical
[`207`](../tactical/207-android-safe-reset-and-clear-data.md) re-opened the
reset, clear, exact deletion, removal-completion, settings, SAF-root, pairing,
metrics, and preference owners against RSTorrent
`c36f3a7c2e5e1adc64a4e3da3942269d4239b62a`, clean JSTorrent sibling revision
`0cad4dacf540f5be42ee53c4f1e1da27aa1b3685`, and pinned libtorrent `2.0.13` at
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. Its source-first record owns the
exact paths, adopted behavior, deliberate differences, and validation matrix.

## Scope And Ownership

This topic owns:

- the stronger readiness bar for updating installed `com.jstorrent.app`
  users, distinct from launching an independent RSTorrent beta;
- the inventory and explicit disposition of current JSTorrent Android
  capabilities;
- Android package, signing, Play, legacy-state, SAF-grant, and payload-safety
  handoff requirements;
- coordinated migration of the production ChromeOS extension/Android
  companion journey; and
- the ordered bounded tacticals and evidence required before replacement.

[`beta-release-readiness.md`](beta-release-readiness.md) continues to own the
independent RSTorrent beta lanes and their signed distribution gates.
[`product-surfaces-and-migration.md`](product-surfaces-and-migration.md) owns
the broader cross-product graduation direction.
[`client-surfaces.md`](client-surfaces.md) owns Android presentation and
lifecycle truth, while [`capability-readiness.md`](capability-readiness.md)
owns the cross-cutting capability scoreboard. Implementing tacticals own exact
contracts, limits, source findings, tests, and execution evidence.

This topic does not require a copy of JSTorrent's QuickJS engine, raw I/O
daemon, service split, or internal storage representation. It does not make
desktop or iOS graduation part of Android replacement. Engine-wide gaps enter
this ledger only when a current Android promise or ordinary replacement
journey depends on them.

## Product Outcome

The desired outcome is a normal Google Play update from a supported current
JSTorrent Android release to the Rust implementation, retaining JSTorrent
branding and the installed application audience. The replacement must:

- retain `com.jstorrent.app`, Play signing continuity, and a monotonic
  `versionCode` when shipped through the existing listing;
- preserve external payload unless the user explicitly requests deletion;
- migrate useful torrent intent, settings, roots, and pairing state only where
  a bounded importer can do so safely, and otherwise use a truthful reset or
  reauthorization flow;
- never convert legacy completion or resume claims into verified content
  without the Rust engine's ordinary integrity checks;
- keep the standalone Android and supported ChromeOS extension journeys
  usable across the rollout; and
- either implement, deliberately retire, or clearly disclose every current
  user-visible JSTorrent capability before the candidate is approved.

The Kotlin namespace and internal class names need not equal the application
ID. Public identifiers, deep links, notification channels, provider
authorities, extension metadata, and store-facing names must nevertheless be
derived from one reviewed production identity rather than today's provisional
`org.rstorrent.bootstrap` values.

## Definition Of Replacement Ready

Android replacement is ready only when:

1. every open **Required** gate below is complete with its proportional
   deterministic, emulator, physical-device, and signed-update evidence;
2. every **Disposition required** capability has an accepted implement,
   retire, or defer-and-disclose decision, with implementation evidence where
   selected;
3. a production-equivalent old JSTorrent fixture upgrades through the actual
   signed Play lane without payload loss, false verified state, or an unusable
   standalone/ChromeOS journey;
4. fresh install, upgrade, interrupted migration, missing/revoked root,
   process death, reboot, uninstall, and explicit data deletion have bounded
   outcomes; and
5. store declarations, privacy/support text, screenshots, permissions, and
   foreground-service behavior describe the candidate exactly.

Feature-for-feature identity is not required. Unclassified regressions are
not acceptable: every deliberate difference must have an owner and a visible
product decision.

## Current Implemented Baseline

The Rust Android product already provides the hard architectural foundation:

- one in-process Rust application service and engine with a generated UniFFI
  boundary and one durable profile owner;
- the maintained Material 3 Library, torrent detail, Files, Peers, Trackers,
  Pieces, Swarm, Disk, Speed, DHT, Logs, and Settings presentations;
- in-application magnet and local `.torrent` intake, queue and torrent
  actions, High/Normal/Skip file intent, completed-file open, and guarded
  keep/delete removal;
- bounded external `magnet:` and cross-package `content://` `.torrent`
  activation through the same activity, service, root, and application owner;
- retained multi-root SAF selection, current/default future binding,
  grant-loss repair, direct storage, restart/recheck, upload, and exact
  cleanup;
- peer, upload-slot, active-download, listener, UPnP, IPv6, encryption, and
  session/per-torrent transfer-limit settings;
- activity/process recovery and a foreground service; and
- controlled Android/ChromeOS transfer evidence across both packaged ABIs.

Completed Tactical
[`165`](../tactical/165-cross-platform-active-download-sleep-inhibition.md)
adds default-on active-work sleep inhibition with one service-owned partial
CPU wake lock. It deliberately removes JSTorrent's Wi-Fi lock because the
deprecated Android mode no longer supplies a truthful screen-off guarantee.

Completed Tactical
[`194`](../tactical/194-chromeos-android-extension-control.md) implements and
physically proves the selected same-device companion architecture, retained
SAF-root workflow, fixed ARC-address listener, and same-LAN refusal. It does
not publish or migrate the production JSTorrent extension, import legacy
pairing/profile state, sign a Play release, or authorize the old raw I/O
daemon architecture.

## Required Replacement Gates

- [x] **JAR-001 — Maintain a first-party Android product.** The Compose,
  in-process Rust, generated-contract, SAF, foreground-service, dual-ABI, AVD,
  physical Android, and physical ChromeOS foundations exist.
- [x] **JAR-002 — Use truthful active-work sleep inhibition.** Tactical `165`
  holds only a partial CPU wake lock for authoritative Starting, Downloading,
  and Checking work, releases it on every nonqualifying/reset/shutdown path,
  and does not restore the deprecated Wi-Fi lock.
- [x] **JAR-003 — Prove the replacement companion architecture.** Tactical
  `194` proves one Android engine/profile owner with Compose and packaged React
  clients, explicit pairing, fixed same-device reachability, retained roots,
  detached transfer, restart, revocation, and cleanup.
- [ ] **JAR-004 — Freeze the production application and migration contract.**
  Inventory the then-current released JSTorrent package, schema, private
  files, SharedPreferences, persisted URI grants, public identifiers,
  `versionCode`, signing path, backup behavior, and extension pairing state.
  Select exact import, reset, reauthorization, recheck, interruption, and
  failure behavior. The current `org.rstorrent.bootstrap` application ID and
  RSTorrent branding are not replacement values.
- [ ] **JAR-005 — Coordinate the production extension rollout.** The existing
  JSTorrent extension expects the legacy raw I/O companion, while Tactical
  `194` intentionally selects a typed semantic connection to the Rust
  application owner. Choose and prove an extension-first, app-first-compatible,
  or coordinated rollout that leaves no installed cohort stranded. Do not add
  a permanent raw I/O compatibility daemon merely to avoid rollout planning.
- [x] **JAR-006 — Add external Android torrent intake.** Completed Tactical
  [`197`](../tactical/197-android-external-torrent-intake.md) registers and bounds
  `magnet:`, `application/x-bittorrent`, and supported `content://` `.torrent`
  delivery through one exported intake owner. Reuse the application add flow
  for cold, warm, duplicate, malformed, oversized, canceled, and storage-root
  cases without creating a second engine or profile owner. JVM, Compose,
  manifest, connected API 34, hostile-provider, controlled-transfer, resource,
  privacy, grant-revocation, and cleanup evidence pass under the exact filters
  and ephemeral service-owned queue.
- [x] **JAR-007 — Add background completion and actionable failure
  notifications.** This is also beta gate `AND-009`. Completed Tactical
  [`198`](../tactical/198-android-completion-and-attention-notifications.md)
  owns one native edge owner for completion plus fatal/storage-repair
  attention, default-on app preferences, low/default/high system channels,
  permission denial, initial/reset suppression, duplicate avoidance, exact
  tap routing, restart, and cleanup. It selects JSTorrent-like transparency:
  denied or blocked notification visibility permits interactive use but not
  an invisible long-running application or companion owner after the visible
  interaction ends. The existing foreground **Stop** action remains; Pause
  All and Resume All are deferred. The implementation, deterministic suite,
  dual-ABI build, API 34/35 connected tests, genuine completion/repair
  campaigns, timeout shutdown, and cleanup pass. The physical ChromeOS
  150/API-33 campaign now also proves the exact completion tap to the fixture
  torrent, notification removal, zero restart/recheck replay, genuine repair,
  the exact attention tap to Storage, malformed-path restoration, and terminal
  cleanup. Its composed lifecycle evidence covers denied-visible-only and
  Compose-explained permission grant, authenticated companion disconnect/
  reconnect, the real ongoing-notification **Stop** action, listener refusal,
  and exact package/credential/power cleanup.
- [ ] **JAR-008 — Enforce unmetered-network policy live.** This is the required
  part of beta gate `AND-010`. Tactical
  [`199`](../tactical/199-android-live-unmetered-network-enforcement.md) now
  implements the default-off **Unmetered networks only** preference, ordered
  default-network observation, fail-closed initial/live application
  prerequisite, intent-preserving automatic recovery, and complete owned-
  generation closure. Deterministic Rust, generated clients, dual-ABI builds,
  and installed API 28/35 AVD campaigns pass, including block/restart/resume,
  exact hashes, paused intent, terminal-zero peers, and resource cleanup. The
  gate remains open only for the explicitly authorized physical-phone handoff
  campaign; no physical device was used. VPN privacy and proxying remain
  excluded.
- [x] **JAR-009 — Implement the selected background lifecycle policy.**
  Tactical
  [`200`](../tactical/200-android-product-background-lifecycle.md) now replaces
  the always-sticky owner with JSTorrent-shaped standalone outcomes over
  RSTorrent's one service/application owner. Background downloads are an
  explicit default-off opt-in gated by notification eligibility; active
  download/metadata/checking and unmetered waiting qualify, completion closes
  unattended work by default, and continued seeding is separately opt-in.
  Visible Compose use remains unrestricted, shutdown preserves torrent
  intent, task removal follows policy, reboot has no launch receiver, and the
  target-35 `dataSync` duration is finite with a persistent exhausted-quota
  fence. Authenticated ChromeOS companion work receives one fixed reconnect
  grace rather than the legacy daemon's configurable idle timer. Pure policy,
  connected API 28/35, controlled transfer/recovery/task-removal/seeding,
  shortened-timeout, dual-ABI, and repository gates pass. The initial
  maintainer-accepted compositional close remains recorded. A later authorized
  physical ChromeOS 150/API-33 campaign directly proves default-off Home/
  reopen, admitted background and sticky recovery, completion, controlled
  background upload, seeding disable, notification denial/grant, authenticated
  companion retention, grace/reconnect cancellation and idle expiry, real
  notification Stop, extension relaunch, listener refusal, and exact cleanup.
  This remains bounded physical evidence, not an indefinite/OEM-wide duration
  claim.
- [ ] **JAR-010 — Qualify the signed Play replacement.** Produce and inspect
  the protected-key release AAB, remove or deliberately retain diagnostic
  components, close current Android API deprecations, complete store/privacy/
  foreground-service declarations, and pass fresh plus signed-upgrade cohorts
  on representative phone and ChromeOS devices. Publication remains a
  separately authorized external operation.

## Capabilities Requiring An Explicit Disposition

These are current JSTorrent behaviors or controls that RSTorrent Android does
not yet match. Each needs an accepted implement, retire, or defer-and-disclose
decision before a replacement candidate. Their absence is not automatically a
blocker for an independent RSTorrent beta.

| Capability | Current comparison | Required disposition |
| --- | --- | --- |
| VPN-only mode | JSTorrent suspends when the active default network is not reported as VPN. RSTorrent has no control. The legacy check does not prove socket binding or leak prevention. | Implement only with Android `Network` binding, fail-closed startup/handover, closure or rebinding of existing TCP/UDP sockets, and peer/tracker/DHT/DNS leakage evidence; otherwise retire and disclose it. |
| SOCKS5 proxy | JSTorrent exposes host, port, optional credentials, and peer/HTTP-tracker/UDP-tracker routing choices. RSTorrent shows a disabled placeholder and has no engine proxy owner. | Use a source-first engine tactical covering DNS, authentication-secret storage, UDP ASSOCIATE or a truthful unsupported state, reconnect, bypass prevention, resource limits, and interoperability. |
| DHT and PEX controls | Completed Tactical [`205`](../tactical/205-durable-dht-and-pex-controls.md) adds backed default-on Compose controls, durable intent, live application truth, and shared engine enforcement. | Implemented. Private-torrent gating remains unconditional regardless of either setting. |
| Seeding and queue policy | JSTorrent has backing state for an active-seed limit, although its current native Settings screen does not render that control, plus a stop/close versus keep-seeding choice. RSTorrent now has exact pinned-libtorrent global active-seed and ratio/time priority semantics, but deliberately does not reproduce stop/close-on-goal. | Tactical `200` selects default background closure and a separate keep-seeding-in-background opt-in. Completed Tactical `201` adds backed Compose settings and exact active/queued/goal truth. Its installed API-35 profile proves one active and two queued seeds across foreground reopening and opt-in background ownership. Reaching a goal does not hard-stop or rewrite torrent intent; whether that deliberate difference needs additional disclosure remains release work. |
| Low-battery shutdown | JSTorrent offers an opt-in 5–50% threshold. RSTorrent has only the active-work sleep setting and a disabled Battery policy row. | Decide whether Android replacement retains this safety valve. If implemented, define charging, threshold hysteresis, notification, intent preservation, and restart behavior. |
| Companion idle/auto-close | JSTorrent can stop its separate legacy daemon after a configured disconnected interval. Tactical `194` instead owns one semantic service/application owner. | Tactical `200` selects a fixed 60-second authenticated-disconnect grace and no user-facing timer. A configurable idle policy remains deferred unless product evidence justifies it. |
| Search and plugins | JSTorrent has search UI plus installed/recommended URL-fetched JavaScript plugins in an Android WebView sandbox. RSTorrent has no search/plugin product capability. | Treat as a separate security and product campaign. Implement only with explicit network-code trust, sandbox, update, disclosure, and Play-review policy; otherwise retire/defer visibly. |
| Native/progressive playback | Completed Tactical `202` gives RSTorrent Android native Media3 playback for typed completed and eligible incomplete video through the shared Rust HTTP capability, with audio focus, picture-in-picture, removal revocation, seek, publication handoff, and playback lifetime ownership proven on physical ChromeOS. | Treat native playback as implemented. Sidecar/external subtitles, codec breadth, resume/history, background-audio controls, and production-package qualification remain separate dispositions. |
| Localization | JSTorrent currently ships system/app locale selection, base English, and 18 non-English locale directories. Completed Tactical [`204`](../tactical/204-cross-product-localization-foundation.md) gives RSTorrent complete checked English catalogs, system locale negotiation, formatting/plurals, and long-LTR/RTL pseudo evidence across React/Tauri, Android, and iOS. | Select and qualify a native-reviewed first real language cohort separately. Do not inherit or advertise JSTorrent's translations without provenance, review, lifecycle, layout, accessibility, and release-disclosure evidence. |
| Reset, clear data, and support | JSTorrent exposes reset settings, clear all data with optional payload deletion, and a prefilled report-bug path. Its reset preserves some preferences and incompletely establishes live engine reapplication; its clear workflow does not join torrent removal before dropping roots. RSTorrent now has the exact external feedback handoff while Reset engine settings remains unavailable and clear-data is absent. | Completed Tactical [`206`](../tactical/206-android-jstorrent-feedback-handoff.md) implements the current JSTorrent external feedback handoff. Ready Tactical [`207`](../tactical/207-android-safe-reset-and-clear-data.md) owns atomic engine-settings reset plus joined clear-with-keep and clear-with-exact-delete outcomes. Payload deletion remains explicit, unchecked by default, metainfo-exact, and unavailable to implicit migration reset. |
| Add-time file selection | JSTorrent can show a file-selection step during add. | Implemented by Tactical [`203`](../tactical/203-jstorrent-shaped-add-time-file-selection.md). Shared React and Compose default to one application-owned pending step: checked is Normal, unchecked is Skip, All/None are logical, magnets fetch metadata without content, and one atomic confirmation starts the durable selection. BEP 53 intent, cancellation/duplicate safety, restart, bounded paging, external intake, API-35, and physical ChromeOS evidence pass. High remains post-add. |
| Download manifest integration | JSTorrent can write a sidecar manifest for external playback integration. RSTorrent does not. | Confirm whether any supported integration consumes it; implement a safe final-path equivalent or retire it. |
| Active-piece memory override | JSTorrent exposes an Android memory-budget override. RSTorrent uses bounded engine-owned resource policy without an equivalent user control. | Prefer measured automatic limits unless physical evidence justifies an advanced setting. Record this as a deliberate difference. |
| Tracker mutation | RSTorrent Android can inspect trackers but not mutate them. | Defer unless current user journeys or ordinary interoperability require it; use a typed application command if implemented. |
| HTTP web seeds and other protocol breadth | Current JSTorrent implements web-seed behavior; RSTorrent's beta boundary still lists BEP 17/19 and several optional discovery extensions as absent. | Keep in the engine capability campaign. Promote only when replacement evidence shows an ordinary advertised journey depends on it. |

Theme selection, dynamic Android colors, ordinary peer encryption, DHT
inspection, UPnP status, IPv6, transfer limits, queue actions, file priorities,
and completed-file external open already have RSTorrent equivalents. Exact UI
layout parity is not required where the replacement preserves the underlying
user outcome truthfully.

## Android Settings Parity Ledger

This detailed native-settings comparison was refreshed against RSTorrent
commit `3c6217285cb981fa9ee4fd6415684b11065a1f1e` and JSTorrent commit
`25e4b701433fd815398ba89526546f5e4f072e3f`, then reconciled through completed
RSTorrent Tacticals `205` and `206`. It distinguishes controls
that a user can actually reach from backing settings that exist only in code.
A missing control is not automatically selected work: the capability table
above and a bounded tactical still own the implement, retire, or
defer-and-disclose decision.

### Storage, Transfer, And Queue

| Setting | JSTorrent Android | RSTorrent Android | Disposition |
| --- | --- | --- | --- |
| Download folders | Add, list, make default, open in a file manager, and remove roots. | Select/change, list, show the current root, disclose unavailable roots, and forget safe unused roots. | Broadly equivalent. RSTorrent deliberately refuses to forget the current or referenced root; JSTorrent alone has a direct Settings action to open a root externally. |
| Multiple storage roots | Supported with a selected default. | Supported through retained SAF roots and a current/default future binding. | Equivalent user outcome, with stronger revocation and reference safety in RSTorrent. |
| Add-time file selection | Default-on preference leads to a checked Normal/unchecked Skip step. | Default-on preference leads to the same Normal/Skip step, with durable pending intent and bounded paging. | Implemented by Tactical `203`; RSTorrent has the stronger restart and resource contract. |
| Global download/upload limits | Presets from unlimited through 10 MB/s. | Exact numeric KiB/s values or unlimited. | Equivalent capability with different presentation. |
| Per-torrent transfer limits | No native Settings control. | Available on torrent detail. | RSTorrent-only capability. |
| Active downloads | Range 1–5, default 5. | Configured range 1–20, default 3; Android currently applies an effective cap of 2. | Both expose the policy. RSTorrent must keep configured versus effective truth visible. |
| Peer limits | Separate global 50–1000 and per-torrent 5–100 controls. | One global session limit, range 1–2000. | Per-torrent peer limits are missing; add only if replacement evidence justifies another admission policy. |
| Upload slots | Values 0, 2, 4, 8, and 16. | Range 0–50. | Equivalent capability with a wider RSTorrent range. |
| Active seeds | Backing preference and setter exist, but the current native Settings screen does not render them. | Visible Unlimited or 0–500 setting with active/queued counts. | RSTorrent is stronger; do not describe the current JSTorrent UI as exposing this control. |
| Seeding goals | No visible ratio/time goal controls. | Visible priority ratio, total seeding time, and idle seeding time goals. | RSTorrent-only capability implemented by Tactical `201`. |
| Active-piece memory | Visible Default, 32, 48, or 64 MiB override. | Bounded automatic engine policy without a user override. | Deliberate RSTorrent difference; prefer measured automatic limits unless device evidence requires an advanced control. |
| Pipeline depth | Backing preference and setter exist, but the current native Settings screen does not render them. | No user setting. | No current visible parity gap. Keep engine policy automatic unless evidence establishes a need. |

### Network And Privacy

| Setting | JSTorrent Android | RSTorrent Android | Disposition |
| --- | --- | --- | --- |
| Unmetered-only transfers | Labeled Wi-Fi only, but implemented from Android's unmetered-network fact. | Labeled Unmetered networks only, with live closure/restart and preserved torrent intent. | Equivalent product intent; RSTorrent has the more accurate label and stronger live-enforcement contract. |
| VPN-only transfers | Suspends the engine when the default network is not reported as VPN. | Disabled placeholder. | Missing, but JSTorrent's observation does not prove socket binding, DNS confinement, or closure of existing TCP/UDP paths. Implement only as a separate fail-closed privacy feature. |
| Peer encryption | Disabled, Allow, Prefer, and Required. | Disabled, Allow, Prefer, and Required. | Equivalent. |
| DHT | Visible enable/disable toggle. | Visible default-on toggle with durable configured/effective/application truth; disable and re-enable act on the long-lived DHT owner. | Equivalent user outcome through Tactical `205`; RSTorrent retains bounded warm routing state while disabled. Private-torrent gating is unconditional. |
| PEX | Visible enable/disable toggle. | Visible default-on toggle with durable configured/effective/application truth; established and future public peers apply it live. | Equivalent user outcome through Tactical `205`; disable additionally purges PEX-only candidates and updates negotiated peers. Private-torrent gating is unconditional. |
| DHT inspection | Link to a DHT view. | Separate detailed DHT screen. | Equivalent; RSTorrent presentation is stronger. |
| UPnP | Visible toggle. | Visible toggle with typed status mapping. | Equivalent. |
| Incoming listener | No separate visible control beyond incoming/UPnP behavior. | Explicit enable/disable control and status. | RSTorrent-only presentation. |
| IPv6 | No visible native control. | Explicit enable/disable control. | RSTorrent-only presentation. |
| SOCKS5 proxy | Host, port, optional username/password, and independent peer, HTTP-tracker, and UDP-tracker routing choices. | Disabled placeholder with no engine proxy owner. | Missing. JSTorrent's engine comments require restart, while its UI does not clearly disclose that. A RSTorrent implementation needs source-first DNS, secret storage, UDP, reconnect, and bypass-prevention work. |

### Notifications And Power

| Setting | JSTorrent Android | RSTorrent Android | Disposition |
| --- | --- | --- | --- |
| Notification permission and system management | Permission/status presentation and system-settings handoff. | Permission, application preference, channel truth, and system-settings handoff. | Equivalent core outcome; RSTorrent exposes more exact app/channel state. |
| Completion notifications | No separate application preference in the current native Settings screen. | Default-on application preference. | RSTorrent-only control implemented by Tactical `198`. |
| Repair/attention notifications | No separate application preference in the current native Settings screen. | Default-on application preference. | RSTorrent-only control implemented by Tactical `198`. |
| Background downloads | Default off; enabling requires usable notifications. | Default off; enabling requires notification eligibility and is enforced through actual work admission and Android 15 finite-work quota. | Equivalent intent with a stronger RSTorrent lifecycle contract. |
| Prevent sleep | Default off and editable only when background downloads are enabled. | Default on for active download/check work and independent of background permission. | Deliberate difference. RSTorrent ties the partial CPU wake lock to authoritative active work rather than one UI preference dependency. |
| Keep seeding in background | Keep-seeding versus stop-and-close behavior. | Separate default-off opt-in that depends on background downloads. | Equivalent user choice with different engine semantics; RSTorrent does not rewrite torrent intent when lifetime closes. |
| Low-battery shutdown | Optional 5–50% threshold, default 15%; it does not trigger while charging. | Disabled Battery policy placeholder. | Missing candidate. JSTorrent asynchronously pauses all torrents, waits 500 ms, then stops, so it is behavior evidence rather than a lifecycle template. |
| Finite Android background disclosure | No equivalent explicit read-only explanation. | Visible read-only target-35 finite-background/quota disclosure. | RSTorrent-only transparency. |

### Advanced, Support, And ChromeOS Companion

| Setting | JSTorrent Android | RSTorrent Android | Disposition |
| --- | --- | --- | --- |
| Theme | System, Light, and Dark. | System, Light, and Dark. | Equivalent. |
| Dynamic colors | No visible control. | Visible Android dynamic-color control. | RSTorrent-only capability. |
| Language | System plus 18 non-English locale choices. | System-following English catalog; no language picker or qualified real translated cohort. | Missing first reviewed language cohort. Tactical `204` completed localization infrastructure, not translated-product readiness. |
| Search and plugins | Recommended plugins, URL installation, enable/disable, and removal. | Disabled placeholder and no plugin product capability. | Missing by design pending a separate security/product decision. Do not treat arbitrary fetched code as an ordinary Settings addition. |
| Download manifest | Can enable `.jstorrent.json` output for PlayVideo integration. | No equivalent. | Confirm a supported consumer before retaining it; otherwise retire and disclose. |
| Report a bug | Opens `jstorrent.com/feedback.html` with app version, Android version, and device manufacturer/model; that page embeds a prefilled Google Form and separately links to a new GitHub issue. | Advanced Settings exposes **Report Bug / Send Feedback** and sends exactly the same four-field URL to one external browser intent. | Implemented by Tactical [`206`](../tactical/206-android-jstorrent-feedback-handoff.md) without a local export, backend, durable state, or automatic submission. Exact unit/AVD tests and a physical ChromeOS live-page check pass. |
| Reset settings | Visible action. | Disabled Reset engine settings row. | Missing; ready Tactical `207` will reset every global engine setting atomically from the configured fresh-profile authority while preserving torrents, roots, per-torrent settings, payload, Android preferences, appearance, pairing, and metrics. |
| Clear all data | Visible confirmation with an unchecked-by-default Also delete downloaded files option. | No equivalent. | Missing; ready Tactical `207` will add one durable joined clear workflow with keep and exact registered-payload deletion outcomes, precise partial failure, unrelated-root-content preservation, and process-death recovery. |
| Chromebook companion mode | Separate daemon mode with its own lifecycle settings. | No separate mode; Compose and the extension share one service/application/profile owner. | Deliberately inapplicable to RSTorrent's accepted architecture. |
| Companion background/idle policy | Run-in-background toggle, configurable 5–120 minute idle close (default 30), prefer-standalone toggle, launch-standalone action, extension link, and Quit. | Ordinary background-download preference plus one fixed authenticated 60-second reconnect grace. | Tactical `200` deliberately selected a fixed grace and no prefer-standalone or user timer. Revisit only with product evidence. |

`preferredListenPort` and tracker HTTPS authentication exist in RSTorrent's
application settings contract but are not currently rendered in Compose.
They are backing-only controls, not Android-visible advantages.

### Reset And Clear-Data Safety Contract

JSTorrent's two destructive-looking actions are useful product references,
but their current implementation should not be copied literally:

- **Reset settings** clears `AndroidConfigHub` and `SettingsStore`, while
  preserving the default root key, locale, theme, and notification-prompt
  state. The dialog's all-settings wording does not disclose those
  exceptions. It explicitly reapplies several live engine values, but does
  not establish immediate reapplication for every cleared limit, proxy
  field, proxy route, or active-work preference. Some changes may therefore
  require a restart.
- **Clear all data** enumerates torrents, calls the non-suspending torrent
  removal method for each, resets settings, then removes every registered
  root. A separate awaitable removal API exists, but this workflow does not
  use it. The source therefore does not establish joined completion,
  aggregate failure handling, or completion of optional payload deletion
  before storage authority is dropped.
- The operation is not equivalent to Android's clear-app-data or reinstall.
  The implementation deliberately preserves installation metrics, while the
  reset path preserves locale, theme, and notification-prompt state; other
  stores such as pairing are not part of this workflow.

Ready Tactical
[`207`](../tactical/207-android-safe-reset-and-clear-data.md) defines one typed,
atomic engine-settings reset and a separate durable joined clear workflow.
Metadata/profile clearing remains distinct from optional payload deletion;
every removal must finish or report a precise partial failure before grants
are released. Its outcome matrix states exact torrent, payload, root, pairing,
metrics, appearance, locale, and permission behavior. Payload deletion stays
unchecked by default, applies only to registered torrent files and exact
engine-owned part artifacts, and can never be selected by migration reset.

### Settings Follow-Up Queue

This comparison produces the following bounded candidates, in recommended
order. It records priority, not implementation authorization:

1. Implement ready Tactical
   [`207`](../tactical/207-android-safe-reset-and-clear-data.md): atomic engine-
   settings reset, joined profile clear while keeping downloaded files, and
   joined profile clear with unchecked-by-default exact registered-payload
   deletion. Never recursively clean selected roots.
2. Decide whether to retain a low-battery policy with charging, hysteresis,
   notification, preserved intent, restart, and joined-shutdown semantics.
3. Treat SOCKS5 and a real VPN-only mode as separate source-first engine and
   privacy tacticals rather than UI-only settings work.
4. Select and qualify the first native-reviewed non-English cohort under the
   localization foundation.
5. Make an explicit security/product decision on search and URL-fetched
   plugins before any implementation.
6. Retain download-manifest integration only if a supported consumer still
   requires it.
7. Add per-torrent peer limits or a manual active-piece-memory override only
   if replacement or device evidence justifies the extra policy.

## Network And Privacy Decisions

Unmetered policy, VPN-only policy, and proxying are three different contracts:

- **Unmetered-only** is a cost policy over Android network capabilities. It is
  required before supported phone replacement and should count any eligible
  unmetered transport, not merely Wi-Fi. Tactical
  [`199`](../tactical/199-android-live-unmetered-network-enforcement.md)
  implements its exact callback, application gate, generated and Compose
  presentation, and AVD-qualified fail-closed recovery contract. Physical
  current-API phone handoff evidence remains required before this replacement
  gate closes.
- **VPN-only** is a privacy boundary. Observing that Android's active default
  network has `TRANSPORT_VPN` and suspending the engine is not sufficient.
  Every newly created and already-open TCP/UDP socket, resolver route, tracker,
  DHT transaction, and peer path must be bound, canceled, or replaced without
  a handover leak.
- **SOCKS5 proxy** is an engine routing feature. It needs explicit coverage of
  peers, HTTP trackers, UDP trackers, name resolution, credentials, fallback,
  and unsupported combinations. Android UI alone cannot implement it.

The three may share Android connectivity observations and product
presentation, but they must not share one ambiguous boolean or a whole-engine
pause/resume shortcut that overwrites torrent intent.

## Background And Power Decisions

Sleep inhibition and background continuation are independent. Tactical `165`
answers whether active work may keep the CPU awake after the product has
already decided it is allowed to run. Tactical
[`200`](../tactical/200-android-product-background-lifecycle.md) now implements
the `JAR-009` outcome when Compose leaves, an active download completes, only
seeding remains, the task is removed, the process restarts, or the device
reboots. Tactical `198` separately supplies its notification-visibility
prerequisite: denial or blocking permits interactive use but ends the owner
when visible interaction ends. Target-35 `dataSync` timeout commits a durable
exhausted edge, enters prompt joined shutdown, and refuses invisible
recreation until a later visible launch resets the platform allowance.
The later physical ChromeOS campaign additionally verifies that these lifetime
decisions compose with the retained extension identity and real ChromeOS
notification surface without becoming torrent commands.

The replacement policy must identify one owner for:

- activity visibility and user-requested background continuation;
- foreground-service start/stop, using Tactical `198`'s already-selected
  notification-permission and channel-block behavior;
- active-download, checking, playback, and seeding reasons to remain alive;
- idle shutdown and any low-battery stop;
- wake-lock acquisition/release; and
- joined application shutdown with observable termination.

Do not restore JSTorrent's Wi-Fi lock. Do not keep the engine alive solely to
preserve a UI cache. Disabling background work or hitting a battery policy
must preserve torrent intent so foreground return can recover predictably.

## Production Handoff And Legacy State

`JAR-004` must inspect the then-current production artifact rather than infer
migration from source types alone. At minimum it classifies:

- application-private databases, preferences, files, caches, and version
  markers;
- retained `content://` tree grants and the root identity users see;
- torrent sources, trackers, file-selection intent, queue/pause intent,
  completion claims, and settings worth importing;
- external payload locations and any sidecar/part/manifest artifacts;
- companion pairing credentials and the installed production extension's
  protocol/version expectations; and
- notification channels, deep links, provider authorities, backup/restore,
  and public component names whose behavior survives an Android update.

The default safe direction is to import compact user intent, not runtime
authority. Legacy verified/completed bits, peer state, in-flight writes,
credentials of uncertain provenance, and ambiguous roots fail closed or
require reauthorization. Existing payload remains untouched and becomes
verified only through the ordinary checker. Migration failure must not fall
through to deleting payload or silently starting unrestricted networking.

The tactical must select whether an old app can be relaunched after an
interrupted or failed rollout, and ensure schema mutation does not create a
half-readable profile. A staged import or versioned cutover needs an explicit
commit point and repeatable crash cases.

## Production Extension Rollout

Tactical `194` proves the new companion implementation but deliberately
excludes the production JSTorrent extension. Replacement therefore needs a
coordinated rollout contract:

1. identify every supported installed extension/app version pair;
2. select which side can understand both the old and new launch/pairing state
   during the transition, or require an explicit paired update;
3. update production extension permissions, Android package/deep-link
   metadata, connection versioning, and recovery presentation;
4. prove extension-first, app-first, stale-extension, revoked-pairing,
   offline-update, and clean-install outcomes; and
5. retain Tactical `194`'s fixed ARC endpoint, origin/Host checks, explicit
   approval, token rotation/revocation, and same-LAN refusal.

The rollout must not reopen a wildcard LAN listener or copy the legacy raw I/O
daemon into the Rust product. A temporarily incompatible pair must fail with
an actionable update path rather than silently hanging or exposing storage.

## Source Audit

### RSTorrent

The initial audit used:

- `clients/android/app/build.gradle.kts`: provisional application ID and
  version;
- `clients/android/app/src/main/AndroidManifest.xml`: launcher, companion deep
  link, services, permissions, and absence of external torrent filters;
- `clients/android/app/src/main/java/org/rstorrent/bootstrap/ProductEngineService.kt`:
  one sticky foreground-service/application owner, fixed `Online` startup
  policy, partial wake lock, and Stop-only notification;
- `clients/android/app/src/main/java/org/rstorrent/bootstrap/ui/ProductApp.kt`:
  current Notifications, Power, Advanced, and unavailable rows;
- `clients/android/app/src/main/java/org/rstorrent/bootstrap/ui/ProductSettingsScreens.kt`:
  backed network settings plus disabled VPN, metered, and proxy rows;
- `clients/android/app/src/main/java/org/rstorrent/bootstrap/{SettingsDraftModel,SettingsPatches}.kt`:
  editable application settings, validation, and typed patch construction;
  and
- `clients/android/app/src/main/java/org/rstorrent/bootstrap/{ProductLifecyclePreferenceStore,ProductNotificationSettings}.kt`:
  Android-owned background, seeding, notification, and sleep preferences.

The implemented/evidence baseline comes from Tacticals `117`, `165`, `191`,
and `194` plus the Android rows in `client-surfaces` and
`capability-readiness`.

### JSTorrent Android

The comparison inspected these paths at the revision recorded above:

- `android/app/build.gradle.kts` and `android/app/src/main/AndroidManifest.xml`:
  production ID/version shape, public components, magnet, MIME, and file
  intake;
- `android/app/src/main/java/com/jstorrent/app/LinkHandlerActivity.kt`:
  standalone/companion routing and external source handling;
- `android/app/src/main/java/com/jstorrent/app/settings/SettingsStore.kt`:
  Android-only network, background, power, add, completion, companion-idle,
  locale, and appearance preferences;
- `android/app/src/main/java/com/jstorrent/app/network/{NetworkMonitor,NetworkRestrictionEnforcer}.kt`:
  unmetered/VPN observations and whole-engine suspension behavior;
- `android/app/src/main/java/com/jstorrent/app/notification/{ForegroundNotificationManager,TorrentNotificationManager}.kt`:
  foreground status/actions plus completion and error attention;
- `android/app/src/main/java/com/jstorrent/app/service/{ServiceLifecycleManager,ForegroundNotificationService}.kt`:
  foreground/background/idle ownership, wake locks, and low-battery handling;
- `android/app/src/main/java/com/jstorrent/app/ui/screens/{NetworkSettingsScreen,PowerManagementSettingsScreen,AdvancedSettingsScreen,StorageSettingsScreen,SpeedConnectionLimitsSettingsScreen,NotificationsSettingsScreen}.kt`:
  proxy, DHT/PEX, power, localization, reset/support, file-selection, seeding,
  notification, and memory controls;
- `android/app/src/main/java/com/jstorrent/app/viewmodel/SettingsViewModel.kt`,
  `android/quickjs-engine/src/main/kotlin/com/jstorrent/quickjs/storage/AndroidConfigHub.kt`,
  and
  `android/quickjs-engine/src/main/kotlin/com/jstorrent/quickjs/EngineController.kt`:
  reset preservation, live bridge reapplication, clear ordering, and the
  non-suspending versus awaitable removal APIs;
- `android/app/src/main/java/com/jstorrent/app/ui/screens/{SearchScreen,SearchPluginSettingsScreen}.kt`
  and `android/app/src/main/java/com/jstorrent/app/search/`: search/plugin
  product and sandbox boundaries; and
- `android/app/src/main/java/com/jstorrent/app/player/PlayerActivity.kt`:
  local/progressive Media3 playback, subtitles, and picture-in-picture.

These sources are behavior and edge-case references, not architecture
templates or source donors. The replacement retains RSTorrent's Rust engine,
one application owner, generated boundary, bounded resources, and ordinary
integrity rules.

## Validation Program

Each tactical defines proportional gates. The integrated replacement cohort
eventually includes:

- deterministic reducers for notification edges, network prerequisites,
  background reasons, migration commit points, and extension-version pairing;
- instrumented AVD cases for external intents, permission denial, metered and
  unmetered transitions, process death, task removal, reboot, migration crash,
  revoked SAF grants, and exact cleanup;
- physical current-API phone screen-off/Doze, Wi-Fi/cellular/metered-handoff,
  notification, completed/error, playback if selected, and battery-policy
  evidence;
- physical ChromeOS Compose and production-extension cold/warm/reconnect,
  app-first/extension-first update, retained-root repair, detached transfer,
  fixed-ARC reachability, and same-LAN refusal;
- signed AAB fresh installation and upgrade from a production-equivalent
  `com.jstorrent.app` fixture with the real signature/update path; and
- payload hashes, root/grant observations, profile inspection, process/task
  residue, descriptor/resource high-water marks, and network capture where a
  privacy claim is involved.

Live public swarms remain opt-in. Controlled fixtures and pinned-libtorrent
interoperability provide ordinary transfer evidence before any representative
live run.

## Recommended Next Work

1. Create the bounded `JAR-004` production handoff and legacy-state tactical.
   It fixes the candidate identity, migration/reset boundary, payload safety,
   signing inputs, and exact old-version fixture before code changes make
   accidental compatibility promises.
2. Preserve completed Tactical
   [`197`](../tactical/197-android-external-torrent-intake.md) as the `JAR-006`
   external-intake regression gate while the provisional product identity is
   replaced later under `JAR-004`.
3. Preserve completed Tactical
   [`198`](../tactical/198-android-completion-and-attention-notifications.md)
   as the `JAR-007` completion/failure notification, companion-aware
   permission-transparency, exact activation, and target-35 timeout regression
   gate.
4. After explicit authorization, finish Tactical
   [`199`](../tactical/199-android-live-unmetered-network-enforcement.md)'s
   bounded physical-phone Wi-Fi/metered handoff and cleanup gate, then close
   the unmetered portion of `JAR-008` without coupling it to a VPN privacy
   claim. The implementation and owned-AVD evidence are already complete.
5. Preserve completed Tactical
   [`200`](../tactical/200-android-product-background-lifecycle.md) as the
   `JAR-009` lifecycle regression gate. Its accepted evidence includes the
   installed API 28/35 campaign, deterministic companion lifetime, Tactical
   `194`'s physical ChromeOS transport/security proof, and the later physical
   ChromeOS 150/API-33 lifecycle/companion/notification strengthening. This is
   still a bounded observation rather than an indefinite or OEM-wide duration
   claim. Low-battery shutdown remains a separately bounded decision.
6. Preserve completed Tactical
   [`201`](../tactical/201-durable-seeding-goals-and-seed-admission.md) as the
   seed-policy regression gate. Its exact pinned-libtorrent goal-met-not-stop
   contract and Compose settings/status remain composed with Tactical `200`'s
   independent background-lifetime decision.
7. Design `JAR-005` with the production extension before either store update
   is scheduled.
8. Preserve completed Tactical
   [`203`](../tactical/203-jstorrent-shaped-add-time-file-selection.md) as the
   Add-time file-selection regression gate and completed Tactical
   [`204`](../tactical/204-cross-product-localization-foundation.md) as the
   cross-product localization-foundation gate, and completed Tactical
   [`206`](../tactical/206-android-jstorrent-feedback-handoff.md) as the exact
   external-feedback regression gate. Implement ready Tactical
   [`207`](../tactical/207-android-safe-reset-and-clear-data.md) independently
   for atomic reset, joined clear-with-keep, and joined exact registered-
   payload deletion. Select the first reviewed real language cohort
   separately, and decide VPN, proxy, search/plugins, playback follow-ups, and
   the remaining table rows individually. Proxy and any engine/network
   privacy work follow the source-first engine campaign; search/plugin and
   playback follow-ups remain separate security/lifecycle campaigns.
9. Run `JAR-010` only after the required gates and disposition ledger converge.
   Signing, store upload, staged rollout, production extension publication,
   and release promotion each remain explicitly authorized operations.
