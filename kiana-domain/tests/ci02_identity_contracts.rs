//! CI-02 Domain identity/config/authority 合同测试。
//!
//! 本文件以固定字符串、合成 ID 和 supplied epoch 构造 Principal、Membership、SecretRef、ProviderAccount、
//! ConfigSnapshot 与 AuthoritySnapshot；它不连接数据库、ProjectTrust、SecretStore 或认证 provider。
//! 断言的是 DTO 自校验/摘要与有限 secret marker 规则，不证明生产身份来源或完整秘密扫描。

use kiana_domain::*;
use serde_json::json;

/// 用 local identity fixture 填入新 PrincipalId，并重算自摘要以满足嵌套身份绑定。
fn principal() -> Principal {
    let principal_id = PrincipalId::new();
    let mut authentication = AuthenticatedPrincipalRef::local();
    authentication.principal_id = principal_id.to_string();
    authentication.principal_digest = authentication.digest();
    Principal::new(principal_id, PrincipalKind::Human, authentication, 1).unwrap()
}

/// 对 JSON 字符串本身计算摘要，作为 config/trust revision 的纯测试标签。
fn digest(value: &str) -> String {
    json_digest(&json!(value))
}

#[test]
/// 覆盖版本化身份/SecretRef/配置/授权 DTO 的成功校验和 supplied epoch 过期拒绝。
fn ci02_domain_contracts_are_versioned_and_secret_free() {
    let principal = principal();
    principal.validate().unwrap();
    assert!(principal.active_at(2));

    let membership = Membership::new(
        MembershipId::new(),
        principal.principal_id,
        OrganizationId::new(),
        1,
        3,
    )
    .unwrap();
    membership.validate().unwrap();

    let secret = SecretRef::new("env", "OPENAI_API_KEY", "provider.request", "openai", 1).unwrap();
    secret.validate().unwrap();
    let encoded = serde_json::to_string(&secret).unwrap();
    assert!(!encoded.contains("sk-"));
    let mut forged = serde_json::to_value(&secret).unwrap();
    // SecretRef schema 没有 raw_value 字段且 deny_unknown_fields；该拒绝点首先证明未知字段不能静默接受。
    forged["raw_value"] = json!("sk-live-secret");
    assert!(serde_json::from_value::<SecretRef>(forged).is_err());

    let account =
        ProviderAccount::new(ProviderAccountId::new(), "openai", "project:project-1").unwrap();
    account.validate().unwrap();
    let config = ConfigSnapshot::new(
        vec!["builtin:provider".to_owned()],
        json!({"provider":"openai","api_key_env":"OPENAI_API_KEY"}),
        digest("config"),
        digest("trust"),
    )
    .unwrap();
    config.validate().unwrap();

    let authority = AuthoritySnapshot::new(
        principal.principal_id,
        membership.organization_id,
        ProjectId::new(),
        "local-user",
        ROLE_BUILDER,
        DEPARTMENT_EXECUTING,
        "default",
        "project:project-1",
        3,
        vec![AssignmentId::new()],
        digest("trust"),
    )
    .unwrap();
    authority.validate().unwrap();
    assert_eq!(
        authority.validate_current_epoch(2).unwrap_err(),
        "authority_epoch_rollback"
    );
    assert_eq!(
        authority.validate_current_epoch(4).unwrap_err(),
        "authority_epoch_stale"
    );
}

#[test]
/// 通过配置 JSON 中的 token/bearer 模式验证有限 secret marker 检查会 fail closed。
fn ci02_snapshot_contracts_reject_raw_secret_fields_and_epoch_drift() {
    let config = ConfigSnapshot::new(
        Vec::new(),
        json!({"token":"bearer live-secret"}),
        digest("config"),
        digest("trust"),
    );
    assert_eq!(
        config.unwrap_err(),
        "config_snapshot_secret_or_size_invalid"
    );
}
