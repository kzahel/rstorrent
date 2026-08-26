#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
INSTALLER="$REPO_ROOT/website/public/install-crostini.sh"
MANIFEST_WRITER="$REPO_ROOT/.github/scripts/write-crostini-release-manifest.mjs"
ARCHIVE="${1:-}"
INSTALL_HOME="${2:-}"
MODE="${3:-install}"
SESSION_HOME="${HOME:-}"

if [ "$(uname -s)" != Linux ] ||
   [ -z "$ARCHIVE" ] || [ ! -f "$ARCHIVE" ] ||
   [ -z "$INSTALL_HOME" ] || [[ "$INSTALL_HOME" != /* ]] ||
   [ "$INSTALL_HOME" = / ]; then
    echo "Usage: $0 <native-package.tar.gz> <absolute-install-home> [install|tampered-manifest|tampered-signature|tampered-package|incompatible-protocol|wrong-architecture]" >&2
    exit 1
fi
case "$MODE" in
    install|tampered-manifest|tampered-signature|tampered-package|incompatible-protocol|wrong-architecture) ;;
    *) echo "Unknown fixture mode: $MODE" >&2; exit 1 ;;
esac
if [ "$MODE" = install ] && [ "$INSTALL_HOME" != "$SESSION_HOME" ]; then
    echo "The positive fixture must repair the current user's real home so systemd ownership remains coherent." >&2
    exit 1
fi
for command_name in curl node openssl python3; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "$command_name is required by the Crostini package fixture." >&2
        exit 1
    fi
done

FIXTURE_DIR=$(mktemp -d)
SERVER_PID=""
fixture_cleanup() {
    if [ -n "$SERVER_PID" ]; then
        kill "$SERVER_PID" >/dev/null 2>&1 || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -rf -- "$FIXTURE_DIR"
}
trap fixture_cleanup EXIT

export RSTORRENT_CROSTINI_INSTALLER_LIB_ONLY=1
# shellcheck source=/dev/null
source "$INSTALLER"
unset RSTORRENT_CROSTINI_INSTALLER_LIB_ONLY

ARCH=$(detect_architecture "$(uname -m)")
VERSION=$(tar -xOf "$ARCHIVE" ./VERSION)
if [[ ! "$VERSION" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "Fixture package has an invalid VERSION." >&2
    exit 1
fi
EXPECTED_NAME="rstorrent-crostini-${VERSION}-${ARCH}.tar.gz"
if [ "$(basename "$ARCHIVE")" != "$EXPECTED_NAME" ]; then
    echo "Fixture package must be named $EXPECTED_NAME." >&2
    exit 1
fi

SERVE_DIR="$FIXTURE_DIR/serve"
mkdir "$SERVE_DIR"
cp "$ARCHIVE" "$SERVE_DIR/$EXPECTED_NAME"
OTHER_ARCH=x86_64
if [ "$ARCH" = x86_64 ]; then OTHER_ARCH=aarch64; fi
cp "$ARCHIVE" "$SERVE_DIR/rstorrent-crostini-${VERSION}-${OTHER_ARCH}.tar.gz"
node "$MANIFEST_WRITER" \
    "$VERSION" 0123456789abcdef0123456789abcdef01234567 \
    "$SERVE_DIR/rstorrent-crostini-${VERSION}-x86_64.tar.gz" \
    "$SERVE_DIR/rstorrent-crostini-${VERSION}-aarch64.tar.gz" \
    "$SERVE_DIR/$MANIFEST_NAME"
case "$MODE" in
    incompatible-protocol)
        sed -i 's/^launch_protocol=1$/launch_protocol=2/' "$SERVE_DIR/$MANIFEST_NAME"
        ;;
    wrong-architecture)
        sed -i \
            "s#^${ARCH}_asset=${EXPECTED_NAME}\$#${ARCH}_asset=rstorrent-crostini-${VERSION}-${OTHER_ARCH}.tar.gz#" \
            "$SERVE_DIR/$MANIFEST_NAME"
        ;;
esac

openssl genpkey -algorithm Ed25519 -out "$FIXTURE_DIR/test.key" >/dev/null 2>&1
openssl pkey -in "$FIXTURE_DIR/test.key" -pubout -outform DER \
    -out "$FIXTURE_DIR/test-public.der" >/dev/null 2>&1
printf '\105\144' > "$FIXTURE_DIR/test-public.packet"
printf '\001\002\003\004\005\006\007\010' >> "$FIXTURE_DIR/test-public.packet"
tail -c 32 "$FIXTURE_DIR/test-public.der" >> "$FIXTURE_DIR/test-public.packet"
FIXTURE_PUBLIC_KEY=$(base64 < "$FIXTURE_DIR/test-public.packet" | tr -d '\n')

openssl dgst -blake2b512 -binary "$SERVE_DIR/$MANIFEST_NAME" \
    > "$FIXTURE_DIR/manifest.digest"
openssl pkeyutl -sign -inkey "$FIXTURE_DIR/test.key" -rawin \
    -in "$FIXTURE_DIR/manifest.digest" -out "$FIXTURE_DIR/manifest.signature"
printf '\105\104' > "$FIXTURE_DIR/signature.packet"
printf '\001\002\003\004\005\006\007\010' >> "$FIXTURE_DIR/signature.packet"
cat "$FIXTURE_DIR/manifest.signature" >> "$FIXTURE_DIR/signature.packet"
{
    echo 'untrusted comment: RSTorrent local package fixture'
    base64 < "$FIXTURE_DIR/signature.packet" | tr -d '\n'
    echo
    echo 'trusted comment: non-production fixture key'
    dd if=/dev/zero bs=64 count=1 status=none | base64 | tr -d '\n'
    echo
} > "$SERVE_DIR/$SIGNATURE_NAME"
case "$MODE" in
    tampered-manifest)
        printf 'unsigned manifest mutation\n' >> "$SERVE_DIR/$MANIFEST_NAME"
        ;;
    tampered-signature)
        sed -i '2s/^./A/' "$SERVE_DIR/$SIGNATURE_NAME"
        ;;
    tampered-package)
        printf 'unsigned package mutation\n' >> "$SERVE_DIR/$EXPECTED_NAME"
        ;;
esac

PORT_FILE="$FIXTURE_DIR/port"
python3 - "$SERVE_DIR" "$PORT_FILE" <<'PY' &
import http.server
import os
import sys

directory, port_file = sys.argv[1:]
class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=directory, **kwargs)

    def log_message(self, *_args):
        pass

    def copyfile(self, source, output):
        try:
            super().copyfile(source, output)
        except BrokenPipeError:
            pass

handler = QuietHandler
server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
with open(port_file, "w", encoding="ascii") as output:
    output.write(str(server.server_port))
    output.flush()
    os.fsync(output.fileno())
server.serve_forever()
PY
SERVER_PID=$!
for _attempt in {1..50}; do
    if [ -s "$PORT_FILE" ]; then break; fi
    sleep 0.1
done
test -s "$PORT_FILE"
FIXTURE_URL="http://127.0.0.1:$(sed -n '1p' "$PORT_FILE")"

download_release_metadata() {
    local _requested_version="$1"
    local destination="$2"
    curl -fsSL "$FIXTURE_URL/$MANIFEST_NAME" -o "$destination/$MANIFEST_NAME"
    curl -fsSL "$FIXTURE_URL/$SIGNATURE_NAME" -o "$destination/$SIGNATURE_NAME"
}
download_release_asset() {
    local _release_url="$1"
    local output="$2"
    local maximum_size="$3"
    curl -fsSL --max-filesize "$maximum_size" \
        "$FIXTURE_URL/$(basename "$output")" -o "$output"
}

MINISIGN_PUBLIC_KEY="$FIXTURE_PUBLIC_KEY"
HOME="$INSTALL_HOME"
export HOME
mkdir -p "$HOME"
snapshot_installation() {
    local inventory="$FIXTURE_DIR/inventory"
    : > "$inventory"
    local path
    for path in \
        "$HOME/.local/bin/rstorrent-crostini" \
        "$HOME/.local/share/applications/com.jstorrent.rstorrent.crostini.desktop" \
        "$HOME/.local/share/icons/hicolor/128x128/apps/com.jstorrent.rstorrent.crostini.png" \
        "$HOME/.local/share/rstorrent-crostini/current" \
        "$HOME/.local/share/rstorrent-crostini/ownership-v1" \
        "$HOME/.local/share/rstorrent-crostini/versions" \
        "$HOME/.config/systemd/user/com.jstorrent.rstorrent.crostini.service"; do
        if [ -e "$path" ] || [ -L "$path" ]; then
            find -P "$path" -printf '%y %m %p %l\n' >> "$inventory"
            find -P "$path" -type f -exec sha256sum {} + >> "$inventory"
        fi
    done
    LC_ALL=C sort "$inventory" | sha256sum | awk '{ print $1 }'
}

if [ "$MODE" != install ]; then
    BEFORE=$(snapshot_installation)
    if installer_main --version "$VERSION"; then
        echo "FAIL: bootstrap accepted $MODE fixture input." >&2
        exit 1
    fi
    AFTER=$(snapshot_installation)
    if [ "$BEFORE" != "$AFTER" ]; then
        echo "FAIL: rejected $MODE input changed the installation." >&2
        exit 1
    fi
    printf '%s\n' "Crostini bootstrap rejected $MODE input without installation mutation."
    exit 0
fi

installer_main --version "$VERSION"
test "$("$HOME/.local/bin/rstorrent-crostini" --version)" = \
    "rstorrent-crostini $VERSION"
printf '%s\n' "Crostini bootstrap consumed the locally served native $ARCH package."
