#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/../.." && pwd)"
script_path="$script_dir/deploy-after-main-push.sh"
git_dir="$(git -C "$project_dir" rev-parse --git-dir)"
case "$git_dir" in
    /*) ;;
    *) git_dir="$project_dir/$git_dir" ;;
esac

state_dir="$git_dir/rstorrent-deploy-after-main-push"
desired_file="$state_dir/desired.tsv"
summary_file="$state_dir/summary.tsv"
completed_file="$state_dir/completed.tsv"
failed_file="$state_dir/failed.tsv"
lock_dir="$state_dir/worker.lock"
log_file="$state_dir/deploy.log"
previous_log="$state_dir/deploy.previous.log"

default_remote="${RSTORRENT_DEPLOY_AFTER_PUSH_REMOTE:-origin}"
default_branch="${RSTORRENT_DEPLOY_AFTER_PUSH_BRANCH:-main}"
poll_seconds="${RSTORRENT_DEPLOY_AFTER_PUSH_POLL_SECONDS:-5}"
settle_seconds="${RSTORRENT_DEPLOY_AFTER_PUSH_SETTLE_SECONDS:-5}"
max_wait_seconds="${RSTORRENT_DEPLOY_AFTER_PUSH_MAX_WAIT_SECONDS:-900}"
max_log_bytes="${RSTORRENT_DEPLOY_AFTER_PUSH_MAX_LOG_BYTES:-8388608}"
retain_log_bytes="${RSTORRENT_DEPLOY_AFTER_PUSH_RETAIN_LOG_BYTES:-4194304}"

snapshot_dir=""
install_dir=""

usage() {
    cat <<EOF
Usage:
  $0 --schedule --remote REMOTE --branch BRANCH --sha SHA
  $0 --worker
  $0 --status
  $0 --log
  $0 --stop

The pre-push hook calls --schedule. A detached worker waits until the remote
branch reports the pushed SHA, builds from a temporary Git archive snapshot,
and redeploys the ordinary-user headless service.
EOF
}

timestamp() {
    date -u +%Y-%m-%dT%H:%M:%SZ
}

epoch_seconds() {
    date -u +%s
}

require_positive_integer() {
    local name="$1"
    local value="$2"
    case "$value" in
        ''|*[!0-9]*|0)
            echo "$name must be a positive integer" >&2
            exit 2
            ;;
    esac
}

validate_configuration() {
    require_positive_integer RSTORRENT_DEPLOY_AFTER_PUSH_POLL_SECONDS "$poll_seconds"
    require_positive_integer RSTORRENT_DEPLOY_AFTER_PUSH_SETTLE_SECONDS "$settle_seconds"
    require_positive_integer RSTORRENT_DEPLOY_AFTER_PUSH_MAX_WAIT_SECONDS "$max_wait_seconds"
    require_positive_integer RSTORRENT_DEPLOY_AFTER_PUSH_MAX_LOG_BYTES "$max_log_bytes"
    require_positive_integer RSTORRENT_DEPLOY_AFTER_PUSH_RETAIN_LOG_BYTES "$retain_log_bytes"
    if [ "$retain_log_bytes" -gt "$max_log_bytes" ]; then
        echo "retained deploy log bytes cannot exceed the maximum" >&2
        exit 2
    fi
}

write_tsv() {
    local path="$1"
    local temporary="$path.$$"
    shift
    printf '%s' "$1" > "$temporary"
    shift
    while (($#)); do
        printf '\t%s' "$1" >> "$temporary"
        shift
    done
    printf '\n' >> "$temporary"
    mv "$temporary" "$path"
}

append_log() {
    mkdir -p "$state_dir"
    printf '[%s] %s\n' "$(timestamp)" "$*" >> "$log_file"
}

log() {
    printf '[%s] %s\n' "$(timestamp)" "$*"
}

write_summary() {
    local state="$1"
    local sha="${2:-}"
    local remote="${3:-}"
    local branch="${4:-}"
    local detail="${5:-}"
    local total_seconds="${6:-}"
    local deploy_seconds="${7:-}"
    mkdir -p "$state_dir"
    write_tsv "$summary_file" \
        "$(timestamp)" "$state" "$sha" "$remote" "$branch" \
        "$total_seconds" "$deploy_seconds" "$detail"
}

record_failure() {
    local sha="$1"
    local remote="$2"
    local branch="$3"
    local kind="$4"
    local detail="$5"
    local total_seconds="${6:-}"
    local deploy_seconds="${7:-}"
    write_tsv "$failed_file" \
        "$sha" "$remote" "$branch" "$(timestamp)" "$kind" \
        "$total_seconds" "$deploy_seconds" "$detail"
    write_summary \
        failed "$sha" "$remote" "$branch" "$kind: $detail" \
        "$total_seconds" "$deploy_seconds"
}

read_desired() {
    [ -f "$desired_file" ] || return 1
    IFS=$'\t' read -r \
        desired_sha desired_remote desired_branch desired_at desired_epoch \
        < "$desired_file" || return 1
    desired_epoch="${desired_epoch:-0}"
    [ -n "${desired_sha:-}" ] && \
        [ -n "${desired_remote:-}" ] && \
        [ -n "${desired_branch:-}" ]
}

desired_still_matches() {
    local sha="$1"
    local remote="$2"
    local branch="$3"
    read_desired || return 1
    [ "$desired_sha" = "$sha" ] && \
        [ "$desired_remote" = "$remote" ] && \
        [ "$desired_branch" = "$branch" ]
}

elapsed_since_desired() {
    local now
    now="$(epoch_seconds)"
    if [ "${desired_epoch:-0}" -gt 0 ] 2>/dev/null; then
        printf '%s' "$((now - desired_epoch))"
    else
        printf ''
    fi
}

remote_head_sha() {
    local remote="$1"
    local branch="$2"
    git -C "$project_dir" ls-remote --heads "$remote" "refs/heads/$branch" |
        awk -v ref="refs/heads/$branch" '$2 == ref { print $1; exit }'
}

worker_is_active() {
    local pid="$1"
    case "$pid" in
        ''|*[!0-9]*) return 1 ;;
    esac
    kill -0 "$pid" 2>/dev/null || return 1
    if [ -r "/proc/$pid/cmdline" ]; then
        tr '\0' ' ' < "/proc/$pid/cmdline" | grep -Fq -- "$script_path --worker"
    else
        return 0
    fi
}

acquire_lock() {
    mkdir -p "$state_dir"
    while ! mkdir "$lock_dir" 2>/dev/null; do
        local pid=""
        if [ -f "$lock_dir/pid" ]; then
            read -r pid < "$lock_dir/pid" || true
        fi
        if worker_is_active "$pid"; then
            log "another deploy worker is active as pid $pid; exiting"
            return 1
        fi
        log "removing stale deploy worker lock"
        rm -rf -- "$lock_dir"
    done
    printf '%s\n' "$$" > "$lock_dir/pid"
}

remove_temporary_directory() {
    local path="$1"
    [ -n "$path" ] || return 0
    case "$path" in
        "$state_dir"/source.*|"$state_dir"/install.*)
            rm -rf -- "$path"
            ;;
        *)
            echo "refusing to remove unexpected temporary path: $path" >&2
            return 1
            ;;
    esac
}

clear_temporary_directories() {
    remove_temporary_directory "$install_dir"
    install_dir=""
    remove_temporary_directory "$snapshot_dir"
    snapshot_dir=""
}

trim_log() {
    [ -f "$log_file" ] || return 0
    local size
    size="$(wc -c < "$log_file")"
    if [ "$size" -le "$max_log_bytes" ]; then
        return 0
    fi
    local temporary="$log_file.trim.$$"
    tail -c "$retain_log_bytes" "$log_file" > "$temporary"
    : > "$log_file"
    cat "$temporary" >> "$log_file"
    rm -f -- "$temporary"
}

cleanup_worker() {
    local status=$?
    clear_temporary_directories || true
    if [ -f "$lock_dir/pid" ]; then
        local owner=""
        read -r owner < "$lock_dir/pid" || true
        if [ "$owner" = "$$" ]; then
            rm -rf -- "$lock_dir"
        fi
    fi
    trim_log || true
    return "$status"
}

stop_on_signal() {
    local status="$1"
    local signal_name="$2"
    log "deploy worker received $signal_name"
    write_summary stopped \
        "${desired_sha:-}" "${desired_remote:-}" "${desired_branch:-}" \
        "worker stopped by $signal_name"
    exit "$status"
}

wait_for_settle_window() {
    local sha="$1"
    local remote="$2"
    local branch="$3"
    local until_seconds=$((SECONDS + settle_seconds))
    while ((SECONDS < until_seconds)); do
        local remaining=$((until_seconds - SECONDS))
        local sleep_for="$poll_seconds"
        if ((remaining < sleep_for)); then
            sleep_for="$remaining"
        fi
        if ((sleep_for > 0)); then
            sleep "$sleep_for"
        fi
        if ! desired_still_matches "$sha" "$remote" "$branch"; then
            log "a newer push replaced $sha during the settle window"
            return 1
        fi
    done
}

prepare_snapshot() {
    local sha="$1"
    local resolved
    resolved="$(git -C "$project_dir" rev-parse "$sha^{commit}")" || return $?
    if [ "$resolved" != "$sha" ]; then
        echo "desired commit resolved to unexpected object $resolved" >&2
        return 1
    fi
    snapshot_dir="$(mktemp -d "$state_dir/source.XXXXXX")" || return $?
    git -C "$project_dir" archive --format=tar "$sha" |
        tar -xf - -C "$snapshot_dir" || return $?
}

build_and_install_snapshot() {
    local sha="$1"
    local version architecture archive

    log "hydrating exact web dependencies for $sha"
    (
        cd "$snapshot_dir"
        npm ci --prefix clients/web --no-audit --no-fund
    ) 2>&1 | tail -c "$retain_log_bytes" || return $?
    trim_log || return $?

    log "building release binaries for $sha"
    (
        cd "$snapshot_dir"
        CARGO_TARGET_DIR="$project_dir/target" \
            cargo build --locked --release \
                -p rstorrent-gateway -p rstorrent-headless
    ) 2>&1 | tail -c "$retain_log_bytes" || return $?
    trim_log || return $?

    log "building and validating the headless package for $sha"
    (
        cd "$snapshot_dir"
        scripts/build-headless-package.sh \
            --binary-directory "$project_dir/target/release"
    ) 2>&1 | tail -c "$retain_log_bytes" || return $?
    trim_log || return $?

    version="$(sed -n '/^\[package\]/,/^\[/s/^version = "\([^"]*\)"/\1/p' \
        "$snapshot_dir/crates/rstorrent-headless/Cargo.toml")"
    case "$(uname -m)" in
        x86_64) architecture="x86_64" ;;
        aarch64|arm64) architecture="aarch64" ;;
        *)
            echo "unsupported headless architecture: $(uname -m)" >&2
            return 1
            ;;
    esac
    archive="$snapshot_dir/target/headless/rstorrent-headless-${version}-linux-${architecture}.tar.gz"
    if [ ! -f "$archive" ]; then
        echo "headless package was not created at $archive" >&2
        return 1
    fi

    install_dir="$(mktemp -d "$state_dir/install.XXXXXX")" || return $?
    tar -xzf "$archive" -C "$install_dir" || return $?
    log "installing the exact package for $sha"
    "$install_dir/install.sh" || return $?
    "$HOME/.local/bin/rstorrent-headless" status || return $?
}

run_worker() {
    cd "$project_dir"
    acquire_lock || exit 0
    trap cleanup_worker EXIT
    trap 'stop_on_signal 130 SIGINT' INT
    trap 'stop_on_signal 143 SIGTERM' TERM

    if [ -r "$HOME/.profile" ]; then
        # shellcheck disable=SC1091
        source "$HOME/.profile"
    fi

    local last_target=""
    local remote_deadline=$((SECONDS + max_wait_seconds))
    log "deploy-after-push worker started as pid $$"

    while true; do
        if ! read_desired; then
            log "no pending deploy; worker exiting"
            write_summary idle "" "" "" "no pending deploy"
            exit 0
        fi

        local sha="$desired_sha"
        local remote="$desired_remote"
        local branch="$desired_branch"
        local target="$remote/$branch@$sha"
        if [ "$target" != "$last_target" ]; then
            last_target="$target"
            remote_deadline=$((SECONDS + max_wait_seconds))
            log "waiting for $target"
            write_summary waiting_remote "$sha" "$remote" "$branch" \
                "waiting for the remote branch to reach the pushed commit"
        fi

        local completed_sha=""
        if [ -f "$completed_file" ]; then
            IFS=$'\t' read -r completed_sha _ < "$completed_file" || true
        fi
        if [ "$completed_sha" = "$sha" ]; then
            log "$sha is already deployed; clearing the duplicate request"
            if desired_still_matches "$sha" "$remote" "$branch"; then
                rm -f -- "$desired_file"
            fi
            continue
        fi

        local current_sha=""
        current_sha="$(remote_head_sha "$remote" "$branch" 2>/dev/null || true)"
        if [ "$current_sha" != "$sha" ]; then
            if ((SECONDS > remote_deadline)); then
                local timeout_detail="timed out waiting for $target"
                log "$timeout_detail"
                record_failure "$sha" "$remote" "$branch" remote-timeout \
                    "$timeout_detail" "$(elapsed_since_desired)"
                exit 1
            fi
            log "remote $remote/$branch is ${current_sha:-unreadable}; waiting for $sha"
            sleep "$poll_seconds"
            continue
        fi

        log "remote reached $sha; settling for ${settle_seconds}s"
        write_summary waiting_settle "$sha" "$remote" "$branch" \
            "remote reached the pushed commit; settling quick successive pushes"
        wait_for_settle_window "$sha" "$remote" "$branch" || continue

        current_sha="$(remote_head_sha "$remote" "$branch" 2>/dev/null || true)"
        if [ "$current_sha" != "$sha" ]; then
            log "remote moved to ${current_sha:-unreadable}; returning to confirmation"
            continue
        fi
        if ! desired_still_matches "$sha" "$remote" "$branch"; then
            log "a newer push replaced $sha before snapshot preparation"
            continue
        fi

        local deploy_started deploy_seconds total_seconds
        deploy_started="$(epoch_seconds)"
        log "exporting exact commit $sha into a temporary source snapshot"
        write_summary preparing_snapshot "$sha" "$remote" "$branch" \
            "exporting the accepted commit"
        if prepare_snapshot "$sha" && build_and_install_snapshot "$sha"; then
            deploy_seconds="$(( $(epoch_seconds) - deploy_started ))"
            total_seconds="$(elapsed_since_desired)"
            log "local headless deploy succeeded for $sha in ${deploy_seconds}s"
            write_tsv "$completed_file" \
                "$sha" "$remote" "$branch" "$(timestamp)" \
                "$total_seconds" "$deploy_seconds"
            write_summary succeeded "$sha" "$remote" "$branch" \
                "package installed and service healthy" \
                "$total_seconds" "$deploy_seconds"
            if desired_still_matches "$sha" "$remote" "$branch"; then
                rm -f -- "$desired_file"
            fi
            clear_temporary_directories
        else
            local status=$?
            deploy_seconds="$(( $(epoch_seconds) - deploy_started ))"
            total_seconds="$(elapsed_since_desired)"
            log "local headless deploy failed for $sha with status $status"
            record_failure "$sha" "$remote" "$branch" deploy-failed \
                "snapshot build or installation exited with $status" \
                "$total_seconds" "$deploy_seconds"
            exit "$status"
        fi
    done
}

validate_remote() {
    local remote="$1"
    if [ "${#remote}" -gt 128 ]; then
        return 1
    fi
    case "$remote" in
        ''|-*|*[!A-Za-z0-9._/-]*) return 1 ;;
    esac
    git -C "$project_dir" remote get-url "$remote" >/dev/null 2>&1
}

validate_branch() {
    local branch="$1"
    [ "$branch" = "$default_branch" ] &&
        git check-ref-format --branch "$branch" >/dev/null 2>&1
}

validate_sha() {
    local sha="$1"
    case "$sha" in
        *[!0-9a-f]*) return 1 ;;
    esac
    if [ "${#sha}" -ne 40 ] && [ "${#sha}" -ne 64 ]; then
        return 1
    fi
    git -C "$project_dir" cat-file -e "$sha^{commit}" 2>/dev/null
}

rotate_log_before_worker() {
    [ -f "$log_file" ] || return 0
    local size
    size="$(wc -c < "$log_file")"
    if [ "$size" -gt "$max_log_bytes" ]; then
        tail -c "$retain_log_bytes" "$log_file" > "$previous_log"
        : > "$log_file"
    fi
}

schedule() {
    local remote="$default_remote"
    local branch="$default_branch"
    local sha=""
    while (($#)); do
        case "$1" in
            --remote) remote="$2"; shift 2 ;;
            --branch) branch="$2"; shift 2 ;;
            --sha) sha="$2"; shift 2 ;;
            *)
                echo "unknown --schedule argument: $1" >&2
                usage >&2
                exit 2
                ;;
        esac
    done

    if ! validate_remote "$remote"; then
        echo "refusing unconfigured or invalid deploy remote: $remote" >&2
        return 2
    fi
    if ! validate_branch "$branch"; then
        echo "refusing deploy branch outside the configured target: $branch" >&2
        return 2
    fi
    if ! validate_sha "$sha"; then
        echo "refusing invalid deploy commit: $sha" >&2
        return 2
    fi

    mkdir -p "$state_dir"
    write_tsv "$desired_file" \
        "$sha" "$remote" "$branch" "$(timestamp)" "$(epoch_seconds)"
    write_summary scheduled "$sha" "$remote" "$branch" \
        "detached worker wake-up requested"
    rotate_log_before_worker
    append_log "scheduled deploy after $remote/$branch reaches $sha"

    nohup setsid "$script_path" --worker >> "$log_file" 2>&1 </dev/null &
    append_log "worker wake-up requested as pid $!"
    echo "pre-push: scheduled local headless deploy for ${sha:0:12}." >&2
}

show_status() {
    echo "state dir: $state_dir"
    for entry in \
        "summary:$summary_file" \
        "pending:$desired_file" \
        "latest completed:$completed_file" \
        "latest failed:$failed_file"; do
        local label="${entry%%:*}"
        local path="${entry#*:}"
        if [ -f "$path" ]; then
            echo "$label: $(sed -n '1p' "$path")"
        else
            echo "$label: none"
        fi
    done
    if [ -f "$lock_dir/pid" ]; then
        echo "worker pid: $(sed -n '1p' "$lock_dir/pid")"
    else
        echo "worker pid: none"
    fi
    echo "log: $log_file"
}

show_log() {
    if [ -f "$log_file" ]; then
        tail -n 200 "$log_file"
    else
        echo "deploy log does not exist: $log_file" >&2
        return 1
    fi
}

stop_worker() {
    if [ ! -f "$lock_dir/pid" ]; then
        echo "no deploy worker is active"
        return 0
    fi
    local pid=""
    read -r pid < "$lock_dir/pid" || true
    if ! worker_is_active "$pid"; then
        echo "no live deploy worker owns the recorded lock"
        return 0
    fi
    kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid"
    echo "requested stop for deploy worker process group $pid"
}

validate_configuration
case "${1:-}" in
    --schedule) shift; schedule "$@" ;;
    --worker) run_worker ;;
    --status) show_status ;;
    --log) show_log ;;
    --stop) stop_worker ;;
    -h|--help|"") usage ;;
    *) usage >&2; exit 2 ;;
esac
