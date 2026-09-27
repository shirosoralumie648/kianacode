#[test]
fn swarm_retirement_keeps_unknown_release_and_fact_retention_explicit() {
    let domain = include_str!("../../kiana-domain/src/swarm_retirement.rs");
    let core = include_str!("../src/swarm_retirement.rs");
    for marker in [
        "SwarmRetirementFact",
        "residual_budget_digest",
        "path_lock_release_digest",
        "release_attempt",
        "BlockedUnknown",
        "facts_retained",
        "validate_swarm_retirement_fact",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-15 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "release_budget",
        "delete_facts",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-15 retirement widened effect boundary: {forbidden}"
        );
    }
}
