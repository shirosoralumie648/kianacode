//! UI-18 source/boundary guard.
//!
//! Complements `ui18_web_sse.rs` and the runtime `web::tests::ui18_*` fixtures with the invariants
//! that must survive refactors: the SSE route stays read-only and bounded, the reconnect state
//! machine cannot submit a command, a lagged/slow consumer becomes an explicit error instead of an
//! unbounded buffer, and the declarative fixture, the manual lane, the automatic shard and the
//! baseline stay wired together.

use serde_json::Value;

fn sse_fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/ui18-web-sse.json"))
        .expect("valid UI-18 SSE fixture")
}

#[test]
fn ui18_sse_route_is_read_only_bounded_and_cursor_fenced() {
    let source = include_str!("../src/web.rs");
    let events = source
        .split("async fn events(")
        .nth(1)
        .and_then(|rest| rest.split("fn stream_attach_state").next())
        .expect("events handler");
    for marker in [
        "authorize_sse(&app",
        "stream_cursor_from_request",
        "subscribe_run_after",
        "next_run_stream_frame",
    ] {
        assert!(
            events.contains(marker),
            "UI-18 event guard marker missing: {marker}"
        );
    }
    for forbidden in [
        "harness_run::run_envelope_on_host",
        "harness_run::continue_envelope_on_host",
        "harness_run::cancel_envelope_on_host",
    ] {
        assert!(
            !events.contains(forbidden),
            "the display route must never execute a run: {forbidden}"
        );
    }
    // The single frame pump owns the heartbeat/gap/error/terminal mapping.
    let pump = source
        .split("async fn next_run_stream_frame")
        .nth(1)
        .and_then(|rest| rest.split("async fn notification_events").next())
        .expect("frame pump");
    for marker in [
        "WEB_SSE_HEARTBEAT_INTERVAL",
        "stream_heartbeat_sse_event",
        "stream_error_sse_event_with_cursor",
        "stream_subscription_lagged",
        "stream_closed_before_terminal",
        "run_stream_sse_event",
        "RunStreamEvent::Unknown => continue",
        "state.done = terminal",
    ] {
        assert!(pump.contains(marker), "UI-18 pump marker missing: {marker}");
    }
    assert!(
        !pump.contains("stream_gap_sse_event_with_cursor"),
        "the gap frame belongs to the attach path, not to the pump"
    );
    assert!(
        !pump.contains("web_token") && !pump.contains("println!"),
        "display frames must never carry a credential or a log line"
    );
    // A cursor is fully validated before it can reach a subscription.
    let cursor = source
        .split("fn stream_cursor_from_request")
        .nth(1)
        .and_then(|rest| rest.split("fn run_stream_sse_event").next())
        .expect("cursor parser");
    for marker in [
        "last-event-id",
        "stream_cursor_conflict",
        "stream_cursor_invalid",
        "MAX_WEB_SSE_CURSOR_BYTES",
    ] {
        assert!(
            cursor.contains(marker),
            "UI-18 cursor marker missing: {marker}"
        );
    }
    // Bounded frames: an oversized delta degrades into a bounded hydrate-required control frame.
    let helpers = source
        .split("fn run_stream_sse_event")
        .nth(1)
        .and_then(|rest| rest.split("async fn run_turn").next())
        .expect("SSE payload helpers");
    for marker in [
        "MAX_WEB_SSE_EVENT_BYTES",
        "stream_payload_too_large",
        "bounded_sse_reason",
        "WEB_SSE_SCHEMA",
    ] {
        assert!(
            helpers.contains(marker),
            "UI-18 bounded-frame marker missing: {marker}"
        );
    }
    assert!(!helpers.contains("web_token"));
    assert!(!helpers.contains("println!"));
}

#[test]
fn ui18_reconnect_never_persists_state_or_resubmits_commands() {
    let page = include_str!("../src/web_page.html");
    for marker in [
        "SSE_RECONNECT_BASE_MS",
        "SSE_RECONNECT_MAX_MS",
        "SSE_MAX_RECONNECTS",
        "MAX_STREAM_TEXT_BYTES",
        "MAX_SSE_METRIC_REASONS",
        "window.__KIANA_SSE_METRICS__",
        "scheduleEventStreamReconnect",
        "requestStreamHydrate",
        "stream_sequence_gap",
        "stream_epoch_changed",
        "stream_reconnect_exhausted",
        "duplicate_frame",
        "source.addEventListener('heartbeat', handleStreamHeartbeat)",
        "source.addEventListener('stream_gap', handleStreamGap)",
        "source.addEventListener('stream_error', handleStreamError)",
        "x-kiana-web-token",
    ] {
        assert!(
            page.contains(marker),
            "UI-18 page guard marker missing: {marker}"
        );
    }
    let reconnect = page
        .split("function scheduleEventStreamReconnect")
        .nth(1)
        .and_then(|rest| rest.split("async function requestStreamHydrate").next())
        .expect("reconnect state machine");
    assert!(reconnect.contains("SSE_MAX_RECONNECTS"));
    assert!(reconnect.contains("setTimeout"));
    assert!(reconnect.contains("ensureEventStream(session, false, true)"));
    assert!(!reconnect.contains("/api/run"));
    assert!(!reconnect.contains("submitCommand"));
    assert!(!reconnect.contains("api('/api/cancel'"));
    assert!(!page.contains("console.log(window.__KIANA_WEB_TOKEN__"));
    assert!(!page.contains("history.pushState({}, '', window.__KIANA_WEB_TOKEN__"));
    for forbidden in [
        "localStorage.setItem('kiana.web",
        "localStorage.getItem('kiana.web",
        "sessionStorage.setItem('kiana",
        "new KianaHarness",
        "new CapabilityBroker",
    ] {
        assert!(
            !page.contains(forbidden),
            "the page must not persist or re-execute Web state: {forbidden}"
        );
    }
    // The metrics record is bounded and credential-free.
    let metrics = page
        .split("function recordSseMetric(")
        .nth(1)
        .and_then(|rest| rest.split("window.__KIANA_SSE_METRICS__").next())
        .expect("reconnect metrics");
    assert!(metrics.contains("MAX_SSE_METRIC_REASONS"));
    assert!(metrics.contains("sseMetrics.reasons.shift()"));
    for forbidden in ["__KIANA_WEB_TOKEN__", "tabDrafts", "promptEl"] {
        assert!(
            !metrics.contains(forbidden),
            "the metrics record must stay credential-free: {forbidden}"
        );
    }
}

/// Completion slice: the declarative fixture, the behaviour fixtures, the manual lane, the
/// baseline document and the automatic shard must stay wired together.
#[test]
fn ui18_completion_slice_stays_wired_to_the_fixture_lane_and_shard() {
    let source = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    let tests = include_str!("ui18_web_sse.rs");
    let fixture_raw = include_str!("fixtures/ui18-web-sse.json");
    let baseline = include_str!("../../docs/roadmap/ui18-web-sse-baseline.md");
    let card = include_str!("../../docs/roadmap/ui-entrypoints.md");
    let lane = include_str!("../../.github/workflows/ui18-web-sse.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");

    // 1. the declared budgets and schemas match the real source constants.
    let fixture: Value =
        serde_json::from_str(fixture_raw).expect("the UI-18 fixture must stay valid JSON");
    assert_eq!(fixture["schema"], "kiana.ui18-web-sse.v1");
    assert_eq!(fixture["step"], "UI-18");
    assert_eq!(fixture["budgets"]["frame_bytes"].as_u64(), Some(262_144));
    assert_eq!(
        fixture["budgets"]["browser_text_bytes"].as_u64(),
        Some(262_144)
    );
    assert_eq!(fixture["budgets"]["cursor_bytes"].as_u64(), Some(256));
    assert_eq!(fixture["budgets"]["reason_bytes"].as_u64(), Some(512));
    assert_eq!(fixture["budgets"]["heartbeat_seconds"].as_u64(), Some(15));
    assert_eq!(fixture["budgets"]["queue_capacity"].as_u64(), Some(256));
    assert_eq!(
        fixture["budgets"]["reconnect_max_attempts"].as_u64(),
        Some(6)
    );
    assert_eq!(fixture["budgets"]["metric_reasons"].as_u64(), Some(8));
    for declaration in [
        "pub const MAX_WEB_SSE_EVENT_BYTES: usize = 256 * 1024;",
        "const MAX_WEB_SSE_CURSOR_BYTES: usize = 256;",
        "const MAX_WEB_SSE_REASON_BYTES: usize = 512;",
        "const WEB_SSE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);",
    ] {
        assert!(
            source.contains(declaration),
            "UI-18 server budget drifted from: {declaration}"
        );
    }
    for declaration in [
        "const SSE_RECONNECT_BASE_MS = 250;",
        "const SSE_RECONNECT_MAX_MS = 8000;",
        "const SSE_MAX_RECONNECTS = 6;",
        "const MAX_STREAM_TEXT_BYTES = 256 * 1024;",
        "const MAX_SSE_METRIC_REASONS = 8;",
    ] {
        assert!(
            page.contains(declaration),
            "UI-18 browser budget drifted from: {declaration}"
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
        declared.len() >= 8,
        "the completion slice keeps every named fixture"
    );
    for name in &declared {
        assert!(
            tests.contains(&format!("fn {name}(")) || source.contains(&format!("fn {name}(")),
            "UI-18 fixture is not defined: {name}"
        );
    }
    for name in [
        "ui18_events_frames_carry_server_owned_cursors_and_fence_last_event_id_conflicts",
        "ui18_reconnect_and_gap_paths_never_resubmit_effectful_commands",
        "ui18_terminal_retention_replays_the_latest_terminal_without_deltas",
        "ui18_slow_tab_becomes_an_explicit_lagged_error_without_blocking_facts",
        "ui18_heartbeat_and_error_frames_re_state_the_cursor_without_payloads",
    ] {
        assert!(
            source.contains(&format!("async fn {name}(")),
            "the UI-18 runtime boundary fixture must live in web.rs: {name}"
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
    for marker in fixture["server_required"]
        .as_array()
        .expect("server_required")
    {
        let marker = marker.as_str().expect("server marker");
        assert!(
            source.contains(marker),
            "the server source is missing the contract marker: {marker}"
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
    // The route declaration is still the read-only token-protected SSE route.
    for route in fixture["routes"].as_array().expect("routes") {
        let path = route["path"].as_str().expect("route path");
        assert_eq!(route["method"], "GET");
        assert_eq!(route["token_required"], true);
        assert_eq!(route["class"], "Events");
        assert!(
            source.contains(&format!(".route(\"{path}\"")),
            "UI-18 route is not registered: {path}"
        );
    }

    // 4. required markers are documented in the baseline, and the card/baseline keep their anchors.
    for marker in fixture["required"].as_array().expect("required") {
        let marker = marker.as_str().expect("required marker");
        assert!(
            baseline.contains(marker),
            "UI-18 baseline is missing the required marker: {marker}"
        );
    }
    assert!(card.contains("UI-18"));
    assert!(card.contains("ui18-web-sse-baseline.md"));
    for marker in [
        "ui18-web-sse.json",
        "ui18_web_sse",
        "ui18_web_sse_guard",
        "ui18-web-sse.yml",
        "proof_level",
        "limitations",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-18 baseline is missing: {marker}"
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
            "UI-18 lane must not use {forbidden}"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-entrypoints --test ui18_web_sse --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --test ui18_web_sse_guard --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --lib web::tests::ui18_ --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --no-run --locked",
    ] {
        assert!(lane.contains(command), "UI-18 lane is missing: {command}");
    }

    // 6. the automatic gate keeps covering the new targets through the whole-crate shard.
    let shards: Value =
        serde_json::from_str(shards_raw).expect("the shard registry must stay valid JSON");
    assert!(
        shards["kiana-entrypoints"]["targets"].is_null(),
        "kiana-entrypoints must keep its whole-crate shard so new UI-18 targets are discovered"
    );

    // 7. no second execution path or authority was added to the display projection.
    let display = source
        .split("struct EventStreamState")
        .nth(1)
        .and_then(|rest| rest.split("async fn notification_events").next())
        .expect("display stream region");
    for forbidden in [
        "harness_run::",
        "execute_tool_call",
        "KianaHarness",
        "kiana_runner",
        "CapabilityBroker",
        "tokio::process",
        "std::process",
        "Command::new(",
    ] {
        assert!(
            !display.contains(forbidden),
            "the Web display stream must not gain a second executor: {forbidden}"
        );
    }
    for forbidden in [
        "KianaHarness",
        "kiana_runner",
        "CapabilityBroker",
        "ModelClient",
    ] {
        assert!(
            !source.contains(forbidden),
            "the Web entrypoint must not embed a second model loop: {forbidden}"
        );
    }
}
