#![forbid(unsafe_code)]

#[test]
fn pure_core_manifest_excludes_runtime_and_product_layers() {
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
            "pure crypto core must not depend on {forbidden}"
        );
    }
}
