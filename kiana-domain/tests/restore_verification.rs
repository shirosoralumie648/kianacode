//! PD-23 恢复事实的 Domain 夹具，先拒绝、后成功。
//!
//! 这里只验证值合同：结构、摘要、状态与证据的关系，以及 artifact 引用与 `SnapshotManifest`
//! 文件哈希的绑定。不读磁盘、不取锁、不激活 root，也不证明外部 effect 真实发生。

use kiana_domain::{
    bind_restore_artifact_refs, restore_artifact_ref, EffectObservationState,
    RestoreEffectResolution, RestoreExternalEffectClaim, RestoreVerificationFact,
    RestoreVerificationState, SnapshotFileSeal, SnapshotManifest, SnapshotMode,
    RESTORE_VERIFICATION_SCHEMA,
};
use serde_json::json;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FILE_A: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const FILE_B: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn digest(fact: &RestoreVerificationFact) -> String {
    kiana_domain::json_digest(&json!({
        "schema": fact.schema,
        "snapshot_manifest_digest": fact.snapshot_manifest_digest,
        "owner_scope": fact.owner_scope,
        "store_id": fact.store_id,
        "previous_instance_id": fact.previous_instance_id,
        "restored_instance_id": fact.restored_instance_id,
        "source_cursor": fact.source_cursor,
        "projection_cursor": fact.projection_cursor,
        "projection_digest": fact.projection_digest,
        "data_epoch": fact.data_epoch,
        "previous_data_epoch": fact.previous_data_epoch,
        "authority_epoch": fact.authority_epoch,
        "previous_authority_epoch": fact.previous_authority_epoch,
        "artifact_refs": fact.artifact_refs,
        "pending_approval_count": fact.pending_approval_count,
        "active_lease_count": fact.active_lease_count,
        "trigger_count": fact.trigger_count,
        "unknown_effect_count": fact.unknown_effect_count,
        "old_instance_fenced": fact.old_instance_fenced,
        "old_authority_fenced": fact.old_authority_fenced,
        "external_effect_claims": fact.external_effect_claims,
        "default_paused": fact.default_paused,
        "reconciliation_refs": fact.reconciliation_refs,
        "state": fact.state,
        "reason": fact.reason,
    }))
}

fn manifest() -> SnapshotManifest {
    SnapshotManifest::new(
        "snapshot:one",
        "project:/repo",
        "store:one",
        "instance:old",
        "/repo/.kiana",
        "/backup/repo",
        SnapshotMode::Full,
        12,
        1,
        None,
        true,
        true,
        vec![
            SnapshotFileSeal {
                relative_path: "events.jsonl".to_owned(),
                size_bytes: 10,
                content_hash: FILE_A.to_owned(),
            },
            SnapshotFileSeal {
                relative_path: "acl/roles.json".to_owned(),
                size_bytes: 20,
                content_hash: FILE_B.to_owned(),
            },
        ],
    )
    .expect("manifest")
}

fn fact(state: RestoreVerificationState) -> RestoreVerificationFact {
    let mut fact = RestoreVerificationFact {
        schema: RESTORE_VERIFICATION_SCHEMA.to_owned(),
        snapshot_manifest_digest: D1.to_owned(),
        owner_scope: "project:/repo".to_owned(),
        store_id: "store:one".to_owned(),
        previous_instance_id: Some("instance:old".to_owned()),
        restored_instance_id: "instance:new".to_owned(),
        source_cursor: 12,
        projection_cursor: 12,
        projection_digest: D2.to_owned(),
        data_epoch: 2,
        previous_data_epoch: Some(1),
        authority_epoch: 4,
        previous_authority_epoch: Some(3),
        artifact_refs: vec!["artifact:manifest".to_owned(), "artifact:acl".to_owned()],
        pending_approval_count: u32::from(matches!(
            state,
            RestoreVerificationState::NeedsReconciliation
        )),
        active_lease_count: 0,
        trigger_count: 0,
        unknown_effect_count: u32::from(matches!(
            state,
            RestoreVerificationState::NeedsReconciliation
        )),
        old_instance_fenced: true,
        old_authority_fenced: true,
        external_effect_claims: Vec::new(),
        default_paused: true,
        reconciliation_refs: if matches!(state, RestoreVerificationState::NeedsReconciliation) {
            vec!["reconcile:restore-1".to_owned()]
        } else {
            Vec::new()
        },
        state,
        reason: if matches!(state, RestoreVerificationState::Verified) {
            "ok".to_owned()
        } else {
            "pending_reconciliation".to_owned()
        },
        verification_digest: String::new(),
    };
    fact.verification_digest = digest(&fact);
    fact
}

// ---------------------------------------------------------------------------
// 拒绝面优先：未知 effect、缺失/伪造 artifact、未就绪状态。
// ---------------------------------------------------------------------------

#[test]
fn unknown_effect_cannot_be_sealed_as_reconciled_evidence() {
    // 第一道拒绝：Unknown 不是任何一种处置。即使摘要自洽也必须拒绝。
    for resolution in [
        RestoreEffectResolution::Reconciled,
        RestoreEffectResolution::RetryWithoutEffect,
        RestoreEffectResolution::Abandoned,
        RestoreEffectResolution::Compensated,
    ] {
        assert_eq!(
            RestoreExternalEffectClaim::new(
                D1,
                EffectObservationState::Unknown,
                resolution,
                D2,
                "approval-1",
                4,
            )
            .unwrap_err(),
            "restore_effect_claim_unknown",
            "resolution {resolution:?}"
        );
    }

    // 处置与观察状态不相容同样拒绝：没有 effect 无从补偿；未证明无 effect 不得放弃/重发。
    for (state, resolution) in [
        (
            EffectObservationState::NoEffect,
            RestoreEffectResolution::Compensated,
        ),
        (
            EffectObservationState::ConfirmedFailure,
            RestoreEffectResolution::Abandoned,
        ),
        (
            EffectObservationState::ConfirmedFailure,
            RestoreEffectResolution::RetryWithoutEffect,
        ),
    ] {
        assert_eq!(
            RestoreExternalEffectClaim::new(D1, state, resolution, D2, "approval-1", 4)
                .unwrap_err(),
            "restore_effect_claim_resolution_state_mismatch",
            "state {state:?} resolution {resolution:?}"
        );
    }

    // 手工伪造的 claim（绕过构造器、摘要被重算）仍必须被 fact 拒绝。
    let mut forged = RestoreExternalEffectClaim::new(
        D1,
        EffectObservationState::NoEffect,
        RestoreEffectResolution::Abandoned,
        D2,
        "approval-1",
        4,
    )
    .expect("valid claim");
    forged.observed_state = EffectObservationState::Unknown;
    forged.recompute_digest();
    let mut fact = fact(RestoreVerificationState::Verified);
    fact.external_effect_claims = vec![forged];
    fact.recompute_digest();
    assert_eq!(fact.validate(), Err("restore_effect_claim_unknown"));
}

#[test]
fn artifact_refs_must_cover_the_manifest_files_with_matching_hashes() {
    let manifest = manifest();
    let expected = vec![
        restore_artifact_ref(&manifest.files[0]),
        restore_artifact_ref(&manifest.files[1]),
    ];
    // 缺一个文件。
    assert_eq!(
        bind_restore_artifact_refs(&manifest, &expected[..1]),
        Err("restore_artifact_ref_missing")
    );
    // 引用清单里没有的文件。
    assert_eq!(
        bind_restore_artifact_refs(
            &manifest,
            &[
                expected[0].clone(),
                expected[1].clone(),
                format!("artifact:ghost.jsonl#{FILE_A}"),
            ],
        ),
        Err("restore_artifact_ref_unknown")
    );
    // 同一路径、不同哈希（伪造内容）。
    assert_eq!(
        bind_restore_artifact_refs(
            &manifest,
            &[
                expected[0].clone(),
                format!("artifact:acl/roles.json#{FILE_A}")
            ],
        ),
        Err("restore_artifact_ref_hash_mismatch")
    );
    // 不是规范的 `artifact:<path>#<sha256>` 形状。
    for bad in [
        "artifact:events.jsonl",
        "events.jsonl#sha256:00",
        "artifact:#",
    ] {
        assert_eq!(
            bind_restore_artifact_refs(&manifest, &[bad.to_owned()]),
            Err("restore_artifact_ref_invalid"),
            "reference {bad}"
        );
    }
    // 成功列在最后：引用逐条对应真实文件哈希。
    bind_restore_artifact_refs(&manifest, &expected).expect("bound refs");
}

#[test]
fn blocked_and_reconciliation_facts_are_not_ready_for_activation() {
    // 结构合法不等于可激活：只有 Verified 通过就绪门。
    for state in [
        RestoreVerificationState::Blocked,
        RestoreVerificationState::NeedsReconciliation,
    ] {
        let fact = fact(state);
        assert!(fact.validate().is_ok(), "structure of {state:?}");
        assert_eq!(
            fact.require_verified(),
            Err("restore_verification_not_verified"),
            "state {state:?}"
        );
    }
    fact(RestoreVerificationState::Verified)
        .require_verified()
        .expect("verified fact is ready");
}

#[test]
fn needs_reconciliation_cannot_carry_external_effect_claims() {
    // 证据只在 Verified 下成立；NeedsReconciliation 携带 claim 会同时说「已对账」和「待对账」。
    let claim = RestoreExternalEffectClaim::new(
        D1,
        EffectObservationState::NoEffect,
        RestoreEffectResolution::Abandoned,
        D2,
        "approval-1",
        4,
    )
    .expect("claim");
    let mut pending = fact(RestoreVerificationState::NeedsReconciliation);
    pending.external_effect_claims = vec![claim];
    pending.recompute_digest();
    assert_eq!(
        pending.validate(),
        Err("restore_reconciliation_evidence_invalid")
    );
}

#[test]
fn duplicate_effect_claims_and_unsealed_claims_fail_closed() {
    let claim = RestoreExternalEffectClaim::new(
        D1,
        EffectObservationState::ConfirmedSuccess,
        RestoreEffectResolution::Reconciled,
        D2,
        "approval-1",
        4,
    )
    .expect("claim");
    let mut duplicated = fact(RestoreVerificationState::Verified);
    duplicated.external_effect_claims = vec![claim.clone(), claim];
    duplicated.recompute_digest();
    assert_eq!(duplicated.validate(), Err("restore_effect_claim_duplicate"));

    let mut unsealed = RestoreExternalEffectClaim::new(
        D1,
        EffectObservationState::ConfirmedSuccess,
        RestoreEffectResolution::Reconciled,
        D2,
        "approval-1",
        4,
    )
    .expect("claim");
    unsealed.receipt_digest = D1.to_owned();
    let mut tampered = fact(RestoreVerificationState::Verified);
    tampered.external_effect_claims = vec![unsealed];
    tampered.recompute_digest();
    assert_eq!(tampered.validate(), Err("restore_effect_claim_invalid"));
}

#[test]
fn verified_restore_requires_new_epochs_fences_and_quiescent_references() {
    assert!(fact(RestoreVerificationState::Verified).validate().is_ok());

    let mut rollback = fact(RestoreVerificationState::Verified);
    rollback.authority_epoch = 3;
    rollback.recompute_digest();
    assert_eq!(
        rollback.validate(),
        Err("restore_verification_fact_invalid")
    );

    let mut unverified = fact(RestoreVerificationState::Verified);
    unverified.unknown_effect_count = 1;
    unverified.recompute_digest();
    assert_eq!(unverified.validate(), Err("restore_verification_not_ready"));
}

#[test]
fn old_instance_fence_and_shape_tampering_fail_closed() {
    let mut unfenced = fact(RestoreVerificationState::Verified);
    unfenced.old_instance_fenced = false;
    unfenced.recompute_digest();
    assert_eq!(unfenced.validate(), Err("restore_old_instance_not_fenced"));

    let mut value = serde_json::to_value(fact(RestoreVerificationState::Verified)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<RestoreVerificationFact>(value).is_err());
}
