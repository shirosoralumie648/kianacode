#[test]
fn daemon_authn_adapter_is_opaque_and_precedes_existing_control_plane_path() {
    let daemon = include_str!("../../kiana-daemon/src/authn.rs");
    let host = include_str!("../../kiana-daemon/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/session_contracts.rs");
    for marker in [
        "LocalAuthnAdapter",
        "open_session",
        "validate_if_present",
        "AUTH_SESSION_REPLAY",
        "AUTH_SESSION_MISSING",
        "AUTH_PRINCIPAL_MISMATCH",
        "protected",
    ] {
        assert!(daemon.contains(marker), "authn marker missing: {marker}");
    }
    for marker in [
        "authn:",
        "validate_if_present",
        "SecurityContext",
        "ControlPlane",
    ] {
        assert!(host.contains(marker), "host authn marker missing: {marker}");
    }
    for marker in [
        "SessionAssertion",
        "AuthenticationAssurance",
        "SessionStatus",
        "deny_unknown_fields",
    ] {
        assert!(domain.contains(marker), "session marker missing: {marker}");
    }
    for forbidden in [
        "access_token",
        "refresh_token",
        "secret_value",
        "CapabilityBroker",
        "authorize_and_execute",
    ] {
        assert!(
            !daemon.contains(forbidden),
            "local authn adapter must not store or execute {forbidden}"
        );
    }
}
