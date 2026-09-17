use kiana_domain::{
    json_digest, AuthenticatedPrincipalRef, AuthenticationAssurance, Principal, PrincipalId,
    PrincipalKind, SessionAssertion, SessionStatus, SESSION_ASSERTION_SCHEMA,
};
use serde_json::json;

#[test]
fn session_assertion_is_versioned_bounded_and_round_trips() {
    let principal = AuthenticatedPrincipalRef::local();
    let assertion = SessionAssertion::new(
        "sc06-session",
        principal,
        AuthenticationAssurance::ProtectedLocal,
        100,
        500,
        1,
        3,
    )
    .unwrap();
    assert_eq!(assertion.schema, SESSION_ASSERTION_SCHEMA);
    assert!(assertion.active_at(100));
    assert!(assertion.active_at(499));
    assert!(!assertion.active_at(500));
    assert!(assertion.validate_at(200).is_ok());
    let encoded = assertion.to_json().unwrap();
    assert_eq!(SessionAssertion::from_json(&encoded).unwrap(), assertion);

    let mut unknown = encoded;
    unknown["raw_token"] = json!("bearer secret");
    assert!(SessionAssertion::from_json(&unknown).is_err());
}

#[test]
fn session_status_is_monotonic_and_expiry_or_revoke_is_terminal() {
    let assertion = SessionAssertion::new(
        "sc06-session",
        AuthenticatedPrincipalRef::local(),
        AuthenticationAssurance::Local,
        100,
        500,
        1,
        1,
    )
    .unwrap();
    let suspended = assertion.transition(SessionStatus::Suspended).unwrap();
    assert!(!suspended.active_at(200));
    assert!(suspended.transition(SessionStatus::Active).is_err());
    let revoked = assertion.revoke().unwrap();
    assert!(revoked.status.terminal());
    assert!(revoked.transition(SessionStatus::Active).is_err());
    assert_eq!(
        revoked.validate_at(200).unwrap_err(),
        "AUTH_SESSION_REVOKED"
    );
    assert_eq!(
        assertion.validate_at(500).unwrap_err(),
        "AUTH_SESSION_EXPIRED"
    );

    let anonymous = SessionAssertion::new(
        "anonymous",
        AuthenticatedPrincipalRef::local(),
        AuthenticationAssurance::Anonymous,
        100,
        500,
        1,
        1,
    );
    assert_eq!(anonymous.unwrap_err(), "session_assertion_anonymous_active");
}

#[test]
fn principal_snapshot_expiry_and_identity_binding_are_explicit() {
    let principal_id = PrincipalId::new();
    let mut authentication = AuthenticatedPrincipalRef::local();
    authentication.principal_id = principal_id.to_string();
    authentication.principal_digest = authentication.digest();
    let principal =
        Principal::new(principal_id, PrincipalKind::Human, authentication, 100).unwrap();
    assert!(principal.active_at(100));
    assert!(principal.active_at(101));
    assert!(principal.validate().is_ok());
    assert_eq!(
        json_digest(&json!({"id": principal.principal_id})),
        json_digest(&json!({"id": principal.principal_id}))
    );
}
