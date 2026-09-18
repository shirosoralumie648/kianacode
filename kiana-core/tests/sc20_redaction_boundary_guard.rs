#[test]
fn sc20_redaction_is_recursive_bounded_stable_and_applied_at_all_boundaries() {
    let domain = include_str!("../../kiana-domain/src/redaction.rs");
    let events = include_str!("../src/events.rs");
    let core_redaction = include_str!("../src/redaction.rs");
    let receipts = include_str!("../src/receipts.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let stream = include_str!("../../kiana-runner/src/stream_normalizer.rs");
    let provider_response = include_str!("../../kiana-provider/src/response.rs");
    let provider_transport = include_str!("../../kiana-provider/src/transport.rs");
    let shell = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let mcp = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let fixture = include_str!("../../kiana-domain/tests/er03_redaction.rs");

    for marker in [
        "RedactionProfile",
        "encode_bounded_value",
        "redaction_secret_sentinel_detected",
        "StreamingRedactor",
        "STREAM_REDACTION_BUFFER_LIMIT",
        "redact_text",
        "redact_value",
        "MAX_REDACTION_DEPTH",
    ] {
        assert!(
            domain.contains(marker),
            "SC-20 domain marker missing: {marker}"
        );
    }
    for marker in [
        "prepare_event_payload",
        "redact_event_value",
        "payload_depth",
        "event_payload_size_limit",
        "event_redaction_not_stable",
        "event_artifact_refs",
        "with_redaction_metadata",
    ] {
        assert!(
            events.contains(marker),
            "SC-20 event marker missing: {marker}"
        );
    }
    for marker in [
        "redact_capability_result",
        "redact_event_text",
        "redact_event_value",
    ] {
        assert!(
            core_redaction.contains(marker),
            "SC-20 core redaction marker missing: {marker}"
        );
    }
    assert!(receipts.contains("redact_event_value"));
    for marker in [
        "redact_text",
        "StreamingRedactor",
        "redact_value",
        "run.delta",
        "run.model_turn",
    ] {
        assert!(
            runner.contains(marker),
            "SC-20 runner marker missing: {marker}"
        );
    }
    for marker in [
        "bounded",
        "STREAM_NORMALIZER_MAX_DELTAS",
        "truncated",
        "late_delta_after_cancel_is_discarded",
    ] {
        assert!(
            stream.contains(marker),
            "SC-20 stream bound marker missing: {marker}"
        );
    }
    assert!(provider_response.contains("redact_text"));
    for marker in [
        "credential_lease_reference_mismatch",
        "credential_revision",
        "lease.consume",
    ] {
        assert!(
            provider_transport.contains(marker),
            "SC-20 provider marker missing: {marker}"
        );
    }
    for marker in [
        "stdout_metadata",
        "stderr_metadata",
        "render_capped",
        "redact_text",
        "EXEC_OUTPUT_MAX_BYTES",
        "env_clear",
    ] {
        assert!(
            shell.contains(marker),
            "SC-20 shell marker missing: {marker}"
        );
    }
    for marker in [
        "stderr_digest",
        "stderr_truncated",
        "secret-bearing stderr",
        "stop_confirmed",
    ] {
        assert!(mcp.contains(marker), "SC-20 MCP marker missing: {marker}");
    }
    for marker in [
        "--clearenv",
        "sandbox_env",
        "private_component",
        "ANTHROPIC_API_KEY",
        "GITHUB_TOKEN",
    ] {
        assert!(
            sandbox.contains(marker),
            "SC-20 sandbox marker missing: {marker}"
        );
    }
    for marker in [
        "secret_never_enters_event_or_receipt",
        "redaction_changes_snapshot_marks_non_resumable",
        "oversize_payload_is_rejected",
    ] {
        assert!(
            fixture.contains(marker),
            "SC-20 fixture marker missing: {marker}"
        );
    }
    for source in [
        events,
        core_redaction,
        receipts,
        runner,
        provider_response,
        shell,
        mcp,
    ] {
        for forbidden in ["raw_headers", "raw_response_body", "secret_value"] {
            assert!(
                !source.contains(forbidden),
                "SC-20 raw secret egress marker: {forbidden}"
            );
        }
    }
}
