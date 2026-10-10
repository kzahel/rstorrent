# AppImage source delivery preparation

Status: local preparation, not a published or complete source offer.
Maintainer decision: retain MIT; rebuild Linux packaging to remove GPL-only
runtime dependencies under Tactical294. Frozen assets below are historical
bindings and must be reconciled with that new package before shipping.
Owner: [beta-release-readiness](topics/beta-release-readiness.md).
Execution and remaining work: [Tactical293](tactical/293-appimage-source-delivery.md).
Linux remains best effort; iOS is excluded.

The reviewed [native recipe](../distribution/linux-native/README.md) now builds
custom TIFF and GTK3 appindicator libraries, retaining original security patches,
archives, copyrights and exact patch/configuration/compiler provenance. The new
pre-signing AppImage hook embeds those source materials and rejects the two
identified GPL-only runtime chains. Local x64/ARM ABI/codec/offscreen tray/package
checks and actual x64 product window/tray pass under294. Fresh signed source
`ff632f45`/run38050770001 now passes independent extracted custom-source/native
gates on both architectures under259. Remaining source delivery is tracked in293;
ARM product UI and outer
runtime relink remain separate.
This does not convert the historical frozen packet into a new public source offer.

The source binding is frozen desktop commit
`2b83ed9137fc3b779fabcb3f20b421e59035fec9`, version0.3.0. Android28 and
extension1.1.3 are separate candidates, not substitutions for this binding.

## Prepared materials

| Local asset | Contents | SHA-256 |
| --- | --- | --- |
| `JSTorrent_0.3.0_AppImage-source-review-materials.tar` | Exact first-party source,89 original Ubuntu source/version pairs, original notices and selected runtime source/recipes | `ad7c607bce9853db22766ed54331f7f29304458065d9614b4b48143e069bf9aa` |
| `JSTorrent_0.3.0_application-source-inputs.tar.gz` | Locked Cargo inputs, exact npm distributions, preferred web sources and provenance | `a0f771fb4c43263f5799fa202d31a5fe59de6157cfff50a98d85c2805a68a906` |

Both archives contain internal checksum inventories. Extract into a new owned
directory, enter each archive's top-level directory, and run:

```bash
sha256sum --check SHA256SUMS
```

The application supplement includes `first-party-2b83ed91.tar`. Extract that
into `source/` beside `cargo-vendor/`. Copy its supplied relative Cargo config
into `source/.cargo/config.toml`. Run `cargo metadata --locked --offline` with
the selected Linux target before building. Empty-Cargo-home metadata has been
checked for both targets; this is not a full offline compilation claim.

Use the frozen source's `DEVELOPMENT.md` and desktop release workflow for Rust,
Node, native dependencies, GLib patch, web build, sidecar and Tauri packaging
commands. A modified local build uses no production signing key. Preserve all
notices; do not distribute a modified binary under an original signature.

## Native rebuild and runtime

Each native source directory includes its original `.dsc`, upstream archive
and Debian changes. For example, in a new owned Ubuntu24.04 build environment:

```bash
dpkg-source -x ubuntu/libayatana-indicator_0.9.4-1build1/libayatana-indicator_0.9.4-1build1.dsc indicator-source
cd indicator-source
dpkg-checkbuilddeps
dpkg-buildpackage -b -us -uc
```

Install the descriptor's declared build dependencies in that environment first.
Its packaging and upstream sources own the actual build instructions. All89
package builds have not been executed. The `.dsc` checksums bind source files;
OpenPGP signatures are retained but not independently authenticated here.

The native packet's nested runtime archive supplies original source/recipes,
patches and notices. Its old x64 relink prototype uses zlib-r1. Separate061
now verifies original unpatched-r0 source building and a separate relink using
the explicit r0 static library, with successful version/help execution. Rebuilt
bytes differ from shipped bytes. ARM relink and mounted AppImage/FUSE execution
remain unrun; none of these receipts establishes binary reproducibility.

## Remaining shipment work

The frozen2b83 images bundle GPL-2+ JBIG through TIFF and GPL-3 Ayatana indicator through
the tray library. The maintainer has selected retaining MIT and removing these
GPL-only runtime dependencies under Tactical294, followed by new qualification.
Existing first-party MIT source terms remain unchanged. Remaining LGPL libraries
still require their notices and source/modification/relink materials.
The final per-package disposition and distribution-ready instructions are open.

The proposed delivery is versioned sources beside the AppImages, linked adjacent
to downloads on the website. Publication, real downloadable URLs and shipment
terms must be verified before that route can count as delivered. No release,
website, feed or store activation is authorized by this preparation.
