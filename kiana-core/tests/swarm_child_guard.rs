#[test]
fn swarm_child_materialization_stays_fresh_and_effect_free() {
    let domain = include_str!("../../kiana-domain/src/swarm_child.rs");
    let core = include_str!("../src/swarm_child.rs");
    for marker in [
        "SwarmChildMaterializationRequest",
        "SwarmChildMaterializationReceipt",
        "parent_session_id",
        "child_session_id",
        "authorized_input_refs",
        "parent_private_history_included",
        "child_scope_is_subset",
        "materialize_child",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-07 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "KianaHarness",
        "std::process::Command",
        "tokio::spawn",
        "EventStore",
        "parent_history",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-07 child boundary must not execute effects or copy private history: {forbidden}"
        );
    }
}
