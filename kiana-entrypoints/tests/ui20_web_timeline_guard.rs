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
}

#[test]
fn ui20_window_keeps_pending_and_unknown_visible() {
    let page = include_str!("../src/web_page.html");
    let preserve = page
        .split("function preserveProtectedTimelineItems(")
        .nth(1)
        .and_then(|rest| rest.split("function setTimelineState").next())
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
}

#[test]
fn ui20_states_and_replay_are_visible_without_execution_authority() {
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

    let reconnect = page
        .split("function scheduleEventStreamReconnect")
        .nth(1)
        .and_then(|rest| rest.split("async function requestStreamHydrate").next())
        .expect("SSE reconnect state machine");
    assert!(!reconnect.contains("/api/run"));
    assert!(!reconnect.contains("/api/cancel"));
    assert!(!reconnect.contains("submitCommand"));
}
