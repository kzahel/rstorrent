# Tactical 169: Hosted Crostini Bootstrap And Release

Status: **Accepted and Now as of 2026-08-26.** Explicit maintainer direction
temporarily yields signed-desktop Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) to this bounded ChromeOS
Linux distribution slice. Tactical `158` resumes as the sole **Now** after the
source plumbing and local/physical failure matrix pass.

Topics: `product-surfaces-and-migration`, `beta-release-readiness`,
`client-surfaces`

Dependencies: the complete local Crostini package and per-user installer from
Tactical [`167`](167-chromeos-crostini-bundled-web-launcher.md), the existing
RSTorrent incubation updater trust root, GitHub Releases, the static
`rstorrent.com` website, and native x86_64/ARM64 Linux release runners.

## Desired Outcome

Provide the source-controlled and reviewable equivalent of 200 OK's
ChromeOS Linux bootstrap:

```sh
curl -fsSL https://rstorrent.com/install-crostini.sh | bash
```

The HTTPS bootstrap detects x86_64 or ARM64, downloads an immutable signed
release manifest and the matching architecture package, verifies the manifest
against the pinned RSTorrent public key, verifies the package's signed size and
SHA-256, validates its versioned bundle shape, and invokes the existing
no-sudo per-user installer.

This tactical makes that flow release-ready but does not itself authorize a
tag, push, GitHub Release, website deployment, or public support claim.

## Scope And Stopping Condition

This tactical owns:

1. `website/public/install-crostini.sh` with strict Linux/architecture/tool
   admission, HTTPS-only bounded downloads, the pinned RSTorrent Minisign
   public key, canonical manifest verification, selected-asset size/hash
   verification, downgrade refusal, safe temporary extraction, version
   identity checks, and the existing bundle installer handoff;
2. a strict `rstorrent-crostini-release-v1` manifest containing release,
   source, repository, runtime, launch-protocol, extension-ID, two-architecture
   asset, size/hash, and metadata-asset identities;
3. deterministic manifest-writer and shell integrity tests covering both
   Minisign encodings, tamper rejection, schema/key-order drift, architecture,
   version, identity, size/hash, downgrade, and archive-shape failures;
4. a separate `crostini-v<version>` workflow that builds the production web
   bundle and GNU/Linux package natively on Ubuntu 22.04 x86_64 and ARM64,
   signs the canonical manifest with the existing RSTorrent updater key,
   validates all artifacts, and creates a non-latest GitHub Release only for an
   exact tag;
5. a release runbook and Crostini changelog that keep source version, tag,
   assets, website pin, validation, and physical acceptance explicit; and
6. a physical x86_64 Chromebook fixture campaign in which the exact bootstrap
   script consumes locally served signed metadata/package bytes, installs or
   repairs the real package, launches it, and fails closed for tampered or
   incompatible input without changing the existing installation.

The slice stops when shell/Node tests, source gates, two byte-identical x86_64
packages, and the physical bootstrap install/failure/cleanup matrix pass. ARM64
support remains release-CI evidence until the native ARM64 job runs; the first
public release remains unclaimed until a separately authorized tag workflow
and physical exact-public-artifact acceptance pass.

## Trust And Ownership Invariants

- The bootstrap's embedded public key must decode to the exact RSTorrent beta
  updater key already pinned by the desktop product. Crostini is another
  package of the same incubation product, not a new trust root.
- The canonical manifest is the only release authority. GitHub JSON, redirect
  targets, HTTP headers, filenames, and the website script cannot override a
  signed repository, tag, version, protocol, extension ID, runtime, size, or
  hash.
- All production downloads require HTTPS for both initial and redirected URLs,
  finite timeouts, and explicit byte limits.
- The default website bootstrap is pinned to one reviewed `crostini-v*` tag.
  Selecting “latest” through the repository's unrelated desktop release is
  forbidden. Advancing the pin is a reviewed website change after release
  acceptance.
- No package bytes execute before the manifest signature and selected package
  size/hash pass. The extracted launcher version and bundle `VERSION` must
  equal the signed version before installation.
- Archive extraction occurs only below a fresh temporary directory after
  signed-byte verification. Absolute paths, parent traversal, backslashes,
  links, devices, and an unexpected top-level bundle shape are rejected.
- The downloaded bundle delegates to the existing installer. It does not
  duplicate ownership policy or gain `sudo`; installation remains below the
  user's XDG/home paths with a static, disabled user service.
- A valid older signed release cannot replace a newer installed version. A
  same-version repair is allowed. Explicit rollback is not introduced by this
  slice because Tactical `167` retains only one installed version.
- Installer tests may substitute local fixture downloads only through a
  source-only test mode that is unreachable in ordinary execution.

## Reference Check

The sibling `web-server-chrome` checkout was inspected at committed revision
`66a8c0ee95494f5b8632f7a2424a36e2da7495dd`; unrelated legacy-migration edits
were present and left untouched. Exact references were:

- `website/public/install-crostini.sh` for pinned-key Minisign verification,
  strict manifest parsing, HTTPS/size bounds, architecture selection, and
  version/downgrade handling;
- `scripts/test-crostini-installer.sh` for legacy and prehashed Minisign
  fixtures plus schema/tamper/version tests;
- `.github/scripts/write-crostini-release-manifest.mjs` and its tests for
  deterministic asset metadata;
- `.github/workflows/crostini-ci.yml` for separate tag family, native artifacts,
  signed manifest, exact release set, and non-latest GitHub Release; and
- `docs/topics/chromeos-crostini-launcher.md` for the one-command and
  download-inspect-run user flows plus exact public-artifact acceptance.

RSTorrent adapts these maintainer-owned MIT release techniques to its bundled
backend/frontend archive. It does not adopt 200 OK's update service, controller
pairing, rollback store, static single-binary runtime, extension protocol, or
server product policy.

This is distribution and platform integration, not BitTorrent protocol or
engine work, so the pinned libtorrent oracle pass is inapplicable.

## Validation

The source baseline is:

```bash
bash -n website/public/install-crostini.sh
bash scripts/test-crostini-installer.sh
node --test .github/scripts/write-crostini-release-manifest.test.mjs
npm run check --prefix website
npm run build --prefix website
cargo fmt --all -- --check
cargo clippy -p rstorrent-crostini -p rstorrent-gateway -- -D warnings
cargo test -p rstorrent-crostini -p rstorrent-gateway
npm run typecheck --prefix clients/web
npm run test --prefix clients/web
```

The physical fixture must record the exact bootstrap/manifest/package hashes,
the installed binary hashes, service/health/UI identity, pre-existing profile
preservation, tampered-manifest/signature/package and wrong-architecture
failures before installation mutation, and cleanup. It may use a loopback
fixture transport in source-only test mode; that is not public-release
evidence.

## Non-Goals

- Creating, pushing, or publishing the first `crostini-v0.1.0` tag/release;
  deploying the website; changing DNS/update services; or exposing the command
  in the extension before the exact public release passes.
- A Crostini in-app updater, automatic update polling, rollback UI/store,
  service enablement, linger, login start, or background update daemon.
- Supporting distributions older than the chosen GNU/Linux baseline,
  ChromeOS Flex claims, native ARM Chromebook claims, or Android integration.
- Signing the archive itself with a second mechanism, adding another key,
  treating GitHub `SHA256SUMS` as the trust root, or downloading executable
  release metadata from an unsigned “latest” endpoint.
- Changing the backend, UI ownership, extension control, profile format,
  download roots, service lifecycle, or existing uninstall/purge behavior.

## Escalation Contract

Ordinary shell/Node implementation, manifest fields, workflow composition,
native runner selection, release documentation, fixture transport, and
physical non-public testing are in scope. Stop for direction before changing
the trust root, adding an update service or rollback model, broadening supported
runtime baselines, enabling a service, modifying public extension guidance, or
tagging, pushing, publishing, or deploying anything.
