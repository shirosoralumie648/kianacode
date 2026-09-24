use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn source(relative: &str) -> String {
    fs::read_to_string(root().join(relative)).unwrap_or_else(|error| panic!("{relative}: {error}"))
}

#[test]
fn operator_evidence_stays_read_only_and_redaction_bound() {
    let operator = source("kiana-core/src/operator_evidence.rs");
    let domain = source("kiana-domain/src/observability.rs");
    let receipts = source("kiana-core/src/receipts.rs");
    let daemon = source("kiana-daemon/src/lib.rs");
    let baseline = source("docs/roadmap/er30-observability-baseline.md");

    for marker in [
        "project_operator_evidence",
        "OperatorEvidenceSnapshot",
        "metric_snapshot_digest",
        "health_snapshot_digest",
        "trace_correlation_digests",
        "effect_success_claim",
        "telemetry_secret_scan_blocks_publish",
        "health_unknown_is_not_healthy",
    ] {
        assert!(
            operator.contains(marker) || domain.contains(marker),
            "missing {marker}"
        );
    }
    assert!(operator.contains("validate_secret_free"));
    assert!(operator.contains("ControlPlane"));
    assert!(daemon.contains("operator_evidence"));
    assert!(receipts.contains("receipt_projection_requires_health_probe"));
    assert!(receipts.contains("effect_success_claim"));
    assert!(!operator.contains("CapabilityBroker"));
    assert!(!operator.contains("Provider"));
    assert!(baseline.contains("feature_status=implemented"));
    assert!(baseline.contains("proof_level=source"));
    assert!(baseline.contains("does not prove"));
}
