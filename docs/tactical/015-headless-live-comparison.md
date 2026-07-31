# Tactical 015: Headless Live Comparison

Status: Complete

Topics: `performance-and-live-evidence`, `oracle-driven-engine-campaign`

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

## Decision-Complete Design

### Catalog And Scenario Identity

`tests/live/torrents.json` is the machine-readable owner of the five magnets
recorded in `docs/test-torrents.md`. Each entry has a stable slug, display
name, v1 info hash, source and license notes, and the source magnet. The runner
derives a common-denominator magnet containing only UDP trackers and a
trackerless magnet with deterministic URL parsing. Known payload geometry is
recorded but may be `null`; live metadata fills in unknown values without
rewriting the catalog.

The runner selects a catalog slug plus one discovery profile:

- `common`: both owners receive the UDP-only magnet and DHT is disabled;
- `dht`: both owners receive the trackerless magnet and DHT is enabled; or
- `full-reference`: libtorrent receives the source magnet with its supported
  discovery mechanisms while RSTorrent receives the source magnet and reports
  which mechanisms it actually enables.

### Owners And Data Flow

`rstorrent-public-probe` is a first-party, headless engine binary. It owns one
online `DownloadControl`, optional `DhtService`, download task, output root,
deadline, cooperative cancellation, and DHT shutdown. Its bounded activity
sink observes verified metadata geometry and verified pieces. It emits exactly
one JSON object after all owned tasks terminate. It never opens a product UI.

`tests/interop/public_compare.py` owns catalog validation, libtorrent session
configuration, isolated temporary roots, subprocess deadlines, alternating
order, cleanup, classification, summaries, and the final report. Libtorrent
is imported only from the locked `tests/interop` environment. Implementations
run sequentially; run zero starts RSTorrent, run one starts libtorrent, and so
on. A failed implementation does not prevent its pair from running.

The Rust probe reports hard observations. The Python runner does not infer a
milestone from log text. Libtorrent milestones come from the pinned Python
bindings for `torrent_status.has_metadata`, `num_pieces`,
`total_wanted_done`, `total_wanted`, and `is_seeding`, whose bindings are in
`reference/libtorrent/bindings/python/src/torrent_status.cpp` and definitions
are in `reference/libtorrent/include/libtorrent/torrent_status.hpp`.

### Milestones And Termination

Supported targets are `metadata`, `first-piece`, `50-percent`, `95-percent`,
`99-percent`, and `complete`. Reports retain all earlier milestones reached:

- `metadata_verified`: the v1 info dictionary was parsed and matched the
  magnet identity;
- `first_piece_verified`: at least one payload piece passed SHA-1;
- percentage milestones: enough unique verified piece bytes crossed the
  threshold; and
- `all_pieces_verified` and `published`: the owner reported the whole wanted
  payload complete and its storage operation completed successfully.

When a nonterminal target is reached, the owner cancels or removes the torrent
and must terminate within the cleanup grace period. Reaching the observation
before a cleanup failure does not turn that failure into success. `complete`
requires verified completion and publication, not merely 100% received bytes.

### Result Contract

The report is versioned JSON with `config`, a catalog snapshot, ordered
`runs`, and an aggregate `summary`. Every implementation result contains:

- `outcome`: `milestone_reached`, `timeout`, `error`,
  `integrity_failure`, or `harness_error`;
- nullable monotonic seconds for every milestone;
- metadata/payload geometry and verified progress;
- enabled discovery and transport capabilities;
- terminal detail, bounded diagnostic state, wall time, and available process
  resource measurements; and
- `cleanup_succeeded`.

Each pair is classified as `both_reached`, `reference_only`, `rstorrent_only`,
`both_incomplete`, or `harness_error`. Speed ratios exist only for
`both_reached`. Aggregate percentile fields remain `null` when there are not
enough comparable samples.

### Bounds And Cleanup

Defaults are one repetition, a 120-second target deadline, ten-second cleanup
grace, 64 MiB RSTorrent payload-memory ceiling, no bandwidth limit, and no
artifact retention. The CLI accepts explicit larger deadlines and repetition
counts for the authorized multi-gigabyte campaign. It rejects nonpositive or
unreasonably large values before starting a session. Each implementation gets
a distinct temporary save root. Normal roots are recursively removed only
after resolving them beneath the harness-owned temporary directory. Optional
retention is bounded to the JSON report and diagnostic text, not payloads.

### Reference Dossier

This tactical uses libtorrent as an outcome and lifecycle oracle, not a linked
dependency. The probe contract was checked against:

- `include/libtorrent/torrent_status.hpp` for milestone state;
- `bindings/python/src/torrent_status.cpp`, `torrent_handle.cpp`, and
  `session.cpp` for exposed status and session operations;
- `bindings/python/simple_client.py` for the minimal status polling lifecycle;
  and
- the repository's existing `magnet_metadata.py`, `dht_magnet.py`, and
  `session_resume.py` for alert capture and cleanup conventions.

Scheduling, metadata request policy, and endgame changes are deliberately
outside this harness tactical. Subsequent campaign slices must create their
own source dossier before changing those owners.

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

Deterministic tests use synthetic probe results and catalog fixtures. They
cover malformed catalogs, all five pair classifications, alternating order,
milestone thresholding, timeout/error separation, and summary math. The
controlled interop fixtures then exercise the Rust JSON probe without public
networking. Live evidence begins with a one-pair Big Buck Bunny metadata run
and one bounded full run; changing swarm state may make either pair
inconclusive without failing the harness.

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

The catalog-backed runner, shared JSON result and classification schema,
alternating order, bounded cleanup, paired public tracker baselines, and a
controlled full-download fixture have landed. The DHT harness is evidence
consumed by this comparator, not a substitute for it.

A manual reference-only precursor exercised the exact current metadata metric
through pinned libtorrent `2.0.13.0` in ten fresh tracker-only and ten fresh
DHT-only sessions. Both modes completed 10/10; tracker acquisition had a
20.94-second median and DHT acquisition a 0.90-second median. The corresponding
RSTorrent cohorts completed 8/10 at a 32.77-second successful median and 7/10
at a 78.69-second successful median. Discovery capabilities were restricted,
but libtorrent retained its ordinary connection concurrency. These sequential
cohorts do not satisfy the tactical's alternating paired-run stopping
condition. They also stop at verified metadata: the 276,445,467-byte payload
and its 1,055 pieces were not downloaded.

The first actual alternating pair ran RSTorrent first in common-denominator
tracker mode. Both implementations reached verified metadata with identical
geometry and cleaned their owned roots. RSTorrent took 51.32 seconds and
libtorrent 20.63 seconds (2.49x). RSTorrent's milestone snapshot retained 128
candidates, 110 eligible candidates, 20 attempts, and the two requests and two
blocks that formed the 21,307-byte dictionary. This validates the paired
contract and exposes a source-first metadata concurrency target; it is only
one live sample and does not establish a distribution. A bounded paired full
download remained before this tactical's final validation.

The controlled final validation kept one libtorrent seed on loopback and ran
both production adapters sequentially against the same 79,000-byte, two-file,
three-piece magnet. Both verified and published every byte, independently
matched both file hashes, emitted `both_reached`, and cleaned their roots.
RSTorrent published in 0.018 seconds and libtorrent in 0.063 seconds. These
times validate harness mechanics and are not public performance evidence.

The first bounded full public pair then ran the same common-denominator Big
Buck Bunny profile with 900-second owner deadlines. RSTorrent verified
metadata at 16.71 seconds and its first piece at 24.15 seconds, but timed out
after verifying 461 of 1,055 pieces and 120,848,384 of 276,445,467 bytes. It
never reached 50%. Its final snapshot had one connected unchoked peer, four
requests outstanding, zero writes pending, 9,491 missing blocks, and the
named state `requestwindowsfull`. Libtorrent independently verified and
published the payload in 30.88 seconds, crossing 50%, 95%, and 99% at 24.75,
28.88, and 29.91 seconds. The pair is therefore `reference_only`, an
actionable engine gap rather than a harness or public-swarm failure.

All ordinary temporary payloads and processes were removed. The Rust workspace
format, clippy, and test gates passed, as did the locked Python unit suite and
the controlled comparison. This closes the harness tactical without claiming
performance parity; the source-first campaign now owns the gaps it exposed.
