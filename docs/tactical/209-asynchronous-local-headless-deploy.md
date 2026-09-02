# Tactical 209: Asynchronous Local Headless Deploy

Status: **Active.** Maintainer direction on 2026-09-02 authorizes the local
hook replacement, one commit, a push to `origin/main`, and monitoring through
an exact healthy redeploy.

Topic: `runtime-configurations-and-headless-deployment`

Dependencies: completed configured-service Tactical
[`170`](170-configured-linux-headless-service.md), completed signed/LAN
Tactical [`171`](171-signed-headless-release-and-lan-service.md), and the
current ordinary-user x86_64 installation.

## Motivation And Desired Outcome

The current machine-local `pre-push` hook synchronously builds the production
web application, two release Rust binaries, the deterministic headless
package, and then performs a same-version service repair. A cold build exceeded
YepAnywhere's 60-second Git-operation deadline. YepAnywhere terminated only the
top-level `git` process, leaving the hook and SSH descendants alive; a second
push then waited on the hook lock while the first detached build completed.

Replace that synchronous local hook with a tracked, explicitly installed
scheduler. A push to `main` records the intended commit and returns immediately.
One finite background worker confirms that the remote branch accepted that
exact commit, exports the commit into a temporary non-worktree snapshot,
builds and validates the ordinary package, runs the existing transactional
installer, and records exact completion or failure evidence.

## Scope And Stopping Condition

This slice includes:

- one tracked `pre-push` entry point and local installer;
- one Git-directory-owned desired-state, worker-lock, summary, completion,
  failure, and bounded-log area;
- exact remote-head confirmation before deployment;
- a short settle window and latest-request replacement for quick successive
  pushes;
- an exact `git archive` source snapshot that is removed after the worker;
- production web dependency hydration, release Rust builds using the retained
  local Cargo target cache, deterministic package construction, installation,
  and installed health verification; and
- status, log, and process-group stop commands for operator observability and
  cancellation.

The slice stops when a real `origin/main` push returns without waiting for the
build, GitHub reports the pushed commit, the worker records that same commit as
completed, no source/install snapshot remains, and the installed service is
active and healthy at the same package version.

## Contracts And Invariants

- The hook schedules only non-deletion updates to `refs/heads/main`. Tags and
  other branches do not deploy.
- Scheduling or deployment failure never rejects the Git push. The failure is
  visible in local state and logs.
- The worker deploys only when `git ls-remote` reports the exact desired SHA as
  the remote branch head. A rejected or failed push cannot deploy.
- Build inputs come from the desired commit's Git objects, never from mutable
  working-tree files. The active checkout may become dirty or advance while
  the worker runs.
- The snapshot and install extraction are fresh `mktemp` directories beneath
  the exact Git-local state directory and are removed on success, failure, or
  worker exit. No persistent sibling worktree or clone is created.
- One `mkdir` lock owns the worker. A scheduled wake-up that loses the lock
  exits; the owner rereads the latest desired record. Stale locks are removed
  only when their recorded process is absent.
- The detached worker has a new process group, closed stdin, file-backed
  stdout/stderr, an observable PID, finite remote wait, signal cleanup, and an
  explicit group-stop command.
- The existing package installer remains the sole owner of immutable-version
  replacement, systemd restart, health verification, and rollback. This
  automation does not enable the service or change operator configuration.
- State contains commit, remote, branch, timestamps, durations, and bounded
  error classifications. It contains no credentials, magnets, torrent names,
  profile content, or payload paths.
- The append log is pruned after each worker and rotated before a new wake-up;
  it retains at most one bounded current and previous log generation.

## Owner, Task, Cancellation, And Data Flow

```text
git pre-push
  -> validate main update
  -> atomically record desired SHA
  -> nohup + setsid worker wake-up
  -> return to git

single background worker
  -> poll exact remote branch head with a finite deadline
  -> settle/coalesce quick successive pushes
  -> mktemp source snapshot
       -> git archive exact accepted commit
       -> npm ci for exact web lockfile
       -> Cargo release build using retained target cache
       -> deterministic headless package validation
  -> mktemp package extraction
  -> existing installer and systemd health/rollback owner
  -> completed/failed/idle records
  -> remove both temporary directories and worker lock

operator stop
  -> TERM the recorded worker process group
  -> terminate build descendants
  -> EXIT cleanup and observable stopped state
```

Git object selection and state-file transitions remain plain shell operations.
The worker owns filesystem, process, network, compiler, package, and service
effects. The installed service retains its existing one-process application
and engine ownership.

## Resource Bounds

- one worker and one desired request per checkout;
- 40- or 64-character lowercase hexadecimal commit IDs;
- one configured Git remote name of at most 128 conservative characters;
- five-second default poll and settle intervals;
- 15-minute default remote-confirmation deadline;
- one temporary source tree and one package extraction directory;
- the existing 128-MiB compressed and 256-MiB expanded package bounds;
- an 8-MiB current deploy log, pruned to the latest 4 MiB after completion,
  plus at most one rotated predecessor; and
- the existing Cargo target directory as the only persistent build cache.

## Reference Dossier

The local Mclone automation was inspected as a behavioral reference:

- `~/code/mclone/.githooks/pre-push` and
  `scripts/local-deploy/pre-push-hook.sh` for lightweight ref routing;
- `scripts/local-deploy/deploy-after-main-push.sh` for remote-SHA confirmation,
  latest-request replacement, one-worker locking, and status records; and
- `scripts/local-deploy/README.md` for the no-client-side-post-push rationale.

RSTorrent independently keeps its own package/installer commands and replaces
Mclone's persistent sibling worktree with a temporary `git archive` snapshot.
No Mclone source or fixture is imported. BitTorrent specifications and the
pinned libtorrent oracle are inapplicable because no engine or protocol
behavior changes.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Static shell | `bash -n` for every hook/deploy script; `git diff --check` |
| Scheduler | Installed `core.hooksPath`, status output, conservative ref/remote/SHA admission, detached stdio/session and single-worker lock review |
| Package | Exact pushed snapshot runs `npm ci`, locked Cargo release build, package builder, and package validator |
| Git | Real `origin/main` push returns before deployment; local, tracking, and remote SHA agree |
| Installed host | Completion record names the pushed SHA; no temporary snapshot remains; installed CLI reports enabled, active, healthy version `0.1.1` |

## Non-Goals And Escalation Boundary

This slice does not add a public release, tag, signed-channel promotion,
unattended updater, system-wide service, new installation/configuration mode,
firewall/router/Tailscale change, payload or profile mutation, GitHub Actions
workflow, cross-platform hook claim, or YepAnywhere process-tree fix.

The authorized local hook install, exact `origin/main` push, ordinary-user
same-version package repair, service restart/health check, and bounded local
state cleanup proceed without another routine approval. Stop for any required
public release, external-device action, system-wide ownership, network-policy
change, destructive operator-data action, or incompatible package contract.

## Implementation And Evidence

The tracked `.githooks/pre-push` delegates to a focused ref router, and the
local installer selects it through checkout-local `core.hooksPath=.githooks`
without deleting the previous `.git/hooks/pre-push` file. The scheduler and
worker live under `scripts/local-deploy/`; DEVELOPMENT and the owning runtime
topic describe the optional workflow and its non-release boundary.

Before the real push, all four shell entry points passed `bash -n` and the
repository passed `git diff --check`. Invalid remote, branch, and commit inputs
failed without creating a worker. A non-target-branch hook input produced no
request. A controlled request for an older commit created a worker whose PID,
PGID, and SID matched, recorded `waiting_remote`, and inherited no Git pipe
descriptors. `--stop` terminated that process group, changed the summary to
`stopped`, removed its lock, and left no source or install snapshot.

The exact `origin/main` push, background package build, completion record, and
installed healthy-service evidence remain pending.
