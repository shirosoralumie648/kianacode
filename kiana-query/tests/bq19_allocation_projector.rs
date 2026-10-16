//! BQ-19 成本分摊投影的行为 fixture。
//!
//! 用例合成 Domain allocations 和 RuntimeEvent，不连接 EventStore、reservation ledger 或产品路由；
//! 覆盖维度汇总、usage 重复/跨 run 拒绝、估算/实测/未知分离和相同输入重建一致性。

use kiana_domain::*;
use kiana_query::{project_cost_allocations, CostAllocationProjectionError};

/// 用重复字母构造只满足 sha256 文本格式的测试值，不是实际加密计算。
fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

/// 构造互相独立的组织、项目、workflow、Cell 和 run 身份。
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

/// 用指定用量 ID、金额和摘要种子构造 Estimated allocation。
fn allocation(
    scope: AllocationScope,
    usage_id: UsageId,
    amount: i128,
    seed: char,
) -> CostAllocation {
    CostAllocation::new(
        CostAllocationId::new(),
        usage_id,
        scope.project_id,
        scope,
        None,
        AllocationCostKind::Estimated {
            amount: Money::new("USD", amount).expect("money"),
            rate_card_id: RateCardId::new(),
            rate_card_version: 1,
        },
        EventId::new(),
        1,
        1,
        digest(seed),
    )
    .expect("allocation")
}

/// 将 Domain allocation 放入成本分摊事件 payload，并把 run/aggregate/revision 写进 envelope metadata。
fn event(sequence: u64, allocation: &CostAllocation) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        COST_ALLOCATION_EVENT,
        serde_json::to_value(allocation).expect("allocation json"),
    )
    .expect("event")
    .with_stream_metadata(
        "cost_allocation",
        allocation.allocation_id.to_string(),
        allocation.revision,
    )
}

/// 两个唯一叶子分别进入 run 和四个维度的平行统计；每个视图都是 17 微单位，而不是相加成更大总额。
#[test]
fn projector_folds_each_leaf_once_across_all_dimensions() {
    let scope = scope();
    let first = allocation(scope, UsageId::new(), 10, 'a');
    let second = allocation(scope, UsageId::new(), 7, 'b');
    let projection = project_cost_allocations(scope.run_id, &[event(1, &first), event(2, &second)])
        .expect("projection");
    assert_eq!(projection.allocations.len(), 2);
    assert_eq!(
        projection
            .ledger_total()
            .estimated
            .as_ref()
            .expect("run")
            .micros,
        17
    );
    assert_eq!(
        projection
            .project_total(scope.project_id)
            .expect("project")
            .estimated
            .as_ref()
            .expect("project estimate")
            .micros,
        17
    );
    assert_eq!(
        projection
            .organization_total(scope.organization_id)
            .expect("org")
            .estimated
            .as_ref()
            .expect("org estimate")
            .micros,
        17
    );
    assert_eq!(
        projection
            .workflow_total(scope.workflow_id)
            .expect("workflow")
            .estimated
            .as_ref()
            .expect("workflow estimate")
            .micros,
        17
    );
    assert_eq!(
        projection
            .cell_total(scope.cell_id)
            .expect("cell")
            .estimated
            .as_ref()
            .expect("cell estimate")
            .micros,
        17
    );
    assert_eq!(projection.ledger_total().leaf_count, 2);
}

/// 重复同一 usage leaf 或把外层 Run 与 allocation scope 错绑时，拒绝整份结果。
#[test]
fn projector_rejects_repeated_usage_and_cross_run_event() {
    let local = scope();
    let usage = UsageId::new();
    let first = allocation(local, usage, 10, 'a');
    let duplicate = allocation(local, usage, 10, 'b');
    assert!(matches!(
        project_cost_allocations(local.run_id, &[event(1, &first), event(2, &duplicate)]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_leaf_usage_repeated"
    ));
    let foreign_scope = scope();
    let foreign = allocation(foreign_scope, UsageId::new(), 3, 'c');
    assert!(matches!(
        project_cost_allocations(local.run_id, &[event(1, &foreign)]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_identity_or_revision_mismatch"
    ));
}

/// Estimated/Measured 金额分别累计，Unknown 只记数量而不伪造成金额。
#[test]
fn estimated_measured_unknown_stay_separate() {
    let scope = scope();
    let estimated = allocation(scope, UsageId::new(), 10, 'a');
    let measured = CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        scope.project_id,
        scope,
        None,
        AllocationCostKind::Measured {
            amount: Money::new("USD", 4).expect("money"),
            provider_receipt: ProviderReceiptRef::new("provider:receipt:opaque").expect("receipt"),
        },
        EventId::new(),
        1,
        1,
        digest('b'),
    )
    .expect("measured");
    let unknown = CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        scope.project_id,
        scope,
        None,
        AllocationCostKind::Unknown {
            reason: BillingUnknownReason::ResultUnknown,
        },
        EventId::new(),
        1,
        1,
        digest('c'),
    )
    .expect("unknown");
    let projection = project_cost_allocations(
        scope.run_id,
        &[
            event(1, &estimated),
            event(2, &measured),
            event(3, &unknown),
        ],
    )
    .expect("projection");
    assert_eq!(
        projection
            .ledger_total()
            .estimated
            .as_ref()
            .expect("estimate")
            .micros,
        10
    );
    assert_eq!(
        projection
            .ledger_total()
            .measured
            .as_ref()
            .expect("measured")
            .micros,
        4
    );
    assert_eq!(projection.ledger_total().unknown_count, 1);
}

/// 相同有序事件输入重复投影得到一致的明细、来源 ID 和 run total。
///
/// 该断言只覆盖给定内存切片的确定性，不检查 EventStore 持久化或并发一致性。
#[test]
fn projector_rebuild_is_deterministic_and_read_only() {
    let scope = scope();
    let allocation = allocation(scope, UsageId::new(), 11, 'a');
    let events = vec![event(1, &allocation)];
    let left = project_cost_allocations(scope.run_id, &events).expect("left");
    let right = project_cost_allocations(scope.run_id, &events).expect("right");
    assert_eq!(left.run_totals, right.run_totals);
    assert_eq!(left.source_event_ids, right.source_event_ids);
    assert_eq!(left.allocations, right.allocations);
}
