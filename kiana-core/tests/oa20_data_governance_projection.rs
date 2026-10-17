//! OA-20 core fixtures: committed governance facts -> project-scoped snapshot -> per-store
//! deletion propagation settlement.
//!
//! Deny-first: cross-project or unattributed invalidation facts, cache-only settlement and
//! receipts that do not bind the live epoch are refused before any success path.

use kiana_core::{
    plan_deletion_propagation, project_data_governance_snapshot, receipt_data_binding_from_events,
    seal_governance_events,
};
use kiana_domain::{
    project_data_governance, DataClass, DataGovernanceSnapshot, DataPayloadState, DataPolicy,
    DataPropagationReceipt, DataPropagationState, DataPropagationTarget, DeleteRequest, EventId,
    ProcessingGrant, Purpose, RequestId, Retention, RetentionDecision, RetentionDisposition,
    RetentionScan, RuntimeEvent,
};
use serde_json::json;
use std::collections::BTreeSet;

fn hash(byte: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat(byte).take(64).collect::<String>()
    )
}

fn policy() -> DataPolicy {
    let mut policy = DataPolicy::default();
    policy
        .register(ProcessingGrant {
            id: "grant-1".to_owned(),
            source_path: "src/input.txt".to_owned(),
            content_hash: hash('a'),
            class: DataClass::Restricted,
            purpose: Purpose {
                id: "review".to_owned(),
                description: "security review".to_owned(),
            },
            retention: Retention {
                expires_at_ms: None,
                retain_audit_metadata: true,
            },
            parent_ids: Vec::new(),
            created_by: "principal:operator".to_owned(),
            revoked: false,
        })
        .unwrap();
    policy
}

fn event(sequence: u64, source_cursor: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    let mut event =
        RuntimeEvent::new(kiana_domain::RequestId::new(), sequence, kind, data).unwrap();
    event.data["source_cursor"] = json!(source_cursor);
    event
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

/// A retention scan/delete request pair bound to revision 7, epoch 3 and cursor 5.
fn deletion_scan(disposition: RetentionDisposition) -> RetentionScan {
    RetentionScan::new(
        "/repo",
        7,
        3,
        5,
        4,
        vec![EventId::new()],
        vec![RetentionDecision {
            object_ref: "src/input.txt".to_owned(),
            class: DataClass::Restricted,
            purpose_id: "delete".to_owned(),
            source_digest: hash('a'),
            payload: DataPayloadState::Expired,
            disposition,
            retain_until_ms: 10,
            hold_id: None,
        }],
        Vec::new(),
    )
    .unwrap()
}

fn deletion_request() -> DeleteRequest {
    DeleteRequest::new(
        RequestId::new(),
        "/repo",
        BTreeSet::from(["src/input.txt".to_owned()]),
        "delete",
        "data_subject_request",
        "principal:operator",
        7,
        3,
        5,
    )
    .unwrap()
}

/// The server-owned snapshot the deletion planning path must agree with.
fn deletion_snapshot() -> DataGovernanceSnapshot {
    let mut policy = policy();
    policy.revision = 7;
    policy.data_epoch = 3;
    policy.refresh_digest();
    project_data_governance(&policy, "/repo", 5, vec![EventId::new()], 1).unwrap()
}

#[test]
fn foreign_or_unattributed_project_scope_facts_are_refused() {
    // 别的项目的撤销事实不能被套用到当前项目。
    let foreign = vec![event(
        1,
        1,
        "data.revocation_requested",
        json!({"project_root":"/other","grant_id":"grant-1"}),
    )];
    assert_eq!(
        project_data_governance_snapshot(&policy(), "/repo", &foreign, 1).unwrap_err(),
        "data_governance_project_scope_mismatch"
    );
    // 缺少项目归属的撤销事实同样不能靠“调用方应该已经过滤过”来假定。
    let unattributed = vec![event(
        1,
        1,
        "data.revocation_requested",
        json!({"grant_id":"grant-1"}),
    )];
    assert_eq!(
        project_data_governance_snapshot(&policy(), "/repo", &unattributed, 1).unwrap_err(),
        "data_governance_project_root_missing"
    );
    // retention/revocation 结果里声明的项目根也必须与快照一致。
    let foreign_result = vec![event(
        1,
        1,
        "execution.result_committed",
        json!({"result":{"output":{
            "schema":"kiana.data-governance-result.v1",
            "project_root":"/other",
            "policy":{"revoked_sources":["src/input.txt"]}
        }}}),
    )];
    assert_eq!(
        project_data_governance_snapshot(&policy(), "/repo", &foreign_result, 1).unwrap_err(),
        "data_governance_project_scope_mismatch"
    );
}

#[test]
fn deletion_propagation_plan_keeps_every_store_open_until_settled() {
    let (deletion, plan) = plan_deletion_propagation(
        &deletion_request(),
        &deletion_scan(RetentionDisposition::Eligible),
        &deletion_snapshot(),
        &hash('t'),
        11,
    )
    .unwrap();
    assert_eq!(plan.kind, "delete");
    assert_eq!(plan.previous_epoch, 3);
    assert_eq!(plan.data_epoch, deletion.next_data_epoch);
    assert_eq!(plan.unresolved_payload_targets(), PAYLOAD_TARGETS.to_vec());
    assert_eq!(
        plan.require_payload_coverage().unwrap_err(),
        "data_propagation_payload_coverage_incomplete"
    );
    // 只清理 cache 副本不能把删除标记成完成。
    let mut plan = plan;
    let cache = DataPropagationReceipt::settled(
        DataPropagationTarget::Cache,
        "/repo",
        plan.previous_epoch,
        plan.data_epoch,
        plan.source_cursor,
        hash('t'),
        DataPropagationState::Invalidated,
        hash('c'),
        11,
    )
    .unwrap();
    plan.settle(&cache).unwrap();
    assert_eq!(
        plan.require_payload_coverage().unwrap_err(),
        "data_propagation_payload_coverage_incomplete"
    );
    // 每个载荷 store 都有自己的已绑定回执后，删除才算传播完成。
    for target in PAYLOAD_TARGETS {
        let receipt = DataPropagationReceipt::settled(
            target,
            "/repo",
            plan.previous_epoch,
            plan.data_epoch,
            plan.source_cursor,
            hash('t'),
            DataPropagationState::Invalidated,
            hash('c'),
            11,
        )
        .unwrap();
        plan.settle(&receipt).unwrap();
    }
    plan.require_payload_coverage().unwrap();
    plan.require_complete().unwrap();
}

#[test]
fn receipt_binding_separates_payload_refs_from_audit_metadata_and_epoch() {
    let mut source = event(1, 1, "run.completed", json!({"run_id":"run-1"}));
    source.artifact_refs.push("artifact:oa20-report".to_owned());
    let binding =
        receipt_data_binding_from_events(&hash('r'), "/repo", &policy(), &[source]).unwrap();
    assert!(binding.metadata.retained);
    assert_eq!(binding.payload_refs.len(), 1);
    // 有引用不等于有读取许可：没有新鲜治理快照时载荷保持 Unknown。
    assert_eq!(binding.payload_state, DataPayloadState::Unknown);
    assert_eq!(
        binding
            .authorize_payload(binding.metadata.data_epoch)
            .unwrap_err(),
        "receipt_payload_authorization_denied"
    );
    let mut authorized = binding.clone();
    authorized.payload_state = DataPayloadState::Available;
    authorized.binding_digest = authorized.digest();
    authorized.validate().unwrap();
    assert!(authorized
        .authorize_payload(binding.metadata.data_epoch)
        .is_ok());
    // epoch 漂移后即使是 Available 也拒绝读取。
    assert_eq!(
        authorized
            .authorize_payload(binding.metadata.data_epoch + 1)
            .unwrap_err(),
        "receipt_payload_authorization_denied"
    );
    let encoded = serde_json::to_string(&authorized).unwrap();
    assert!(!encoded.contains("secret-sentinel"));
}

#[test]
fn rebuild_from_the_same_committed_facts_is_stable_across_restarts() {
    let events = vec![event(
        1,
        1,
        "execution.result_committed",
        json!({
            "result": {
                "output": {
                    "schema":"kiana.data-governance-result.v1",
                    "project_root":"/repo",
                    "policy":{"revoked_sources":["src/input.txt"]}
                }
            }
        }),
    )];
    let first = project_data_governance_snapshot(&policy(), "/repo", &events, 1).unwrap();
    // 重启后由同一份已提交事实重建：摘要、游标与序列化结果都一致。
    let rebuilt = project_data_governance_snapshot(&policy(), "/repo", &events, 1).unwrap();
    assert_eq!(first.digest(), rebuilt.digest());
    assert_eq!(first.snapshot_digest, rebuilt.snapshot_digest);
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&rebuilt).unwrap()
    );
    assert_eq!(first.source_event_ids, vec![events[0].event_id]);
    let seal = seal_governance_events(&events, first.data_epoch, "sealed-audit").unwrap();
    let rebuilt_seal = seal_governance_events(&events, first.data_epoch, "sealed-audit").unwrap();
    assert_eq!(seal.seal_digest, rebuilt_seal.seal_digest);
    let binding =
        receipt_data_binding_from_events(&hash('r'), "/repo", &policy(), &events).unwrap();
    assert_eq!(binding.payload_state, DataPayloadState::Unknown);
}

#[test]
fn pending_revocation_fences_payload_and_all_derived_store_projections() {
    let events = vec![event(
        1,
        1,
        "data.revocation_requested",
        json!({"project_root":"/repo","grant_id":"grant-1"}),
    )];
    let snapshot = project_data_governance_snapshot(&policy(), "/repo", &events, 1).unwrap();
    assert!(snapshot
        .observations
        .iter()
        .all(|observation| observation.payload == DataPayloadState::Unknown));
    assert!(snapshot
        .derived_store_states
        .values()
        .all(|state| *state == DataPayloadState::Unknown));
    snapshot.validate().unwrap();
}

#[test]
fn committed_revocation_propagates_revoke_without_erasing_source_event_ids() {
    let events = vec![event(
        1,
        1,
        "execution.result_committed",
        json!({
            "result": {
                "output": {
                    "schema":"kiana.data-governance-result.v1",
                    "project_root":"/repo",
                    "policy":{"revoked_sources":["src/input.txt"]}
                }
            }
        }),
    )];
    let event_id = events[0].event_id;
    let snapshot = project_data_governance_snapshot(&policy(), "/repo", &events, 1).unwrap();
    assert_eq!(snapshot.source_event_ids, vec![event_id]);
    assert_eq!(snapshot.observations[0].payload, DataPayloadState::Revoked);
    assert!(snapshot
        .derived_store_states
        .values()
        .all(|state| *state == DataPayloadState::Revoked));
}

#[test]
fn cursor_gap_and_duplicate_source_fail_closed() {
    let gap = vec![
        event(
            1,
            1,
            "data.revocation_requested",
            json!({"project_root":"/repo"}),
        ),
        event(2, 3, "workspace.restored", json!({"project_root":"/repo"})),
    ];
    assert_eq!(
        project_data_governance_snapshot(&policy(), "/repo", &gap, 1).unwrap_err(),
        "data_governance_source_cursor_gap"
    );
    let duplicate = event(
        1,
        1,
        "data.revocation_requested",
        json!({"project_root":"/repo"}),
    );
    let duplicate_id = duplicate.clone();
    assert_eq!(
        project_data_governance_snapshot(&policy(), "/repo", &[duplicate, duplicate_id], 1)
            .unwrap_err(),
        "data_governance_source_event_duplicate"
    );
}

#[test]
fn governance_snapshot_does_not_expose_raw_payload_content() {
    let events = vec![event(
        1,
        1,
        "data.revocation_requested",
        json!({"project_root":"/repo","payload":"raw-secret"}),
    )];
    let snapshot = project_data_governance_snapshot(&policy(), "/repo", &events, 1).unwrap();
    let encoded_value = serde_json::to_value(&snapshot).unwrap();
    let encoded = encoded_value.to_string();
    assert!(!encoded.contains("raw-secret"));
    assert_eq!(
        encoded_value["observations"][0]["payload"],
        json!("unknown")
    );
    assert!(encoded_value["observations"][0]
        .as_object()
        .unwrap()
        .get("raw_payload")
        .is_none());
    assert_eq!(snapshot.source_event_ids, vec![events[0].event_id]);
}
