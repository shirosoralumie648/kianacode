#[test]
fn provider_retry_after_and_typed_status_boundary_is_source_visible() {
    let transport = include_str!("../src/transport.rs");
    let config = include_str!("../src/config.rs");
    for marker in [
        "parse_retry_after(value: &str, now: SystemTime)",
        "ModelRetryClass::Rejected",
        "429 | 408",
        "ModelSideEffectState::None",
        "classify_connect_failure",
        "SystemTime::now()",
    ] {
        assert!(
            transport.contains(marker),
            "BQ-17 provider marker missing: {marker}"
        );
    }
    assert!(config.contains("retry(reqwest::retry::never())"));
    assert!(transport.contains("provider_http_503"));
    assert!(transport.contains("ModelRetryClass::Never"));
}

#[test]
fn provider_boundary_does_not_add_a_second_retry_loop_or_copy_error_bodies() {
    let transport = include_str!("../src/transport.rs");
    for forbidden in [
        "reqwest::retry::exponential",
        "tokio::spawn(send_inner",
        "response.text().await",
        "error_body",
    ] {
        assert!(
            !transport.contains(forbidden),
            "BQ-17 provider retry bypass marker present: {forbidden}"
        );
    }
}
