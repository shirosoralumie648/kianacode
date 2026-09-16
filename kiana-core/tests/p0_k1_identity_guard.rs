#[test]
fn protected_ingress_uses_server_identity_and_authority_epoch() {
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let authority = include_str!("../src/authority.rs");
    let sessions = include_str!("../src/sessions.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    for marker in [
        "AuthenticatedPrincipal::local",
        "project_authority",
        "principal_role_not_authorized",
        "context.actor_id = Some(self.principal.actor_id.clone())",
        "synchronize_authority",
        "bind_session_assignment",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon identity marker missing: {marker}"
        );
    }
    for marker in [
        "pub(crate) async fn authority_epoch",
        "stream_version",
        "authority_epoch",
    ] {
        assert!(
            authority.contains(marker),
            "authority epoch marker missing: {marker}"
        );
    }
    for marker in [
        "SessionAssignment",
        "append_expected",
        "session_assignment_mismatch",
        "let authority_epoch = self.authority_epoch",
    ] {
        assert!(
            sessions.contains(marker),
            "session assignment marker missing: {marker}"
        );
    }
    assert!(lifecycle.contains("\"authority_epoch\":authority_epoch"));
}
