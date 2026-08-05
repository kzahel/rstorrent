# Tactical 087: Uniform Detail Tab Footprints

Status: complete.

Topic: `web-ui-design`

## Motivation And Desired Outcome

The detail strip gives only Trackers and Peers fixed-width count badges. Those
two compound labels are substantially wider than the other tabs, and their
invisible placeholder badges retain the same uneven gaps when no torrent is
selected. The counts duplicate information in the detail content and make the
ten-tab strip harder to scan and fit.

Use label-only tabs with one equal footprint at each interface size. Preserve
the divider between torrent-scoped and session-scoped tabs and retain
horizontal scrolling when the available detail width cannot contain all ten.

## Scope

- Remove configured-tracker and connected-peer counts from detail tabs.
- Give every detail tab the same fixed inline size within each Compact,
  Standard, and Spacious interface preset.
- Preserve the existing torrent/session divider, active underline, keyboard
  behavior, coarse-pointer height, and active-tab scroll positioning.
- Update component and browser geometry coverage plus the owning UI topic.

## Non-Goals

- Moving session-scoped tabs to a separate destination or menu.
- Changing tab names, order, scope, requested views, or content-level counts.
- Making all ten tabs fit without scrolling at every viewport width.
- Adding a dependency, application setting, engine field, or contract change.

## Invariants And Edge Cases

- All ten rendered tab buttons have equal widths for a given interface size.
- Selecting a tab or changing the selected torrent does not move or resize any
  tab.
- The Disk tab retains the visible boundary before the session-scoped group.
- Narrow and phone layouts scroll in one row; tabs never wrap or compress
  below their selected interface-size footprint.
- Arrow-key navigation and automatic active-tab visibility remain intact.

## Validation

- Component coverage asserts label-only tab content and unchanged selection.
- Browser coverage asserts equal widths and stable geometry at representative
  wide, compact-window, and phone widths, including horizontal overflow.
- Run frontend formatting, TypeScript, focused unit tests, the focused
  Playwright scenario, and a production build.
- Inspect representative wide and phone screenshots.

## Stopping Condition

This slice is complete when the detail strip contains no count badges, every
tab has the same interface-size-specific footprint, the scope divider and
horizontal overflow behavior remain intact, focused validation passes, and
the UI topic records the accepted result and evidence.

## Implementation And Evidence

`DetailPane` now renders label-only buttons. One semantic width token supplies
88, 100, and 112 CSS-pixel footprints for Compact, Standard, and Spacious;
each button uses that value as both its fixed flex basis and inline size. The
existing Disk border and automatic margin still mark and position the session
group, while the tab list retains horizontal overflow and active-tab
centering. Interface-size changes retrigger that centering after the fixed
footprint changes, keeping a previously active clipped-end tab visible.

Component coverage verifies that Peers and Trackers contain only their labels.
Headless Chrome verifies equal and selection-stable geometry at 1,440, 920,
and 390 pixels, horizontal overflow at phone width, the Disk divider, and all
three exact interface-size footprints. The 1,440- and 390-pixel tab strips were
captured and visually inspected: the wide strip presents both groups with the
divider intact, and the phone strip presents four equal tabs without clipping
or wrapping before scrolling.

Validation run on 2026-08-05:

```text
source ~/.profile
cd clients/web
npm run typecheck
  passed
npm test
  30 files passed, 2 skipped; 178 tests passed, 2 skipped
RSTORRENT_PLAYWRIGHT_BASE_URL=http://127.0.0.1:4187 \
  npx playwright test tests/inspection-demo.spec.ts \
  --grep 'detail tabs keep equal stable footprints|interface size settings persist'
  2 passed
npm run build
  passed, including the CSP bundle check
```

The full headless browser suite was also screened: 28 tests passed and seven
opt-in live tests skipped. Its one failure is outside this slice: the removal
dialog's destructive warning currently has a 3.73:1 foreground/background
contrast ratio where Axe requires 4.5:1. No removal-dialog source or styling
is changed here.
