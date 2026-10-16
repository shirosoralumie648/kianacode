//! CP-01 服务端主体引用、项目身份和会话 assignment 的 Domain fixture。
//!
//! 测试直接提供 canonical path、device/inode 和 trust revision，不触碰文件系统或 ProjectTrust；
//! 它验证稳定 ID/摘要与 DTO 篡改检测，不证明 Daemon 已执行这些解析步骤。

use kiana_domain::{AuthenticatedPrincipalRef, ProjectIdentity, SessionAssignment};
use serde_json::json;

#[test]
/// 不同显示路径别名经相同 canonical root 输入后派生相同 project ID 和 identity digest。
fn principal_and_project_identity_are_server_shaped_and_deterministic() {
    let principal = AuthenticatedPrincipalRef::local();
    principal.validate().unwrap();
    assert_eq!(principal.principal_id, "local-user");
    assert!(!principal.principal_digest.is_empty());

    let first =
        ProjectIdentity::new("/repo", "/repo", Some(1), Some(2), json_digest("trusted")).unwrap();
    let second = ProjectIdentity::new(
        "/alias/../repo",
        "/repo",
        Some(1),
        Some(2),
        json_digest("trusted"),
    )
    .unwrap();
    assert_eq!(first.project_id, second.project_id);
    assert_eq!(first.identity_digest, second.identity_digest);
}

#[test]
/// assignment 将主体、项目、角色与部门放入同一摘要绑定 DTO，未知字段和字段篡改均被拒绝。
fn assignment_binds_principal_project_role_and_department() {
    let principal = AuthenticatedPrincipalRef::local();
    let project =
        ProjectIdentity::new("/repo", "/repo", None, None, json_digest("trusted")).unwrap();
    let assignment = SessionAssignment::new(
        principal,
        project,
        "session-1",
        "builder",
        "executing",
        1,
        1,
    )
    .unwrap();
    assignment.validate().unwrap();
    let mut encoded = serde_json::to_value(&assignment).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SessionAssignment>(encoded).is_err());
    let mut tampered = assignment.clone();
    tampered.role_id = "reviewer".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "session_assignment_digest_mismatch"
    );
}

/// 为 trust revision 构造纯 fixture 摘要，不读取 ProjectTrust authority。
fn json_digest(value: &str) -> String {
    kiana_domain::json_digest(&json!(value))
}
