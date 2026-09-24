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
    assert!(!detail.contains("Command::new"));
    assert!(!detail.contains("std::fs"));
    let diff = source
        .split("async fn diff_detail(")
        .nth(1)
        .and_then(|rest| rest.split("async fn receipt_detail(").next())
        .expect("diff detail route");
    assert!(diff.contains("diff_detail_page("));
    assert!(!diff.contains("harness_run::"));
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
