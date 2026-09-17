use kiana_domain::{
    classify_security_reason, SecurityReason, SecurityReasonClass, SecurityReasonCode,
    SecurityRemediation, SecurityRetryability, SECURITY_REASON_SCHEMA,
};
use serde_json::json;

#[test]
fn security_reason_codes_have_stable_families_and_policies() {
    let denied = SecurityReasonCode::AuthCallerUntrusted;
    assert_eq!(denied.as_str(), "AUTH_CALLER_UNTRUSTED");
    assert_eq!(denied.policy().class, SecurityReasonClass::Auth);
    assert_eq!(denied.policy().retryability, SecurityRetryability::Never);
    assert_eq!(
        denied.policy().remediation,
        SecurityRemediation::TrustProject
    );

    let unknown = SecurityReasonCode::UnknownResultUnconfirmed;
    assert_eq!(unknown.as_str(), "UNKNOWN_RESULT_UNCONFIRMED");
    assert!(unknown.is_unknown());
    assert_eq!(
        unknown.policy().retryability,
        SecurityRetryability::RequiresReconciliation
    );
    assert_eq!(
        unknown.policy().remediation,
        SecurityRemediation::ReconcileExternalEffect
    );

    for code in SecurityReasonCode::ALL {
        assert!(!code.as_str().is_empty());
        assert_eq!(SecurityReasonCode::parse(code.as_str()), *code);
    }
    assert_eq!(
        SecurityReasonCode::parse("arbitrary-provider-error"),
        SecurityReasonCode::UnknownUnclassified
    );
}

#[test]
fn legacy_reason_mapping_is_stable_and_does_not_echo_text() {
    assert_eq!(
        classify_security_reason("port_failed:result_unknown:provider leaked token"),
        SecurityReasonCode::UnknownResultUnconfirmed
    );
    assert_eq!(
        classify_security_reason("permission_denied:raw secret"),
        SecurityReasonCode::AuthCallerUntrusted
    );
    assert_eq!(
        classify_security_reason("unknown_schema:kiana.security-object.v9"),
        SecurityReasonCode::FactSchemaUnknownMajor
    );
    assert_eq!(
        classify_security_reason("shell_timeout"),
        SecurityReasonCode::UnknownTimeout
    );
}

#[test]
fn security_reason_round_trip_is_strict_and_digest_bound() {
    let operation_id = kiana_domain::OperationId::new();
    let evidence_id = kiana_domain::EvidenceRefId::new();
    let detail_digest = kiana_domain::json_digest(&json!({"redacted": true}));
    let reason = SecurityReason::with_refs(
        SecurityReasonCode::PolicyApprovalRequired,
        Some(operation_id),
        Some(detail_digest),
        vec![evidence_id],
    )
    .expect("security reason");
    assert_eq!(reason.schema, SECURITY_REASON_SCHEMA);
    assert_eq!(reason.policy().class, SecurityReasonClass::Policy);
    let encoded = reason.to_json().expect("reason json");
    assert_eq!(SecurityReason::from_json(&encoded).unwrap(), reason);

    let mut unknown_field = encoded.clone();
    unknown_field["raw_error"] = json!("provider token");
    assert!(SecurityReason::from_json(&unknown_field).is_err());

    let mut bad_digest = encoded;
    bad_digest["detail_digest"] = json!("provider token");
    assert!(SecurityReason::from_json(&bad_digest).is_err());

    let duplicate = SecurityReason::with_refs(
        SecurityReasonCode::FactDigestMismatch,
        None,
        None,
        vec![evidence_id, evidence_id],
    );
    assert_eq!(
        duplicate.unwrap_err(),
        "security_reason_evidence_refs_invalid"
    );
}

#[test]
fn unknown_reason_is_conservative_and_has_no_raw_error_field() {
    let reason = SecurityReason::new(SecurityReasonCode::UnknownUnclassified).unwrap();
    let value = reason.to_json().unwrap();
    assert_eq!(value["code"], "UNKNOWN_UNCLASSIFIED");
    assert!(value.get("error").is_none());
    assert!(value.get("message").is_none());
    assert!(reason.policy().retryability != SecurityRetryability::Never);
}
