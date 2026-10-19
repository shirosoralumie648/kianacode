#[test]
fn ui22_web_detail_routes_are_read_only_and_server_scoped() {
    let source = include_str!("../src/web.rs");
    for route in ["/api/artifact/detail", "/api/diff", "/api/receipt/detail"] {
        assert!(source.contains(&format!(".route(\"{route}\"")));
    }
    for marker in [
        "require_web_tab(&headers)?",
        "authorize_feed_tab(&app, &session_id, &tab_id)?",
        "artifact_not_found",
        "WEB_DETAIL_SCOPE_SCHEMA",
        "content_encoding",
        "server_diff_not_available",
        "result_unknown",
        "query original receipt",
        "web_page_cursor_expired",
        "web_partial_file_approval_unsupported",
        "artifact_binary_content_not_rendered",
        "artifact_size_limit_exceeded",
        "receipt_source_is_typed_run_projection",
        "WEB_ARTIFACT_MAX_DETAIL_BYTES",
        "mime_is_text",
    ] {
        assert!(
            source.contains(marker),
            "UI-22 guard marker missing: {marker}"
        );
    }
    let detail = source
        .split("async fn artifact_detail(")
        .nth(1)
        .and_then(|rest| rest.split("async fn diff_detail(").next())
        .expect("artifact detail route");
    assert!(detail.contains("authorize_mutation(&app, &headers)?"));
    assert!(detail.contains("artifact_detail_page("));
    assert!(detail.contains("refuse_partial_file_selection(query.files.as_deref())?"));
    assert!(!detail.contains("Command::new"));
    assert!(!detail.contains("std::fs"));
    let diff = source
        .split("async fn diff_detail(")
        .nth(1)
        .and_then(|rest| rest.split("async fn receipt_detail(").next())
        .expect("diff detail route");
    assert!(diff.contains("diff_detail_page("));
    assert!(diff.contains("refuse_partial_file_selection(query.files.as_deref())?"));
    assert!(!diff.contains("harness_run::"));
    let receipt = source
        .split("async fn receipt_detail_page(")
        .nth(1)
        .and_then(|rest| rest.split("fn issue_page_cursor(").next())
        .expect("receipt detail page");
    assert!(receipt.contains("run_receipt"));
    assert!(receipt.contains("typed_digest"));
    assert!(receipt.contains("receipt_source_is_typed_run_projection"));
    assert!(
        !receipt.contains("Command::new"),
        "receipt detail must not execute anything"
    );
    let page = include_str!("../src/web_page.html");
    for forbidden in [
        "innerHTML",
        "new KianaHarness",
        "CapabilityBroker",
        "fetch(artifact",
    ] {
        assert!(
            !page.contains(forbidden),
            "Web detail gained authority: {forbidden}"
        );
    }
}
