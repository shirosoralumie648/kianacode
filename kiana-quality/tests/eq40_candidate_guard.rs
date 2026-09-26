#[test]
fn candidate_binding_stays_single_dimension_and_pure() {
    let source = include_str!("../src/candidate.rs");
    for forbidden in [
        "std::fs",
        "tokio::",
        "reqwest::",
        "Provider",
        "EventStore",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "Command::new",
        "promote",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden candidate evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "CANDIDATE_INPUT_SCHEMA",
        "QualityCandidate",
        "changed_dimension",
        "passive_version_refs",
        "snapshot_versions",
        "candidate_passive_version_drift",
        "candidate_snapshot_digest_mismatch",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-40 boundary marker: {required}"
        );
    }
}
