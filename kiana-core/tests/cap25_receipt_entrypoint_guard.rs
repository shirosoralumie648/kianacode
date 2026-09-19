//! CAP-25 source guard for receipt, result-pairing and four-entry parity.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-25 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn receipts_are_event_projections_with_owner_and_effect_boundaries() {
    let receipts = include_str!("../src/receipts.rs");
    let contracts = include_str!("../../kiana-domain/src/receipt_contracts.rs");
    let aggregation = include_str!("../../kiana-domain/src/receipt_aggregation.rs");
    let redaction = include_str!("../src/redaction.rs");
    let attempts = include_str!("../src/capability_attempt_projection.rs");
    let baseline = include_str!("../../docs/roadmap/cap25-receipt-entrypoint-baseline.md");

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
            "AggregationVerification::Unknown",
        ],
        "aggregation",
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
                !source.contains(forbidden),
                "CAP-25 bypass marker present: {forbidden}"
            );
        }
    }
}
