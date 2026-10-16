//! 项目可信快照、部门角色快照和 assignment 目录的 Domain 合同测试。
//!
//! 这些用例直接创建内存对象和 JSON 值，不读取真实项目目录、不调用 Daemon 的 ProjectTrust adapter、
//! 不访问数据库或 EventLog，也不测试 ControlPlane 是否会据此授权。它们证明的是构造/校验规则，
//! 不是快照事实的来源真实性。

use kiana_domain::{
    json_digest, AssignmentDirectory, AssignmentId, AuthenticatedPrincipalRef, DepartmentSnapshot,
    DepartmentSpec, OrganizationId, ProjectAssignment, ProjectAssignmentId, ProjectIdentity,
    ProjectTrustSnapshot, RoleAssignment, RoleAssignmentStatus, SessionId,
};
use serde_json::json;

fn project() -> ProjectIdentity {
    ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        json_digest(&json!({"trust":"stored"})),
    )
    .unwrap()
}

#[test]
/// 确认项目快照可往返编码，并拒绝摘要失效或合同外字段。
///
/// `/repo` 只是 `ProjectIdentity` 的字符串 fixture，不要求磁盘上存在该目录；`trusted=true` 和
/// source 标签也由测试直接提供。更改 project ID 后保留旧 trust digest 会触发内容绑定错误；
/// 加入 `raw_token` 则在 Serde 的 `deny_unknown_fields` 边界被拒绝。
fn project_trust_snapshot_is_server_scoped_and_strict() {
    let project = project();
    let trust =
        ProjectTrustSnapshot::from_project(&project, true, "stored-project-trust", 2).unwrap();
    assert!(trust.trusted);
    assert_eq!(trust.project_id, project.project_id);
    assert!(trust.validate().is_ok());
    assert_eq!(
        ProjectTrustSnapshot::from_json(&trust.to_json().unwrap()).unwrap(),
        trust
    );

    let mut tampered = trust.to_json().unwrap();
    tampered["project_id"] = json!(kiana_domain::ProjectId::new());
    assert!(ProjectTrustSnapshot::from_json(&tampered).is_err());
    tampered["raw_token"] = json!("bearer secret");
    assert!(ProjectTrustSnapshot::from_json(&tampered).is_err());
}

#[test]
/// 检查部门规格生成的角色列表能被查询并经 JSON 往返，且不接受跨部门角色。
///
/// `DepartmentSpec::executing()` 是仓库内存中的静态规格；将角色改为 `reviewer` 后重新解码会失败，
/// 因为该角色不属于 executing 部门。此用例不绑定具体操作者，也不创建授权记录。
fn department_snapshot_is_canonical_and_role_bound() {
    let spec = DepartmentSpec::executing();
    let snapshot = DepartmentSnapshot::from_spec(&spec, 3, 4).unwrap();
    assert!(snapshot.contains_role("builder"));
    assert!(snapshot.validate().is_ok());
    assert_eq!(
        DepartmentSnapshot::from_json(&serde_json::to_value(&snapshot).unwrap()).unwrap(),
        snapshot
    );

    let mut tampered = serde_json::to_value(&snapshot).unwrap();
    tampered["role_ids"] = json!(["reviewer"]);
    assert!(DepartmentSnapshot::from_json(&tampered).is_err());
}

#[test]
/// 通过内存 assignment directory 验证主体、项目、角色和授权代次的联合解析。
///
/// RoleAssignment 与 ProjectAssignment 都是测试现场构造的值；`resolve(..., 100)` 用模拟当前时间
/// 100 毫秒解析它们，结果从同一目录记录中带回 principal、project、role、department 和 epoch。
/// 本测试不持久化 assignment，也不访问身份提供方；最后的 SessionId 断言只是验证该稳定 ID 包装类型。
fn assignment_directory_resolution_binds_principal_project_role_and_epoch() {
    let principal = AuthenticatedPrincipalRef::local();
    let project = project();
    let organization_id = OrganizationId::new();
    let assignment_id = AssignmentId::new();
    let role = RoleAssignment::new(
        assignment_id,
        principal.clone(),
        organization_id,
        "builder",
        "executing",
        vec![project.project_id],
        1,
        1_000,
        RoleAssignmentStatus::Active,
        2,
        5,
    )
    .unwrap();
    let project_assignment = ProjectAssignment::new(
        ProjectAssignmentId::new(),
        assignment_id,
        principal.clone(),
        organization_id,
        project.project_id,
        1,
        1_000,
        false,
        2,
        5,
    )
    .unwrap();
    let mut directory = AssignmentDirectory::new();
    directory.register_role(role).unwrap();
    directory.register_project(project_assignment).unwrap();
    let resolved = directory
        .resolve(
            &principal,
            organization_id,
            project.project_id,
            "builder",
            100,
        )
        .unwrap();
    assert_eq!(resolved.principal, principal);
    assert_eq!(resolved.project_id, project.project_id);
    assert_eq!(resolved.role_id, "builder");
    assert_eq!(resolved.department_id, "executing");
    assert_eq!(resolved.authority_epoch, 5);
    assert!(SessionId::new("scope").as_str() == "scope");
}
