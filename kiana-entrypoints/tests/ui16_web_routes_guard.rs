#[test]
fn ui16_web_routes_keep_authority_and_health_redaction_boundaries() {
    let source = include_str!("../src/web.rs");
    for marker in [
        "WEB_ROUTE_MATRIX",
        "WebRouteClass::Health",
        "WebRouteClass::State",
        "WebRouteClass::Sessions",
        "WebRouteClass::Events",
        "WebRouteClass::Run",
        "WebRouteClass::Cancel",
        "WebRouteClass::Trust",
        "WebRouteClass::Sandbox",
        "WebRouteClass::Session",
        "WebRouteClass::Receipt",
        "WebRouteClass::Approval",
        "WebRouteClass::Resume",
        "WebRouteClass::Command",
        "WebRouteClass::Diagnostics",
        "DefaultBodyLimit::max(MAX_WEB_BODY_BYTES)",
        "MAX_WEB_URI_BYTES",
        "MAX_WEB_REQUESTS_PER_WINDOW",
        "security_headers",
        "path_contains_traversal",
        "validate_web_session_id",
        "web_path_traversal_denied",
        "web_rate_limit_exceeded",
        "origin_matches_bound_addr",
        "single_header",
        "x-kiana-web-token",
        "DaemonHost",
        "harness_run::run_envelope_on_host",
    ] {
        assert!(
            source.contains(marker),
            "UI-16 guard marker missing: {marker}"
        );
    }

    let health = source
        .split("async fn health")
        .nth(1)
        .and_then(|rest| rest.split("async fn parity").next())
        .expect("health handler");
    assert!(
        !health.contains("folder"),
        "health must not leak an absolute folder"
    );
    assert!(
        !health.contains("{error}"),
        "health must not expose internal error text"
    );
    assert!(health.contains("health_projection_unavailable"));

    let auth = source
        .split("fn authorize_web_request")
        .nth(1)
        .and_then(|rest| rest.split("fn authorize_host").next())
        .expect("web authorization helper");
    assert!(auth.contains("supplied != Some(app.web_token.as_str())"));
    assert!(auth.contains("authorize_host(app, headers)?"));
    let host = source
        .split("fn authorize_host")
        .nth(1)
        .and_then(|rest| rest.split("fn authority_matches_bound_addr").next())
        .expect("host authorization helper");
    assert!(host.contains("origin_matches_bound_addr"));
    assert!(host.contains("authority_matches_bound_addr"));
}
