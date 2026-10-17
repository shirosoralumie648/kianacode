//! BQ-19 成本归属 domain 夹具（先拒绝、后成功）。
//!
//! 夹具全部在内存中合成：一条 allocation 叶子、五个归属维度和一个可选的跨项目
//! `SharingGrant`。它们不写 EventLog、不消费预算、不调用 provider，也不把 wire 自报字段当成
//! 授权；成功只表示输入可以进入事件提交阶段。

use kiana_domain::*;
use serde_json::json;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn nil_text() -> &'static str {
    "00000000-0000-0000-0000-000000000000"
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
        rate_card_version: 4,
    }
}

#[allow(clippy::too_many_arguments)]
fn allocation_with(
    scope: AllocationScope,
    source_project_id: ProjectId,
    usage_id: UsageId,
    sharing_grant: Option<SharingGrantRef>,
    cost: AllocationCostKind,
    source_event_id: EventId,
    source_cursor: u64,
    seed: char,
) -> CostAllocation {
    CostAllocation::new(
        CostAllocationId::new(),
        usage_id,
        source_project_id,
        scope,
        sharing_grant,
        cost,
        source_event_id,
        source_cursor,
        1,
        digest(seed),
    )
    .expect("allocation")
}

fn same_project_allocation() -> CostAllocation {
    same_project_leaf(UsageId::new(), estimated_cost(17), EventId::new(), 7, 'a')
}

fn same_project_leaf(
    usage_id: UsageId,
    cost: AllocationCostKind,
    source_event_id: EventId,
    source_cursor: u64,
    seed: char,
) -> CostAllocation {
    let scope = scope();
    allocation_with(
        scope,
        scope.project_id,
        usage_id,
        None,
        cost,
        source_event_id,
        source_cursor,
        seed,
    )
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
    allocation_with(
        scope,
        grant.source_project,
        UsageId::new(),
        Some(SharingGrantRef::from_grant(grant).expect("grant ref")),
        estimated_cost(9),
        EventId::new(),
        11,
        'b',
    )
}

/// wire 自报的归属字段只是断言：任何维度的漂移都必须在事实成形前拒绝。
#[test]
fn wire_reported_attribution_never_grants_ownership() {
    let fact = same_project_allocation();
    let trusted = fact.scope;

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
            fact.validate_wire_scope(&reported),
            Err("cost_allocation_wire_scope_mismatch".to_owned()),
            "wire {dimension} drift must be refused"
        );
    }

    assert_eq!(
        fact.validate_wire_attribution(&trusted, ProjectId::new(), None),
        Err("cost_allocation_wire_source_project_mismatch".to_owned())
    );

    // 服务端事实带 grant 时，wire 声称“没有授权”同样是漂移；反之亦然。
    let target = scope();
    let source_project = ProjectId::new();
    let grant = grant_for(
        source_project,
        target.project_id,
        vec![COST_ALLOCATION_OPERATION.to_owned()],
        10_000,
        2,
    );
    let granted = cross_project_allocation(target, &grant);
    assert_eq!(
        granted.validate_wire_attribution(&target, source_project, None),
        Err("cost_allocation_wire_sharing_grant_mismatch".to_owned())
    );
    let foreign_ref = SharingGrantRef::from_grant(&grant).expect("grant ref");
    assert_eq!(
        fact.validate_wire_attribution(&trusted, fact.source_project_id, Some(&foreign_ref)),
        Err("cost_allocation_wire_sharing_grant_mismatch".to_owned())
    );

    // 与服务端事实逐字段一致的报告才通过；通过也不授予新的所有权。
    assert_eq!(
        fact.validate_wire_attribution(&trusted, fact.source_project_id, None),
        Ok(())
    );
    assert_eq!(
        granted.validate_wire_attribution(&target, source_project, granted.sharing_grant.as_ref()),
        Ok(())
    );
}

/// 一条 usage 叶子只能被计费一次；换 allocation id、换维度标签或自称父级都不产生第二笔。
#[test]
fn one_usage_leaf_is_charged_once_whatever_the_dimension_labels_say() {
    let mut ledger = CostAllocationLedger::default();
    let usage_id = UsageId::new();
    let first = same_project_leaf(usage_id, estimated_cost(5), EventId::new(), 3, 'a');
    let outcome = ledger.append(first.clone()).expect("append");
    assert_eq!(
        outcome,
        CostAllocationAppendOutcome::Appended(first.clone())
    );
    assert_eq!(ledger.leaf_count(), 1);
    assert_eq!(ledger.source_cursor(), 3);
    assert_eq!(
        ledger
            .allocation_for_usage(usage_id)
            .expect("leaf")
            .allocation_id,
        first.allocation_id
    );

    // 同 allocation id + 同摘要 → 幂等重放返回原事实，不再是第二笔。
    assert_eq!(
        ledger.append(first.clone()).expect("replay"),
        CostAllocationAppendOutcome::Replayed(first.clone())
    );
    assert_eq!(ledger.leaf_count(), 1);

    // 同 allocation id 但内容不同（重算摘要）→ 重放冲突，不能原地改账。
    let mut tampered = first.clone();
    tampered.cost = estimated_cost(99);
    tampered.allocation_digest = tampered.digest();
    assert_eq!(
        ledger.append(tampered).unwrap_err(),
        "cost_allocation_replay_conflict".to_owned()
    );

    // 同 usage leaf 换 allocation id、换 workflow/cell 维度再次计费 → 稳定拒绝。
    let second = same_project_leaf(usage_id, estimated_cost(5), EventId::new(), 4, 'b');
    assert_eq!(
        ledger.append(second).unwrap_err(),
        "cost_allocation_usage_already_charged".to_owned()
    );

    // 复用来源事件或让来源 cursor 回退/持平 → 拒绝，账本保持只追加。
    let reused_event = same_project_leaf(
        UsageId::new(),
        estimated_cost(5),
        first.source_event_id,
        5,
        'c',
    );
    assert_eq!(
        ledger.append(reused_event).unwrap_err(),
        "cost_allocation_source_event_duplicate".to_owned()
    );
    let stale_cursor = same_project_leaf(UsageId::new(), estimated_cost(5), EventId::new(), 3, 'd');
    assert_eq!(
        ledger.append(stale_cursor).unwrap_err(),
        "cost_allocation_source_cursor_regression".to_owned()
    );
    assert_eq!(ledger.leaf_count(), 1);
}

/// 跨项目归属必须有精确、活跃、含 `cost.allocate` 的 grant；每条失败都有独立原因。
#[test]
fn cross_project_requires_an_exact_active_operation_bound_grant() {
    let target = scope();
    let source_project = ProjectId::new();

    // 没有 grant 引用时跨项目事实根本无法成形。
    assert_eq!(
        CostAllocation::new(
            CostAllocationId::new(),
            UsageId::new(),
            source_project,
            target,
            None,
            estimated_cost(9),
            EventId::new(),
            11,
            1,
            digest('b'),
        )
        .unwrap_err(),
        "cost_allocation_cross_project_sharing_grant_required".to_owned()
    );

    // 引用与 grant 的目标项目不一致 → project mismatch。
    let foreign_target_grant = grant_for(
        source_project,
        ProjectId::new(),
        vec![COST_ALLOCATION_OPERATION.to_owned()],
        10_000,
        2,
    );
    assert_eq!(
        CostAllocation::new(
            CostAllocationId::new(),
            UsageId::new(),
            source_project,
            target,
            Some(SharingGrantRef::from_grant(&foreign_target_grant).expect("grant ref")),
            estimated_cost(9),
            EventId::new(),
            11,
            1,
            digest('b'),
        )
        .unwrap_err(),
        "cost_allocation_sharing_grant_project_mismatch".to_owned()
    );

    let grant = grant_for(
        source_project,
        target.project_id,
        vec![COST_ALLOCATION_OPERATION.to_owned()],
        10_000,
        2,
    );
    let fact = cross_project_allocation(target, &grant);
    assert_eq!(fact.validate_grant_binding(&grant, 10, 2), Ok(()));

    // grant 摘要漂移的引用即使重算 allocation 摘要也不能与活跃 grant 绑定。
    let mut drifted_reference = fact.clone();
    let mut reference = SharingGrantRef::from_grant(&grant).expect("grant ref");
    reference.grant_digest = digest('f');
    drifted_reference.sharing_grant = Some(reference);
    drifted_reference.allocation_digest = drifted_reference.digest();
    assert_eq!(
        drifted_reference.validate_grant_binding(&grant, 10, 2),
        Err("cost_allocation_sharing_grant_reference_mismatch".to_owned())
    );

    // 过期或授权世代漂移 → inactive。
    assert_eq!(
        fact.validate_grant_binding(&grant, grant.expires_at_unix_ms, grant.authority_epoch),
        Err("cost_allocation_sharing_grant_inactive".to_owned())
    );
    assert_eq!(
        fact.validate_grant_binding(&grant, 10, grant.authority_epoch + 1),
        Err("cost_allocation_sharing_grant_inactive".to_owned())
    );

    // grant 不含成本归属操作 → operation missing。
    let tool_only_grant = grant_for(
        source_project,
        target.project_id,
        vec!["usage.read".to_owned()],
        10_000,
        2,
    );
    let tool_only_fact = cross_project_allocation(target, &tool_only_grant);
    assert_eq!(
        tool_only_fact.validate_grant_binding(&tool_only_grant, 10, 2),
        Err("cost_allocation_sharing_grant_operation_missing".to_owned())
    );

    // 同项目事实不允许携带 grant，避免把无关分享授权混进账本。
    let same_project = same_project_allocation();
    assert_eq!(
        same_project.validate_grant_binding(&grant, 10, 2),
        Err("cost_allocation_same_project_grant_unexpected".to_owned())
    );
}

/// 头部、来源摘要、范围、rate card 与金额符号的伪造都在成形前拒绝。
#[test]
fn allocation_header_provenance_and_cost_state_are_fenced() {
    let base = same_project_allocation();

    let mut nil_usage = base.clone();
    nil_usage.usage_id = UsageId::parse_str(nil_text()).expect("nil usage");
    nil_usage.allocation_digest = nil_usage.digest();
    assert_eq!(
        nil_usage.validate(),
        Err("cost_allocation_header_invalid".to_owned())
    );

    let mut nil_allocation = base.clone();
    nil_allocation.allocation_id = CostAllocationId::parse_str(nil_text()).expect("nil allocation");
    nil_allocation.allocation_digest = nil_allocation.digest();
    assert_eq!(
        nil_allocation.validate(),
        Err("cost_allocation_header_invalid".to_owned())
    );

    let mut zero_cursor = base.clone();
    zero_cursor.source_cursor = 0;
    zero_cursor.allocation_digest = zero_cursor.digest();
    assert_eq!(
        zero_cursor.validate(),
        Err("cost_allocation_header_invalid".to_owned())
    );

    let mut zero_revision = base.clone();
    zero_revision.revision = 0;
    zero_revision.allocation_digest = zero_revision.digest();
    assert_eq!(
        zero_revision.validate(),
        Err("cost_allocation_header_invalid".to_owned())
    );

    let mut nil_scope = base.clone();
    nil_scope.scope.run_id = RunId::parse_str(nil_text()).expect("nil run");
    nil_scope.allocation_digest = nil_scope.digest();
    assert_eq!(
        nil_scope.validate(),
        Err("cost_allocation_scope_invalid".to_owned())
    );

    let mut wrong_schema = base.clone();
    wrong_schema.schema = "kiana.cost-allocation.v2".to_owned();
    wrong_schema.allocation_digest = wrong_schema.digest();
    assert_eq!(
        wrong_schema.validate(),
        Err("cost_allocation_header_invalid".to_owned())
    );

    let mut future_major = base.clone();
    future_major.version = SchemaVersion::new(2, 0);
    future_major.allocation_digest = future_major.digest();
    assert_eq!(
        future_major.validate(),
        Err("cost_allocation_header_invalid".to_owned())
    );

    let mut bad_source_digest = base.clone();
    bad_source_digest.source_digest = "not-a-digest".to_owned();
    bad_source_digest.allocation_digest = bad_source_digest.digest();
    assert_eq!(
        bad_source_digest.validate(),
        Err("cost_allocation_source_digest_invalid".to_owned())
    );

    let mut bad_digest = base.clone();
    bad_digest.allocation_digest = "sha256:zzzz".to_owned();
    assert_eq!(
        bad_digest.validate(),
        Err("cost_allocation_digest_invalid".to_owned())
    );

    // 金额被改写但不重算摘要 → digest mismatch，而不是静默接受新金额。
    let mut tampered_amount = base.clone();
    tampered_amount.cost = estimated_cost(1_000);
    assert_eq!(
        tampered_amount.validate(),
        Err("cost_allocation_digest_mismatch".to_owned())
    );

    // 伪造 rate card：nil id 或零版本都不能作为估算依据。
    for (rate_card_id, rate_card_version) in [
        (RateCardId::parse_str(nil_text()).expect("nil card"), 4),
        (RateCardId::new(), 0),
    ] {
        let forged = CostAllocation::new(
            CostAllocationId::new(),
            UsageId::new(),
            base.source_project_id,
            base.scope,
            None,
            AllocationCostKind::Estimated {
                amount: Money::new("USD", 17).expect("money"),
                rate_card_id,
                rate_card_version,
            },
            EventId::new(),
            12,
            1,
            digest('e'),
        );
        assert_eq!(
            forged.unwrap_err(),
            "cost_allocation_rate_card_invalid".to_owned()
        );
    }

    // 负金额既不能伪装成估算，也不能伪装成实测。
    let negative_estimate = CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        base.source_project_id,
        base.scope,
        None,
        estimated_cost(-1),
        EventId::new(),
        12,
        1,
        digest('e'),
    );
    assert_eq!(
        negative_estimate.unwrap_err(),
        "cost_allocation_estimated_amount_negative".to_owned()
    );
    let negative_measured = CostAllocation::new(
        CostAllocationId::new(),
        UsageId::new(),
        base.source_project_id,
        base.scope,
        None,
        AllocationCostKind::Measured {
            amount: Money::new("USD", -7).expect("money"),
            provider_receipt: ProviderReceiptRef::new("provider:receipt:opaque").expect("receipt"),
        },
        EventId::new(),
        12,
        1,
        digest('e'),
    );
    assert_eq!(
        negative_measured.unwrap_err(),
        "cost_allocation_measured_amount_negative".to_owned()
    );

    // 空/带 secret 的 provider receipt 连构造都被拒绝。
    assert!(ProviderReceiptRef::new("").is_err());
    assert!(ProviderReceiptRef::new("provider:secret:opaque").is_err());
}

/// 两类事实必须通过注册的事件契约；缺身份键或夹带额外字段都无法进入日志。
#[test]
fn cost_facts_are_registered_cost_allocation_events() {
    let fact = same_project_allocation();
    let payload = serde_json::to_value(&fact).expect("allocation json");
    assert_eq!(
        validate_event_payload(COST_ALLOCATION_EVENT, &payload),
        Ok(())
    );
    let event = RuntimeEvent::new(RequestId::new(), 3, COST_ALLOCATION_EVENT, payload.clone())
        .expect("event");
    assert_eq!(validate_runtime_event(&event), Ok(()));
    assert_eq!(event.data["run_id"], json!(fact.scope.run_id));
    assert_eq!(event.data["project_id"], json!(fact.scope.project_id));
    assert_eq!(
        event.data["organization_id"],
        json!(fact.scope.organization_id)
    );
    assert_eq!(event.data["workflow_id"], json!(fact.scope.workflow_id));
    assert_eq!(event.data["cell_id"], json!(fact.scope.cell_id));

    let mut missing_id = payload.clone();
    missing_id
        .as_object_mut()
        .expect("object")
        .remove("allocation_id");
    assert_eq!(
        validate_event_payload(COST_ALLOCATION_EVENT, &missing_id),
        Err("event_required_id_missing:allocation_id".to_owned())
    );

    let mut with_transcript = payload.clone();
    with_transcript.as_object_mut().expect("object").insert(
        "transcript".to_owned(),
        json!("model said the cost is zero"),
    );
    assert_eq!(
        validate_event_payload(COST_ALLOCATION_EVENT, &with_transcript),
        Err("event_payload_unknown_field".to_owned())
    );
}

/// 模型文本无法解码成 allocation：额外字段、未知 cost 变体和夹带金额全部失败。
#[test]
fn model_transcript_cannot_be_decoded_into_an_allocation_leaf() {
    let fact = same_project_allocation();
    let payload = serde_json::to_value(&fact).expect("allocation json");

    let mut with_transcript = payload.clone();
    with_transcript
        .as_object_mut()
        .expect("object")
        .insert("transcript".to_owned(), json!("I hereby charge 0"));
    assert!(
        serde_json::from_value::<CostAllocation>(with_transcript).is_err(),
        "transcript must not decode into an allocation"
    );

    let mut unknown_variant = payload.clone();
    unknown_variant["cost"] = json!({"model_text": {"amount": "0 USD"}});
    assert!(serde_json::from_value::<CostAllocation>(unknown_variant).is_err());

    // unknown 只能带原因；追加 amount 字段必须解码失败，绝不能变成 0 金额。
    let unknown_fact = same_project_leaf(
        UsageId::new(),
        AllocationCostKind::Unknown {
            reason: BillingUnknownReason::ResultUnknown,
        },
        EventId::new(),
        13,
        'c',
    );
    let mut smuggled_amount = serde_json::to_value(&unknown_fact).expect("unknown json");
    smuggled_amount["cost"] = json!({
        "unknown": {"reason": "result_unknown", "amount": {"currency": "USD", "micros": 0}}
    });
    assert!(serde_json::from_value::<CostAllocation>(smuggled_amount).is_err());
    let serialized = serde_json::to_string(&unknown_fact).expect("unknown text");
    assert!(!serialized.contains("\"amount\""));
    assert!(!serialized.to_ascii_lowercase().contains("transcript"));
}

/// 一条 allocation 只从一份 BQ-13 breakdown 复制，金额与 digest 不做二次计算。
#[test]
fn allocation_is_copied_from_one_breakdown_without_recalculation() {
    let run_id = RunId::new();
    let usage = usage(run_id);
    let card = card();
    let breakdown = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let scope = AllocationScope::new(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowInstanceId::new(),
        CellId::new(),
        run_id,
    )
    .expect("scope");
    let estimate_amount = breakdown.kind.amount().cloned().expect("estimate amount");
    let allocation = CostAllocation::from_cost_breakdown(
        CostAllocationId::new(),
        usage.usage_id,
        scope.project_id,
        scope,
        None,
        &breakdown,
        EventId::new(),
        21,
        1,
    )
    .expect("allocation");
    assert_eq!(
        allocation.cost.estimated_amount(),
        Some(&estimate_amount),
        "estimate must be copied, never recalculated"
    );
    assert_eq!(allocation.source_digest, breakdown.breakdown_digest);

    // 外层 run 与 breakdown 不一致时在事实成形前拒绝。
    let foreign_scope = AllocationScope::new(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowInstanceId::new(),
        CellId::new(),
        RunId::new(),
    )
    .expect("scope");
    assert_eq!(
        CostAllocation::from_cost_breakdown(
            CostAllocationId::new(),
            usage.usage_id,
            foreign_scope.project_id,
            foreign_scope,
            None,
            &breakdown,
            EventId::new(),
            22,
            1,
        )
        .unwrap_err(),
        "cost_allocation_breakdown_run_mismatch".to_owned()
    );

    // unknown breakdown 只保留原因与来源摘要，不产生金额。
    let unknown_breakdown = CostBreakdown::unknown(
        run_id,
        Some(usage.attempt_id),
        digest('d'),
        UsageConfidence::Known,
        BillingUnknownReason::RateCardMissing,
    )
    .expect("unknown breakdown");
    let unknown_allocation = CostAllocation::from_cost_breakdown(
        CostAllocationId::new(),
        UsageId::new(),
        scope.project_id,
        scope,
        None,
        &unknown_breakdown,
        EventId::new(),
        23,
        1,
    )
    .expect("unknown allocation");
    assert_eq!(
        unknown_allocation.cost.unknown_reason(),
        Some(BillingUnknownReason::RateCardMissing)
    );
    assert!(unknown_allocation.cost.estimated_amount().is_none());
    assert!(unknown_allocation.cost.measured_amount().is_none());
}

/// 成功列：一条叶子携带五个维度，estimated/measured/unknown 互斥且可序列化追溯。
#[test]
fn one_leaf_carries_every_dimension_for_parallel_rollups() {
    let scope = scope();
    let allocation = allocation_with(
        scope,
        scope.project_id,
        UsageId::new(),
        None,
        estimated_cost(37),
        EventId::new(),
        31,
        'a',
    );
    assert_eq!(allocation.validate(), Ok(()));
    assert_eq!(allocation.scope.run_id, scope.run_id);
    assert_eq!(allocation.scope.cell_id, scope.cell_id);
    assert_eq!(allocation.scope.workflow_id, scope.workflow_id);
    assert_eq!(allocation.scope.organization_id, scope.organization_id);
    assert_eq!(
        allocation.cost.estimated_amount().expect("estimate").micros,
        37
    );
    assert!(allocation.cost.measured_amount().is_none());
    assert!(allocation.cost.unknown_reason().is_none());

    let measured = allocation_with(
        scope,
        scope.project_id,
        UsageId::new(),
        None,
        AllocationCostKind::Measured {
            amount: Money::new("USD", 21).expect("money"),
            provider_receipt: ProviderReceiptRef::new("provider:receipt:opaque").expect("receipt"),
        },
        EventId::new(),
        32,
        'b',
    );
    assert!(matches!(measured.cost, AllocationCostKind::Measured { .. }));
    assert!(measured.cost.estimated_amount().is_none());

    let unknown = allocation_with(
        scope,
        scope.project_id,
        UsageId::new(),
        None,
        AllocationCostKind::Unknown {
            reason: BillingUnknownReason::ResultUnknown,
        },
        EventId::new(),
        33,
        'c',
    );
    assert_eq!(
        unknown.cost.unknown_reason(),
        Some(BillingUnknownReason::ResultUnknown)
    );
    assert!(unknown.cost.estimated_amount().is_none());
    assert!(unknown.cost.measured_amount().is_none());

    let mut ledger = CostAllocationLedger::default();
    for leaf in [allocation.clone(), measured.clone(), unknown.clone()] {
        ledger.append(leaf).expect("append");
    }
    assert_eq!(ledger.leaf_count(), 3);
    assert_eq!(ledger.source_cursor(), 33);
    let json = serde_json::to_value(&allocation).expect("allocation json");
    assert_eq!(json["schema"], json!(COST_ALLOCATION_SCHEMA));
    assert_eq!(json["version"], json!({"major": 1, "minor": 0}));
    assert_eq!(json["run_id"], json!(scope.run_id));
}

fn card() -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider:test",
        "model:test",
        "USD",
        Some(2),
        Some(3),
        1_000,
        Some(10_000),
        7,
        "bq19-fixture",
    )
    .expect("rate card");
    card.request_price = Some(5);
    card.tool_price = Some(7);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid rate card");
    card
}

fn usage(run_id: RunId) -> NormalizedUsage {
    NormalizedUsage::new(
        UsageId::new(),
        AttemptId::new(),
        run_id,
        Some("provider:test".to_owned()),
        "model:test",
        "route:test",
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(10),
            output_tokens: Some(4),
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        UsageConfidence::Known,
        None,
        "provider fixture",
        digest('a'),
    )
    .expect("usage")
}
