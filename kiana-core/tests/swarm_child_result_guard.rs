#[test]
fn child_result_is_typed_and_review_bound() {
    let domain = include_str!("../../kiana-domain/src/swarm_child_result.rs");
    let core = include_str!("../src/swarm_child_result.rs");
    for marker in [
        "TypedChildResult",
        "Succeeded",
        "Failed",
        "ResultUnknown",
        "independent_review_required",
        "artifact_refs",
        "validate_child_result",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-12 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore",
        "merge_result",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-12 child result boundary must not execute/merge effects: {forbidden}"
        );
    }
}
