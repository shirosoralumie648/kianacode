use kiana_domain::{DataPayloadState, DataPropagationPlan, DataPropagationTarget};
use kiana_query::{governed_index_invalidation, index_read_allowed};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

#[test]
fn revoked_or_stale_index_is_not_an_empty_success() {
    assert!(index_read_allowed(1, 2, DataPayloadState::Available).is_err());
    assert!(index_read_allowed(2, 2, DataPayloadState::Revoked).is_err());
    assert!(index_read_allowed(2, 2, DataPayloadState::Available).is_ok());
}

#[test]
fn governance_plan_keeps_index_unknown_until_rebuild_receipt() {
    let snapshot = kiana_domain::DataGovernanceSnapshot::new(
        "/project",
        1,
        2,
        3,
        vec![kiana_domain::EventId::new()],
        Vec::new(),
        std::collections::BTreeMap::new(),
        false,
    )
    .unwrap();
    let plan = DataPropagationPlan::from_snapshot(&snapshot, 1, "expire", digest('a'), 4).unwrap();
    assert!(governed_index_invalidation(&plan, 2).unwrap());
    assert!(plan
        .targets
        .iter()
        .any(|target| target.target == DataPropagationTarget::Index));
}
