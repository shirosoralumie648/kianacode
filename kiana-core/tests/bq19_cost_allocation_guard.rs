//! BQ-19 成本归属准入的源码与接线守卫（先拒绝、后成功）。
//!
//! 守卫只在源码层钉住 marker、拒绝码矩阵、夹具名与 CI 接线：它不执行准入、不连接 EventStore、
//! 不消费 SharingGrant，也不把 include_str! 命中当成运行时证据。

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/billing_allocation.rs");
const CORE_SOURCE: &str = include_str!("../src/billing_allocation.rs");
const QUERY_SOURCE: &str = include_str!("../../kiana-query/src/allocation_projector.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/bq19_cost_allocation.rs");
const CORE_FIXTURES: &str = include_str!("bq19_cost_allocation.rs");
const QUERY_FIXTURES: &str = include_str!("../../kiana-query/tests/bq19_allocation_projector.rs");
const QUERY_GUARD: &str =
    include_str!("../../kiana-query/tests/bq19_allocation_projector_guard.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq19-cost-allocation.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

#[test]
fn allocation_contracts_keep_their_markers_and_hardening() {
    for marker in [
        "CostAllocation",
        "AllocationScope",
        "AllocationCostKind",
        "SharingGrantRef",
        "CostAllocationAppendOutcome",
        "CostAllocationLedger",
        "cost_allocation_usage_already_charged",
        "cost_allocation_replay_conflict",
        "cost_allocation_source_event_duplicate",
        "cost_allocation_source_cursor_regression",
        "validate_grant_binding",
        "validate_wire_attribution",
        "cost_allocation_wire_scope_mismatch",
        "cost_allocation_wire_source_project_mismatch",
        "cost_allocation_wire_sharing_grant_mismatch",
        "cost_allocation_cross_project_sharing_grant_required",
        "cost_allocation_sharing_grant_reference_mismatch",
        "cost_allocation_sharing_grant_inactive",
        "cost_allocation_sharing_grant_operation_missing",
        "cost_allocation_estimated_amount_negative",
        "cost_allocation_measured_amount_negative",
        "COST_ALLOCATION_EVENT",
        "COST_ALLOCATION_OPERATION",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker),
            "BQ-19 domain marker missing: {marker}"
        );
    }
    for marker in [
        "CostAllocationAdmission",
        "authorize_wire",
        "authorize_wire_attribution",
        "authorize_sharing_grant",
        "admit_cost_allocation",
        "admit_cost_allocation_wire",
        "allocation_event",
        "RuntimeEvent",
        "with_stream_metadata",
        "ControlPlane",
    ] {
        assert!(
            CORE_SOURCE.contains(marker),
            "BQ-19 core marker missing: {marker}"
        );
    }
    // Core 只委托 Domain 的单一 grant 规则，不自己再实现一遍时效/操作名判断。
    assert!(CORE_SOURCE.contains("validate_grant_binding"));
    assert!(!CORE_SOURCE.contains("grant.active_at("));
}

#[test]
fn allocation_sources_own_no_update_delete_or_second_authority() {
    for forbidden in [
        "fn update",
        "fn delete",
        "fn replace",
        "fn mutate",
        "DELETE FROM",
        "UPDATE ",
        "set_amount",
        "CapabilityBrokerPort",
        "EventStorePort",
        "KeyringPort",
        "reqwest",
        "TcpStream",
        "std::fs",
        "std::net",
        "std::process",
        "unsafe",
        "#[ignore",
        "tokio::spawn",
        "ModelOutput",
        "FinancialBudget",
        "Uuid::new_v4",
    ] {
        for (name, source) in [
            ("domain", DOMAIN_SOURCE),
            ("core", CORE_SOURCE),
            ("query", QUERY_SOURCE),
        ] {
            assert!(
                !source.contains(forbidden),
                "BQ-19 {name} authority widened: {forbidden}"
            );
        }
    }
    // 五个模型可见工具面不变：成本归属路径不注册工具、不新增 harness 入口。
    for forbidden in ["apply_patch", "memory.search", "memory.write", "ToolSchema"] {
        for source in [DOMAIN_SOURCE, CORE_SOURCE, QUERY_SOURCE] {
            assert!(
                !source.contains(forbidden),
                "BQ-19 allocation path gained a model tool surface: {forbidden}"
            );
        }
    }
}

#[test]
fn refusal_codes_and_fixture_names_stay_pinned_in_the_fixtures() {
    for code in [
        "cost_allocation_wire_scope_mismatch",
        "cost_allocation_wire_source_project_mismatch",
        "cost_allocation_wire_sharing_grant_mismatch",
        "cost_allocation_usage_already_charged",
        "cost_allocation_replay_conflict",
        "cost_allocation_source_event_duplicate",
        "cost_allocation_source_cursor_regression",
        "cost_allocation_cross_project_sharing_grant_required",
        "cost_allocation_sharing_grant_project_mismatch",
        "cost_allocation_sharing_grant_reference_mismatch",
        "cost_allocation_sharing_grant_inactive",
        "cost_allocation_sharing_grant_operation_missing",
        "cost_allocation_same_project_grant_unexpected",
        "cost_allocation_header_invalid",
        "cost_allocation_scope_invalid",
        "cost_allocation_source_digest_invalid",
        "cost_allocation_digest_invalid",
        "cost_allocation_digest_mismatch",
        "cost_allocation_rate_card_invalid",
        "cost_allocation_estimated_amount_negative",
        "cost_allocation_measured_amount_negative",
        "cost_allocation_breakdown_run_mismatch",
        "event_required_id_missing:allocation_id",
        "event_payload_unknown_field",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(code),
            "BQ-19 domain fixture refusal code missing: {code}"
        );
    }
    for code in [
        "cost_allocation_wire_scope_mismatch",
        "cost_allocation_wire_source_project_mismatch",
        "cost_allocation_wire_sharing_grant_mismatch",
        "cost_allocation_server_scope_mismatch",
        "cost_allocation_server_source_project_mismatch",
        "same_project_grant_unexpected",
        "cross_project_sharing_grant_required",
        "cost_allocation_sharing_grant_reference_mismatch",
        "cost_allocation_sharing_grant_inactive",
        "cost_allocation_sharing_grant_operation_missing",
        "cost_allocation_digest_mismatch",
    ] {
        assert!(
            CORE_FIXTURES.contains(code),
            "BQ-19 core fixture refusal code missing: {code}"
        );
    }
    for code in [
        "cost_allocation_leaf_usage_repeated",
        "cost_allocation_projection_duplicate_source_event",
        "cost_allocation_projection_sequence_regression",
        "cost_allocation_identity_or_revision_mismatch",
        "cost_allocation_currency_mismatch",
        "cost_allocation_total_overflow",
        "cost_allocation_projection_header_invalid",
        "cost_allocation_projection_source_event_duplicate",
        "cost_allocation_projection_totals_recompute_mismatch",
        "cost_allocation_unknown_count_invalid",
        "cost_allocation_portfolio_digest_mismatch",
        "cost_allocation_portfolio_total_mismatch",
    ] {
        assert!(
            QUERY_FIXTURES.contains(code),
            "BQ-19 query fixture refusal code missing: {code}"
        );
    }

    for name in [
        "wire_reported_attribution_never_grants_ownership",
        "one_usage_leaf_is_charged_once_whatever_the_dimension_labels_say",
        "cross_project_requires_an_exact_active_operation_bound_grant",
        "allocation_header_provenance_and_cost_state_are_fenced",
        "cost_facts_are_registered_cost_allocation_events",
        "model_transcript_cannot_be_decoded_into_an_allocation_leaf",
        "allocation_is_copied_from_one_breakdown_without_recalculation",
        "one_leaf_carries_every_dimension_for_parallel_rollups",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(name),
            "BQ-19 domain fixture missing: {name}"
        );
    }
    for name in [
        "admission_refuses_wire_reported_attribution_and_server_binding_drift",
        "sharing_grant_admission_requires_an_exact_active_operation_bound_grant",
        "allocation_events_are_append_only_stream_bound_and_validated",
        "control_plane_facade_admits_only_bound_allocations",
    ] {
        assert!(
            CORE_FIXTURES.contains(name),
            "BQ-19 core fixture missing: {name}"
        );
    }
    for name in [
        "projector_refuses_a_second_charge_for_one_usage_leaf",
        "projector_rejects_identity_metadata_and_payload_drift",
        "projector_rejects_mixed_currency_and_overflowing_totals",
        "projected_dimension_rollups_are_parallel_views_of_one_ledger_total",
        "estimated_measured_and_unknown_stay_separate_without_zero_faking",
        "portfolio_view_is_recomputable_from_the_same_leaf_set",
        "projection_validate_rejects_header_source_and_recompute_drift",
        "projector_skips_unrelated_events_and_rejects_empty_or_nil_sources",
    ] {
        assert!(
            QUERY_FIXTURES.contains(name),
            "BQ-19 query fixture missing: {name}"
        );
    }

    for (name, source) in [
        ("domain", DOMAIN_FIXTURES),
        ("core", CORE_FIXTURES),
        ("query", QUERY_FIXTURES),
    ] {
        assert!(
            !source.contains("#[ignore"),
            "BQ-19 {name} fixtures must not be ignored"
        );
    }
    // 两个 guard 自身必须保留 `#[ignore` 负断言文本，因此只对 query guard 与 lane 检查
    // `--ignored`，不把“fixture 被禁用”的负断言当成禁用。
    for (name, source) in [("query_guard", QUERY_GUARD), ("manual_lane", MANUAL_LANE)] {
        assert!(
            !source.contains("--ignored"),
            "BQ-19 {name} must not skip fixtures"
        );
    }
}

#[test]
fn bq19_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards() {
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test bq19_cost_allocation --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq19_cost_allocation --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq19_cost_allocation_guard --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq19_allocation_projector --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq19_allocation_projector_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-19 manual lane missing: {command}"
        );
    }
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-19 manual lane must stay dispatch-only: {forbidden}"
        );
    }
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("permissions:"));

    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("shard registry json");
    let domain_targets = shards["kiana-domain-s1/4"]["targets"]
        .as_array()
        .expect("domain shard targets");
    assert!(domain_targets
        .iter()
        .any(|target| target.as_str() == Some("bq19_cost_allocation")));
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    for target in ["bq19_cost_allocation", "bq19_cost_allocation_guard"] {
        assert!(
            core_targets
                .iter()
                .any(|candidate| candidate.as_str() == Some(target)),
            "BQ-19 core target missing from shard: {target}"
        );
    }
    // Query 与自动 lane 是整 crate 分片（targets = null），新夹具自动纳入。
    assert!(shards["kiana-query"]["targets"].is_null());
}
