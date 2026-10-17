//! BQ-14 成本账本/修正的源码与接线守卫（先拒绝、后成功）。
//!
//! 守卫只在源码层钉住 marker、拒绝码矩阵、夹具名与 CI 接线：它不执行账本重放、不连接
//! EventStore、不消费审批，也不把 include_str! 命中当成运行时证据。

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/billing_ledger.rs");
const COMPAT_SOURCE: &str = include_str!("../../kiana-domain/src/cost_correction.rs");
const CORE_SOURCE: &str = include_str!("../src/cost_correction.rs");
const QUERY_SOURCE: &str = include_str!("../../kiana-query/src/cost_ledger_projector.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/bq14_cost_correction.rs");
const CORE_FIXTURES: &str = include_str!("bq14_cost_correction.rs");
const QUERY_FIXTURES: &str = include_str!("../../kiana-query/tests/bq14_cost_ledger_projector.rs");
const QUERY_GUARD: &str =
    include_str!("../../kiana-query/tests/bq14_cost_ledger_projector_guard.rs");
const PROTOCOL_FIXTURES: &str =
    include_str!("../../kiana-protocol/tests/bq14_cost_correction_wire.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq14-cost-correction.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

#[test]
fn ledger_entry_and_correction_contracts_keep_their_markers() {
    for marker in [
        "CostLedgerEntry",
        "CostLedgerEntryKind",
        "CostCorrectionLedger",
        "CostCorrectionCommand",
        "CostCorrectionApproval",
        "CostCorrectionAppendOutcome",
        "append_entry",
        "append_correction",
        "deny_unknown_fields",
        "cost_ledger_entry_digest_mismatch",
        "cost_ledger_entry_attempt_required",
        "cost_correction_target_binding_mismatch",
        "cost_correction_approval_binding_mismatch",
        "cost_correction_currency_mismatch",
        "cost_correction_command_replay_conflict",
        "cost_correction_idempotency_conflict",
        "COST_LEDGER_ENTRY_EVENT",
        "COST_CORRECTION_EVENT",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker),
            "BQ-14 domain marker missing: {marker}"
        );
    }
    // 兼容入口只重新导出同一份事实，不定义第二账本。
    assert!(COMPAT_SOURCE.contains("pub use crate::billing_ledger::"));
    for marker in [
        "CostCorrectionAdmission",
        "admit_cost_correction",
        "admit_cost_correction_with_approval",
        "authorize_with_approval_record",
        "ledger_entry_event",
        "correction_event",
        "RuntimeEvent",
        "ControlPlane",
    ] {
        assert!(
            CORE_SOURCE.contains(marker),
            "BQ-14 core marker missing: {marker}"
        );
    }
    for marker in [
        "project_cost_ledger",
        "CostLedgerProjection",
        "cost_ledger_projection_sequence_regression",
        "cost_ledger_projection_duplicate_source_event",
        "entry_identity_or_revision_mismatch",
        "correction_identity_or_revision_mismatch",
        "source_cursor",
    ] {
        assert!(
            QUERY_SOURCE.contains(marker),
            "BQ-14 query marker missing: {marker}"
        );
    }
}

#[test]
fn ledger_and_correction_sources_own_no_update_delete_or_second_authority() {
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
                "BQ-14 {name} authority widened: {forbidden}"
            );
        }
    }
    // 五个模型可见工具面不变：成本路径不注册工具、不新增 harness 入口。
    for forbidden in ["apply_patch", "memory.search", "memory.write", "ToolSchema"] {
        for source in [DOMAIN_SOURCE, CORE_SOURCE, QUERY_SOURCE] {
            assert!(
                !source.contains(forbidden),
                "BQ-14 ledger path gained a model tool surface: {forbidden}"
            );
        }
    }
}

#[test]
fn refusal_codes_and_fixture_names_stay_pinned_in_the_fixtures() {
    for code in [
        "cost_correction_approval_required",
        "cost_correction_evidence_required",
        "cost_correction_evidence_duplicate",
        "cost_correction_reason_invalid",
        "cost_correction_requested_by_invalid",
        "cost_idempotency_key_invalid",
        "cost_correction_delta_required",
        "cost_correction_zero_delta",
        "cost_correction_currency_mismatch",
        "cost_correction_target_binding_mismatch",
        "cost_correction_command_digest_mismatch",
        "cost_correction_approval_binding_mismatch",
        "cost_correction_conflicted_approver",
        "cost_correction_approval_digest_mismatch",
        "cost_correction_approval_header_invalid",
        "cost_correction_approval_command_digest_invalid",
        "cost_correction_approval_approver_invalid",
        "cost_correction_target_missing",
        "cost_correction_id_duplicate",
        "cost_correction_command_replay_conflict",
        "cost_correction_idempotency_conflict",
        "cost_ledger_entry_duplicate",
        "cost_ledger_source_event_duplicate",
        "cost_ledger_entry_header_invalid",
        "cost_ledger_entry_attempt_required",
        "cost_ledger_entry_currency_mismatch",
        "cost_ledger_entry_digest_mismatch",
        "cost_ledger_entry_source_digest_invalid",
        "cost_ledger_entry_rate_card_required",
        "cost_ledger_entry_rate_card_invalid",
        "cost_ledger_entry_provider_receipt_required",
        "cost_ledger_entry_provider_receipt_without_measured",
        "cost_ledger_entry_correction_amount_required",
        "event_required_id_missing:entry_id",
        "event_payload_unknown_field",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(code),
            "BQ-14 domain fixture refusal code missing: {code}"
        );
    }
    for code in [
        "cost_correction_approval_required",
        "cost_correction_approval_record_mismatch",
        "cost_correction_target_binding_mismatch",
        "cost_correction_reason_invalid",
        "stream_version_invalid",
        "cost_ledger_entry_digest_mismatch",
    ] {
        assert!(
            CORE_FIXTURES.contains(code),
            "BQ-14 core fixture refusal code missing: {code}"
        );
    }
    for code in [
        "cost_ledger_projection_sequence_regression",
        "cost_correction_target_missing",
        "cost_correction_approval_digest_mismatch",
        "cost_correction_target_binding_mismatch",
        "cost_correction_currency_mismatch",
        "correction_identity_or_revision_mismatch",
        "cost_ledger_projection_run_mismatch",
        "cost_ledger_projection_header_invalid",
    ] {
        assert!(
            QUERY_FIXTURES.contains(code),
            "BQ-14 query fixture refusal code missing: {code}"
        );
    }

    for name in [
        "unapproved_evidence_free_or_model_text_commands_are_refused",
        "target_fence_drift_is_refused_before_any_fact_forms",
        "approval_binding_requires_an_independent_approver_and_the_exact_subject",
        "ledger_is_append_only_and_refuses_duplicate_or_unknown_sources",
        "replayed_commands_return_the_original_fact_and_conflicting_digests_fail",
        "original_corrected_values_and_approver_stay_traceable",
        "ledger_entry_invariants_reject_forged_provenance",
        "ledger_entries_keep_breakdown_provenance_without_transcript_authority",
        "ledger_facts_are_registered_cost_ledger_events",
        "model_transcript_cannot_be_decoded_into_command_or_ledger_facts",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(name),
            "BQ-14 domain fixture missing: {name}"
        );
    }
    for name in [
        "admission_refuses_unapproved_stale_or_foreign_commands",
        "approval_record_must_bind_the_exact_command_digest_and_approver",
        "event_builders_are_append_only_stream_bound_and_validated",
        "control_plane_facade_admits_only_approved_bound_commands",
    ] {
        assert!(
            CORE_FIXTURES.contains(name),
            "BQ-14 core fixture missing: {name}"
        );
    }
    for name in [
        "projector_rebuilds_original_and_append_only_correction",
        "projector_rejects_foreign_run_and_duplicate_source_event",
        "projector_rejects_correction_before_its_target_and_sequence_regression",
        "projector_rejects_tampered_correction_payload_bindings",
        "projector_skips_unrelated_events_and_rejects_empty_or_nil_sources",
        "projection_validate_rejects_header_and_run_drift",
    ] {
        assert!(
            QUERY_FIXTURES.contains(name),
            "BQ-14 query fixture missing: {name}"
        );
    }
    for name in [
        "approved_correction_rides_the_shared_versioned_command_route",
        "unapproved_or_model_text_commands_are_refused_before_the_wire",
        "response_shape_is_digest_bound_and_rejects_drift",
    ] {
        assert!(
            PROTOCOL_FIXTURES.contains(name),
            "BQ-14 protocol fixture missing: {name}"
        );
    }

    for (name, source) in [
        ("domain", DOMAIN_FIXTURES),
        ("core", CORE_FIXTURES),
        ("query", QUERY_FIXTURES),
        ("query_guard", QUERY_GUARD),
        ("protocol", PROTOCOL_FIXTURES),
    ] {
        assert!(
            !source.contains("#[ignore"),
            "BQ-14 {name} fixtures must not be ignored"
        );
    }
}

#[test]
fn bq14_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards() {
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test bq14_cost_correction --locked -- --test-threads=1",
        "cargo test -p kiana-protocol --test bq14_cost_correction_wire --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq14_cost_correction --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq14_cost_correction_guard --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq14_cost_ledger_projector --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq14_cost_ledger_projector_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-14 manual lane missing: {command}"
        );
    }
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-14 manual lane must stay dispatch-only: {forbidden}"
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
        .any(|target| target.as_str() == Some("bq14_cost_correction")));
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    for target in ["bq14_cost_correction", "bq14_cost_correction_guard"] {
        assert!(
            core_targets
                .iter()
                .any(|candidate| candidate.as_str() == Some(target)),
            "BQ-14 core target missing from shard: {target}"
        );
    }
    // Query/protocol 与自动 lane 是整 crate 分片（targets = null），新夹具自动纳入。
    assert!(shards["kiana-query"]["targets"].is_null());
    assert!(shards["kiana-protocol"]["targets"].is_null());
}
