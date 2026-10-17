//! CAP-25 source guard for receipt, result-pairing and four-entry parity.

#[path = "support/source_guard.rs"]
mod source_guard;

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        let present = if *marker == "stream_gap" {
            let subscribe = source_guard::item_source(source, "pub(crate) fn subscribe_after(");
            let code = source_guard::compact_code(&subscribe);
            code.contains("letgap=!same_epoch||after.is_some_and(|cursor|cursor.sequence>channel.sequence)||channel.sequence>last_sequence;")
                && code.contains("RunStreamSubscription{")
                && code.contains("gap,")
                && source_guard::item_source(source, "pub const fn has_gap(").contains("self.gap")
                && source_guard::item_source(source, "fn gap_frame(").contains("snapshot_required: true")
        } else {
            source.contains(marker)
        };
        assert!(present, "CAP-25 {label} marker missing: {marker}");
    }
}

#[test]
fn receipts_are_event_projections_with_owner_and_effect_boundaries() {
    let receipts = include_str!("../src/receipts.rs");
    let contracts = include_str!("../../kiana-domain/src/receipt_contracts.rs");
    let aggregation = include_str!("../../kiana-domain/src/receipt_aggregation.rs");
    let redaction = include_str!("../src/redaction.rs");
    let attempts = include_str!("../src/capability_attempt_projection.rs");
    let baseline = &[
        include_str!("../../docs/roadmap/cap25-receipt-entrypoint-baseline.md"),
        include_str!("../../docs/roadmap/capability.md"),
    ]
    .concat();

    require(
        receipts,
        &[
            "read_receipt",
            "receipt_owner_mismatch",
            "receipt_data_revoked",
            "run_terminal_conflict",
            "result_unknown",
            "receipt_from_events",
            "typed_run_receipt",
            "typed_execution_receipts",
            "execution_receipt_evidence",
            "descriptor_unavailable",
            "input_digest_unavailable",
            "scope_digest_unavailable",
            "plan_digest_unavailable",
            "files_changed",
            "cost_ledger",
            "invocations",
            "execution_receipts",
            "aggregation",
            "redact_event_value",
        ],
        "receipt projection",
    );
    require(
        contracts,
        &[
            "RunReceipt",
            "ExecutionReceipt",
            "ExecutionReceiptEvidence",
            "with_evidence",
            "descriptor_id",
            "descriptor_digest",
            "input_digest",
            "scope_digest",
            "plan_digest",
            "resource_usage_digest",
            "changes_digest",
            "limitations",
            "MAX_RECEIPT_LIMITATIONS",
            "source_event_ids",
            "action_digest",
            "effect_known",
            "stop_confirmed",
            "fenced",
            "redaction_profile",
            "proof_level",
        ],
        "receipt contract",
    );
    require(
        aggregation,
        &[
            "ReceiptAggregation",
            "usage_unknown",
            "files_changed",
            "evidence_ref_digests",
            "provider_receipt_refs",
        ],
        "aggregation",
    );
    // 全限定拼法只可能出现在调用点：receipt_aggregation.rs 里声明的是
    // 裸变体 Unknown。同文件另外两个标记已是这个形式。
    assert!(
        receipts.contains("AggregationVerification::Unknown"),
        "AggregationVerification::Unknown missing at its call site"
    );
    require(
        attempts,
        &[
            "request_id",
            "attempt",
            "action_digest",
            "effect_known",
            "source_event_ids",
        ],
        "attempt evidence",
    );
    require(
        redaction,
        &["redact_event_value", "redact_event_text"],
        "receipt redaction",
    );
    require(
        baseline,
        &[
            "late_progress_cannot_overwrite_terminal_outcome",
            "foreign_run_cannot_read_output_or_receipt",
            "invalid_adapter_result_cannot_be_rendered_as_success",
        ],
        "CAP-25 acceptance card",
    );
    assert!(!receipts.contains("EventLog::append"));
    assert!(!receipts.contains("CapabilityBroker::execute"));
}

#[test]
fn all_four_surfaces_share_parity_and_terminal_replay_without_execution() {
    let parity = include_str!("../src/parity.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/run_stream.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let entrypoint_guard = include_str!("er27_entrypoint_parity_guard.rs");
    let parity_fixture = include_str!("oa24_entrypoint_parity.rs");
    let protocol_fixture = include_str!("../../kiana-entrypoints/tests/cp22_protocol_surfaces.rs");
    let web_fixture = include_str!("../../kiana-entrypoints/tests/p2_m5_01_web_sync.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");

    require(
        parity,
        &[
            "project_entrypoint_parity",
            "EntryPointParitySnapshot",
            "EntryPointKind",
            "receipt_digest",
            "source_event_ids",
            "receipt_owner_mismatch",
            "data_revoked",
            "run_owner_mismatch",
        ],
        "parity projection",
    );
    require(
        protocol,
        &[
            "ReceiptRequest",
            "RunStreamEnvelope",
            "ResponseEnvelope",
            "RequestBody::Receipt",
        ],
        "protocol read model",
    );
    require(
        daemon,
        &[
            "RUN_STREAM_CAPACITY",
            "publish_terminal",
            "subscribe_after",
            "channel.terminal = Some",
            "stream_gap",
            "Terminal",
        ],
        "run stream",
    );
    require(
        client,
        &["receipt", "RequestEnvelope::receipt", "ResponseEnvelope"],
        "client",
    );
    require(
        entrypoint_guard,
        &[
            "CLI",
            "Web",
            "Workbench",
            "Desktop",
            "receipt_envelope_on_host",
        ],
        "ER-27 regression",
    );
    require(
        parity_fixture,
        &[
            "all_entrypoints_project_the_same_committed_facts",
            "receipt_digest",
            "source_cursor",
        ],
        "parity fixture",
    );
    require(
        protocol_fixture,
        &[
            "receipt authoritative",
            "subscribe_after",
            "terminal = Some(envelope.clone())",
        ],
        "protocol fixture",
    );
    require(
        web_fixture,
        &["stream_closed_before_terminal", "receipt authoritative"],
        "web fixture",
    );
    require(
        runner,
        &[
            "CapabilityResult",
            "request_id",
            "capability_result_mismatch",
        ],
        "result pairing",
    );
    for source in [parity, daemon, client] {
        for forbidden in [
            "CapabilityBroker::new",
            "auto_approve",
            "execute_authorized_request(",
        ] {
            assert!(
                !source_guard::production_source(source).contains(forbidden),
                "CAP-25 bypass marker present: {forbidden}"
            );
        }
    }
}

#[test]
fn behavioural_fixtures_are_wired_and_deny_first() {
    let core_fixtures = include_str!("../src/receipt_entrypoint_tests.rs");
    let domain_fixtures = include_str!("../../kiana-domain/tests/cap25_receipt_evidence.rs");
    let receipts = include_str!("../src/receipts.rs");
    let daemon = include_str!("../../kiana-daemon/src/run_stream.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let workflow = include_str!("../../.github/workflows/cap25-receipt-entrypoint.yml");

    // The card-named deny fixtures drive the production CLI-side read model and the real
    // run-stream bus rather than a parallel harness.
    for fixture in [
        "foreign_run_cannot_read_output_or_receipt",
        "invalid_adapter_result_cannot_be_rendered_as_success",
        "execution_receipt_projects_plan_evidence_without_inventing_metrics",
        "execution_receipt_limitations_stay_bounded_and_explicit",
        "read_receipt",
        "entrypoint_parity",
        "run_owner_mismatch",
    ] {
        assert!(
            core_fixtures.contains(fixture),
            "CAP-25 core fixture marker missing: {fixture}"
        );
    }
    for fixture in [
        "plan_evidence_is_bound_into_the_execution_receipt",
        "invalid_adapter_result_cannot_be_rendered_as_success",
        "capability_result_receipt_inconsistent",
        "adapter_result_unknown_not_fenced",
    ] {
        assert!(
            domain_fixtures.contains(fixture),
            "CAP-25 domain fixture marker missing: {fixture}"
        );
    }
    assert!(daemon.contains("late_progress_cannot_overwrite_terminal_outcome"));
    assert!(daemon.contains("reconnect_replays_terminal_without_reinvoking_runner_or_model"));
    assert!(daemon.contains("feed_terminal_delta_forbidden"));
    assert!(daemon.contains("feed_terminal_duplicate"));
    assert!(receipts.contains("receipt_entrypoint_tests.rs"));
    // The normalized request fact carries the canonical input digest used by receipt evidence.
    assert!(capabilities.contains("canonical_action_input_digest"));
    assert!(capabilities.contains("input_digest"));
    // The focused lane runs the fixtures that can execute on a stock Ubuntu runner.
    assert!(workflow.contains("cap25_receipt_evidence"));
    assert!(workflow.contains("kiana-daemon --lib run_stream::tests"));
}
