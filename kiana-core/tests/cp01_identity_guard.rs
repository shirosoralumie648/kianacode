//! CP-01 Daemon/Core/Protocol 身份绑定静态源码守卫。
//!
//! guard 用 `include_str!` 搜索服务端主体、项目 metadata 和 session assignment CAS 的文本标记；
//! 它不会运行 DaemonHost、写 EventLog 或证明实际认证与 ProjectTrust 数据来源。

#[test]
/// 固定 daemon-owned principal/project 和 session assignment CAS 的预期接线标记。
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
