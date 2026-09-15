use kiana_domain::{json_digest, LiveHandoffManifest, LiveHandoffStatus, LiveHandoffTarget};
use serde_json::json;

fn manifest(status: LiveHandoffStatus) -> Result<LiveHandoffManifest, String> {
    LiveHandoffManifest::new(
        LiveHandoffTarget::Provider,
        "provider-account-staging",
        "staging",
        "secret-ref:managed/provider",
        json_digest(&json!({"provider":"fixture","model":"bounded"})),
        "git:fixture-source",
        Some("receipt:provider-1".to_owned()),
        Some("approval:operator-1".to_owned()),
        "audit-30d",
        None,
        "revoke credential, retain redacted receipt, delete workspace",
        vec![],
        status,
    )
}

#[test]
fn not_supported_is_explicit_and_does_not_enable_a_target() {
    let manifest = LiveHandoffManifest::new(
        LiveHandoffTarget::OtlpBackend,
        "otlp-none",
        "ci",
        "secret-ref:unused",
        json_digest(&json!({"endpoint":"none"})),
        "git:source",
        None,
        None,
        "none",
        None,
        "no external backend configured",
        vec!["external_backend_not_configured".to_owned()],
        LiveHandoffStatus::NotSupported,
    )
    .unwrap();
    assert_eq!(manifest.status, LiveHandoffStatus::NotSupported);
    manifest.validate().unwrap();
}

#[test]
fn verified_requires_independent_approval_receipt_and_secret_ref() {
    let verified = manifest(LiveHandoffStatus::Verified).unwrap();
    verified.validate().unwrap();

    let missing_approval = LiveHandoffManifest::new(
        LiveHandoffTarget::Provider,
        "provider-account",
        "staging",
        "secret-ref:managed/provider",
        json_digest(&json!({"provider":"fixture"})),
        "git:source",
        Some("receipt:1".to_owned()),
        None,
        "audit-30d",
        None,
        "revoke credential and retain receipt",
        vec![],
        LiveHandoffStatus::Verified,
    );
    assert_eq!(
        missing_approval.unwrap_err(),
        "live_handoff_verified_evidence_incomplete"
    );

    let raw_secret = LiveHandoffManifest::new(
        LiveHandoffTarget::Provider,
        "provider-account",
        "staging",
        "Bearer raw-secret-token",
        json_digest(&json!({"provider":"fixture"})),
        "git:source",
        Some("receipt:1".to_owned()),
        Some("approval:1".to_owned()),
        "audit-30d",
        None,
        "revoke credential and retain receipt",
        vec![],
        LiveHandoffStatus::OptedIn,
    );
    assert_eq!(
        raw_secret.unwrap_err(),
        "live_handoff_credential_ref_secret_or_invalid"
    );
}

#[test]
fn unknown_requires_limitation_and_digest_tampering_is_rejected() {
    let unknown = LiveHandoffManifest::new(
        LiveHandoffTarget::Connector,
        "connector-account",
        "staging",
        "secret-ref:managed/connector",
        json_digest(&json!({"connector":"fixture"})),
        "git:source",
        None,
        Some("approval:1".to_owned()),
        "audit-30d",
        Some("incident:1".to_owned()),
        "reconcile unknown effect before retry",
        vec!["provider_receipt_unavailable".to_owned()],
        LiveHandoffStatus::Unknown,
    )
    .unwrap();
    unknown.validate().unwrap();
    let mut encoded = serde_json::to_value(&unknown).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<LiveHandoffManifest>(encoded).is_err());

    let mut tampered = manifest(LiveHandoffStatus::Verified).unwrap();
    tampered.target_id = "other-account".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "live_handoff_manifest_digest_mismatch"
    );
}
