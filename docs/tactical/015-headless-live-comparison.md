# Tactical 015: Headless Live Comparison

Status: In progress

Topic: `performance-and-live-evidence`

## Motivation And Outcome

RSTorrent needs repeatable public-swarm evidence without launching Tauri,
Chrome, Android UI, or a physical device. This tactical adds the minimum
headless reference and comparison infrastructure required by the DHT campaign.

The stopping condition remains a bounded command that can run one cataloged torrent
through pinned libtorrent and RSTorrent in isolated temporary profiles, verify
and classify the results, and emit machine-readable timing and resource
metadata. A public failure remains an honest result rather than failing the
harness itself.

## Dependencies And References

- [`../topics/performance-and-live-evidence.md`](../topics/performance-and-live-evidence.md)
- [`../test-torrents.md`](../test-torrents.md)
- pinned libtorrent `2.0.13` at
  `7d7fc38fac61177fa5e02148f791b2f65250b09d`
- the existing `tests/interop` locked Python environment
- the existing headless `rstorrent-session` and engine diagnostic binaries

The reference sync must support selecting one named checkout. It must not
require updating the dirty first-party JSTorrent sibling merely to read pinned
libtorrent or the BEPs. Libtorrent's GPL `simulation/libsimulator` submodule is
not initialized, linked, copied, or distributed by this tactical.

## Scope

- Add `--only NAME` selection to reference `sync` and `status`, with validation
  for unknown or duplicate names.
- Keep selected external checkouts detached at their exact pins and preserve
  all existing origin, cleanliness, and path checks.
- Add a machine-readable public-torrent catalog derived from
  `docs/test-torrents.md` without duplicating large torrent payloads.
- Add a bounded Python comparator with common-denominator, full-reference, and
  trackerless-DHT modes.
- Run implementations sequentially in separate temporary roots and alternate
  implementation order across repeated runs.
- Emit JSON containing configuration, implementation outcomes, completion and
  integrity state, timing milestones available from each owner, resource
  observations, and the paired classification.
- Retain bounded diagnostics only for actionable RSTorrent mismatches and
  remove ordinary temporary payload, database, and profile artifacts.
- Establish at least one tracker-based invocation, even if current public
  swarm state makes its outcome inconclusive.

## Non-Goals

- No product UI or visible automation.
- No CI speed threshold.
- No claim that public swarm speed is deterministic.
- No remote daemon or new product transport.
- No use of libtorrent as an RSTorrent runtime dependency.
- No DHT implementation in this tactical.

## Invariants And Bounds

- Live runs are opt-in and have explicit wall-time, payload-size, connection,
  bandwidth, disk, output, and retained-diagnostic limits.
- Correct verified bytes and publication are hard gates when an implementation
  reports completion.
- Missing metrics are `null` or explicitly unavailable, never fabricated as
  zero.
- Libtorrent and RSTorrent capability settings are included in every result.
- `libtorrent complete / RSTorrent incomplete` is actionable; both incomplete
  is inconclusive; both complete is comparable; RSTorrent-only completion is
  recorded but reference comparison is inconclusive.
- The harness always terminates processes it owns and removes temporary state
  unless an explicit bounded retention option is selected.

## Validation

- deterministic parser/configuration tests for catalog and classification;
- reference selection tests covering unknown, selected, dirty, and missing
  checkouts without mutating unrelated repositories;
- a controlled small libtorrent/RSTorrent run through the same result schema;
- one opt-in public tracker run from the catalog; and
- `git diff --check`, Rust workspace gates, and locked Python execution.

## Stopping Condition

The tactical is complete when the comparator can produce a bounded JSON report
for both implementations without a visible application, correctly classify
all four outcome combinations in deterministic tests, and leave no owned
processes or ordinary payload artifacts behind. Tactical 016 has installed the
RSTorrent capability needed by trackerless DHT mode.

## Partial Outcome

Tactical 016 required and landed two prerequisites from this plan:

- repeatable `--only` reference sync/status selection for the pinned BEPs and
  libtorrent without inspecting or mutating the independently maintained
  JSTorrent sibling; and
- a bounded headless DHT harness that completes controlled metadata/content
  exchange, validates incoming DHT participation, cleans its owned processes,
  and can run an opt-in single-sided public probe.

The tactical remains in progress because there is no catalog-backed paired
runner, shared JSON result/classification schema, alternating run order, or
paired public tracker baseline yet. The DHT harness is evidence consumed by
that future comparator, not a substitute for it.
