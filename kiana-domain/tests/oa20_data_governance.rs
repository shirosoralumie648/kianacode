//! OA-20 domain fixtures: DataClass/Purpose/Retention/Deletion propagation.
//!
//! Ordering is deny-first: a cleanup that only touches disposable copies, a receipt that does not
//! bind the plan boundary, raw payload bytes offered to a governance DTO and audit metadata used
//! as a payload read permit are all refused before the success path settles every payload store.

use kiana_domain::{
    project_data_governance, DataClass, DataGovernanceSnapshot, DataPayloadState, DataPolicy,
    DataPropagationPlan, DataPropagationReceipt, DataPropagationState, DataPropagationTarget,
    EventId, ProcessingGrant, Purpose, ReceiptAuditMetadata, ReceiptDataBinding, ReceiptPayloadRef,
    Retention,
};

fn hash(byte: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat(byte).take(64).collect::<String>()
    )
}

fn grant(
    id: &str,
    path: &str,
    expires_at_ms: Option<u64>,
    parent_ids: Vec<String>,
) -> ProcessingGrant {
    ProcessingGrant {
        id: id.to_owned(),
        source_path: path.to_owned(),
        content_hash: hash('a'),
        class: DataClass::Confidential,
        purpose: Purpose {
            id: "audit".to_owned(),
            description: "security review".to_owned(),
        },
        retention: Retention {
            expires_at_ms,
            retain_audit_metadata: true,
        },
        parent_ids,
        created_by: "principal:operator".to_owned(),
        revoked: false,
    }
}

fn payload_ref(data_epoch: u64) -> ReceiptPayloadRef {
    ReceiptPayloadRef {
        object_ref: "artifact:receipt-1".to_owned(),
        source_digest: hash('a'),
        data_epoch,
        payload_digest: hash('d'),
    }
}

/// One revoked and one still-active grant: revocation advances data epoch and revision, so a
/// propagation plan (which requires `data_epoch > previous_epoch`) can be derived from it.
fn revoked_policy() -> DataPolicy {
    let mut policy = DataPolicy::default();
    policy
        .register(grant("parent", "src/a.txt", None, Vec::new()))
        .unwrap();
    policy
        .register(grant("active", "src/b.txt", None, Vec::new()))
        .unwrap();
    policy.revoke("parent").unwrap();
    policy
}

fn projected() -> DataGovernanceSnapshot {
    let policy = revoked_policy();
    project_data_governance(&policy, "project:one", 4, vec![EventId::new()], 1).unwrap()
}

fn propagation_plan(snapshot: &DataGovernanceSnapshot) -> DataPropagationPlan {
    DataPropagationPlan::from_snapshot(snapshot, 1, "delete", hash('b'), 5).unwrap()
}

fn settled_receipt(
    target: DataPropagationTarget,
    snapshot: &DataGovernanceSnapshot,
    state: DataPropagationState,
) -> DataPropagationReceipt {
    DataPropagationReceipt::settled(
        target,
        "project:one",
        1,
        snapshot.data_epoch,
        snapshot.source_cursor,
        hash('b'),
        state,
        hash('c'),
        5,
    )
    .unwrap()
}

const PAYLOAD_TARGETS: [DataPropagationTarget; 7] = [
    DataPropagationTarget::Receipt,
    DataPropagationTarget::Artifact,
    DataPropagationTarget::Memory,
    DataPropagationTarget::Index,
    DataPropagationTarget::Cache,
    DataPropagationTarget::Checkpoint,
    DataPropagationTarget::Export,
];

#[test]
fn cache_only_cleanup_cannot_claim_payload_coverage() {
    let snapshot = projected();
    let mut plan = propagation_plan(&snapshot);
    // 建立计划不等于完成治理：所有载荷 store 仍然只有 Unknown。
    assert_eq!(plan.unresolved_payload_targets(), PAYLOAD_TARGETS.to_vec());
    assert_eq!(
        plan.require_payload_coverage().unwrap_err(),
        "data_propagation_payload_coverage_incomplete"
    );
    assert_eq!(
        plan.require_complete().unwrap_err(),
        "data_propagation_targets_unresolved"
    );
    // 只清理可丢弃的 cache 副本不能让一次删除“完成”。
    let cache = settled_receipt(
        DataPropagationTarget::Cache,
        &snapshot,
        DataPropagationState::Invalidated,
    );
    plan.settle(&cache).unwrap();
    assert!(!plan
        .unresolved_payload_targets()
        .contains(&DataPropagationTarget::Cache));
    assert_eq!(
        plan.require_payload_coverage().unwrap_err(),
        "data_propagation_payload_coverage_incomplete"
    );
    assert_eq!(
        plan.require_complete().unwrap_err(),
        "data_propagation_targets_unresolved"
    );
}

#[test]
fn receipt_binding_drift_and_unset_receipts_are_refused_before_any_slot_changes() {
    let snapshot = projected();
    let mut plan = propagation_plan(&snapshot);
    let before = plan.plan_digest.clone();
    for forged in [
        DataPropagationReceipt::settled(
            DataPropagationTarget::Artifact,
            "project:two",
            1,
            snapshot.data_epoch,
            snapshot.source_cursor,
            hash('b'),
            DataPropagationState::Invalidated,
            hash('c'),
            5,
        )
        .unwrap(),
        DataPropagationReceipt::settled(
            DataPropagationTarget::Artifact,
            "project:one",
            1,
            snapshot.data_epoch + 1,
            snapshot.source_cursor,
            hash('b'),
            DataPropagationState::Invalidated,
            hash('c'),
            5,
        )
        .unwrap(),
        DataPropagationReceipt::settled(
            DataPropagationTarget::Artifact,
            "project:one",
            1,
            snapshot.data_epoch,
            snapshot.source_cursor + 1,
            hash('b'),
            DataPropagationState::Invalidated,
            hash('c'),
            5,
        )
        .unwrap(),
        DataPropagationReceipt::settled(
            DataPropagationTarget::Artifact,
            "project:one",
            1,
            snapshot.data_epoch,
            snapshot.source_cursor,
            hash('e'),
            DataPropagationState::Invalidated,
            hash('c'),
            5,
        )
        .unwrap(),
    ] {
        assert_eq!(
            plan.settle(&forged).unwrap_err(),
            "data_propagation_receipt_binding_mismatch"
        );
    }
    let unset = DataPropagationReceipt::unknown(
        DataPropagationTarget::Artifact,
        "project:one",
        1,
        snapshot.data_epoch,
        snapshot.source_cursor,
        hash('b'),
        5,
    );
    assert_eq!(
        plan.settle(&unset).unwrap_err(),
        "data_propagation_receipt_not_settled"
    );
    // `settled()` 不能把 Unknown 伪装成已完成的回执。
    assert_eq!(
        DataPropagationReceipt::settled(
            DataPropagationTarget::Artifact,
            "project:one",
            1,
            snapshot.data_epoch,
            snapshot.source_cursor,
            hash('b'),
            DataPropagationState::Unknown,
            hash('c'),
            5,
        )
        .unwrap_err(),
        "data_propagation_receipt_invalid"
    );
    // 被拒绝的回执没有改动任何槽位或计划摘要。
    assert_eq!(plan.plan_digest, before);
    let artifact = plan
        .targets
        .iter()
        .find(|slot| slot.target == DataPropagationTarget::Artifact)
        .unwrap();
    assert_eq!(artifact.state, DataPropagationState::Unknown);
}

#[test]
fn raw_payload_and_spool_bytes_cannot_be_decoded_into_governance_dtos() {
    let snapshot = projected();
    let plan = propagation_plan(&snapshot);
    let receipt = settled_receipt(
        DataPropagationTarget::Artifact,
        &snapshot,
        DataPropagationState::Invalidated,
    );
    let mut forged_receipt = serde_json::to_value(&receipt).unwrap();
    forged_receipt["payload"] = serde_json::json!("secret-sentinel");
    assert!(serde_json::from_value::<DataPropagationReceipt>(forged_receipt).is_err());
    let mut forged_snapshot = serde_json::to_value(&snapshot).unwrap();
    forged_snapshot["raw_spool"] = serde_json::json!("secret-sentinel");
    assert!(serde_json::from_value::<DataGovernanceSnapshot>(forged_snapshot).is_err());
    let mut forged_ref = serde_json::to_value(payload_ref(snapshot.data_epoch)).unwrap();
    forged_ref["body"] = serde_json::json!("secret-sentinel");
    assert!(serde_json::from_value::<ReceiptPayloadRef>(forged_ref).is_err());
    // 快照/计划/回执只携带引用与摘要，不携带任何原始字节。
    for encoded in [
        serde_json::to_string(&snapshot).unwrap(),
        serde_json::to_string(&plan).unwrap(),
        serde_json::to_string(&receipt).unwrap(),
    ] {
        assert!(!encoded.contains("secret-sentinel"));
    }
    // 传播计划与回执连来源路径都不携带：只保留项目、epoch、游标和摘要绑定。
    for encoded in [
        serde_json::to_string(&plan).unwrap(),
        serde_json::to_string(&receipt).unwrap(),
    ] {
        assert!(!encoded.contains("src/"));
    }
}

#[test]
fn retained_audit_metadata_never_authorizes_payload_reads() {
    let snapshot = projected();
    let metadata = ReceiptAuditMetadata {
        project_ref: "project:one".to_owned(),
        policy_revision: snapshot.policy_revision,
        data_epoch: snapshot.data_epoch,
        source_cursor: snapshot.source_cursor,
        source_event_ids: snapshot.source_event_ids.clone(),
        redaction_profile: "kiana.redaction-profile.v1:audit".to_owned(),
        retained: true,
    };
    let rejected = ReceiptDataBinding::new(
        hash('c'),
        metadata.clone(),
        vec![payload_ref(snapshot.data_epoch)],
        DataPayloadState::Revoked,
    )
    .unwrap();
    assert!(rejected.metadata.retained);
    assert_eq!(
        rejected.authorize_payload(snapshot.data_epoch).unwrap_err(),
        "receipt_payload_authorization_denied"
    );
    // 脱敏只移除引用，不会把审计元数据变成读取许可。
    assert_eq!(
        rejected
            .redact_payload_refs()
            .unwrap()
            .authorize_payload(snapshot.data_epoch)
            .unwrap_err(),
        "receipt_payload_authorization_denied"
    );
    let available = ReceiptDataBinding::new(
        hash('c'),
        metadata,
        vec![payload_ref(snapshot.data_epoch)],
        DataPayloadState::Available,
    )
    .unwrap();
    assert!(available.authorize_payload(snapshot.data_epoch).is_ok());
    // 数据 epoch 漂移后即使是 Available 也拒绝。
    assert_eq!(
        available
            .authorize_payload(snapshot.data_epoch + 1)
            .unwrap_err(),
        "receipt_payload_authorization_denied"
    );
    // 快照不能把“审计元数据保留”和“payload 仍可用”混为一谈。
    let mut tampered = snapshot.clone();
    tampered.audit_metadata_retained = false;
    tampered.snapshot_digest = tampered.digest();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "data_audit_metadata_propagation_conflict"
    );
}

#[test]
fn expired_payload_keeps_metadata_separate_and_leaves_payload_targets_open() {
    let mut policy = DataPolicy::default();
    policy
        .register(grant("expired", "src/expired.txt", Some(10), Vec::new()))
        .unwrap();
    policy.data_epoch = 2;
    policy.refresh_digest();
    let snapshot =
        project_data_governance(&policy, "project:one", 4, vec![EventId::new()], 10).unwrap();
    assert_eq!(snapshot.observations[0].payload, DataPayloadState::Expired);
    assert!(snapshot.audit_metadata_retained);
    let plan = DataPropagationPlan::from_snapshot(&snapshot, 1, "expire", hash('b'), 11).unwrap();
    let audit = plan
        .targets
        .iter()
        .find(|slot| slot.target == DataPropagationTarget::Audit)
        .unwrap();
    assert_eq!(audit.state, DataPropagationState::PreservedMetadata);
    // 到期只改变 payload 的可用性判断；各载荷 store 仍然没有回执。
    assert_eq!(plan.unresolved_payload_targets().len(), 7);
    assert_eq!(
        plan.require_payload_coverage().unwrap_err(),
        "data_propagation_payload_coverage_incomplete"
    );
}

#[test]
fn settled_payload_targets_are_complete_and_digest_stable() {
    let snapshot = projected();
    let mut plan = propagation_plan(&snapshot);
    for target in PAYLOAD_TARGETS {
        plan.settle(&settled_receipt(
            target,
            &snapshot,
            DataPropagationState::Invalidated,
        ))
        .unwrap();
    }
    plan.require_payload_coverage().unwrap();
    plan.require_complete().unwrap();
    assert!(plan.unresolved_targets().is_empty());
    // 源事实只被封存、审计元数据只被保留：两者都不会被写成“payload 已删除”。
    for target in [
        DataPropagationTarget::EventProjection,
        DataPropagationTarget::Audit,
    ] {
        let slot = plan
            .targets
            .iter()
            .find(|slot| slot.target == target)
            .unwrap();
        assert_eq!(slot.state, DataPropagationState::PreservedMetadata);
    }
    assert!(plan.immutable_event_seal);
    // 同一份计划序列化再解码后仍然满足同一覆盖约束，摘要保持不变（重启后可重建同一事实）。
    let encoded = serde_json::to_string(&plan).unwrap();
    let rebuilt: DataPropagationPlan = serde_json::from_str(&encoded).unwrap();
    assert_eq!(rebuilt, plan);
    assert_eq!(rebuilt.plan_digest, plan.plan_digest);
    rebuilt.require_payload_coverage().unwrap();
    rebuilt.require_complete().unwrap();
}

#[test]
fn policy_epoch_and_digest_change_on_revocation_with_parent_cascade() {
    let mut policy = DataPolicy::default();
    policy
        .register(grant("parent", "src/a.txt", None, Vec::new()))
        .unwrap();
    policy
        .register(grant("child", "src/b.txt", None, vec!["parent".to_owned()]))
        .unwrap();
    policy.validate().unwrap();
    let before_epoch = policy.data_epoch;
    let before_digest = policy.digest();
    let affected = policy.revoke("parent").unwrap();
    assert_eq!(affected, vec!["child".to_owned(), "parent".to_owned()]);
    assert!(policy.data_epoch > before_epoch);
    assert_ne!(policy.policy_digest, before_digest);
    policy.validate().unwrap();
    let encoded = serde_json::to_string(&policy).unwrap();
    assert_eq!(
        serde_json::from_str::<DataPolicy>(&encoded).unwrap(),
        policy
    );
}

#[test]
fn expired_payload_is_separate_from_retained_audit_metadata_and_propagates() {
    let mut policy = DataPolicy::default();
    policy
        .register(grant("expired", "src/expired.txt", Some(10), Vec::new()))
        .unwrap();
    let snapshot =
        project_data_governance(&policy, "project:one", 4, vec![EventId::new()], 10).unwrap();
    snapshot.validate().unwrap();
    assert_eq!(snapshot.observations[0].payload, DataPayloadState::Expired);
    assert!(snapshot.observations[0].audit_metadata_retained);
    assert!(snapshot.audit_metadata_retained);
    assert!(snapshot
        .derived_store_states
        .values()
        .all(|state| *state == DataPayloadState::Expired));
}

#[test]
fn tampered_policy_or_snapshot_digest_is_rejected() {
    let mut policy = DataPolicy::default();
    policy
        .register(grant("active", "src/active.txt", None, Vec::new()))
        .unwrap();
    let mut forged = policy.clone();
    forged.data_epoch += 1;
    assert_eq!(
        forged.validate().unwrap_err(),
        "data_policy_digest_mismatch"
    );
    let snapshot =
        project_data_governance(&policy, "project:one", 1, vec![EventId::new()], 0).unwrap();
    let mut forged_snapshot = snapshot.clone();
    forged_snapshot.audit_metadata_retained = false;
    assert_eq!(
        forged_snapshot.validate().unwrap_err(),
        "data_governance_snapshot_digest_mismatch"
    );
}
