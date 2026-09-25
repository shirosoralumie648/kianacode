use serde_json::Value;

#[test]
fn ui20_fixture_captures_typed_server_kinds_and_bounded_states() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/ui20-web-timeline.json"))
        .expect("valid UI-20 timeline fixture");
    assert_eq!(fixture["schema"], "kiana.web-timeline.v1");
    assert_eq!(fixture["item_schema"], "kiana.web-timeline-item.v1");
    for kind in ["delta", "tool", "approval", "error", "unknown", "terminal"] {
        assert!(fixture["server_item_kinds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == kind));
    }
    for state in ["loading", "partial", "replay", "ready", "offline"] {
        assert!(fixture["visible_states"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == state));
    }
    assert_eq!(fixture["window"]["size"], 64);
    assert_eq!(fixture["window"]["max_items"], 512);
    for protected in ["pending", "unknown"] {
        assert!(fixture["window"]["protected"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == protected));
    }
    for denial in [
        "text_body_kind_guess",
        "innerHTML_untrusted_timeline",
        "array_index_render_key",
        "window_drops_pending",
        "window_drops_unknown",
        "protected_overflow_stays_bounded",
        "replay_promoted_to_live",
        "timeline_starts_execution_loop",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == denial));
    }
}

#[test]
fn ui20_client_and_server_expose_stable_timeline_contract() {
    let client = include_str!("../../kiana-client/src/web_timeline.rs");
    let thread = include_str!("../src/web_thread.rs");
    for marker in [
        "WEB_TIMELINE_SCHEMA",
        "WEB_TIMELINE_ITEM_SCHEMA",
        "WebTimelineItemKind",
        "WebTimelineViewState",
        "WebTimelineWindow",
        "WEB_TIMELINE_WINDOW_SIZE",
        "WEB_TIMELINE_MAX_ITEMS",
        "is_protected",
        "from_server_kind",
    ] {
        assert!(
            client.contains(marker),
            "UI-20 client marker missing: {marker}"
        );
    }
    for marker in [
        "pub id: String",
        "array position as a key",
        "format!(\"{item_prefix}:user\")",
        "format!(\"{item_prefix}:error\")",
    ] {
        assert!(
            thread.contains(marker),
            "UI-20 server item marker missing: {marker}"
        );
    }
}

#[test]
fn ui20_browser_source_wires_typed_timeline_to_existing_hydrate_and_sse_state() {
    let page = include_str!("../src/web_page.html");
    for marker in [
        "WEB_TIMELINE_SCHEMA",
        "SERVER_ITEM_KINDS",
        "normalizeServerTimelineItem",
        "renderTimelineItem",
        "renderTimelineProjection",
        "preserveProtectedTimelineItems",
        "TIMELINE_WINDOW_SIZE",
        "TIMELINE_MAX_ITEMS",
        "timelineState",
        "loading",
        "partial",
        "replay",
        "stream_gap",
        "requestStreamHydrate",
        "hydrateCache",
        "stateCache",
        "textContent",
        "dataset.renderKey",
    ] {
        assert!(page.contains(marker), "UI-20 page marker missing: {marker}");
    }
    for kind in ["delta", "tool", "approval", "error", "unknown", "terminal"] {
        assert!(page.contains(kind), "UI-20 page kind missing: {kind}");
    }
}
