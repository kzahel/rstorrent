# RSTorrent Development

RSTorrent has a pure protocol core, a Tokio runtime, an application service,
first-party desktop and Android clients, and loopback libtorrent
interoperability harnesses. Product clients explicitly use online tracker and
peer networking while diagnostic tools retain loopback-only policy. It is not
yet a generally useful torrent client.

## Starting A Session

Read these in order:

1. [`README.md`](README.md)
2. [`docs/vision.md`](docs/vision.md)
3. [`docs/engineering-principles.md`](docs/engineering-principles.md)
4. [`docs/topics/product-direction.md`](docs/topics/product-direction.md)
5. [`docs/topics/capability-readiness.md`](docs/topics/capability-readiness.md)
6. [`docs/references.md`](docs/references.md)
7. The active document under [`docs/tactical/`](docs/tactical/README.md), once
   one exists

Before changing an established continuing concern, look for and read its topic
under `docs/topics/`.

## Current Tactical State

[`001-bounded-large-piece.md`](docs/tactical/001-bounded-large-piece.md) is
complete. It replaced the first slice's piece-sized allocation with a
budgeted 16 KiB block pipeline, unverified staging storage, and streamed
verification of a 32 MiB piece under a 256 KiB payload allowance.

[`000-first-verified-piece.md`](docs/tactical/000-first-verified-piece.md)
remains the completed execution record for the initial protocol/runtime
vertical slice.

[`002-selective-multi-file-storage.md`](docs/tactical/002-selective-multi-file-storage.md)
is complete. It established bounded multi-file parsing and mapping, selected
staging, compact skipped-file part slots, streamed mixed-source verification,
padding omission, durable reopen, and verified materialization.

[`003-android-storage-feasibility.md`](docs/tactical/003-android-storage-feasibility.md)
is complete on `jstorrent-tablet`, Chromebook ARCVM, a physical Pixel 7a, and
a physical Moto X4 using both internal and removable exFAT storage. It proved
fixed-buffer native file-descriptor operations, persisted SAF reopen,
descriptor ownership, cancellable termination, staging publication, and exact
cleanup. The exFAT result also showed that sparse-mode file growth may perform
full allocation and block on a destination that does not preserve holes.

[`004-android-engine-bootstrap.md`](docs/tactical/004-android-engine-bootstrap.md)
is complete. It packaged the actual engine behind UniFFI in a foreground
service and passed the edge-rich selective fixture, bounded lifecycle,
cancellation, failure, and cleanup matrix on the AVD, Chromebook ARCVM, and
Moto X4.

[`005-saf-selective-storage.md`](docs/tactical/005-saf-selective-storage.md)
is complete. It connected the selective engine to user-granted SAF documents
through synchronously duplicated descriptors, explicit native preparation and
provider publication phases, restart verification, and bounded cleanup.

[`006-magnet-metadata-peer-hint.md`](docs/tactical/006-magnet-metadata-peer-hint.md)
is complete. It added bounded v1 magnet parsing, direct peer-hint bootstrap,
and bidirectional BEP 9 metadata exchange.

[`007-durable-session-control.md`](docs/tactical/007-durable-session-control.md)
is complete. It established the SQLite-backed application service, semantic
commands, verified metadata retention, piece checkpoints, and conservative
restart.

[`008-reactive-multi-surface-control.md`](docs/tactical/008-reactive-multi-surface-control.md)
is complete. It added bounded recoverable reactive views and generated client
types, then proved the same controlled pause/resume download through Chrome
over authenticated WebSocket, Tauri over commands and Channels, and Android
Compose/UniFFI under foreground-service ownership.

[`009-android-saf-session-storage.md`](docs/tactical/009-android-saf-session-storage.md)
through
[`012-bounded-diagnostics-progress.md`](docs/tactical/012-bounded-diagnostics-progress.md)
are complete. They connected Android to durable SAF session storage, added
bounded peer and one-shot UDP tracker lifecycles, and established equivalent
typed progress and diagnostics on the shared web and Compose surfaces.

[`013-explicit-live-network-policy.md`](docs/tactical/013-explicit-live-network-policy.md)
is complete. It gives every runtime an explicit offline, loopback-only, or
online outbound policy, selects online networking for desktop and Android,
and replaces the former whole-download timeout with bounded network-operation
deadlines.

[`014-scheduled-udp-tracker-lifecycle.md`](docs/tactical/014-scheduled-udp-tracker-lifecycle.md)
is complete. It replaces one-shot tracker exhaustion with supervised
per-torrent UDP scheduling, multi-tracker fallback and promotion, bounded
retransmission and connection-token reuse, automatic reannounce, and
equivalent waiting/retry diagnostics in the web and Android clients.

Tactical `016` completed the bounded session-owned IPv4 DHT foundation with
private-torrent gating and useful warm restart. Tactical `017` completed the
bounded multi-peer connection/request owner, parallel metadata acquisition,
late discovery, expiry, replacement, failover, and ordinary multi-piece
single-file execution. Endgame and integrity recovery are the next reliability
slice; Tactical `015` remains the independent paired live-comparison work. See
[`docs/topics/capability-readiness.md`](docs/topics/capability-readiness.md),
[`docs/topics/peer-lifecycle.md`](docs/topics/peer-lifecycle.md), and
[`docs/topics/download-correctness.md`](docs/topics/download-correctness.md).

Tactical `033` completed the leased view-set, generated contract, polling
client, and headless application boundary. Tactical `034` completed the fresh
responsive React/Zustand/CSS Modules inspection surface, virtual torrent and
peer grids, and permanent deterministic named demo adapter. Tactical `035`
completed stable Rust torrent and active-peer projections, the semantic live
adapter, independently reaped view sets, and suspended-client recovery; see
[`docs/topics/web-ui-design.md`](docs/topics/web-ui-design.md). Tactical `041`
adds the complete selected-torrent Files projection, independent Done and
Verified progress, configurable virtual columns, and a 4,096-row scale
scenario through the same headless surface.

Run the controlled production-web peer inspection proof without launching a
visible browser or Tauri window with:

```bash
source ~/.profile
uv run --project tests/interop --locked \
  python tests/interop/browser_peer_inspection_surface.py
```

Pass `--screenshot-dir target/headless-evidence/t041-live-files` to retain
loopback-only peer and Files wide, compact, phone, and reconnecting captures.
The harness creates and removes temporary application, browser, seed, and
download state, uses the pinned Python libtorrent environment, verifies
payload SHA-1, and requires every child process to join.

Add `--disk-pressure` to run the isolated Tactical `044` storage proof. It
uses only loopback traffic, injects a bounded slow-write profile into the
unauthenticated development gateway, verifies high/low pressure recovery and
exact output, and can retain Disk screenshots through the same
`--screenshot-dir` option.

Add `--piece-map` to run the isolated Tactical `045` Pieces proof. A bounded
loopback seed drives a 17-piece transfer through active work, deliberate
view-set lease expiry, fresh-snapshot recovery, exact verified completion, and
external payload comparison. The option can retain active and complete Canvas
screenshots without launching a visible client.

## Toolchain

On the maintainer's configured development machines, load installed Rust,
Android, Java, and related toolchains with:

```bash
source ~/.profile
```

Once a Rust workspace exists, the expected default validation baseline is:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

## Launching The Desktop App

Install the locked web dependencies when necessary, build the static web
application and optimized debug Tauri binary, then launch without a Vite
server, listening port, or installer bundle:

```bash
./scripts/desktop
```

The wrapper runs npm from `clients/web`; no separate npm command is required.
Later runs reuse installed dependencies and Cargo build output while still
refreshing the static web assets. The process remains attached to the terminal
so `Ctrl+C` stops it.

## Launching The Live Web UI

The new React inspection surface can run against a real online application
service in the normal browser without launching Tauri:

```bash
./scripts/webui
```

The launcher installs locked web dependencies when needed, builds production
web assets and the Rust gateway, starts both servers on loopback, and opens the
exact live URL in the default browser. It remains attached to the terminal;
one `Ctrl+C` gracefully stops and joins both servers. The browser tab itself
may remain open and will reconnect after the next launch.

Web UI state and downloads persist beneath `.local/webui`, which is separate
from the Tauri application profile and ignored by Git. Override that location
with `RSTORRENT_WEBUI_DATA_ROOT` or the preview port with
`RSTORRENT_WEBUI_PORT`. `./scripts/webui --no-open` starts the same servers
without disturbing the visible browser and is the automation/debug form.

The launcher selects online torrent networking by default. The current live
React surface supports magnet intake, inspection, pause, and resume. Paste a
magnet into the toolbar and use Add or Enter; unsupported remote `.torrent`
URLs are rejected without clearing the input. More > Add test torrent exposes
the five recorded WebTorrent magnets for quick interactive testing. These are
variable public swarms, not deterministic success fixtures. Local `.torrent`
file selection is reserved for a later slice. Switching Tauri to this surface
and deciding how to migrate or redesign the categorized Logs view remain
separate work.

## Exercising The Frontend Headlessly

The named demo adapter drives the responsive React inspection application
without launching Tauri, starting Rust, or using torrent networking. Start the
local development host when manual browser inspection is wanted:

```bash
npm run dev --prefix clients/web -- --host 127.0.0.1
```

Then open a deterministic scenario such as:

```text
http://127.0.0.1:5173/?demo=healthy-download&at=42000&autoplay=0
http://127.0.0.1:5173/?demo=tracker-recovery&at=30000&autoplay=0
http://127.0.0.1:5173/?demo=large-swarm&at=0&autoplay=0
```

The stable scenario IDs are `healthy-download`, `stalled-metadata`,
`tracker-recovery`, `endgame`, `large-swarm`, `disk-error`, and
`empty-library`. Omit `autoplay=0` for a running demo clock. Headless Chrome
validates wide, compact, phone, accessibility, keyboard, and virtualized-scale
behavior without touching the visible desktop:

```bash
npm run test:e2e --prefix clients/web
```

The deterministic first-piece interoperability scenario uses its own locked
Python environment and a loopback-only Rasterbar libtorrent seed:

```bash
uv run --project tests/interop --locked \
  python tests/interop/first_verified_piece.py --runs 3
```

Tactical `001`'s bounded large-piece profile streams a deterministic 32 MiB
fixture through a 256 KiB engine-owned payload allowance:

```bash
uv run --project tests/interop --locked \
  python tests/interop/first_verified_piece.py --large-piece --runs 3
```

Tactical `002`'s selective profile forces boundary, skipped-only, padding,
zero-length, final-short, reopen, and materialization behavior:

```bash
uv run --project tests/interop --locked \
  python tests/interop/first_verified_piece.py --selective-files --runs 3
```

The representative selective-hash profile downloads 32 MiB in 128 pieces
across three unaligned wanted files. It checks exact publication and cleanup
while retaining transfer-only timings for storage-operation changes:

```bash
uv run --project tests/interop --locked \
  python tests/interop/selective_hash_profile.py --runs 3
```

The controlled DHT profile obtains peers from an independent KRPC router,
downloads metadata and content from libtorrent, and then probes RSTorrent's
incoming query and token-authenticated announcement behavior:

```bash
uv run --project tests/interop --locked \
  python tests/interop/dht_magnet.py
```

The controlled mixed-peer profile keeps a scripted, valid, permanently choked
peer in the content swarm while pinned libtorrent supplies and accounts for a
16-piece single-file payload:

```bash
uv run --project tests/interop --locked \
  python tests/interop/multi_peer_liveness.py
```

Tactical `003`'s self-contained Android probe builds both supported native
ABIs and targets only an explicitly verified environment:

```bash
experiments/android-storage-probe/build_probe.sh
python3 experiments/android-storage-probe/run_probe.py \
  --target avd --avd jstorrent-tablet --runs 3 --no-build
python3 experiments/android-storage-probe/run_probe.py \
  --target chromeos --runs 3 --no-build
python3 experiments/android-storage-probe/run_probe.py \
  --target pixel7a --runs 3 --no-build
python3 experiments/android-storage-probe/run_probe.py \
  --target motox4 --storage internal --runs 3 --no-build
python3 experiments/android-storage-probe/run_probe.py \
  --target motox4 --storage sdcard --runs 3 --no-build
```

Android and desktop tacticals should add the smallest meaningful build, smoke,
interoperability, and physical-device gates for the behavior they introduce.
Record exactly what ran in the tactical.

Documentation-only changes should at least run:

```bash
git diff --check
```

## Local Reference Checkouts

Reproduce and validate the local reference set with:

```bash
python3 scripts/references.py sync
python3 scripts/references.py status
```

Pinned external source checkouts live under the gitignored `reference/`
directory. The main local behavioral reference remains the first-party sibling
at `~/code/jstorrent`. See [`reference/README.md`](reference/README.md) and
[`reference/pins.toml`](reference/pins.toml) for their roles and exact
revisions.

The original BEP sources are available offline after sync under
`reference/bittorrent.org/beps/`. Prefer them to the older converted copies in
JSTorrent.

Do not vendor, import, or commit reference source merely for convenient
reading. The ignored checkouts are not RSTorrent dependencies.

Reference implementations do not determine RSTorrent architecture. Follow the
policy in [`docs/references.md`](docs/references.md) before adapting source,
fixtures, or test data.
