#[test]
fn ui13_renderer_is_bounded_typed_and_display_only() {
    let renderer = include_str!("../src/workbench_render.rs");
    let workbench = include_str!("../src/workbench_chat.rs");

    for marker in [
        "MAX_TIMELINE_ITEMS",
        "MAX_TIMELINE_BYTES",
        "MAX_TIMELINE_ITEM_BYTES",
        "MAX_TIMELINE_JSON_BYTES",
        "TimelineItemKind::Gap",
        "TimelineItemKind::Limit",
        "IgnoredDuplicate",
        "GapRequiresSnapshot",
        "IgnoredAfterTerminal",
        "redact_and_strip_controls",
        "Authorization header is a scheme plus credentials",
        "timeline_event_run_mismatch",
        "toggle_collapsed",
        "set_loading",
    ] {
        assert!(
            renderer.contains(marker),
            "UI-13 renderer marker missing: {marker}"
        );
    }
    for marker in [
        "self.timeline.reset(run_id)",
        "self.timeline.apply_event(event.clone())",
        "fn display_messages(&self)",
        "TimelineItemKind::ToolCall",
        "TimelineItemKind::Approval",
        "TimelineItemKind::Artifact",
    ] {
        assert!(
            workbench.contains(marker),
            "Workbench timeline wiring missing: {marker}"
        );
    }

    for forbidden in [
        "std::process::Command",
        "tokio::spawn",
        "ControlPlane",
        "CapabilityBroker",
        "KianaHarness",
        "reqwest::",
        "std::fs::",
        "pulldown_cmark",
    ] {
        assert!(
            !renderer.contains(forbidden),
            "renderer gained execution, transport, filesystem, or markdown authority: {forbidden}"
        );
    }
}
