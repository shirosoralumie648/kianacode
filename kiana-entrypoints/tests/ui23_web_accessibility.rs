use serde_json::Value;

#[test]
fn ui23_fixture_captures_accessibility_and_content_security_matrix() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/ui23-web-accessibility.json"))
        .expect("valid UI-23 accessibility fixture");
    assert_eq!(fixture["schema"], "kiana.web-accessibility.v1");
    assert_eq!(fixture["focus_scope_schema"], "kiana.web-focus-scope.v1");
    assert_eq!(fixture["text_sink"], "textContent-only");
    assert_eq!(fixture["csp"]["unsafe_inline"], false);
    for state in ["loading", "ready", "partial", "unknown", "offline"] {
        assert!(fixture["states"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == state));
    }
    for denial in [
        "focus_escape_from_modal",
        "focus_return_to_stale_session",
        "focus_return_to_stale_epoch",
        "hidden_action_or_unknown_decision_enabled",
        "xss_innerHTML_or_inline_handler",
        "html_svg_markdown_execution",
        "javascript_or_data_url",
        "ansi_osc_control_sequence",
        "secret_or_raw_content_leakage",
        "stale_session_projection",
        "stale_epoch_projection",
        "csp_violation_swallowed",
        "unknown_result_auto_retry",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == denial));
    }
    assert_eq!(fixture["keyboard"]["modal_trap"], true);
    assert_eq!(fixture["keyboard"]["focus_restore"], true);
    assert_eq!(fixture["responsive"]["narrow_max_width_px"], 650);
    assert_eq!(fixture["responsive"]["zoom_reflow_max_width_px"], 400);
}

#[test]
fn ui23_page_uses_text_only_dom_and_deny_first_focus_scope() {
    let page = include_str!("../src/web_page.html");
    for marker in [
        "nonce=\"__KIANA_CSP_NONCE__\"",
        "aria-live",
        "aria-atomic",
        "aria-keyshortcuts",
        "skip-link",
        ":focus-visible",
        "FOCUSABLE_SELECTOR",
        "focusTrapState",
        "focusTrapKeydown",
        "focusScope",
        "currentUiEpoch",
        "setCoach(false)",
        "sanitizeUntrustedContent",
        "sanitizeDisplayText",
        "sanitizeUrl",
        "textContent",
        "stale_session",
        "stale_epoch",
        "securitypolicyviolation",
        "不能自动重试",
        "@media (max-width:650px)",
        "@media (max-width:400px)",
        "@media (prefers-contrast:more)",
        "@media (forced-colors:active)",
        "@media (prefers-reduced-motion:reduce)",
    ] {
        assert!(page.contains(marker), "UI-23 page marker missing: {marker}");
    }
    for forbidden in [
        "innerHTML",
        "unsafe-inline",
        "eval(",
        "new Function",
        "sendBeacon",
        "<script>alert",
    ] {
        assert!(
            !page.contains(forbidden),
            "unsafe Web page marker present: {forbidden}"
        );
    }
}

#[test]
fn ui23_server_and_typed_client_bind_nonce_scope_and_report_contracts() {
    let web = include_str!("../src/web.rs");
    let client = include_str!("../../kiana-client/src/web_accessibility.rs");
    for marker in [
        "/api/csp-report",
        "csp_report",
        "security_headers",
        "csp_nonce",
        "script-src 'nonce-",
        "style-src 'nonce-",
        "report-uri /api/csp-report",
        "authorize_host(&app, &headers)?",
        "DefaultBodyLimit::max(MAX_WEB_BODY_BYTES)",
    ] {
        assert!(web.contains(marker), "UI-23 Web marker missing: {marker}");
    }
    for marker in [
        "WEB_ACCESSIBILITY_SCHEMA",
        "WEB_FOCUS_SCOPE_SCHEMA",
        "WebFocusScope",
        "WebAccessibilityState",
        "validate_text_only",
        "sanitize_text_only",
        "web_text_ansi_or_osc_denied",
        "web_text_executable_url_denied",
        "matches(&self",
    ] {
        assert!(
            client.contains(marker),
            "UI-23 client marker missing: {marker}"
        );
    }
    assert!(!web.contains("unsafe-inline"));
}
