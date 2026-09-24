#[test]
fn int13_event_and_receipt_boundaries_fail_closed_on_secret_shapes() {
    let events = include_str!("../src/events.rs");
    let redaction = include_str!("../src/redaction.rs");
    let receipts = include_str!("../src/receipts.rs");
    let provider = include_str!("../../kiana-provider/src/response.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let fixture = include_str!("../../kiana-domain/tests/int13_secret_redaction.rs");

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
    ] {
        assert!(
            fixture.contains(marker),
            "INT-13 fixture marker missing: {marker}"
        );
    }
    for source in [events, redaction, receipts, provider, runner] {
        for forbidden in ["raw_headers", "raw_response_body", "secret_value"] {
            assert!(
                !source.contains(forbidden),
                "raw secret source marker: {forbidden}"
            );
        }
    }
}
