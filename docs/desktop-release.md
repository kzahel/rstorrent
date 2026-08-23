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

## Update Service

The application checks
`https://updates.graehlarts.com/rstorrent/tauri/<target>/<arch>/<version>`.
[`update-server/rstorrent.json`](../update-server/rstorrent.json) is the
product-owned configuration consumed by the shared update service. A current
version should return HTTP 204; an older version should return signed Tauri
metadata referencing the immutable public GitHub Release.

After publishing, verify at least one exact current-version key and all five
older-version keys. Do not treat metadata checks as installed-update evidence:
the beta gate also requires an older installed signed build to download,
install, relaunch, and report the new version/build on each supported target.
