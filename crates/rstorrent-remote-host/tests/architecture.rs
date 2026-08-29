#![forbid(unsafe_code)]

#[test]
fn runtime_owner_does_not_become_a_relay_or_second_application_contract() {
    let manifest = include_str!("../Cargo.toml");
    for forbidden in ["axum", "opaque-ke", "chacha20poly1305"] {
        assert!(
            !manifest.contains(forbidden),
            "host runtime must reuse its owned boundary instead of depending directly on {forbidden}"
        );
    }
    let source = concat!(
        include_str!("../src/owner.rs"),
        include_str!("../src/runtime.rs"),
    );
    assert!(!source.contains("struct ApplicationClientFrame"));
    assert!(!source.contains("struct ApplicationServerFrame"));
}
