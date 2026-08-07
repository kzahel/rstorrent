# Tactical 106: Live Transfer-Rate Tab Title

Status: Complete on 2026-08-07.

Topic: `web-ui-design`

## Motivation And Outcome

The shared browser/Tauri application leaves the document title at
`RSTorrent`, so a background tab gives no indication that a download or seed
is active. Add the familiar JSTorrent transfer-rate title using RSTorrent's
existing exact session speed projection and browser-local data-unit choice.

The slice stops when an active transfer renders
`RSTorrent - ↓<download>/s ↑<upload>/s`, the rates refresh from a bounded
one-second session view, idle or disconnected state restores `RSTorrent`, and
focused frontend tests and production validation pass.

## Scope And Invariants

- Keep the static HTML title as the startup and fallback value.
- In the mounted shared React application, update the document title from the
  session-wide `payload_received` and `payload_uploaded` current rates.
- Lease one separate two-series session-speed projection at a one-second
  delivery cadence so the title does not depend on the Speed tab, selected
  torrent, destination, or chart-series preferences.
- Continue to lease the existing independently configured Speed projection
  only while its detail tab is open; its range and eight-series allowance do
  not compete with the title metrics.
- Use the active Decimal/Binary data-unit preference immediately. Known zero
  is `0 B/s`; unavailable data remains an em dash rather than a fabricated
  zero.
- Show live rates only while the session is connected or the deterministic
  demo adapter is active. Restore the base title on idle, reconnecting,
  offline, and component unmount paths.
- Throttle rate-driven `document.title` assignments to at most once per
  second. Apply the first change immediately and coalesce intervening state
  into one trailing update containing the latest rates. Component disposal
  may restore the base title immediately as lifecycle cleanup.
- Add no engine counter, generated contract, persistence field, dependency,
  public-network run, repeating browser interval, or visible-client launch.

## Reference And Ownership

No BitTorrent specification governs a browser title, so no pinned libtorrent
survey is required. JSTorrent commit
`9895410beeed6aff554053769bd006a3fbd373ef` updates
`packages/client/src/App.tsx` to
`JSTorrent - ↓<download>/s ↑<upload>/s` while transfer is active. RSTorrent
adopts that product vocabulary but uses the typed session view rather than
mutable engine objects and refreshes at the requested one-second cadence.

The Rust speed owner remains authoritative for both current rates. The live
adapter owns the always-interested two-metric view. Zustand retains the mapped
session summary, and one React-owned leading/trailing throttle owns
`document.title` for the mounted application lifetime. It has at most one
pending one-shot timeout, cancels it on unmount, and restores the base title.
Existing controller/view-set cancellation and join paths own the added lease;
no repeating task or interval is introduced.

## Validation

- Pure title formatting covers active download, upload-only, unavailable
  direction, Decimal/Binary units, idle, and disconnected states.
- Deterministic fake-clock coverage proves an immediate leading update, no
  second assignment inside one second, latest-value trailing coalescing, and
  pending-timeout cancellation during disposal.
- React coverage proves the mounted demo title and unmount reset.
- Live-adapter coverage proves the independent two-metric, one-second view,
  maps both current rates, and retains it across responsive detail changes.
- Run formatting, TypeScript checking, focused and complete Vitest, production
  build/CSP validation, `git diff --check`, and a proportional headless browser
  title assertion.

## Non-Goals

- Per-torrent titles, ETA, progress percentage, torrent names, notification
  badges, favicon changes, or operating-system tray labels.
- Android Compose, CLI, engine, database, generated-contract, or semantic
  application-API changes.
- A user setting for title updates or a second rate-sampling implementation.

## Implemented Result

The shared React application now derives its desired document title from the
mapped session summary. Connected live and deterministic demo sessions show
both directions while either one is active, use the current Decimal/Binary
preference, preserve unavailable directionality, and return to `RSTorrent`
when idle, disconnected, or unmounted. A leading/trailing throttle applies the
first change immediately, retains only the latest desired title, and performs
no second rate-driven assignment until at least one second has elapsed.

The live adapter leases a separate `session-rates` projection with exactly
`payload_received` and `payload_uploaded`, the ten-minute live tier's
one-second owner cadence, and a one-second minimum delivery interval. This
projection remains present across destination and responsive detail changes.
The existing `session-speed` detail projection remains separately leased only
for the Speed tab and retains its selected range and up-to-eight metrics.

## Validation Evidence

The following ran from `clients/web` on 2026-08-07:

- `npm run typecheck`: pass.
- Focused Vitest for the title formatter, React application, and live adapter:
  59 tests passed.
- Focused fake-clock throttle and React coverage: 47 tests passed.
- `npm test`: 35 files and 221 tests passed; the two existing opt-in files and
  tests remained skipped.
- `npm run build`: pass, including the production CSP scan of both JavaScript
  bundles. The existing large-chunk advisory remained non-fatal.
- Focused headless Chrome on an isolated Vite port: the production component
  path exposed the active transfer title while retaining the existing
  destination and accessibility assertions.
- `git diff --check`: pass.

No generated contract, Rust source, database, Android code, dependency,
public-network process, visible product client, or physical device changed or
ran. The standard Playwright port was already owned by an existing preview
process; validation left it untouched and used an isolated development port.
