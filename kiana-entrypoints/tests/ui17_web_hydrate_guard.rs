#[test]
fn ui17_server_queries_are_read_only_and_cursor_bound() {
    let source = include_str!("../src/web.rs");
    for marker in [
        "async fn bootstrap",
        "async fn history",
        "async fn artifact",
        "resolve_human_session",
        "history_page",
        "artifact_page",
        "consume_page_cursor",
        "issue_page_cursor",
        "web_page_cursor_replayed",
        "web_page_cursor_unknown",
        "web_page_cursor_scope_mismatch",
        "web_page_cursor_limit_mismatch",
        "web_page_source_changed",
        "artifact_content_requires_server_ref",
        "attach_hydrate_tab",
    ] {
        assert!(
            source.contains(marker),
            "UI-17 guard marker missing: {marker}"
        );
    }
    let history = source
        .split("async fn history(")
        .nth(1)
        .and_then(|rest| rest.split("async fn artifact(").next())
        .expect("history route");
    assert!(history.contains("authorize_mutation(&app, &headers)?"));
    assert!(history.contains("require_web_tab(&headers)?"));
    assert!(history.contains("history_page("));
    assert!(!history.contains("claim_ui_headers"));
    assert!(!history.contains("harness_run::"));
    let artifact = source
        .split("async fn artifact(")
        .nth(1)
        .and_then(|rest| rest.split("struct EventStreamState").next())
        .expect("artifact route");
    assert!(artifact.contains("require_web_tab(&headers)?"));
    assert!(artifact.contains("artifact_page("));
    assert!(!artifact.contains("std::fs"));
    assert!(!artifact.contains("Command::new"));
    assert!(source.contains("cache: \"memory_only\""));
}

#[test]
fn ui17_browser_cache_never_promotes_local_state_or_cross_tab_data() {
    let page = include_str!("../src/web_page.html");
    for marker in [
        "const tabOwnerId = crypto.randomUUID();",
        "cacheKeyFor(session)",
        "envelope.tab_id !== tabOwnerId",
        "memory_only",
        "history_page_scope_mismatch",
        "history_cursor_replayed",
        "snapshot offline",
        "snapshot loading",
        "loadHistoryPage",
    ] {
        assert!(
            page.contains(marker),
            "UI-17 browser guard marker missing: {marker}"
        );
    }
    assert!(!page.contains("localStorage.setItem('kiana.web"));
    assert!(!page.contains("localStorage.getItem('kiana.web"));
    assert!(!page.contains("localStorage.setItem('kiana.state"));
    assert!(!page.contains("localStorage.getItem('kiana.state"));
}
