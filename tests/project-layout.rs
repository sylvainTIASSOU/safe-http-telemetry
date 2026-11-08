#[test]
fn package_identity_is_stable() {
    assert_eq!(env!("CARGO_PKG_NAME"), "safe-http-telemetry");
    assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0-dev.0");
}
