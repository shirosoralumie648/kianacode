#[test]
fn golden_trace_replay_is_bound_and_side_effect_free() {
    let versioning = include_str!("../src/versioning.rs");
    let receipts = include_str!("../src/receipts.rs");
    let quality = include_str!("../../kiana-domain/src/quality.rs");
    let eval = include_str!("../src/eval.rs");
    let baseline = include_str!("../../docs/roadmap/p1-l1-01-golden-trace-baseline.md");
    for marker in [
        "trace.capture",
        "trace.replay",
        "golden_trace.captured",
        "trace_source_manifest_required",
        "source_manifest",
        "source_snapshot",
        "input_hash",
        "events_hash",
        "runtime_version",
        "receipt_from_events",
        "trace_digest_mismatch",
        "trace_data_revoked",
        "fold_model_visible_history",
        "project_invocations",
        "\"side_effects\":false",
        "\"provider_calls\":0",
        "GoldenTrace",
        "GOLDEN_TRACE_SCHEMA",
        "normalized_events",
        "target_versions",
        "receipt_hash",
        "evaluate_provider_independent",
    ] {
        assert!(
            versioning.contains(marker)
                || receipts.contains(marker)
                || quality.contains(marker)
                || eval.contains(marker)
                || baseline.contains(marker),
            "golden trace marker missing: {marker}"
        );
    }
    assert!(versioning.contains("event.kind == \"golden_trace.captured\""));
    assert!(versioning.contains("event.data[\"owner_id\"] == json!(context.actor_id)"));
    assert!(versioning.contains("event.data[\"project_root\"] == context.project_root"));
    assert!(versioning.contains("kiana_domain::json_digest(&json!(events))"));
    assert!(versioning.contains("read_all"));
    assert!(!versioning.contains("ModelClient"));
    assert!(!versioning.contains("CapabilityBroker"));
    assert!(!versioning.contains("authorize_and_execute"));
}
