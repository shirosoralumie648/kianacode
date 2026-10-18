//! UI-20 completion slice: Web timeline component migration.
//!
//! The runtime boundary fixtures live in `kiana-entrypoints/src/web.rs` (`web::tests::ui20_*`)
//! because they drive the real loopback router and the embedded page artifact.  This file keeps
//! the declarative fixture, the typed client contract, the DOM golden and the runtime-fixture
//! wiring bound to the source.

use kiana_client::web_timeline::{
    web_timeline_window_bounds, WebTimelineBundle, WebTimelineComponent, WebTimelineItem,
    WebTimelineItemKind, WebTimelineMigration, WebTimelineMigrationDisposition, WebTimelineWindow,
    WEB_TIMELINE_BUNDLE_SCHEMA, WEB_TIMELINE_COMPONENTS, WEB_TIMELINE_COMPONENT_MAX_BYTES,
    WEB_TIMELINE_COMPONENT_SCHEMA, WEB_TIMELINE_ITEM_SCHEMA, WEB_TIMELINE_MAX_BODY_BYTES,
    WEB_TIMELINE_MAX_ID_BYTES, WEB_TIMELINE_MAX_ITEMS, WEB_TIMELINE_MIGRATION_SCHEMA,
    WEB_TIMELINE_PAGE_MAX_BYTES, WEB_TIMELINE_RENDERER_TEXT_GUESS,
    WEB_TIMELINE_RENDERER_TYPED_INLINE, WEB_TIMELINE_REVIEWED_RENDERERS, WEB_TIMELINE_SCHEMA,
    WEB_TIMELINE_WINDOW_SIZE,
};
use serde_json::Value;

fn timeline_fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/ui20-web-timeline.json"))
        .expect("valid UI-20 timeline fixture")
}

fn timeline_golden() -> Value {
    serde_json::from_str(include_str!("fixtures/ui20-web-timeline-golden.json"))
        .expect("valid UI-20 DOM golden")
}

fn page() -> &'static str {
    include_str!("../src/web_page.html")
}

#[test]
fn ui20_fixture_binds_the_typed_timeline_and_migration_contract() {
    let fixture = timeline_fixture();
    let page = page();
    let source = include_str!("../src/web.rs");
    assert_eq!(fixture["schema"], "kiana.ui20-web-timeline.v1");
    assert_eq!(fixture["step"], "UI-20");
    assert_eq!(fixture["item_schema"], WEB_TIMELINE_ITEM_SCHEMA);
    assert_eq!(fixture["window_schema"], WEB_TIMELINE_SCHEMA);
    assert_eq!(fixture["component_schema"], WEB_TIMELINE_COMPONENT_SCHEMA);
    assert_eq!(fixture["migration_schema"], WEB_TIMELINE_MIGRATION_SCHEMA);
    assert_eq!(fixture["bundle_schema"], WEB_TIMELINE_BUNDLE_SCHEMA);
    assert_eq!(
        fixture["selected_renderer"],
        WEB_TIMELINE_RENDERER_TYPED_INLINE
    );
    assert_eq!(
        fixture["reviewed_renderers"].as_array().unwrap().len(),
        WEB_TIMELINE_REVIEWED_RENDERERS.len()
    );
    assert_eq!(
        fixture["selected_renderer"].as_str(),
        Some(WEB_TIMELINE_REVIEWED_RENDERERS[0])
    );

    // The server-owned migration contract validates for the artifact this crate serves.
    let migration = WebTimelineMigration::select(page);
    assert!(migration.is_active(), "{migration:?}");
    assert_eq!(migration.validate(), Ok(()));
    assert_eq!(
        migration.selected,
        fixture["selected_renderer"].as_str().unwrap()
    );
    assert_eq!(
        migration.renderers.len(),
        fixture["reviewed_renderers"].as_array().unwrap().len()
    );
    let kinds: Vec<&str> = migration
        .server_item_kinds
        .iter()
        .map(String::as_str)
        .collect();
    for kind in fixture["server_item_kinds"].as_array().unwrap() {
        assert!(
            kinds.contains(&kind.as_str().unwrap()),
            "UI-20 kind {kind} must stay in the migration contract"
        );
    }
    for state in fixture["visible_states"].as_array().unwrap() {
        assert!(
            migration
                .visible_states
                .iter()
                .any(|value| value == state.as_str().unwrap()),
            "UI-20 state {state} must stay declared"
        );
    }

    // Declared budgets must match the real source constants (typed client, server and browser).
    let budgets = &fixture["budgets"];
    assert_eq!(
        budgets["page_bytes"].as_u64(),
        Some(WEB_TIMELINE_PAGE_MAX_BYTES as u64)
    );
    assert_eq!(
        budgets["component_bytes"].as_u64(),
        Some(WEB_TIMELINE_COMPONENT_MAX_BYTES as u64)
    );
    assert_eq!(
        budgets["max_body_bytes"].as_u64(),
        Some(WEB_TIMELINE_MAX_BODY_BYTES as u64)
    );
    assert_eq!(
        budgets["max_id_bytes"].as_u64(),
        Some(WEB_TIMELINE_MAX_ID_BYTES as u64)
    );
    assert_eq!(
        budgets["window_size"].as_u64(),
        Some(WEB_TIMELINE_WINDOW_SIZE as u64)
    );
    assert_eq!(
        budgets["max_items"].as_u64(),
        Some(WEB_TIMELINE_MAX_ITEMS as u64)
    );
    assert_eq!(
        fixture["window"]["size"].as_u64(),
        Some(web_timeline_window_bounds().size as u64)
    );
    assert_eq!(
        fixture["window"]["max_items"].as_u64(),
        Some(web_timeline_window_bounds().max_items as u64)
    );
    assert_eq!(
        fixture["window"]["older_page"].as_str(),
        Some(migration.window.older_page.as_str())
    );
    for protected in fixture["window"]["protected"].as_array().unwrap() {
        assert!(
            migration
                .window
                .protected
                .iter()
                .any(|value| value == protected.as_str().unwrap()),
            "UI-20 protected kind {protected} must stay declared"
        );
    }
    for declaration in [
        "const TIMELINE_WINDOW_SIZE = 64;",
        "const TIMELINE_MAX_ITEMS = 512;",
        "const TIMELINE_MAX_BODY_BYTES = 16 * 1024;",
        "const WEB_TIMELINE_SCHEMA = 'kiana.web-timeline.v1';",
        "const WEB_TIMELINE_ITEM_SCHEMA = 'kiana.web-timeline-item.v1';",
        "const WEB_TIMELINE_MIGRATION_SCHEMA = 'kiana.web-timeline-migration.v1';",
    ] {
        assert!(
            page.contains(declaration),
            "UI-20 browser budget drifted from: {declaration}"
        );
    }

    // The component directory is one typed contract, mirrored by the page with the same names,
    // selectors and dataset markers.
    let declared = fixture["components"].as_array().unwrap();
    assert_eq!(declared.len(), WEB_TIMELINE_COMPONENTS.len());
    for (entry, (name, selector, dataset)) in declared.iter().zip(WEB_TIMELINE_COMPONENTS.iter()) {
        assert_eq!(entry["name"].as_str(), Some(*name));
        assert_eq!(entry["selector"].as_str(), Some(*selector));
        assert_eq!(entry["dataset"].as_str(), Some(*dataset));
        let needle =
            format!("{{ name: '{name}', selector: '{selector}', component: '{dataset}' }}");
        assert!(
            page.contains(&needle),
            "the page component directory drifted from the typed contract: {needle}"
        );
    }
    for component in &migration.components {
        assert!(!component.may_execute, "{component:?}");
    }

    // Every refused renderer request is a real source literal; the conceptual denials of the card
    // are mapped to concrete assertions by the UI-20 guard.
    for (pattern, code) in [
        (
            "text_body_kind_guess",
            "web_timeline_renderer_text_guess_forbidden",
        ),
        (
            "renderer_outside_reviewed_allowlist",
            "web_timeline_renderer_unsupported",
        ),
    ] {
        assert_eq!(fixture["refused_renderer_requests"][pattern]["code"], code);
        assert!(
            source.contains(code),
            "the server source is missing the renderer denial code: {code}"
        );
    }
    assert_eq!(
        fixture["refused_renderer_requests"]["text_body_kind_guess"]["request"],
        WEB_TIMELINE_RENDERER_TEXT_GUESS
    );
    let denials: Vec<&str> = fixture["deny_first"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
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
    for forbidden in ["location.hash", "URLSearchParams", "searchParams"] {
        assert!(
            !page.contains(forbidden),
            "a migration switch value must never come from the URL: {forbidden}"
        );
    }
    for marker in fixture["server_required"].as_array().unwrap() {
        let marker = marker.as_str().unwrap();
        assert!(
            source.contains(marker),
            "UI-20 server marker missing from kiana-entrypoints/src/web.rs: {marker}"
        );
    }
    let client_source = include_str!("../../kiana-client/src/web_timeline.rs");
    for marker in fixture["client_required"].as_array().unwrap() {
        let marker = marker.as_str().unwrap();
        assert!(
            client_source.contains(marker),
            "UI-20 client marker missing from kiana-client/src/web_timeline.rs: {marker}"
        );
    }
    for marker in fixture["browser_required"].as_array().unwrap() {
        let marker = marker.as_str().unwrap();
        assert!(
            page.contains(marker),
            "UI-20 browser contract marker missing from web_page.html: {marker}"
        );
    }
    for forbidden in fixture["browser_forbidden"].as_array().unwrap() {
        let forbidden = forbidden.as_str().unwrap();
        assert!(
            !page.contains(forbidden),
            "the timeline must never gain authority or local state: {forbidden}"
        );
    }
    for route in fixture["routes"].as_array().unwrap() {
        let path = route["path"].as_str().unwrap();
        assert!(
            source.contains(&format!(".route(\"{path}\"")),
            "UI-20 route is not registered: {path}"
        );
        assert_eq!(route["method"], "GET");
        assert_eq!(route["token_required"], true);
        assert!(source.contains("WebRouteClass::State"));
    }
    assert!(!fixture["success"].as_array().unwrap().is_empty());
    assert!(!fixture["limitations"].as_array().unwrap().is_empty());
    for forbidden in ["localStorage", "ModelClient", "CapabilityBroker"] {
        assert!(!source.contains(forbidden), "Web server leaked {forbidden}");
    }
}

#[test]
fn ui20_typed_kind_and_window_contract_never_guess_from_text() {
    // The kind comes from the explicit server field only: every canonical kind, every declared
    // legacy alias, and an unknown fallback that stays `unknown` instead of guessing from text.
    for (server_kind, expected) in [
        ("delta", WebTimelineItemKind::Delta),
        ("tool", WebTimelineItemKind::Tool),
        ("approval", WebTimelineItemKind::Approval),
        ("error", WebTimelineItemKind::Error),
        ("unknown", WebTimelineItemKind::Unknown),
        ("terminal", WebTimelineItemKind::Terminal),
        ("userMessage", WebTimelineItemKind::Delta),
        ("agentMessage", WebTimelineItemKind::Delta),
        ("commandExecution", WebTimelineItemKind::Tool),
        ("fileChange", WebTimelineItemKind::Tool),
        ("future_item_kind", WebTimelineItemKind::Unknown),
        ("", WebTimelineItemKind::Unknown),
    ] {
        assert_eq!(WebTimelineItemKind::from_server_kind(server_kind), expected);
    }
    // "delta-looking" text never selects the kind: the same text is `unknown` for both a body and
    // a title, because neither is read.
    for text in ["delta", "tool", "terminal", "<b>error</b>"] {
        assert_eq!(
            WebTimelineItemKind::from_server_kind(text),
            WebTimelineItemKind::Unknown,
            "text must never select a canonical kind: {text}"
        );
    }
    assert!(WebTimelineItemKind::Unknown.is_protected(false));
    assert!(WebTimelineItemKind::Delta.is_protected(true));
    assert!(!WebTimelineItemKind::Delta.is_protected(false));

    let item = |id: &str, kind: WebTimelineItemKind, pending: bool| WebTimelineItem {
        schema: WEB_TIMELINE_ITEM_SCHEMA.to_owned(),
        id: id.to_owned(),
        kind,
        status: "completed".to_owned(),
        title: "delta".to_owned(),
        body: "tool".to_owned(),
        pending,
        replay: false,
        sequence: Some(1),
    };
    assert_eq!(
        item("stable-1", WebTimelineItemKind::Delta, false).validate(),
        Ok(())
    );
    assert_eq!(
        item("stable-1", WebTimelineItemKind::Terminal, true).validate(),
        Err("web_timeline_terminal_pending".to_owned())
    );
    assert_eq!(
        item("", WebTimelineItemKind::Delta, false).validate(),
        Err("web_timeline_item_id_invalid".to_owned())
    );
    let mut oversized = item("stable-2", WebTimelineItemKind::Delta, false);
    oversized.body = "x".repeat(WEB_TIMELINE_MAX_BODY_BYTES + 1);
    assert_eq!(
        oversized.validate(),
        Err("web_timeline_item_body_invalid".to_owned())
    );

    // Duplicate stable ids and an over-long cursor are refused by the window contract.
    let duplicate_window = WebTimelineWindow {
        schema: WEB_TIMELINE_SCHEMA.to_owned(),
        state: kiana_client::web_timeline::WebTimelineViewState::Ready,
        cursor: None,
        items: vec![
            item("dup", WebTimelineItemKind::Delta, false),
            item("dup", WebTimelineItemKind::Approval, true),
        ],
        has_older: false,
        replay: false,
    };
    assert_eq!(
        duplicate_window.validate(),
        Err("web_timeline_duplicate_item".to_owned())
    );
    // A recent window keeps every pending/unknown item even when older history is paged away.
    let protected_window = WebTimelineWindow {
        schema: WEB_TIMELINE_SCHEMA.to_owned(),
        state: kiana_client::web_timeline::WebTimelineViewState::Partial,
        cursor: Some("cursor-1".to_owned()),
        items: vec![
            item("delta-1", WebTimelineItemKind::Delta, false),
            item("pending-1", WebTimelineItemKind::Delta, true),
            item("unknown-1", WebTimelineItemKind::Unknown, false),
            item("terminal-1", WebTimelineItemKind::Terminal, false),
        ],
        has_older: true,
        replay: false,
    };
    assert_eq!(protected_window.validate(), Ok(()));
    let protected: Vec<&str> = protected_window.protected_item_ids().collect();
    assert_eq!(protected, vec!["pending-1", "unknown-1"]);
}

#[test]
fn ui20_bundle_and_component_region_fail_closed() {
    let page = page();

    // The served artifact measures inside the reviewed budgets, and its component region is the
    // marked block rather than the whole page.
    let bundle = WebTimelineBundle::measure(page).expect("marked component region");
    assert_eq!(bundle.schema, WEB_TIMELINE_BUNDLE_SCHEMA);
    assert_eq!(bundle.page_bytes, page.len());
    assert!(bundle.component_bytes > 0);
    assert!(bundle.component_bytes < bundle.page_bytes);
    assert!(bundle.within_budget);
    assert_eq!(bundle.validate(), Ok(()));
    let migration = WebTimelineMigration::select(page);
    assert_eq!(migration.bundle, bundle);
    assert_eq!(migration.bundle.page_bytes, page.len());

    // An unmarked artifact is not a zero-byte bundle: the migration is published as unavailable.
    let unmarked = "<html><body>no component markers</body></html>";
    assert!(WebTimelineBundle::measure(unmarked).is_none());
    let unavailable = WebTimelineMigration::select(unmarked);
    assert_eq!(
        unavailable.disposition,
        WebTimelineMigrationDisposition::Unavailable
    );
    assert_eq!(unavailable.reason, "web_timeline_component_region_missing");
    assert_eq!(
        unavailable.validate(),
        Err("web_timeline_migration_unavailable".to_owned())
    );
    assert!(!unavailable.bundle.within_budget);

    // An over-budget component region is refused instead of being rendered anyway.
    let oversized = format!(
        "{}{}{}",
        kiana_client::web_timeline::WEB_TIMELINE_COMPONENT_START_MARKER,
        "x".repeat(WEB_TIMELINE_COMPONENT_MAX_BYTES + 1),
        kiana_client::web_timeline::WEB_TIMELINE_COMPONENT_END_MARKER
    );
    let over = WebTimelineBundle::measure(&oversized).expect("marked region");
    assert!(!over.within_budget);
    assert_eq!(
        over.validate(),
        Err("web_timeline_bundle_over_budget".to_owned())
    );
    let denied = WebTimelineMigration::select(&oversized);
    assert_eq!(denied.reason, "web_timeline_bundle_over_budget");
    assert!(!denied.is_active());

    // A component may never carry execution or decision authority, and the window bounds cannot
    // drift away from the reviewed constants.
    let mut authoritative = WebTimelineComponent {
        name: "item".to_owned(),
        selector: "article.timeline-item".to_owned(),
        dataset_component: "item".to_owned(),
        may_execute: true,
    };
    assert_eq!(
        authoritative.validate(),
        Err("web_timeline_component_authority_denied".to_owned())
    );
    authoritative.may_execute = false;
    assert_eq!(authoritative.validate(), Ok(()));
    let mut drifted = WebTimelineMigration::select(page);
    drifted.window.size = WEB_TIMELINE_WINDOW_SIZE + 1;
    assert_eq!(
        drifted.validate(),
        Err("web_timeline_window_bounds_drifted".to_owned())
    );
    let mut unselected = WebTimelineMigration::select(page);
    unselected.selected = WEB_TIMELINE_RENDERER_TEXT_GUESS.to_owned();
    assert_eq!(
        unselected.validate(),
        Err("web_timeline_renderer_not_reviewed".to_owned())
    );
}

#[test]
fn ui20_browser_component_directory_and_switch_stay_text_only() {
    let page = page();
    let golden = timeline_golden();
    assert_eq!(golden["schema"], "kiana.ui20-web-timeline-golden.v1");
    assert_eq!(golden["step"], "UI-20");
    assert_eq!(golden["renderer"], WEB_TIMELINE_RENDERER_TYPED_INLINE);
    assert_eq!(golden["source"], "kiana-entrypoints/src/web_page.html");
    // No browser lane exists in CI, so a real screenshot stays an explicit deferral rather than a
    // claim.  The DOM golden below is what the lane can actually verify.
    assert_eq!(golden["screenshot"]["status"], "deferred");
    assert!(!golden["screenshot"]["reason"].as_str().unwrap().is_empty());

    // Item golden: class, label and dataset markers match the page renderer exactly.
    let item_golden = &golden["item"];
    assert_eq!(item_golden["tag"], "article");
    assert_eq!(item_golden["base_class"], "item timeline-item");
    assert_eq!(item_golden["class_prefix"], "timeline-");
    assert_eq!(item_golden["dataset_component"], "item");
    for attribute in item_golden["dataset_attributes"].as_array().unwrap() {
        let attribute = attribute.as_str().unwrap();
        assert!(
            page.contains(&format!("dataset.{attribute}")),
            "the golden dataset attribute is not set by the renderer: {attribute}"
        );
    }
    let renderer = page
        .split("function renderTimelineItem(")
        .nth(1)
        .and_then(|rest| rest.split("function preserveProtectedTimelineItems").next())
        .expect("timeline item renderer");
    let normalize = page
        .split("function normalizeServerTimelineItem(")
        .nth(1)
        .and_then(|rest| rest.split("function timelineLabelForKind").next())
        .expect("typed kind normalization");
    assert_eq!(golden["items"].as_array().unwrap().len(), 6);
    for item in golden["items"].as_array().unwrap() {
        let kind = item["kind"].as_str().unwrap();
        let suffix = item["class_suffix"].as_str().unwrap();
        assert_eq!(suffix, kind);
        let label = item["label"].as_str().unwrap();
        let needle = format!("case '{kind}': return '{label}';");
        assert!(
            page.contains(&needle),
            "the golden label drifted from the renderer: {needle}"
        );
        let css_needle = format!(".timeline-item.timeline-{suffix}");
        assert_eq!(
            page.contains(&css_needle),
            item["css_defined"].as_bool().unwrap(),
            "the golden CSS expectation drifted for {kind}"
        );
        if item["pending_default"].as_bool().unwrap() {
            assert!(
                normalize.contains("|| kind === 'approval'"),
                "a pending-by-default kind must be marked pending by the normalizer"
            );
        }
    }
    assert!(renderer.contains("'item timeline-item timeline-' + normalized.kind"));

    // Component golden: every declared component exists with its selector, dataset marker and (when
    // declared) its CSS rule.
    let components = golden["components"].as_array().unwrap();
    assert_eq!(components.len(), WEB_TIMELINE_COMPONENTS.len());
    for component in components {
        let selector = component["selector"].as_str().unwrap();
        assert!(
            page.contains(selector),
            "the golden component selector is missing: {selector}"
        );
        let dataset = component["dataset"].as_str().unwrap();
        assert!(
            page.contains(&format!("component: '{dataset}'")),
            "the golden component dataset marker is missing: {dataset}"
        );
        if component["css_defined"].as_bool() == Some(true) {
            let class = component["class"].as_str().unwrap();
            assert!(
                page.contains(&format!(".{class}")),
                "the golden component CSS rule is missing: {class}"
            );
        }
    }
    let turn = page
        .split("function renderTimelineProjection(")
        .nth(1)
        .and_then(|rest| rest.split("function mountTimelineComponents").next())
        .expect("timeline projection renderer");
    assert!(turn.contains("item.turn_id"));
    assert!(turn.contains("timeline-turn"));
    assert!(
        !turn.contains("index"),
        "a turn grouping must never be derived from the array position"
    );

    // The migration switch is read from the server contract only: a renderer, a window or a bundle
    // outside the reviewed contract fails closed, and the unsupported state stays visible.
    let switch = page
        .split("function applyTimelineContract(")
        .nth(1)
        .and_then(|rest| rest.split("// <<< UI-20 timeline components").next())
        .expect("timeline migration contract");
    for marker in [
        "contract.schema !== WEB_TIMELINE_MIGRATION_SCHEMA",
        "TIMELINE_ALLOWED_RENDERERS",
        "contract.disposition !== 'active'",
        "renderer_contract_schema_mismatch",
        "renderer_not_reviewed",
        "window_contract_mismatch",
        "component_directory_mismatch",
        "bundle_not_reviewed",
        "timelineProjection.unsupported = null",
    ] {
        assert!(
            switch.contains(marker),
            "UI-20 migration switch marker missing: {marker}"
        );
    }
    let unsupported = golden["unsupported_renderer"]["state"];
    assert_eq!(unsupported, "partial");
    let fail = page
        .split("function failTimelineRenderer(")
        .nth(1)
        .and_then(|rest| rest.split("function applyTimelineContract").next())
        .expect("renderer failure fence");
    assert!(fail.contains("timelineProjection.unsupported"));
    assert!(fail.contains("timelineProjection.renderer = 'unsupported'"));
    let state_fn = page
        .split("function setTimelineState(")
        .nth(1)
        .and_then(|rest| rest.split("function trimTimelineProjection").next())
        .expect("timeline state bar");
    assert!(
        state_fn.contains("if (timelineProjection.unsupported)"),
        "an unreviewed renderer may never be reported as ready"
    );
    let append = page
        .split("function appendTimelineItem(")
        .nth(1)
        .and_then(|rest| rest.split("function replaceTimelineFromServer").next())
        .expect("timeline append boundary");
    assert!(
        append.contains("!normalized.pending && !TIMELINE_PROTECTED_KINDS.has(normalized.kind)")
    );
    assert!(
        append.contains("timelineProjection.unsupported"),
        "the append boundary must keep refusing ordinary items for an unreviewed renderer"
    );

    // The switch never comes from the URL or local state, and the mount is a naming contract only.
    for forbidden in [
        "location.hash",
        "location.search",
        "URLSearchParams",
        "searchParams",
        "localStorage.setItem('kiana.web-timeline",
    ] {
        assert!(
            !page.contains(forbidden),
            "the timeline migration switch must stay server-owned: {forbidden}"
        );
    }
    let mount = page
        .split("function mountTimelineComponents(")
        .nth(1)
        .and_then(|rest| rest.split("function failTimelineRenderer").next())
        .expect("component mount");
    assert!(mount.contains("TIMELINE_COMPONENT_DIRECTORY.forEach"));
    assert!(mount.contains("node.dataset.component = component.component"));
    assert!(!mount.contains("innerHTML"));
}

#[test]
fn ui20_runtime_fixtures_are_wired_to_the_web_module() {
    let fixture = timeline_fixture();
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
    let this_file = include_str!("ui20_web_timeline.rs");
    for name in &declared {
        assert!(
            this_file.contains(&format!("fn {name}(")) || source.contains(&format!("fn {name}(")),
            "declared UI-20 fixture is not defined anywhere: {name}"
        );
    }
    for name in [
        "ui20_bootstrap_publishes_the_server_owned_timeline_migration_contract",
        "ui20_timeline_renderer_requests_denied_before_lease_or_projection",
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
    // The runtime fixtures drive the real router and the embedded artifact instead of asserting
    // markers, and the renderer denial runs before any lease or projection work.
    for marker in [
        "axum::serve(listener, router(app)).await",
        "DaemonHost::local()",
        "web_timeline_contract()",
        "WebTimelineMigration::select(PAGE)",
        "validate_timeline_renderer(query.timeline_renderer.as_deref())?",
        "ui17_server",
        "ui17_json",
        "use crate::web_ui::PAGE;",
    ] {
        assert!(
            source.contains(marker),
            "UI-20 runtime wiring marker missing: {marker}"
        );
    }
    let bootstrap = source
        .split("async fn bootstrap(")
        .nth(1)
        .and_then(|rest| rest.split("async fn parity(").next())
        .expect("bootstrap handler");
    let deny_at = bootstrap
        .find("validate_timeline_renderer")
        .expect("renderer validation");
    let claim_at = bootstrap
        .find("claim_session_tab")
        .expect("session lease claim");
    let project_at = bootstrap
        .find("app.snapshot(&session_id)")
        .expect("projection read");
    assert!(
        deny_at < claim_at && deny_at < project_at,
        "an unreviewed renderer request must be refused before any lease or projection work"
    );
    assert!(!bootstrap.contains("harness_run::run_envelope_on_host"));
}

/// The fixture, the golden and the shard registry must stay valid JSON so a marker edit cannot
/// silently disable the lane.
#[test]
fn ui20_declarative_assets_stay_machine_readable() {
    let fixture = timeline_fixture();
    let golden = timeline_golden();
    assert!(fixture.is_object() && golden.is_object());
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("the shard registry must stay valid JSON");
    assert!(
        shards["kiana-entrypoints"]["targets"].is_null(),
        "kiana-entrypoints must keep its whole-crate shard so new UI-20 targets are discovered"
    );
}
