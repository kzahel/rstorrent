# Local Headless Deploy Hook

This optional machine-local automation keeps `git push` independent from the
production web/Rust/package build and ordinary-user headless service repair.
Install it in this checkout with:

```bash
scripts/local-deploy/install-hook.sh
```

The tracked `pre-push` hook schedules only updates to `main` and returns. A
detached single worker waits until the configured remote reports the exact
pushed commit, allows a short settle window for quick follow-up pushes, and
then exports that commit with `git archive` into a temporary source directory.
It does not create a sibling worktree or build from mutable working-tree files.

The worker installs exact web dependencies, builds the two release binaries
using the checkout's retained Cargo target cache, constructs and validates the
ordinary headless package, and invokes its existing health-checked installer.
Source and package-extraction directories are removed on every worker exit.

Inspect or stop the worker with:

```bash
scripts/local-deploy/deploy-after-main-push.sh --status
scripts/local-deploy/deploy-after-main-push.sh --log
scripts/local-deploy/deploy-after-main-push.sh --stop
```

State and logs live under `.git/rstorrent-deploy-after-main-push/`. Scheduling
or deployment failure never rejects the push. `summary.tsv`, `completed.tsv`,
and `failed.tsv` retain the latest result; `deploy.log` contains build and
installer output.

Defaults may be adjusted in the hook environment:

- `RSTORRENT_DEPLOY_AFTER_PUSH_REMOTE` (`origin`)
- `RSTORRENT_DEPLOY_AFTER_PUSH_BRANCH` (`main`)
- `RSTORRENT_DEPLOY_AFTER_PUSH_POLL_SECONDS` (`5`)
- `RSTORRENT_DEPLOY_AFTER_PUSH_SETTLE_SECONDS` (`5`)
- `RSTORRENT_DEPLOY_AFTER_PUSH_MAX_WAIT_SECONDS` (`900`)
- `RSTORRENT_DEPLOY_AFTER_PUSH_MAX_LOG_BYTES` (`8388608`)
- `RSTORRENT_DEPLOY_AFTER_PUSH_RETAIN_LOG_BYTES` (`4194304`)

This is local development automation, not a release channel or unattended
updater. It never enables the service, changes its configuration, or changes
firewall, router, DNS, Tailscale, or public release state.
