/// P4-J7-24 守卫：用量结算必须保持“值层”边界——它记录事实与估价，但不授权、不重试、
/// 不写事件，也不发明金额。本测试只做源码级标记检查，运行成本为零。
#[test]
fn usage_settlement_keeps_cost_and_authority_boundaries_separate() {
    let domain = include_str!("../../kiana-domain/src/provider_usage_settlement.rs");
    let pricing = include_str!("../../kiana-domain/src/billing_pricing.rs");
    let provider = include_str!("../../kiana-provider/src/usage.rs");
    for marker in [
        "SettlementCost::Unknown",
        "RateCard",
        "attach_measured",
        "provider_usage_attempt_conflict",
        "BillingState::Unknown",
        "provider_usage_run_mismatch",
        "from_usage_with_execution_id",
        "provider_usage_execution_id_invalid",
        // P4-J7-24 追加：协议包含关系、预留/消费/释放账本与迟到用量更正。
        "ProviderUsageContainment",
        "has_undeclared_overlap",
        "provider_usage_containment_unprovable",
        "provider_usage_containment_violated",
        "estimate_with_containment",
        "ProviderAttemptReservation",
        "ProviderUsageAttemptLedger",
        "ProviderChargeBasis",
        "provider_usage_budget_exceeded",
        "provider_usage_attempt_not_dispatched",
        "provider_usage_dispatched_attempt_cannot_release",
        "provider_usage_charge_exceeds_reservation",
        "ProviderUsageCorrection",
        "provider_usage_correction_target_mismatch",
        "RateCardSnapshot",
        "provider_usage_overflow",
        // 只有累计观测能结算：增量（delta）必须先经 accumulator 累加。
        "provider_usage_delta_observation_not_settleable",
    ] {
        assert!(
            domain.contains(marker) || pricing.contains(marker) || provider.contains(marker),
            "P4-J7-24 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "tokio::spawn",
        "reqwest::Client",
        "EventStore",
    ] {
        assert!(
            !domain.contains(forbidden)
                && !pricing.contains(forbidden)
                && !provider.contains(forbidden),
            "usage settlement boundary widened: {forbidden}"
        );
    }
    // 结算与账本的算术必须显式检查：回绕或饱和把超支伪装成正常值。
    for forbidden in ["wrapping_add", "wrapping_mul", "saturating_add"] {
        assert!(
            !domain.contains(forbidden),
            "usage settlement must not use lossy arithmetic: {forbidden}"
        );
    }
    // 未知用量只能按预留上限保守计费，不允许被折叠成零成本。
    assert!(
        domain.contains("ProviderChargeBasis::ReservedUpperBound"),
        "P4-J7-24 marker missing: ProviderChargeBasis::ReservedUpperBound"
    );
    assert!(
        domain.contains("unknown_cost_reason"),
        "P4-J7-24 marker missing: unknown_cost_reason"
    );
}
