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

The main local behavioral reference is normally `~/code/jstorrent`. External
reference repositories may be cloned outside this repository when useful.
Do not vendor or copy reference source into RSTorrent merely for convenient
reading.

Reference implementations do not determine RSTorrent architecture. Follow the
policy in [`docs/references.md`](docs/references.md) before adapting source,
fixtures, or test data.
