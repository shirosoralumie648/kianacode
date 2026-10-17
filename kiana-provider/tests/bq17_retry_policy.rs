//! BQ-17 provider boundary source fixtures: Retry-After parsing stays injectable, only 429/408
//! map to a typed retryable rejection, and no implicit or second retry loop exists.

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
        "provider_http_503",
        "ModelRetryClass::Never",
    ] {
        assert!(
            transport.contains(marker),
            "BQ-17 provider marker missing: {marker}"
        );
    }
    assert!(config.contains("retry(reqwest::retry::never())"));
    // The only typed retryable HTTP statuses are 429/408; every other non-2xx is `Never`.
    assert!(transport.contains("if matches!(status.as_u16(), 429 | 408) {"));
    assert!(transport.contains("&format!(\"provider_http_{}\", status.as_u16())"));
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

#[test]
fn provider_transport_retry_fixtures_stay_injectable_and_not_ignored() {
    let transport = include_str!("../src/transport.rs");
    for name in [
        "retry_after_http_date_uses_injected_clock",
        "retry_after_large_delta_is_preserved_for_absolute_deadline_check",
        "only_typed_network_connect_failures_are_retryable",
        "tls_failure_is_not_transient",
    ] {
        assert!(
            transport.contains(name),
            "BQ-17 provider transport fixture missing: {name}"
        );
    }
    // Retry-After stays a pure function over an injected clock; tests never sleep on a real
    // wall clock and no fixture is ignored.
    assert!(transport.contains("let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);"));
    assert!(transport.contains("parse_retry_after(\"18446744073709551615\", now)"));
    let ignore_attribute = format!("#[{}", "ignore");
    assert!(!transport.contains(&ignore_attribute));
}
