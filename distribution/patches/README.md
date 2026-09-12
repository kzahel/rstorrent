# Proposed GLib backport (not applied)

`glib-0.18.5-variant-iterator.patch` is a review artifact, not an active Cargo
patch. It independently corrects the C variadic out-pointer's mutability in
the exact published glib 0.18.5 source, following RustSec RUSTSEC-2024-0429 and
<https://github.com/gtk-rs/gtk-rs-core/pull/1343>. GLib's Rust bindings use the
MIT license; the published crate retains its original license and attribution.
No dependency implementation has been imported into the product tree.

A disposable native Linux x86_64 probe used Rust 1.97.0, system GLib 2.80.0,
and optimized builds of the original and patched 0.18.5 crate. The original
terminated with SIGSEGV (-11); the patched version passed forward collection,
`nth`, `last`, `next_back` and `nth_back` over a three-string variant. This
confirms the defect and bounded candidate repair, not the complete product.

Adoption requires an explicit source-maintenance decision: vendor the one
existing crate with its original license and a pinned patch, add the optimized
regression to native Linux CI, validate the full Linux product/picker/tray
suite, and make the advisory review verify the exact patched source before
clearing its blocker. Do not silently treat unchanged 0.18.5 metadata as fixed
or replace the GTK3 graph with an incompatible glib >=0.20 dependency.
