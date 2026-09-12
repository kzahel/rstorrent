# Tactical 215: Windows Incubation Reset Recovery

Status: **Complete locally and hosted (2026-09-12); repaired signed-package
qualification remains open.** Discovered during Tactical 158 installed
signed 0.1.1-to-0.1.3 acceptance on native Windows x86_64.

Hosted run `34684328524` passes 43 desktop, 345 session and the native address
test on Windows, then builds/installs/inspects the unsigned NSIS package.
Tactical 211 repairs a test-only recheck/seed-admission wait exposed by the
first hosted run. The [exact evidence](../evidence/release-readiness-ci-2026-09-12.md)
supersedes pending-hosted statements below; it does not close the separate
signed older-to-newer update requirement.

Topics: `client-persistence`, `beta-release-readiness`, `capability-readiness`,
`oracle-driven-engine-campaign`

## Scope, Invariants And Stopping Condition

Repair the platform-specific catalog reset directory barrier. Public signed
0.1.3 replaces the older executable but exits before opening its window:
`sync profile directory: Access is denied. (os error 5)`. Current source uses
the same unconditional `File::open(directory).sync_all()` operation.

Keep the Tactical 179 fixed three-file reset scope, flushed checksum marker,
exclusive SQLite inspection, current committed report and hostile/future/busy
rejection. Never touch selected payload roots or treat old have state as
verified. Reset is synchronous before runtime startup; there are no new tasks,
API contracts, schema versions or dependencies. Unix, including Android,
retains its directory barrier. Windows follows the existing remote-authority
store's file-flush/atomic-replacement policy: no portable directory fsync is
claimed. Process-interruption recovery is required; arbitrary power-loss
durability is not inferred from process tests.

Stop after a native Windows negative reproduction and passing reset/recovery
suite, proportional common/Android validation, an unattended Windows gate,
and reconciled installed evidence. A repaired unsigned build cannot establish
signed-release qualification; publication remains a separate explicit action.

## Evidence And Reference Basis

Read Tactical 179, `client-persistence`, `profile_reset.rs`, the existing
Windows policy in `rstorrent-remote-access/src/store.rs`, and desktop updater
configuration persistence. Microsoft CreateFileW requires backup semantics
for directory handles; FlushFileBuffers documents writable file/volume
handles, not a portable unprivileged directory-fsync primitive:

- <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew>
- <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers>

This is an RSTorrent-private reset format and Windows filesystem adapter
repair, with no BEP or interoperable resume transition to adopt. Tactical 179
owns the compatibility dossier. Pinned libtorrent 2.0.13 file implementation
and file tests are inspected for platform opening semantics, not copied.
JSTorrent's old-state migration is outside the accepted disposable boundary.

## Required Cases

Fresh/current reopen; recognized old reset with payload sentinel; interrupted
marker beside absent, old and committed catalogs; malformed, future, busy and
unsafe files remain protected. Native Windows must actually execute these
tests, since Linux-only workspace tests missed the failure. Retain exact
installed signature, version, startup failure and recovery distinctions.

The bounded native session-suite expansion finds four tests deleting their
temporary profile while the shut-down ApplicationService still owns its
SQLite store. Explicitly drop the application before fixture deletion.
Two listener handover tests also used the public default port 6881; the
installed signed application remained listening on Windows wildcard 0.0.0.0
at that port after the test's more-specific loopback listener retired.
A five-second refusal diagnostic disproved the initial retirement-delay
hypothesis. Keep a test-owned preferred-port blocker alive and let automatic
selection choose a fresh OS port. Retain immediate refused-connection assertions
and the existing joined handover, with no production sleeps or policy changes.
Run the complete native suite alongside the installed default-port application
as a regression for this interference.

## Execution And Validation

Pinned libtorrent `7d7fc38fac61177fa5e02148f791b2f65250b09d`:
`src/file.cpp` `create_file`/`file_flags` separates native file access;
`test/test_file.cpp` `create_directory`, `file_status`, and `directory` cover
creation, status and enumeration, not private SQLite reset or portable
Windows directory flush. Adopt platform-specific access, not its architecture
or source. No broader durability claim follows from these reference tests.

The real signed startup failure is the negative control. Cross-build the
current session and desktop library tests with Rust 1.97.0 and cargo-xwin
0.23.1, then execute their x86_64 MSVC PE binaries in the native Windows 11
appliance, with the signed app still serving wildcard port 6881:

- Session: **345 passed, 2 ignored**, 79.90 seconds; all reset, interruption,
  hostile catalog and listener handover cases pass. SHA-256
  `b1c937de1146b7e682f3af98326c5bbd44ae57f75324a7ba71b4182262b43495`.
- Desktop: **43 passed**, including native Windows sleep-inhibition and
  lifecycle cases. SHA-256
  `3debc05b368a72af05f6606379e6230f4ca7ad427dd3fbfc423bc43e6a03b721`.
- macOS session: **346 passed, 2 ignored**. Workspace fmt and clippy with
  warnings denied pass. Workspace tests passed earlier in this campaign;
  the final fixture changes repeat the affected complete session suite.
- `clients/android/build.sh`: both Rust ABIs, generated Kotlin, JVM tests
  and debug APK pass. No generated application contract changed.

Cross-compilation still emits existing Windows conditional-code warnings;
this is native runtime evidence, not a claim of Windows clippy cleanliness.
The desktop CI matrix now runs `cargo test --locked -p rstorrent-session --lib`
on every native platform, in one command so PowerShell cannot hide an earlier
failure. Exact hosted execution requires a later authorized push.

[Installed evidence](../evidence/desktop-v0.1.1-to-v0.1.3-x86_64.md) records
Linux public update success and Windows signed reset failure separately.
A future repaired signed candidate must repeat automatic reset/relaunch and
pre-update payload preservation before the Windows update gate can close.
