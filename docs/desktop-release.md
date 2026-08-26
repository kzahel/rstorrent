# Desktop Release Runbook

RSTorrent desktop releases use the `desktop-v<version>` tag family and the
[`Desktop Release`](../.github/workflows/desktop-release.yml) workflow. The
workflow builds signed updater packages and ordinary installers for macOS
arm64/x86_64, Windows x86_64, and Linux x86_64/arm64.

## Rehearse Without Publishing

Run the protected workflow from `main`:

```bash
gh workflow run desktop-release.yml --ref main
gh run list --workflow desktop-release.yml --limit 1
gh run watch RUN_ID --exit-status
```

This uses the real updater, Developer ID/App Store Connect, and Azure signing
credentials. It uploads five private Actions artifacts retained for 14 days;
it does not create a tag or GitHub Release. Check the job assertions for both
macOS notarization/stapling and Windows Authenticode validation before using a
rehearsal package.

The latest proven rehearsal is GitHub Actions run
[`32627436936`](https://github.com/kzahel/rstorrent/actions/runs/32627436936)
at commit `f34961c1cbd34508e2f62edc68d1c2a321d78767`. Its source gate and all five
release legs passed on 2026-08-23. It retained separate private artifacts for
macOS arm64/x86_64, Linux arm64/x86_64, and Windows x86_64. Both macOS legs
passed Developer ID, Gatekeeper, notarization/stapling, and updater-artifact
checks; both Windows installers passed expected-publisher Authenticode checks;
both Linux legs passed the AppImage/DEB/RPM matrix.

## Cut A Release

1. Update the same stable version in `clients/web/package.json`,
   `clients/desktop/src-tauri/Cargo.toml`, and
   `clients/desktop/src-tauri/tauri.conf.json`; refresh both lockfiles.
2. Replace the matching `CHANGELOG.md` `Unreleased` heading with the release
   date and include supported behavior, known limitations, persistence or
   migration changes, and security/privacy changes.
3. Validate the source and push the release commit:

   ```bash
   node scripts/validate-desktop-release.mjs --tag desktop-vX.Y.Z
   cargo fmt --all -- --check
   cargo clippy --workspace -- -D warnings
   cargo test --workspace
   npm run typecheck --prefix clients/web
   npm run test --prefix clients/web
   npm run build --prefix clients/web
   git push origin main
   ```

4. After ordinary CI passes, create and push the exact tag:

   ```bash
   git tag -a desktop-vX.Y.Z -m "RSTorrent desktop X.Y.Z"
   git push origin desktop-vX.Y.Z
   ```

The tagged workflow creates a draft while its five serialized build legs add
assets to one `latest.json`. The finalizer refuses incomplete or unsigned
assets, validates same-release URLs and all five updater keys, adds
`SHA256SUMS`, and only then publishes the release. A failed build remains a
draft. The GitHub release is non-prerelease because the shared update server
intentionally ignores prerelease entries; the `0.x` version and release notes
carry the incubation-beta status.

The first published release is
[`desktop-v0.1.0`](https://github.com/kzahel/rstorrent/releases/tag/desktop-v0.1.0).
Tagged workflow
[`32656926123`](https://github.com/kzahel/rstorrent/actions/runs/32656926123)
passed every source/build/finalizer job. Its independent checksum, route, and
installed macOS smoke evidence is recorded in
[`desktop-v0.1.0`](evidence/desktop-v0.1.0.md).

The first updater-validation release is
[`desktop-v0.1.1`](https://github.com/kzahel/rstorrent/releases/tag/desktop-v0.1.1).
Tagged workflow
[`32661616090`](https://github.com/kzahel/rstorrent/actions/runs/32661616090)
passed its source gate, five signed target jobs, and finalizer. Independent
checksums, route probes, and the exact installed macOS arm64
`0.1.0`-to-`0.1.1` update are recorded in
[`desktop-v0.1.0-to-v0.1.1`](evidence/desktop-v0.1.0-to-v0.1.1.md).

The current repair-bearing release is
[`desktop-v0.1.2`](https://github.com/kzahel/rstorrent/releases/tag/desktop-v0.1.2).
Tagged workflow
[`32959820514`](https://github.com/kzahel/rstorrent/actions/runs/32959820514)
passed its source gate, five signed package jobs, and publication finalizer at
exact commit `788e953d1ed578c238beccbbc224907b0d9dc95c`. Its complete package
matrix and bounded exact-public-DMG macOS arm64 launch/native-host spot check
are recorded in [`desktop-v0.1.2`](evidence/desktop-v0.1.2.md). This release
record does not claim the still-open installed `0.1.1`-to-`0.1.2` update
campaign.

## Update Service

The application checks
`https://updates.graehlarts.com/rstorrent/tauri/<target>/<arch>/<version>`.
[`update-server/rstorrent.json`](../update-server/rstorrent.json) is the
product-owned configuration consumed by the shared update service. The
production `/rstorrent` route and product registration were deployed and the
service health check passed on 2026-08-23. Public `desktop-v0.1.1` now resolves:
a current `0.1.1` version returns HTTP 204 and installed `0.1.0` returns signed
Tauri metadata referencing that immutable GitHub Release. Both results passed
for all five default updater targets after publication. The installed macOS
arm64 client then completed the real replacement/relaunch and reported the
new exact version/build; the other four installed targets remain open.

## Windows First-Launch Consent

The first launch of an unsigned fresh-profile Windows package from `main`
displayed Windows Security Allow/Cancel consent for the incoming listener.
Choosing Cancel granted no broader firewall access and left the application
and native download-folder picker usable; it did not prove incoming
reachability.

For each signed Windows release candidate, record whether this prompt appears
on a clean profile and describe the supported private/public-network choice in
the release evidence and user-facing known limitations. Test automation must
not select Allow or create a firewall rule implicitly. Keep firewall consent
distinct from application startup, root-picker, and updater success.

After publishing, verify at least one exact current-version key and all five
older-version keys. Do not treat metadata checks as installed-update evidence:
the beta gate also requires an older installed signed build to download,
install, relaunch, and report the new version/build on each supported target.
