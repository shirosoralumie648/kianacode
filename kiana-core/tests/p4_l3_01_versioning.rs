#[test]
fn drift_report_is_bucketed_by_version() {
    let core = include_str!("../src/versioning.rs");
    let domain = include_str!("../../kiana-domain/src/versioning.rs");
    let prompts = include_str!("../../kiana-domain/src/prompts.rs");
    let provider = include_str!("../../kiana-services/src/api/provider.rs");
    let baseline = include_str!("../../docs/roadmap/p4-l3-01-versioning-baseline.md");

    for marker in [
        "version.drift",
        "RouteDecision",
        "DriftBucket",
        "DriftReport",
        "ROUTE_DECISION_SCHEMA",
        "DRIFT_REPORT_SCHEMA",
        "ModelProfile",
        "PromptBundle",
        "model_profile",
        "prompt_hash",
        "route_digest",
        "configuration_revision",
        "budget_schema",
        "runtime_version",
        "BTreeMap",
        "version_key",
        "json_digest",
        "run.model_turn",
        "event_ids",
        "automatic_model_switch",
        "observed_turns",
        "cost",
        "drift_report_is_bucketed_by_version",
    ] {
        assert!(
            core.contains(marker)
                || domain.contains(marker)
                || prompts.contains(marker)
                || provider.contains(marker)
                || baseline.contains(marker),
            "versioning marker missing: {marker}"
        );
    }

    assert!(core.contains("kiana_domain::DriftReport::default()"));
    assert!(core.contains("kiana_domain::RouteDecision::from_model_turn"));
    assert!(core.contains("report.record"));
    assert!(core.contains("report.validate"));
    assert!(!core.contains("ModelClient"));
    assert!(!core.contains("CapabilityBroker"));
    assert!(!core.contains("\"automatic_model_switch\":true"));
    assert!(!core.contains("switch_model"));
}
