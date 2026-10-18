//! BQ-20 ledger projector 的源码/接线守卫（先拒绝、后成功）。
//!
//! 守卫只在源码层钉住 marker、拒绝码矩阵、夹具名与 CI 接线：`include_str!` 把 projector、
//! Domain/Core/EventLog 合同与夹具读作文本，确认没有任何一处被改写成可写账本、第二授权面或
//! 重复的重放链实现。它不运行投影，也不证明其他模块没有调用副作用接口。

const PROJECTOR_SOURCE: &str = include_str!("../src/billing_ledger_projector.rs");
const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/billing_projection.rs");
const CORE_SOURCE: &str = include_str!("../../kiana-core/src/billing_projection.rs");
const EVENTLOG_SOURCE: &str = include_str!("../../kiana-eventlog/src/ledger_source.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/bq20_ledger_projection.rs");
const EVENTLOG_FIXTURES: &str = include_str!("../../kiana-eventlog/tests/bq20_ledger_source.rs");
const QUERY_FIXTURES: &str = include_str!("bq20_ledger_projector.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq20-ledger-projector.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

#[test]
fn billing_projector_is_read_only_and_shares_one_replay_chain() {
    // 所有 marker 必须保留在同一投影源文件中；这里检查名称存在，不检查每个 rollup 的数值。
    for marker in [
        "BillingLedgerProjector",
        "BillingProjectionSnapshot",
        "BillingProjectionCursor",
        "BillingQuarantineRecord",
        "source_cursor",
        "source_epoch",
        "replay_fence",
        "kiana_domain::billing_replay_fence",
        "quarantine",
        "correction_stream_regression",
        "correction_stream_gap",
        "utc_day_key",
        "BILLING_WINDOW_MILLIS",
        "BILLING_LEDGER_PROJECTOR_IS_READ_ONLY",
        "BILLING_PROJECTOR_NO_FACT_WRITES",
    ] {
        assert!(
            PROJECTOR_SOURCE.contains(marker),
            "BQ-20 projector marker missing: {marker}"
        );
    }
    // 这些字符串只构成单文件的静态负向约束，不能替代 workspace-wide 权限或调用图审计。
    for forbidden in [
        "EventStorePort",
        "CapabilityBrokerPort",
        "commit_transition",
        "append(",
        "fn release",
        "fn reserve",
        "fn update",
        "fn delete",
        "fn replace",
        "fn mutate",
        "ApprovalStore",
        "tokio::spawn",
        "reqwest",
        "std::fs",
        "std::net",
        "std::process",
        "ModelOutput",
        "FinancialBudget",
        "Uuid::new_v4",
        "unsafe",
        concat!("#[", "ignore"),
    ] {
        assert!(
            !PROJECTOR_SOURCE.contains(forbidden),
            "projection boundary widened: {forbidden}"
        );
    }
    // 重放链只有 Domain 一份实现：查询投影器与 ControlPlane fence 都调用它，不各自复制算法。
    for marker in [
        "pub fn billing_replay_fence",
        "pub fn billing_source_event_id_tail",
        "BillingProjectionFreshness",
        "quarantine_for_cursor",
        "quarantine_for_event",
        "quarantine_by_payload_digest",
        "billing_projection_stale",
        "billing_projection_cursor_ahead",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker),
            "BQ-20 domain rule missing: {marker}"
        );
    }
    assert!(CORE_SOURCE.contains("billing_replay_fence"));
    assert!(CORE_SOURCE.contains("billing_source_event_id_tail"));
    assert!(CORE_SOURCE.contains("billing_projection_source_fence_mismatch"));
    assert!(!CORE_SOURCE.contains("std::collections::BTreeMap"));
    // EventLog 适配器仍然是唯一读取入口，且它自己不做重放链/聚合。
    for marker in [
        "read_billing_page",
        "BillingSourcePage",
        "BILLING_SOURCE_IS_READ_ONLY",
    ] {
        assert!(
            EVENTLOG_SOURCE.contains(marker),
            "BQ-20 eventlog marker missing: {marker}"
        );
    }
    assert!(!EVENTLOG_SOURCE.contains("billing_replay_fence"));
}

#[test]
fn refusal_codes_and_fixture_names_stay_pinned_in_the_fixtures() {
    for code in [
        "usage_cost_decode_failed",
        "usage_cost_kind_mismatch",
        "usage_cost_attempt_missing",
        "usage_cost_stream_mismatch",
        "usage_cost_revision_gap",
        "usage_cost_transition_invalid",
        "ledger_entry_stream_mismatch",
        "ledger_entry_duplicate",
        "correction_stream_mismatch",
        "correction_target_missing",
        "correction_target_mismatch",
        "correction_duplicate",
        "correction_stream_regression",
        "correction_stream_gap",
        "allocation_stream_mismatch",
        "allocation_usage_duplicate",
        "billing_projection_source_epoch_invalid",
        "billing_projection_source_cursor_invalid",
        "billing_projection_source_cursor_gap",
        "billing_projection_source_replay",
        "billing_projection_source_event_duplicate",
        "billing_quarantine_limit",
        "billing_projection_snapshot_requires_rebuild",
    ] {
        assert!(
            QUERY_FIXTURES.contains(code),
            "BQ-20 query fixture refusal code missing: {code}"
        );
    }

    for name in [
        "rebuild_keeps_usage_and_ledger_separate_and_rolls_daily_window",
        "malformed_cost_is_quarantined_but_reservation_is_not_consumed",
        "failed_page_does_not_advance_cursor_and_replay_is_rejected",
        "snapshot_rebuild_failure_does_not_advance_cursor",
        "cursor_gap_and_empty_page_regression_are_refused",
        "quarantine_keeps_bad_facts_queryable_by_cursor_event_and_payload_digest",
        "quarantine_limit_fails_the_page_without_publishing",
        "projector_rejects_nil_and_empty_sources",
        "correction_stream_versions_must_follow_the_ledger_stream",
        "usage_revisions_fold_to_the_latest_and_timestamp_less_facts_stay_unbucketed",
        "allocation_views_do_not_multiply_ledger_totals",
        "new_process_rebuilds_the_same_snapshot_digest",
    ] {
        assert!(
            QUERY_FIXTURES.contains(name),
            "BQ-20 query fixture missing: {name}"
        );
    }

    for (name, source) in [
        ("domain", DOMAIN_FIXTURES),
        ("eventlog", EVENTLOG_FIXTURES),
        ("query", QUERY_FIXTURES),
    ] {
        assert!(
            !source.contains("#[ignore"),
            "BQ-20 {name} fixtures must not be ignored"
        );
    }
    assert!(
        !MANUAL_LANE.contains(concat!("--", "ignored")),
        "BQ-20 manual lane must not skip fixtures"
    );
}

#[test]
fn bq20_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards() {
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test bq20_ledger_projection --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq20_ledger_projector_guard --locked -- --test-threads=1",
        "cargo test -p kiana-eventlog --test bq20_ledger_source --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq20_ledger_projector --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq20_ledger_projector_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-20 manual lane missing: {command}"
        );
    }
    for forbidden in ["push:", "pull_request:", "continue-on-error"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-20 manual lane must stay dispatch-only: {forbidden}"
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
        .any(|target| target.as_str() == Some("bq20_ledger_projection")));
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    assert!(core_targets
        .iter()
        .any(|target| target.as_str() == Some("bq20_ledger_projector_guard")));
    // EventLog 与 Query 是整 crate 分片（targets = null），新夹具自动纳入。
    assert!(shards["kiana-eventlog"]["targets"].is_null());
    assert!(shards["kiana-query"]["targets"].is_null());
}
