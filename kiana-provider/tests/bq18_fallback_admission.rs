#[test]
fn provider_fallback_boundary_is_validation_only_and_route_bound() {
    let fallback = include_str!("../src/fallback.rs");
    let gateway = include_str!("../src/lib.rs");
    let transport = include_str!("../src/transport.rs");
    for marker in [
        "FallbackAttemptAdmission",
        "validate_fallback_attempt",
        "fallback_route_admission_drift",
        "fallback_credential_revision_drift",
        "fallback_permit_admission_drift",
    ] {
        assert!(
            fallback.contains(marker) || gateway.contains(marker),
            "BQ-18 provider marker missing: {marker}"
        );
    }
    for forbidden in [
        "choose_fallback",
        "fallback_loop",
        "tokio::spawn(fallback",
        "send_fallback",
        "fallback.unwrap()",
        "raw_credential",
    ] {
        assert!(
            !fallback.contains(forbidden) && !transport.contains(forbidden),
            "provider fallback boundary widened: {forbidden}"
        );
    }
    assert!(gateway.contains("ProviderGateway"));
    assert!(transport.contains("send_inner_attempt"));
}
