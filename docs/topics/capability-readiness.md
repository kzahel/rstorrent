# Capability Readiness

Topic: `capability-readiness`

Work-selection policy: the former single-**Now** convention is retired as of
2026-08-29. Multiple independent tacticals may be **Active** concurrently.
**Active**, **Ready**, and **Later** describe current planning state; they are
not locks, authorization gates, or a mandatory execution sequence. A direct
user request may activate bounded work without demoting or pausing unrelated
active tacticals. Sequence only concrete dependencies, overlapping ownership
or edits, and incompatible contracts. Older **Now**, yield, resume, and
displacement wording below is retained only as historical scheduling
narrative and has no current force.

Status: Authoritative cross-cutting feature-readiness scoreboard and current
work inventory for the functional public-incubation release line. Maintainer
direction on 2026-08-22 completed beta-release foundation Tactical `157` and
cross-platform presubmit Tactical `159`, then selected desktop release/updater
Tactical `158`. Native Windows evidence exposed its release-blocking default
listener defect; bounded Tactical `160` now repairs it and adds native Windows
presubmit coverage. Completed Tactical `161` adds the parented native packaged
picker, passes installed unsigned Windows setup/repair/restart, and removes the
packaged Linux helper dependency. Completed Tactical `162` closes
single-instance, tray, joined desktop lifecycle, release-only Windows GUI
launch, and native Linux arm64 package coverage with installed Windows
x86_64/Linux arm64 evidence. Maintainer direction on 2026-08-24 makes
installed `magnet:` and local `.torrent` activation a beta usability
requirement. Completed Tactical `163`
has its bounded implementation, deterministic/package gates, and installed
Linux arm64, Windows x86_64-application, and macOS arm64 campaigns. The Windows
package ran as a real x86_64 PE under Windows 11 arm64 x64 emulation. The
macOS campaign preserved JSTorrent's inherited default handler while proving
targeted RSTorrent LaunchServices delivery, cold/visible/hidden Add flow,
bounded failures, duplicates, tray Quit, and exact cleanup. Exact hosted run
`32775002484` passed all eight platform jobs. Maintainer direction on
2026-08-25 selected desktop completion and fatal/repair notification Tactical
[`164`](../tactical/164-desktop-completion-and-attention-notifications.md)
before the next signed candidate. It is complete with deterministic/package
gates and installed macOS arm64, Windows x86_64, and Linux arm64 evidence.
Explicit maintainer direction then selected cross-platform active-download
sleep-inhibition Tactical
[`165`](../tactical/165-cross-platform-active-download-sleep-inhibition.md).
It is complete with deterministic/build gates and guest-native installed
macOS arm64, Windows arm64, Linux arm64, physical Android API 37, and physical
iOS evidence. Explicit maintainer direction on 2026-08-26 temporarily yielded
release/updater Tactical `158` to bounded desktop native-bootstrap and
extension-scaffold Tactical
[`166`](../tactical/166-desktop-native-bootstrap-and-extension-scaffold.md).
That tactical is complete after its exact-ID installed Chrome `hello` and
cold-launch smoke; Tactical `158` remains active. Later
desktop extension control remains undecided.

Explicit maintainer direction later on 2026-08-26 temporarily yields Tactical
`158` to ChromeOS Linux Tactical
[`167`](../tactical/167-chromeos-crostini-bundled-web-launcher.md). It is
complete: the bundled gateway/React package, static on-demand service,
ChromeOS Launcher, and exact beta-extension handoff pass the available
physical Chromebook lifecycle, detachable-transfer, preservation, and purge
matrix. The conditional full reboot was unavailable because the testbed has no
approved profile-login credential. Tactical `158` remains active.

Explicit maintainer direction then temporarily yields Tactical `158` to
platform-aware extension launcher Tactical
[`168`](../tactical/168-platform-aware-extension-launcher.md). It is complete:
ChromeOS gets the exact published JSTorrent Android listing and ChromeOS Linux
controls, desktop gets the native bootstrap, and unknown platforms retain both
as a recovery fallback. Deterministic/package gates and the physical ChromeOS
chooser, exact Play destination, and retained Crostini handoff pass without
detecting Play or Android-app availability. Tactical `158` remains active.

Explicit maintainer direction next temporarily yielded Tactical `158` to
hosted Crostini bootstrap/release Tactical
[`169`](../tactical/169-hosted-crostini-bootstrap-and-release.md). That slice
is complete: the pinned-key installer, strict manifest, native x86_64/ARM64
workflow, deterministic failure corpus, and physical x86_64 real-package
fixture pass. A subsequent explicitly authorized operation published
non-latest `crostini-v0.1.0`, deployed the website bootstrap, verified both
public archives and the production-key manifest, and passed the exact website
install/Launcher/relaunch path on the physical x86_64 Chromebook. Tactical
`158` remains active.

Maintainer direction on 2026-08-26 temporarily yielded Tactical `158` to
configured Linux headless-service Tactical
[`170`](../tactical/170-configured-linux-headless-service.md). That slice is
complete: strict root/listener/origin/auth configuration, disabled-by-default
systemd user installation, rollback-safe repair, preservation-safe removal,
isolated TLS/WSS proxy evidence, byte-identical x86_64/ARM64 packages, and the
real x86_64 Linux no-presentation transfer/seeding/restart campaign pass. It
adds no built-in TLS, relay authentication, extension control, seeding goals,
or public release. Tactical `158` remains active with its
open signed Windows and Linux x86_64 evidence unchanged.

Explicit maintainer direction later on 2026-08-26 temporarily yielded Tactical
`158` to signed headless release and trusted-LAN service Tactical
[`171`](../tactical/171-signed-headless-release-and-lan-service.md). This slice
is complete with the strict two-architecture signed release/update lane,
operator-approved CLI and browser checks, one exact RFC 1918 `lan-none` mode
with truthful
full-control presentation, and an enabled healthy deployment bound only to the
current x86_64 Linux machine's selected Ethernet address. It performed no tag,
public release, website/channel deployment, unattended update, system-wide
service, firewall change, or Raspberry Pi mutation. Tactical `158` remains
active with its open signed Windows and Linux x86_64 gates unchanged.

Explicit maintainer direction on 2026-08-27 temporarily yielded Tactical `158`
to exact tailnet headless-access Tactical
[`174`](../tactical/174-exact-tailnet-headless-access.md). The bounded design
keeps the installed exact LAN endpoint, adds one exact loopback endpoint for a
dedicated Tailscale Serve HTTPS authority, retains one application owner, and
rejects wildcard binds, direct shared-range binds, Funnel, and tailnet policy
mutation. Its deterministic, installed-service, package-repair, exact-route,
WSS/media, and phone-sized browser evidence now passes without changing any
existing Serve route or ACL. Tactical `158` remains active.

Explicit maintainer direction later on 2026-08-27 temporarily yielded Tactical
`158` to retained Swarm transfer-total Tactical
[`175`](../tactical/175-retained-swarm-peer-transfer-totals.md). This bounded
slice makes exact useful payload downloaded from and uploaded to each retained
peer record visible through disconnect, backoff, and reconnect. It does not
change peer selection, retry policy, persistence, or the current stalled
torrent's runtime behavior. Exact engine/session transitions, generated web
and UniFFI contracts, React desktop/phone presentation, Android dual-ABI
build, full workspace/web gates, and the repaired installed LAN/tailnet
service pass. Tactical `158` remains active.

Explicit user direction later on 2026-08-27 temporarily yields Tactical `158`
to durable High file-priority Tactical
[`176`](../tactical/176-durable-high-file-priority.md). The bounded slice adds
High/Normal/Skip persistence, weighted ordinary piece activation, and truthful
first-party presentation while composing beneath completed Tactical `139`'s
independent transient streaming urgency. Implementation and every available
Linux/web/Android gate pass; the updated SwiftUI source still needs its
macOS-only simulator/archive compile. Tactical `176` remains active
until that stopping-condition gate closes.

Explicit user direction subsequently yields Tacticals `176` and `158` to
bounded dry-swarm recovery Tactical
[`177`](../tactical/177-bounded-dry-swarm-recovery.md). The observed running
swarm exhausted its ordinary three-attempt ceiling for every retained peer and
had no eligible, dialing, connected, or payload-active alternative despite
continuing discovery. Tactical `177` retains that ceiling while adding exact
tracker-source rehabilitation plus one globally paced transient-failure probe
only when no ordinary action remains. Deterministic source/selection/cadence
evidence, scripted fourth-attempt verified completion, full workspace gates,
and both maintained Android native builds pass. Tactical `177` is complete and
Tactical `176` remains active.

Explicit user direction subsequently temporarily yielded Tactical `176` to
Crostini storage-guidance Tactical
[`178`](../tactical/178-crostini-storage-guidance.md). Five alternating
physical Chromebook trials establish the Linux-Downloads versus shared-
ChromeOS performance direction. The exact Crostini health identity now gates
Add and Downloads guidance, known-root performance labels, automatic
**Linux files > Downloads** visibility, and the complete **Share with Linux**
workflow without changing root authority or defaults. Deterministic web
tests, typecheck, and the production/CSP build pass. A physical Chromebook
Launcher run additionally proves the collapsed/expanded Settings help, Add
dialog label, cancel path, and exact Files-app context action while preserving
the installed public binaries and restoring the original web tree. Tactical
`178` is complete and Tactical `176` remains active.

Explicit user direction subsequently yields Tacticals `176` and `158` to
disposable-incubation state Tactical
[`179`](../tactical/179-disposable-incubation-state-epoch.md). It establishes
schema 21 as a fresh catalog epoch, resets every recognized schema `1..=20`,
and removes compatibility-only readers for DHT snapshot v1, desktop-shell
settings v1/v2, and browser appearance v1/v2. The existing bounded pre-task
reset, fail-closed hostile-state handling, and external-payload preservation
remain mandatory. All repository/web/Android gates pass; Tactical `179` is
complete, and Tactical `176` remains active with only its existing
macOS-only iOS compile gate.

Explicit user direction then selects typed settings patch and draft-
convergence Tactical
[`180`](../tactical/180-typed-settings-patches-and-draft-convergence.md). It
removes the unsupported whole-value and pair-specific settings commands
without aliases or a version bridge, updates every first-party generated
boundary, and makes web and Android settings edits survive complete live view
updates through receipt/revision convergence. Tactical `180` was selected for
active work; Tactical `176` retained its existing macOS-only iOS compile gate.

Tactical `180` is complete. Closed client/torrent patches, atomic merge and
replay semantics, generated browser/Tauri/Android/Swift boundaries, web and
Compose draft convergence, full repository/web/Android gates, and controlled
active listener handover/restart/bind-recovery evidence pass. The Linux host
could inspect but not compile the regenerated Swift boundary; Tactical `176`
therefore remains active for its unchanged macOS-only iOS
simulator/archive gate. Tactical `158` remains next.

Explicit user direction subsequently temporarily yields Tactical `176` to
production WebSocket UI bandwidth-baseline Tactical
[`183`](../tactical/183-production-websocket-ui-bandwidth-baseline.md). This
measurement-only slice records transition and steady application bytes for
the current interest-selected production React views, including default Normal
Diagnostics, before choosing a delivery or wire optimization. It adds no
polling fallback, product telemetry, delivery policy, or API optimization.
Tactical `176` retains only its unchanged macOS-hosted iOS compile gate.

Tactical `183` is complete. Its clean production React run uses exactly one
WebSocket and no semantic HTTP, cross-checks browser and gateway bytes, and
separates initial/transition traffic from equal steady windows across
Transfers, Peers, General, Files, Pieces, and Normal Logs. Projection interest
already excludes inactive details, but interleaved view IDs defeat the
view-set's tail-only current-state coalescing; complete Library and Summary
rows dominate the measured Workbench traffic before Logs or framing. No
product API or delivery behavior changed. Tactical `176` remained active for
its unchanged macOS-only iOS compile gate.

Explicit user direction on 2026-08-28 temporarily yields Tactical `176` to
view-aware current-state coalescing Tactical
[`184`](../tactical/184-view-aware-current-state-coalescing.md), followed by a
separate measured typed sparse-row repair. Tactical `184` changes no public
DTO: it makes compatible patches for one logical view coalesce across
interleaved other view IDs, retains ordered Diagnostics and exact queue/reset
semantics, and reruns Tactical `183` before the sparse contract is selected.
Both semantic changes must remain encoding-independent so a much later binary
codec can reuse them. Tactical `176` retains only its unchanged macOS-hosted
iOS compile gate.

Tactical `184` is complete. Its clean retained run reduces total server
application bytes by 76.48% and active detail rates by 70--86%, with zero
duplicate-view batches, resets, or lost progress and no public-contract change.
Typed sparse hot-view Tactical
[`185`](../tactical/185-typed-sparse-hot-view-patches.md) is complete. It
replaces repeated Torrent, File, Peer, and active-piece rows across every
first-party reducer and cuts another 36.77% from the post-coalescing clean run,
with zero resets or lost progress. Semantic fields remain independent from
JSON and from a much later negotiated binary codec. Tactical `176` remained
active with only its unchanged macOS-hosted iOS compile gate.

Explicit user direction then temporarily yields Tactical `176` to current-rate
and incremental speed-history Tactical
[`186`](../tactical/186-current-rates-and-incremental-speed-history.md). The
measured always-present `session-rates` projection currently sends a complete
300-point ten-minute graph once per second merely to read current upload and
download. Tactical `186` separates that tiny latest-value state from the
interest-selected graph and makes completed graph buckets bounded contiguous
appends validated by semantic history position while reusing the established
view-set cursor and acknowledgement. It is active; Tactical `176`
retains its unchanged macOS-only iOS compile gate.

Tactical `186` is complete. The obsolete combined `SessionSpeed` contract is
removed: `SessionCurrentRates` now carries tiny complete latest values, while
interest-selected `SessionSpeedHistory` carries one bounded snapshot followed
by exact nullable contiguous appends anchored by history epoch and completed-
bucket position. Existing view-set cursor acknowledgement remains the sole
transport reliability and backpressure mechanism. React requests history only
while Speed is visible; Android composes current and history subscriptions for
its Speed route; every Linux-available first-party reducer rejects continuity
or shape gaps atomically. The clean retained run reduces Tactical `185`'s
server payload from 783,539 to 454,581 bytes (-41.98%) and idle Transfers from
5.28 to 0.41 KiB/s, with exact browser/gateway agreement, zero resets, and
progress from 1% to 20%. Tactical `176` remains active with only
its unchanged macOS-hosted iOS compile gate.

Explicit user direction subsequently temporarily yields Tactical `176` to
compact metadata-acquisition progress Tactical
[`187`](../tactical/187-compact-metadata-acquisition-progress.md). The bounded
slice carries one selected-torrent packed BEP 9 block map for v1, v2, and
hybrid metadata into an accessible React General card. Pure-v2 and hybrid BEP
52 hash acquisition remains a separate coarse active/waiting preparation phase
without an invented percentage. Tactical `187` is active; Tactical
`176` retains only its unchanged macOS-hosted iOS compile gate.

Tactical `187` is complete. The engine now emits current generation-fenced
BEP 9 state as four two-bit blocks per byte, and one interest-selected
`torrent_preparation` view carries at most 480 raw/640 base64 map bytes plus
bounded scalars. General renders one accessible Canvas and text legend for v1,
v2, and hybrid metadata; v2/hybrid Merkle preparation remains a separate
active/waiting record without an invented percentage. Generated web and
UniFFI contracts, semantic validation, all first-party reducers, wide/phone
browser evidence, Android dual-ABI/APK/unit gates, Linux-available Apple
boundary checks, and full workspace/web gates pass. Tactical `176` remained
active with only its unchanged macOS-hosted iOS compile gate.

Explicit user direction on 2026-08-28 temporarily yields Tactical `176` to
Library torrent-detail Tactical
[`072`](../tactical/072-derived-media-catalog.md). The activated revision keeps
the existing torrent-backed Library collection, adds one separately leased
derived video/episode catalog for an explicitly opened source, and makes card
activation enter a responsive Media/All files detail with exact per-file
progress and availability. Thumbnails, artwork, playback presentation,
Library-wide item aggregation, persistence, and engine behavior remain outside
the slice. Tactical `072` is active; Tactical `176` retains only its
unchanged macOS-hosted iOS compile gate.

Tactical `072` is complete. A pure versioned classifier now derives
recognized video and conservative episode identity once per verified file
catalog; one cached application model joins authoritative progress and exposes
it through a separately leased generated `torrent_media` projection. React
Library cards open a responsive media-first detail with numeric episode
sorting, exact download/selection/availability state, explicit All files,
same-document history, focus repair, and bounded virtualization. A controlled
eight-file libtorrent run observed metadata pending, six Media rows, all eight
Files rows, exact lease eviction/recovery, zero semantic HTTP calls, and joined
cleanup. Thumbnails, artwork, playback, persistence, and Library-wide item
aggregation remain deferred. Tactical `176` remains active with
only its unchanged macOS-hosted iOS compile gate.

Explicit user direction on 2026-08-28 temporarily yielded Tactical `176` to
Library playback and torrent-size repair Tactical
[`189`](../tactical/189-library-playback-and-torrent-size.md). Tactical `189`
is complete. Eligible Library Media rows now expose accessible Play controls
through the existing ephemeral browser/Tauri media action, while ineligible
rows remain explicitly disabled. Exact nullable decimal total size now comes
from verified content geometry through complete summaries, typed sparse
updates, generated contracts, and every first-party reducer; Library,
Transfers, and Workbench therefore share the same value instead of permanent
pending placeholders. Workspace, web, Android dual-ABI/APK/unit, and
Linux-available Apple boundary gates pass. The Library browser case passes at
wide and phone sizes; the complete Playwright run retains one Swarm-only
scroll-indicator failure that passes alone and is outside this repair.
Embedded playback, media enrichment, and native mobile playback presentation
remain separate. Tactical `176` remains active with only its
unchanged macOS-hosted iOS compile gate.

Completed existing-payload adoption and recheck Tactical
[`188`](../tactical/188-existing-payload-adoption-and-recheck.md) replaces a
fresh-row `output already exists` repair with bounded
path/platform discovery and the common complete checker, commits discovered
ownership and pending verification atomically, and makes managed cleanup
metainfo-exact so unrelated content survives. It changes no schema, trusted
fast-resume rule, or user setting. Its deterministic, controlled-libtorrent,
workspace, web, Android, package, and local-service gates pass. Tactical `176`
remains active with only its unchanged macOS-hosted iOS compile
gate.

Tactical `176` is complete on 2026-08-29. Xcode 26.6 regenerated and compiled
the current Swift boundaries, all 26 iOS unit tests and two product-surface UI
tests pass on the simulator, and the unsigned generic arm64 device archive
contains the expected application. The gate also found and repaired one
Swift grammar regression from later existing-payload Tactical `188` without
changing cleanup semantics. Tactical `158` remains active with its signed
Windows and installed Linux x86_64 gates unchanged.

The updater tactical's client, production route, five-target signed hosted
rehearsal, public `0.1.0`, `0.1.1`, and `0.1.2` releases, installed macOS arm64
launch smoke, and exact macOS arm64 and Linux arm64 `0.1.0`-to-`0.1.1` updates
now pass. Public `0.1.2` contains the completed Windows and desktop-integration
repairs; its complete signed matrix and a bounded exact-public-DMG macOS arm64
launch/native-host check pass. Windows x86_64 updater replacement/relaunch is
still proven only under the older automatic-loopback profile. Clean repaired
Windows and Linux x86_64 updates plus signed Windows firewall-consent
characterization remain open; installed Intel macOS testing is deliberately
omitted. These gaps keep the tactical active. The tables below record current
support, evidence, and highest-risk gaps; implementation history remains in
the linked tacticals and focused topics.

Maintainer direction on 2026-08-27 makes every `0.1.x` package and current
platform preview disposable incubation output. Existing cross-version runs
remain updater/package evidence, but no `0.1.x` torrent, setting, root,
selection, verification, generated-API, updater-identity, or rollback
retention is a support requirement. A fresh compatibility baseline begins only
when a future version is explicitly declared the first supported beta or
release.

## Purpose And Ownership

This topic answers four recurring questions:

- What can RSTorrent actually do now?
- What evidence supports each claim?
- Which missing capability presents the highest product or correctness risk?
- Which bounded implementation slices are active or useful candidates?

This is a roll-up, not a second source of detailed design truth. Focused topics
own their invariants and decisions, numbered tacticals own implementation
plans and execution records, and tests remain the executable evidence. This
topic links those records and states the current priorities.

[`product-direction.md`](product-direction.md) owns durable product posture and
dependency direction. [`protocol-support.md`](protocol-support.md) owns
BEP-level claims.
[`download-correctness.md`](download-correctness.md) owns completion, integrity,
and recovery scenarios.
[`beta-release-readiness.md`](beta-release-readiness.md) owns the external-beta
gap ledger, distribution gates, and release feature boundary. This topic owns
the cross-cutting capability view and its non-exclusive active and ready work
sets.

## Reading The Scoreboard

Implementation state and evidence are independent:

- **Absent** means the product has no usable implementation of the named
  capability. Supporting values or a test stub alone do not change this.
- **Partial** means a useful subset exists, but a named common path, failure
  mode, or owner remains missing.
- **Implemented** means the exact bounded scope stated in that row exists. It
  does not imply that the surrounding category or client is complete.

Evidence labels are a set, not a maturity score:

- **deterministic**: runtime-independent unit or state-transition tests;
- **runtime**: scripted sockets, storage, persistence, process death, or task
  lifecycle tests;
- **interop**: controlled exchange with an independent implementation;
- **web**: the shared web product surface driven in headless Chrome;
- **desktop**: a real Tauri shell smoke or lifecycle run;
- **AVD**: the Android Compose product surface on an owned no-window emulator;
- **physical**: an explicitly authorized Android or ChromeOS device run;
- **live**: a representative non-controlled swarm observation.

An observed outcome without enough captured state to reproduce or explain it
is recorded as an observation, not promoted to verified evidence.

## Priority Policy

Choose work in this order unless new evidence justifies an explicit change:

1. prevent data corruption, unsafe input effects, or security boundary leaks;
2. make an otherwise valid download complete under ordinary peer failure;
3. improve interoperability with common swarms and discovery sources;
4. bound or improve resource use, latency, and throughput; and
5. add protocol breadth and product convenience.

Within the highest applicable class, prefer a slice that has a deterministic
failure fixture, establishes an owner needed by later work, and has a
falsifiable end-to-end stopping condition. This order is a default for choosing
otherwise unspecified work, not a reason to redirect an explicit user request.
Keep the **Active** set honest and bounded enough that ownership and working-
tree conflicts remain visible, but impose no artificial item count. **Ready**
means sufficiently framed to start; it does not mean blocked behind every
active item.

The completed DHT and multi-peer foundations now let late tracker or DHT
observations improve active content work. The current campaign and restart
checkpoint live in
[`oracle-driven-engine-campaign.md`](oracle-driven-engine-campaign.md). It
uses pinned libtorrent source and tests to choose coherent owner-level changes,
then validates them through deterministic, controlled, and paired public
evidence.

Tactical [`074`](../tactical/074-context-specific-metainfo-limits.md) is
complete. Generic bencode, BEP 9, durable metadata, and parser-only explicit
import gained independent byte and structure profiles; Tactical `081`
subsequently scales those profiles and adds product byte intake.

Tactical [`075`](../tactical/075-ephemeral-application-state.md) is complete.
The application can now select private bounded in-memory session and metrics
stores, preserve the ordinary semantic and owner lifecycle while open, report
page exhaustion as a resource limit, and close without creating profile or
payload artifacts in the metadata-only case. Durable persistence behavior is
unchanged.

Tactical [`078`](../tactical/078-local-single-peer-tcp-seeding.md) is complete.
One application-owned IPv4 loopback listener now routes bounded handshakes to
eligible complete path-backed torrents and serves verified BEP 9 metadata and
payload to one peer across download-task completion and durable application
restart. Scripted engine/application evidence and controlled libtorrent and
RSTorrent single-/multi-file transfers pass. This is not persisted settings,
multi-peer upload policy, listener advertisement, LAN/public binding, or NAT
reachability.

Tactical [`081`](../tactical/081-v1-torrent-byte-intake.md) is complete. Exact
source and operational metadata remain distinct; SQLite retains bounded
original source bytes in durable or ephemeral mode; metainfo tracker tiers are
operational state; and one semantic byte operation is adapted to WebSocket,
HTTP automation, and raw Tauri IPC. The implemented compatibility target
follows pinned libtorrent's v1 limits: 30-MiB BEP 9 receive, 64-MiB
explicit/durable/source input and local metadata upload, 2,097,152 pieces,
536,854,528-byte pieces, and measured calibration where parser, catalog, path,
view, or platform representations are not apples-to-apples.

Tactical
[`082`](../tactical/082-bounded-multi-peer-upload-ownership.md) is complete.
Incoming and outgoing sockets share one descriptor-aware session budget; eight
fixed upload slots, one optimistic grant, exact request/read/writer bounds, and
physical peer/torrent/session payload accounting now enforce multi-peer
seeding. Two RSTorrent and two libtorrent 2.0.13 clients overlapped against one
seed, independently verified 67,109,595 bytes each, and produced the exact
268,438,380-byte physical upload total. This explicitly directed completion
did not change the active product-surface work.

Tactical [`083`](../tactical/083-shared-torrent-file-picker.md) is complete.
Empty Add now opens the shared browser/Tauri single-file chooser, reuses the
root/start options, and submits one bounded `ArrayBuffer` through the active
adapter with initial selection `all`. Rust derives the source digest while
preserving legacy receipt replay. Headless Chrome proves one chooser, one
WebSocket binary frame, zero semantic HTTP calls, a visible imported row,
empty serious/critical axe findings, joined cleanup, and no metadata-only
payload artifacts.

Tactical
[`086`](../tactical/086-long-lived-torrent-peer-runtime.md) is complete. One
task-free torrent peer state and application-generation runtime now retain
ordinary peer authority across download and completed-seed lifetimes. Routed
incoming sockets populate the existing Peers/Swarm contract and compact flags.
An authenticated gateway run observed simultaneous pinned libtorrent 2.0.13
and RSTorrent rows while both independently verified 67,109,595 bytes, then
observed exact removal, inactive empty pause state, exact 134,219,190-byte
physical upload, and terminal zero ownership.

Tactical [`088`](../tactical/088-upnp-mapped-external-tcp-seeding.md) is
complete. Schema version 10 adds explicitly enabled local-network listening
and mapping policy while migrations stay disabled. One joined coordinator and
bounded IGD v2 `WANIPConnection:2` runtime install, query, renew, and delete a
finite TCP mapping. A controlled off-LAN peer directly hash-verified all 257
pieces and 4,195,035 payload bytes through the mapped public endpoint;
ordinary Peers/Swarm state, exact upload, independent deletion, failed
reconnect, and terminal-zero evidence pass.

Tactical [`089`](../tactical/089-coordinated-session-listen-sockets.md) is
complete. Schema version 11 adds the preferred port with default `6881`. One
allocator holds coordinated TCP and UDP sockets through a shared ten-conflict
retry budget and system fallback; one 64-datagram session UDP route feeds DHT.
Controlled loopback and eligible local-network traffic matches the separately
reported TCP listener and DHT UDP source, while fixed TCP failure retains
independent DHT service and all tasks join terminally. Tactical `097` now
applies that transport policy live.

Tactical
[`095`](../tactical/095-bounded-http-https-tracker-transport.md) is complete.
UDP, HTTP, and HTTPS rows now share the long-lived tracker schedule and exact
session-wide eight-operation ceiling. Bounded HTTP/1.1, gzip, redirects,
Basic authentication, compact/noncompact IPv4/IPv6 peers, AAAA-only tracker
connectivity, only-`peers6` outbound transfer, controlled libtorrent
introduction, and Android HTTPS product evidence pass. HTTPS is deliberately
encrypted but unauthenticated at that historical checkpoint. An opt-in
official Ubuntu smoke also reached both HTTPS rows and verified metadata.
Tactical `096` repairs the metadata-only activation gap exposed by that first
smoke; its repeat reached both HTTPS rows, verified metadata with no payload
artifacts, and projected each actual last-successful connection family.

Tactical
[`098`](../tactical/098-authenticated-https-tracker-platform-trust.md) is
complete. Schema version 12 defaults every profile to desktop or Android
platform trust, retains one hidden explicit compatibility value, and applies
it live through the existing session reconciler and bounded family client
pair. Generated certificate/name failures, construction fencing, captured
in-flight generations, redacted categories, cross-platform runtimes, Android
packaging/product behavior, and authenticated HTTPS tracker introduction to
pinned libtorrent all pass. The current AVD rejected the credential-free
Ubuntu tracker certificate while accepting another public trusted origin, so
that observation does not become a public-tracker reliability claim.

Tactical [`111`](../tactical/111-mse-peer-stream-encryption.md) is complete.
TCP initiator and responder roles support both negotiated MSE/PE payload
methods under one live persisted four-value policy, bounded session DH work,
exact diagnostics, and the truthful encrypted-or-obfuscated peer flag. All 28
controlled pinned-libtorrent cases, six paired 1 GiB performance runs per
implementation, Android cross-build, API 34 AVD, and API 37 physical Pixel 7a
product evidence pass. The physical run completed five forced-RC4 sessions,
exact publication, a three-job DH high-water under the four-job ceiling, full
owner drain, bounded storage/descriptors, and cleanup. The claim is protocol
compatibility and obfuscation, never transport security.
Completed bounded follow-up Tactical
[`115`](../tactical/115-mse-policy-advertisement-and-peer-detail.md) aligns
default incoming method selection with stock libtorrent, adds live-policy
HTTP tracker capability announcement, and exposes exact method detail through
the existing quiet `E` presentation. Its 29-case controlled matrix passes; it
adds no setting, Android Compose work, uTP support, or broader protocol claim.

Tactical
[`112`](../tactical/112-dual-stack-transport-and-ipv6-dht.md) is complete.
One session transport generation now owns an independent TCP/UDP pair per
enabled family; one DHT actor owns separate IPv4/IPv6 nodes and persisted
state; tracker and DHT advertisement select the same-family listener port; and
one default-enabled persisted policy gates all IPv6 ingress, discovery, and
dials. Controlled IPv6 DHT-only libtorrent discovery/download, a public
dual-family metadata run, web/Tauri contract coverage, both Android ABIs, and
an API 34 AVD degradation/restart profile pass. The named API 37 Pixel 7a
subsequently passed the same default, disable, forced-restart persistence,
degraded re-enable, and cleanup assertions on its current no-eligible-address
network. No IPv6 pinhole or incoming-reachability claim is made.

Tactical
[`114`](../tactical/114-session-wide-concurrent-torrent-admission.md) is
complete. Schema 17 persists one automatic download order and a default-three
active limit; the application restores every runnable intent and admits a
bounded active-generation map. Request, payload, active-piece, storage/hash,
tracker, outbound-turn, connection, and file-handle authority is session-wide.
One-/two-torrent performance gates pass, 1/2/3/4/8 saturation is recorded,
100 runnable and 500 complete catalog scales remain bounded, shared browser/
Tauri controls pass headlessly, and a physical Pixel 7a proves Android's
configured-three/effective-two clamp, promotion, exact payload, and terminal
resource cleanup.

Tactical
[`116`](../tactical/116-platform-storage-coherence-and-ios-feasibility.md) is
complete. Path and supported Android SAF storage now share logical artifact
geometry, typed observations, root-health admission, published reads, the
40-handle pool, and explicit namespace transitions. Full AVD and physical
Android matrices pass, including exact SAF-backed upload and cleanup. A
physical-iPhone harness proves the real Rust storage and direct-network seams,
bookmark restoration, coordination, and bounded lifecycle behavior for an
app-owned fixture; external File Provider support remains unproven. No fast-
resume trust decision was added.

Tactical
[`123`](../tactical/123-ios-on-device-root-persistence-and-recovery.md) is
complete with the accepted app-owned-only outcome. A versioned opaque probe
root, fail-closed eligibility state, per-operation coordination, exact partial-
workspace recovery, and balanced resource evidence pass on a physical iPhone.
The later dedicated-testbed run reaches both physical picker controls. iCloud
is rejected as ubiquitous, while a separate On My iPhone directory remains
unclassifiable after its public File Provider lookup fails; both report local
and internal volume flags. Picker registration is therefore still compiled
off and no provider support is claimed.

Tactical
[`117`](../tactical/117-jstorrent-shaped-android-product-ui.md) is complete.
The maintained Compose product now follows JSTorrent Android standalone's
Library, six-tab torrent detail, Speed, DHT, Logs, and Settings hierarchy with
RSTorrent branding. It consumes the existing bounded application views and
commands through one service-scoped presentation owner, adds bounded raw
`.torrent` intake and completed-file launch, and labels unsupported engine or
platform policy unavailable. Workspace, generated two-ABI, Gradle lint/unit,
API 34 Compose navigation, and controlled SAF/concurrent-download evidence
pass; no new physical-device UI claim was made.

Tactical
[`119`](../tactical/119-deterministic-utp-transport-core.md) is complete.
The dependency-free protocol crate now owns a hostile bounded uTP v1 codec,
explicit wrapping arithmetic, exact initiating and accepting connection IDs,
64-packet/1-MiB receive ordering, a 1,024-packet/1-MiB sent ledger,
cumulative/SACK acknowledgement and loss signals, Karn-safe RTT/RTO state,
bounded retransmission attempts, FIN close readiness, and terminal cleanup.
All 41 focused uTP tests and the complete Rust workspace baseline pass. This
is pure state evidence only: no RSTorrent uTP datagram, runtime stream,
interoperability, WAN path, product behavior, or support claim exists.

Tactical
[`121`](../tactical/121-deterministic-utp-loss-congestion-and-mtu.md) is
complete at its required pre-runtime review. The protocol crate now composes
exact receive credit, bounded stream packetization, delayed ACKs,
retransmission execution, fixed-point RFC 6817 congestion/pacing, and binary
path-MTU discovery. Fixed exact-hash scenarios pass clean, impaired, 1% loss,
queue, clock, receive-pressure, black-hole, and TCP-like foreground gates.
This remains deterministic state only: peer execution is still TCP, and no
RSTorrent uTP socket, task, ordered stream, interoperability exchange, WAN
path, product behavior, or support claim exists.

Tactical
[`125`](../tactical/125-shared-udp-utp-runtime-and-loopback-interop.md) is
complete at its required post-Stage 3 review. One shared session UDP receiver
now isolates bounded DHT/uTP routes; one supervised runtime owns generation-
fenced connections and ordered streams; and one concrete peer-stream boundary
feeds controlled plaintext uTP into common framing and incoming peer/upload
owners. Ten runtime cases, twelve shared-UDP cases, and both pinned-libtorrent
roles pass. Each role transfers the exact 2,097,883-byte fixture with one
loopback uTP peer, zero TCP peers, exact SHA-1, bounded high-waters, no drops or
worker panics, and terminal zero ownership. This adds no WAN evidence, product
selection/listener, UDP mapping, IPv6 uTP, MSE-over-uTP, or support claim.

Completed Tactical
[`127`](../tactical/127-mapped-utp-wan-interoperability.md) corrects closed
Tactical `126`'s overly narrow reachability premise. It treats `pimom` as an
authorized NATed Internet peer, establishes an isolated pinned libtorrent
oracle, and proves one exact RSTorrent-leecher transfer through a finite remote
UDP UPnP mapping over the direct public path. The 82.239-second run observes
one uTP peer, zero TCP peers, the exact 2,097,883-byte fixture and SHA-1,
bounded transport resources, and terminal zero ownership. Exact lease deletion
plus an independent audit prove zero mapping, process, and per-run artifact
residue. The local-mapping fallback was not needed; product uTP remains
disabled.

Tactical
[`120`](../tactical/120-per-torrent-trusting-fast-resume.md) is complete.
Ordinary coherent path and supported local SAF resumes now validate exact
per-torrent structure and trust only synchronized committed bits with zero
payload reads or hashes. Structural disagreement invokes the unchanged full
checker only for that torrent; unavailable or malformed ownership retains its
existing recovery state. Force and pending verification remain full. Three
checkpoint-death boundaries, a stable completed neighbor, 500 completed seeds
beside three downloads, same-length mutation plus Force, the complete pinned-
libtorrent oracle, both Android builds, and two API 34 AVD lifecycle gates
pass. No schema, setting, provider, physical-device, or protocol claim was
added.

Tactical
[`128`](../tactical/128-controlled-tcp-performance-diagnosis.md) is complete.
Its retained TCP-only loopback harness reproduces the sustained 1 GiB gap,
rejects storage-worker count, checkpoint sync, observation overhead, and
resumable semantics as primary causes, and isolates excessive storage intake
backlog. An 8 MiB control improved the 1 GiB/16 MiB-piece resumable plaintext
path from 332.9 to 394.4 MiB/s and cut storage-job high water from about 3,083
to 399; forced RC4 moved in the same direction. Exact hashes, one TCP/zero uTP
peer, bounded resources, cleanup, and alternating orders passed.

Tactical
[`132`](../tactical/132-utp-default-readiness-evidence.md) is complete at its
product-default review. The existing 1,000-record peer registry now owns
volatile endpoint uTP capability and five-minute-to-one-hour suppression;
joined transport outcomes, direct-TCP repeats, exact expiry recovery, and PEX
refresh pass without a second cache or task. The pinned-libtorrent application
suite again verifies incoming uTP, outgoing uTP, and TCP fallback with exact
content and terminal cleanup. One explicit Big Buck Bunny public profile
verified metadata in 2.862383 seconds while observing both TCP and uTP, fixed
548-byte MTU, bounded queues, no drops/panics, and terminal zero UDP/uTP
ownership. Shipped/default clients and the BEP 29 claim remain unchanged.

Tactical
[`133`](../tactical/133-utp-product-default-enablement.md) is complete. The
common durable and ephemeral application constructors now default to the
existing fixed-548 IPv4/plaintext `PreferUtp` policy, while explicit
`TcpOnly` diagnostics retain isolated TCP, Fast, and MSE coverage. Exact
pinned-libtorrent incoming uTP, outgoing uTP, and sequential TCP-fallback
application transfers, desktop/Android lifecycle tests, both Android native
builds, and complete repository gates pass. The bounded BEP 29 claim is now
**Partial**. At that checkpoint mapping, incoming-endpoint advertisement,
public incoming reachability, persisted presentation, IPv6, and MSE-over-uTP
remained absent; Tacticals `137` and `140` subsequently refine that state.

Tactical
[`134`](../tactical/134-hierarchical-transfer-rate-enforcement.md) is
complete. Durable live session and per-torrent upload/download limits now
compose in one torrent-first fair engine owner across initiated and accepted
TCP/uTP duplex I/O, including TCP MSE. Controlled unequal-peer fairness,
session/torrent caps, full duplex, exact hashes, terminal ownership, schema-18
restart/convergence, generated React/Compose controls, both Android builds,
headless Chrome, API 34 AVD, and complete repository gates pass. The policy
counts established peer-stream bytes; automatic network policy, total-device
accounting, and ratio/time seeding goals remain separate.

Tactical
[`135`](../tactical/135-controlled-tcp-storage-near-parity.md) is complete.
Desktop and Android now separate a 1 MiB hysteretic storage-intake watermark
from their larger resident safety ceilings. Hash reads execute in one bounded
fixed-buffer blocking task per physical span rather than per 16 KiB read.
Four-run TCP-only medians reach `1.146x` pinned libtorrent for plaintext,
`1.225x` for forced RC4, and `1.213x`--`1.336x` across the smaller-piece
matrix, with exact integrity, bounded resources, both Android builds, and
complete repository gates. Pending-write hash input remains unselected.

Tactical
[`136`](../tactical/136-shared-tracker-operation-executor.md) is complete.
One task-free UDP/HTTP/HTTPS operation executor now serves the separate
application and focused direct lifecycle owners. The direct path retains raw-
magnet HTTP(S), authenticated system trust, tracker IDs, common peer outcomes,
and bounded completed/stopped finalization. Scripted HTTP lifecycle/fallback/
cancellation, controlled pinned-libtorrent HTTPS, repository/web/Android
gates, and a bounded Ubuntu public dispatch rerun pass. That rerun later
stalled after six verified pieces, so it closes the tracker integration gap
without authorizing a peer-policy change from one changing-swarm sample.

Tactical
[`138`](../tactical/138-verified-http-file-serving.md) is complete. Verified
logical-file reads, bounded volatile capabilities, the shared gateway/Tauri
media router, and the React/Tauri Files `Open` action pass repository, web,
desktop, and Android compatibility gates. Android retains its native
complete-file open and starts no HTTP listener. Maintainer direction
subsequently reactivated Tactical `137` for end-to-end implementation; it is
complete with controlled desktop, Android AVD, and repository evidence.

Tactical
[`140`](../tactical/140-incoming-utp-reachability.md) is complete. One product
reachability owner independently maps the actual TCP and IPv4 UDP/uTP
listeners; tracker/BEP 10 advertisement remains TCP while IPv4 DHT uses the
explicit UDP/uTP endpoint. The additive UDP mapping status reaches every
generated first-party client. Controlled tracker-only TCP and DHT-only uTP,
both Android ABI builds, and the API 34 lifecycle gate pass. An explicitly
authorized physical continuation repaired wildcard UDP mapping eligibility
and the WAN harness, then proved an exact 2,097,883-byte product-owned public
incoming-uTP transfer over one uTP and zero TCP peers. Joined mapping deletion,
an independent absent inventory, and process/artifact cleanup pass. One
preceding mapped dial timed out cleanly, so repeatability remains unclaimed.

Tactical
[`139`](../tactical/139-incomplete-file-streaming-demand.md) is complete.
Compact current/ahead leases, verified active logical reads, bounded time-
critical peer scheduling, progressive full/range HTTP, exact publication
handoff, typed `streamable` eligibility, and the shared React/Tauri `Open`
path pass controlled pinned-libtorrent, repository, web, desktop, both Android
ABIs, and API 34 AVD gates. Android retains completed-file-only native open
and starts no HTTP listener.

Tactical
[`143`](../tactical/143-dual-identity-and-persistence-foundation.md) is
complete. One opaque stable owner and explicit full protocol aliases replace
the old hash-as-owner assumption across schema 19, artifacts, runtime,
generated clients, React, Tauri, and Android. Controlled v1 transfer,
persistence/crash, both-ABI build, and API-34 reset evidence passes while BEP
52 input and wire behavior remain absent at that foundation checkpoint.

Tactical
[`151`](../tactical/151-complete-source-pure-v2-runtime-vertical.md) is
complete. Strict complete local pure-v2 `.torrent` input now passes through
ordinary product intake, aligned selective path/SAF storage, streamed SHA-256
Merkle verification, durable have, restart/recheck, publication, verified
reads, active upload, and completed seeding. Controlled pinned-libtorrent
transfer passes in both roles with TCP, default uTP, forced RC4 MSE, tracker,
and DHT coverage. Browser lifecycle, platform storage, both Android ABIs, an
API 34 AVD, Tauri adapters, an unsigned iOS archive, recovery cases, exact
cleanup, and bounded-resource gates pass. The BEP 52 claim is Partial only for
the demonstrated pure-v2 subsets. Completed Tactical
[`155`](../tactical/155-v2-magnet-authenticated-hash-exchange.md) adds exact
`btmh` intake/export, SHA-256 metadata, authenticated hash messages 21--23,
hash-first selective payload, incomplete-candidate refetch, complete-file
reconstruction, leaf repair, and hash/payload service. Pinned libtorrent
passes in both magnet roles; production browser, Tauri, both Android ABIs,
API 34 SAF, and unsigned iOS gates pass. Completed Tactical
[`156`](../tactical/156-hybrid-dual-swarm-runtime-closure.md) adds strict
hybrid source and dual-topic magnet intake, atomic provisional reconciliation,
two exact discovery lanes, direct-v2 and negotiated-upgrade peer entry,
mandatory SHA-1 plus SHA-256 verification, restart/recheck, upload, and
seeding. Both pinned-libtorrent roles and entry lanes, exact tracker/DHT keys,
browser, desktop, Android API 34 SAF, iOS archive, resource, cleanup, and full
repository gates pass. Creation and broader BEP 52 behavior remain absent.

Tacticals [`147`](../tactical/147-ios-client-foundation-and-qualified-roots.md),
[`148`](../tactical/148-jstorrent-swiftui-product-surface.md),
[`149`](../tactical/149-ios-lifecycle-recovery-and-distribution-readiness.md),
and
[`152`](../tactical/152-ios-multifile-selected-root-coordination.md)
are complete. The maintained iOS 16+ product runs the Rust application service
in-process, uses app Documents or physically qualified selected on-device
folders, rejects iCloud and positively identified providers, directly adapts
the first-party JSTorrent SwiftUI surface with Search deferred, and owns
finite background/restart behavior plus cold/warm magnet and file handoff.
Controlled one-peer physical transfer, publication, restart, Force recheck,
preview, unavailable-root repair, exact managed cleanup, phone/iPad simulator
tests, notification opt-in, force-close and finite-background recovery, and
unsigned/development archives pass. No indefinite background, cloud-provider,
App Store, TestFlight, or public-release claim is made.

Tactical `152` closes the selected-root multifile defect exposed after those
three tacticals. Exact-target coordination passes deterministic sibling-lease
and three-handle release tests. A controlled cross-file physical transfer
publishes, survives restart and Force recheck, hands off a complete file, and
removes exactly. The repository Big Buck Bunny magnet completes 1,055 of
1,055 pieces and all 276,445,467 bytes from the public swarm, publishes its
three files, and plays the MP4 through Apple Files' system video presentation.
Completed Tactical
[`154`](../tactical/154-ios-truthful-progress-and-system-preview.md) reserves
100% and Finished for canonical Complete/Published state and changes **Open
using** to direct Apple Quick Look presentation. A second exact public-swarm
run reached 1,055/1,055, Published/Seeding, opened the available MP4 in one tap,
advanced system playback from 1:46 to 2:10, and removed the selected-root tree
exactly.

## Current Work

### Active

- Resume **Tactical `158`** and close the clean Windows and Linux x86_64
  signed replacement/relaunch evidence plus Windows firewall-consent
  characterization. Prove clean launch or bounded reset and payload safety,
  not retention of disposable `0.1.x` application state.

### Ready

- Execute **Tactical `192`** to turn the passing ephemeral OPAQUE/relay proof
  into one local production-shaped desktop/configured-headless owner-password
  path with durable authority, explicit lifecycle/recovery UX, automatic
  challenge-bound private-browser resume, complete authorization/circuit audit,
  a release-built browser profile and a separate loopback-only relay service.
  No external service, publication or supported remote capability is in scope;
  accounts, passkeys, delegated roles and remote media remain excluded.
- Execute **Tactical `191`** to replace hidden staging and managed publication
  with direct libtorrent-shaped final-path storage, existing-data recheck, a
  selective-boundary-only part file, plain exact data deletion, and one
  publication-free contract across path, Android SAF, iOS, and first-party
  clients.
- Declare the future first supported version and freeze its fresh application
  identities and persistence/API baseline only from that version forward.
  Complete changelog, privacy/support presentation, and the repeatable beta
  torrent cohort without migrating `0.1.x` state.

### Later

Completed cross-platform presubmit Tactical
[`159`](../tactical/159-cross-platform-presubmit-ci.md) provides credential-free
Rust/web, deterministic browser E2E, native desktop package, Android dual-ABI,
iOS simulator/archive, and short loopback-interoperability signal on every
`main` update and pull request. Signed package and updater artifacts plus
native desktop architecture breadth now pass the public `desktop-v0.1.0` and
`desktop-v0.1.1` releases. Public `desktop-v0.1.2` now also passes the complete
signed matrix and a bounded exact-DMG macOS arm64 launch/native-host check; one
installed macOS arm64 launch plus exact macOS arm64 and Linux arm64 cross-
version updates pass. Windows x86_64 updater replacement is proven under the
stated older-profile limitation. Remaining cross-platform clean-machine
installation and installed updater evidence,
mobile
emulator/device release runs, and broad interoperability remain separate gates.

Decision-complete measurement Tactical
[`153`](../tactical/153-wired-lan-utp-data-plane-scalability.md) remains ready
for a bounded wired gigabit-effective Mac-to-native-desktop LAN matrix. The
explicit 2026-08-22 beta-readiness priority supersedes its queue position, not
its measurement design or value.

Completed Tacticals
[`142`](../tactical/142-wan-transport-performance-matrix.md),
[`145`](../tactical/145-sustained-utp-reliability-and-throughput-near-parity.md),
and [`150`](../tactical/150-bounded-utp-sender-startup.md) retain the reusable
WAN lab, stable remote-seed 256 MiB near-parity cohort, and bounded 1 GiB scale
corroboration. The separate remote-placement RSTorrent TCP seed disconnect and
current-network local UPnP limitation remain typed evidence outside that
completed campaign.

Completed Tactical
[`156`](../tactical/156-hybrid-dual-swarm-runtime-closure.md) closes the strict
hybrid consumption/seeding campaign. First-party v2/hybrid torrent creation,
durable incomplete sparse-tree state, arbitrary Merkle base layers, broader
historical layout compatibility, and public-swarm reliability remain Later
and require their own bounded tactical before implementation.

Seeding goals and automatic network policy,
multi-interface and BEP 45 multi-address binding,
local service discovery,
NAT traversal, dynamic VPN and metered-network controls, and production
remote access remain important. Completed Tactical
[`190`](../tactical/190-opaque-wasm-relay-foundation.md) proves the controlled
OPAQUE native/Wasm dumb-relay composition without public exposure, durable
authority, browser resume, authorization audit or account delegation. Ready
Tactical
[`192`](../tactical/192-production-owner-relay-access.md) owns the narrower
local production-shaped password path, including bounded named-browser
authorization, automatic resume and owner security review, and proceeds
independently from Tactical `158`. A later separately authorized tactical must
own public relay/client deployment, DNS/TLS, external-path evidence, operations
and the first supported remote-access claim.
Tactical
`112` now owns IPv6 DHT operation and dual-stack
listening. Closed Tactical `113` implements IPv6 firewall-pinhole control but
records positive physical capability as unknown on the current hardware after
the live gateway returned typed `606` to `AddPinhole`.
The uTP topic records the adaptive campaign. Tacticals `118`, `119`, `121`,
`125`, `127`, `131`, and `132` are complete; bounded deterministic state, both
loopback roles, application composition, endpoint memory, and one remote-
mapped direct-public-path leecher transfer pass. Tactical
`126` remains the evidence-limited record of its superseded direct-interface
preflight. Tactical `130` is closed after proving the complementary mapped-WAN
direction, the fixed real-socket impairment matrix, hostile lifecycle bounds,
and controlled diagnostic-MTU convergence. Its WAN cohort remains evidence-
limited after two repaired diagnostic peer-wire gaps and two intermittent
cleaned-up local-send timeouts exhausted the external attempt budget. Tactical
`132` also records one successful ordinary-swarm metadata observation with
both TCP and uTP and complete cleanup. Completed Tactical `133` made the
bounded fixed-548 IPv4/plaintext path a product default and the BEP 29 claim
**Partial**.
Completed Tactical `137` supplies the shared-egress seam, safe
Linux/Android/macOS platform adapters, revalidation/downward recovery,
protected-send product runtime integration, controlled path, efficiency,
rate, and pinned-libtorrent application evidence. Both Android ABIs and the
API 34 AVD option/send/replacement/application/cleanup matrix pass. Completed
Tactical `140` adds independent product TCP/UDP mapping, transport-specific
tracker/DHT advertisement, controlled DHT-only incoming uTP, Android status
parity, and one positive product-owned public incoming-uTP transfer with zero-
residue cleanup. Completed Tactical `139` supplies bounded incomplete-file
stream demand, verified active reads, progressive HTTP, shared client
eligibility, controlled wire evidence, and proportional Android parity.
Tactical
[`100`](../tactical/100-bep53-select-only-and-duplicate-add-feedback.md)
completed the BEP 53 slice and its deliberately narrow duplicate-add product
policy. After core parity,
common-denominator versus full-reference deltas and the protocol evidence
matrix choose BEP breadth; visible novelty alone does not.

PCP and NAT-PMP require their own bounded tactical and suitable controlled or
physical gateway evidence; pinned source inspection is not a support claim.

Availability-ranked activation, the complete BEP 6 request lifecycle, and
bounded BEP 11 PEX are complete in Tacticals
[`091`](../tactical/091-availability-ranked-piece-activation.md) and
[`093`](../tactical/093-bep6-fast-request-lifecycle.md), and
[`094`](../tactical/094-bounded-bep11-peer-exchange.md). Full snub
and parole selection remain evidence-gated rather than preplanned slices.

## Capability Scoreboard

### Input, Identity, And Metadata

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded bencode and metainfo dictionaries | Implemented | deterministic, runtime, interop | Generic, 30-MiB peer BEP 9, and 64-MiB explicit/durable/local-upload profiles independently bound bytes, decoded items, depth, collections, files, pieces, paths, and trackers. Product v1, strict complete-source pure-v2/hybrid `.torrent`, and SHA-256-authenticated pure-v2/hybrid info-only ingestion pass; malformed or inconsistent hybrid input fails closed. | [`protocol-support`](protocol-support.md) |
| Product add from a magnet | Implemented subset | deterministic, persistence, runtime, interop, web, Android build, AVD, physical, live | Bounded v1 `btih`, exact pure-v2 hexadecimal `btmh:1220`, or matching dual-topic hybrid identity and supported fields survive canonicalization; malformed, conflicting, or metadata-inconsistent identity fails closed. A valid 255-byte `dn` remains a separate provisional list label across restart and verified metadata supersedes it; exact source text and tracker credentials stay outside routine views. Controlled v1 tracker/public, pure-v2 peer-hint selective, and hybrid single-/dual-topic paths activate ordinary discovery/acquisition and preserve exact source intent; authenticated pre-content duplicates reconcile into one owner. | [`client-persistence`](client-persistence.md), [`bittorrent-v2-and-hybrid`](bittorrent-v2-and-hybrid.md) |
| BEP 53 select-only magnet intent | Implemented | deterministic, persistence, runtime, oracle, web, Android build, AVD | Strict bounded `so` ranges remain compact before metadata and become a skipped default plus at most 4,096 wanted exceptions. Duplicate selection is additive; ordinary duplicates are typed no-ops. The pinned libtorrent magnet suite, maximum-span parser case, 4,097-file atomic rejection, restart/runtime fences, generated adapters, React reveal behavior, and pure-v2 browser/Android selective transfers pass. | [`protocol-support`](protocol-support.md), [`client-persistence`](client-persistence.md) |
| BEP 9 metadata download | Implemented | deterministic, runtime, interop, live | One bounded torrent owner assembles blocks across a 30-peer combined pending-dial/connected-worker cohort paced without burst at ten accepted attempts per second, accepts an authoritative piece-zero size up to 30 MiB, verifies exact SHA-1 and/or SHA-256 from typed identities, rejects a discovered format conflict, reconciles only authenticated pre-content hybrid owners, and recovers from expiry, rejection, and hash failure. Pinned libtorrent transfers the exact 31,457,280-byte maximum profile and pure-v2/hybrid info-only metadata in both roles. Tacticals `181` and `182` prove exact 30/31 admission, one-at-a-time saturated zero-contributor turnover with useful/sparse protection, and cancellation-to-zero. | [`peer-lifecycle`](peer-lifecycle.md), [`libtorrent-policy-alignment`](libtorrent-policy-alignment.md) |
| Bounded metadata upload | Implemented | deterministic, runtime, interop | The diagnostic server remains metadata-only; the application listener shares immutable registration-owned v1, pure-v2, or hybrid metadata across bounded incoming peers and serves every requested 16-KiB block of valid local metadata up to the 64-MiB profile. Hybrid v1 and v2 routes share the same owner and source bytes. | [`incoming-reachability-and-seeding`](incoming-reachability-and-seeding.md), [`peer-lifecycle`](peer-lifecycle.md) |
| Product add from a `.torrent` file | Implemented | deterministic, runtime, interop, web, Tauri, AVD | One atomic 64-MiB byte operation preserves exact source, operational info and tracker tiers across restart through HTTP, WebSocket, raw Tauri IPC, and native adapters. V1 and the strict complete-source pure-v2 and hybrid subsets pass. Empty Add opens the shared single-file chooser, reuses root/start options, sends selection `all`, and requires no caller digest or secure context. | [`application-control`](application-control.md) |
| Opaque torrent ownership and protocol identity foundation | Implemented | deterministic, persistence, runtime, interop, web, AVD | Schema 19 established full v1/v2 alias values, versioned wire-key lookup, owner/fingerprint-bound have and part state, retained exact sources, generated clients, and both Android ABIs. Tactical `179` carries that current shape into fresh schema 21 while resetting every recognized prior catalog. Production pure-v2 and hybrid source/magnet rows retain the full 32-byte v2 identity while versioned tracker, DHT, handshake, and MSE paths use the appropriate typed key. Atomic provisional hybrid reconciliation leaves one row with two aliases. | [`bittorrent-v2-and-hybrid`](bittorrent-v2-and-hybrid.md), [`143`](../tactical/143-dual-identity-and-persistence-foundation.md), [`151`](../tactical/151-complete-source-pure-v2-runtime-vertical.md), [`155`](../tactical/155-v2-magnet-authenticated-hash-exchange.md), [`156`](../tactical/156-hybrid-dual-swarm-runtime-closure.md) |
| v2 and hybrid metadata, hashing, and transfer | Partial | deterministic, runtime, interop, web, Tauri, Android build, AVD, iOS build | Exact-byte models, aligned file-local geometry, complete and volatile sparse hash knowledge, SHA-256 Merkle verification, selective path/SAF storage, restart/recheck, publication, leaf repair, upload, seeding, standard peer transfer, and versioned tracker/DHT/TCP/uTP/MSE routing pass for pure-v2 source/magnet input and strict hybrid source/single-/dual-topic magnet input. Hybrid payload requires both SHA-1 and SHA-256, internal padding is synthesized, and one owner serves direct-v2 plus negotiated/declined v1 routes. Both pinned-libtorrent roles/entry lanes, browser, Tauri, Android SAF, iOS archive, bounded-resource, and cleanup evidence pass. Creation, arbitrary Merkle base layers, durable incomplete sparse hashes, broader historical layouts, and public reliability remain absent. | [`bittorrent-v2-and-hybrid`](bittorrent-v2-and-hybrid.md), [`146`](../tactical/146-runtime-free-bep52-metainfo-geometry-merkle.md), [`151`](../tactical/151-complete-source-pure-v2-runtime-vertical.md), [`155`](../tactical/155-v2-magnet-authenticated-hash-exchange.md), [`156`](../tactical/156-hybrid-dual-swarm-runtime-closure.md), [`protocol-support`](protocol-support.md) |

### Discovery

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Explicit magnet peer hints | Implemented | deterministic, runtime, interop | Hints are bounded and feed the registry, but are not a general discovery mechanism. | [`peer-lifecycle`](peer-lifecycle.md) |
| Scheduled UDP tracker announces | Implemented | deterministic, runtime, interop, web, AVD, live | One long-lived session owner provides UDP connect/announce, fallback, backoff, retransmission, token reuse, interval/corrective reannounce, exact counters, started/completed/stopped lifecycle, the selected TCP endpoint or port-`1` sentinel, and an eight-operation ceiling shared with HTTP/HTTPS. A hybrid registration runs its exact v1 and v2 lanes under those same budgets with versioned diagnostics; controlled evidence observes one announce per key and uTP completion. Controlled tracker-only and mapped off-LAN discovery-to-seed evidence also passes. | [`tracker-discovery`](tracker-discovery.md) |
| Multiple magnet trackers | Partial | deterministic, runtime, interop, live | Up to eight startup operations contribute peers, but magnet trackers form one synthetic tier because magnets contain no BEP 12 tier structure. | [`tracker-discovery`](tracker-discovery.md) |
| Metainfo tracker tiers | Implemented | deterministic, runtime, interop, web, live | Outer `announce-list`/`announce`, tier and source survive restart. UDP/HTTP/HTTPS rows share the transport operation executor and each lifecycle owner's eight-operation schedule ceiling; controlled imported application and focused-direct trackers complete exact content. | [`tracker-discovery`](tracker-discovery.md) |
| HTTP and HTTPS trackers | Implemented | deterministic, runtime, interop, web, desktop, AVD, live | The long-lived application owner provides bounded HTTP/1.1 requests, Basic auth, five redirects, gzip/`x-gzip`, permissive hostile bencode, tracker IDs and BEP 31, compact/noncompact IPv4/IPv6 peers, policy/family DNS, lifecycle/cancellation, metadata-only activation, and connection-family projection. The focused resumable owner now uses the same task-free operation executor with system trust, tracker-ID continuation, mixed-transport fallback, cancellation, and completed/stopped lifecycle. Controlled libtorrent discovery/authenticated transfers, platform trust, and official Ubuntu HTTPS dispatch pass. One hidden application compatibility value remains encrypted but unauthenticated. Proxies, scrape, other authentication, custom roots/pins, and a public reliability claim are absent. | [`tracker-discovery`](tracker-discovery.md) |
| DHT | Partial | deterministic, runtime, interop, live | One bounded actor owns independent IPv4/IPv6 identities, routing, tokens, transactions, traversals, peer values, native-family bootstrap, warm state, incoming queries, private gating, merged product lookups, and family-port self-announcement. One session scheduler survives download completion; strict hybrid evidence shares bootstrap while independently looking up and announcing both exact keys and completes over uTP. Controlled DHT-only discovery passes in both families, mapped off-LAN IPv4 seed discovery passes, and a native public IPv6 node reached 40 routing nodes and 41 valid responses during successful merged metadata acquisition. Foreign-family bootstrap optimization, BEP 5 `PORT`, and incoming IPv6 reachability remain absent. | [`dht-discovery`](dht-discovery.md) |
| Peer exchange | Implemented | deterministic, runtime, interop | Verified-public BEP 11 uses bounded directional BEP 10 negotiation, 16-KiB/50-contact messages, 50-per-source and 200-per-torrent admission, a 4,096-event shared timeline, exact provenance/privacy cleanup, and the ordinary registry/dial owner. A controlled complementary two-hop pinned-libtorrent run captures one addition, an oracle-observed RSTorrent drop, and exact 16-MiB completion; underpopulated recent-peer exemptions, BEP 40, and durable PEX state remain absent. | [`peer-lifecycle`](peer-lifecycle.md), [`protocol-support`](protocol-support.md) |
| Local service discovery | Absent | none | Interface, multicast, and local-network policy are unimplemented. | [`protocol-support`](protocol-support.md) |

### Peer And Swarm Lifecycle

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded peer registry and source merging | Implemented | deterministic, runtime, interop | Records remain volatile and endpoint-keyed while the separate exact peer-ID admission index permits at most one established generation per claimed remote ID. Crossed, same-direction, self, stale-removal, saturation, and pinned-libtorrent cases pass without merging provenance or reputation. | [`peer-lifecycle`](peer-lifecycle.md) |
| Registry-backed Swarm inspection | Implemented | deterministic, runtime, interop, web, Android build, installed service | The bounded volatile registry, exact state counts, source merging, retry eligibility, terminal cleanup, typed self/duplicate closure reasons, and exact payload downloaded/uploaded across retained active, disconnected, backed-off, and reconnected generations are visible. Counters reset on process restart or record eviction; durable history remains absent. | [`peer-lifecycle`](peer-lifecycle.md), [`application-view-api`](application-view-api.md), [`175`](../tactical/175-retained-swarm-peer-transfer-totals.md) |
| Deterministic dial selection and guarded attempts | Implemented | deterministic, runtime, interop | Ordinary selection retains the three-failure ceiling. Tactical `177` trusts only exact tracker refresh to rehabilitate one failure, and only a completely dry content swarm may spend one existing turn/slot on an expired transient-failure record under a 5-to-60-minute torrent cadence. Definite/integrity failures remain excluded; a scripted fourth attempt completes after three handshake failures. Post-handshake peer-ID admission still resolves crossed/repeated connections without treating IDs as durable identity, and Tactical `132` retains bounded uTP capability suppression/recovery. | [`peer-lifecycle`](peer-lifecycle.md), [`177`](../tactical/177-bounded-dry-swarm-recovery.md) |
| Pre-content peer failover | Implemented | deterministic, runtime, interop, live | Bounded parallel metadata peers share one block owner; two tracker cohorts, 10/10 fresh-DHT owner runs, and 12/12 cross-catalog pairs pass. | [`peer-lifecycle`](peer-lifecycle.md) |
| Multiple simultaneous live peers | Implemented | deterministic, runtime, interop, live | Thirty established and thirty half-open attempts remain separate outbound torrent-local defaults beneath one shared session budget whose ordinary default is 200 after descriptor clamping and whose incoming-only slack is ten. Exact saturation, cancellation, mixed-direction release, and simultaneous incoming evidence pass. | [`peer-lifecycle`](peer-lifecycle.md) |
| Transfer request ownership and failover | Implemented | deterministic, runtime, interop, live | Ordinary blocks have one generation; strict endgame adds bounded duplicate attempts, first-response cancellation, and harmless losing payload. | [`download-correctness`](download-correctness.md) |
| BEP 6 Fast request lifecycle | Implemented | deterministic, scripted runtime, interop | Bilateral negotiation, exact initial availability, choke-retained requests, exact reject/refill, terminal upload responses, 32-entry advisory bounds, equal-rarity suggestion bias, and canonical ten-entry IPv4 allowed-fast generation pass. Controlled capture verifies both pinned-libtorrent directions and exact 40,000-byte payload hashes; predictive requests, super-seeding, and an invented IPv6 set remain absent. | [`protocol-support`](protocol-support.md), [`download-correctness`](download-correctness.md) |
| Incoming peer connections | Implemented | deterministic, runtime, interop, web | One bounded incoming owner accepts independently bound IPv4 and eligible global-unicast IPv6 listeners, each with a five-entry backlog, under eight pending handshake slots, 1,024 generation-fenced registrations, and the shared effective-plus-ten-slack connection budget. Ordinary automatic/fixed settings still describe one preferred numeric port; each family independently resolves a coordinated TCP/UDP pair and a failed family leaves its sibling serving. The default-enabled persisted IPv6 policy applies live and closes plaintext and MSE IPv6 generations before `Applied`. Existing evidence proves mapped off-LAN IPv4 seeding, live candidate-first replacement, truthful family advertisement, and terminal cleanup. Tactical `113` implements one independent finite-lease IPv6 firewall-pinhole slot and typed product status under the same reachability coordinator; deterministic and scripted-gateway evidence pass. Its live negative control passes, but the observed gateway rejects `AddPinhole` with typed `606`, so no physical off-network incoming IPv6 or cleanup claim is made. | [`incoming-reachability-and-seeding`](incoming-reachability-and-seeding.md), [`peer-lifecycle`](peer-lifecycle.md) |
| uTP peer transport | Partial | deterministic, runtime, interop, live | Tacticals `119` and `121` prove the bounded v1 wire, reliability, receive, RFC 6817 congestion/pacing, and path-MTU state. Tactical `125` adds bounded shared DHT/uTP routing, generation-fenced runtime/stream ownership, peer-I/O composition, and exact pinned-libtorrent loopback transfers in both roles. Tacticals `127` and `130` prove both first-sample mapped-public-path directions, a six-profile real-socket matrix, hostile lifecycle bounds, and diagnostic convergence to a 1,269-byte floor under a controlled 1,280-byte black hole. Tacticals `131` and `132` add ordinary application composition, endpoint capability memory, suppression/backoff, PEX refresh, exact expiry recovery, and one ordinary-swarm metadata observation with both transports. Completed Tactical `133` makes the fixed-548 IPv4/plaintext `PreferUtp` policy the common application construction default; explicit `TcpOnly` retains TCP/Fast/MSE isolation. Completed Tactical `137` supplies safe Linux/Android/macOS fragmentation-protected sends, revalidation/downward recovery, dynamic product packetization, and fixed fallback. Controlled 1,500/1,280 paths select 1,457/1,269 bytes, five alternating pairs reduce median DATA packets 62.97%, and the exact capped pinned-libtorrent application gate passes in both roles. Tactical `140` independently maps the product TCP and UDP/uTP listeners, keeps trackers on TCP, selects the explicit IPv4 UDP/uTP endpoint for DHT, exposes both mapping states to first-party clients, proves controlled DHT-only incoming uTP plus Android lifecycle parity, and completes one exact product-owned public incoming-uTP transfer with zero TCP masking and zero-residue cleanup. Tacticals `142`, `145`, and `150` add the repeatable cross-engine WAN lab, repair sustained reliability, packetization, receive, and bounded sender-startup defects, and complete a stable 24-cell remote-seed 256 MiB cohort. Every RSTorrent uTP median reaches 94.85%--100.74% of the matched oracle and at least 98.49% of its own TCP median on one connection; 14 exact 1 GiB cells corroborate scale. Persisted transport policy, public-DHT discovery over the mapped endpoint, repeated reverse-direction evidence, IPv6 uTP, MSE-over-uTP, and racing remain absent. | [`utp-transport-campaign`](utp-transport-campaign.md), [`protocol-support`](protocol-support.md) |
| Peer reputation and integrity attribution | Partial | deterministic, runtime, live | Exact connection generations receive bounded asymmetric trust; a sole corrupt source is banned and ambiguous sources are only suspected, while full parole selection and persistence are absent. | [`download-correctness`](download-correctness.md) |

### Content Transfer And Completion

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Bounded 16 KiB block pipeline | Implemented | deterministic, runtime, interop, live, physical | Per-connection depth adapts under distinct local bounds while session request/payload/active-piece totals remain 256 MiB/32 MiB/256 MiB on desktop and 128 MiB/16 MiB/128 MiB on Android. Fair generation-scoped admission prevents active torrent count from multiplying them. | [`download-correctness`](download-correctness.md) |
| Sequential multi-piece download | Implemented | deterministic, runtime, interop | BEP 3 `length`, one-entry `files`, and ordinary multi-file torrents share one download, durable resume, repair, and publication pipeline. | [`download-correctness`](download-correctness.md) |
| Availability-aware piece selection | Implemented | deterministic, runtime, interop, performance | Requestable active work remains first; exact live nonseed counts plus a separate seed count feed a compact incrementally indexed rarest-first default with an in-order baseline. Durable High/Normal priority composes with rarity through the pinned libtorrent weighted key and updates picker plus v2 hash order live. Bounded transient stream demand remains the stronger overlay with current-before-ahead scheduling, safe ordinary-work preemption, peer queue estimates, and one adaptive duplicate. Independent count, byte, peer-ratio, and block-pressure limits pass hostile maximum-geometry and release CPU/memory gates; unique unplanned pieces remain protected. Player-supplied deadlines, raw piece-priority controls, reverse rarity for snubbed peers, and parole remain absent. | [`download-correctness`](download-correctness.md), [`http-file-serving-and-streaming`](http-file-serving-and-streaming.md) |
| Choke recovery | Implemented | deterministic, runtime, interop | Requests move to another peer and full choked sets are replaceable; mature choking/reputation policy is absent. | [`download-correctness`](download-correctness.md) |
| Per-request timeout and slow-peer handling | Implemented | deterministic, runtime | Useful response samples derive a bounded inactivity deadline and reduce a stalled peer to one probe; broader snub reputation remains absent. | [`download-correctness`](download-correctness.md) |
| Endgame | Implemented | deterministic, runtime, live | Strict duplicates, core cancels, late-loss safety, exact accounting, and public verified publication pass; throughput parity remains open. | [`download-correctness`](download-correctness.md) |
| Hash-failure recovery | Implemented subset | deterministic, runtime, interop, live | A failed v1 generation resets the whole piece with bounded contributors. A pure-v2 or hybrid generation obtains authenticated leaf proofs, retains exact good blocks/contributors, and refetches only bad blocks; reject or stall falls back to whole-piece reset. Hybrid have requires both schemes, and a one-scheme pass is a typed terminal inconsistency. Full parole selection remains absent. | [`download-correctness`](download-correctness.md) |
| Reliable completion on ordinary swarms | Partial | deterministic, runtime, interop, live | Multi-peer liveness, endgame, corrupt-generation retry, and bounded storage completion pass, but completion latency is not yet comparable and public corruption was not induced. | [`download-correctness`](download-correctness.md) |
| Payload upload and seeding | Implemented | deterministic, runtime, interop, web, AVD, physical | Published and active incomplete torrents serve exact verified/readable availability and bounded 16-KiB requests through initiated or accepted TCP/uTP peers under the shared live 0--50 slot, ten-read, 40-handle, writer, and hierarchical rate bounds. Complementary RSTorrent/libtorrent ordinary, Fast, forced-MSE, cross-file, part-backed, rate-limited full-duplex, and API 34 SAF transfers capture Piece frames in both directions before completion and independently verify every final hash. Active routed torrents advertise the real tracker/DHT port with nonzero `left`; failure and lifecycle changes retract or replace authority before stale reads. Exact completed-seed local, mapped off-LAN, AVD, and physical evidence remains. Ratio/time goals and discovery-driven public incomplete-swarm reliability remain absent. | [`incoming-reachability-and-seeding`](incoming-reachability-and-seeding.md), [`protocol-support`](protocol-support.md) |
| Hierarchical peer-transfer rate limits | Implemented | deterministic, persistence, runtime, interop, web, AVD | Semantic Unlimited or bounded upload/download limits compose at session and torrent levels across initiated and accepted TCP/uTP plaintext and TCP MSE streams. One torrent-first fair owner bounds grants, bursts, registrations, and waits; excludes deliberate throttling from network-stall clocks; applies live without replacing peer generations; and terminates empty. Schema-18 restart, unequal three-peer/one-peer fairness, session/torrent cap, full-duplex pinned-libtorrent, responsive React/Axe, both Android builds, and API 34 limited concurrent-transfer gates pass. Tactical `180` replaces the old forced pair/whole-client commands with independent typed patches and makes web/Compose drafts converge by receipt revision under complete cloned updates and resets; controlled 8 MiB listener handover and bind recovery pass. The scope is established peer-stream bytes, not total-device traffic; network automation, generic weights/classes, and seeding goals remain separate. | [`application-control`](application-control.md), [`settings-mutation-and-draft-consistency`](settings-mutation-and-draft-consistency.md), [`performance-and-live-evidence`](performance-and-live-evidence.md) |

### Integrity, Storage, And Resume

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| SHA-1 piece verification before have state | Implemented | deterministic, runtime, interop | Failure resets only the attempted v1 piece and preserves unrelated verified state. | [`download-correctness`](download-correctness.md) |
| Multi-file mapping and selective files | Implemented | deterministic, persistence, runtime, interop, web, Android build | Path and dynamic-SAF High/Normal/Skip routing, sparse current-schema priority persistence, lazy part storage, boundary materialization, metadata-only intake, weighted picker and v2 hash ordering, and live generation-preserving updates pass. Tactical `176`'s historical schema-19 retention migration is superseded by Tactical `179`'s fresh schema-21 epoch and recognized-incubation reset. Low and raw numeric priority controls remain absent. | [`client-persistence`](client-persistence.md), [`download-correctness`](download-correctness.md), [`android-saf-storage`](android-saf-storage.md) |
| Cross-file, skipped-file, and padding storage | Implemented | deterministic, runtime, interop, web | Lazy part creation, retained lowered destinations, route-epoch promotion/demotion, exact verified-span export, uncertain boundary-piece invalidation, and empty-part cleanup pass; BEP 47 symlinks are deliberately rejected. | [`client-persistence`](client-persistence.md) |
| Direct final-path storage | Accepted, not implemented | source/test dossier, tactical | Tactical `191` removes hidden full-payload staging and publication state, writes wanted bytes at final metainfo paths, reuses checked existing data, retains only selective-boundary part storage, and makes completed wanted files independently usable. The implemented product still follows the superseded staging/publication model until that tactical lands. | [`direct-filesystem-storage`](direct-filesystem-storage.md), [`191`](../tactical/191-direct-filesystem-storage.md) |
| Path-backed staging and publication | Implemented, superseded direction | deterministic, runtime, interop | Explicit file/tree topology, hash-owned internal artifacts, durable publishing intent, atomic no-replace rename, namespace sync, crash reconciliation, and fail-closed removal pass. This remains current code but is rejected as the future product model and is removed by Tactical `191`, not retained as an option. | [`direct-filesystem-storage`](direct-filesystem-storage.md), [`client-persistence`](client-persistence.md), [`download-roots`](download-roots.md) |
| Bounded asynchronous content storage | Implemented | deterministic, runtime, interop, live, physical | Payload sync and batched SQLite checkpoints use a separate bounded joined owner; immutable positional writes and fixed-buffer per-span hashes execute with independent session totals, root/torrent fairness, explicit generation joins, a 1 MiB intake watermark, and the shared 40-handle pool. Controlled TCP plaintext/RC4 throughput exceeds pinned libtorrent across 256 KiB--16 MiB pieces; multi-torrent/root isolation and physical Android concurrency pass. Broader provider/root performance remains open. | [`storage-throughput-architecture`](storage-throughput-architecture.md), [`performance-and-live-evidence`](performance-and-live-evidence.md) |
| Android SAF storage and publication | Implemented, superseded direction | deterministic, runtime, interop, AVD, physical | The product uses lazy dynamic acquisition and one 40-handle path/SAF pool. Typed observations gate root health, trusting ordinary resume, active and published reads, and provider repair; fixed manifests are diagnostic-only. Tactical `139` reuses the active logical-range owner and cross-builds its scheduler/storage semantics while Android retains completed-file-only presentation. The API 34 partial-state profile fails closed on grant loss, repairs, exchanges complementary Fast payload, and removes exactly at 7/40 handles and 2/16 pending requests. Earlier trusting-resume and complete physical matrices retain download, selection, checking, publication, upload, cancellation, concurrency, and cleanup coverage. Tactical `191` must preserve the capability and bounds while replacing provider publication/rename with direct final-document I/O. General root management, cloud/removable policy, migration, and relocation remain absent. | [`direct-filesystem-storage`](direct-filesystem-storage.md), [`android-saf-storage`](android-saf-storage.md), [`client-persistence`](client-persistence.md) |
| Durable have state and per-torrent resume | Implemented | deterministic, persistence, runtime, interop, web, AVD, physical | Schema 14 stores one payload fact and generation-fenced verification evidence. Exact ordinary path/SAF structure admits only synchronized committed bits with zero payload reads/hashes; disagreement invokes the full selection-independent checker only for that torrent, malformed state cannot abort profile open, and Force always hashes. Same-length external mutation is deliberately outside ordinary detection. | [`client-persistence`](client-persistence.md), [`download-correctness`](download-correctness.md) |
| Recovery after content hash failure | Implemented | deterministic, runtime | Sole corrupt and ambiguous multi-source generations retry cleanly with bounded exact-generation attribution. | [`download-correctness`](download-correctness.md) |

### Application And Product Surfaces

| Capability | State | Evidence | Highest-risk limit | Owner |
| --- | --- | --- | --- | --- |
| Durable semantic application control | Implemented | deterministic, persistence, runtime, interop, web, Tauri, Android build, physical | Archive, fenced keep/delete removal, metadata-only add, atomic v1 torrent-byte add, serialized live High/Normal/Skip file priority, retained checker pause/resume, atomic `Download now`, queue movement, automatic concurrent admission, and exact-or-synthesized magnet export are implemented; stable public compatibility remains absent. | [`application-control`](application-control.md) |
| Ephemeral application state | Implemented | deterministic, runtime | Private bounded session and metrics SQLite stores preserve receipts, exact source, metadata, settings, views, DHT and speed state for one joined service lifetime, then disappear without profile files. One maximum source plus info fits the 256-MiB session cap and a second maximum import rolls back with a typed resource limit; payload storage remains external. | [`client-persistence`](client-persistence.md), [`application-control`](application-control.md) |
| Leased application view sets and delivery clients | Implemented | deterministic, runtime, interop, web, Tauri | Named summary, generation-scoped checker progress, piece, structured diagnostic, active-peer, registry-backed Swarm, paged file and tracker, separately leased derived media, global Disk, range-selected session Speed, and latest-value session DHT views have bounded replay/reset, independent lease expiry, fresh-snapshot recovery, diagnostic HTTP polling, acknowledged browser WebSocket streaming, and acknowledged in-process Tauri streaming. The retained observer matrices still expose Summary reset storms and trace/all-view serialization pressure; stable public compatibility remains unimplemented. | [`application-view-api`](application-view-api.md), [`application-connection-architecture`](application-connection-architecture.md) |
| Shared web and Tauri desktop UI | Partial | runtime, interop, web, desktop | The responsive surface now has Library, Transfers, and Workbench destinations, truthful bounded torrent-backed cards with exact metadata-backed total size, a media-first Library torrent detail with typed numeric episode sorting, exact per-file progress, accessible Play for verified or active-streamable video through the existing ephemeral media capability, and an All files fallback, verified-name then provisional magnet-name presentation, accessible determinate/indeterminate checker progress with exact selected-summary counters, shared multi-selection, magnet and local `.torrent` add, source-preserving or name/tracker-rich bounded magnet copy, metadata-only add, live High/Normal/Skip file actions plus atomic `Download now` for skipped targets, verified and active-streamable file `Open`, archives, guarded removal, live peer/swarm/file/tracker inspection, global Disk pressure, bounded Canvas Pieces, a smooth exact session Speed history, a one-second download/upload tab title, and the exact routing-space DHT observatory. Exact Crostini hosting also adds capability-gated Linux-versus-ChromeOS storage guidance without changing other surfaces. Embedded playback, thumbnails/artwork, and Library-wide semantic aggregation remain incomplete. | [`client-surfaces`](client-surfaces.md), [`application-interface-direction`](application-interface-direction.md) |
| Desktop and ChromeOS extension bootstrap | Implemented | deterministic, desktop, physical ChromeOS | Tactical `166` adds the distinct bounded `com.jstorrent.rstorrent.native` compatibility/launch host, exact production and beta-extension origins, registration repair, sidecar packaging, and the Manifest V3 JSTorrent Beta seed. Tactical `167` adds the exact local Crostini handoff; Tactical `168` makes version `0.3.0` platform-aware. Permissions remain only `nativeMessaging` and `storage`: desktop gets native bootstrap, ChromeOS gets the exact published JSTorrent Android listing plus ChromeOS Linux, and unknown platforms retain both. Deterministic package gates, installed macOS native `hello`/cold launch, and physical ChromeOS chooser/Play-link/Crostini handoff pass. Hosted Windows/Linux package evidence and every torrent-control transport remain open later breadth. | [`client-surfaces`](client-surfaces.md), [`product-surfaces-and-migration`](product-surfaces-and-migration.md), [`beta-release-readiness`](beta-release-readiness.md) |
| ChromeOS Crostini bundled web launcher | Implemented | deterministic, runtime, web, physical, release | Tactical `167` packages the Rust backend and mature React UI behind one exact-authority same-origin gateway, static on-demand user service, mapped Linux Launcher, and exact beta-extension handoff. On the physical x86_64 Chromebook, warm and twice-stopped-VM launch retain one service/listener/UI, an active controlled transfer survives UI detachment, and normal uninstall/reinstall plus explicit purge preserve and remove only the specified data. Tactical `169` adds the exact-updater-key bootstrap, strict two-architecture manifest/release workflow, and physical signed-fixture matrix. Public non-latest `crostini-v0.1.0` now passes native x86_64/ARM64 builds, independent signed-asset validation, and the exact website install/Launcher/relaunch path on physical x86_64. Tactical `178` adds exact-product storage guidance backed by five paired physical trials: Linux Downloads remains the faster default and shared ChromeOS Downloads is an explicit convenience option. Full reboot, physical native ARM64, updating/rollback, suspend guarantees, Android-versus-Crostini comparison, and broader hardware performance distributions remain later breadth. | [`client-surfaces`](client-surfaces.md), [`product-surfaces-and-migration`](product-surfaces-and-migration.md), [`beta-release-readiness`](beta-release-readiness.md) |
| Linux configured headless service | Implemented | deterministic, runtime, interop, web, Linux VM, installed Linux, physical Android | Tactical `170` packages one ordinary-user application/profile owner, exact React assets, strict versioned root/listener/origin/auth configuration, and a disabled-by-default systemd user unit with rollback-safe repair and preservation-safe uninstall. Isolated HTTPS/WSS and real x86_64 Ubuntu evidence prove local pairing, exact hosted Basic/Host/Origin rejection, an 8-MiB 128-piece transfer with all views detached, completed re-seeding to pinned libtorrent, idle reachability, missing-root safety, joined restart, repair, uninstall preservation, and exact cleanup. Tactical `171` adds strict signed `headless-v*` source workflow/bootstrap/check/apply machinery and exact RFC 1918 `lan-none`; a physical Android phone reaches that exact LAN service. Tactical `174` upgrades the current machine to byte-identical x86_64 package `0.1.1`, retains `192.168.1.129:3030`, and adds one exact loopback gateway behind a dedicated tailnet-only Tailscale Serve HTTPS authority. One PID/application/media owner serves both, endpoint-local Host/Origin rejection and origin-correct real WSS media calls pass, a phone-sized tailnet HTTPS/WSS browser smoke persists its one-time full-owner notice dismissal, and same-version repair probes both endpoints. No wildcard, direct Tailscale-interface bind, Funnel, ACL mutation, or public headless channel exists. The native ARM64 workflow exists, while physical off-LAN phone, physical ARM64 systemd/update, built-in owner E2E authentication, reboot/mount, and long-run claims remain absent. | [`runtime-configurations-and-headless-deployment`](runtime-configurations-and-headless-deployment.md), [`client-surfaces`](client-surfaces.md), [`beta-release-readiness`](beta-release-readiness.md) |
| Desktop native notifications | Implemented | deterministic, web, desktop | Completed Tactical `164` adds one Rust-owned authoritative torrent-list edge reducer, versioned Tauri-only settings, and native completion plus fatal/repair attention. Initial/reset/settings/restart terminal state does not replay, focused-window display is default-on, and hidden-to-tray delivery passes installed macOS arm64, Windows x86_64, and Linux arm64. The exact standard Tauri package owns macOS/Windows; a bounded direct adapter retains the same underlying Linux notification handle because the package wrapper dropped it. Linux click restores the existing window; macOS/Windows retain tray restoration after measured package click limits. Progress, aggregation, and mobile work remain excluded. | [`client-surfaces`](client-surfaces.md), [`beta-release-readiness`](beta-release-readiness.md) |
| Active-work sleep inhibition | Implemented | deterministic, web, desktop, Android build, physical | Completed Tactical `165` adds one default-on desktop/Android preference driven by authoritative `Starting`, `Downloading`, and `Checking` states. macOS/Windows use exact `keepawake` 0.6.1; GNOME uses its suspend inhibitor and other Linux sessions use a bounded XDG portal fallback. Android retains one partial CPU wake lock and removes its Wi-Fi lock. Installed macOS arm64, Windows arm64, Linux arm64, native Windows x86_64, and physical Android API 37 prove held, minimized/screen-off, preference, pause/restart, Start, and cleanup transitions without display inhibition. Physical iOS retains finite-background behavior and exposes no false keep-awake control. The unsigned native x86_64 preflight leaves package trust and the integrated signed update repeat to Tactical `158`. | [`client-surfaces`](client-surfaces.md), [`beta-release-readiness`](beta-release-readiness.md) |
| Authenticated private web host | Implemented | deterministic, runtime, web, live | One explicitly configured maintainer host serves the production React bundle and multiplexed application WebSocket behind bounded Basic authentication and exact HTTPS Origin checks. Exact-push isolated build, candidate smoke, supervised restart, authenticated private-listener/public verification, and rollback-on-failure pass; this is not a relay, account, pairing, encryption, or stable public compatibility claim. | [`application-connection-architecture`](application-connection-architecture.md), [`client-surfaces`](client-surfaces.md) |
| Local headless web authentication | Implemented | deterministic, runtime, web | Fresh loopback profiles have a communicated ten-minute setup choice between local-open and at most 32 rolling remembered-browser sessions. Four-digit one-use approval, five-attempt exhaustion, HttpOnly Strict cookies, exact Host/Origin checks, Settings revocation, typed live-socket termination, restart persistence, and explicit one-browser recovery pass. This is not password, LAN, relay, device-identity, or E2E remote authentication. | [`application-connection-architecture`](application-connection-architecture.md), [`web-ui-design`](web-ui-design.md), [`remote-access-authentication`](remote-access-authentication.md) |
| Controlled owner-password relay foundation | Proven, not a product capability | deterministic, runtime, web, real browser | Tactical `190` selects RFC 9807 OPAQUE Ristretto255/SHA-512/3DH, measured Argon2id, one native/Wasm record core, blocking host pin, bounded local dumb relay and proof-only native host. Real Chrome provisions and runs the existing React negotiation/snapshot/view update/ack/benign-call trace identically to direct delivery; pin, password, unknown-route and modified-handshake failures plus zero-owner cleanup pass. Authority is ephemeral, every reconnect needs the password, the relay is local/unoperated, bulk/media are rejected, and no product mode is enabled. Tactical `192` owns the durable production-shaped local composition plus required authorized-browser resume, revocation and security audit; deployment remains later. | [`remote-access-authentication`](remote-access-authentication.md), [`application-connection-architecture`](application-connection-architecture.md), [`190`](../tactical/190-opaque-wasm-relay-foundation.md) |
| Android Compose foreground client | Implemented | deterministic, runtime, Android build, AVD, physical | The maintained Material 3 product provides the JSTorrent-shaped Library, six-tab torrent detail, Speed, dual-family DHT, structured Logs, and Settings hierarchy with RSTorrent branding. One service-scoped owner consumes every Android-relevant bounded projection; magnet and `.torrent` intake, SAF setup/repair, High/Normal/Skip file priority/open, torrent and queue actions, backed settings including session/per-torrent transfer limits and active-work sleep inhibition, activity/process recovery, and controlled concurrent downloads pass. Search/plugins, playback, tracker mutation, and dynamic network policy remain explicitly unavailable; Tactical `117` makes no new physical-device UI claim. | [`client-surfaces`](client-surfaces.md) |
| iOS native client | Implemented | deterministic, runtime, interop, simulator, physical, live | The maintained iOS 16+ SwiftUI product runs the application service in-process through generated Swift UniFFI. App Documents and qualified on-device selected roots support controlled single- and multifile transfer, exact-target coordinated descriptors, publication, restart/Force recheck, complete-file handoff, and managed cleanup. Progress reserves 100%/Finished for Complete/Published. The exact three-file Big Buck Bunny public magnet completed 1,055/1,055 pieces and 276,445,467 bytes, published, and opened in one tap through Apple Quick Look; native video playback advanced before exact cleanup. The directly adapted JSTorrent surface, lifecycle, intake, phone/iPad layouts, and archives remain implemented. iCloud/identified providers, indefinite background work, embedded/progressive playback, migration, and public distribution remain absent. | [`product-direction`](product-direction.md), [`client-surfaces`](client-surfaces.md), [`download-roots`](download-roots.md) |
| Derived progress, torrent ETA, and bounded diagnostics | Implemented | deterministic, runtime, interop, web, AVD | Progress remains an application projection. Selection-aware torrent ETA adds exact required/remaining non-padding peer work, a 184-byte scalar model, one shared cadence, and typed warming/estimate/stalled/unavailable presentation. Exact complete torrent size now derives from verified content geometry and renders across the shared web destinations; file ETA and the broader Size/Progress repair remain absent. Structured hierarchical diagnostics, typed context, capture interest, explicit source/delivery/local loss, and the global ordered console are complete. | [`application-control`](application-control.md), [`application-view-api`](application-view-api.md), [`download-correctness`](download-correctness.md) |
| Offline, loopback-only, and online egress policy | Implemented | deterministic, runtime, web, AVD | Policy is fixed for one service lifetime; Android VPN and metered-network controls are absent. | [`application-control`](application-control.md) |
| Headless product validation | Implemented | web, AVD, Linux VM | Temporary browser/AVD harnesses and the installed configured-Linux service prove real presentation detachment without launching Tauri; physical devices and visible desktop automation still require explicit authorization. | [`client-surfaces`](client-surfaces.md) |
| Comparative live performance harness | Implemented | deterministic, interop, web, live | Named hardware profiles retain row-specific 1/10 GiB engine gates, per-view/adversarial application ratios, environment applicability, and artifact-producing CI. The schema-v2 public comparator adds isolated RSTorrent/libtorrent workers, matched plaintext/RC4 profiles, exact metainfo, independent verification, process resources, atomic owner checkpoints, bounded cleanup, and discovery-versus-active-transfer timing. Its first quick run found Big Buck Bunny's libtorrent active phase about 19% faster and exposed the focused HTTP(S) gap. The post-fix Ubuntu rerun received two tracker batches and verified six pieces before a later 120-second stall, closing dispatch without yielding a throughput ratio. Public speed remains a distribution rather than a CI threshold. | [`performance-and-live-evidence`](performance-and-live-evidence.md) |
| Multi-torrent queue and resource budgets | Implemented | deterministic, persistence, runtime, interop, web, physical | Schema 17 stores automatic queue order and configured limit; desktop defaults to three and Android clamps effectively to two. One application owner admits exact generations under shared memory, storage/hash, tracker, outbound, peer, file-handle, and hierarchical transfer-rate ceilings. Controlled performance gates, 100-runnable/500-complete scale, headless queue/settings actions, and physical Pixel promotion/cleanup pass; seed ranking and adaptive platform pressure remain later. | [`application-control`](application-control.md), [`performance-and-live-evidence`](performance-and-live-evidence.md) |

## Maintenance Contract

Every substantial tactical must update this topic when it changes a row,
evidence label, risk, or active/ready/later classification. It must also update
the focused owner, the relevant correctness scenarios, and any affected BEP
claims.

Every user-observed correctness failure gets a stable observation or scenario
entry before it can disappear into a generic backlog item. Closing it requires
either reproducible passing evidence or a recorded explanation that the
observation was caused outside the claimed product scope.

The active set changes as work starts, completes, becomes invalidated by new
evidence, or is explicitly reprioritized. Starting one tactical does not
silently pause another. Completed tacticals remain execution records; this
topic should not accumulate their implementation narrative.
