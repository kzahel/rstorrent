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

## Package Dependencies And Release Artifacts

Rust, npm, Gradle, and Python dependencies remain under their respective
licenses. The manifests and lockfiles identify the resolved dependency sets;
upstream packages and artifacts contain their applicable license texts.

Notable distribution considerations in the current dependency graph include:

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
