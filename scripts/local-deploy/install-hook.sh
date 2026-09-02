#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
hooks_path="$(git -C "$repo_root" config --local --get core.hooksPath || true)"

if [ -n "$hooks_path" ] && [ "$hooks_path" != ".githooks" ]; then
    echo "refusing to replace existing core.hooksPath: $hooks_path" >&2
    exit 1
fi
if [ ! -x "$repo_root/.githooks/pre-push" ]; then
    echo "tracked pre-push hook is not executable" >&2
    exit 1
fi

git -C "$repo_root" config --local core.hooksPath .githooks
echo "Installed RSTorrent local deploy hook through core.hooksPath=.githooks"
echo "Status: $repo_root/scripts/local-deploy/deploy-after-main-push.sh --status"
echo "Log:    $repo_root/scripts/local-deploy/deploy-after-main-push.sh --log"
echo "Stop:   $repo_root/scripts/local-deploy/deploy-after-main-push.sh --stop"
