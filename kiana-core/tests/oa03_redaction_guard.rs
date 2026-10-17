#[test]
fn audit_export_uses_the_bounded_export_profile_before_rendering_any_format() {
    let export = include_str!("../src/audit_export.rs");
    let redaction = include_str!("../../kiana-domain/src/redaction.rs");

    for marker in [
        "RedactionSignal::Export",
        "encode_bounded_value(&profile, &raw)",
        "audit_export_record_changed",
        "AuditExportError::RedactionFailed",
        "for record in &redacted",
        "oa03_audit_export_rejects_secret_sentinel_without_returning_content",
    ] {
        assert!(
            export.contains(marker),
            "missing OA-03 export boundary: {marker}"
        );
    }
    for marker in [
        "contains_malformed_json_secret_key",
        "redaction_secret_sentinel_detected",
        "serde_json::from_str::<Value>(trimmed).is_err()",
    ] {
        assert!(
            redaction.contains(marker),
            "missing OA-03 malformed-JSON boundary: {marker}"
        );
    }
}

#[test]
fn runtime_stream_and_artifact_sinks_bind_the_channel_redaction_profile() {
    let redaction = include_str!("../../kiana-domain/src/redaction.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let output = include_str!("../../kiana-daemon/src/execution_output.rs");
    let control = include_str!("../../kiana-daemon/src/execution_control.rs");

    for marker in [
        "encode_bounded_text_for",
        "encode_bounded_value_for",
        "pub const fn for_scan_channel(",
        "pub fn for_channel(",
        "pub struct StreamingRedactionSink",
        "pub struct BoundedStreamTail",
        "redaction_stream_failed",
    ] {
        assert!(
            redaction.contains(marker),
            "missing OA-03 unified encoder marker: {marker}"
        );
    }

    // The model transcript stream is a published signal: it composes the profile-bound sink over the
    // base streaming redactor and never emits the raw text when the sink errors.
    for marker in [
        "StreamingRedactionSink::from_redactor",
        "StreamingRedactor::new()",
        "RedactionProfile::for_channel(SecretScanChannel::Transcript)",
        "let text = redactor.push(&text)?;",
    ] {
        assert!(
            runner.contains(marker),
            "missing OA-03 runner stream wiring: {marker}"
        );
    }

    // The shell preview sink applies the same bounded encoder before publishing to model/UI.
    for marker in [
        "encode_bounded_text_for(&profile, channel, &text)",
        "RedactionProfile::for_channel(channel)",
        "preview_profile_digest",
    ] {
        assert!(
            output.contains(marker),
            "missing OA-03 daemon preview wiring: {marker}"
        );
    }
    assert!(
        control.contains("redaction_profile_digest")
            && control.contains("execution_output::preview_profile_digest"),
        "missing OA-03 archived artifact profile digest"
    );
}
