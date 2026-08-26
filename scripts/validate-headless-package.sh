#!/usr/bin/env bash
set -euo pipefail

ARCHIVE="${1:-}"
if [ -z "$ARCHIVE" ] || [ ! -f "$ARCHIVE" ]; then
    echo "Usage: $0 <rstorrent-headless.tar.gz>" >&2
    exit 1
fi

ENTRIES="$(tar -tzf "$ARCHIVE")"
for required in \
    './ARCH' \
    './PACKAGE_ID' \
    './VERSION' \
    './install.sh' \
    './bin/rstorrent-headless' \
    './bin/rstorrent-gateway' \
    './resources/com.jstorrent.rstorrent.headless.service.in' \
    './resources/headless.toml.example' \
    './web/index.html'; do
    if ! grep -Fxq "$required" <<< "$ENTRIES"; then
        echo "Headless package is missing $required" >&2
        exit 1
    fi
done
if grep -Eq '(^/|(^|/)\.\.(/|$)|\\)' <<< "$ENTRIES"; then
    echo "Headless package contains an unsafe path." >&2
    exit 1
fi
if tar -tvzf "$ARCHIVE" | awk '$1 !~ /^[-d]/ { exit 1 }'; then
    :
else
    echo "Headless package contains a link or unsupported entry." >&2
    exit 1
fi

SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
tar -xzf "$ARCHIVE" -C "$SCRATCH"
FILE_COUNT="$(find "$SCRATCH" -type f | wc -l | tr -d ' ')"
BYTE_COUNT="$(find "$SCRATCH" -type f -exec stat -c '%s' {} + | awk '{ total += $1 } END { print total + 0 }')"
if [ "$FILE_COUNT" -gt 4105 ] || [ "$BYTE_COUNT" -gt 268435456 ]; then
    echo "Headless package exceeds its file or byte limit." >&2
    exit 1
fi
if [ ! -x "$SCRATCH/install.sh" ] || [ ! -x "$SCRATCH/bin/rstorrent-headless" ] ||
   [ ! -x "$SCRATCH/bin/rstorrent-gateway" ]; then
    echo "Headless package executables have incorrect modes." >&2
    exit 1
fi
if grep -R -n -E 'systemctl[[:space:]]+--user[[:space:]]+enable|loginctl[[:space:]]+enable-linger' \
    "$SCRATCH/resources" "$SCRATCH/install.sh"; then
    echo "Headless package unexpectedly enables a service or lingering." >&2
    exit 1
fi
if ! grep -Fq 'RestartPreventExitStatus=78' \
    "$SCRATCH/resources/com.jstorrent.rstorrent.headless.service.in" ||
   ! grep -Fq 'WantedBy=default.target' \
    "$SCRATCH/resources/com.jstorrent.rstorrent.headless.service.in"; then
    echo "Headless package service template is incomplete." >&2
    exit 1
fi
"$SCRATCH/bin/rstorrent-headless" validate-package --bundle "$SCRATCH"
HEADLESS_VERSION="$($SCRATCH/bin/rstorrent-headless --version)"
GATEWAY_VERSION="$($SCRATCH/bin/rstorrent-gateway --version)"
VERSION="$(tr -d '\n' < "$SCRATCH/VERSION")"
if [ "$HEADLESS_VERSION" != "rstorrent-headless $VERSION" ] ||
   [ "$GATEWAY_VERSION" != "rstorrent-gateway $VERSION" ]; then
    echo "Headless package binary versions do not match VERSION." >&2
    exit 1
fi
echo "RSTorrent headless package validation passed ($FILE_COUNT files, $BYTE_COUNT bytes)."
