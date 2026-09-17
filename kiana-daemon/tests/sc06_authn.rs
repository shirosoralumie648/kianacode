use kiana_daemon::LocalAuthnAdapter;
use kiana_domain::{AuthenticatedPrincipalRef, AuthenticationAssurance};

#[test]
fn local_authn_adapter_issues_and_validates_opaque_sessions() {
    let adapter = LocalAuthnAdapter::new(AuthenticatedPrincipalRef::local()).unwrap();
    let session = adapter
        .open_session(
            "sc06-session",
            100,
            100,
            AuthenticationAssurance::ProtectedLocal,
        )
        .unwrap();
    assert_eq!(
        adapter
            .validate_if_present("sc06-session", 150, true)
            .unwrap()
            .unwrap(),
        session
    );
    assert_eq!(
        adapter.authenticate("sc06-session", 150).unwrap(),
        AuthenticatedPrincipalRef::local()
    );
    assert_eq!(
        adapter
            .open_session("sc06-session", 100, 100, AuthenticationAssurance::Local)
            .unwrap_err()
            .to_string(),
        "conflict: AUTH_SESSION_REPLAY"
    );
}

#[test]
fn local_authn_adapter_denies_expired_revoked_and_missing_protected_sessions() {
    let adapter = LocalAuthnAdapter::new(AuthenticatedPrincipalRef::local()).unwrap();
    adapter
        .open_session("expired", 100, 10, AuthenticationAssurance::Local)
        .unwrap();
    assert!(adapter.validate_if_present("expired", 110, true).is_err());
    assert!(adapter.validate_if_present("missing", 100, true).is_err());
    assert!(adapter
        .validate_if_present("missing", 100, false)
        .unwrap()
        .is_none());

    adapter
        .open_session("revoked", 100, 100, AuthenticationAssurance::Local)
        .unwrap();
    adapter.revoke_session("revoked").unwrap();
    assert!(adapter.validate_if_present("revoked", 110, true).is_err());
    assert!(adapter.session_snapshot("revoked").is_some());
}
