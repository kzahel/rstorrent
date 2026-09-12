# Third-Party Notices

RSTorrent is licensed under the MIT License. This file records bundled
third-party material and release-distribution considerations. It does not
replace the license information supplied by package managers or upstream
projects.

## Gradle Wrapper

The Android experiments include the official Gradle 8.11.1 wrapper scripts
and wrapper JAR, licensed under Apache License 2.0. The scripts retain their
upstream license headers, and each wrapper JAR contains its upstream license
in `META-INF/LICENSE`.

Source: <https://github.com/gradle/gradle>

## Public Test Torrent Metadata

`tests/live/torrents.json` records magnet links and factual metadata from the
[WebTorrent Free Torrents](https://webtorrent.io/free-torrents) catalog for
opt-in interoperability testing. The repository does not include the torrents'
media payloads. Each work remains subject to its own public-domain or Creative
Commons terms, which must be checked before redistributing a payload.

## MSE Primitive Evidence

`rstorrent-protocol` uses `crypto-bigint` 0.7.5 under its dual Apache-2.0 OR
MIT license for fixed-width, constant-time modular exponentiation over the
legacy MSE DH group. The crate's default features and RNG integration are
disabled.

Source: <https://crates.io/crates/crypto-bigint/0.7.5>

The independently authored RC4 tests transcribe selected output bytes from
IETF RFC 6229. Treat those bytes conservatively as RFC Code Components under
the Simplified BSD terms in the IETF Trust Legal Provisions. The test contains
an adjacent origin citation and does not copy explanatory RFC prose.

Source: <https://www.rfc-editor.org/rfc/rfc6229.html>

## Package Dependencies And Release Artifacts

Rust, npm, Gradle, and Python dependencies remain under their respective
licenses. The manifests and lockfiles identify the resolved dependency sets;
upstream packages and artifacts contain their applicable license texts.

Notable distribution considerations in the current dependency graph include:

- `dontfrag` 1.0.1 is used only on macOS under its dual MIT OR Apache-2.0
  license to access the IPv4 don't-fragment socket policy through a safe API.
- `keepawake` 0.6.1 is used only on macOS and Windows under its MIT license to
  hold and release the native user-idle/system execution assertion while
  download or verification work is active.
- The desktop default-enabled `direct-file-webrtc` feature uses the `rtc` 0.20.4
  crate family under its MIT OR Apache-2.0 terms and Ring 0.17.14 under Apache
  2.0 AND ISC. It is present in ordinary desktop release graphs; non-desktop feature
  defaults must be checked independently.
- `net.java.dev.jna:jna:5.17.0` is used under its Apache-2.0 option.
- Some Rust and npm packages are MPL-2.0 licensed.
- Linux desktop packages may use WebKitGTK and GTK system libraries under
  their respective licenses.

Before distributing an Android or desktop binary, generate a notice and
license bundle from that release's exact resolved dependency graph. Include
the Apache-2.0 text for JNA and other shipped Apache components, the license
texts for shipped MPL-2.0 components, and any notices required by bundled
platform libraries. A source checkout's root license and this notice are not
by themselves a complete binary dependency notice bundle.


Tactical `214` adds `scripts/prepare-release-notices.py` and the pinned
cargo-about 0.9.2 generator. Desktop package/release overlays include its
Rust/npm texts and lockfile-bound manifest in `notices/`; the package inspector
verifies the embedded text checksum. `distribution/licenses/` contains exact
upstream license documents with source revisions and checksums for crates
whose published archives omit them. Those texts retain their upstream terms.
CRC32C's additional Zlib-derived component attribution is included separately.
Reviewed manifest-only grants are explicitly distinguished from original
upstream notices; no copyright owner is invented.

The generator covers the target's default-feature Rust graph (including
build dependencies), web production dependencies and Ajv standalone code
generation. It does not claim to inventory Android Maven/AAR dependencies or
all native libraries bundled by AppImage tooling. Distribution review must
reconcile those platform components against the saved package inventories
before a supported release. See `distribution/dependency-review.json` for
currently unresolved advisory blockers and its expiring review date.
