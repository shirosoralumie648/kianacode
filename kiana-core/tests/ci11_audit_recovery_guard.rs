//! CI-11 source guard for audit/redaction/credential recovery projection boundaries.

#[path = "support/source_guard.rs"]
mod source_guard;
use source_guard::{compact_code, item_source, production_source};

#[test]
fn ci11_recovery_projection_requires_explicit_re_admission_and_audit_redaction_binding() {
    let recovery = include_str!("../../kiana-domain/src/credential_recovery_evidence.rs");
    let events = include_str!("../../kiana-domain/src/event_contracts.rs");
    let replay = include_str!("../src/credential_recovery.rs");
    let audit = include_str!("../src/audit_projection.rs");
    let redaction = include_str!("../src/redaction.rs");
    let blocker = item_source(recovery, "pub enum CredentialRecoveryBlocker");
    let admission = compact_code(&item_source(recovery, "pub fn explicit_re_admit("));
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
        "recovery.credential",
        "project_credential_recovery",
        "restart_after_index",
    ] {
        assert!(
            recovery.contains(marker)
                || events.contains(marker)
                || replay.contains(marker)
                || audit.contains(marker)
                || redaction.contains(marker),
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
            match marker {
                "unknown schema" =>
                    blocker.contains("UnknownSchema")
                        && recovery.contains("credential_recovery_event_schema_unknown"),
                "lease missing" => blocker.contains("LeaseMissing"),
                "credential refresh failure" => blocker.contains("CredentialRefreshFailed"),
                "explicit re-admission" =>
                    admission
                        .contains("fact.event!=CredentialRecoveryEventKind::ReAdmissionAuthorized")
                        && admission.contains("credential_recovery_admission_binding_mismatch"),
                "partial" =>
                    baseline.contains("source/local-fixture bounded")
                        && baseline.contains("not a durable")
                        && baseline.contains("no live provider"),
                _ => baseline.contains(marker),
            },
            "CI-11 baseline marker missing: {marker}"
        );
    }
    assert!(!production_source(recovery).contains("CapabilityBroker::new"));
}
