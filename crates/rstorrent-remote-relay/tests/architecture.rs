#![forbid(unsafe_code)]

#[test]
fn relay_manifest_has_no_crypto_or_application_dependency() {
    let manifest = include_str!("../Cargo.toml");
    for forbidden in [
        "rstorrent-remote-crypto",
        "opaque-ke",
        "argon2",
        "chacha20poly1305",
        "rstorrent-gateway",
        "rstorrent-session",
        "rstorrent-engine",
        "serde_json",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "dumb relay must not depend on {forbidden}"
        );
    }
}
