#!/usr/bin/env bash
set -euo pipefail

ARCHIVE="${1:-}"
if [ -z "$ARCHIVE" ] || [ ! -f "$ARCHIVE" ]; then
    echo "Usage: $0 <rstorrent-crostini.tar.gz>" >&2
    exit 1
fi

ENTRIES="$(tar -tzf "$ARCHIVE")"
for required in \
    './VERSION' \
    './install.sh' \
    './bin/rstorrent-crostini' \
    './bin/rstorrent-gateway' \
    './icons/rstorrent-128.png' \
    './web/index.html'; do
    if ! grep -Fxq "$required" <<< "$ENTRIES"; then
        echo "Crostini package is missing $required" >&2
        exit 1
    fi
done
if grep -Eq '(^/|(^|/)\.\.(/|$)|\\)' <<< "$ENTRIES"; then
    echo "Crostini package contains an unsafe path." >&2
    exit 1
fi
if tar -tvzf "$ARCHIVE" | awk '$1 !~ /^[-d]/ { exit 1 }'; then
    :
else
    echo "Crostini package contains a link or unsupported entry." >&2
    exit 1
fi

SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
tar -xzf "$ARCHIVE" -C "$SCRATCH"
FILE_COUNT="$(find "$SCRATCH" -type f | wc -l | tr -d ' ')"
BYTE_COUNT="$(find "$SCRATCH" -type f -exec stat -c '%s' {} + | awk '{ total += $1 } END { print total + 0 }')"
if [ "$FILE_COUNT" -gt 4104 ] || [ "$BYTE_COUNT" -gt 268435456 ]; then
    echo "Crostini package exceeds its file or byte limit." >&2
    exit 1
fi
if [ ! -x "$SCRATCH/install.sh" ] || [ ! -x "$SCRATCH/bin/rstorrent-crostini" ] ||
   [ ! -x "$SCRATCH/bin/rstorrent-gateway" ]; then
    echo "Crostini package executables have incorrect modes." >&2
    exit 1
fi
if grep -R -n -E 'systemctl[[:space:]]+--user[[:space:]]+enable|loginctl[[:space:]]+enable-linger' "$SCRATCH" --exclude='rstorrent-crostini' --exclude='rstorrent-gateway'; then
    echo "Crostini package unexpectedly enables a service or lingering." >&2
    exit 1
fi
echo "RSTorrent Crostini package validation passed ($FILE_COUNT files, $BYTE_COUNT bytes)."
