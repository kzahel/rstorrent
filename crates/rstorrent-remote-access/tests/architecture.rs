#![forbid(unsafe_code)]

#[test]
fn authority_domain_excludes_runtime_and_product_layers() {
    let manifest = include_str!("../Cargo.toml");
    for forbidden in [
        "tokio",
        "axum",
        "tungstenite",
        "rusqlite",
        "rstorrent-gateway",
        "rstorrent-session",
        "rstorrent-engine",
        "rstorrent-platform",
        "wasm-bindgen",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "remote authority domain must not depend on {forbidden}"
        );
    }
}
