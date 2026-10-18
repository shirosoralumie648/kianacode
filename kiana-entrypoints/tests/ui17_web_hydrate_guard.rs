//! UI-17 source/boundary guard.
//!
//! Complements `ui17_web_hydrate.rs` and the runtime `web::tests::ui17_*` fixtures with the
//! invariants that must survive refactors: the hydrate/history/artifact queries stay read-only,
//! page cursors are consumed only after the full scope (and its limit) was validated, the browser
//! cache never promotes persisted local state or another tab's data, and the declarative fixture,
//! the manual lane, the automatic shard and the baseline stay wired together.

use serde_json::Value;

fn hydrate_fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/ui17-web-hydrate.json"))
        .expect("valid UI-17 hydrate fixture")
}

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
        "ensure_page_source_unchanged",
        "web_page_cursor_replayed",
        "web_page_cursor_unknown",
        "web_page_cursor_scope_mismatch",
        "web_page_cursor_limit_mismatch",
        "web_page_source_changed",
        "cursors.get(token).cloned()",
        "artifact_content_requires_server_ref",
        "attach_hydrate_tab",
        "mark_historical_hydrate",
        "WEB_HYDRATE_TRACE_SCHEMA",
        "MAX_WEB_HYDRATE_TRACE_STEPS",
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
    assert!(source.contains("\"cache\": \"memory_only\""));
    let cursor_scope = source
        .split("fn consume_page_cursor")
        .nth(1)
        .and_then(|rest| rest.split("// 为每次 daemon 调用").next())
        .expect("page cursor consumer");
    assert!(
        cursor_scope.find("web_page_cursor_scope_mismatch").unwrap()
            < cursor_scope.find("web_page_cursor_limit_mismatch").unwrap(),
        "the full cursor scope is validated before the follow-up limit"
    );
    assert!(
        cursor_scope.find("web_page_cursor_limit_mismatch").unwrap()
            < cursor_scope.find("cursors.remove(token)").unwrap(),
        "a wrong limit must not burn the owner's cursor"
    );
    assert!(
        cursor_scope.find("web_page_cursor_scope_mismatch").unwrap()
            < cursor_scope.find("cursors.remove(token)").unwrap(),
        "scope must be checked before a cursor is consumed"
    );
    // A historical hydrate stays read-only: no lease, no action authority.
    let historical = source
        .split("fn mark_historical_hydrate")
        .nth(1)
        .and_then(|rest| rest.split("fn validate_web_page_limit").next())
        .expect("historical hydrate marker");
    assert!(historical.contains("\"closed\""));
    assert!(historical.contains("\"event_log_projection\""));
    assert!(historical.contains("Value::Bool(false)"));
    let consume = source
        .split("fn consume_page_cursor")
        .nth(1)
        .and_then(|rest| rest.split("// 为每次 daemon 调用").next())
        .expect("cursor consumer");
    assert!(consume.contains("source_cursor"));
    assert!(!consume.contains("Command::new"));
    assert!(!consume.contains("std::fs"));
}

#[test]
fn ui17_browser_cache_never_promotes_local_state_or_cross_tab_data() {
    let page = include_str!("../src/web_page.html");
    for marker in [
        "const tabOwnerId = crypto.randomUUID();",
        "cacheKeyFor(session)",
        "envelope.tab_id !== tabOwnerId",
        "memory_only",
        "hydrate_cache_scope_invalid",
        "history_page_scope_mismatch",
        "history_cursor_replayed",
        "snapshot offline",
        "snapshot loading",
        "loadHistoryPage",
        "recordHydrateTrace('hydrate_snapshot')",
        "recordHydrateTrace('install_feed_listener')",
        "recordHydrateTrace('resume_or_render')",
        "recordHydrateTrace('history_page')",
        "window.__KIANA_HYDRATE_TRACE__",
        "MAX_HYDRATE_TRACE",
        "MAX_HISTORY_SESSIONS",
        "evictHistorySessions",
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
    assert!(!page.contains("sessionStorage.setItem('kiana"));
    assert!(!page.contains("sessionStorage.getItem('kiana"));
    assert!(!page.contains("new KianaHarness"));
    assert!(!page.contains("new CapabilityBroker"));
}

/// Completion slice: the declarative fixture, the behaviour fixtures, the manual lane, the
/// baseline document and the automatic shard must stay wired together.
#[test]
fn ui17_completion_slice_stays_wired_to_the_fixture_lane_and_shard() {
    let source = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    let tests = include_str!("ui17_web_hydrate.rs");
    let fixture_raw = include_str!("fixtures/ui17-web-hydrate.json");
    let baseline = include_str!("../../docs/roadmap/ui17-web-hydrate-baseline.md");
    let card = include_str!("../../docs/roadmap/ui-entrypoints.md");
    let lane = include_str!("../../.github/workflows/ui17-web-hydrate.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");

    // 1. the declared budgets and schemas match the real source constants.
    let fixture: Value =
        serde_json::from_str(fixture_raw).expect("the UI-17 fixture must stay valid JSON");
    assert_eq!(fixture["schema"], "kiana.ui17-web-hydrate.v1");
    assert_eq!(fixture["step"], "UI-17");
    assert_eq!(fixture["budgets"]["history_page_max"].as_u64(), Some(64));
    assert_eq!(fixture["budgets"]["page_cursor_max"].as_u64(), Some(256));
    assert_eq!(fixture["budgets"]["page_cursor_bytes"].as_u64(), Some(256));
    assert_eq!(fixture["budgets"]["server_trace_steps"].as_u64(), Some(4));
    for declaration in [
        "const MAX_WEB_HISTORY_PAGE: usize = 64;",
        "const MAX_WEB_PAGE_CURSORS: usize = 256;",
        "const MAX_WEB_PAGE_CURSOR_BYTES: usize = 256;",
        "const MAX_WEB_HYDRATE_TRACE_STEPS: usize = 4;",
    ] {
        assert!(
            source.contains(declaration),
            "UI-17 server budget drifted from: {declaration}"
        );
    }
    for declaration in [
        "const MAX_HYDRATE_CACHE = 8;",
        "const MAX_HISTORY_CACHE_ENTRIES = 256;",
        "const MAX_HISTORY_SESSIONS = 8;",
        "const MAX_HYDRATE_TRACE = 16;",
    ] {
        assert!(
            page.contains(declaration),
            "UI-17 browser budget drifted from: {declaration}"
        );
    }

    // 2. every declared fixture is a real behaviour test.
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
    for name in &declared {
        assert!(
            tests.contains(&format!("fn {name}(")) || source.contains(&format!("fn {name}(")),
            "UI-17 fixture is not defined: {name}"
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
            "the UI-17 runtime boundary fixture must live in web.rs: {name}"
        );
    }
    // Build the needle so the fixture/source checks never match this file's own assertion.
    let ignore_attribute = format!("#[{}]", "ignore");
    assert!(!source.contains(&ignore_attribute));
    assert!(!tests.contains(&ignore_attribute));
    assert!(!fixture_raw.contains("--ignored"));

    // 3. every declared denial is a real source literal, and the browser refusals exist.
    for code in fixture["deny_first"].as_array().expect("deny_first") {
        let code = code.as_str().expect("denial code");
        assert!(
            source.contains(code),
            "the server source is missing the denial code: {code}"
        );
    }
    for marker in fixture["browser_required"]
        .as_array()
        .expect("browser_required")
    {
        let marker = marker.as_str().expect("browser marker");
        assert!(
            page.contains(marker),
            "the browser source is missing the contract marker: {marker}"
        );
    }
    for forbidden in fixture["browser_forbidden"]
        .as_array()
        .expect("browser_forbidden")
    {
        let forbidden = forbidden.as_str().expect("forbidden marker");
        assert!(
            !page.contains(forbidden),
            "the page must never persist Web state: {forbidden}"
        );
    }

    // 4. required markers are documented in the baseline, and the card/baseline keep their anchors.
    for marker in fixture["required"].as_array().expect("required") {
        let marker = marker.as_str().expect("required marker");
        assert!(
            baseline.contains(marker),
            "UI-17 baseline is missing the required marker: {marker}"
        );
    }
    assert!(card.contains("UI-17"));
    assert!(card.contains("ui17-web-hydrate-baseline.md"));
    for marker in [
        "ui17-web-hydrate.json",
        "ui17_web_hydrate",
        "ui17_web_hydrate_guard",
        "ui17-web-hydrate.yml",
        "proof_level",
        "limitations",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-17 baseline is missing: {marker}"
        );
    }

    // 5. the manual lane stays dispatch-only, read-only and bound to the same targets.
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("permissions:"));
    assert!(lane.contains("contents: read"));
    for forbidden in [
        "pull_request",
        "  push:",
        "schedule:",
        "continue-on-error",
        "|| true",
        "--ignored",
    ] {
        assert!(
            !lane.contains(forbidden),
            "UI-17 lane must not use {forbidden}"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-entrypoints --test ui17_web_hydrate --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --test ui17_web_hydrate_guard --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --lib web::tests::ui17_ --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --no-run --locked",
    ] {
        assert!(lane.contains(command), "UI-17 lane is missing: {command}");
    }

    // 6. the automatic gate keeps covering the new targets through the whole-crate shard.
    let shards: Value =
        serde_json::from_str(shards_raw).expect("the shard registry must stay valid JSON");
    assert!(
        shards["kiana-entrypoints"]["targets"].is_null(),
        "kiana-entrypoints must keep its whole-crate shard so new UI-17 targets are discovered"
    );
    // 7. no second execution path or authority was added to the hydrate/history/artifact queries.
    let queries = source
        .split("async fn bootstrap(")
        .nth(1)
        .and_then(|rest| rest.split("struct EventStreamState").next())
        .expect("the hydrate/history/artifact query region");
    for forbidden in [
        "execute_tool_call",
        "KianaHarness",
        "kiana_runner",
        "CapabilityBroker",
        "tokio::process",
        "std::process",
        "Command::new(",
    ] {
        assert!(
            !queries.contains(forbidden),
            "the hydrated Web queries must not gain a second executor: {forbidden}"
        );
    }
}
