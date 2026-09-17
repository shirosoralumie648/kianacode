#[test]
fn daemon_entry_builds_one_server_owned_context_before_control_plane_dispatch() {
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let core = include_str!("../src/security_context.rs");
    for marker in [
        "resolve_security_context",
        "apply_to_request",
        "project_identity",
        "self.principal.identity.clone()",
        "project_trusted",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon context marker missing: {marker}"
        );
    }
    for marker in [
        "pub struct SecurityContext",
        "validate_request_assertions",
        "AuthCallerUntrusted",
        "AuthProjectMismatch",
        "AUTH_PROJECT_UNTRUSTED",
        "authority_epoch",
        "data_epoch",
    ] {
        assert!(
            core.contains(marker),
            "core context marker missing: {marker}"
        );
    }
    assert!(!daemon.contains("metadata.actor_id = Some(self.principal.actor_id.clone())"));
    assert!(!core.contains("authorize_and_execute"));
    assert!(!core.contains("CapabilityBroker"));
}
