//! PD-23 Core 源码与行为守卫。
//!
//! 两个测试：
//! 1. 源码边界守卫：Domain/Core 的恢复验证模块不得执行 effect（无文件系统、无 broker、
//!    无 EventLog append、无第二 lease/fence 铸造），且必须保留本卡的关键拒绝码；
//! 2. 行为守卫：激活就绪门、外部 effect claim 派生与交叉核对、Unknown 拒绝。

use kiana_core::{
    reconcile_effect, require_restore_ready, restore_effect_claim_from_receipt,
    verify_restore_effect_claims, ActivationState, EffectReconciliationRequest, EffectResolution,
    RestoreActivationRecord, RestoreActivationReport, RestoreActivationRequest,
    RestoreEffectClaimError, RootWriteMode, SupersededRootWriter,
};
use kiana_domain::{
    EffectObservation, EffectObservationState, ExecutionId, FenceTokenId, InstanceId, InvocationId,
    OperationId, OperationLease, OperationLeaseCas, QuarantineStage, RequestId,
    RestoreEffectResolution, RestoreExternalEffectClaim, RestoreMode, RestoreQuarantine,
    RestoreRoot, RestoreScanReport, RestoreVerificationFact, RestoreVerificationState,
    StorageRootId, RESTORE_VERIFICATION_SCHEMA,
};

#[test]
fn restore_verification_keeps_fences_epochs_and_pause_visible() {
    let domain = include_str!("../../kiana-domain/src/restore_verification.rs");
    let core = include_str!("../src/restore_verification.rs");
    let daemon = include_str!("../../kiana-daemon/src/restore_verifier.rs");
    for marker in [
        "RestoreVerificationFact",
        "snapshot_manifest_digest",
        "projection_cursor",
        "old_instance_fenced",
        "old_authority_fenced",
        "pending_approval_count",
        "unknown_effect_count",
        "default_paused",
        "validate_restore_verification_fact",
        "require_verified_restore_verification",
        "bind_restore_artifact_refs",
        "restore_artifact_ref_hash_mismatch",
        "restore_artifact_ref_missing",
        "restore_effect_claim_unknown",
        "RestoreExternalEffectClaim",
        "external_effect_claims",
        "restore_verification_not_verified",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || daemon.contains(marker),
            "PD-23 marker missing: {marker}"
        );
    }
    for forbidden in [
        "EventStore::append",
        "CapabilityBroker::new",
        "std::process::Command",
        "activate_root",
        "revive_trigger",
        "auto_success",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden) && !daemon.contains(forbidden),
            "PD-23 Core/domain executes restore effects: {forbidden}"
        );
    }
    // 第二权威负断言：daemon verifier 不自己造 lease/fence，只消费 Core 的 ActivationState。
    for forbidden in ["OperationLeaseCas", "OperationLease::", "FenceTokenId::new"] {
        assert!(
            !daemon.contains(forbidden),
            "PD-23 daemon mints its own lease/fence: {forbidden}"
        );
    }
    assert!(
        daemon.contains("kiana_core::require_restore_ready")
            || daemon.contains("require_restore_ready")
    );
}

// ---------------------------------------------------------------------------
// 行为守卫夹具
// ---------------------------------------------------------------------------

fn uuid(nibble: char, filler: char) -> String {
    format!("{nibble}{filler}{filler}{filler}0000-0000-0000-0000-000000000000")
}

fn root_id(nibble: char) -> StorageRootId {
    StorageRootId::parse_str(&uuid(nibble, '1')).expect("storage root id")
}

fn instance_id(nibble: char) -> InstanceId {
    InstanceId::parse_str(&uuid(nibble, '2')).expect("instance id")
}

fn fence_id(nibble: char) -> FenceTokenId {
    FenceTokenId::parse_str(&uuid(nibble, '3')).expect("fence token id")
}

fn request_id(nibble: char) -> RequestId {
    RequestId::parse_str(&uuid(nibble, '4')).expect("request id")
}

fn operation_id(nibble: char) -> OperationId {
    OperationId::parse_str(&uuid(nibble, '5')).expect("operation id")
}

const MANIFEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn released_lease() -> OperationLease {
    let mut cas = OperationLeaseCas::new(root_id('a'), 3, 2).expect("cas");
    let active = cas
        .acquire(
            cas.revision(),
            operation_id('a'),
            instance_id('a'),
            fence_id('a'),
            3,
            2,
            1_000,
            5_000,
        )
        .expect("acquire");
    cas.release(
        cas.revision(),
        active.operation_id,
        active.owner_instance_id,
        active.fence_token,
        2_000,
    )
    .expect("release")
}

fn ready_root() -> RestoreRoot {
    let staged = RestoreRoot::new(
        "restore-1",
        root_id('b'),
        instance_id('b'),
        root_id('a'),
        instance_id('a'),
        MANIFEST,
        1_000,
        5,
        900,
        4,
        5,
        Some(2),
        Some(3),
        vec!["artifact/a".to_owned(), "artifact/b".to_owned()],
        Some(2),
        true,
        RestoreMode::FullRebuild,
    )
    .expect("staged root");
    let quarantine =
        RestoreQuarantine::new(root_id('a'), instance_id('a'), vec![staged]).expect("quarantine");
    quarantine
        .advance("restore-1", QuarantineStage::Rebuilt)
        .expect("rebuilt")
        .advance("restore-1", QuarantineStage::Ready)
        .expect("ready")
        .roots[0]
        .clone()
}

fn activation_request() -> RestoreActivationRequest {
    let root = ready_root();
    let scan_report =
        RestoreScanReport::scan(&root, &["artifact/a".to_owned(), "artifact/b".to_owned()])
            .expect("scan");
    let superseded = SupersededRootWriter::new(
        instance_id('a'),
        root_id('a'),
        fence_id('a'),
        released_lease(),
        RootWriteMode::ReadOnly,
        true,
        true,
    )
    .expect("superseded writer");
    RestoreActivationRequest::new(
        request_id('b'),
        "operator",
        "project-a",
        root,
        scan_report,
        superseded,
        instance_id('b'),
        operation_id('b'),
        fence_id('b'),
        6,
        4,
        3_000,
        5_000,
        true,
        0,
        Vec::new(),
    )
    .expect("activation request")
}

fn activated_state() -> ActivationState {
    let request = activation_request();
    let report = RestoreActivationReport::evaluate(&request).expect("report");
    let record = RestoreActivationRecord::activate(&request, &report, Vec::new()).expect("record");
    ActivationState::from_record(&record).expect("activated")
}

fn prepared_state() -> ActivationState {
    ActivationState::prepared(&activation_request()).expect("prepared")
}

fn restore_fact(state: RestoreVerificationState) -> RestoreVerificationFact {
    let mut fact = RestoreVerificationFact {
        schema: RESTORE_VERIFICATION_SCHEMA.to_owned(),
        snapshot_manifest_digest: MANIFEST.to_owned(),
        owner_scope: "project:/repo".to_owned(),
        store_id: "store:one".to_owned(),
        previous_instance_id: Some("instance:old".to_owned()),
        restored_instance_id: "instance:new".to_owned(),
        source_cursor: 12,
        projection_cursor: 12,
        projection_digest: MANIFEST.to_owned(),
        data_epoch: 2,
        previous_data_epoch: Some(1),
        authority_epoch: 4,
        previous_authority_epoch: Some(3),
        artifact_refs: vec!["artifact:manifest".to_owned()],
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
            vec!["reconcile:1".to_owned()]
        } else {
            Vec::new()
        },
        state,
        reason: if matches!(state, RestoreVerificationState::Verified) {
            "ok".to_owned()
        } else {
            "pending".to_owned()
        },
        verification_digest: String::new(),
    };
    fact.recompute_digest();
    fact
}

fn observation(state: EffectObservationState) -> EffectObservation {
    EffectObservation::new(
        ExecutionId::new(),
        InvocationId::new(),
        1,
        MANIFEST.to_owned(),
        MANIFEST.to_owned(),
        MANIFEST.to_owned(),
        None,
        Some("pending".to_owned()),
        1_700_000_000_000,
        Some(MANIFEST.to_owned()),
        vec![MANIFEST.to_owned()],
        state,
    )
    .expect("observation")
}

fn abandoned_receipt() -> (
    EffectReconciliationRequest,
    kiana_core::EffectReconciliationReceipt,
    EffectObservation,
) {
    let observation = observation(EffectObservationState::NoEffect);
    let request = EffectReconciliationRequest::new(
        observation.clone(),
        EffectResolution::Abandoned,
        None,
        None,
        Some("provider confirmed nothing landed".to_owned()),
        "approval-1",
        4,
        "fence-1",
        Vec::new(),
        "idem-1",
    )
    .expect("request");
    let receipt = reconcile_effect(&request).expect("receipt");
    (request, receipt, observation)
}

#[test]
fn readiness_gate_requires_a_verified_fact_and_an_activated_state() {
    let verified = restore_fact(RestoreVerificationState::Verified);
    require_restore_ready(&verified, &activated_state()).expect("verified + activated");

    for state in [
        RestoreVerificationState::Blocked,
        RestoreVerificationState::NeedsReconciliation,
    ] {
        assert_eq!(
            require_restore_ready(&restore_fact(state), &activated_state()),
            Err("restore_verification_not_verified"),
            "state {state:?}"
        );
    }
    assert_eq!(
        require_restore_ready(&verified, &prepared_state()),
        Err("restore_activation_not_ready")
    );
}

#[test]
fn effect_claims_are_rederived_from_receipts_and_unknown_is_never_reconciled() {
    let (request, receipt, observation) = abandoned_receipt();
    let derived = restore_effect_claim_from_receipt(&request, &receipt, &observation, 4)
        .expect("derived claim");
    assert_eq!(derived.observed_state, EffectObservationState::NoEffect);
    assert_eq!(derived.resolution, RestoreEffectResolution::Abandoned);
    assert_eq!(derived.receipt_digest, receipt.receipt_digest);

    let mut fact = restore_fact(RestoreVerificationState::Verified);
    fact.external_effect_claims = vec![derived];
    fact.recompute_digest();
    verify_restore_effect_claims(&fact, &[(&request, &receipt, &observation)])
        .expect("claims match the receipts");

    // 改写 approval_ref：重新派生后不一致。
    let mut drifted = fact.clone();
    let mut claim = drifted.external_effect_claims[0].clone();
    claim.approval_ref = "approval-forged".to_owned();
    claim.recompute_digest();
    drifted.external_effect_claims = vec![claim];
    drifted.recompute_digest();
    assert_eq!(
        verify_restore_effect_claims(&drifted, &[(&request, &receipt, &observation)]),
        Err(RestoreEffectClaimError::ReceiptMismatch)
    );

    // Unknown 无论摘要如何都不能作为证据。
    let mut forged = RestoreExternalEffectClaim::new(
        MANIFEST,
        EffectObservationState::NoEffect,
        RestoreEffectResolution::Abandoned,
        MANIFEST,
        "approval-1",
        4,
    )
    .expect("claim");
    forged.observed_state = EffectObservationState::Unknown;
    forged.recompute_digest();
    let mut unknown_fact = restore_fact(RestoreVerificationState::Verified);
    unknown_fact.external_effect_claims = vec![forged];
    unknown_fact.recompute_digest();
    assert_eq!(unknown_fact.validate(), Err("restore_effect_claim_unknown"));
    assert_eq!(
        verify_restore_effect_claims(&unknown_fact, &[]),
        Err(RestoreEffectClaimError::Unknown)
    );
}

#[test]
fn a_claim_without_its_receipt_is_refused() {
    let (request, receipt, observation) = abandoned_receipt();
    let claim = restore_effect_claim_from_receipt(&request, &receipt, &observation, 4)
        .expect("derived claim");
    let mut fact = restore_fact(RestoreVerificationState::Verified);
    fact.external_effect_claims = vec![claim];
    fact.recompute_digest();
    assert_eq!(
        verify_restore_effect_claims(&fact, &[]),
        Err(RestoreEffectClaimError::MissingReceipt)
    );
}

#[test]
fn command_intake_is_refused_until_the_root_is_activated() {
    use kiana_core::admit_restored_command;

    let verified = restore_fact(RestoreVerificationState::Verified);
    let activated = activated_state();
    let prepared = prepared_state();

    // 在激活前：同一 root/fence/epoch 也必须被拒绝，且原因就是就绪门。
    assert_eq!(
        admit_restored_command(
            &verified,
            &prepared,
            &root_id('b'),
            &instance_id('b'),
            fence_id('b'),
            6,
            4,
        ),
        Err("restore_activation_not_ready".to_owned())
    );

    // 未 Verified 的 fact 即使激活状态已 Activated 也不放行。
    assert_eq!(
        admit_restored_command(
            &restore_fact(RestoreVerificationState::NeedsReconciliation),
            &activated,
            &root_id('b'),
            &instance_id('b'),
            fence_id('b'),
            6,
            4,
        ),
        Err("restore_verification_not_verified".to_owned())
    );

    // 旧 root 永不放行；新 root 只有全部身份/围栏/epoch 一致才放行。
    assert_eq!(
        admit_restored_command(
            &verified,
            &activated,
            &root_id('a'),
            &instance_id('b'),
            fence_id('b'),
            6,
            4,
        ),
        Err("restore_activation_superseded_root_write".to_owned())
    );
    admit_restored_command(
        &verified,
        &activated,
        &root_id('b'),
        &instance_id('b'),
        fence_id('b'),
        6,
        4,
    )
    .expect("activated root admits its own command");
}
