use kiana_protocol::{CleanupCause, ResourceCleanupPlan, ResourceRetention, RunId};

#[test]
fn cleanup_contracts_are_wire_visible_and_bounded() {
    let retention = ResourceRetention::new(4, 8, 1024).unwrap();
    let encoded = serde_json::to_value(retention).unwrap();
    assert_eq!(encoded["schema"], "kiana.resource-retention.v1");
    let _ = CleanupCause::Shutdown;
    let plan = ResourceCleanupPlan::new(RunId::new(), CleanupCause::Shutdown, Vec::new()).unwrap();
    assert_eq!(
        serde_json::to_value(plan).unwrap()["schema"],
        "kiana.resource-cleanup-plan.v1"
    );
}
