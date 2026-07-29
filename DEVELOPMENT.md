# RSTorrent Development

RSTorrent is currently a documentation-only repository. The first
implementation tactical should choose and justify the initial Rust workspace
shape rather than allowing a scaffold to make that decision accidentally.

## Starting A Session

Read these in order:

1. [`README.md`](README.md)
2. [`docs/topics/product-direction.md`](docs/topics/product-direction.md)
3. [`docs/references.md`](docs/references.md)
4. The active document under [`docs/tactical/`](docs/tactical/README.md), once
   one exists

Before changing an established continuing concern, look for and read its topic
under `docs/topics/`.

## Active Tactical

[`000-first-verified-piece.md`](docs/tactical/000-first-verified-piece.md) is
the draft first implementation slice. It defines a loopback download of one
multi-block, SHA-1-verified piece from a Rasterbar libtorrent peer and the
runtime-independent protocol boundary that the initial workspace must enforce.
Do not begin implementation until its draft decisions and prerequisites have
been reviewed.

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

Do not vendor, import, or commit reference source merely for convenient
reading. The ignored checkouts are not RSTorrent dependencies.

Reference implementations do not determine RSTorrent architecture. Follow the
policy in [`docs/references.md`](docs/references.md) before adapting source,
fixtures, or test data.
