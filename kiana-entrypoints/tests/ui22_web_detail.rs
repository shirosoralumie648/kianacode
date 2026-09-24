use serde_json::Value;

#[test]
fn ui22_fixture_binds_server_owned_detail_contracts() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/ui22-web-detail.json"))
        .expect("valid UI-22 artifact detail fixture");
    assert_eq!(fixture["schema"], "kiana.web-artifact-detail.v1");
    for key in [
        "scope_schema",
        "artifact_ref_schema",
        "artifact_page_schema",
        "diff_schema",
        "receipt_schema",
    ] {
        assert!(fixture[key].as_str().is_some(), "missing {key}");
    }
    for denial in fixture["deny_first"].as_array().unwrap() {
        assert!(!denial.as_str().unwrap().is_empty());
    }
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let client = include_str!("../../kiana-client/src/web_detail.rs");
    let web = include_str!("../src/web.rs");
    for marker in [
        "UI_ARTIFACT_DETAIL_SCHEMA",
        "UiArtifactRefV1",
        "UiArtifactPageV1",
        "UiDiffDetailV1",
        "UiReceiptDetailV1",
        "UiDetailScopeV1",
        "UI_DETAIL_LINK_SCHEMA",
    ] {
        assert!(
            protocol.contains(marker),
            "protocol marker missing: {marker}"
        );
    }
    for marker in [
        "WebArtifactViewer",
        "WebDetailCursor",
        "web_artifact_revision_mismatch",
        "web_artifact_page_digest_mismatch",
        "WebDetailStatus::Unknown",
        "content_text",
    ] {
        assert!(client.contains(marker), "client marker missing: {marker}");
    }
    for marker in [
        "artifact_detail_page",
        "diff_detail_page",
        "receipt_detail_page",
        "WEB_ARTIFACT_DETAIL_SCHEMA",
        "WEB_DIFF_DETAIL_SCHEMA",
        "WEB_RECEIPT_DETAIL_SCHEMA",
        "authorize_feed_tab",
        "artifact_not_found",
        "web_page_source_changed",
    ] {
        assert!(web.contains(marker), "server marker missing: {marker}");
    }
}

#[test]
fn ui22_detail_fixture_keeps_unknown_and_security_cases_explicit() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/ui22-web-detail.json"))
        .expect("valid UI-22 artifact detail fixture");
    let deny = fixture["deny_first"].as_array().unwrap();
    for expected in [
        "client_payload_mutation",
        "stale_artifact_revision",
        "page_digest_mismatch",
        "cross_session_artifact_enumeration",
        "unknown_result_green_success",
        "result_unknown_auto_retry",
        "raw_secret_leak",
        "raw_path_leak",
        "html_svg_ansi_injection",
        "client_computed_diff",
        "arbitrary_url_fetch",
    ] {
        assert!(
            deny.iter().any(|value| value == expected),
            "missing {expected}"
        );
    }
    let page = include_str!("../src/web_page.html");
    assert!(page.contains("loadArtifactDetail"));
    assert!(page.contains("loadDiffDetail"));
    assert!(page.contains("/api/artifact/detail"));
    assert!(page.contains("/api/diff"));
    assert!(page.contains("textContent"));
    assert!(!page.contains("innerHTML"));
    assert!(!page.contains("fetch(artifact"));
    assert!(!page.contains("window.location = artifact"));
}
