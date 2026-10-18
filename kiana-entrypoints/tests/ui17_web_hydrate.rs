//! UI-17 completion slice: Web snapshot hydrate, history and pagination.
//!
//! The runtime boundary fixtures live in `kiana-entrypoints/src/web.rs` (`web::tests::ui17_*`)
//! because they drive the real loopback router over a seeded EventLog. This file keeps the
//! declarative fixture, the browser order/cache contract and the runtime-fixture wiring bound to
//! the source.

use serde_json::Value;

fn hydrate_fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/ui17-web-hydrate.json"))
        .expect("valid UI-17 hydrate fixture")
}

#[test]
fn ui17_fixture_binds_hydrate_cache_and_page_cursor_contract() {
    let fixture = hydrate_fixture();
    assert_eq!(fixture["schema"], "kiana.ui17-web-hydrate.v1");
    assert_eq!(fixture["step"], "UI-17");
    assert_eq!(fixture["hydrate_schema"], "kiana.web-hydrate.v1");
    assert_eq!(fixture["history_schema"], "kiana.web-history-page.v1");
    assert_eq!(fixture["artifact_schema"], "kiana.web-artifact-page.v1");
    assert_eq!(fixture["trace_schema"], "kiana.web-hydrate-trace.v1");
    assert_eq!(fixture["cache"]["storage"], "memory_only");
    assert_eq!(fixture["cache"]["stale_action"], "web_page_cursor_replayed");
    for state in ["loading", "empty", "partial", "limited", "ready", "offline"] {
        assert!(
            fixture["states"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == state),
            "UI-17 state {state} must stay declared"
        );
    }
    for step in fixture["sequence"].as_array().unwrap() {
        assert!(
            fixture["trace"]["page_steps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == step),
            "the declared sequence must be recorded by the page trace: {step}"
        );
    }

    // Declared budgets must match the real source constants (server and browser).
    let source = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    let budgets = &fixture["budgets"];
    for (budget, declaration) in [
        (
            "history_page_max",
            "const MAX_WEB_HISTORY_PAGE: usize = 64;",
        ),
        (
            "page_cursor_max",
            "const MAX_WEB_PAGE_CURSORS: usize = 256;",
        ),
        (
            "page_cursor_bytes",
            "const MAX_WEB_PAGE_CURSOR_BYTES: usize = 256;",
        ),
        (
            "server_trace_steps",
            "const MAX_WEB_HYDRATE_TRACE_STEPS: usize = 4;",
        ),
    ] {
        assert!(
            budgets[budget].as_u64().is_some(),
            "UI-17 server budget {budget} must stay declared"
        );
        assert!(
            source.contains(declaration),
            "UI-17 server budget {budget} drifted from: {declaration}"
        );
    }
    for (budget, declaration) in [
        ("hydrate_cache_entries", "const MAX_HYDRATE_CACHE = 8;"),
        (
            "history_cache_entries",
            "const MAX_HISTORY_CACHE_ENTRIES = 256;",
        ),
        ("history_cache_sessions", "const MAX_HISTORY_SESSIONS = 8;"),
        ("hydrate_trace_steps", "const MAX_HYDRATE_TRACE = 16;"),
    ] {
        assert!(
            page.contains(declaration),
            "UI-17 browser budget {budget} drifted from: {declaration}"
        );
    }
    assert_eq!(budgets["history_page_default"].as_u64(), Some(32));
    assert!(
        page.contains("'&limit=32'"),
        "the default history page size must stay bound to the fixture budget"
    );

    // Every declared deny-first rule is a real rejection code/marker in the server source.
    for code in fixture["deny_first"].as_array().unwrap() {
        let code = code.as_str().unwrap();
        assert!(
            source.contains(code),
            "UI-17 server denial code missing from kiana-entrypoints/src/web.rs: {code}"
        );
    }
    // Browser-side refusals and cache fences exist, and no persisted Web state does.
    for marker in fixture["browser_required"].as_array().unwrap() {
        let marker = marker.as_str().unwrap();
        assert!(
            page.contains(marker),
            "UI-17 browser contract marker missing from web_page.html: {marker}"
        );
    }
    for forbidden in fixture["browser_forbidden"].as_array().unwrap() {
        let forbidden = forbidden.as_str().unwrap();
        assert!(
            !page.contains(forbidden),
            "the page must never persist Web state: {forbidden}"
        );
    }
    for forbidden in fixture["trace"]["forbidden"].as_array().unwrap() {
        let forbidden = forbidden.as_str().unwrap();
        assert!(
            !page.contains(&format!("recordHydrateTrace('{forbidden}")),
            "a forbidden payload must never become a trace step: {forbidden}"
        );
    }
    // Every hydrate/history/artifact route keeps its token requirement.
    for route in fixture["routes"].as_array().unwrap() {
        let path = route["path"].as_str().unwrap();
        assert!(
            source.contains(&format!(".route(\"{path}\"")),
            "UI-17 route is not registered: {path}"
        );
        assert_eq!(route["token_required"], true);
        let class = match route["class"].as_str().unwrap() {
            "bootstrap" => "WebRouteClass::Bootstrap",
            "history" => "WebRouteClass::History",
            "artifact" => "WebRouteClass::Artifact",
            other => panic!("unknown UI-17 route class: {other}"),
        };
        assert!(
            source.contains(class),
            "UI-17 route class missing from the matrix: {class}"
        );
    }
    for forbidden in ["localStorage", "ModelClient", "CapabilityBroker"] {
        assert!(!source.contains(forbidden), "Web server leaked {forbidden}");
    }
    assert!(!fixture["limitations"].as_array().unwrap().is_empty());
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
            < render_body.find("ensureEventStream(sessionId)").unwrap(),
        "the snapshot must be accepted before the feed listener is installed"
    );
    assert!(
        render_body.find("ensureEventStream(sessionId)").unwrap()
            < render_body
                .find("recordHydrateTrace('resume_or_render')")
                .unwrap(),
        "the render commit must be the last hydrate step"
    );

    // The hydrate trace records the canonical order and never carries credentials or user text.
    let accept = page
        .split("function acceptHydrateSnapshot")
        .nth(1)
        .and_then(|rest| rest.split("function assertHistoryPage").next())
        .unwrap();
    assert!(accept.contains("recordHydrateTrace('hydrate_snapshot')"));
    assert!(accept.contains("envelope.cache !== 'memory_only'"));
    assert!(accept.contains("hydrate_cache_scope_invalid"));
    assert!(
        accept
            .find("recordHydrateTrace('hydrate_snapshot')")
            .unwrap()
            < accept.find("return true").unwrap(),
        "an accepted snapshot must be recorded before it is rendered"
    );
    assert!(accept.contains("recordHydrateTrace('hydrate_scope_reset'"));
    assert!(accept.contains("historyPages.delete(snapshot.session_id)"));
    let stream_body = page
        .split("function ensureEventStream")
        .nth(1)
        .and_then(|rest| rest.split("function renderBusyStatus").next())
        .unwrap();
    assert!(stream_body.contains("recordHydrateTrace('install_feed_listener')"));
    let history = page
        .split("async function loadHistoryPage(")
        .nth(1)
        .and_then(|rest| rest.split("function announce").next())
        .unwrap();
    assert!(history.contains("recordHydrateTrace('history_replay_reject'"));
    assert!(history.contains("recordHydrateTrace('history_page')"));
    assert!(history.contains("evictHistorySessions()"));
    let trace_fn = page
        .split("function recordHydrateTrace(")
        .nth(1)
        .and_then(|rest| rest.split("window.__KIANA_HYDRATE_TRACE__").next())
        .unwrap();
    for forbidden in [
        "__KIANA_WEB_TOKEN__",
        "promptEl",
        "tabDrafts",
        "approval",
        "artifact",
    ] {
        assert!(
            !trace_fn.contains(forbidden),
            "the hydrate trace must stay credential-free and payload-free: {forbidden}"
        );
    }
    let evict = page
        .split("function evictHistorySessions(")
        .nth(1)
        .and_then(|rest| rest.split("async function loadHistoryPage").next())
        .unwrap();
    assert!(evict.contains("MAX_HISTORY_SESSIONS"));
    assert!(evict.contains("key !== sessionId"));

    assert!(page.contains("const tabOwnerId = crypto.randomUUID();"));
    assert!(page.contains("const hydrateCache = new Map();"));
    assert!(page.contains("cacheKeyFor(session)"));
    assert!(page.contains("envelope.tab_id !== tabOwnerId"));
    assert!(page.contains("MAX_HYDRATE_CACHE"));
    assert!(page.contains("history_page_scope_mismatch"));
    assert!(page.contains("history_cursor_replayed"));
    assert!(page.contains("x-kiana-ui-tab"));
    assert!(page.contains("/api/bootstrap"));
    assert!(page.contains("/api/history"));
    assert!(page.contains("snapshot loading"));
    assert!(page.contains("snapshot offline"));
    assert!(page.contains("window.addEventListener('offline'"));
    assert!(!page.contains("localStorage.setItem('kiana.web"));
    assert!(!page.contains("localStorage.getItem('kiana.web"));
    assert!(!page.contains("localStorage.setItem('kiana.state"));
    assert!(!page.contains("localStorage.getItem('kiana.state"));
}

#[test]
fn ui17_runtime_fixtures_are_wired_to_the_web_module() {
    let fixture = hydrate_fixture();
    let source = include_str!("../src/web.rs");
    let declared: Vec<&str> = fixture["fixtures"]
        .as_array()
        .expect("fixture list")
        .iter()
        .map(|value| value.as_str().expect("fixture name"))
        .collect();
    assert!(
        declared.len() >= 7,
        "the completion slice keeps every named fixture"
    );
    let this_file = include_str!("ui17_web_hydrate.rs");
    for name in &declared {
        assert!(
            this_file.contains(&format!("fn {name}(")) || source.contains(&format!("fn {name}(")),
            "declared UI-17 fixture is not defined anywhere: {name}"
        );
    }
    for name in [
        "ui17_bootstrap_hydrates_before_feed_and_matches_the_cli_parity_snapshot",
        "ui17_history_pages_follow_one_server_cursor_and_refuse_replay_and_foreign_tabs",
        "ui17_artifact_page_is_run_scoped_and_never_returns_bytes",
        "ui17_source_change_fence_rejects_a_cursor_that_moved_mid_read",
    ] {
        assert!(
            source.contains(&format!("async fn {name}(")),
            "the runtime boundary fixture must live in web.rs: {name}"
        );
    }
    // Build the needle so this file does not match its own assertion.
    let ignore_attribute = format!("#[{}]", "ignore");
    assert!(!source.contains(&ignore_attribute));
    assert!(!this_file.contains(&ignore_attribute));
    // The runtime fixtures drive the real router/host and read the CLI parity projection.
    for marker in [
        "axum::serve(listener, router(app)).await",
        "write_ledger(&home, &events)",
        "DaemonHost::local()",
        "harness_run::parity_envelope_on_host",
        "EntryPointKind::Cli",
        "web_page_cursor_scope_mismatch",
        "web_page_cursor_replayed",
        "web_page_cursor_limit_mismatch",
        "web_page_source_changed",
        "WEB_HYDRATE_TRACE_SCHEMA",
        "mark_historical_hydrate",
        "ensure_page_source_unchanged",
    ] {
        assert!(
            source.contains(marker),
            "UI-17 runtime wiring marker missing: {marker}"
        );
    }
    let declared_server_steps: Vec<&str> = fixture["trace"]["server_steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(declared_server_steps, ["snapshot", "history", "feed"]);
    for step in &declared_server_steps {
        assert!(
            source.contains(&format!("\"step\": \"{step}\"")),
            "the server hydrate trace must declare the step: {step}"
        );
    }
}
