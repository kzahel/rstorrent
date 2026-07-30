# RSTorrent Development

RSTorrent now has its first bounded engine slice: a pure protocol crate, a
Tokio runtime crate, and a loopback libtorrent interoperability harness. It is
not yet a generally useful torrent client.

## Starting A Session

Read these in order:

1. [`README.md`](README.md)
2. [`docs/vision.md`](docs/vision.md)
3. [`docs/engineering-principles.md`](docs/engineering-principles.md)
4. [`docs/topics/product-direction.md`](docs/topics/product-direction.md)
5. [`docs/references.md`](docs/references.md)
6. The active document under [`docs/tactical/`](docs/tactical/README.md), once
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

Build the static web application and the optimized debug Tauri binary, then
launch it without a Vite server or installer bundle:

```bash
./scripts/desktop
```

The first run installs the locked web dependencies when necessary. Later runs
reuse both npm and Cargo build output. The process remains attached to the
terminal so `Ctrl+C` stops it.

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
