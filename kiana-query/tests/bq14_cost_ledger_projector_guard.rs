//! BQ-14 Query 账本 projector 的静态源码边界与接线守卫。
//!
//! `include_str!` 检查 projector 的只读投影 marker、稳定拒绝码、夹具名与手动 lane 接线，
//! 并确认该文件未直接依赖若干写入/执行接口。它不执行账本重放，也不扫描整个 workspace
//! 的副作用调用。

const PROJECTOR: &str = include_str!("../src/cost_ledger_projector.rs");
const FIXTURES: &str = include_str!("bq14_cost_ledger_projector.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq14-cost-correction.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

#[test]
fn query_projector_is_read_only_and_digest_bound() {
    // 这些 marker 仅说明预期文本留在 Query projector 源码中。
    for marker in [
        "project_cost_ledger",
        "append_correction",
        "source_cursor",
        "cost_ledger_projection_sequence_regression",
        "cost_ledger_projection_duplicate_source_event",
        "entry_identity_or_revision_mismatch",
        "correction_identity_or_revision_mismatch",
    ] {
        assert!(
            PROJECTOR.contains(marker),
            "BQ-14 projector marker missing: {marker}"
        );
    }
    for forbidden in [
        "EventStorePort",
        "CapabilityBrokerPort",
        "ModelOutput",
        "append_event(",
        "commit_transition",
        "std::fs",
        "std::net",
        "reqwest",
        "unsafe",
        concat!("#[", "ignore"),
        "Uuid::new_v4",
    ] {
        assert!(
            !PROJECTOR.contains(forbidden),
            "BQ-14 projector gained write/execution authority: {forbidden}"
        );
    }
}

#[test]
fn query_fixtures_stay_deny_first_and_wired_to_the_lane() {
    for name in [
        "projector_rebuilds_original_and_append_only_correction",
        "projector_rejects_foreign_run_and_duplicate_source_event",
        "projector_rejects_correction_before_its_target_and_sequence_regression",
        "projector_rejects_tampered_correction_payload_bindings",
        "projector_skips_unrelated_events_and_rejects_empty_or_nil_sources",
        "projection_validate_rejects_header_and_run_drift",
    ] {
        assert!(
            FIXTURES.contains(name),
            "BQ-14 query fixture missing: {name}"
        );
    }
    for code in [
        "cost_ledger_projection_sequence_regression",
        "cost_correction_target_missing",
        "cost_correction_approval_digest_mismatch",
        "correction_identity_or_revision_mismatch",
        "cost_ledger_projection_run_mismatch",
        "cost_ledger_projection_header_invalid",
    ] {
        assert!(
            FIXTURES.contains(code),
            "BQ-14 query refusal code missing: {code}"
        );
    }
    assert!(!FIXTURES.contains(concat!("#[", "ignore")));

    assert!(MANUAL_LANE.contains(
        "cargo test -p kiana-query --test bq14_cost_ledger_projector --locked -- --test-threads=1"
    ));
    assert!(MANUAL_LANE.contains(
        "cargo test -p kiana-query --test bq14_cost_ledger_projector_guard --locked -- --test-threads=1"
    ));
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(!MANUAL_LANE.contains("push:"));
    // Query 与 protocol 是整 crate 分片，夹具随 crate 一起在自动 lane 运行。
    assert!(SHARDS.contains("\"kiana-query\""));
    assert!(SHARDS.contains("\"kiana-protocol\""));
}
