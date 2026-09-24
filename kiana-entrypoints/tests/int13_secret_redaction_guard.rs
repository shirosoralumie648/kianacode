#[test]
fn int13_workbench_projection_never_renders_unscanned_stream_text() {
    let render = include_str!("../src/workbench_render.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let fixture = include_str!("../../kiana-domain/tests/int13_secret_redaction.rs");

    for marker in [
        "redact_and_strip_controls",
        "redact_text",
        "scan_secret_sentinels",
        "SecretScanChannel::Transcript",
        "[REDACTED]",
        "MAX_TIMELINE_SCAN_BYTES",
    ] {
        assert!(
            render.contains(marker),
            "Workbench scan marker missing: {marker}"
        );
    }
    for marker in ["scan_secret_channels", "SecretScanChannel", "redact_text"] {
        assert!(
            protocol.contains(marker),
            "protocol scanner export missing: {marker}"
        );
    }
    for sentinel in [
        "INT13_TOKEN_SENTINEL",
        "INT13_API_KEY_SENTINEL",
        "INT13_ECHO_SENTINEL",
    ] {
        assert!(fixture.contains(sentinel));
    }
    for forbidden in ["raw_provider_body", "raw_access_token", "secret_value"] {
        assert!(
            !render.contains(forbidden),
            "UI raw secret marker: {forbidden}"
        );
    }
}
