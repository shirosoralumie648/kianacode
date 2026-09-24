use kiana_domain::*;
use std::collections::BTreeMap;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn snapshot() -> DataGovernanceSnapshot {
    DataGovernanceSnapshot::new(
        "/project",
        2,
        2,
        7,
        vec![EventId::new()],
        vec![DataRetentionObservation {
            source_ref: "src/input.txt".to_owned(),
            class: DataClass::Confidential,
            purpose_id: "run".to_owned(),
            payload: DataPayloadState::Revoked,
            audit_metadata_retained: true,
            source_digest: digest('a'),
        }],
        BTreeMap::from([("memory".to_owned(), DataPayloadState::Revoked)]),
        true,
    )
    .unwrap()
}

#[test]
fn receipt_metadata_and_payload_refs_are_separate_and_epoch_fenced() {
    let metadata = ReceiptAuditMetadata {
        project_ref: "/project".to_owned(),
        policy_revision: 1,
        data_epoch: 2,
        source_cursor: 3,
        source_event_ids: vec![EventId::new()],
        redaction_profile: digest('b'),
        retained: true,
    };
    let binding = ReceiptDataBinding::new(
        digest('c'),
        metadata,
        vec![ReceiptPayloadRef {
            object_ref: "artifact:one".to_owned(),
            source_digest: digest('d'),
            data_epoch: 2,
            payload_digest: digest('e'),
        }],
        DataPayloadState::Available,
    )
    .unwrap();
    assert!(binding.authorize_payload(2).is_ok());
    assert!(binding.authorize_payload(1).is_err());
    let redacted = binding.redact_payload_refs().unwrap();
    assert!(redacted.payload_refs.is_empty());
    assert!(redacted.authorize_payload(2).is_err());
    assert!(serde_json::to_value(&redacted)
        .unwrap()
        .get("metadata")
        .is_some());
}

#[test]
fn propagation_plan_preserves_event_facts_and_starts_unknown() {
    let plan =
        DataPropagationPlan::from_snapshot(&snapshot(), 1, "revoke", digest('f'), 100).unwrap();
    assert!(plan.immutable_event_seal);
    assert!(plan.targets.iter().any(|target| target.target
        == DataPropagationTarget::EventProjection
        && target.state == DataPropagationState::PreservedMetadata));
    assert!(plan
        .unresolved_targets()
        .contains(&DataPropagationTarget::Memory));
    assert!(plan.validate().is_ok());
}

#[test]
fn immutable_event_seal_rejects_duplicate_source_ids() {
    let event_id = EventId::new();
    let mut seal = ImmutableEventSeal::new(3, 2, vec![event_id], "sealed-audit").unwrap();
    seal.source_event_ids.push(event_id);
    assert!(seal.validate().is_err());
}
