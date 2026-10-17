#[test]
fn int13_event_and_receipt_boundaries_fail_closed_on_secret_shapes() {
    let events = include_str!("../src/events.rs");
    let redaction = include_str!("../src/redaction.rs");
    let receipts = include_str!("../src/receipts.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/event_store_core.rs");
    let provider = include_str!("../../kiana-provider/src/response.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let fixture = include_str!("../../kiana-domain/tests/int13_secret_redaction.rs");
    let eventlog_fixture = include_str!("../../kiana-eventlog/tests/int13_event_redaction.rs");

    for marker in [
        "prepare_event_payload",
        "scan_secret_value",
        "SecretScanChannel::Event",
        "event_redaction_not_stable",
    ] {
        assert!(
            events.contains(marker),
            "event boundary marker missing: {marker}"
        );
    }
    for marker in [
        "SecretScanChannel::Receipt",
        "secret_redaction_failed",
        "redact_capability_result",
    ] {
        assert!(
            redaction.contains(marker),
            "receipt boundary marker missing: {marker}"
        );
    }
    for marker in [
        "redact_event_value",
        "receipt_from_events",
        "result_unknown",
    ] {
        assert!(
            receipts.contains(marker),
            "receipt projection marker missing: {marker}"
        );
    }
    for marker in [
        "validate_secret_free",
        "scan_secret_value_with_limit",
        "SecretScanChannel::Event",
        "MAX_JOURNAL_EVENT_BYTES",
        "eventlog_",
    ] {
        assert!(
            eventlog.contains(marker),
            "EventStore storage boundary marker missing: {marker}"
        );
    }
    for marker in [
        "event_store_refuses_shapes_that_redaction_alone_cannot_rewrite",
        "redacted_event_payloads_round_trip_without_retaining_the_candidate",
        "the_storage_boundary_keeps_both_secret_checks_wired",
        "SK_SENTINEL",
        "PROXY_SENTINEL",
    ] {
        assert!(
            eventlog_fixture.contains(marker),
            "INT-13 EventStore fixture marker missing: {marker}"
        );
    }
    assert!(provider.contains("redact_text"));
    for marker in [
        "safe_channel_text",
        "SecretScanChannel::Prompt",
        "SecretScanChannel::Transcript",
        "scan_secret_sentinels",
    ] {
        assert!(
            runner.contains(marker),
            "runner channel marker missing: {marker}"
        );
    }
    for marker in [
        "Prompt",
        "Transcript",
        "Event",
        "Receipt",
        "Stdout",
        "Stderr",
        "Argv",
        "Env",
        "Cache",
        "INT13_ECHO_SENTINEL",
        "provider_error_redacted",
        "every_channel_refuses_every_credential_shape_without_retaining_the_candidate",
        "structured_secret_keys_nul_bytes_and_token_metrics_bind_to_every_channel",
        "echo_sentinel_sets_are_bounded_and_findings_never_embed_the_sentinel",
        "provider_raw_errors_are_projected_to_code_and_digest_only",
    ] {
        assert!(
            fixture.contains(marker),
            "INT-13 fixture marker missing: {marker}"
        );
    }
    for source in [events, redaction, receipts, eventlog, provider, runner] {
        for forbidden in [
            "raw_headers",
            "raw_response_body",
            "\"secret_value\"",
            "secret_value:",
        ] {
            assert!(
                !source.contains(forbidden),
                "raw secret source marker: {forbidden}"
            );
        }
    }
}
