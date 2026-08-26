# Tactical 169: Hosted Crostini Bootstrap And Release

Status: **Complete as of 2026-08-26.** The signed bootstrap source, strict
manifest, native two-architecture workflow, release runbook, deterministic
failure corpus, and physical x86_64 package fixture passed the bounded source
slice. Subsequent explicit release authorization published
`crostini-v0.1.0`, deployed the website bootstrap, independently verified the
public artifacts, and passed the exact website install/Launcher/relaunch path
on the physical x86_64 Chromebook. Signed-desktop Tactical
[`158`](158-desktop-signed-packaging-and-updater.md) has resumed as the sole
**Now**.

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

## Completion Evidence

Implementation commit `1b676f7` adds the website bootstrap, canonical manifest
writer, ordinary-CI checks, separate native release workflow, changelog,
runbook, deterministic integrity corpus, and real-package loopback harness.
The bootstrap embeds the exact updater public key and the manifest writer
independently derives the pinned extension ID from the checked-in extension
key. No release secret is used outside a tagged workflow's final release job.

The following source checks pass:

- Bash syntax for all three bootstrap/fixture scripts and the complete Linux
  integrity script on Debian 12.12;
- three Node manifest/trust-identity tests;
- website check/build plus a byte comparison proving the public script is
  emitted unchanged at `/install-crostini.sh`;
- `actionlint` 1.7.9 for ordinary CI and the new release workflow;
- Rust formatting, warning-denied clippy, and 46 Crostini/gateway unit tests;
  and
- shared-web typecheck plus 279 passing and two skipped unit tests.

The physical campaign used ChromeOS `16700.60.0` M150, x86_64 Debian 12.12
`penguin`, OpenSSL 3.0.18, and the exact non-public package from Tactical 167.
Relevant SHA-256 values were:

- bootstrap: `188064c7c983d44230785639d3e2d0c1d8963a507b709059b101af876785bed0`;
- manifest writer: `b8b130101ce41cc925e571b17ac0357c79bc8bbddff7b03381ff6b23348f2956`;
- canonical local fixture manifest:
  `0464a9869641ad4a6a2ba89410c2f1e2c4ea4453c836462f2aa3dc6ab4043eb8`;
- x86_64 package:
  `8db3c4cfae0fccac014e8e68538013c7420d850089cf44ff8ff7a489fa95fd88`;
- installed launcher:
  `8a9d0b62b589bcd89ca34ebe58bdcdfc5792efbe9b648c38415107449a386861`;
  and
- installed gateway:
  `42d9f72709368274e7103156430debe470bb6d32e531c4383f728f52ba5fa61a`.

Two deterministic repacks of the exact extracted bundle matched each other
and the original package byte-for-byte at the package hash above. The locally
served signed fixture then repaired the real installation. Before and after
that repair, `metrics.db`, `session.db`, and `web-auth.sqlite3` retained exact
hashes `10ab53f2…`, `9c0d34a1…`, and `3fe7d314…`; the installed binaries also
matched the values above.

The final physical failure matrix ran against the real installed home. A
tampered manifest, tampered signature bytes, oversized/tampered package,
signed incompatible launch protocol, and signed wrong-architecture asset all
failed before installation mutation; the harness compared every owned
version, link, command, desktop file, icon, service file, and ownership record
before and after each case. An early isolated-positive fixture revealed that
the live user service manager may retain an XDG-selected unit path outside the
real home. The harness consequently refuses positive installation into a
synthetic home; the real-home repair restored coherent service ownership, and
a subsequent ChromeOS Launcher cold start passed.

Final device evidence was one active static service and one UI target with
`RSTorrent` accessibility identity. `/healthz` reported product
`rstorrent-crostini`, build `0.1.0`, and launch protocol `1`; the CLI reported
the same identity. Cleanup closed the tab, stopped the service, removed the
exact fixture tree, retained the installed package/profile, and stopped the
Crostini VM. The unit remained `static` and `inactive`. At source-tactical
close, ARM64 build/runtime evidence, the production-key tag workflow, public
artifacts, and the website deployment were deliberately unclaimed. A later
explicitly authorized release operation supplied the evidence below.

## Post-Completion Public Release Acceptance

Annotated tag `crostini-v0.1.0` resolves to source commit
`4abf165f07a94d86a88f443bd9f879c2079d227c`. GitHub Actions run
[`32986250710`](https://github.com/kzahel/rstorrent/actions/runs/32986250710)
passed the complete source gate, native Ubuntu 22.04 x86_64 and ARM64 package
jobs, production-key manifest signing, strict package/signature/asset-set
verification, fail-closed draft creation, and final non-latest publication.
The resulting public release is
[`crostini-v0.1.0`](https://github.com/kzahel/rstorrent/releases/tag/crostini-v0.1.0).

Independent public download and verification recorded these SHA-256 values:

- website bootstrap:
  `188064c7c983d44230785639d3e2d0c1d8963a507b709059b101af876785bed0`;
- x86_64 package:
  `1d0ec34e55e7fc58742cb59ae8e40100e3b8a429f4d908440a1e26ecc8189979`;
- ARM64 package:
  `67a3922170b970e7b11ef7a4a628a546922b0a486f15e42311f0988df4843919`;
- manifest:
  `881881456a4653a9d3df7fb09b41941d73a689db367dd4eb7ec79374f886bf44`;
- manifest signature:
  `1a3e1469caac6b349c0e4d10d1efa2c906cdf3f0da16df75c4a8975f3110ba07`;
  and
- `SHA256SUMS`:
  `f6a573a3ac8e162a2343a5f9ef8dd7dd13b6195df2b958669e097ee2e643e07d`.

Every checksum row and the production-key manifest signature passed, the
signed manifest named the exact tag/source/protocol/extension/runtime, and
both public archives passed the package validator independently in Debian
12.12. The ARM64 package therefore has native hosted build and archive
evidence, but no physical ARM64 runtime claim.

On the physical x86_64 Chromebook, the exact public command
`curl -fsSL https://rstorrent.com/install-crostini.sh | bash` verified and
installed the public package as the ordinary Crostini user. The same-version
repair preserved `metrics.db`, `session.db`, and `web-auth.sqlite3`
byte-for-byte. Installed launcher and gateway hashes were
`24788ce9280609485b19963eb5d10d5b3b80e8b006342346f138fe3f04a12d10`
and
`77289ce2834a4250917fd7754a63b4d12712f0529f04b3c0a60e473e74d5ed6c`.

The registered ChromeOS Launcher item produced exactly one static service,
one port-3030 listener, and one RSTorrent tab. Exact-authority `/healthz`
reported product `rstorrent-crostini`, build `0.1.0`, and launch protocol `1`;
the React accessibility surface reported `connected`. Closing the tab,
stopping the service, and launching again restored the same singleton and UI
identity. Full ChromeOS reboot, physical native ARM64 execution, updating,
rollback, suspend, and performance remain unclaimed.

## Non-Goals

- During the bounded source slice, creating, pushing, or publishing the first
  `crostini-v0.1.0` tag/release; deploying the website; changing DNS/update
  services; or exposing the command in the extension before exact public
  acceptance. The later authorized release operation is recorded above.
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
