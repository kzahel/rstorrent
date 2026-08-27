# Tactical 178: Crostini Storage Guidance

Status: **Complete (2026-08-27).** Explicit user direction temporarily yielded
durable High file-priority Tactical `176` to this bounded ChromeOS Linux
usability slice. Its stopping condition and proportional source gates pass;
Tactical `176` resumes as the sole **Now**.

Topics: `download-roots`, `client-surfaces`, `web-ui-design`,
`product-surfaces-and-migration`

Dependencies: completed Crostini package Tactical
[`167`](167-chromeos-crostini-bundled-web-launcher.md), its exact
`rstorrent-crostini` health identity, and the existing shared React download-
root and folder-picker flows.

## Desired Outcome

Make the Crostini product explain its two storage choices at the moment a user
chooses or manages a download folder:

- Linux `~/Downloads` is the recommended, faster location and is already
  visible in the ChromeOS Files app under **Linux files > Downloads**;
- a folder under ChromeOS **My files** must first receive the ChromeOS
  **Share with Linux** grant and is then selectable below
  `/mnt/chromeos/MyFiles`, but can be materially slower for torrent writes,
  checking, reading, and seeding.

The slice stops when exact Crostini backend identity gates concise guidance in
both Add and Downloads Settings, the expanded help gives the complete sharing
and picker steps, known Linux-Downloads and ChromeOS-shared paths receive
truthful performance labels, and deterministic web tests and build gates pass.

## Evidence And Product Decision

An authorized physical Chromebook campaign on 2026-08-27 compared five
alternating 128 MiB trials in Crostini-local `~/Downloads` (Btrfs) and the
user-shared `/mnt/chromeos/MyFiles/Downloads` (9P). Checksums matched in every
pair and all temporary data was removed. Median results included:

| Workload | Linux Downloads | ChromeOS Downloads | ChromeOS penalty |
| --- | ---: | ---: | ---: |
| Sequential read | 2,064 MiB/s | 42.4 MiB/s | 48.6x slower |
| Durable sequential write | 127.9 MiB/s | 65.7 MiB/s | 1.95x slower |
| Durable 16 KiB scattered writes | 148.3 MiB/s | 64.7 MiB/s | 2.29x slower |
| Four concurrent durable writers | 150.1 MiB/s | 27.3 MiB/s | 5.5x slower |
| 128-file durable publication | 13.1 MiB/s | 12.8 MiB/s | approximately equal |

Before **Share with Linux**, the ChromeOS Downloads mount existed and was
readable but not writable from `penguin`. After the normal Files-app grant it
was writable. The product therefore keeps Linux Downloads as the default and
offers ChromeOS folders only through explicit user selection; mere path
existence is not usable-root evidence.

Absolute throughput is device-specific. Product copy uses only the durable
directional conclusion (Linux is faster; the ChromeOS sharing boundary can be
much slower), not benchmark numbers or a universal multiplier.

## Scope And Invariants

1. The browser bootstrap recognizes Crostini only from the exact health
   product and launch-protocol identity. Hostname, user agent, path spelling,
   and generic Linux detection are not backend authority.
2. The detected presentation fact flows into the existing React App, Add
   dialog, and Downloads Settings without entering the portable application
   command/view contract or becoming durable profile state.
3. Collapsed guidance recommends Linux Downloads and explains that ChromeOS
   Files already exposes it. Expanded guidance gives the exact Files-app
   **Share with Linux**, RSTorrent **Choose folder**, and native-picker steps
   without requiring a typed Linux path or keyboard shortcut.
4. A known `/home/<user>/Downloads` root may be labelled faster/recommended;
   a known `/mnt/chromeos` root may be labelled shared/slower. Other paths are
   not classified from weak heuristics.
5. The UI does not grant access, mutate ChromeOS sharing, probe arbitrary
   paths, expose filesystem authority to JavaScript, or auto-select a root.
6. Non-Crostini browser, headless, Tauri, demo, Android, and iOS presentation
   remains unchanged.

There are no new owners, tasks, queues, cancellation paths, resource bounds,
or generated-contract values. One bounded health request already used for
host integration supplies the presentation-only environment fact.

## Non-Goals

- changing the established Crostini default root;
- automatically sharing, mounting, copying, moving, or relocating content;
- measuring or displaying per-device storage performance at runtime;
- adding a browser filesystem API or path-string command;
- changing Android SAF, iOS bookmarks, desktop picker semantics, or headless
  configured-root policy; and
- publishing a Crostini package or changing the public bootstrap pin.

## Validation

The proportional source gates are:

```bash
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
npm run build --prefix clients/web
```

Focused tests must prove exact Crostini health admission and malformed
identity rejection; Crostini-only Add and Settings guidance; both performance
labels; complete sharing instructions; and absence on ordinary hosted and
Tauri/demo surfaces. A physical UI run is proportional confirmation rather
than a stopping-condition requirement because this slice presents already
observed platform behavior and does not change filesystem or package
mechanics.

## Completion Evidence

`clients/web/src/headless-updater.ts` now admits the exact four-field Crostini
health identity and rejects wrong protocol or shape. That presentation-only
fact flows through the hosted bootstrap to App without changing the generated
application contract. Non-Crostini hosted products retain their prior updater
and access-mode behavior.

The shared `CrostiniStorageHelp` component appears in both Add and Downloads
Settings. Its collapsed state recommends Linux Downloads and names its
automatic ChromeOS Files location. Its expanded state provides the exact
Files-app context action, RSTorrent chooser action, and instruction to select
the folder just shared. It deliberately avoids exposing `Ctrl+L` or a typed
`/mnt/chromeos` implementation path. Known local and shared root paths receive
the accepted faster/recommended and convenient/slower labels only inside this
exact product. The disclosure is native keyboard-accessible HTML and avoids a
nested modal inside the existing Add dialog.

Validation passed:

- TypeScript typecheck;
- 61 focused hosted-integration and App component tests;
- the full web suite with 295 passing and two intentionally skipped tests;
- the production Vite build; and
- the bundled-browser CSP check across all ten JavaScript bundles.

The controller uses Node 25.2, whose experimental process-global Web Storage
conflicts with jsdom's `localStorage`. Test invocations therefore set
`NODE_OPTIONS=--no-experimental-webstorage`; without that controller flag,
every pre-existing App test fails during its `localStorage` cleanup rather
than at a product assertion. Typecheck and production build used their normal
commands.

An additional physical x86_64 Chromebook run used the registered ChromeOS
Launcher and installed public gateway. Its exact `/healthz` identity admitted
the guidance. Accessibility and display captures proved the collapsed and
expanded Downloads Settings states, the faster/recommended label on the
installed `/home/<user>/Downloads` root, and the same guidance and label in
the Add dialog. Cancelling a synthetic magnet left the queue at zero. The
ChromeOS Files context menu independently exposed the documented **Share with
Linux** action on **My files > Downloads**.

The controller-built native package requires GLIBC 2.39, while this Debian 12
Crostini installation does not provide it. The physical UI run therefore
temporarily substituted only the validated production `web/` directory over
the unchanged public binaries rather than claiming a package-install pass.
Afterward the original public web tree was restored to aggregate SHA-256
`4539fef998145d9ae50f71659ab84497cd059aadc7b4313f3948a9712b07f774`;
the launcher and gateway retained their original hashes, the service was
inactive, the ChromeOS Downloads share remained absent, and the VM was
stopped normally. No package, release, website pin, generated contract,
filesystem grant, or durable device state changed.
