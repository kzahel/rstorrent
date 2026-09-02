#!/usr/bin/env bash
set -euo pipefail

remote="${1:-origin}"
target_branch="${RSTORRENT_DEPLOY_AFTER_PUSH_BRANCH:-main}"
zero_oid_sha1="0000000000000000000000000000000000000000"
zero_oid_sha256="0000000000000000000000000000000000000000000000000000000000000000"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
scheduler="$script_dir/deploy-after-main-push.sh"

while read -r local_ref local_oid remote_ref remote_oid; do
    if [ "$remote_ref" != "refs/heads/$target_branch" ]; then
        continue
    fi
    if [ "$local_oid" = "$zero_oid_sha1" ] || \
        [ "$local_oid" = "$zero_oid_sha256" ]; then
        continue
    fi

    if ! "$scheduler" \
        --schedule \
        --remote "$remote" \
        --branch "$target_branch" \
        --sha "$local_oid"; then
        echo "pre-push: WARNING: failed to schedule the local headless deploy; continuing push." >&2
    fi
done

exit 0
