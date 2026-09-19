//! CI-11 source guard for audit/redaction/credential recovery projection boundaries.

#[test]
fn ci11_recovery_projection_requires_explicit_re_admission_and_audit_redaction_binding() {
    let recovery = include_str!("../../kiana-domain/src/credential_recovery_evidence.rs");
    let audit = include_str!("../src/audit_projection.rs");
    let redaction = include_str!("../src/redaction.rs");
    let baseline = include_str!("../../docs/roadmap/ci11-audit-recovery-baseline.md");
    for marker in [
        "CredentialRecoveryProjection",
        "CredentialRecoveryBlocker",
        "credential_recovery_auto_resume_forbidden",
        "credential_recovery_unknown_reason_missing",
        "audit_projection_digest",
        "redaction_profile_digest",
        "explicit_re_admission",
        "lease_state",
    ] {
        assert!(
            recovery.contains(marker) || audit.contains(marker) || redaction.contains(marker),
            "CI-11 source marker missing: {marker}"
        );
    }
    for marker in [
        "CI-11",
        "unknown schema",
        "stale epoch",
        "lease missing",
        "credential refresh failure",
        "redaction",
        "audit",
        "explicit re-admission",
        "partial",
        "durable",
    ] {
        assert!(
            baseline.contains(marker),
            "CI-11 baseline marker missing: {marker}"
        );
    }
    assert!(!recovery.contains("CapabilityBroker::new"));
}
