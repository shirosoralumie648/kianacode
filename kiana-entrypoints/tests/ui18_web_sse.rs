//! UI-18 completion slice: Web SSE reconnect, Last-Event-ID and gap.
//!
//! The runtime boundary fixtures live in `kiana-entrypoints/src/web.rs` (`web::tests::ui18_*`)
//! because they drive the real loopback router and a real `DaemonHost` run over the wire.  This
//! file keeps the declarative fixture, the browser reconnect/cursor contract and the
//! runtime-fixture wiring bound to the source.

use serde_json::Value;

fn sse_fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/ui18-web-sse.json"))
        .expect("valid UI-18 SSE fixture")
}

#[test]
fn ui18_fixture_binds_the_sse_reconnect_cursor_and_metric_contract() {
    let fixture = sse_fixture();
    assert_eq!(fixture["schema"], "kiana.ui18-web-sse.v1");
    assert_eq!(fixture["step"], "UI-18");
    assert_eq!(fixture["transport"], "sse");
    assert_eq!(fixture["route"], "/api/events");
    assert_eq!(fixture["event_schema"], "kiana.protocol.v1");
    assert_eq!(fixture["control_schema"], "kiana.web-sse.v1");
    assert_eq!(fixture["metrics_schema"], "kiana.web-sse-metrics.v1");
    for field in ["id", "event", "schema", "epoch", "sequence"] {
        assert!(
            fixture["event_fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == field),
            "UI-18 frame field {field} must stay declared"
        );
    }
    assert_eq!(fixture["last_event_id"]["header"], "Last-Event-ID");
    assert_eq!(fixture["last_event_id"]["query_fallback"], "last_event_id");
    assert_eq!(fixture["last_event_id"]["max_bytes"], 256);
    assert_eq!(
        fixture["last_event_id"]["conflict"],
        "stream_cursor_conflict"
    );
    assert_eq!(fixture["last_event_id"]["invalid"], "stream_cursor_invalid");
    for event in [
        "delta",
        "usage",
        "tool_call",
        "approval_requested",
        "runtime_error",
        "heartbeat",
        "stream_gap",
        "stream_error",
        "terminal",
    ] {
        assert!(
            fixture["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == event),
            "UI-18 event {event} must stay declared"
        );
    }
    for step in [
        "listener_before_command",
        "last_event_id_resume",
        "query_cursor_fallback",
        "heartbeat",
        "bounded_backoff",
        "max_reconnects",
        "gap_to_hydrate",
        "terminal_to_receipt",
    ] {
        assert!(
            fixture["lifecycle"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == step),
            "UI-18 lifecycle step {step} must stay declared"
        );
    }

    // Declared budgets must match the real source constants (server, browser and daemon queue).
    let source = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    let daemon = include_str!("../../kiana-daemon/src/run_stream.rs");
    let budgets = &fixture["budgets"];
    for (budget, declaration) in [
        (
            "frame_bytes",
            "pub const MAX_WEB_SSE_EVENT_BYTES: usize = 256 * 1024;",
        ),
        (
            "cursor_bytes",
            "const MAX_WEB_SSE_CURSOR_BYTES: usize = 256;",
        ),
        (
            "reason_bytes",
            "const MAX_WEB_SSE_REASON_BYTES: usize = 512;",
        ),
        (
            "heartbeat_seconds",
            "const WEB_SSE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);",
        ),
    ] {
        assert!(
            budgets[budget].as_u64().is_some(),
            "UI-18 server budget {budget} must stay declared"
        );
        assert!(
            source.contains(declaration),
            "UI-18 server budget {budget} drifted from: {declaration}"
        );
    }
    assert_eq!(budgets["queue_capacity"].as_u64(), Some(256));
    assert!(
        daemon.contains("const RUN_STREAM_CAPACITY: usize = 256;"),
        "the bounded display queue must stay bound to the fixture budget"
    );
    for declaration in [
        "const SSE_RECONNECT_BASE_MS = 250;",
        "const SSE_RECONNECT_MAX_MS = 8000;",
        "const SSE_MAX_RECONNECTS = 6;",
        "const MAX_STREAM_TEXT_BYTES = 256 * 1024;",
        "const MAX_SSE_METRIC_REASONS = 8;",
        "const SSE_METRICS_SCHEMA = 'kiana.web-sse-metrics.v1';",
    ] {
        assert!(
            page.contains(declaration),
            "UI-18 browser budget drifted from: {declaration}"
        );
    }
    assert_eq!(budgets["reconnect_max_attempts"].as_u64(), Some(6));
    assert_eq!(
        budgets["reconnect_backoff_ms"].as_array().unwrap(),
        &vec![Value::from(250), Value::from(8000)]
    );

    // Every declared server marker is real, every browser refusal exists, and no persisted Web
    // state or client-side harness was introduced.
    for marker in fixture["server_required"].as_array().unwrap() {
        let marker = marker.as_str().unwrap();
        assert!(
            source.contains(marker),
            "UI-18 server marker missing from kiana-entrypoints/src/web.rs: {marker}"
        );
    }
    for code in fixture["deny_first"].as_array().unwrap() {
        let code = code.as_str().unwrap();
        assert!(
            source.contains(code),
            "UI-18 server denial code missing from kiana-entrypoints/src/web.rs: {code}"
        );
    }
    for marker in fixture["browser_required"].as_array().unwrap() {
        let marker = marker.as_str().unwrap();
        assert!(
            page.contains(marker),
            "UI-18 browser contract marker missing from web_page.html: {marker}"
        );
    }
    for forbidden in fixture["browser_forbidden"].as_array().unwrap() {
        let forbidden = forbidden.as_str().unwrap();
        assert!(
            !page.contains(forbidden),
            "the page must never persist Web state: {forbidden}"
        );
    }
    // The display stream is a read-only route with the same token requirement as the rest.
    for route in fixture["routes"].as_array().unwrap() {
        let path = route["path"].as_str().unwrap();
        assert!(
            source.contains(&format!(".route(\"{path}\"")),
            "UI-18 route is not registered: {path}"
        );
        assert_eq!(route["token_required"], true);
        assert_eq!(route["class"], "Events");
        assert!(
            source.contains("WebRouteClass::Events"),
            "UI-18 SSE route class is missing from the matrix"
        );
    }
    for forbidden in ["localStorage", "ModelClient", "CapabilityBroker"] {
        assert!(!source.contains(forbidden), "Web server leaked {forbidden}");
    }
    assert!(!fixture["limitations"].as_array().unwrap().is_empty());
}

#[test]
fn ui18_browser_reconnect_state_machine_never_resubmits_commands_or_persists_state() {
    let page = include_str!("../src/web_page.html");
    let fixture = sse_fixture();

    // The reconnect state machine is bounded, backoff-driven, and never submits a command: a
    // dropped stream may only recreate the EventSource with the last server cursor.
    let reconnect = page
        .split("function scheduleEventStreamReconnect")
        .nth(1)
        .and_then(|rest| rest.split("async function requestStreamHydrate").next())
        .expect("reconnect state machine");
    for marker in [
        "SSE_MAX_RECONNECTS",
        "setTimeout",
        "ensureEventStream(session, false, true)",
        "recordSseMetric('reconnects', reason || 'network')",
        "recordSseMetric('exhausted', 'stream_reconnect_exhausted')",
        "requestStreamHydrate('stream_reconnect_exhausted')",
        ".catch(() => scheduleEventStreamReconnect(session, 'reconnect_failed'))",
    ] {
        assert!(
            reconnect.contains(marker),
            "UI-18 reconnect marker missing: {marker}"
        );
    }
    for forbidden in [
        "/api/run",
        "submitCommand",
        "api('/api/cancel'",
        "/api/command",
    ] {
        assert!(
            !reconnect.contains(forbidden),
            "a reconnect must never resubmit an effectful command: {forbidden}"
        );
    }

    // The listener set is installed before the open handler can release the caller, so a delta
    // published between resume and open is not dropped.
    let install = page
        .split("function ensureEventStream")
        .nth(1)
        .and_then(|rest| rest.split("function renderBusyStatus").next())
        .expect("event stream installer");
    let open_handler = install.find("source.onopen = ").expect("open handler");
    for listener in [
        "source.addEventListener('delta', handleStreamDelta)",
        "source.addEventListener('terminal', handleStreamTerminal)",
        "source.addEventListener('heartbeat', handleStreamHeartbeat)",
        "source.addEventListener('stream_gap', handleStreamGap)",
        "source.addEventListener('stream_error', handleStreamError)",
        "['usage','tool_call','approval_requested','runtime_error'].forEach(name => source.addEventListener(name, handleRuntimeEvent))",
    ] {
        let at = install
            .find(listener)
            .unwrap_or_else(|| panic!("UI-18 listener missing: {listener}"));
        assert!(
            at < open_handler,
            "listeners must be installed before the stream is released: {listener}"
        );
    }
    assert!(install.contains("recordSseMetric('installs', 'install_feed_listener')"));
    assert!(install.contains("recordSseMetric('errors', 'stream_connect_failed')"));
    assert!(install.contains("eventSourceOpen || Promise.resolve()"));
    assert!(install.contains("streamCursors.get(session)"));
    assert!(install.contains("last_event_id="));

    // The turn submission is the only path that may issue `/api/run`, and it awaits the listener.
    let send = page
        .split("async function sendPrompt")
        .nth(1)
        .and_then(|rest| rest.split("document.getElementById('send').onclick").next())
        .expect("command submission");
    assert!(send.contains("await ensureEventStream(sessionId, true)"));
    assert!(send.contains("api('/api/run'"));

    // A gap or error always hydrates a fresh snapshot and marks the turn incomplete; the cursor
    // fences reject duplicates, skipped frames and stale epochs.
    let hydrate = page
        .split("async function requestStreamHydrate")
        .nth(1)
        .and_then(|rest| rest.split("function handleStreamGap").next())
        .expect("snapshot hydration");
    for marker in [
        "recordSseMetric('hydrate_requests', reason || 'stream_gap')",
        "markStreamIncomplete(reason || 'stream_gap')",
        "closeEventStream(true)",
        "await refresh()",
        "recordSseMetric('hydrated', 'snapshot_hydrated')",
        "recordSseMetric('hydrate_offline', 'snapshot_unavailable')",
    ] {
        assert!(
            hydrate.contains(marker),
            "UI-18 hydrate path marker missing: {marker}"
        );
    }
    let cursor = page
        .split("function applyStreamCursor(")
        .nth(1)
        .and_then(|rest| rest.split("function readStreamEnvelope").next())
        .expect("display cursor fence");
    for marker in [
        "recordSseMetric('duplicate_frames', 'duplicate_frame')",
        "recordSseMetric('sequence_gaps', 'stream_sequence_gap')",
        "recordSseMetric('stale_epochs', 'stream_epoch_changed')",
        "requestStreamHydrate('stream_sequence_gap')",
        "requestStreamHydrate('stream_epoch_changed')",
    ] {
        assert!(
            cursor.contains(marker),
            "UI-18 cursor fence marker missing: {marker}"
        );
    }
    assert!(page.contains("markStreamIncomplete('stream_connect_failed')"));
    assert!(page.contains("'Builder · 不完整 / 已中断'"));
    assert!(page.contains("请以 receipt 为准"));

    // The observable reconnect metrics are bounded, credential-free, and rendered for the user.
    let metrics = page
        .split("function recordSseMetric(")
        .nth(1)
        .and_then(|rest| rest.split("window.__KIANA_SSE_METRICS__").next())
        .expect("reconnect metrics");
    for marker in [
        "MAX_SSE_METRIC_REASONS",
        "sseMetrics.reasons.shift()",
        "sseMetrics.last_cursor",
        "sseMetrics.updated_at",
        "renderSseMetrics()",
        "node.textContent",
    ] {
        assert!(
            metrics.contains(marker),
            "UI-18 metric contract marker missing: {marker}"
        );
    }
    for forbidden in [
        "__KIANA_WEB_TOKEN__",
        "promptEl",
        "tabDrafts",
        "approval",
        "artifact",
    ] {
        assert!(
            !metrics.contains(forbidden),
            "the reconnect metrics must stay credential-free and payload-free: {forbidden}"
        );
    }
    assert!(page.contains("role=\"status\" aria-live=\"off\" aria-label=\"实时流重连指标\""));
    assert!(page.contains("window.__KIANA_SSE_METRICS__ = sseMetrics;"));

    // No persisted Web state and no client-side execution surface.
    for forbidden in [
        "localStorage.setItem('kiana.web",
        "localStorage.getItem('kiana.web",
        "sessionStorage.setItem('kiana",
        "new KianaHarness",
        "new CapabilityBroker",
        "console.log(window.__KIANA_WEB_TOKEN__",
        "scheduleEventStreamReconnect(session, '/api/run'",
    ] {
        assert!(
            !page.contains(forbidden),
            "the page must never persist or re-execute Web state: {forbidden}"
        );
    }
    assert!(!fixture["browser_forbidden"].as_array().unwrap().is_empty());
}

#[test]
fn ui18_runtime_fixtures_are_wired_to_the_web_module() {
    let fixture = sse_fixture();
    let source = include_str!("../src/web.rs");
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
    let this_file = include_str!("ui18_web_sse.rs");
    for name in &declared {
        assert!(
            this_file.contains(&format!("fn {name}(")) || source.contains(&format!("fn {name}(")),
            "declared UI-18 fixture is not defined anywhere: {name}"
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
            "the runtime boundary fixture must live in web.rs: {name}"
        );
    }
    // Build the needle so this file does not match its own assertion.
    let ignore_attribute = format!("#[{}]", "ignore");
    assert!(!source.contains(&ignore_attribute));
    assert!(!this_file.contains(&ignore_attribute));
    // The runtime fixtures drive the real router/host over the wire instead of asserting markers.
    for marker in [
        "axum::serve(listener, router(app)).await",
        "DaemonHost::with_env_harness()",
        "ui16_page_token",
        "ui18_server",
        "ui18_open_stream",
        "ui18_next_frame",
        "ui18_expect_no_frame",
        "subscribe_run_after",
        "run_stream_cursor",
        "next_run_stream_frame",
        "stream_cursor_from_request",
        "stream_subscription_lagged",
        "UI_FEED_QUEUE_CAPACITY",
        "ui_events()",
        "PROTOCOL_SCHEMA",
    ] {
        assert!(
            source.contains(marker),
            "UI-18 runtime wiring marker missing: {marker}"
        );
    }
    // The scripted harness streams two deltas around a blocking tool call, which is what lets the
    // fixtures drop the connection in the middle of a real turn.
    assert!(
        source.contains("sleep 2"),
        "the stream script must stay paused"
    );
    assert!(source.contains("\"run.authorized\""));
    assert!(source.contains("\"run.prompt\""));
}
