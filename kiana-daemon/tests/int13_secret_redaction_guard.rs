#[test]
fn int13_daemon_scans_provider_errors_and_process_channels_before_projection() {
    let connectors = include_str!("../src/connectors.rs");
    let capabilities = include_str!("../src/harness_capabilities.rs");
    let output = include_str!("../src/execution_output.rs");
    let mcp = include_str!("../src/mcp_stdio.rs");
    let sandbox = include_str!("../src/harness_sandbox.rs");
    let domain = include_str!("../../kiana-domain/src/redaction.rs");

    for marker in [
        "project_redacted_error",
        "SecretScanChannel::Event",
        "redacted_health_code",
        "probe_error_redacted",
    ] {
        assert!(
            connectors.contains(marker),
            "daemon connector marker missing: {marker}"
        );
    }
    for marker in [
        "stdout_metadata",
        "stderr_metadata",
        "redact_text",
        "EXEC_OUTPUT_MAX_BYTES",
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
        "SecretScanChannel::Stderr",
        "scan_secret_sentinels",
    ] {
        assert!(
            output.contains(marker),
            "output channel marker missing: {marker}"
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
