#[test]
fn workbench_review_is_a_display_and_validation_boundary() {
    let source = include_str!("../src/workbench_review.rs");
    for marker in [
        "WorkbenchInboxCard",
        "reason",
        "scope",
        "expires_at_unix_ms",
        "fields",
        "allowed_decisions",
        "ArtifactRef",
        "artifact_page_digest_mismatch",
        "artifact_revision_mismatch",
        "ReceiptFile",
        "ReceiptCost",
        "limitations",
        "provenance",
        "ResultUnknown",
        "approval_expired",
        "approval_revoked",
        "client_payload_mutation",
        "ControlPlane",
    ] {
        assert!(source.contains(marker), "UI-15 marker missing: {marker}");
    }
    for forbidden in [
        "DaemonHost::",
        "CapabilityBroker",
        "KianaHarness",
        "std::fs",
        "tokio::",
        "reqwest::",
        "Command::new",
        "apply_patch",
        "fn execute",
        "fn approve",
    ] {
        assert!(
            !source.contains(forbidden),
            "display boundary leaked {forbidden}"
        );
    }
}
