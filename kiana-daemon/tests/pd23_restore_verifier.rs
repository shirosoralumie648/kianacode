//! PD-23 Daemon 恢复门的内存集成 fixture，先拒绝、后成功。
//!
//! 测试从合成 `SnapshotManifest`、`RestoreVerificationFact`、Core 重新派生的 effect receipt
//! 和 DEP-23 `ActivationState` 走过 Daemon 的公开恢复门与 `DaemonHost::restore_gate*`。
//! fixture 不访问磁盘、EventStore、租约服务或真实 provider，因此只证明这些传入值在当前内存
//! 中的校验结果。
//!
//! 拒绝面优先的顺序：
//! 1. artifact 引用缺失 / 未知 / 哈希不符；
//! 2. 非 `Verified` fact（Blocked / NeedsReconciliation）不能进入激活；
//! 3. `Unknown` 观察不能被声明成已对账；
//! 4. 伪造/不匹配的 receipt 不能作为证据；
//! 5. 重启后的 `prepared` 状态不得放行；
//! 6. 成功路径（Verified + Activated + 绑定证据）在最后。

use kiana_core::{
    reconcile_effect, ActivationState, EffectReconciliationRequest, EffectResolution,
    ExternalReceiptRef, RestoreActivationRecord, RestoreActivationReport, RestoreActivationRequest,
    RootWriteMode, SupersededRootWriter,
};
use kiana_daemon::{verify_restore, verify_restore_ready, DaemonHost};
use kiana_domain::{
    restore_artifact_ref, EffectObservation, EffectObservationState, ExecutionId, FenceTokenId,
    InstanceId, InvocationId, OperationId, OperationLease, OperationLeaseCas, QuarantineStage,
    RequestId, RestoreEffectResolution, RestoreExternalEffectClaim, RestoreMode, RestoreQuarantine,
    RestoreRoot, RestoreScanReport, RestoreVerificationFact, RestoreVerificationState,
    SnapshotFileSeal, SnapshotManifest, SnapshotMode, StorageRootId, RESTORE_VERIFICATION_SCHEMA,
};

const PROJECTION: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FILE_A: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const FILE_B: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

// ---------------------------------------------------------------------------
// 合成清单与 fact
// ---------------------------------------------------------------------------

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
        2,
        None,
        true,
        true,
        vec![
            SnapshotFileSeal {
                relative_path: "acl/roles.json".to_owned(),
                size_bytes: 20,
                content_hash: FILE_B.to_owned(),
            },
            SnapshotFileSeal {
                relative_path: "events.jsonl".to_owned(),
                size_bytes: 10,
                content_hash: FILE_A.to_owned(),
            },
        ],
    )
    .expect("manifest")
}

/// 为传入清单构造一个自洽的 Verified 状态 fixture；artifact refs 逐条绑定清单文件哈希。
fn restore_fact(
    manifest: &SnapshotManifest,
    state: RestoreVerificationState,
) -> RestoreVerificationFact {
    let mut fact = RestoreVerificationFact {
        schema: RESTORE_VERIFICATION_SCHEMA.to_owned(),
        snapshot_manifest_digest: manifest.manifest_digest.clone(),
        owner_scope: manifest.owner_scope.clone(),
        store_id: manifest.store_id.clone(),
        previous_instance_id: Some(manifest.instance_id.clone()),
        restored_instance_id: "instance:new".to_owned(),
        source_cursor: manifest.source_cursor,
        projection_cursor: manifest.source_cursor,
        projection_digest: PROJECTION.to_owned(),
        data_epoch: manifest.data_epoch + 1,
        previous_data_epoch: Some(manifest.data_epoch),
        authority_epoch: 3,
        previous_authority_epoch: Some(2),
        artifact_refs: manifest.files.iter().map(restore_artifact_ref).collect(),
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
    fact.recompute_digest();
    fact
}

// ---------------------------------------------------------------------------
// DEP-23 激活状态（消费 Core，不在 daemon 重造 lease/fence）
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
    let manifest = manifest();
    let staged = RestoreRoot::new(
        "restore-1",
        root_id('b'),
        instance_id('b'),
        root_id('a'),
        instance_id('a'),
        manifest.manifest_digest.clone(),
        manifest.source_cursor,
        5,
        10,
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
    let record = RestoreActivationRecord::activate(&request, &report, vec!["audit/1".to_owned()])
        .expect("record");
    ActivationState::from_record(&record).expect("activated state")
}

fn prepared_state() -> ActivationState {
    ActivationState::prepared(&activation_request()).expect("prepared state")
}

// ---------------------------------------------------------------------------
// 外部 effect 证据（Core 重新派生 receipt）
// ---------------------------------------------------------------------------

fn observation(
    state: EffectObservationState,
    provider_receipt_id: Option<&str>,
    evidence: Vec<String>,
) -> EffectObservation {
    EffectObservation::new(
        ExecutionId::new(),
        InvocationId::new(),
        1,
        PROJECTION.to_owned(),
        FILE_A.to_owned(),
        FILE_B.to_owned(),
        provider_receipt_id.map(str::to_owned),
        Some("pending".to_owned()),
        1_700_000_000_000,
        Some(PROJECTION.to_owned()),
        evidence,
        state,
    )
    .expect("observation")
}

/// A `NoEffect` observation proved by a query, abandoned because nothing will ever happen.
fn reconciled_evidence() -> (
    EffectReconciliationRequest,
    kiana_core::EffectReconciliationReceipt,
    EffectObservation,
) {
    let observation = observation(
        EffectObservationState::NoEffect,
        None,
        vec![FILE_A.to_owned()],
    );
    let request = EffectReconciliationRequest::new(
        observation.clone(),
        EffectResolution::Abandoned,
        None,
        None,
        Some("provider confirmed the request never landed".to_owned()),
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

fn claim_for(observation: &EffectObservation) -> RestoreExternalEffectClaim {
    RestoreExternalEffectClaim::new(
        observation.observation_digest.clone(),
        EffectObservationState::NoEffect,
        RestoreEffectResolution::Abandoned,
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "approval-1",
        4,
    )
    .expect("claim")
}

fn host() -> DaemonHost {
    use kiana_capability_broker::CapabilityBroker;
    use kiana_core::ControlPlane;
    use kiana_eventlog::MemoryEventLog;
    use kiana_gates::DefaultGateEngine;
    use kiana_policy::DefaultPolicyEngine;
    use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
    use std::sync::Arc;

    struct NoApprovals;
    #[async_trait::async_trait]
    impl ApprovalStorePort for NoApprovals {
        async fn stage(
            &self,
            _: &kiana_domain::RequestContext,
            _: kiana_domain::CapabilityRequest,
            _: &str,
        ) -> Result<kiana_domain::ApprovalChallenge, PortError> {
            Err(PortError::Unavailable("fixture_no_approval".to_owned()))
        }
        async fn activate(&self, _: kiana_domain::ApprovalId) -> Result<(), PortError> {
            Err(PortError::Unavailable("fixture_no_approval".to_owned()))
        }
        async fn consume(
            &self,
            _: &kiana_domain::RequestContext,
            _: kiana_domain::ApprovalId,
        ) -> Result<kiana_domain::PendingApproval, PortError> {
            Err(PortError::Unavailable("fixture_no_approval".to_owned()))
        }
    }

    struct NoRunner;
    #[async_trait::async_trait]
    impl RunnerPort for NoRunner {
        async fn send(
            &self,
            _: kiana_runner_protocol::RunnerCommand,
        ) -> Result<Vec<kiana_runner_protocol::RunnerEvent>, PortError> {
            Err(PortError::Unavailable("fixture_no_runner".to_owned()))
        }
        async fn restore(
            &self,
            _: kiana_domain::RunId,
            _: serde_json::Value,
        ) -> Result<(), PortError> {
            Err(PortError::Unavailable("fixture_no_runner".to_owned()))
        }
    }

    let events: Arc<dyn EventStorePort> = Arc::new(MemoryEventLog::new());
    let core = Arc::new(ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events,
        Arc::new(CapabilityBroker::new()),
        Arc::new(NoApprovals),
        Arc::new(NoRunner),
    ));
    DaemonHost::new(core)
}

// ---------------------------------------------------------------------------
// 拒绝面优先
// ---------------------------------------------------------------------------

#[test]
fn missing_unknown_and_mismatched_artifact_refs_are_refused() {
    let manifest = manifest();
    let expected: Vec<String> = manifest.files.iter().map(restore_artifact_ref).collect();

    let mut missing = restore_fact(&manifest, RestoreVerificationState::Verified);
    missing.artifact_refs = vec![expected[0].clone()];
    missing.recompute_digest();
    assert_eq!(
        verify_restore(&manifest, &missing),
        Err("restore_artifact_ref_missing")
    );

    let mut unknown = restore_fact(&manifest, RestoreVerificationState::Verified);
    unknown
        .artifact_refs
        .push(format!("artifact:ghost.jsonl#{FILE_A}"));
    unknown.recompute_digest();
    assert_eq!(
        verify_restore(&manifest, &unknown),
        Err("restore_artifact_ref_unknown")
    );

    let mut mismatched = restore_fact(&manifest, RestoreVerificationState::Verified);
    mismatched.artifact_refs = vec![
        format!("artifact:acl/roles.json#{FILE_A}"),
        format!("artifact:events.jsonl#{FILE_A}"),
    ];
    mismatched.recompute_digest();
    assert_eq!(
        verify_restore(&manifest, &mismatched),
        Err("restore_artifact_ref_hash_mismatch")
    );

    let mut malformed = restore_fact(&manifest, RestoreVerificationState::Verified);
    malformed.artifact_refs = vec!["artifact:manifest".to_owned()];
    malformed.recompute_digest();
    assert_eq!(
        verify_restore(&manifest, &malformed),
        Err("restore_artifact_ref_invalid")
    );

    // 成功列：逐条绑定清单文件哈希。
    let bound = restore_fact(&manifest, RestoreVerificationState::Verified);
    verify_restore(&manifest, &bound).expect("bound fact");
}

#[test]
fn blocked_and_needs_reconciliation_facts_never_pass_the_ready_gate() {
    let manifest = manifest();
    let (request, receipt, observation) = reconciled_evidence();
    let evidence = [(&request, &receipt, &observation)];
    for state in [
        RestoreVerificationState::Blocked,
        RestoreVerificationState::NeedsReconciliation,
    ] {
        let fact = restore_fact(&manifest, state);
        // 结构合法（Blocked 需要非 ok reason；fixture 的 pending reason 满足）。
        assert!(fact.validate().is_ok(), "structure of {state:?}");
        assert_eq!(
            verify_restore_ready(&manifest, &fact, &activated_state(), &evidence),
            Err("restore_verification_not_verified".to_owned()),
            "state {state:?}"
        );
    }
}

#[test]
fn unknown_effect_cannot_be_claimed_as_reconciled_evidence() {
    let manifest = manifest();
    // 第一道门：claim 本身拒绝 Unknown（即使摘要被重算）。
    let mut forged = RestoreExternalEffectClaim::new(
        PROJECTION,
        EffectObservationState::NoEffect,
        RestoreEffectResolution::Abandoned,
        FILE_A,
        "approval-1",
        4,
    )
    .expect("claim");
    forged.observed_state = EffectObservationState::Unknown;
    forged.recompute_digest();
    assert_eq!(forged.validate(), Err("restore_effect_claim_unknown"));

    // 第二道门：事实层同样拒绝。
    let mut fact = restore_fact(&manifest, RestoreVerificationState::Verified);
    fact.external_effect_claims = vec![forged];
    fact.recompute_digest();
    assert_eq!(fact.validate(), Err("restore_effect_claim_unknown"));

    // 第三道门：即使 receipt/observation 是真实 Unknown，也不能作为证据通过。
    let unknown_observation = observation(
        EffectObservationState::Unknown,
        None,
        vec![FILE_A.to_owned()],
    );
    let request = EffectReconciliationRequest::new(
        unknown_observation.clone(),
        EffectResolution::Compensated,
        None,
        Some("compensate".to_owned()),
        None,
        "approval-1",
        4,
        "fence-1",
        Vec::new(),
        "idem-2",
    )
    .expect("request");
    let receipt = reconcile_effect(&request).expect("receipt");
    let mut factual = restore_fact(&manifest, RestoreVerificationState::Verified);
    factual.external_effect_claims = vec![claim_for(&unknown_observation)];
    factual.recompute_digest();
    assert_eq!(
        verify_restore_ready(
            &manifest,
            &factual,
            &activated_state(),
            &[(&request, &receipt, &unknown_observation)],
        ),
        Err("restore_effect_claim_unknown".to_owned())
    );
}

#[test]
fn evidence_that_does_not_match_its_claim_or_receipt_is_refused() {
    let manifest = manifest();
    let (request, receipt, observation) = reconciled_evidence();

    // 无 receipt 的 claim。
    let mut missing_receipt = restore_fact(&manifest, RestoreVerificationState::Verified);
    missing_receipt.external_effect_claims = vec![claim_for(&observation)];
    missing_receipt.recompute_digest();
    assert_eq!(
        verify_restore_ready(&manifest, &missing_receipt, &activated_state(), &[]),
        Err("restore_effect_claim_receipt_missing".to_owned())
    );

    // claim 指向另一条 observation；不匹配。
    let mut foreign = restore_fact(&manifest, RestoreVerificationState::Verified);
    let mut claim = claim_for(&observation);
    claim.observation_digest =
        "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_owned();
    claim.recompute_digest();
    foreign.external_effect_claims = vec![claim];
    foreign.recompute_digest();
    assert_eq!(
        verify_restore_ready(
            &manifest,
            &foreign,
            &activated_state(),
            &[(&request, &receipt, &observation)]
        ),
        Err("restore_effect_claim_receipt_missing".to_owned())
    );

    // 真实 receipt，但 claim 的 approval_ref 被改写：重新派生后不一致。
    let mut drifted = restore_fact(&manifest, RestoreVerificationState::Verified);
    let mut claim = claim_for(&observation);
    claim.approval_ref = "approval-forged".to_owned();
    claim.recompute_digest();
    drifted.external_effect_claims = vec![claim];
    drifted.recompute_digest();
    assert_eq!(
        verify_restore_ready(
            &manifest,
            &drifted,
            &activated_state(),
            &[(&request, &receipt, &observation)]
        ),
        Err("restore_effect_claim_receipt_mismatch".to_owned())
    );
}

#[test]
fn a_prepared_activation_never_admits_commands_after_restart() {
    let manifest = manifest();
    let fact = restore_fact(&manifest, RestoreVerificationState::Verified);
    let (request, receipt, observation) = reconciled_evidence();
    let evidence = [(&request, &receipt, &observation)];

    // 未激活的 root 即使事实已 Verified 也不得放行。
    assert_eq!(
        verify_restore_ready(&manifest, &fact, &prepared_state(), &evidence),
        Err("restore_activation_not_ready".to_owned())
    );

    // 宿主重启入口同样默认拒绝：没有激活事实就保持 prepared。
    let host = host();
    assert_eq!(
        host.restore_gate_after_restart(&manifest, &fact, &prepared_state(), &evidence),
        Err("restore_activation_not_ready".to_owned())
    );
    // 结构调整：prepared 状态连显式 gate 也不放行。
    assert_eq!(
        host.restore_gate(&manifest, &fact, &prepared_state(), &evidence),
        Err("restore_activation_not_ready".to_owned())
    );
}

// ---------------------------------------------------------------------------
// 成功列：Verified fact + 绑定证据 + Activated 状态。
// ---------------------------------------------------------------------------

#[test]
fn verified_fact_with_bound_evidence_and_activated_state_passes_the_host_gate() {
    let manifest = manifest();
    let (request, receipt, observation) = reconciled_evidence();
    let mut fact = restore_fact(&manifest, RestoreVerificationState::Verified);
    fact.external_effect_claims =
        vec![
            kiana_core::restore_effect_claim_from_receipt(&request, &receipt, &observation, 4)
                .expect("claim derived from the receipt"),
        ];
    fact.recompute_digest();
    let evidence = [(&request, &receipt, &observation)];

    verify_restore(&manifest, &fact).expect("manifest bound");
    verify_restore_ready(&manifest, &fact, &activated_state(), &evidence).expect("ready");

    let host = host();
    host.restore_gate(&manifest, &fact, &activated_state(), &evidence)
        .expect("host gate");
    host.restore_gate_after_restart(&manifest, &fact, &activated_state(), &evidence)
        .expect("restart gate with activated state");
}

#[test]
fn host_command_intake_never_admits_commands_before_activation() {
    let manifest = manifest();
    let (request, receipt, observation) = reconciled_evidence();
    let evidence = [(&request, &receipt, &observation)];
    let mut fact = restore_fact(&manifest, RestoreVerificationState::Verified);
    fact.external_effect_claims =
        vec![
            kiana_core::restore_effect_claim_from_receipt(&request, &receipt, &observation, 4)
                .expect("claim derived from the receipt"),
        ];
    fact.recompute_digest();
    let host = host();

    // prepared：即使命令字段全部正确也不放行。
    assert_eq!(
        host.admit_restored_command(
            &manifest,
            &fact,
            &prepared_state(),
            &evidence,
            &root_id('b'),
            &instance_id('b'),
            fence_id('b'),
            6,
            4,
        ),
        Err("restore_activation_not_ready".to_owned())
    );

    let activated = activated_state();
    // 旧 root 的写入：被 Core 的窗口门拒绝。
    assert_eq!(
        host.admit_restored_command(
            &manifest,
            &fact,
            &activated,
            &evidence,
            &root_id('a'),
            &instance_id('b'),
            fence_id('b'),
            6,
            4,
        ),
        Err("restore_activation_superseded_root_write".to_owned())
    );
    // 新 root + 新 fence + 新 epoch：放行。
    host.admit_restored_command(
        &manifest,
        &fact,
        &activated,
        &evidence,
        &root_id('b'),
        &instance_id('b'),
        fence_id('b'),
        6,
        4,
    )
    .expect("activated root admits its own command");
}

#[test]
fn an_empty_effect_claim_list_is_only_valid_when_nothing_is_unknown() {
    let manifest = manifest();
    let fact = restore_fact(&manifest, RestoreVerificationState::Verified);
    let (request, receipt, observation) = reconciled_evidence();
    // 空 claim 列表 + 0 unknown 是本切片的合法形态（无外部 effect 需要核对）。
    verify_restore_ready(
        &manifest,
        &fact,
        &activated_state(),
        &[(&request, &receipt, &observation)],
    )
    .expect("no claims to verify");

    let mut unknown_left = fact.clone();
    unknown_left.unknown_effect_count = 1;
    unknown_left.recompute_digest();
    assert_eq!(
        unknown_left.validate(),
        Err("restore_verification_not_ready")
    );
}

#[test]
fn external_receipt_is_never_accepted_without_a_provider_observation() {
    // 记录一个明确边界：claim 只能由真实 receipt 派生，`ExternalReceiptRef` 单独存在不是证据。
    let manifest = manifest();
    let (request, receipt, _observation) = reconciled_evidence();
    let mut fact = restore_fact(&manifest, RestoreVerificationState::Verified);
    fact.external_effect_claims = vec![RestoreExternalEffectClaim::new(
        request.observation.observation_digest.clone(),
        EffectObservationState::NoEffect,
        RestoreEffectResolution::Abandoned,
        receipt.receipt_digest.clone(),
        receipt.approval_ref.clone(),
        receipt.authority_epoch,
    )
    .expect("claim re-derived from the receipt")];
    fact.recompute_digest();
    // `ExternalReceiptRef` 只是一个引用；没有 observation 时同样无法通过证据门。
    let _ = ExternalReceiptRef::new("ext-1", FILE_B, 1_700_000_000_000);
    assert_eq!(
        verify_restore_ready(&manifest, &fact, &activated_state(), &[]),
        Err("restore_effect_claim_receipt_missing".to_owned())
    );
}
