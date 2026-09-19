//! CAP-13 source guard for one bounded output/redaction/artifact boundary.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-13 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn output_capture_redaction_and_artifact_cursor_share_one_budget() {
    let domain = include_str!("../../kiana-domain/src/execution_output.rs");
    let output = include_str!("../../kiana-daemon/src/execution_output.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let execution = include_str!("../../kiana-daemon/src/execution_control.rs");

    require(
        domain,
        &[
            "ExecutionOutputBudget",
            "collect_max_bytes",
            "preview_max_bytes",
            "persist_max_bytes",
            "observed_max_bytes",
            "max_lines",
            "execution_output_budget_invalid",
        ],
        "domain budget",
    );
    require(
        output,
        &[
            "read_capped",
            "drain_capped",
            "safe_output_text",
            "bounded_preview",
            "output_total_limit",
            "output_line_limit",
            "output_reader_failed",
            "output_drain_timeout",
            "metadata",
        ],
        "daemon output",
    );
    require(
        harness,
        &[
            "ExecutionOutputBudget",
            "read_capped",
            "drain_capped",
            "render_capped",
            "stdout_metadata",
            "stderr_metadata",
        ],
        "shell output",
    );
    require(
        execution,
        &[
            "ExecutionOutputRef",
            "output_persist_quota_exceeded",
            "output_cursor_invalid",
            "output_content_digest_mismatch",
            "output_data_revoked",
            "data_epoch",
            "execution_output::safe_output_text",
        ],
        "long-running artifact",
    );
    assert!(!harness.contains("async fn read_capped"));
    assert!(!harness.contains("pub(crate) fn safe_output_text"));
}

#[test]
fn output_boundary_redacts_control_and_secret_markers_before_projection() {
    let output = include_str!("../../kiana-daemon/src/execution_output.rs");
    for marker in [
        "redact_text",
        "from_utf8_lossy",
        "\\u{1b}",
        "preview-truncated",
    ] {
        assert!(
            output.contains(marker),
            "CAP-13 projection marker missing: {marker}"
        );
    }
}
