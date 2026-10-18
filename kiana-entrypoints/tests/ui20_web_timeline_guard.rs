//! UI-20 source/boundary guard.
//!
//! Complements `ui20_web_timeline.rs` and the runtime `web::tests::ui20_*` fixtures with the
//! invariants that must survive refactors: the renderer maps an explicit server kind into text
//! nodes keyed by the server item id, the bounded window never drops pending/unknown items, the
//! server-owned migration switch fails closed for an unreviewed renderer, and the declarative
//! fixture, the DOM golden, the manual lane, the automatic shard and the baseline stay wired
//! together.

use serde_json::Value;

fn timeline_fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/ui20-web-timeline.json"))
        .expect("valid UI-20 timeline fixture")
}

fn timeline_golden() -> Value {
    serde_json::from_str(include_str!("fixtures/ui20-web-timeline-golden.json"))
        .expect("valid UI-20 DOM golden")
}

#[test]
fn ui20_renderer_uses_server_kind_text_nodes_and_stable_ids() {
    let page = include_str!("../src/web_page.html");
    let renderer = page
        .split("function renderTimelineItem(")
        .nth(1)
        .and_then(|rest| rest.split("function preserveProtectedTimelineItems").next())
        .expect("timeline item renderer");
    assert!(renderer.contains("normalizeServerTimelineItem"));
    assert!(renderer.contains("createElement('article')"));
    assert!(renderer.contains("dataset.renderKey"));
    assert!(renderer.contains("dataset.kind"));
    assert!(renderer.contains("dataset.component"));
    assert!(renderer.contains("textContent"));
    assert!(!renderer.contains("innerHTML"));
    assert!(!renderer.contains("body.includes"));
    assert!(!renderer.contains("body.match"));
    assert!(!renderer.contains("index"));

    let normalize = page
        .split("function normalizeServerTimelineItem(")
        .nth(1)
        .and_then(|rest| rest.split("function timelineLabelForKind").next())
        .expect("typed kind normalization");
    assert!(normalize.contains("item.kind"));
    assert!(normalize.contains("SERVER_ITEM_KINDS"));
    assert!(normalize.contains("'unknown'"));
    assert!(!normalize.contains("item.title.includes"));
    // The render key is the server item id (with a deterministic compatibility fallback), never the
    // array position of a projection.
    let key = page
        .split("function timelineStableKey(")
        .nth(1)
        .and_then(|rest| rest.split("function normalizeServerTimelineItem").next())
        .expect("stable item key");
    assert!(key.contains("item.id || item.event_id || item.key"));
    assert!(!key.contains("indexOf"));
    assert!(!key.contains("index"));
}

#[test]
fn ui20_window_keeps_pending_and_unknown_visible() {
    let page = include_str!("../src/web_page.html");
    let preserve = page
        .split("function preserveProtectedTimelineItems(")
        .nth(1)
        .and_then(|rest| rest.split("function timelineUnsupportedDetail").next())
        .expect("protected timeline window");
    assert!(preserve.contains("TIMELINE_WINDOW_SIZE"));
    assert!(preserve.contains("item.pending"));
    assert!(preserve.contains("TIMELINE_PROTECTED_KINDS"));
    assert!(preserve.contains("timelineProjection.order"));

    let trim = page
        .split("function trimTimelineProjection(")
        .nth(1)
        .and_then(|rest| rest.split("function appendTimelineItem").next())
        .expect("bounded timeline trim");
    assert!(trim.contains("TIMELINE_MAX_ITEMS"));
    assert!(trim.contains("!item.pending"));
    assert!(trim.contains("!TIMELINE_PROTECTED_KINDS.has(item.kind)"));
    assert!(!trim.contains("order.shift()"));
    let append = page
        .split("function appendTimelineItem(")
        .nth(1)
        .and_then(|rest| rest.split("function replaceTimelineFromServer").next())
        .expect("timeline append boundary");
    assert!(append.contains("timelineProjection.order.length >= TIMELINE_MAX_ITEMS"));
    assert!(append.contains("保留 pending/unknown"));
    // An all-protected projection keeps its hard bound and reports the limit instead of evicting a
    // protected item; the bound is checked before the new item is retained.
    assert!(append.contains("setTimelineState('partial'"));
    let bound_at = append
        .find("timelineProjection.order.length >= TIMELINE_MAX_ITEMS")
        .unwrap();
    let insert_at = append
        .find("timelineProjection.items.set(normalized.id, normalized);")
        .unwrap();
    assert!(
        bound_at < insert_at,
        "the protected overflow bound must be checked before an item is retained"
    );
}

#[test]
fn ui20_states_and_renderer_switch_fail_closed_without_execution_authority() {
    let page = include_str!("../src/web_page.html");
    for marker in [
        "setTimelineState",
        "data-state",
        "timelineProjection.loading",
        "timelineProjection.partial",
        "timelineProjection.replay",
        "replaceTimelineFromServer",
        "s.read_only ? 'replay'",
        "requestStreamHydrate",
        "hydrateCache",
        "stateCache",
        "timelineProjection.unsupported",
        "TIMELINE_ALLOWED_RENDERERS",
        "mountTimelineComponents",
        "applyTimelineContract",
    ] {
        assert!(
            page.contains(marker),
            "UI-20 state marker missing: {marker}"
        );
    }
    for forbidden in [
        "new KianaHarness",
        "new CapabilityBroker",
        "ControlPlane.execute",
        "innerHTML",
    ] {
        assert!(
            !page.contains(forbidden),
            "timeline page gained forbidden authority: {forbidden}"
        );
    }
    // A replay projection is marked as history and cannot be promoted to a live run: the only live
    // authority is the server stream, which the renderer never starts by itself.
    let replay = page
        .split("function replaceTimelineFromServer(")
        .nth(1)
        .and_then(|rest| rest.split("function renderTimelineProjection").next())
        .expect("replay projection");
    assert!(replay.contains("timelineProjection.replay = replay;"));
    assert!(replay.contains("replay ? 'replay' : state"));
    assert!(!replay.contains("ensureEventStream"));
    assert!(!replay.contains("/api/run"));

    let reconnect = page
        .split("function scheduleEventStreamReconnect")
        .nth(1)
        .and_then(|rest| rest.split("async function requestStreamHydrate").next())
        .expect("SSE reconnect state machine");
    assert!(!reconnect.contains("/api/run"));
    assert!(!reconnect.contains("/api/cancel"));
    assert!(!reconnect.contains("submitCommand"));
}

#[test]
fn ui20_switch_and_window_bounds_stay_server_owned() {
    let page = include_str!("../src/web_page.html");
    let source = include_str!("../src/web.rs");
    let golden = timeline_golden();

    // The migration switch is only ever read from the server contract: no URL, hash, local storage
    // or tab state may select a renderer, and a contract that diverges fails closed.
    for forbidden in [
        "location.hash",
        "location.search",
        "URLSearchParams",
        "searchParams",
        "localStorage.getItem('kiana.web-timeline",
        "indexedDB",
    ] {
        assert!(
            !page.contains(forbidden),
            "the timeline migration switch must stay server-owned: {forbidden}"
        );
    }
    let switch = page
        .split("function applyTimelineContract(")
        .nth(1)
        .and_then(|rest| rest.split("// <<< UI-20 timeline components").next())
        .expect("migration contract fence");
    assert!(switch.contains("contract.disposition !== 'active'"));
    assert!(switch.contains("renderer_not_reviewed"));
    assert!(switch.contains("window_contract_mismatch"));
    assert!(switch.contains("component_directory_mismatch"));
    assert!(switch.contains("bundle_not_reviewed"));
    assert!(switch.contains("timelineProjection.unsupported = null"));
    assert_eq!(golden["unsupported_renderer"]["state"], "partial");
    for kept in golden["unsupported_renderer"]["keeps"].as_array().unwrap() {
        assert!(
            ["pending", "unknown"].contains(&kept.as_str().unwrap()),
            "only protected items may stay visible for an unreviewed renderer: {kept}"
        );
    }

    // The server publishes the switch from the typed migration contract of the artifact it serves,
    // and the renderer request is validated before any lease or projection work.
    for marker in [
        "fn web_timeline_contract()",
        "WebTimelineMigration::select(PAGE)",
        "fn validate_timeline_renderer(",
        "web_timeline_renderer_text_guess_forbidden",
        "web_timeline_renderer_unsupported",
        "WEB_TIMELINE_RENDERER_TYPED_INLINE",
        "WEB_TIMELINE_RENDERER_TEXT_GUESS",
    ] {
        assert!(
            source.contains(marker),
            "UI-20 server marker missing from web.rs: {marker}"
        );
    }
    let validate = source
        .split("fn validate_timeline_renderer(")
        .nth(1)
        .and_then(|rest| rest.split("fn web_timeline_contract").next())
        .expect("renderer validation");
    assert!(validate.contains("None => Ok(WEB_TIMELINE_RENDERER_TYPED_INLINE)"));
    assert!(
        validate
            .find("web_timeline_renderer_text_guess_forbidden")
            .unwrap()
            < validate.find("web_timeline_renderer_unsupported").unwrap(),
        "a text-guessing request keeps its own stable denial code"
    );
    let bootstrap = source
        .split("async fn bootstrap(")
        .nth(1)
        .and_then(|rest| rest.split("async fn parity(").next())
        .expect("bootstrap handler");
    assert!(
        bootstrap.find("validate_timeline_renderer").unwrap()
            < bootstrap.find("claim_session_tab").unwrap(),
        "the renderer request is refused before the session lease is claimed"
    );
    assert!(
        !bootstrap.contains("harness_run::run_envelope_on_host"),
        "the hydrate route must not execute a run"
    );
}

/// Completion slice: the declarative fixture, the DOM golden, the behaviour fixtures, the manual
/// lane, the baseline document and the automatic shard must stay wired together.
#[test]
fn ui20_completion_slice_stays_wired_to_the_fixture_lane_and_shard() {
    let page = include_str!("../src/web_page.html");
    let source = include_str!("../src/web.rs");
    let tests = include_str!("ui20_web_timeline.rs");
    let fixture_raw = include_str!("fixtures/ui20-web-timeline.json");
    let golden_raw = include_str!("fixtures/ui20-web-timeline-golden.json");
    let baseline = include_str!("../../docs/roadmap/ui20-web-timeline-baseline.md");
    let card = include_str!("../../docs/roadmap/ui-entrypoints.md");
    let lane = include_str!("../../.github/workflows/ui20-web-timeline.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");

    // 1. the declared budgets and schemas match the real source constants.
    let fixture: Value =
        serde_json::from_str(fixture_raw).expect("the UI-20 fixture must stay valid JSON");
    assert_eq!(fixture["schema"], "kiana.ui20-web-timeline.v1");
    assert_eq!(fixture["step"], "UI-20");
    assert_eq!(fixture["budgets"]["page_bytes"].as_u64(), Some(160 * 1024));
    assert_eq!(
        fixture["budgets"]["component_bytes"].as_u64(),
        Some(64 * 1024)
    );
    assert_eq!(
        fixture["budgets"]["max_body_bytes"].as_u64(),
        Some(16 * 1024)
    );
    assert_eq!(fixture["budgets"]["window_size"].as_u64(), Some(64));
    assert_eq!(fixture["budgets"]["max_items"].as_u64(), Some(512));
    for declaration in [
        "const TIMELINE_WINDOW_SIZE = 64;",
        "const TIMELINE_MAX_ITEMS = 512;",
        "const TIMELINE_MAX_BODY_BYTES = 16 * 1024;",
        "const TIMELINE_ALLOWED_RENDERERS = Object.freeze({ typed_inline: 'typed_inline' });",
    ] {
        assert!(
            page.contains(declaration),
            "UI-20 browser budget drifted from: {declaration}"
        );
    }
    // The marked component region is the measured baseline, not an arbitrary slice of the page.
    for marker in [
        kiana_client::WEB_TIMELINE_COMPONENT_START_MARKER,
        kiana_client::WEB_TIMELINE_COMPONENT_END_MARKER,
    ] {
        assert!(
            page.contains(marker),
            "the served artifact must keep its component markers: {marker}"
        );
    }
    let bundle = kiana_client::WebTimelineBundle::measure(page)
        .expect("the served artifact keeps a marked component region");
    assert!(bundle.within_budget, "{bundle:?}");
    assert_eq!(bundle.page_bytes, page.len());

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
            "UI-20 fixture is not defined: {name}"
        );
    }
    for name in [
        "ui20_bootstrap_publishes_the_server_owned_timeline_migration_contract",
        "ui20_timeline_renderer_requests_denied_before_lease_or_projection",
    ] {
        assert!(
            source.contains(&format!("async fn {name}(")),
            "the UI-20 runtime fixture must live in web.rs: {name}"
        );
    }
    // Build the needle so the fixture/source checks never match this file's own assertion.
    let ignore_attribute = format!("#[{}]", "ignore");
    assert!(!source.contains(&ignore_attribute));
    assert!(!tests.contains(&ignore_attribute));
    assert!(!fixture_raw.contains("--ignored"));
    assert!(!golden_raw.contains("--ignored"));

    // 3. the conceptual denials of the card stay declared and mapped to concrete guards, the two
    // renderer requests stay real source literals, and the DOM golden matches the shipped renderer.
    let denials: Vec<&str> = fixture["deny_first"]
        .as_array()
        .expect("deny_first")
        .iter()
        .map(|value| value.as_str().expect("denial"))
        .collect();
    for denial in [
        "text_body_kind_guess",
        "innerHTML_untrusted_timeline",
        "array_index_render_key",
        "window_drops_pending",
        "window_drops_unknown",
        "protected_overflow_stays_bounded",
        "replay_promoted_to_live",
        "timeline_starts_execution_loop",
        "stream_reconnect_submits_command",
    ] {
        assert!(
            denials.contains(&denial),
            "UI-20 must keep its deny-first declaration: {denial}"
        );
    }
    for code in [
        "web_timeline_renderer_text_guess_forbidden",
        "web_timeline_renderer_unsupported",
    ] {
        assert!(
            source.contains(code),
            "the server source is missing the denial code: {code}"
        );
        assert!(
            denials.contains(&code),
            "the fixture is missing the denial code: {code}"
        );
    }
    // Each conceptual denial is implemented by a named guard in this file, so a marker rename cannot
    // silently drop the deny path.
    let this_file = include_str!("ui20_web_timeline_guard.rs");
    for guard in [
        "ui20_renderer_uses_server_kind_text_nodes_and_stable_ids",
        "ui20_window_keeps_pending_and_unknown_visible",
        "ui20_states_and_renderer_switch_fail_closed_without_execution_authority",
        "ui20_switch_and_window_bounds_stay_server_owned",
        "ui20_completion_slice_stays_wired_to_the_fixture_lane_and_shard",
    ] {
        assert!(
            this_file.contains(&format!("fn {guard}(")),
            "the UI-20 guard file lost its deny-path guard: {guard}"
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
    let client = include_str!("../../kiana-client/src/web_timeline.rs");
    for marker in fixture["client_required"]
        .as_array()
        .expect("client_required")
    {
        let marker = marker.as_str().expect("client marker");
        assert!(
            client.contains(marker),
            "the typed client source is missing the contract marker: {marker}"
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
            "the page must never gain local state or authority: {forbidden}"
        );
    }
    let golden: Value =
        serde_json::from_str(golden_raw).expect("the UI-20 golden must stay valid JSON");
    assert_eq!(golden["schema"], "kiana.ui20-web-timeline-golden.v1");
    assert_eq!(golden["step"], "UI-20");
    assert_eq!(golden["source"], "kiana-entrypoints/src/web_page.html");
    assert_eq!(golden["screenshot"]["status"], "deferred");
    let item_kinds: Vec<&str> = fixture["server_item_kinds"]
        .as_array()
        .expect("server_item_kinds")
        .iter()
        .map(|value| value.as_str().expect("kind"))
        .collect();
    let golden_kinds: Vec<&str> = golden["items"]
        .as_array()
        .expect("golden items")
        .iter()
        .map(|value| value["kind"].as_str().expect("kind"))
        .collect();
    assert_eq!(item_kinds, golden_kinds);
    // The route declaration is still the read-only token-protected hydrate route.
    for route in fixture["routes"].as_array().expect("routes") {
        let path = route["path"].as_str().expect("route path");
        assert_eq!(route["method"], "GET");
        assert_eq!(route["token_required"], true);
        assert_eq!(route["class"], "State");
        assert!(
            source.contains(&format!(".route(\"{path}\"")),
            "UI-20 route is not registered: {path}"
        );
    }

    // 4. required markers are documented in the baseline, and the card/baseline keep their anchors.
    for marker in fixture["required"].as_array().expect("required") {
        let marker = marker.as_str().expect("required marker");
        assert!(
            baseline.contains(marker),
            "UI-20 baseline is missing the required marker: {marker}"
        );
    }
    assert!(card.contains("UI-20"));
    assert!(card.contains("ui20-web-timeline-baseline.md"));
    for marker in [
        "ui20-web-timeline.json",
        "ui20-web-timeline-golden.json",
        "ui20_web_timeline",
        "ui20_web_timeline_guard",
        "ui20-web-timeline.yml",
        "proof_level",
        "limitations",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-20 baseline is missing: {marker}"
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
            "UI-20 lane must not use {forbidden}"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-entrypoints --test ui20_web_timeline --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --test ui20_web_timeline_guard --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --lib web::tests::ui20_ --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --no-run --locked",
    ] {
        assert!(lane.contains(command), "UI-20 lane is missing: {command}");
    }

    // 6. the automatic gate keeps covering the new targets through the whole-crate shard.
    let shards: Value =
        serde_json::from_str(shards_raw).expect("the shard registry must stay valid JSON");
    assert!(
        shards["kiana-entrypoints"]["targets"].is_null(),
        "kiana-entrypoints must keep its whole-crate shard so new UI-20 targets are discovered"
    );

    // 7. no second execution path or authority was added to the timeline renderer.
    let timeline_region = page
        .split("// >>> UI-20 timeline components")
        .nth(1)
        .and_then(|rest| rest.split("// <<< UI-20 timeline components").next())
        .expect("timeline component region");
    for forbidden in [
        "KianaHarness",
        "kiana_runner",
        "CapabilityBroker",
        "ControlPlane",
        "Command::new(",
        "harness_run",
        "innerHTML",
        "eval(",
        "Function(",
        "document.write",
    ] {
        assert!(
            !timeline_region.contains(forbidden),
            "the timeline component region must not gain a second executor: {forbidden}"
        );
    }
    for forbidden in [
        "KianaHarness",
        "kiana_runner",
        "CapabilityBroker",
        "Command::new(",
        "std::process",
    ] {
        let timeline_server = source
            .split("fn validate_timeline_renderer(")
            .nth(1)
            .and_then(|rest| rest.split("fn web_now_unix_ms").next())
            .expect("timeline server helpers");
        assert!(
            !timeline_server.contains(forbidden),
            "the timeline server helpers must not gain a second executor: {forbidden}"
        );
    }
    assert!(!fixture["limitations"].as_array().unwrap().is_empty());
}
