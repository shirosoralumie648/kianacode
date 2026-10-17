//! BQ-19 成本归属准入与事件构造夹具（先拒绝、后成功）。
//!
//! 准入助手只校验“服务端可信 scope/source project + 精确、活跃、含 `cost.allocate` 的
//! SharingGrant”，不写账本、不派发 capability、不调用 provider。夹具全部在内存中合成，
//! ControlPlane facade 只用于证明同一门面没有第二条写入路径。

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{ControlPlane, CostAllocationAdmission, CostAllocationAdmissionError};
use kiana_domain::{
    AllocationCostKind, AllocationScope, BillingUnknownReason, CapabilityRequest, CellId,
    CostAllocation, CostAllocationId, EventId, Money, OrganizationId, PendingApproval, ProjectId,
    RateCardId, RequestContext, RequestId, RunId, SharingGrant, SharingGrantId, SharingGrantRef,
    UsageId, WorkflowInstanceId, COST_ALLOCATION_EVENT, COST_ALLOCATION_OPERATION,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::sync::Arc;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn scope() -> AllocationScope {
    AllocationScope::new(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowInstanceId::new(),
        CellId::new(),
        RunId::new(),
    )
    .expect("scope")
}

fn estimated_cost(micros: i128) -> AllocationCostKind {
    AllocationCostKind::Estimated {
        amount: Money::new("USD", micros).expect("money"),
        rate_card_id: RateCardId::new(),
        rate_card_version: 1,
    }
}

fn allocation(scope: AllocationScope, seed: char) -> CostAllocation {
    CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        scope.project_id,
        scope,
        None,
        estimated_cost(5),
        EventId::new(),
        1,
        1,
        digest(seed),
    )
    .expect("allocation")
}

fn grant_for(
    source_project: ProjectId,
    target_project: ProjectId,
    operations: Vec<String>,
    expires_at_unix_ms: u64,
    authority_epoch: u64,
) -> SharingGrant {
    SharingGrant::new(
        SharingGrantId::new(),
        source_project,
        target_project,
        vec!["usage".to_owned()],
        "cost allocation",
        operations,
        expires_at_unix_ms,
        authority_epoch,
    )
    .expect("grant")
}

fn cross_project_allocation(scope: AllocationScope, grant: &SharingGrant) -> CostAllocation {
    CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        grant.source_project,
        scope,
        Some(SharingGrantRef::from_grant(grant).expect("grant ref")),
        AllocationCostKind::Unknown {
            reason: BillingUnknownReason::Partial,
        },
        EventId::new(),
        2,
        1,
        digest('b'),
    )
    .expect("allocation")
}

struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

struct UnavailableApprovalStore;

#[async_trait]
impl ApprovalStorePort for UnavailableApprovalStore {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<kiana_domain::ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn activate(&self, _approval_id: kiana_domain::ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: kiana_domain::ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }
}

fn plane() -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        Arc::new(MemoryEventLog::new()),
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnavailableApprovalStore),
        Arc::new(UnavailableRunner),
    )
}

/// wire 自报 scope/source project/grant 与服务端事实或服务端绑定不一致时全部拒绝。
#[test]
fn admission_refuses_wire_reported_attribution_and_server_binding_drift() {
    let trusted = scope();
    let fact = allocation(trusted, 'a');

    let mut organization_drift = trusted;
    organization_drift.organization_id = OrganizationId::new();
    let mut project_drift = trusted;
    project_drift.project_id = ProjectId::new();
    let mut workflow_drift = trusted;
    workflow_drift.workflow_id = WorkflowInstanceId::new();
    let mut cell_drift = trusted;
    cell_drift.cell_id = CellId::new();
    let mut run_drift = trusted;
    run_drift.run_id = RunId::new();
    for (dimension, reported) in [
        ("organization", organization_drift),
        ("project", project_drift),
        ("workflow", workflow_drift),
        ("cell", cell_drift),
        ("run", run_drift),
    ] {
        assert_eq!(
            CostAllocationAdmission::authorize_wire(
                &fact,
                &reported,
                &trusted,
                trusted.project_id,
                None,
                100,
                1,
            )
            .unwrap_err(),
            CostAllocationAdmissionError::WireScopeInvalid(
                "cost_allocation_wire_scope_mismatch".to_owned()
            ),
            "wire {dimension} drift must fail"
        );
    }

    assert_eq!(
        CostAllocationAdmission::authorize_wire_attribution(
            &fact,
            &trusted,
            ProjectId::new(),
            None,
            &trusted,
            trusted.project_id,
            None,
            100,
            1,
        )
        .unwrap_err(),
        CostAllocationAdmissionError::WireScopeInvalid(
            "cost_allocation_wire_source_project_mismatch".to_owned()
        )
    );

    let target = scope();
    let source_project = ProjectId::new();
    let grant = grant_for(
        source_project,
        target.project_id,
        vec![COST_ALLOCATION_OPERATION.to_owned()],
        500,
        3,
    );
    let granted = cross_project_allocation(target, &grant);
    // 服务端事实带 grant 时，wire 声称没有授权同样是漂移。
    assert_eq!(
        CostAllocationAdmission::authorize_wire_attribution(
            &granted,
            &target,
            source_project,
            None,
            &target,
            source_project,
            Some(&grant),
            100,
            3,
        )
        .unwrap_err(),
        CostAllocationAdmissionError::WireScopeInvalid(
            "cost_allocation_wire_sharing_grant_mismatch".to_owned()
        )
    );
    // 与服务端事实一致的 wire 报告仍要过服务端 binding。
    assert!(CostAllocationAdmission::authorize_wire_attribution(
        &granted,
        &target,
        source_project,
        granted.sharing_grant.as_ref(),
        &target,
        source_project,
        Some(&grant),
        100,
        3,
    )
    .is_ok());

    // 服务端 scope/source project 与 allocation 不一致 → ServerBindingInvalid。
    let mut foreign_server_scope = trusted;
    foreign_server_scope.cell_id = CellId::new();
    assert_eq!(
        CostAllocationAdmission::authorize(
            &fact,
            &foreign_server_scope,
            trusted.project_id,
            None,
            100,
            1,
        )
        .unwrap_err(),
        CostAllocationAdmissionError::ServerBindingInvalid(
            "cost_allocation_server_scope_mismatch".to_owned()
        )
    );
    assert_eq!(
        CostAllocationAdmission::authorize(&fact, &trusted, ProjectId::new(), None, 100, 1)
            .unwrap_err(),
        CostAllocationAdmissionError::ServerBindingInvalid(
            "cost_allocation_server_source_project_mismatch".to_owned()
        )
    );

    // 被篡改的金额即使通过 wire 比较也会在 Domain 校验处失败。
    let mut tampered = fact.clone();
    tampered.cost = estimated_cost(999);
    assert_eq!(
        CostAllocationAdmission::authorize(&tampered, &trusted, trusted.project_id, None, 100, 1)
            .unwrap_err(),
        CostAllocationAdmissionError::AllocationInvalid(
            "cost_allocation_digest_mismatch".to_owned()
        )
    );
}

/// 跨项目准入要求精确、活跃、含 `cost.allocate` 的 grant；同项目携带 grant 也被拒绝。
#[test]
fn sharing_grant_admission_requires_an_exact_active_operation_bound_grant() {
    let target = scope();
    let source_project = ProjectId::new();
    let grant = grant_for(
        source_project,
        target.project_id,
        vec![COST_ALLOCATION_OPERATION.to_owned()],
        500,
        3,
    );
    let fact = cross_project_allocation(target, &grant);

    assert!(CostAllocationAdmission::authorize(
        &fact,
        &target,
        source_project,
        Some(&grant),
        100,
        3,
    )
    .is_ok());

    // 缺 grant → cross project required。
    assert_eq!(
        CostAllocationAdmission::authorize(&fact, &target, source_project, None, 100, 3)
            .unwrap_err(),
        CostAllocationAdmissionError::SharingGrantInvalid(
            "cross_project_sharing_grant_required".to_owned()
        )
    );

    // 同项目事实携带 grant → same_project_grant_unexpected。
    let same = allocation(target, 'c');
    assert_eq!(
        CostAllocationAdmission::authorize(&same, &target, target.project_id, Some(&grant), 100, 3)
            .unwrap_err(),
        CostAllocationAdmissionError::SharingGrantInvalid(
            "same_project_grant_unexpected".to_owned()
        )
    );

    // 过期与授权世代漂移 → inactive。
    assert_eq!(
        CostAllocationAdmission::authorize(&fact, &target, source_project, Some(&grant), 500, 3)
            .unwrap_err(),
        CostAllocationAdmissionError::SharingGrantInvalid(
            "cost_allocation_sharing_grant_inactive".to_owned()
        )
    );
    assert_eq!(
        CostAllocationAdmission::authorize(&fact, &target, source_project, Some(&grant), 100, 4)
            .unwrap_err(),
        CostAllocationAdmissionError::SharingGrantInvalid(
            "cost_allocation_sharing_grant_inactive".to_owned()
        )
    );

    // 缺少成本归属操作 → operation missing；目标项目不同的 grant → reference mismatch。
    let tool_only_grant = grant_for(
        source_project,
        target.project_id,
        vec!["usage.read".to_owned()],
        500,
        3,
    );
    let tool_only_fact = cross_project_allocation(target, &tool_only_grant);
    assert_eq!(
        CostAllocationAdmission::authorize(
            &tool_only_fact,
            &target,
            source_project,
            Some(&tool_only_grant),
            100,
            3,
        )
        .unwrap_err(),
        CostAllocationAdmissionError::SharingGrantInvalid(
            "cost_allocation_sharing_grant_operation_missing".to_owned()
        )
    );
    let foreign_grant = grant_for(
        source_project,
        ProjectId::new(),
        vec![COST_ALLOCATION_OPERATION.to_owned()],
        500,
        3,
    );
    assert_eq!(
        CostAllocationAdmission::authorize(
            &fact,
            &target,
            source_project,
            Some(&foreign_grant),
            100,
            3,
        )
        .unwrap_err(),
        CostAllocationAdmissionError::SharingGrantInvalid(
            "cost_allocation_sharing_grant_reference_mismatch".to_owned()
        )
    );

    // 被篡改的 grant 引用（摘要漂移）重算 allocation 摘要后仍不能绑定。
    let mut drifted = fact.clone();
    let mut reference = SharingGrantRef::from_grant(&grant).expect("grant ref");
    reference.grant_digest = digest('f');
    drifted.sharing_grant = Some(reference);
    drifted.allocation_digest = drifted.digest();
    assert_eq!(
        CostAllocationAdmission::authorize(&drifted, &target, source_project, Some(&grant), 100, 3)
            .unwrap_err(),
        CostAllocationAdmissionError::SharingGrantInvalid(
            "cost_allocation_sharing_grant_reference_mismatch".to_owned()
        )
    );
}

/// 事件构造只追加、绑定 `cost_allocation` 流和 allocation revision，并拒绝后来被改写的值。
#[test]
fn allocation_events_are_append_only_stream_bound_and_validated() {
    let trusted = scope();
    let fact = allocation(trusted, 'a');
    let event =
        CostAllocationAdmission::allocation_event(RequestId::new(), 4, &fact).expect("event");
    assert_eq!(event.kind, COST_ALLOCATION_EVENT);
    assert_eq!(event.aggregate_type.as_deref(), Some("cost_allocation"));
    assert_eq!(
        event.aggregate_id.as_deref(),
        Some(fact.allocation_id.to_string().as_str())
    );
    assert_eq!(event.stream_version, Some(fact.revision));
    assert_eq!(event.sequence, 4);
    assert_eq!(event.data["run_id"], serde_json::json!(trusted.run_id));
    assert_eq!(
        event.data["organization_id"],
        serde_json::json!(trusted.organization_id)
    );
    assert_eq!(
        event.data["workflow_id"],
        serde_json::json!(trusted.workflow_id)
    );
    assert_eq!(event.data["cell_id"], serde_json::json!(trusted.cell_id));
    assert!(
        !serde_json::to_string(&event.data)
            .expect("payload text")
            .to_ascii_lowercase()
            .contains("transcript"),
        "allocation events must never carry a transcript"
    );
    assert_eq!(
        kiana_domain::validate_event_payload(COST_ALLOCATION_EVENT, &event.data),
        Ok(())
    );

    // 事件边界重新校验 Domain；篡改金额后不能凭已有准入结果构造事件。
    let mut tampered = fact.clone();
    tampered.cost = estimated_cost(7);
    assert_eq!(
        CostAllocationAdmission::allocation_event(RequestId::new(), 5, &tampered).unwrap_err(),
        CostAllocationAdmissionError::AllocationInvalid(
            "cost_allocation_digest_mismatch".to_owned()
        )
    );
}

/// ControlPlane facade 只复用同一份准入规则，不新增第二条写入路径。
#[test]
fn control_plane_facade_admits_only_bound_allocations() {
    let plane = plane();
    let trusted = scope();
    let fact = allocation(trusted, 'a');

    assert!(plane
        .admit_cost_allocation(&fact, &trusted, trusted.project_id, None, 100, 1)
        .is_ok());

    let mut foreign_server_scope = trusted;
    foreign_server_scope.organization_id = OrganizationId::new();
    assert!(matches!(
        plane.admit_cost_allocation(
            &fact,
            &foreign_server_scope,
            trusted.project_id,
            None,
            100,
            1
        ),
        Err(CostAllocationAdmissionError::ServerBindingInvalid(_))
    ));

    let target = scope();
    let source_project = ProjectId::new();
    let grant = grant_for(
        source_project,
        target.project_id,
        vec![COST_ALLOCATION_OPERATION.to_owned()],
        500,
        3,
    );
    let granted = cross_project_allocation(target, &grant);
    assert!(plane
        .admit_cost_allocation_wire(
            &granted,
            &target,
            source_project,
            granted.sharing_grant.as_ref(),
            &target,
            source_project,
            Some(&grant),
            100,
            3,
        )
        .is_ok());
    assert_eq!(
        plane
            .admit_cost_allocation_wire(
                &granted,
                &target,
                source_project,
                granted.sharing_grant.as_ref(),
                &target,
                source_project,
                None,
                100,
                3,
            )
            .unwrap_err(),
        CostAllocationAdmissionError::SharingGrantInvalid(
            "cross_project_sharing_grant_required".to_owned()
        )
    );
    assert_eq!(
        plane
            .admit_cost_allocation_wire(
                &granted,
                &target,
                ProjectId::new(),
                granted.sharing_grant.as_ref(),
                &target,
                source_project,
                Some(&grant),
                100,
                3,
            )
            .unwrap_err(),
        CostAllocationAdmissionError::WireScopeInvalid(
            "cost_allocation_wire_source_project_mismatch".to_owned()
        )
    );
}
