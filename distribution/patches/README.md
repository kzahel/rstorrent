# Maintained GLib 0.18.5 backport

The user approved this narrow source backport on 2026-09-12. The workspace
Cargo override selects `vendor/glib-0.18.5`, copied from the exact published
crate. Its version, API, original MIT license and all other source bytes are
preserved. Only `src/variant_iter.rs` changes: make the C out-pointer mutable
and pass `&mut p`, following RUSTSEC-2024-0429 and
<https://github.com/gtk-rs/gtk-rs-core/pull/1343>.

`glib-0.18.5-source.json` records the archive checksum, source revision,
complete original file inventory, patched file checksum and patch checksum.
`scripts/glib_backport.py` rejects drift, extra/missing files, symlinks and an
incorrect Cargo override/lock. Git preserves upstream bytes on every host.
Independent public-API regressions live in the desktop integration tests and
run with release optimization on both native Linux CI architectures.

Cargo-audit omits path packages. `scripts/audit-cargo-dependencies.py` feeds
an in-memory audit-only lock projection with the original registry identity,
so future GLib advisories remain visible. The real Cargo.lock never changes.
Review binds that projection to the current lock and independently verified
backport; other warnings and review expiry remain enforced.
Linux package notices include the original license, exact source provenance
and patch text; package inspection rejects missing backport attribution.

The original optimized Linux x86_64 probe (Rust 1.97.0 / system GLib 2.80.0)
terminated with SIGSEGV (-11); the two-line candidate passed forward
collection, nth, last, next_back and nth_back. Tactical 218 records adoption
and full qualification, including remaining gates. Source verification alone
is not complete product qualification.

Remove this maintained override once a supported upstream dependency graph
includes the repair and the same optimized and package gates pass. Do not
rename the crate version to impersonate an upstream release, suppress the
advisory without exact source evidence, or substitute incompatible glib
>=0.20 into the current GTK3 graph.
