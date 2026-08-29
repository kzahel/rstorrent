#![forbid(unsafe_code)]

#[test]
fn wasm_binding_depends_only_on_the_pure_core_and_wasm_bindgen() {
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
        "web-sys",
        "js-sys",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "narrow Wasm binding must not depend on {forbidden}"
        );
    }
}
