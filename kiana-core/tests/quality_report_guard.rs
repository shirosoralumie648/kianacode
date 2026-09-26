#[test]
fn quality_report_uses_read_only_control_plane_adapter() {
    let domain = include_str!("../../kiana-domain/src/quality_report.rs");
    let core = include_str!("../src/quality_report.rs");
    let lib = include_str!("../src/lib.rs");
    for marker in [
        "QUALITY_REPORT_SCHEMA",
        "QualityReportArtifacts",
        "render_json",
        "render_junit",
        "render_human_summary",
        "render_evidence_manifest",
        "render_reproduction_command",
        "redact_report_text",
        "scan_secret_sentinels",
        "validate_quality_report",
        "QUALITY_REPORT_COMMAND",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || lib.contains(marker),
            "EQ-48 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "std::fs::",
        "publish_report",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "EQ-48 report adapter must not execute effects: {forbidden}"
        );
    }
}
