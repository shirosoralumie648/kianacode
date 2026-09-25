use serde_json::Value;

#[test]
fn ui17_fixture_binds_hydrate_cache_and_page_cursor_contract() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/ui17-web-hydrate.json"))
        .expect("valid UI-17 hydrate fixture");
    assert_eq!(fixture["schema"], "kiana.web-hydrate.v1");
    assert_eq!(fixture["history_schema"], "kiana.web-history-page.v1");
    assert_eq!(fixture["artifact_schema"], "kiana.web-artifact-page.v1");
    assert_eq!(fixture["cache"]["storage"], "memory_only");
    assert_eq!(fixture["cache"]["max_entries"], 8);
    for state in ["loading", "empty", "partial", "limited", "ready", "offline"] {
        assert!(fixture["states"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == state));
    }
    for expected in [
        "localStorage_state_only",
        "empty_response_is_no_session",
        "cross_tab_owner_cache_reuse",
        "web_page_cursor_replayed",
        "web_page_cursor_scope_mismatch",
        "web_page_cursor_source_changed",
        "scope_mismatch_does_not_consume_cursor",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == expected));
    }

    let source = include_str!("../src/web.rs");
    for marker in [
        "WEB_HYDRATE_SCHEMA",
        "WEB_HISTORY_SCHEMA",
        "WEB_ARTIFACT_SCHEMA",
        "WEB_PAGE_CURSOR_SCHEMA",
        "WebRouteClass::Bootstrap",
        "WebRouteClass::History",
        "WebRouteClass::Artifact",
        "async fn bootstrap",
        "async fn history",
        "async fn artifact",
        "page_context",
        "instance_id",
        "epoch",
        "session_id",
        "tab_id",
        "cache",
        "memory_only",
        "web_page_cursor_replayed",
        "web_page_cursor_scope_mismatch",
        "web_page_source_changed",
        "DaemonHost",
    ] {
        assert!(
            source.contains(marker),
            "UI-17 source marker missing: {marker}"
        );
    }
    for forbidden in ["localStorage", "ModelClient", "CapabilityBroker"] {
        assert!(!source.contains(forbidden), "Web server leaked {forbidden}");
    }
    for route in fixture["routes"].as_array().unwrap() {
        let path = route["path"].as_str().unwrap();
        assert!(source.contains(&format!(".route(\"{path}\"")));
    }
}

#[test]
fn ui17_page_hydrates_before_feed_and_keeps_cache_tab_scoped() {
    let page = include_str!("../src/web_page.html");
    let hydrate = page.find("function acceptHydrateSnapshot").unwrap();
    let stream = page.find("function ensureEventStream").unwrap();
    let render = page.find("function renderState").unwrap();
    assert!(hydrate < render);
    assert!(stream < render);
    let render_body = page
        .split("function renderState(s)")
        .nth(1)
        .and_then(|rest| rest.split("async function refresh()").next())
        .unwrap();
    assert!(
        render_body.find("acceptHydrateSnapshot(s)").unwrap()
            < render_body.find("ensureEventStream(sessionId)").unwrap()
    );
    assert!(page.contains("const tabOwnerId = crypto.randomUUID();"));
    assert!(page.contains("const hydrateCache = new Map();"));
    assert!(page.contains("MAX_HISTORY_CACHE_ENTRIES"));
    assert!(page.contains("WEB_HYDRATE_SCHEMA"));
    assert!(page.contains("WEB_HISTORY_SCHEMA"));
    assert!(page.contains("history_cursor_replayed"));
    assert!(page.contains("x-kiana-ui-tab"));
    assert!(page.contains("/api/bootstrap"));
    assert!(page.contains("/api/history"));
    assert!(page.contains("snapshot loading"));
    assert!(page.contains("history offline"));
    assert!(page.contains("window.addEventListener('offline'"));
    assert!(!page.contains("localStorage.setItem('kiana.web"));
    assert!(!page.contains("localStorage.getItem('kiana.web"));
}
