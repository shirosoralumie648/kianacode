//! BQ-19 成本分摊投影与 CO-45 portfolio 视图的行为 fixture（先拒绝、后成功）。
//!
//! 用例合成 Domain allocations 和 RuntimeEvent，不连接 EventStore、reservation ledger 或产品
//! 路由；覆盖 usage 叶子唯一性、来源围栏、identity/metadata 漂移、币种与溢出、维度 rollup 与
//! 总账一致、estimated/measured/unknown 分离，以及 portfolio 视图的可重算只读边界。

use kiana_domain::*;
use kiana_query::{
    project_cost_allocation_portfolio, project_cost_allocations, AllocationTotals,
    CostAllocationPortfolio, CostAllocationProjectionError,
};

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

/// 在同一 run 下构造另一个维度组合，用于父子/多维重复计的拒绝面。
fn sibling_scope(run_id: RunId) -> AllocationScope {
    AllocationScope::new(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowInstanceId::new(),
        CellId::new(),
        run_id,
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

/// 用指定币种构造 Estimated allocation，用于证明跨币种合计被拒绝。
fn allocation_in_currency(
    scope: AllocationScope,
    usage_id: UsageId,
    currency: &str,
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
            amount: Money::new(currency, amount).expect("money"),
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

fn measured_allocation(
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
        AllocationCostKind::Measured {
            amount: Money::new("USD", amount).expect("money"),
            provider_receipt: ProviderReceiptRef::new("provider:receipt:opaque").expect("receipt"),
        },
        EventId::new(),
        1,
        1,
        digest(seed),
    )
    .expect("measured")
}

fn unknown_allocation(scope: AllocationScope, usage_id: UsageId, seed: char) -> CostAllocation {
    CostAllocation::new(
        CostAllocationId::new(),
        usage_id,
        scope.project_id,
        scope,
        None,
        AllocationCostKind::Unknown {
            reason: BillingUnknownReason::ResultUnknown,
        },
        EventId::new(),
        1,
        1,
        digest(seed),
    )
    .expect("unknown")
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

/// 重复 usage leaf、重复来源事件与 sequence 回退都必须在任何合计成形前拒绝。
#[test]
fn projector_refuses_a_second_charge_for_one_usage_leaf() {
    let local = scope();
    let usage = UsageId::new();
    let first = allocation(local, usage, 10, 'a');

    // 同一个 usage leaf 换 allocation id 并换组织/项目/workflow/cell 标签 → 仍然只允许一笔。
    let sibling = sibling_scope(local.run_id);
    let duplicate = allocation(sibling, usage, 10, 'b');
    assert!(matches!(
        project_cost_allocations(local.run_id, &[event(1, &first), event(2, &duplicate)]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_leaf_usage_repeated"
    ));

    // 复用同一个来源事件 ID（不同 payload）→ duplicate source event。
    let other = allocation(local, UsageId::new(), 7, 'c');
    let mut replayed_event = event(2, &other);
    replayed_event.event_id = event(1, &first).event_id;
    assert!(matches!(
        project_cost_allocations(local.run_id, &[event(1, &first), replayed_event]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_projection_duplicate_source_event"
    ));

    // sequence 持平或回退 → sequence regression，不允许乱序重放。
    assert!(matches!(
        project_cost_allocations(local.run_id, &[event(2, &first), event(2, &other)]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_projection_sequence_regression"
    ));
    assert!(matches!(
        project_cost_allocations(local.run_id, &[event(2, &first), event(1, &other)]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_projection_sequence_regression"
    ));
}

/// 身份/元数据漂移和被篡改的 payload 必须在 totals 之前失败。
#[test]
fn projector_rejects_identity_metadata_and_payload_drift() {
    let local = scope();
    let fact = allocation(local, UsageId::new(), 8, 'a');

    // allocation 属于另一个 run。
    let foreign_scope = scope();
    let foreign = allocation(foreign_scope, UsageId::new(), 4, 'b');
    assert!(matches!(
        project_cost_allocations(local.run_id, &[event(1, &foreign)]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_identity_or_revision_mismatch"
    ));

    let mut wrong_aggregate_type = event(1, &fact);
    wrong_aggregate_type.aggregate_type = Some("cost_ledger".to_owned());
    assert!(matches!(
        project_cost_allocations(local.run_id, &[wrong_aggregate_type]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_identity_or_revision_mismatch"
    ));

    let mut wrong_aggregate_id = event(1, &fact);
    wrong_aggregate_id.aggregate_id = Some(CostAllocationId::new().to_string());
    assert!(matches!(
        project_cost_allocations(local.run_id, &[wrong_aggregate_id]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_identity_or_revision_mismatch"
    ));

    let mut wrong_stream_version = event(1, &fact);
    wrong_stream_version.stream_version = Some(fact.revision + 1);
    assert!(matches!(
        project_cost_allocations(local.run_id, &[wrong_stream_version]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_identity_or_revision_mismatch"
    ));

    // payload 被改写但不重签 allocation 摘要 → digest mismatch。
    let mut tampered = event(1, &fact);
    tampered.data["cost"] = serde_json::json!({
        "estimated": {
            "amount": {"currency": "USD", "micros": 0},
            "rate_card_id": fact.cost.estimated_amount().map(|_| RateCardId::new()),
            "rate_card_version": 1
        }
    });
    assert!(matches!(
        project_cost_allocations(local.run_id, &[tampered]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_decode_failed"
                || reason == "cost_allocation_digest_mismatch"
                || reason == "cost_allocation_rate_card_invalid"
    ));

    // payload 缺字段 → 解码失败，而不是默认成零。
    let mut missing_field = event(1, &fact);
    missing_field
        .data
        .as_object_mut()
        .expect("object")
        .remove("project_id");
    assert!(matches!(
        project_cost_allocations(local.run_id, &[missing_field]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_decode_failed"
    ));
}

/// 跨币种合计与整数溢出都被拒绝，绝不允许出现看似有效但被截断的报表。
#[test]
fn projector_rejects_mixed_currency_and_overflowing_totals() {
    let local = scope();
    let usd = allocation(local, UsageId::new(), 5, 'a');
    let eur = allocation_in_currency(local, UsageId::new(), "EUR", 5, 'b');
    assert!(matches!(
        project_cost_allocations(local.run_id, &[event(1, &usd), event(2, &eur)]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_currency_mismatch"
    ));

    let large = allocation(local, UsageId::new(), i128::MAX, 'c');
    let one = allocation(local, UsageId::new(), 1, 'd');
    assert!(matches!(
        project_cost_allocations(local.run_id, &[event(1, &large), event(2, &one)]),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_total_overflow"
    ));
}

/// 各维度 rollup 是同一批叶子的平行视图：run total 才是账本总额，维度相加必然重复计费。
#[test]
fn projected_dimension_rollups_are_parallel_views_of_one_ledger_total() {
    let local = scope();
    // 第二片叶子与第一片共享 organization，但在 project/workflow/cell 上不同：
    // organization 视图是一次真正的父子折叠，project/workflow/cell 则是互不重叠的分区视图。
    let mut other = local;
    other.project_id = ProjectId::new();
    other.workflow_id = WorkflowInstanceId::new();
    other.cell_id = CellId::new();
    let first = allocation(local, UsageId::new(), 10, 'a');
    let second = allocation(other, UsageId::new(), 7, 'b');
    let projection = project_cost_allocations(local.run_id, &[event(1, &first), event(2, &second)])
        .expect("projection");

    assert_eq!(projection.allocations.len(), 2);
    assert_eq!(projection.source_cursor, 2);
    assert_eq!(projection.source_event_ids.len(), 2);
    let ledger = projection.ledger_total();
    assert_eq!(ledger.estimated.as_ref().expect("ledger").micros, 17);
    assert_eq!(ledger.leaf_count, 2);

    // 组织父视图看到两片叶子共 17，而不是把同一叶子按维度重复计费。
    assert_eq!(
        projection
            .organization_total(local.organization_id)
            .expect("org")
            .estimated
            .as_ref()
            .expect("org estimate")
            .micros,
        17
    );
    // project/workflow/cell 是分区视图：只有逐项相加才等于账本总额。
    assert_eq!(
        projection
            .project_total(local.project_id)
            .expect("project")
            .estimated
            .as_ref()
            .expect("project estimate")
            .micros,
        10
    );
    assert_eq!(
        projection
            .project_total(other.project_id)
            .expect("another project")
            .estimated
            .as_ref()
            .expect("project estimate")
            .micros,
        7
    );
    assert_eq!(
        projection
            .workflow_total(other.workflow_id)
            .expect("workflow")
            .estimated
            .as_ref()
            .expect("workflow estimate")
            .micros,
        7
    );
    assert_eq!(
        projection
            .cell_total(other.cell_id)
            .expect("cell")
            .estimated
            .as_ref()
            .expect("cell estimate")
            .micros,
        7
    );

    // 把 run、organization、project、workflow、cell 五份视图相加会得到五倍重复计费；
    // 账本只承认 run total 这一份。
    let dimension_sum: i128 = std::iter::once(projection.run_totals.estimated.as_ref())
        .chain(
            projection
                .organization_totals
                .values()
                .map(|totals| totals.estimated.as_ref()),
        )
        .chain(
            projection
                .project_totals
                .values()
                .map(|totals| totals.estimated.as_ref()),
        )
        .chain(
            projection
                .workflow_totals
                .values()
                .map(|totals| totals.estimated.as_ref()),
        )
        .chain(
            projection
                .cell_totals
                .values()
                .map(|totals| totals.estimated.as_ref()),
        )
        .flatten()
        .map(|amount| amount.micros)
        .sum();
    assert_eq!(dimension_sum, 5 * 17);
    assert_ne!(
        dimension_sum,
        ledger.estimated.as_ref().expect("ledger").micros
    );
}

/// Estimated/Measured 金额分别累计，Unknown 只记数量而不伪造成金额。
#[test]
fn estimated_measured_and_unknown_stay_separate_without_zero_faking() {
    let local = scope();
    let estimated = allocation(local, UsageId::new(), 10, 'a');
    let measured = measured_allocation(local, UsageId::new(), 4, 'b');
    let unknown = unknown_allocation(local, UsageId::new(), 'c');
    let projection = project_cost_allocations(
        local.run_id,
        &[
            event(1, &estimated),
            event(2, &measured),
            event(3, &unknown),
        ],
    )
    .expect("projection");
    let ledger = projection.ledger_total();
    assert_eq!(ledger.estimated.as_ref().expect("estimate").micros, 10);
    assert_eq!(ledger.measured.as_ref().expect("measured").micros, 4);
    assert_eq!(ledger.unknown_count, 1);
    assert_eq!(ledger.leaf_count, 3);
    assert_eq!(
        projection
            .project_total(local.project_id)
            .expect("project")
            .unknown_count,
        1
    );

    // 只有 unknown 时不存在任何金额槽位，不能读取成 0。
    let unknown_only =
        project_cost_allocations(local.run_id, &[event(1, &unknown)]).expect("unknown projection");
    assert!(unknown_only.ledger_total().estimated.is_none());
    assert!(unknown_only.ledger_total().measured.is_none());
    assert_eq!(unknown_only.ledger_total().unknown_count, 1);
}

/// CO-45 portfolio 视图由同一批叶子确定性重算，且篡改会被摘要/叶子数围栏拦住。
#[test]
fn portfolio_view_is_recomputable_from_the_same_leaf_set() {
    let local = scope();
    let other = sibling_scope(local.run_id);
    let first = allocation(local, UsageId::new(), 10, 'a');
    let second = allocation(other, UsageId::new(), 7, 'b');
    let events = [event(1, &first), event(2, &second)];

    let left = project_cost_allocation_portfolio(local.run_id, &events).expect("left");
    let right = project_cost_allocation_portfolio(local.run_id, &events).expect("right");
    assert_eq!(left, right);
    assert_eq!(left.recompute_digest, right.recompute_digest);
    assert_eq!(left.projects.len(), 2);
    assert_eq!(
        left.ledger_total.estimated.as_ref().expect("ledger").micros,
        17
    );

    let projection = project_cost_allocations(local.run_id, &events).expect("projection");
    let from_projection =
        CostAllocationPortfolio::from_projection(&projection).expect("portfolio from projection");
    assert_eq!(from_projection.recompute_digest, left.recompute_digest);

    assert_eq!(
        left.project(local.organization_id, local.project_id)
            .expect("project view")
            .estimated
            .as_ref()
            .expect("project estimate")
            .micros,
        10
    );
    assert_eq!(
        left.project(other.organization_id, other.project_id)
            .expect("project view")
            .estimated
            .as_ref()
            .expect("project estimate")
            .micros,
        7
    );

    // 摘要被改写 → digest mismatch，不能对外声称可重算。
    let mut tampered_digest = left.clone();
    tampered_digest.recompute_digest = digest('z');
    assert!(matches!(
        tampered_digest.validate(),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_portfolio_digest_mismatch"
    ));

    // 行叶子数与账本不一致 → total mismatch（在摘要之前失败）。
    let mut tampered_count = left.clone();
    tampered_count.projects[0].totals.leaf_count = 9;
    assert!(matches!(
        tampered_count.validate(),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_portfolio_total_mismatch"
    ));

    // 金额被改写但叶子数不变 → 摘要失配，不能悄悄改账。
    let mut tampered_amount = left.clone();
    let mut entry = tampered_amount.projects.remove(0);
    entry.totals.estimated = Some(Money::new("USD", 999).expect("money"));
    tampered_amount.projects.insert(0, entry);
    assert!(matches!(
        tampered_amount.validate(),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_portfolio_digest_mismatch"
    ));

    // 空行集合不是合法 portfolio 视图。
    assert!(matches!(
        project_cost_allocation_portfolio(
            RunId::parse_str("00000000-0000-0000-0000-000000000000").expect("nil run"),
            &events
        ),
        Err(CostAllocationProjectionError::RunInvalid)
    ));
}

/// 公开 `validate()` 必须拒绝头部、来源事件重复和无法由明细重算的汇总。
#[test]
fn projection_validate_rejects_header_source_and_recompute_drift() {
    let local = scope();
    let fact = allocation(local, UsageId::new(), 12, 'a');
    let events = [event(1, &fact)];
    let projection = project_cost_allocations(local.run_id, &events).expect("projection");
    assert_eq!(projection.validate(), Ok(()));

    let mut missing_sources = projection.clone();
    missing_sources.source_event_ids.clear();
    assert!(matches!(
        missing_sources.validate(),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_projection_header_invalid"
    ));

    let mut duplicate_sources = projection.clone();
    duplicate_sources.allocations.push(fact.clone());
    duplicate_sources
        .source_event_ids
        .push(projection.source_event_ids[0]);
    assert!(matches!(
        duplicate_sources.validate(),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_projection_source_event_duplicate"
    ));

    let mut recomputed_run_total = projection.clone();
    recomputed_run_total.run_totals = AllocationTotals {
        estimated: Some(Money::new("USD", 13).expect("money")),
        measured: None,
        unknown_count: 0,
        leaf_count: 1,
    };
    assert!(matches!(
        recomputed_run_total.validate(),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_projection_totals_recompute_mismatch"
    ));

    let mut extra_dimension = projection.clone();
    extra_dimension
        .organization_totals
        .insert(OrganizationId::new(), AllocationTotals::default());
    assert!(matches!(
        extra_dimension.validate(),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_projection_totals_recompute_mismatch"
    ));

    // 手工构造的未知计数超过叶子数也必须被局部校验拦住。
    let mut bad_unknown = projection.clone();
    bad_unknown.run_totals.unknown_count = 2;
    assert!(matches!(
        bad_unknown.validate(),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_unknown_count_invalid"
    ));
}

/// 其他 kind 的事件被跳过、nil run 和空来源被拒绝，投影不会假装成功。
#[test]
fn projector_skips_unrelated_events_and_rejects_empty_or_nil_sources() {
    let local = scope();
    let fact = allocation(local, UsageId::new(), 3, 'a');
    let unrelated = RuntimeEvent::new(
        RequestId::new(),
        99,
        "run.snapshot",
        serde_json::json!({"run_id": local.run_id}),
    )
    .expect("unrelated event");
    let projection = project_cost_allocations(
        local.run_id,
        &[unrelated.clone(), event(1, &fact), unrelated],
    )
    .expect("projection");
    assert_eq!(projection.allocations.len(), 1);
    assert_eq!(projection.source_cursor, 1);

    let nil_run = RunId::parse_str("00000000-0000-0000-0000-000000000000").expect("nil run");
    assert!(matches!(
        project_cost_allocations(nil_run, &[event(1, &fact)]),
        Err(CostAllocationProjectionError::RunInvalid)
    ));

    let unrelated_run = scope();
    assert!(matches!(
        project_cost_allocations(
            unrelated_run.run_id,
            &[
                RuntimeEvent::new(RequestId::new(), 1, "run.snapshot", serde_json::json!({}),)
                    .expect("unrelated")
            ]
        ),
        Err(CostAllocationProjectionError::SourceEmpty)
    ));

    // 手工构造的投影也不能把重复 usage 藏进明细。
    let mut duplicate_leaf = projection.clone();
    duplicate_leaf.allocations.push(fact.clone());
    duplicate_leaf.source_event_ids.push(EventId::new());
    assert!(matches!(
        duplicate_leaf.validate(),
        Err(CostAllocationProjectionError::Invalid(reason))
            if reason == "cost_allocation_leaf_duplicate_or_run_mismatch"
    ));
}
