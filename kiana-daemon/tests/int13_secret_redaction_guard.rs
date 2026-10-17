#[test]
fn int13_daemon_scans_provider_errors_and_process_channels_before_projection() {
    let connectors = include_str!("../src/connectors.rs");
    let domain_connectors = include_str!("../../kiana-domain/src/connectors.rs");
    let capabilities = include_str!("../src/harness_capabilities.rs");
    let output = include_str!("../src/execution_output.rs");
    let mcp = include_str!("../src/mcp_stdio.rs");
    let sandbox = include_str!("../src/harness_sandbox.rs");
    let daemon_host = include_str!("daemon_host.rs");
    let domain = include_str!("../../kiana-domain/src/redaction.rs");

    // The daemon health path must classify probe failures through the shared domain projection;
    // the scanner call itself lives in `kiana-domain/src/connectors.rs` after INT-09 moved the
    // classification there, so both files are pinned together.
    for marker in ["classify_connector_probe_error", "probe_error_redacted"] {
        assert!(
            connectors.contains(marker),
            "daemon connector marker missing: {marker}"
        );
    }
    for marker in [
        "project_redacted_error",
        "SecretScanChannel::Event",
        "redacted_connector_probe_error_code",
    ] {
        assert!(
            domain_connectors.contains(marker),
            "domain connector redaction marker missing: {marker}"
        );
    }
    for marker in [
        "stdout_metadata",
        "stderr_metadata",
        "render_capped_for",
        "output_budget",
        "env_clear",
    ] {
        assert!(
            capabilities.contains(marker),
            "shell channel marker missing: {marker}"
        );
    }
    for marker in [
        "safe_output_text_for",
        "render_capped_for",
        "SecretScanChannel::Stdout",
        "SecretScanChannel",
        "scan_secret_sentinels",
        "mod int13_channels",
        "stdout_and_stderr_previews_never_publish_a_credential_shape",
        "bounded_previews_keep_channel_labels_and_fall_back_to_a_placeholder",
    ] {
        assert!(
            output.contains(marker),
            "output channel marker missing: {marker}"
        );
    }
    // End-to-end daemon behaviour fixtures already cover argv/stdout/stderr/env projection.
    for marker in [
        "shell_secret_sentinels_do_not_reach_events_receipt_or_model_context",
        "split_secret_across_stream_deltas_never_reaches_stdout_events_receipt_or_model_context",
        "argv-sentinel",
        "stdout-sentinel",
        "stderr-sentinel",
        "env-sentinel",
    ] {
        assert!(
            daemon_host.contains(marker),
            "daemon host channel fixture missing: {marker}"
        );
    }
    for marker in ["stderr_digest", "stderr_truncated", "secret-bearing stderr"] {
        assert!(mcp.contains(marker), "MCP channel marker missing: {marker}");
    }
    for marker in [
        "--clearenv",
        "sandbox_env",
        "ANTHROPIC_API_KEY",
        "GITHUB_TOKEN",
    ] {
        assert!(
            sandbox.contains(marker),
            "environment fence marker missing: {marker}"
        );
    }
    for marker in [
        "scan_secret_sentinels",
        "scan_secret_value",
        "scan_secret_channels",
        "SecretSentinelKind::Jwt",
        "SecretSentinelKind::UrlUserinfo",
        "RedactedErrorProjection",
    ] {
        assert!(
            domain.contains(marker),
            "shared scanner marker missing: {marker}"
        );
    }

    for forbidden in [
        "raw_response_body",
        "provider_response_body",
        "secret_value",
    ] {
        assert!(
            !connectors.contains(forbidden),
            "raw provider field crossed daemon boundary: {forbidden}"
        );
    }
}
