# Installation Product State And Feedback

Topic: `product-state-and-feedback`

Status: Direction accepted on 2026-08-03. Installation-wide local identity,
coarse engagement counters, campaign-specific prompt state, and visible
user-submitted diagnostic context belong together in a product-state boundary
above any one profile. No store, feedback transport, prompt campaign, or
remote analytics service is implemented yet.

## Scope

This topic owns:

- installation-wide local identity and first-run age;
- small monotonic product-usage counters useful for support context and local
  engagement decisions;
- durable state for a concrete review, feedback, migration, or companion-
  installation prompt once that prompt is designed;
- sparse installation lifecycle and version facts; and
- the privacy and consent boundary for including local context in feedback or
  bug reports.

[`client-persistence.md`](client-persistence.md) owns the correctness-critical
profile database and torrent restart state. Tactical
[`066`](../tactical/066-smooth-session-speed-history.md) owns transfer-rate
sampling and history. Those stores have different scope, write cadence,
retention, and failure consequences from the product facts here.

This topic does not authorize continuous analytics upload, a telemetry vendor,
an arbitrary event API, a generic prompt framework, or collection of torrent
names, hashes, paths, trackers, peers, endpoints, or payload content.

## Why Keep These Facts

A few coarse local facts can make voluntary feedback substantially more useful.
For example, knowing that a report came after twenty successful downloads over
ninety days is different from a report on first launch before any torrent was
accepted. The same facts can support restrained local milestones such as
asking an established successful user whether they want to leave a review.

Local collection and remote transmission are separate decisions. RSTorrent may
retain the accepted bounded facts without contacting a server. A later feedback
surface decides what to show and submit; a later analytics decision cannot
inherit permission merely because local counters already exist.

## Identity Vocabulary

Do not reuse one identifier across unrelated ownership boundaries:

| Identity | Scope and purpose | Persistence and transmission |
| --- | --- | --- |
| Installation ID | One local product-data installation across profiles and application updates | Random 128-bit value in installation product state; local by default |
| Profile ID | One torrent catalog, settings set, roots, and application-service instance | Stable beneath the profile root; never an analytics identity |
| Client instance ID | One attached presentation lifetime and its reconnect/takeover ownership | Ephemeral; not product history |
| Report ID | One user-submitted feedback or support case | Generated for that submission; may be sent with the visible report |
| Analytics ID | Possible future remote longitudinal identity | Does not exist; must be separately justified, disclosed, resettable, and revocable |

RSTorrent currently has stable profile identity and an ephemeral WebSocket
client-instance identity. It has no durable installation ID or analytics ID.
The installation ID introduced by a future tactical must not be derived from a
profile ID, device serial, account, path, peer ID, or hardware fingerprint.

An installation's age is the elapsed time since its product state was first
created. Presentation should call this **days since first use** unless platform
backup, restore, uninstall, and reinstall behavior makes a stronger "days
installed" claim truthful. Reset-all-data creates a new installation identity;
ordinary profile deletion does not.

## Accepted Storage Boundary

Use an installation-wide, application-private `product.db` above profile
directories:

```text
application data/
  product.db
  profiles/
    <stable-profile-id>/
      session.db
```

Transfer-history storage may add a separate profile-local derived-data store;
it does not absorb installation product state. Conversely, `product.db` does
not become a transfer time-series database or a second torrent-catalog
authority.

One product-lifetime owner above application-service instances owns
`product.db`. Platform adapters supply the application-data root, current
product version, wall-clock observation, and foreground lifecycle facts.
Profile application services publish bounded semantic milestones upward only
after the authoritative profile transition succeeds. Product clients read a
typed summary and record prompt disposition through that owner; they do not
open SQLite independently.

The initial store is local to one installation and does not sync through an
operating-system account or merge across devices. Backup inclusion remains an
explicit platform decision because restoring `product.db` also restores its
identity, age, prompt dispositions, and counters.

`product.db` is durable product state, not disposable telemetry. Its corruption
or write failure must be observable and conservatively resettable, but it must
not establish torrent resume truth or prevent a download from running. Whether
the future installation-level profile registry shares this database requires a
separate failure-domain decision when that registry is designed.

## Initial Product Summary

The first schema should remain closed and typed. Candidate installation facts
are:

- installation ID and product-state creation time;
- first-seen and current product versions;
- last product start and last completed clean shutdown;
- total successfully added torrents;
- total downloads completed from an incomplete state; and
- foreground product sessions, if the platform-independent session boundary is
  defined precisely enough to count consistently.

Counters are unsigned, saturating, and monotonically increasing. They are
diagnostic and engagement context, not billing, protocol, or correctness
evidence. A write failure degrades the summary and local prompting without
rolling back the torrent command that produced the milestone.

Counter meanings must be based on semantic transitions rather than UI or
transport notifications:

- **Torrents added** advances once after creation of a new durable catalog
  record succeeds. Invalid input, a rejected duplicate, a no-op, and replay of
  the same request do not advance it. Removing and later adding a new catalog
  record is another addition.
- **Downloads completed** advances once when a torrent added incomplete first
  reaches complete through download work. Restart restoration, repeated
  completion notification, recheck of already complete content, and importing
  preexisting complete content do not advance it.
- **Foreground sessions** must mean an intentional product activation, not a
  WebSocket reconnect, webview reload, background-service restart, activity
  recreation, or diagnostic-client connection. Omit this counter until
  desktop and Android can share an honest definition.

The implementation may add a small number of similarly defined coarse facts
when a concrete feedback or prompt use requires them. It must not accept
arbitrary metric names or silently turn every application event into retained
history.

## Prompt Campaign State

Prompt state means the durable disposition of a specific in-product
solicitation, such as:

- asking a successful established user to leave a store review;
- offering the browser extension when the native product is already useful;
- requesting feedback after a meaningful milestone; or
- offering an explicit JSTorrent migration or successor handoff.

Usage counters exist independently from prompts. Do not add generic prompt
rows merely because the product store exists. When a concrete prompt is
accepted, give its code-defined campaign a stable versioned key and retain only
the state required to avoid nagging: first/last shown time, bounded shown count,
and a typed disposition such as `not_now`, `declined`, or `completed`.

Eligibility rules live in reviewed product code and use the local summary;
they are not remotely mutable analytics rules. A prompt records `shown` only
when it was actually visible. `Not now` permits a documented cooldown, while
`declined` and `completed` suppress that campaign permanently unless a future
campaign has materially different purpose and a new key.

## Operational Facts

Startup, clean-shutdown completion, detection of a previous unclean stop,
product-version change, and schema-migration outcome are sparse typed
installation facts rather than numeric transfer buckets. `product.db` is their
natural scope if a later tactical demonstrates a diagnostic or migration need.

That tactical must decide whether each fact needs only a latest value or a
bounded chronological record. It must define crash semantics, retention,
backup behavior, and payload limits before adding an event table. Error text,
paths, URLs, torrent identity, and arbitrary structured payloads do not belong
in a generic operational record.

## Feedback And Diagnostic Submission

A feedback or bug-report surface should build its context snapshot locally and
show the exact fields before submission. The user can omit optional fields and
can submit feedback without a stable installation identifier. Appropriate
default context may include:

- product and platform version;
- days since first use;
- the closed aggregate counters;
- current capability and lifecycle summaries; and
- separately reviewed bounded diagnostics relevant to the reported problem.

Do not put the installation ID, counters, errors, or diagnostic context in a
navigation query string. Query strings leak into browser history, logs,
referrers, and embedded-resource requests. Opening a feedback page must not
transmit the preview merely because an iframe or form loaded. Generate a fresh
report ID and transmit the visible snapshot only after the user explicitly
submits it.

The installation ID is correlation, not diagnostic context, and is omitted by
default. A future support flow that genuinely needs cross-report correlation
must explain that purpose and offer an explicit visible choice. Continuous
remote analytics, if ever adopted, requires its own topic and cannot reuse the
installation ID or report consent.

## JSTorrent Reference Evidence

The inspected JSTorrent revision is
`9895410beeed6aff554053769bd006a3fbd373ef`.

- `extension/src/lib/telemetry-id.ts` creates a random UUID in
  `chrome.storage.local` under `telemetryId`.
- `extension/src/lib/metrics.ts` keeps completed-download, torrent-add, and
  session counters plus per-platform/device aggregates in
  `chrome.storage.sync`, while the installation timestamp stays local.
- The extension's uninstall URL contains version, telemetry ID, days,
  connection status, downloads, additions, sessions, and device count.
- `android/app/src/main/java/com/jstorrent/app/settings/MetricsStore.kt` keeps a
  local install UUID, timestamp, counters, and review-prompt state in dedicated
  preferences. Its review rule requires three completed downloads and seven
  days, uses a thirty-day `not now` cooldown, and respects permanent decline
  or completion.
- `docs/archive/design/profile-system.md` later separates telemetry ID from
  profile ID explicitly, retaining the former only for metrics and the latter
  for session state and roots.

RSTorrent adopts the useful separation and local milestone behavior, not
Chrome account sync, the exact counters or thresholds, the storage format, or
automatic identifier transmission.

JSTorrent made the usage summary visible in its uninstall and feedback pages,
but encoded context in the page URL and immediately used it to load a
pre-populated Google Forms iframe. Form submission was optional, yet visiting
the page could already disclose the query and iframe parameters. RSTorrent's
explicit-submit boundary is an intentional difference.

## Invariants

- Product-state scope is one local installation, not one torrent profile,
  presentation, process, device account, or remote user.
- Profile and transport identities are never repurposed as analytics identity.
- Local measurement does not imply remote collection consent.
- Prompt eligibility is evaluated locally and prompt dismissal is durable.
- Counter and prompt writes are low-frequency and never enter peer, hashing,
  storage, or other engine hot paths.
- Derived product-state failure cannot establish or invalidate verified torrent
  state and cannot fail a download.
- Feedback context is visible before explicit submission and excludes sensitive
  torrent or endpoint data by default.
- Adding a counter, prompt campaign, operational event, submitted field, or
  identity use requires a named semantic purpose and bounded representation.

## Known Gaps And Next Work

- Define the first exact `product.db` schema, migration, SQLite durability,
  corruption recovery, reset, and platform backup policy in a bounded
  tactical.
- Decide which platform/product lifecycle constitutes first use and a
  foreground session on desktop, Android, ChromeOS, and extension-controlled
  configurations.
- Select the first concrete feedback surface and its reviewed visible context;
  no transport or recipient is selected here.
- Design each review, feedback, extension, or migration prompt as an explicit
  campaign with its own eligibility, cooldown, and disposition behavior.
- Decide whether sparse lifecycle/version facts need latest values or a bounded
  event record.
- Decide whether the installation-level profile registry remains separate from
  `product.db` after evaluating failure recovery and backup as one unit.
- Define deletion and migration behavior when replacing JSTorrent while
  preserving or intentionally resetting historical product context.
