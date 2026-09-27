# Web And Desktop Data Reset

Status: Implementation complete (2026-09-27); browser and installed desktop
acceptance remain open.

Topics: `web-ui-design`, `client-surfaces`, `client-persistence`.

## Scope and stopping condition

Add a Data & reset category to the shared Settings dialog. Restore default
preferences resets appearance, add prompts, global engine settings, the
optional desktop update channel, and native notification and power preferences.
It preserves torrents, roots, payload,
remote and web access, and privacy choices. Clear app data removes torrents,
root registrations, product preferences and access state. Downloaded files
remain unless the user separately opts into exact registered-file deletion.

The two actions require distinct confirmations. Clear never recursively deletes
a selected root or unrelated file. Report partial failure as failure and retain
enough state to retry. Neither action runs in demo or on a remote client.

The slice stops when local browser and desktop owners complete these outcomes,
the relevant native and web checks pass, and Settings reports success only
after the underlying operation is complete.

## Ownership

The React dialog owns confirmation and progress only. The application service
owns torrent and root removal. Each host owns its access, privacy, and profile
lifecycle. Browser storage owns presentation preferences. No protocol or peer
runtime module depends on UI or host code.

## Validation

Use isolated temporary profiles for destructive cases. Verify keep-files and
exact-delete outcomes, failed removals, preference preservation, and restart.

The session clear command refuses retained torrents, checks the profile's
fixed database files, joins application shutdown, removes only session and
metrics databases, and reopens a fresh profile with no registered roots. Its
offline placeholder blocks other mutations if clearing or reopening fails,
so the command can be retried. Existing exact per-torrent removal jobs own
payload deletion; the browser waits for each to complete before clearing the
profile. The desktop shell, product identity, remote access, web access, and
browser preferences are cleared by their respective owners. The UI keeps the
confirmation open on failure and marks success only after all calls finish.

Focused session profile, symlink, product identity, gateway access, browser
session, desktop shell, React confirmation, and semantic command tests pass.
Session, gateway, and desktop cargo check, strict clippy, and full crate tests
pass (455 tests). Rust format, web typecheck, and localization pass. The
isolated staged tree passes the full
web suite (393 passed, 2 skipped); one unrelated external-add assertion failed
on the first run and passed on rerun. The shared working tree's full web run
still has stale Transfers/Workbench expectations from Tactical 225. No
physical browser or installed desktop clear run was performed. A process
crash between the composed clear steps can leave partial progress; the user
can reopen Settings and retry, but there is no cross-owner durable phase
record.
