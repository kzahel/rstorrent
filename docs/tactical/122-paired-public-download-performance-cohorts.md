# Tactical 122: Paired Public Download Performance Cohorts

Status: Approved and in progress on 2026-08-10. Human review paused uTP after
Tactical `126` closed evidence-limited, returned to the readiness queue, and
explicitly authorized this tactical for autonomous end-to-end execution and
bounded commits. This tactical does not change uTP support or reopen its WAN
stage.

Topics: `performance-and-live-evidence`, `public-torrent-testing`,
`oracle-driven-engine-campaign`, `capability-readiness`, `peer-lifecycle`,
`storage-throughput-architecture`

Dependencies: completed Tacticals
[`015`](015-headless-live-comparison.md),
[`026`](026-paired-peer-utility-timeline.md),
[`054`](054-bounded-independent-storage-execution.md),
[`057`](057-hardware-performance-baselines.md),
[`091`](091-availability-ranked-piece-activation.md),
[`111`](111-mse-peer-stream-encryption.md),
[`112`](112-dual-stack-transport-and-ipv6-dht.md), and
[`114`](114-session-wide-concurrent-torrent-admission.md), and
[`124`](124-duplex-verified-piece-upload.md) own the headless
public comparator, bounded timeline, local throughput and hardware evidence,
piece scheduling, MSE policies, dual-stack networking, and session resource
and incomplete-torrent upload authority this tactical consumes.

## Decision And Desired Outcome

Extend the existing headless comparison tools into a repeatable, bounded
download-performance harness for the same real public torrent under
RSTorrent and pinned libtorrent. Keep synthetic throughput evidence as the
controlled ceiling, then add paired public cohorts that answer two distinct
questions:

1. how the engines compare when input, peer transport, discovery, encryption,
   connection target, upload behavior, and download limits are matched as far
   as their current architectures permit; and
2. how their supported product-default capability sets compare for the
   user-visible download outcome.

The implementation evolves `tests/interop/public_compare.py` and the existing
headless RSTorrent probe. It does not establish a second benchmark framework.
The result is a versioned JSON artifact that preserves exact scenario
identity, settings, run order, milestones, integrity, process resources,
bounded owner telemetry, cleanup, and paired distributions.

This tactical establishes a trustworthy baseline and diagnoses material gaps.
It does not authorize scheduler, picker, storage, networking, or protocol
tuning discovered from that baseline. Any such change requires its own
source-first tactical so the comparator does not become an optimization grab
bag.

## Stopping Condition

The tactical is complete only when all of the following are true:

1. direct-metainfo payload mode supplies the exact same independently
   validated v1 metainfo to both owners, while the retained magnet mode is
   clearly classified as discovery evidence rather than payload-only
   performance;
2. deterministic tests prove catalog refresh, profile expansion, balanced
   ordering, classification, statistics, resource rejection, privacy
   redaction, independent piece verification, cancellation, and cleanup;
3. the release-mode workers pass controlled full-download comparisons for
   matched plaintext and forced RC4, with exact payload verification and no
   owned task, process, or artifact left behind;
4. the baseline cohorts in [Required Live Evidence](#required-live-evidence)
   are attempted under one retained hardware and filesystem profile, every
   result is kept, and every failure is classified;
5. no integrity, publication, cleanup, privacy, or resource-bound violation is
   present in retained evidence; and
6. the tactical, owning topics, catalog provenance, and campaign checkpoint
   record the actual commands, environment, outcomes, distributions, known
   semantic differences, and recommended next owner-level slice.

Public speed parity is not a stopping gate. A slower or incomplete RSTorrent
cohort is a successful measurement when the harness is healthy and the
terminal boundary is explicit. The campaign's existing functional and
comparable thresholds may interpret the result, but they are not rewritten or
weakened to close this tactical.

## Evidence Model

Synthetic and public evidence retain separate roles:

- `tests/interop/local_throughput_compare.py` remains the deterministic
  release-mode throughput and MSE-retention baseline across controlled payload
  sizes and piece geometries.
- A controlled loopback run through the public-report schema proves settings,
  milestone, integrity, metric, and cleanup mechanics. It is not evidence
  about a real swarm.
- Direct-metainfo public cohorts measure payload acquisition after both owners
  begin with the same verified metainfo and tracker tiers.
- Magnet public cohorts measure metadata and discovery as well as payload and
  must not be compared to direct-metainfo timing as if the start points were
  equal.
- Matched profiles reduce known capability differences. Product-default
  profiles deliberately retain them and answer an end-user question rather
  than an algorithm-only one.

Public peers, paths, tracker responses, remote load, and cache state cannot be
made identical between sequential runs. Alternating order and repeated pairs
reduce obvious bias but do not turn the Internet into a controlled experiment.
Reports say `paired public observation`, not `benchmark proof` or
`same-peer comparison`.

## Scenario Inputs And Catalog

### Input modes

`metainfo` is the primary payload-performance mode. The harness fetches a
cataloged official `.torrent` resource once per invocation, validates it, and
places the same bytes in a harness-owned read-only input location for both
workers. RSTorrent enters through the existing verified-info/configured-
tracker path used by resumable magnet downloads; libtorrent receives a
`torrent_info` parsed from those bytes. Neither owner times the HTTP fetch.

`magnet` retains Tactical `015`'s existing behavior for discovery evidence.
The timer begins before metadata acquisition and results retain
`metadata_verified`. A magnet run may not contribute to a direct-metainfo
payload ratio or distribution.

The two modes are explicit enum values in configuration and every worker
result. The implementation must not silently synthesize one from the other.

### Catalog version 2

Extend `tests/live/torrents.json` rather than create a second source list. A
version-2 entry supports either a stable published magnet or a dated official
`.torrent` source recipe and records:

- stable slug, display name, source organization, source page, retrieval date,
  and redistribution/license note;
- outer metainfo SHA-256, v1 info hash, payload bytes, piece length, piece
  count, file count, private flag, padding geometry, tracker tiers, web seeds,
  and allowed input modes;
- intended roles such as `small-primary`, `medium-distro`, `large-distro`,
  `dht-only`, or `tracker-breadth`; and
- per-role maximum target, owner deadline, and payload/network ceiling.

At execution time, refresh dated distribution candidates from their official
download pages. Fetch only HTTPS source URLs and redirects whose final host is
on the entry's explicit official-host allowlist. Reject more than five
redirects, more than 64 MiB of metainfo, non-v1 or hybrid-only identity,
private torrents, path-invalid metainfo, unexpected info hash, changed
geometry, or an unrecorded tracker/web-seed set before any payload owner
starts. A changed official release is reviewed and committed as a provenance
update; it is never silently accepted during a benchmark.

An offline `inspect-candidate` command emits the normalized proposed catalog
record and never modifies tracked files. The harness does not commit or retain
third-party `.torrent` bytes or payloads. The report may retain the normalized
identity and hashes needed to reproduce the observation.

### Initial cohort roles

The planned catalog roles are:

| Role | Initial candidate | Purpose |
| --- | --- | --- |
| Small primary | WebTorrent Free Torrents' Big Buck Bunny | Fast complete runs, continuity with prior paired evidence, and full integrity verification. |
| Medium distro | Current official Debian amd64 netinst | A practical complete multi-peer run and plaintext HTTP/non-default tracker breadth after metainfo retrieval. |
| Large distro | Current official Ubuntu amd64 live-server | The primary large, well-seeded, application-shaped workload. |
| DHT-only breadth | Current official Arch Linux x86_64 image | Trackerless discovery and payload breadth, kept outside matched tracker-only ratios. |
| Tracker breadth | Current official Linux Mint Cinnamon 64-bit image | Independent UDP tracker and large-distribution diversity. |
| Small breadth | Remaining WebTorrent catalog works | Confirmation that conclusions are not unique to one Blender torrent. |

These product names describe roles, not permanent release versions or guaranteed
swarm health. If an official source is unavailable or the pinned libtorrent
reference cannot reach the role reliably, retain the failed observation and
replace the role only through a reviewed catalog provenance change. Do not
substitute a third-party mirror merely to make a cohort green.

## Comparison Profiles

Every profile expands to a complete settings snapshot for both owners. The
snapshot is stored in the report before either worker starts, then workers
echo the effective values they applied. A mismatch is `harness_error`, not a
performance result.

### `matched-plain-30`

This is the primary common-denominator profile:

- direct verified metainfo and its ordered tracker tiers;
- online outbound TCP peers only; incoming peer connections, uTP, DHT, PEX,
  LSD, UPnP, NAT-PMP, web seeds, and unrelated discovery are disabled;
- MSE is disabled for incoming and outgoing peer streams on both owners;
- both owners have a nominal session-global and per-torrent maximum of 30 peer
  connections, at most 30 pending outbound dials, and 30 new connection
  attempts per second;
- peer connect timeout is 15 seconds, request timeout is 60 seconds, request
  queue time is three seconds, and the advertised maximum outgoing request
  queue is 500 where the owner exposes that setting;
- download is unlimited unless the invocation gives one identical positive
  byte-per-second cap; and
- both owners use eight session-wide upload slots and unlimited payload upload,
  consuming Tactical `124`'s completed incomplete-torrent uploader; both
  workers stop without post-completion seeding and report actual uploaded
  payload and protocol bytes.

`30` is a nominal policy target, not a claim that the implementations schedule
or count half-open sockets identically. The result therefore reports configured
limits plus observed established, connecting, handshaking, useful, and peak
concurrency. It also records unsupported or non-equivalent settings instead of
inventing parity.

### `matched-rc4-30`

This profile is identical to `matched-plain-30` except that RSTorrent uses
`required` MSE and libtorrent uses forced encryption with RC4 as the only
allowed payload method and RC4 preference enabled. The run is valid only if
every established payload contributor reports RC4 and neither owner reports a
plaintext payload stream.

The controlled local full-download pair is the performance authority for this
profile. The public run is a bounded compatibility and availability smoke
because requiring RC4 selects a different subset of the live peer population;
its speed ratio must not be compared to the plaintext cohort as a pure crypto
cost.

### `product-default`

Each owner uses the outgoing capabilities presently supported by its product
configuration. Incoming peer connections remain disabled so NAT, firewall,
and port-mapping reachability do not dominate a downloader comparison.
Libtorrent may use TCP, uTP, tracker, DHT, PEX, web seeds, ordinary compatible
MSE, and incomplete-torrent upload; RSTorrent may use TCP, tracker, DHT, PEX,
ordinary compatible MSE, and its completed incomplete-torrent uploader. The
exact lists, including address families, MSE method counts, and actual upload
slots/bytes, are part of every result.

This profile measures the user-visible capability gap. A result is never
described as a scheduler, picker, TCP, or storage comparison unless telemetry
and a controlled follow-up isolate that owner.

### `dht-only`

This retained discovery profile uses the exact v1 info hash without trackers
or web seeds, starts each owner with a cold session, and enables only its
supported DHT and peer transports. It is used for the Arch role and does not
contribute to tracker-profile aggregates. Warm DHT state, incoming
reachability, and mixed tracker/DHT experiments are outside this tactical.

## Run Ordering And Cohorts

A `pair` is two sequential workers for one exact catalog identity, input mode,
profile, target, machine profile, and invocation. Pair zero runs RSTorrent then
libtorrent, pair one reverses the order, and four-pair cohorts use ABBA order:

```text
RSTorrent/libtorrent
libtorrent/RSTorrent
libtorrent/RSTorrent
RSTorrent/libtorrent
```

No owner overlaps another owner. Each gets a new process, session, output root,
and cold in-process state. OS page and filesystem caches are left in their
ordinary uncontrolled state and the order is retained; the harness does not
require root or purge system caches.

Named suites prevent an accidental large run:

- `smoke`: one direct-metainfo Big Buck Bunny matched-plain complete pair;
- `standard`: four direct-metainfo matched-plain complete pairs for Big Buck
  Bunny and four for the refreshed medium-distro role;
- `large`: two direct-metainfo matched-plain complete pairs for the Ubuntu
  large-distro role, one beginning with each owner;
- `product`: two direct-metainfo product-default complete pairs for Big Buck
  Bunny and two for Ubuntu, one in each start order;
- `encryption`: the controlled local plaintext/forced-RC4 gate plus one public
  direct-metainfo matched-RC4 Big Buck Bunny pair targeting first verified
  piece;
- `breadth`: one appropriately bounded pair for every remaining WebTorrent
  entry, the DHT-only role, and the tracker-breadth role; and
- `diagnostic`: at most ten alternating pairs for one explicitly selected
  catalog/profile/target after a baseline identifies a gap.

Only `smoke`, `standard`, `large`, `product`, and `encryption` are required to
close this tactical. `breadth` is attempted when the current official payload
and aggregate network ceilings fit the authorized invocation; otherwise each
skipped role is recorded with the exact preflight reason. `diagnostic` is not
run merely to accumulate samples and never includes an engine change.

## Result And Measurement Contract

### Provenance and environment

The report schema records:

- RSTorrent git commit, dirty state, release binary SHA-256, Cargo profile, and
  feature set;
- pinned libtorrent semantic version, exact commit, Python binding version,
  effective settings, and worker-file SHA-256;
- harness and catalog schema versions and catalog snapshot;
- OS/version, architecture, CPU model, logical CPUs, physical memory, power
  source where available, filesystem type, free space, process limits, and
  explicitly configured bandwidth cap; and
- invocation identifier, UTC start, monotonic durations, pair/cohort identity,
  owner order, input mode, cache policy, and all effective resource limits.

A dirty tree is allowed for local investigation but prominently recorded.
Evidence used for a checked-in baseline requires a clean tree at the reported
commit except for the output path outside the repository.

### Milestones and rates

Workers report nullable monotonic offsets for:

- process ready and torrent admitted;
- metainfo verified;
- first candidate, first connection, first payload byte, and first verified
  piece;
- 10%, 50%, 90%, 95%, and 99% of wanted bytes verified;
- all wanted pieces verified, publication complete, cancellation requested,
  owner stopped, and all tasks/processes joined.

Primary elapsed time is torrent admission through successful publication.
For direct-metainfo completed runs, also derive first-payload-to-95% and
10%-to-90% steady intervals when both endpoints exist. Sampled verified-byte
rates are descriptive. A libtorrent instantaneous rate and an RSTorrent
windowed rate are not treated as the same measurement unless their definitions
match; unavailable fields are `null`, never zero.

### Resource and owner telemetry

Each implementation runs in its own child process. The orchestrator samples
process-tree CPU time and peak RSS and records sampling cadence and gaps. Each
worker additionally emits owner-native cumulative and high-water observations
when available:

- physical payload and protocol download/upload bytes, failed and redundant
  payload, disk read/write bytes, disk queue/service observations, and hash
  work;
- candidates, connection attempts, connecting, handshaking, established,
  unchoked, useful, TCP/uTP, IPv4/IPv6, plaintext/RC4, and disconnect reasons;
- request target, outstanding and timed-out requests, busy/request buffers,
  pending disk bytes, active pieces, endgame state, verified pieces/bytes, and
  publication state; and
- bounded stall classification and exact terminal owner/task counts.

The existing bounded utility timeline is sampled no faster than once per
second, stores at most 1,024 points per worker, and coalesces older points when
needed. It contains counts, byte totals, rates, states, and high-water marks,
but no peer endpoints, DNS answers, interface addresses, or raw log text.

Libtorrent fields follow the pinned meanings in `torrent_status`, `peer_info`,
and `session_stats`. RSTorrent fields retain their existing owner meanings.
The schema includes a semantic map and availability reason for each metric;
similar names are not silently combined.

### Integrity and pair statistics

After a completed owner stops and before its root is removed, an independently
authored streaming verifier parses the validated v1 metainfo, maps logical
files and padding, checks exact file kinds and lengths, synthesizes pad bytes
without reading a nonexistent path, hashes every piece across file boundaries,
and compares every SHA-1 in the `pieces` string. It uses at most 1 MiB of
payload buffer and does not call either engine's verification API. Integrity
checking is outside the timed download interval. A nonterminal target retains
the owner's hash-verified progress and exact metainfo identity but makes no
independent full-payload integrity claim and contributes no speed ratio.

A paired speed ratio exists only when both workers use the same validated
input/profile/target, reach publication, pass independent verification, and
clean up. Cohort output includes raw observations, paired ratios, completion
counts, median, p90 only when the sample count makes it meaningful, median
absolute deviation, and order-stratified summaries. It does not impute failed
runs or compute a ratio from one owner's timeout.

Classification retains Tactical `015`'s functional outcomes and adds typed
preflight, resource-bound, settings-mismatch, integrity, publication, cleanup,
privacy, and worker-protocol failures. Public unavailability is evidence, not
a harness failure. If pinned libtorrent completes fewer than three of four
primary small/medium runs or neither large run, that catalog role is
`reference_unhealthy` and no parity conclusion is drawn from it.

## Exact Bounds, Safety, And Cleanup

Public access requires both `--allow-public-network` and an explicit JSON
output path. There is no public-network default in tests, local development,
or CI.

- Metainfo input is at most 64 MiB, v1 payload at most 16 GiB, and one
  invocation at most 20 pairs.
- The default owner deadline is 30 minutes; a catalog role may request up to
  four hours for a large payload. Cleanup grace is 30 seconds.
- Per-owner wire payload is bounded by
  `max(expected_payload * 3 / 2, expected_payload + 256 MiB)`. Exceeding it
  cancels the worker and classifies `resource_bound`.
- Before network access, the harness computes the worst-case invocation wire
  budget from every selected worker. The run requires an explicit
  `--max-network-gib` at least that large and rejects a value over 64 GiB.
- Free space before each worker must be at least expected payload plus the
  larger of 2 GiB or 25% of expected payload. A single worker root may never
  exceed that preflight allowance.
- The normal payload-memory ceiling is 256 MiB per worker, with RSTorrent's
  buffered-payload sublimit recorded separately. A worker exceeding the
  configured RSS/resource contract is canceled rather than allowed to swap
  indefinitely.
- Report JSON is capped at 32 MiB, the utility timeline at 1,024 points, and
  terminal diagnostics at 256 KiB per worker. Excess is a harness error, not
  silent truncation of required evidence.

The orchestrator owns one absolute temporary parent resolved before cleanup.
Each worker gets a distinct child containing only its profile, payload, and
input link/copy. Removal validates that the target remains a descendant of the
owned parent. Normal completion, timeout, interrupt, malformed output, and
worker crash all enter the same cleanup path. The retained artifact is the
bounded JSON report only; payload, metainfo, database, profile, resume data,
raw logs, and packet captures are removed.

The harness makes ordinary outbound public BitTorrent connections and thus may
expose the runner's public IP to trackers and peers. It does not manipulate a
VPN, firewall, router, system cache, network shaper, or privileged OS setting.
Reports exclude peer addresses, tracker peer lists, DNS results, interface
addresses, and local absolute roots. Live authorization begins only when this
tactical becomes active and the operator supplies the opt-in flags.

## Owner, Task, Cancellation, And Dependency Map

- `tests/interop/public_compare.py` owns catalog validation, source preflight,
  profile expansion, pair order, worker supervision, process sampling,
  independent verification, aggregation, report bounds, and temporary-root
  cleanup. It never owns torrent state.
- A separate libtorrent worker owns exactly one pinned `session`, one
  `torrent_handle`, status/session-stat sampling, alert capture, torrent
  removal, and session shutdown. Importing libtorrent no longer places the
  reference owner in the orchestrator process.
- `rstorrent-public-probe` owns exactly one `DownloadControl`, its optional
  discovery owners, one verified metainfo/configuration handoff, bounded
  diagnostics, cooperative cancellation, and task joins. It emits one bounded
  JSON object after all owners terminate.
- The independent verifier is task-free and runs only after its worker is
  stopped. It owns one bounded read buffer and no network or engine object.
- On timeout or interrupt, the orchestrator first sends a cooperative stop,
  waits the cleanup grace, then terminates and finally kills only the exact
  child process if required. RSTorrent cancels its download and joins DHT/UDP
  owners; libtorrent pauses/removes its torrent and aborts its session. The
  orchestrator reaps every child before deleting roots or emitting its report.

Protocol values, codecs, storage mapping, and deterministic engine state do
not depend on Python, process sampling, files used only by the harness, or
libtorrent. Runtime metrics flow outward from existing owners. A small shared
Python module may extract duplicated hardware, process-sampling, MSE-settings,
and JSON-bound helpers already present in local and public comparators. Do not
create a generic benchmark framework, engine trait, new daemon, new crate, or
new third-party dependency.

This is host-only headless test infrastructure. It changes no Android
presentation or product behavior. If shared engine configuration or diagnostic
code changes, keep it platform-neutral and pass both existing Android ABI
cross-builds in the same slice. No emulator or physical-device run is required
unless implementation unexpectedly changes application, storage, or platform
semantics; that expansion requires human review.

## Source-First Record

### Normative behavior

The harness adds no protocol claim, but its identity and capability labels
were checked against the pinned BEP repository at commit
`7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`:

- BEP 3 owns v1 metainfo, file geometry, piece boundaries, and SHA-1 identity;
- BEP 9 applies only to retained magnet metadata acquisition;
- BEP 11, BEP 15, BEP 19, and BEP 29 label PEX, UDP tracker, web-seed, and uTP
  capability differences in product-default or breadth profiles; and
- Tactical [`111`](111-mse-peer-stream-encryption.md) remains the normative
  and implementation owner for MSE/PE. This tactical consumes its established
  `disabled` and `required` policies rather than redefining encryption.

### Pinned libtorrent oracle

The required reference is libtorrent `2.0.13` at exact pinned commit
`7d7fc38fac61177fa5e02148f791b2f65250b09d`. `scripts/references.py status
--only libtorrent` confirmed that exact clean checkout while planning this
tactical. The following source and tests were inspected:

- `include/libtorrent/settings_pack.hpp` transport, connection-rate,
  connection-limit, request-queue, rate-limit, unchoke-slot, and MSE settings;
- `src/settings_pack.cpp` defaults for TCP/uTP, discovery services, timeouts,
  request queues, connection limits/rates, MSE, alert queue, disk/hash threads,
  and redundant-byte reporting;
- `include/libtorrent/torrent_handle.hpp::set_upload_limit`,
  `set_max_uploads`, and `set_max_connections`, plus
  `src/torrent.cpp::set_max_uploads`, which show that zero max uploads means
  unlimited rather than disabled; `src/settings_pack.cpp` confirms the default
  eight session-wide unchoke slots adopted by RSTorrent and retained by the
  matched profile;
- `include/libtorrent/torrent_status.hpp` wanted/downloaded bytes, state,
  completion, piece-count, peer-count, half-open-inclusive connection, failed,
  redundant, and payload-rate meanings;
- `include/libtorrent/peer_info.hpp` connecting, handshake, snubbed, endgame,
  TCP/uTP, plaintext/RC4, rate, queue, timeout, request-buffer, and pending-disk
  observations;
- `src/session_stats.cpp` connection-attempt, half-open/connected, TCP/uTP,
  request, endgame, and wakeup metrics, with
  `src/session_handle.cpp::post_session_stats` as the snapshot entry point;
- Python bindings in `bindings/python/src/torrent_status.cpp` and
  `peer_info.cpp`, which expose the status and peer observations used by the
  worker;
- `test/test_settings_pack.cpp::{default_settings,apply_pack}` and
  `test/settings.cpp` for exact default/closed settings behavior;
- `test/test_transfer.cpp::test_transfer` for controlled TCP-only transfer,
  explicit discovery/MSE disablement, completion, and cleanup;
- `test/swarm_suite.cpp::run_swarm` for multi-session swarm and forced-MSE
  transfer behavior;
- `test/test_pe_crypto.cpp::{diffie_hellman,diffie_hellman_degenerate_key,rc4}`
  for encryption edge inventory;
- `test/test_session.cpp::session_stats` and
  `test/test_alert_types.cpp::session_stats_alert` for stats availability; and
- `test/test_peer_list.cpp` peer/count/connect-candidate limits and transitions.

Adopted behavior is an explicit settings snapshot, separately reported nominal
and observed connection concurrency, exact encryption-method assertion,
status/peer/session-stat meanings, and `null` for unavailable fields.
Intentional differences are RSTorrent's own scheduler and resource owners,
sequential rather than simulated public runs, and product-default capability
reporting. Libtorrent is a subprocess oracle, never an RSTorrent runtime
dependency.

The GPL libtorrent simulator is not initialized, linked, run, copied, or
distributed. No reference source, fixture, test vector, or topology is copied;
tests are independently authored from the recorded contracts.

### JSTorrent product history

The sibling checkout was inspected at commit
`9895410beeed6aff554053769bd006a3fbd373ef`:

- `packages/engine/integration/python/benchmark_tick.py` records throughput,
  tick cost, active pieces, peers, progress, and rates across payload and peer
  counts, showing that paired throughput needs owner telemetry;
- `packages/engine/integration/python/test_connection_limits.py` asserts both
  global and per-torrent observed peer maxima, showing why configured limits
  alone are insufficient;
- `packages/engine/integration/python/test_mse_encryption.py` proves exact
  encrypted interoperability and disabled-policy rejection with full hashes;
- `docs/archive/performance/2025-01-26-fixed-tick-rate-analysis.md` records a
  real Ubuntu workload whose many peers and active pieces exposed work that a
  one-peer LAN benchmark missed; and
- `docs/archive/investigations/js-thread-bottleneck-analysis.md` records an
  Ubuntu Server workload with about 12,800 pieces, 17 peers, sawtooth
  throughput, and full-piece scans, reinforcing the large-distro cohort and
  active-peer/piece/work telemetry.

These are product-history and failure-shape inputs only. The JavaScript engine,
topology, scheduling policy, and tests are not copied, and RSTorrent retains
its first-party Rust hot path.

## Staged Implementation

1. Add version-2 catalog parsing, offline candidate inspection, exact settings
   fixtures, independent verifier, resource/preflight math, privacy filtering,
   and deterministic unit tests without public access.
2. Split libtorrent into its own bounded worker; add process-tree sampling and
   the shared report schema; pass a small controlled plaintext fixture through
   both workers and independent verification.
3. Add RSTorrent direct verified-metainfo input and the minimum platform-neutral
   diagnostic fields needed by the semantic map. Prove cancellation and owner
   drain for success, timeout, malformed input, and interrupt.
4. Pass release-mode controlled 1 GiB plaintext and forced-RC4 comparisons,
   including exact MSE method, payload hashes, resource high waters, and
   cleanup. Reconcile against the retained synthetic throughput baseline.
5. Refresh official catalog candidates through the inspection command,
   review/commit provenance, run `smoke`, then `standard`, then `large`, then
   `product` and `encryption`. A failed earlier suite does not erase evidence;
   it blocks only a larger suite when integrity, cleanup, privacy, or a hard
   resource invariant is at risk.
6. Aggregate and interpret the baseline without changing engine policy. Update
   this tactical and the owning topics with actual commands, distributions,
   terminal classifications, resource high waters, and the narrowest
   source-first next slice.

## Validation Matrix

### Deterministic and scripted

- Version-2 catalog tests cover valid stable magnets and official-source
  recipes, unknown fields, duplicate identities, hostile paths, hybrid-only or
  private metainfo, redirect/host/count/size limits, hash/geometry/tracker
  drift, and a local HTTP source fixture with no Internet dependency.
- Profile tests compare the complete expected RSTorrent and libtorrent setting
  snapshots for matched plaintext, forced RC4, product default, and DHT only,
  including settings-mismatch rejection.
- Pair/cohort tests cover ABBA order, interruption at every worker boundary,
  every classification, insufficient samples, failed-run exclusion, percentile
  math, reference health, report-size limits, and reproducible raw ordering.
- Bounds tests cover multiplication/overflow in disk and wire budgets, free-
  space rejection, pair/timeout limits, output ancestry, symlink/path escape,
  signal escalation, child reaping, and cleanup after malformed worker JSON.
- Privacy tests reject endpoints, DNS/interface values, absolute temporary
  roots, unbounded log fields, and packet content from the retained report.
- The independent verifier covers single/multi-file content, pieces crossing
  file boundaries, padding, final short pieces, zero-length files, missing or
  wrong-kind paths, truncation, extra bytes, and one-byte corruption in every
  mapping class.
- Rust probe tests cover exclusive metainfo/magnet inputs, verified identity,
  tracker order, each encryption/transport profile, payload and memory bounds,
  milestone ordering, timeout, cancellation, and exact zero terminal owners.

### Controlled interoperability and platform gates

- Run the public-report schema against a deterministic small v1 multi-file
  fixture and pinned libtorrent in both peer directions where applicable.
- Run the retained release-mode 1 GiB local throughput comparison for plaintext
  and forced RC4 with full hash verification, exact method assertion, process
  resource sampling, and cleanup.
- Run `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings`,
  and `cargo test --workspace`. Run the locked `tests/interop` Python suite and
  `git diff --check`.
- If common Rust engine code changes, cross-build both existing Android ABIs.
  No web type generation, UI test, emulator, or physical-device run is needed
  when the change remains entirely in headless harness/probe code.

### Required live evidence

On one recorded clean host/filesystem profile, with no unrelated high-load
work and ordinary uncontrolled caches:

1. run `smoke` and retain the complete pair;
2. run `standard`: four complete ABBA pairs each for Big Buck Bunny and the
   refreshed official medium-distro role under `matched-plain-30`;
3. run `large`: two complete-attempt pairs for the refreshed official Ubuntu
   large-distro role under `matched-plain-30`;
4. run `product`: two complete-attempt pairs each for Big Buck Bunny and
   Ubuntu under `product-default`; and
5. run `encryption`: one bounded Big Buck Bunny first-piece pair under
   `matched-rc4-30`, after the controlled full-download RC4 gate passes.

`complete-attempt` means both workers receive their full declared deadline and
all outcomes are retained; it does not assert that the public swarm completes.
Do not proceed from a suite that exposes an integrity, cleanup, privacy, or
hard-bound defect until that harness defect is fixed and the affected suite is
rerun. Ordinary timeout, reference unavailability, or a measured RSTorrent gap
does not authorize tuning in this tactical and does not invalidate healthy
earlier observations.

## Implementation Progress

### Deterministic foundation: 2026-08-10

- `tests/interop/public_compare_contract.py` now owns a runtime-independent
  catalog-v2 validator, hostile bounded v1 bencode/metainfo parser, exact raw
  info-hash and outer SHA-256 identity, normalized tracker/web-seed geometry,
  hashed comparison profiles, ABBA ordering inputs, network/disk math, cleanup
  ancestry, retained-report privacy checks, robust distributions, and the
  independent 1-MiB-buffer publication verifier.
- `tests/live/torrents.json` is schema version 2. Its five existing WebTorrent
  entries now carry source/license records, roles, allowed input modes,
  expected geometry, and per-entry limits. They remain magnet-only until the
  source-refresh stage pins exact metainfo; no third-party bytes were fetched
  or retained in this checkpoint.
- The matched profiles were reconciled with completed Tactical `124`: both
  owners use eight upload slots and unlimited upload rather than suppressing
  libtorrent upload. The explicit snapshots still record actual payload and
  protocol upload.
- Eighteen focused Python tests pass for the old comparator behavior and new
  contract, including cross-file/padding verification, corruption, unsafe
  paths, catalog identity, profile hashes, ABBA order, resource overflow,
  cleanup escape, privacy fields, and distribution math.
- The existing controlled release-mode adapter pair still publishes and
  independently file-hash-checks its 79,000-byte multi-file fixture for both
  owners, classifies `both_reached`, and cleans both roots. This compatibility
  run does not yet exercise direct-metainfo input or the new independent piece
  verifier through the orchestrator.

No public network access occurred. The next checkpoint isolates libtorrent in
its own worker process, moves both workers to schema version 2, and integrates
direct-metainfo input, process sampling, exact settings echo, and independent
verification into the orchestrator.

### Isolated workers and controlled schema-v2 gate: 2026-08-10

- The orchestrator no longer imports libtorrent. It starts one fresh process
  per owner, samples CPU and RSS, bounds stdout, hashes rather than retains
  stderr, enforces an outer terminate/kill deadline, and validates settings,
  identity, cleanup ancestry, and report privacy before retaining a result.
- The libtorrent worker owns exactly one pinned session and applies the
  complete symbolic profile, direct metainfo or magnet input, payload ceiling,
  aggregate alert/peer/transport/MSE telemetry, and terminal cleanup. Raw
  alerts, endpoints, save paths, and logs do not enter the report.
- The RSTorrent probe now uses the production resumable owner for both input
  modes. Direct metainfo is parsed under the explicit-import bounds, exact raw
  `info` bytes and ordered tracker tiers are supplied to the owner, and the
  probe supplies production-shaped peer-budget, MSE-DH, torrent-peer,
  incomplete-upload, cancellation, and publication ownership.
- Two explicit engine configuration switches support the matched harness
  without changing product defaults: BEP 11 can be disabled, and an initiated
  required-MSE stream can offer RC4 only. Focused engine tests prove both
  defaults plus RC4-only negotiation; the product continues to enable PEX and
  to allow both MSE payload methods unless a caller selects otherwise.
- A release-mode 1 MiB direct-metainfo multi-file control passes full
  publication and the independent piece verifier for both owners under
  plaintext and forced RC4. Both RC4 workers record an actual RC4 payload
  contributor and no plaintext contributor. This small control validates the
  adapters; it does not replace the required 1 GiB gate.
- Twenty focused Python tests now pass, including worker stderr privacy,
  orchestrator process isolation, settings/profile contracts, supervision,
  independent verification, bounds, ABBA order, classifications, and robust
  distributions. Six probe tests cover arguments, milestones, aggregation,
  and bounded timelines.

No public network access occurred. The next checkpoint is the release-mode
1 GiB plaintext and forced-RC4 controlled gate, followed by official catalog
refresh only if that gate remains exact and cleanup-safe.

### Controlled 1 GiB gate: 2026-08-10

The required release-mode direct-metainfo control used one 1,073,741,824-byte
multi-file v1 payload with 1 MiB pieces and a pinned libtorrent seeder. Both
owners independently verified every piece and published the exact file set in
both profiles; all four workers stopped and joined cleanly.

| Profile | Owner | Publication | Peak RSS | Payload method |
| --- | --- | ---: | ---: | --- |
| `matched-plain-30` | RSTorrent | 2.573 s | 135.9 MiB | plaintext stream |
| `matched-plain-30` | libtorrent | 1.830 s | 960.3 MiB | plaintext stream |
| `matched-rc4-30` | RSTorrent | 3.680 s | 127.8 MiB | RC4 |
| `matched-rc4-30` | libtorrent | 1.957 s | 1,014.5 MiB | RC4 |

The plaintext publication ratio was 1.41 and the forced-RC4 ratio was 1.88
for this single warm loopback fixture. Those values prove neither a stable
speed gap nor a causal crypto cost; the gate's authority is exact settings,
payload method, integrity, process/resource capture, and cleanup. Each profile
root was removed before the next profile began, and no payload or metainfo was
retained.

No public network access occurred. The next checkpoint refreshes official
catalog candidates, records reviewed exact identities and provenance, and
commits that catalog before any public payload worker starts.

## Non-Goals And Next Boundary

- No engine optimization, picker/scheduler retuning, new protocol capability,
  uTP graduation, incoming reachability, tracker implementation, storage
  architecture change, or connection-limit product change.
- No browser, Tauri window, Android UI, physical-device performance claim,
  remote daemon, benchmark service, CI WAN run, or CI speed threshold.
- No promise that a public torrent, release URL, tracker, peer population, or
  speed remains stable over time.
- No claim that matched settings make internal algorithms, exact peers,
  half-open accounting, kernel behavior, filesystem cache, or reciprocity
  identical.
- No retained public payload, metainfo, peer address, packet capture, raw log,
  or third-party benchmark fixture.
- No automatic regression threshold from the first cohort. A later tactical
  may define a low-frequency smoke or statistical alert only after repeated
  retained baselines demonstrate stable noise and operating cost.

After this tactical closes, the next slice is the narrowest owner-level
source-first change justified by the classified baseline, or a separately
approved recurring-smoke policy if no material gap is present. Ordinary
implementation details inside the declared harness boundaries do not require
human review. Stop for direction if evidence requires changing product
behavior, adding a dependency, retaining third-party data, exceeding the
declared public/disk limits, launching a visible or physical-device flow, or
expanding from measurement into engine optimization.
