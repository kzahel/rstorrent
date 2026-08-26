# ChromeOS Linux Release Runbook

RSTorrent's ChromeOS Linux package uses the `crostini-v<version>` tag family
and the [`Crostini Release`](../.github/workflows/crostini-release.yml)
workflow. It is separate from the desktop updater channel and GitHub's latest
release selection. The website bootstrap is pinned to one reviewed tag.

There is no public Crostini release yet. Until an exact tagged artifact passes
the post-release checks below, the source-controlled installer is release
plumbing rather than a support or availability claim.

## Trust And Artifacts

The workflow natively builds one GNU/Linux archive on Ubuntu 22.04 x86_64 and
one on Ubuntu 22.04 ARM64. It generates a canonical manifest containing the
source commit, launch protocol, extension identity, and exact size and SHA-256
of both packages. The existing RSTorrent beta updater key signs that manifest.

A complete release has exactly these five assets:

- `rstorrent-crostini-X.Y.Z-x86_64.tar.gz`
- `rstorrent-crostini-X.Y.Z-aarch64.tar.gz`
- `rstorrent-crostini-release.manifest`
- `rstorrent-crostini-release.manifest.minisig`
- `SHA256SUMS`

`SHA256SUMS` is useful independent transport evidence, but the bootstrap's
trust root is the embedded public key and signed manifest. The workflow keeps
an incomplete upload as a draft and publishes the release as non-latest only
after checking the remote asset set.

## Rehearse Without Publishing

After the workflow source exists on GitHub, run it manually from `main`:

```bash
gh workflow run crostini-release.yml --ref main
gh run list --workflow crostini-release.yml --limit 1
gh run watch RUN_ID --exit-status
```

The manual run does not read the production signing secret: the source gate
and two native build jobs run, and their packages remain as private Actions
artifacts for 14 days. No tag or GitHub Release is created.

## Cut A Release

1. Set the same stable three-part version in
   `crates/rstorrent-crostini/Cargo.toml`, its `Cargo.lock` entry, the matching
   changelog heading, and `PINNED_TAG` in
   `website/public/install-crostini.sh`. A pin may advance only to the release
   being reviewed; it must never select GitHub's latest release implicitly.
2. Run the source baseline from Tactical 169, including the bootstrap fixture,
   manifest tests, package validation, shared web tests, and website build.
3. Commit the release source and let ordinary CI pass. Do not advertise the
   website command yet.
4. Create and push the exact annotated tag only with explicit maintainer
   authorization:

   ```bash
   git tag -a crostini-vX.Y.Z -m "RSTorrent ChromeOS Linux X.Y.Z"
   git push origin crostini-vX.Y.Z
   ```

5. Watch the tagged workflow. It must pass its source gate, both native package
   jobs, production-key signature verification, exact release-set check, and
   final publication step. Failure before finalization must leave no public
   partial release.
6. Independently download all five immutable release assets. Check every
   `SHA256SUMS` entry, verify the manifest with the public key, compare the
   signed commit/tag/version/protocol/extension/runtime fields, and re-run the
   package validator on both archives.
7. On a physical Chromebook of the matching architecture, run the exact
   website bootstrap, open the ChromeOS Launcher entry, verify health and the
   backend-served UI identity, stop/relaunch once, and confirm existing profile
   data is preserved. Record exact public hashes and cleanup in a dated
   evidence document.
8. Only after that acceptance may product or extension copy present
   `curl -fsSL https://rstorrent.com/install-crostini.sh | bash` as a supported
   installation path.

The website deployment and tag are separate external mutations. A source
commit, manual rehearsal, or successful native build does not authorize either
one.
