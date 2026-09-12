# Locked dependency license sources

These unmodified license and notice texts supplement published Rust crates
that omit their upstream repository's license files. `cargo-sources.json`
records the exact package, published manifest hash, registry VCS revision,
original URL and text checksum. Texts are deduplicated by SHA-256. They are
reproduced solely to preserve the upstream distribution terms and attribution;
their own stated licenses continue to apply. No implementation source is
imported.

Some upstreams publish a manifest grant without a standalone license text.
Those exact reviewed packages are explicitly marked `declaration_only`;
the bundle identifies this provenance and includes the applicable standard
text from pinned cargo-about 0.9.2. It does not fabricate a copyright owner or
assert that a standard text is an original upstream notice. New omissions,
changed manifest declarations or changed supplemental files fail generation.

The source list is a distribution input, not legal advice or a license grant
from RSTorrent. Native platform libraries and Android Maven/AAR dependencies
require their own package inventories; Rust/npm notices do not cover them.


`android-sources.json` adds reviewed Android native supplements and original
artifact hashes. JNA 5.17.0 uses the existing Apache-2.0 selection; its exact
upstream revision `695ae749e7bfd92f88324147c5d96b7129efec3e` supplies the standard
Apache text and bundled libffi's original MIT notice. Graphics-path's pinned
pre-release source history at `794e3806700833665f48f56f7dd3581642a6057f` supplies
only the original Apache-2.0 comment blocks from `Conic.cpp` and its math
headers. Preserve their 2006/2013/2017/2022 Android Open Source Project
attribution. No implementation code is imported. The catalog records exact
source URLs, extraction scope and hashes; these texts are reproduced under
their stated terms to preserve attribution, not to grant new rights.

The Android generator separately records original Maven artifact notices,
POM/parent grants and exact source locators. Rustls's artifact-only Maven
component reuses the existing checksum-bound Cargo license supplement. Missing
or changed native AAR evidence requires review. Final APK/AAB verification
covers embedded text integrity and the exact native-library name inventory;
it does not replace corresponding-source delivery or legal review.
