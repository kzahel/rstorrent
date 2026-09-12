# Tactical 214: Release Dependency And Artifact Review

Status: **Implementation complete locally and hosted (2026-09-12); dependency source
maintenance and native-library notice clearance remain open; native attribution
follow-through is qualified on both architectures.** Authorized
release-readiness campaign.

Run `34684328524` passes advisory review, all notice/content tool checks and
all four unsigned native package lanes at `8f31f98a48313c57c3e3ba16e5b8907b02d63135`.
Windows additionally passes silent NSIS installation, activation registry and
native-host inspection. Its lockfile digests exactly match CRLF checkout bytes;
the protected upstream license bytes retain their recorded hashes. The
advisory report remains release-ready false. See the
[exact hosted record](../evidence/release-readiness-ci-2026-09-12.md); this
supersedes pending-push statements in the local execution history below.

Topics: `beta-release-readiness`, `capability-readiness`

## Scope And Stopping Condition

Audit the exact Cargo and web lockfiles, repair compatible vulnerable/yanked
dependencies, record remaining upstream constraints, and make repeat audits
and distributable dependency notices part of release verification. Inspect
package contents and reject development files, private keys and unreviewed
resource additions before release. Stop with validated tooling, exact local
evidence and substantial commits. Public publication, dependency architecture
replacement, a new supported version and legal conclusions are not implied.

## Invariants And Ownership

The existing package build owns generated notice resources. Derive provenance
from locked package metadata and installed license files; never invent a
license from a package name or silently omit unresolved license evidence.
Retain original attribution and identify the exact source graph. Fail closed
on missing evidence. Generated output contains no machine paths or secrets.
No engine/runtime owner or application contract changes are needed.

Audit tools may fetch official advisory databases and registry metadata.
Record database revision and informational advisories separately from known
vulnerabilities. An upstream GTK constraint must remain visible; suppressing
a warning is not proof of safety. CI owns bounded jobs and seven-day evidence.

## Validation

Use current primary upstream advisories, Cargo reverse dependency trees and
npm audit. Review lockfile changes; run web type/unit/production build checks
and proportional Rust/Android checks for Rust dependency updates. Exercise
notice generation and negative artifact/metadata cases, release validators,
workflow lint and exact installed public-package contents. Preserve the
distinction between existing signed 0.1.3 fixtures and future source packages.


## Execution And Current Evidence

Compatible lockfile repairs:

- Vitest and matching packages `4.1.10` → `4.1.11`, resolving
  [GHSA-82fw-gwwq-j7x9](https://github.com/vitest-dev/vitest/security/advisories/GHSA-82fw-gwwq-j7x9).
- `fast-uri` `3.1.5` → `3.1.7`, including
  [the upstream parsing advisory](https://github.com/fastify/fast-uri/security/advisories/GHSA-5jgf-p345-68v8)
  and the other audit-listed fixes; `nanoid` `3.3.16` → `3.3.19` resolves
  [GHSA-2v37-7h3g-55p8](https://github.com/advisories/GHSA-2v37-7h3g-55p8).
- Yanked `chacha20` `0.10.1` → `0.10.2`, whose
  [upstream release](https://github.com/RustCrypto/stream-ciphers/releases/tag/chacha20-v0.10.2)
  repairs SSE4.1 instructions in an SSE2 backend.

`npm audit` now reports zero vulnerabilities. Cargo audit 0.22.2 against
RustSec revision `b50980aad8b8f14f77e25a97b32dd94bf008b0af` (September 9,
1,243 advisories) reports zero vulnerability entries and no yanked crate,
but retains six unmaintained warnings and **glib 0.18.5 unsoundness
RUSTSEC-2024-0429**. `distribution/dependency-review.json` preserves the exact
warning inventory, expires October 12, and explicitly reports release-ready
false. A changed warning set, vulnerability, stale database or expired review
fails unattended review. Tagged desktop source checks additionally require
release readiness; the known blocker cannot silently pass publication.

The Linux reverse graph is `glib 0.18.5` through GTK3/ATK/GDK, Tauri 2.11.5,
Wry, tray and picker dependencies. The fixed glib `>=0.20` line is not a
compatible substitution for that GTK3 graph. No application use of
`VariantStrIter`/`array_iter_str` was found, but complete transitive
unreachability is not established. The
[upstream fix](https://github.com/gtk-rs/gtk-rs-core/pull/1343) corrects the
mutable out-pointer. A dependency backport would create a maintained source
fork; this campaign prepares its bounded probe rather than silently adopting
that maintenance policy or suppressing the advisory.

### Reproducible Notices And Package Checks

Pinned cargo-about 0.9.2 resolves license expressions; the wrapper supplements
its output with every packaged root license/notice and exact upstream texts
missing from registry archives. Source revision, declared license, original
manifest checksum and imported text checksum fail on drift. Standard texts
for reviewed manifest-only grants are labeled as such. No implementation
source is imported. Include original CRC32C Zlib attribution and license.

All five desktop target generations pass: macOS ARM64 **459** Rust packages,
macOS x86_64 **461**, Windows x86_64 **469**, Linux x86_64 **529**, Linux ARM64
**527**, each with **28** web/Ajv packages. These are conservative build plus
runtime graphs, not claims that every crate contributes executable bytes.
Default desktop WebRTC is included; the root notice's stale default-off claim
is corrected. The generated public manifest has no build-machine paths.

An actual unsigned macOS ARM64 Tauri package build passes with notices and
manifest in `Contents/Resources/notices/`. Its inspector records six package
entries, 59,105,464 uncompressed bytes, and one matching notice bundle.
New package overlays allow only the reviewed notice resource directory;
macOS, installed NSIS and extracted Linux CI checks verify its presence,
content hash and bounded file inventory. Reject external symlinks, development
files, source maps, private-key text and Vite development URLs. Inventories
are retained seven days. This is not a comprehensive secret scanner.

The exact public `0.1.3` AppImage/NSIS hashes from the installed record were
independently rechecked before static extraction. Linux contains **276**
entries / **318,004,410** bytes and 17 distro copyright documents; NSIS
contains **8** extracted entries / **48,309,982** bytes, including the expected
WebView2 bootstrapper and native host. Neither has a Rust/npm notice bundle.
Both pass the bounded content-signature checks in explicit historical mode;
that mode cannot substitute for the new package gate. Native AppImage library
coverage and Android Maven/AAR notices remain separate required reviews, so
this slice does not claim universal binary license clearance.

### Validation

Workspace Rust fmt/clippy/tests and Android dual-ABI/generated Kotlin/JVM/APK
checks pass after the dependency updates. Web typecheck, all **381** active
unit tests (two skipped), production build and CSP checks pass. Nine focused
Python tests cover advisory expiry/drift/blockers, notice corruption, hostile
files, chunk-boundary key detection and escaping symlinks. Desktop release
validators and workflow actionlint pass. The real current macOS package and
all five notice generations pass. Exact hosted execution awaits authorized
push; no route or published artifact changed.


### Concrete GLib Decision And Cleanup

`distribution/patches/glib-0.18.5-variant-iterator.patch` is prepared but not
applied to the dependency graph. An optimized native Linux x86_64 probe on
Rust 1.97.0 / GLib 2.80.0 reproduces SIGSEGV (-11) in the original crate;
the two-mutability-change candidate passes forward collection, `nth`, `last`,
`next_back` and `nth_back`. The source-maintenance decision and complete Linux
product validation remain prerequisites to clearing the blocker. All probe
source, build outputs and extracted public packages lived in the owned
scratch directory, which was removed after successful validation. No owned
guest, probe process or extracted package remains.

### Native AppImage Attribution Follow-through (Qualified)

The corrected hosted inventories expose a concrete gap: x86_64 contains 169
regular shared-library files but only 10 distro copyright documents; ARM64
contains 168 and 108 respectively. Both also bundle `xdg-mime` without its
package copyright. Library counts include multiple copied names for the same
binary and are not counts of unique upstream projects.

This bounded follow-through maps every selected distro ELF and the copied
`xdg-mime` helper to installed package/source versions and original copyright
texts. Include referenced distro common-license texts. Record bundled and
original hashes, use retained GNU build IDs to identify patchelf-modified
libraries, reject absent/ambiguous mappings, and independently verify the
manifest against the final extracted artifact. Build IDs identify the trusted
builder's package provenance; they are not a cryptographic proof of unchanged
machine code. First-party binaries retain their existing Rust/npm coverage.
AppImage launcher/runtime provenance remains explicit separate review scope;
this work does not assert universal license or source-offer clearance.

Use linuxdeploy's output-plugin API to add notices to the fully selected
AppDir before AppImage construction and Tauri updater signing. Keep tools in
the Cargo target directory, pin the delegated output-plugin download by hash,
and leave library selection and runtime behavior unchanged. The hook owns
bounded synchronous subprocesses and temporary downloads, terminates them on
failure, and does not change global Tauri caches. Missing notices fail builds.
No engine/application dependency, vendored GLib, or generated API changes.

Source inspection: npm pins Tauri CLI `2.11.4`, upstream commit
`8909f221d1515955fc843808032bdc5d62209c96`. Its
`crates/tauri-bundler/src/bundle/linux/appimage/linuxdeploy.rs` owns tool
selection and AppDir assembly; `crates/tauri-cli/src/interface/mod.rs` maps
`useLocalToolsDir` to Cargo's target directory, and
`crates/tauri-cli/src/bundle.rs` signs only after bundling returns. The public
linuxdeploy plugin API version 0 gives output plugins the prepared `--appdir`.
The delegated output plugin is revision
`536b068787179ea901964bd7dabc7bf61e4941c3`; `src/main.cpp` passes that directory
and `OUTPUT` to appimagetool. Reference implementation source is inspected,
not imported. Preserve original distro license files byte-for-byte.

Validation/stopping condition: deterministic missing/ambiguous provenance,
modified/omitted/extra binary, notice corruption, symlink and bounds cases;
workflow/package-validator checks; actual unsigned native x86_64 and ARM64
AppImages with matching embedded manifests. Record exact source and CI
artifacts, reconcile the owning topic, and commit the substantial slice.

Local follow-through evidence: **16** distribution-review tests and **12**
desktop release-validator tests pass; the checked-in configuration validator,
actionlint on both changed workflows, and diff whitespace checks pass. No
Rust/application source or generated boundary changed, so those suites were
not repeated for this build-tool slice.

A native Ubuntu 24.04 x86_64 fixture preserves six selected components,
identifies patchelf-modified GLib/GTK/OpenSSL plus copied xdg-mime, and embeds
14 original/common license files for four distro packages. Running Tauri's
actual x86_64 linuxdeploy mirror with the pinned output-plugin delegate then
expands the fixture's native dependencies, embeds notices for **53** distro
packages, and successfully constructs an unsigned AppImage. This is a
packaging fixture, not product launch or signed-release evidence. The exact
full-product x86_64/ARM64 hosted build remains the next gate.

Independent extraction of that fixture passes **60** selected components,
**53** distro packages and **65** notice files. AppImage SHA-256:
`bddae80321fdb8baefc5732839f37d7dc75f29b032f896861f97f1d94ac72c1d`;
manifest SHA-256:
`1b9f66c6ac1f73808a0210d12330f4c7337b31e4930a6d519bd9ad9f2c37b736`.
The inspected linuxdeploy mirror SHA-256 is
`e762bea85c8eb0d4b3508d46e5c1f037f717d0f9303ae3b4aafc8b04991fa1ef`.


Hosted follow-through run
[`34687692788`](https://github.com/kzahel/rstorrent/actions/runs/34687692788)
passes all **12** jobs at `a83de080d27adb80932a03c972db96d026c91520`.
The actual x86_64 AppImage contains 391 entries / 327,392,358 bytes and verifies
174 selected components, 113 distro packages and 128 original/common notice
files. Its native manifest SHA-256 is
`578199fcc5fcecbf89ae7c0387ba89646d3d491bfd1736478a6c55d960ffa165`.
ARM64 contains 487 entries / 340,990,858 bytes and verifies 173 components,
112 packages and 127 notice files; its manifest is
`ca3bbe76fdeb0ca8926b487f278adf770541202f6b74484ea3e41d18af8a3e6a`.
Final artifact inspection passed independently after AppImage extraction.
The owned Linux fixture and its output-plugin extraction cache were removed.
Launcher/runtime and corresponding-source review remain explicit; Android
Maven/AAR follow-through is complete in Tactical 217, with byte-identical
local/hosted notice manifests and all 12 jobs passing in run `34689662485`.
The GLib patch remains unapplied pending the source-maintenance decision.
