#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname -s)" != "Linux" ]; then
    echo "Headless packages must be built natively on Linux." >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VERSION="$(sed -n '/^\[package\]/,/^\[/s/^version = "\([^"]*\)"/\1/p' "$REPO_ROOT/crates/rstorrent-headless/Cargo.toml")"
case "$VERSION" in
    ''|*[!0-9A-Za-z.+_-]*|.*|*.) echo "Invalid headless package version: $VERSION" >&2; exit 1 ;;
esac
case "$(uname -m)" in
    x86_64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) echo "Unsupported headless architecture: $(uname -m)" >&2; exit 1 ;;
esac

cd "$REPO_ROOT"
VITE_RSTORRENT_DEFAULT_LIVE=same-origin npm run build --prefix clients/web
cargo build --release -p rstorrent-gateway -p rstorrent-headless

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT
mkdir -p "$STAGING/bin" "$STAGING/resources" "$STAGING/web" target/headless
install -m 0755 target/release/rstorrent-headless "$STAGING/bin/rstorrent-headless"
install -m 0755 target/release/rstorrent-gateway "$STAGING/bin/rstorrent-gateway"
cp -R clients/web/dist/. "$STAGING/web/"
find "$STAGING/web" -type d -exec chmod 0755 {} +
find "$STAGING/web" -type f -exec chmod 0644 {} +
install -m 0644 crates/rstorrent-headless/resources/com.jstorrent.rstorrent.headless.service.in \
    "$STAGING/resources/com.jstorrent.rstorrent.headless.service.in"
install -m 0644 crates/rstorrent-headless/resources/headless.toml.example \
    "$STAGING/resources/headless.toml.example"
install -m 0755 scripts/headless/install.sh "$STAGING/install.sh"
printf '%s\n' "$VERSION" > "$STAGING/VERSION"
printf '%s\n' 'com.jstorrent.rstorrent.headless' > "$STAGING/PACKAGE_ID"
printf '%s\n' "$ARCH" > "$STAGING/ARCH"
chmod 0644 "$STAGING/VERSION" "$STAGING/PACKAGE_ID" "$STAGING/ARCH"

OUTPUT="$REPO_ROOT/target/headless/rstorrent-headless-${VERSION}-linux-${ARCH}.tar.gz"
tar --sort=name --mtime='UTC 2020-01-01' --owner=0 --group=0 --numeric-owner \
    -czf "$OUTPUT" -C "$STAGING" .
"$REPO_ROOT/scripts/validate-headless-package.sh" "$OUTPUT"
printf '%s\n' "$OUTPUT"
