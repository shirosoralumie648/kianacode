//! UI-13 boundary guards: the Workbench timeline is a bounded, append-only display projection.
//!
//! It may own item kinds, display roles and body budgets, but it must never own a daemon, broker,
//! event store, process spawn, filesystem or model loop. The completion-slice guard keeps the
//! declarative fixture, the behaviour fixtures, the manual lane, the baseline document and the
//! whole-crate shard wired together.

use kiana_entrypoints::workbench_render::{
    MAX_TIMELINE_ARTIFACTS_PER_RESPONSE, MAX_TIMELINE_BYTES, MAX_TIMELINE_DIFF_BYTES,
    MAX_TIMELINE_ITEMS, MAX_TIMELINE_ITEM_BYTES, MAX_TIMELINE_JSON_BYTES,
};

#[test]
fn ui13_renderer_is_bounded_typed_and_display_only() {
    let renderer = include_str!("../src/workbench_render.rs");
    let workbench = include_str!("../src/workbench_chat.rs");

    for marker in [
        "MAX_TIMELINE_ITEMS",
        "MAX_TIMELINE_BYTES",
        "MAX_TIMELINE_ITEM_BYTES",
        "MAX_TIMELINE_JSON_BYTES",
        "MAX_TIMELINE_DIFF_BYTES",
        "MAX_TIMELINE_ARTIFACTS_PER_RESPONSE",
        "TimelineItemKind::Gap",
        "TimelineItemKind::Limit",
        "TimelineDisplayRole",
        "fn display_role",
        "TimelineDisplayRole::Assistant",
        "TimelineDisplayRole::System",
        "TimelineDisplayRole::Changed",
        "IgnoredDuplicate",
        "GapRequiresSnapshot",
        "IgnoredAfterTerminal",
        "redact_and_strip_controls",
        "Authorization header is a scheme plus credentials",
        "timeline_event_run_mismatch",
        "toggle_collapsed",
        "set_loading",
        "fn bounded_item_body",
        "fn artifact_body",
        "fn binary_artifact",
        "fn text_mime",
        "binary artifact not rendered inline",
        "unspecified artifact payload not rendered inline",
    ] {
        assert!(
            renderer.contains(marker),
            "UI-13 renderer marker missing: {marker}"
        );
    }
    let wiring: String = workbench.chars().filter(|ch| !ch.is_whitespace()).collect();
    for marker in [
        "self.timeline.reset(run_id)",
        "self.timeline.apply_event(event.clone())",
        "fn display_messages(&self)",
        "item.kind.display_role()",
        "TimelineDisplayRole::Assistant=>ChatRole::Assistant",
        "TimelineDisplayRole::System=>ChatRole::System",
        "TimelineDisplayRole::Changed=>ChatRole::Changed",
    ] {
        assert!(
            wiring.contains(
                &marker
                    .chars()
                    .filter(|ch| !ch.is_whitespace())
                    .collect::<String>()
            ),
            "Workbench timeline wiring missing: {marker}"
        );
    }
    // The Workbench must not re-derive item kinds into display roles: the renderer is the single
    // authority, so a tool result cannot be re-labelled as assistant text at the entrypoint.
    assert!(
        !workbench.contains("TimelineItemKind::"),
        "Workbench must consume display_role() instead of matching item kinds itself"
    );

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
    // The projection is append-only: no API may clear, drop or rewrite an already recorded row.
    for forbidden in ["items.clear()", ".retain(", "items.remove(", ".drain("] {
        assert!(
            !renderer.contains(forbidden),
            "renderer gained a way to clear or rewrite recorded timeline facts: {forbidden}"
        );
    }
    assert!(!renderer.contains("#[ignore"));
    assert!(!workbench.contains("#[ignore"));
}

/// Completion slice: the declarative fixture, the behaviour fixtures, the manual lane, the baseline
/// document and the automatic shard must stay wired together.
#[test]
fn ui13_completion_slice_stays_wired_to_the_fixture_lane_and_shard() {
    let renderer = include_str!("../src/workbench_render.rs");
    let fixtures = include_str!("ui13_workbench_render.rs");
    let fixture_raw = include_str!("fixtures/ui13-render-snapshot.json");
    let baseline = include_str!("../../docs/roadmap/ui13-workbench-render-baseline.md");
    let card = include_str!("../../docs/roadmap/ui-entrypoints.md");
    let lane = include_str!("../../.github/workflows/ui13-workbench-render.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");

    // 1. the declared budgets match the source constants.
    let fixture: serde_json::Value =
        serde_json::from_str(fixture_raw).expect("the UI-13 fixture must stay valid JSON");
    assert_eq!(fixture["schema"], "kiana.ui13-render-snapshot.v1");
    assert_eq!(fixture["step"], "UI-13");
    for (budget, expected) in [
        ("max_items", MAX_TIMELINE_ITEMS as u64),
        ("max_bytes", MAX_TIMELINE_BYTES as u64),
        ("max_item_bytes", MAX_TIMELINE_ITEM_BYTES as u64),
        ("max_json_bytes", MAX_TIMELINE_JSON_BYTES as u64),
        ("max_diff_bytes", MAX_TIMELINE_DIFF_BYTES as u64),
        (
            "max_artifacts_per_response",
            MAX_TIMELINE_ARTIFACTS_PER_RESPONSE as u64,
        ),
    ] {
        assert_eq!(
            fixture["budgets"][budget].as_u64(),
            Some(expected),
            "UI-13 fixture budget {budget} drifted from the source constant"
        );
    }

    // 2. every declared fixture is a real behaviour test, and every scenario is exercised.
    let declared: Vec<&str> = fixture["fixtures"]
        .as_array()
        .expect("fixture list")
        .iter()
        .map(|value| value.as_str().expect("fixture name"))
        .collect();
    assert!(
        declared.len() >= 13,
        "the completion slice keeps every named fixture"
    );
    for name in &declared {
        assert!(
            fixtures.contains(&format!("fn {name}(")),
            "UI-13 fixture is not defined in kiana-entrypoints/tests/ui13_workbench_render.rs: {name}"
        );
    }
    for name in [
        "arrival_order_and_replays_never_overwrite_the_recorded_error",
        "tool_results_usage_approvals_and_errors_are_never_assistant_text",
        "binary_and_unspecified_artifact_payloads_are_never_inlined",
        "text_diff_artifacts_are_labelled_and_truncated_by_the_declared_policy",
        "terminal_fences_every_late_event_kind",
        "gap_marker_blocks_both_typed_and_legacy_delivery_until_reset",
        "hostile_stream_keeps_items_and_bytes_bounded",
    ] {
        assert!(
            declared.contains(&name),
            "the fixture must keep the completion-slice test: {name}"
        );
    }
    assert!(!fixtures.contains("#[ignore"));
    assert!(!fixture_raw.contains("--ignored"));
    let scenarios = fixture["scenarios"].as_array().expect("scenarios");
    assert!(scenarios.len() >= 5);
    for scenario in scenarios {
        let name = scenario["name"].as_str().expect("scenario name");
        assert!(
            fixtures.contains(name),
            "UI-13 fixture file does not exercise the declared scenario: {name}"
        );
    }

    // 3. required markers are documented in the baseline, denial codes stay in source, and the
    //    protocol-propagated codes are real.
    let required = fixture["required"].as_array().expect("required");
    assert!(!required.is_empty());
    for marker in required {
        let marker = marker.as_str().expect("required marker");
        assert!(
            baseline.contains(marker),
            "UI-13 baseline is missing the required marker: {marker}"
        );
    }
    let denied = fixture["denied"].as_array().expect("denied");
    assert!(!denied.is_empty());
    for code in denied {
        let code = code.as_str().expect("denial code");
        assert!(
            renderer.contains(code),
            "UI-13 renderer is missing the denial code or marker: {code}"
        );
    }
    let propagated = fixture["propagated_denials"]
        .as_array()
        .expect("propagated denials");
    assert!(!propagated.is_empty());
    for code in propagated {
        let code = code.as_str().expect("propagated code");
        assert!(
            protocol.contains(code),
            "the propagated cursor denial must exist in kiana-protocol: {code}"
        );
    }
    assert!(!fixture["limitations"]
        .as_array()
        .expect("limitations")
        .is_empty());

    // 4. the card and baseline keep the lane/test anchors.
    assert!(card.contains("UI-13"));
    for marker in [
        "ui13-render-snapshot.json",
        "ui13_workbench_render",
        "ui13_workbench_render_guard",
        "ui13-workbench-render.yml",
        "proof_level",
        "limitations",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-13 baseline is missing: {marker}"
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
            "UI-13 lane must not use {forbidden}"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-entrypoints --test ui13_workbench_render --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --test ui13_workbench_render_guard --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --no-run --locked",
    ] {
        assert!(lane.contains(command), "UI-13 lane is missing: {command}");
    }

    // 6. the automatic shard book keeps the whole-crate shard that discovers both targets.
    let shards: serde_json::Value =
        serde_json::from_str(shards_raw).expect("test shards must stay valid JSON");
    assert!(
        shards["kiana-entrypoints"]["targets"].is_null(),
        "kiana-entrypoints must stay a whole-crate shard so new UI-13 targets are discovered"
    );
}
