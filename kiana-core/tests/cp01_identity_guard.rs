#[test]
fn daemon_identity_guard_uses_server_principal_and_project_metadata() {
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let sessions = include_str!("../src/sessions.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    assert!(daemon.contains("AuthenticatedPrincipalRef"));
    assert!(daemon.contains("project_identity"));
    assert!(daemon.contains("project_authority"));
    assert!(daemon.contains("principal_role_not_authorized"));
    assert!(sessions.contains("SessionAssignment"));
    assert!(sessions.contains("append_expected"));
    assert!(sessions.contains("session_assignment_mismatch"));
    assert!(protocol.contains("RequestMetadata"));
}
