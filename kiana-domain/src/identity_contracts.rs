//! 主体、成员关系、凭据引用、配置和授权快照的版本化合同，属于 `kiana-domain`。
//!
//! 【系统位置与数据流】
//! ```text
//! Daemon/Identity adapter -> Principal + Membership/Authority metadata
//! Provider ConfigResolver -> ConfigSnapshot -> ConfigSnapshotStore / provider route
//! Credential config -> SecretRef -> CredentialResolver/Store -> Broker effect boundary
//! ```
//! 本文件定义可传递、可序列化的数据形状和纯校验，不实现身份数据库、credential store、授权决策或凭据解析。
//! `SecretRef` 只是定位秘密值的引用；`AuthoritySnapshot` 只是某时点声明，ControlPlane 仍要在每个 effect 边界重验。
//! Principal/Membership/ProviderAccount/ServiceIdentity 等类型并不都已接入生产 authority store；类型可构造不等于服务端已强制它。
//! `deny_unknown_fields` 让新增字段不会被旧版本静默忽略；各 digest 都是无密钥完整性摘要，不是签名或主体认证。
use crate::{
    json_digest, AssignmentId, AuthenticatedPrincipalRef, OrganizationId, PrincipalId, ProjectId,
    ProviderAccountId, ServiceIdentityId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Principal DTO 的 wire schema 名称。
pub const PRINCIPAL_SCHEMA: &str = "kiana.principal.v1";
/// Principal-Membership 关系 DTO 的 wire schema 名称。
pub const MEMBERSHIP_SCHEMA: &str = "kiana.membership.v1";
/// SecretRef 凭据定位符的 wire schema 名称。
pub const SECRET_REF_SCHEMA: &str = "kiana.secret-ref.v1";
/// 上游 Provider account 元数据 DTO 的 wire schema 名称。
pub const PROVIDER_ACCOUNT_SCHEMA: &str = "kiana.provider-account.v1";
/// 服务身份 DTO 的 wire schema 名称。
pub const SERVICE_IDENTITY_SCHEMA: &str = "kiana.service-identity.v1";
/// 非秘密 workspace config snapshot 的 wire schema 名称。
pub const CONFIG_SNAPSHOT_SCHEMA: &str = "kiana.config-snapshot.v1";
/// 授权输入快照 DTO 的 wire schema 名称。
pub const AUTHORITY_SNAPSHOT_SCHEMA: &str = "kiana.authority-snapshot.v1";
/// 旧主体到新主体迁移记录的 wire schema 名称。
pub const IDENTITY_MIGRATION_SCHEMA: &str = "kiana.identity-migration.v1";

/// 校验必填字符串的空白、字节上限和 NUL 字符。
///
/// 返回 `field_invalid` 稳定错误；不会判断主体是否存在或字符串是否代表真实身份。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 校验 SHA-256 文本格式，不从数据库、配置源或秘密存储中核对输入。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 校验字符串列表的数量、单项非空、长度和 NUL 字符。
fn list(values: &[String], field: &str, max: usize) -> Result<(), String> {
    if values.len() > max
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 4_096 || value.contains('\0'))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 主体属于哪类调用者；类型分类本身不会授予权限。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    /// 自然人或本地用户。
    Human,
    /// 由 harness 执行的模型/agent 主体。
    Agent,
    /// 服务间调用主体。
    Service,
    /// MCP server/tool 侧的主体分类。
    Mcp,
    /// 外部模型供应商相关主体分类。
    Provider,
}

/// Principal 生命周期状态；是否可用还需结合时间窗口和权威存储。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalStatus {
    /// 已登记但尚未激活。
    Proposed,
    /// 当前有效状态；`active_at()` 还会检查 created/expiry 时间。
    Active,
    /// 临时暂停，不能当作 active 使用。
    Suspended,
    /// 已撤销，不应恢复为有效主体。
    Revoked,
    /// 归档状态，保留历史但不代表当前可用。
    Archived,
}

/// 版本化主体记录，绑定稳定 PrincipalId、主体类别、状态和认证元数据引用。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    /// 固定 schema 名称。
    pub schema: String,
    /// 本系统内部稳定主体 ID，不是认证凭据。
    pub principal_id: PrincipalId,
    /// Human/Agent/Service/MCP/Provider 分类。
    pub kind: PrincipalKind,
    /// Proposed/Active/Suspended/Revoked/Archived 生命周期状态。
    pub status: PrincipalStatus,
    /// 主体创建时间，Unix epoch 毫秒。
    pub created_at_unix_ms: u64,
    /// 可选过期时间；None 表示本记录未声明到期点，不代表授权永久有效。
    #[serde(default)]
    pub expires_at_unix_ms: Option<u64>,
    /// 认证主体引用；只携带 opaque metadata，不包含 raw credential。
    pub authentication: AuthenticatedPrincipalRef,
    /// 对 Principal 字段计算的无密钥摘要。
    pub principal_digest: String,
}

impl Principal {
    /// 构造 Active Principal，并把认证引用与 PrincipalId 绑定。
    ///
    /// 【调用者】当前主要由 ports/domain fixtures 构造；生产主体存储和创建工作流未由此函数实现。
    /// 【输入/输出】调用者提供稳定 ID、类别、AuthenticatedPrincipalRef 和创建时间，返回带摘要的 DTO。
    /// 【失败情况】时间无效、主体引用与 PrincipalId 不匹配或嵌套字段/摘要校验失败时返回结构化字符串错误。
    /// 【副作用】只构造内存值，不写身份存储或认证事件。
    pub fn new(
        principal_id: PrincipalId,
        kind: PrincipalKind,
        authentication: AuthenticatedPrincipalRef,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut principal = Self {
            schema: PRINCIPAL_SCHEMA.to_owned(),
            principal_id,
            kind,
            status: PrincipalStatus::Active,
            created_at_unix_ms,
            expires_at_unix_ms: None,
            authentication,
            principal_digest: String::new(),
        };
        principal.principal_digest = principal.digest();
        principal.validate()?;
        Ok(principal)
    }

    /// 校验主体头、认证引用 ID 绑定和自摘要。
    ///
    /// 不向 authority store 查询状态，也不验证当前 clock；过期窗口由 `active_at()` 用调用方传入的 now 判断。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PRINCIPAL_SCHEMA
            || self.created_at_unix_ms == 0
            || self
                .expires_at_unix_ms
                .is_some_and(|expires| expires <= self.created_at_unix_ms)
        {
            return Err("principal_header_invalid".to_owned());
        }
        self.authentication.validate()?;
        if self.authentication.principal_id != self.principal_id.to_string() {
            return Err("principal_identity_mismatch".to_owned());
        }
        required(&self.authentication.principal_id, "principal_id", 256)?;
        digest(&self.principal_digest, "principal_digest")?;
        if self.principal_digest != self.digest() {
            return Err("principal_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 判断 supplied time 是否位于 Active 主体的创建/过期区间 `[created, expires)`。
    ///
    /// `now_unix_ms` 来自调用方；本 helper 不读取可信时钟，也不会把错误 DTO 变成已授权身份。
    pub fn active_at(&self, now_unix_ms: u64) -> bool {
        self.status == PrincipalStatus::Active
            && now_unix_ms >= self.created_at_unix_ms
            && self
                .expires_at_unix_ms
                .is_none_or(|expires| now_unix_ms < expires)
    }

    /// 清空自引用字段后计算 Principal JSON 摘要。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("principal_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

/// 成员关系生命周期状态；单有主体和组织 ID 不等于当前 membership 有效。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipStatus {
    /// 已发出邀请但成员尚未接受。
    Invited,
    /// 成员已接受，尚未进入 Active 状态。
    Accepted,
    /// 在有效时间窗内可以作为成员关系候选。
    Active,
    /// 暂停中的成员关系。
    Suspended,
    /// 已超过有效期。
    Expired,
    /// 已撤销的成员关系。
    Revoked,
}

/// 将 Principal 与 Organization 通过一个有时间范围和 authority epoch 的成员关系 DTO 连接。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Membership {
    /// 固定 schema 名称。
    pub schema: String,
    /// 此关系记录的稳定 ID。
    pub membership_id: crate::MembershipId,
    /// 成员主体 ID；该字段本身不查询 Principal store。
    pub principal_id: PrincipalId,
    /// 关系所处的组织 ID。
    pub organization_id: OrganizationId,
    /// 邀请、接受、激活、暂停、过期或撤销状态。
    pub status: MembershipStatus,
    /// 有效窗口起点，Unix epoch 毫秒。
    pub valid_from_unix_ms: u64,
    /// 可选有效窗口终点；使用左闭右开区间。
    #[serde(default)]
    pub expires_at_unix_ms: Option<u64>,
    /// 这条关系声明所属的 authority 版本。
    pub authority_epoch: u64,
    /// 对 membership 字段计算的无密钥摘要。
    pub membership_digest: String,
}

impl Membership {
    /// 构造默认 Active 的 membership 并立即校验。
    ///
    /// 【调用者】Domain fixtures/authority adapter；本函数不把 relation 写入持久化 ledger。
    /// 【失败情况】有效时间、epoch 或自摘要不合法时返回稳定 `membership_*` 错误。
    pub fn new(
        membership_id: crate::MembershipId,
        principal_id: PrincipalId,
        organization_id: OrganizationId,
        valid_from_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut membership = Self {
            schema: MEMBERSHIP_SCHEMA.to_owned(),
            membership_id,
            principal_id,
            organization_id,
            status: MembershipStatus::Active,
            valid_from_unix_ms,
            expires_at_unix_ms: None,
            authority_epoch,
            membership_digest: String::new(),
        };
        membership.membership_digest = membership.digest();
        membership.validate()?;
        Ok(membership)
    }

    /// 检查 schema、时间窗、非零 epoch 和自摘要；不查 Principal/Organization 的真实存在性。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMBERSHIP_SCHEMA
            || self.valid_from_unix_ms == 0
            || self.authority_epoch == 0
            || self
                .expires_at_unix_ms
                .is_some_and(|expires| expires <= self.valid_from_unix_ms)
        {
            return Err("membership_header_invalid".to_owned());
        }
        digest(&self.membership_digest, "membership_digest")?;
        if self.membership_digest != self.digest() {
            return Err("membership_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 判断 membership 在 supplied time 是否处于 Active 且有效的 `[valid_from, expires)` 区间。
    ///
    /// 调用者负责提供可信 `now_unix_ms`，并在权限边界重新检查当前 authority epoch。
    pub fn active_at(&self, now_unix_ms: u64) -> bool {
        self.status == MembershipStatus::Active
            && now_unix_ms >= self.valid_from_unix_ms
            && self
                .expires_at_unix_ms
                .is_none_or(|expires| now_unix_ms < expires)
    }

    /// 清空自摘要字段后生成 membership JSON 摘要。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("membership_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

/// 指向秘密存储中一条凭据的非秘密定位符。
///
/// 该 DTO 允许通过配置、端口、事件和 Broker 传递；真实 secret 只能由有权限的 CredentialStore/adapter 在 effect 边界解析。
/// 保存或序列化 SecretRef 不证明对应值存在、仍有效或可以访问。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRef {
    /// 固定 schema 名称。
    pub schema: String,
    /// 选择哪个秘密存储 adapter，例如 env/keyring/vault 的逻辑名称。
    pub store: String,
    /// 在指定 store 中查找值的 key；可为环境变量名，但不能放 raw secret。
    pub key: String,
    /// 使用凭据的业务目的，用于避免跨用途复用。
    pub purpose: String,
    /// 允许解析凭据的接收方/服务标签。
    pub audience: String,
    /// 凭据轮换代数；store 可用它拒绝旧引用。
    pub generation: u64,
    /// 对引用 metadata 计算的无密钥摘要，不含实际 secret 值。
    pub reference_digest: String,
}

impl SecretRef {
    /// 构造秘密存储定位符并校验文本和 generation。
    ///
    /// 【调用者】Provider/OAuth/connector/deployment 配置路径与 fixtures。
    /// 【输入/输出】store/key/purpose/audience/generation → `SecretRef`。
    /// 【失败情况】空字段、CR/LF、超长文本、generation 为零或摘要错误时返回 `secret_ref_*` 错误。
    /// 【副作用/安全边界】纯数据构造，不访问 store、不解析 secret；不能把 key 当作凭据值交给模型。
    pub fn new(
        store: impl Into<String>,
        key: impl Into<String>,
        purpose: impl Into<String>,
        audience: impl Into<String>,
        generation: u64,
    ) -> Result<Self, String> {
        let mut reference = Self {
            schema: SECRET_REF_SCHEMA.to_owned(),
            store: store.into(),
            key: key.into(),
            purpose: purpose.into(),
            audience: audience.into(),
            generation,
            reference_digest: String::new(),
        };
        reference.reference_digest = reference.digest();
        reference.validate()?;
        Ok(reference)
    }

    /// 校验引用 schema、字段边界、非零 generation 和 reference digest。
    ///
    /// 这里故意只检查定位符；CredentialResolver 仍须按 purpose/audience/generation 在真实 store 查询并处理过期/撤销。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECRET_REF_SCHEMA || self.generation == 0 {
            return Err("secret_ref_header_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.store, "secret_ref_store", 128),
            (&self.key, "secret_ref_key", 1_024),
            (&self.purpose, "secret_ref_purpose", 256),
            (&self.audience, "secret_ref_audience", 256),
        ] {
            required(value, field, max)?;
            if value.contains(['\n', '\r']) {
                return Err(format!("{field}_invalid"));
            }
        }
        digest(&self.reference_digest, "secret_ref_digest")?;
        if self.reference_digest != self.digest() {
            return Err("secret_ref_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对引用 metadata 生成稳定摘要；实际 secret 不在该输入中。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "store": self.store,
            "key": self.key,
            "purpose": self.purpose,
            "audience": self.audience,
            "generation": self.generation,
        }))
    }
}

/// 外部 provider account 的运行状态，不表示 Kiana 项目角色或执行权限。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAccountStatus {
    /// provider account metadata 当前标记为可用。
    Active,
    /// 暂停使用。
    Suspended,
    /// 已撤销。
    Revoked,
    /// 状态来源缺失或无法判断；应按调用方策略 fail-closed。
    Unknown,
}

/// 外部 provider 账号的标识和数据边界声明。
///
/// 它不是用户 Principal、项目授权或已验证的 OAuth token；OAuth 运行记录由专门的 account store/adapter 管理。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderAccount {
    /// 固定 schema 名称。
    pub schema: String,
    /// Kiana 内部 provider account ID。
    pub account_id: ProviderAccountId,
    /// provider 类型标签，例如 `openai`。
    pub provider_id: String,
    /// 可选 provider 侧 subject 标签；不是秘密。
    #[serde(default)]
    pub external_subject: Option<String>,
    /// 可选租户标签，不意味着已执行 tenant 隔离。
    #[serde(default)]
    pub tenant: Option<String>,
    /// 数据驻留/处理边界声明，由消费方解释。
    pub data_boundary: String,
    /// 外部账户 metadata 状态。
    pub status: ProviderAccountStatus,
    /// 此账户 metadata 的修订号；只要求非零，不在这里做 CAS。
    pub revision: u64,
    /// 对账户 DTO 计算的无密钥摘要。
    pub account_digest: String,
}

impl ProviderAccount {
    /// 构造初始 Active provider account metadata。
    ///
    /// 【调用者】当前主要为 Domain fixtures；不创建外部 OAuth/provider account。
    /// 【输入/输出】接收内部 account id、provider label 与 data boundary，其他外部标识留空。
    /// 【失败情况】必要标签、revision 或自摘要无效时返回 `provider_account_*` 错误。
    /// 【副作用】仅生成领域值，不发网络请求、不解析 SecretRef。
    pub fn new(
        account_id: ProviderAccountId,
        provider_id: impl Into<String>,
        data_boundary: impl Into<String>,
    ) -> Result<Self, String> {
        let mut account = Self {
            schema: PROVIDER_ACCOUNT_SCHEMA.to_owned(),
            account_id,
            provider_id: provider_id.into(),
            external_subject: None,
            tenant: None,
            data_boundary: data_boundary.into(),
            status: ProviderAccountStatus::Active,
            revision: 1,
            account_digest: String::new(),
        };
        account.account_digest = account.digest();
        account.validate()?;
        Ok(account)
    }

    /// 校验账户元数据格式和自摘要。
    ///
    /// 不访问 provider、不检查 subject/tenant 唯一性，也不根据 status 授予项目或 capability 权限。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_ACCOUNT_SCHEMA || self.revision == 0 {
            return Err("provider_account_header_invalid".to_owned());
        }
        required(&self.provider_id, "provider_id", 128)?;
        required(&self.data_boundary, "provider_data_boundary", 256)?;
        for (value, field) in [
            (&self.external_subject, "provider_external_subject"),
            (&self.tenant, "provider_tenant"),
        ] {
            if value
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.len() > 512)
            {
                return Err(format!("{field}_invalid"));
            }
        }
        digest(&self.account_digest, "provider_account_digest")?;
        if self.account_digest != self.digest() {
            return Err("provider_account_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 清空自摘要后计算 provider account JSON 指纹。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("account_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

/// 服务间调用身份元数据，绑定 Principal、凭据引用、能力 scope 和轮换策略声明。
///
/// 目前 DTO 没有专用 `new()`/存储实现；消费端仍需要检查 credential 状态并按最小权限解释 scopes。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceIdentity {
    /// 固定 schema 名称。
    pub schema: String,
    /// 此 service identity 的稳定 ID。
    pub service_identity_id: ServiceIdentityId,
    /// 代表该服务的内部 Principal ID。
    pub principal_id: PrincipalId,
    /// 用于取得 service credential 的 opaque reference，不携带凭据值。
    pub credential_ref: SecretRef,
    /// 服务允许申请的 capability 名称列表，不是已授权 grant。
    pub capability_scopes: Vec<String>,
    /// 调用方声明的轮换政策描述。
    pub rotation_policy: String,
    /// 对 identity metadata 计算的无密钥摘要。
    pub identity_digest: String,
}

impl ServiceIdentity {
    /// 校验 service identity 的嵌套 SecretRef、scope 列表、轮换描述和自摘要。
    ///
    /// `Ok(())` 只表示 DTO 自洽；本方法不会检索 Principal、credential store、scope catalog 或轮换状态。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SERVICE_IDENTITY_SCHEMA {
            return Err("service_identity_schema_invalid".to_owned());
        }
        self.credential_ref.validate()?;
        list(&self.capability_scopes, "service_capability_scopes", 128)?;
        required(&self.rotation_policy, "service_rotation_policy", 512)?;
        digest(&self.identity_digest, "service_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("service_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对身份字段和 credential reference metadata 计算摘要，不包含 raw credential。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "service_identity_id": self.service_identity_id,
            "principal_id": self.principal_id,
            "credential_ref": self.credential_ref,
            "capability_scopes": self.capability_scopes,
            "rotation_policy": self.rotation_policy,
        }))
    }
}

/// 递归检查 serde_json 值中常见的 raw secret 字段和值模式。
///
/// 对象 key 使用 ASCII 小写匹配常见敏感字段；`*_ref`、`*_env`、`*_id` 作为引用/名称保留，
/// 但它们的字符串值仍会继续扫描。数组逐项递归，字符串仅拦截包含 `bearer ` 或以 `sk-` 开头的模式。
/// 这是有限 fail-closed 规则，不是通用 secret detector，也不能证明所有密钥都不存在。
fn contains_raw_secret(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            let normalized = key.to_ascii_lowercase();
            let forbidden_key = matches!(
                normalized.as_str(),
                "secret"
                    | "token"
                    | "password"
                    | "api_key"
                    | "access_token"
                    | "refresh_token"
                    | "authorization"
            );
            let reference_key = normalized.ends_with("_ref")
                || normalized.ends_with("_env")
                || normalized.ends_with("_id");
            (forbidden_key && !reference_key) || contains_raw_secret(value)
        }),
        Value::Array(values) => values.iter().any(contains_raw_secret),
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            lower.contains("bearer ") || lower.starts_with("sk-")
        }
        _ => false,
    }
}

/// provider/workspace 配置解析后形成的非秘密有效配置快照。
///
/// Provider resolver 把 source refs、非秘密 JSON 配置和 config/trust revision 固定为一个摘要；
/// downstream store/route 可比较 revision，避免把配置文本和密码/令牌一起保存。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSnapshot {
    /// 固定 schema 名称。
    pub schema: String,
    /// 本快照格式版本。
    pub version: u64,
    /// 提供配置的 source labels，例如 builtin/profile 文件；这些标签不证明来源可信。
    pub source_refs: Vec<String>,
    /// 已解析的非秘密 JSON 值；序列化尺寸上限为 64 KiB。
    pub effective_non_secret_config: Value,
    /// 有效配置内容版本的摘要/修订标签。
    pub config_revision: String,
    /// 对应的 ProjectTrust 版本，用于提示信任变化需要重新解析。
    pub project_trust_revision: String,
    /// 对快照业务字段计算的无密钥摘要。
    pub snapshot_digest: String,
}

impl ConfigSnapshot {
    /// 构造配置快照并执行秘密/体积检查。
    ///
    /// 【调用者】`kiana-provider::ConfigResolver::workspace_snapshot()` 与 config store fixtures。
    /// 【输入/输出】source refs、有效非秘密 JSON、config revision、ProjectTrust revision → 自摘要绑定的 ConfigSnapshot。
    /// 【失败情况】JSON 过大、发现被禁止的 secret pattern、引用列表或 revision digest 无效时返回 `config_snapshot_*` 错误。
    /// 【副作用】纯内存检查；不会读取配置文件、查询 ProjectTrust 或访问 secret store。
    pub fn new(
        source_refs: Vec<String>,
        effective_non_secret_config: Value,
        config_revision: impl Into<String>,
        project_trust_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: CONFIG_SNAPSHOT_SCHEMA.to_owned(),
            version: 1,
            source_refs,
            effective_non_secret_config,
            config_revision: config_revision.into(),
            project_trust_revision: project_trust_revision.into(),
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// 校验 schema/version、JSON 字节上限、有限 secret marker、source refs/revisions 和自摘要。
    ///
    /// 64 KiB 是序列化 JSON 的 byte 上限；通过检查只代表当前有限扫描规则未命中，不构成全面秘密清理证明。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONFIG_SNAPSHOT_SCHEMA
            || self.version == 0
            || serde_json::to_vec(&self.effective_non_secret_config)
                .map_or(true, |bytes| bytes.len() > 64 * 1024)
            || contains_raw_secret(&self.effective_non_secret_config)
        {
            return Err("config_snapshot_secret_or_size_invalid".to_owned());
        }
        list(&self.source_refs, "config_source_refs", 128)?;
        digest(&self.config_revision, "config_revision")?;
        digest(&self.project_trust_revision, "project_trust_revision")?;
        digest(&self.snapshot_digest, "config_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("config_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对配置快照字段计算无密钥 JSON 摘要，不包含 `snapshot_digest` 自身。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "source_refs": self.source_refs,
            "effective_non_secret_config": self.effective_non_secret_config,
            "config_revision": self.config_revision,
            "project_trust_revision": self.project_trust_revision,
        }))
    }
}

/// 某一 authority epoch 下的主体、组织、项目、角色、策略和 assignment ID 集合。
///
/// 它把多条上游解析结果打包，供 ControlPlane/ports 做一致性比较；snapshot 不能单独授予访问权，
/// 调用方仍要对当前 epoch、session/project 绑定和具体能力做重新授权。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritySnapshot {
    /// 固定 schema 名称。
    pub schema: String,
    /// 被解析的主体 ID。
    pub principal_id: PrincipalId,
    /// 主体所处的组织 ID。
    pub organization_id: OrganizationId,
    /// 当前请求的项目 ID。
    pub project_id: ProjectId,
    /// 绑定 session 的所有者标签。
    pub session_owner: String,
    /// 当前角色 ID。
    pub role_id: String,
    /// 当前部门 ID。
    pub department_id: String,
    /// 下游用于挑选策略的 profile 标签。
    pub policy_profile: String,
    /// 数据读写边界标签。
    pub data_boundary: String,
    /// 解析快照所依据的 authority revision；旧值不得用于新 effect。
    pub authority_epoch: u64,
    /// 本主体/项目绑定的 Assignment IDs，必须按 UUID 严格递增且不重复。
    pub assignment_ids: Vec<AssignmentId>,
    /// ProjectTrust/authority source revision 的摘要引用。
    pub trust_revision: String,
    /// 对以上字段计算的无密钥自摘要。
    pub snapshot_digest: String,
}

/// 记录固定历史主体 `local-user` 迁移到新 principal ID 的兼容事实。
///
/// 这是可审计迁移描述，不是认证断言，也不会授权角色、项目或 capability；未来 ingress 可替换主体 ID，
/// 而无需改写这条历史记录。实际写入事件或身份存储由调用方负责。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityMigration {
    /// 固定 schema 名称。
    pub schema: String,
    /// 本次迁移事实的唯一 ID。
    pub migration_id: String,
    /// 被迁移的历史 principal label。
    pub legacy_principal_id: String,
    /// 目标 principal label。
    pub principal_id: String,
    /// 为什么需要迁移的审计说明。
    pub reason: String,
    /// 迁移发生时间，Unix epoch 毫秒；由调用方提供。
    pub migrated_at_unix_ms: u64,
    /// 对迁移描述字段计算的无密钥摘要。
    pub migration_digest: String,
}

impl IdentityMigration {
    /// 构造旧/新主体不同的迁移事实并计算摘要。
    ///
    /// 【调用者】Daemon `legacy_local_user_migration()` 调用本构造器并返回 DTO。
    /// 【失败情况】字段为空/超长、时间为零、主体未变化或摘要不一致时返回 `identity_migration_*` 错误。
    /// 【副作用】不认证新主体，也不写 EventLog；只把 supplied migration metadata 固定成 typed value。
    pub fn new(
        migration_id: impl Into<String>,
        legacy_principal_id: impl Into<String>,
        principal_id: impl Into<String>,
        reason: impl Into<String>,
        migrated_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut migration = Self {
            schema: IDENTITY_MIGRATION_SCHEMA.to_owned(),
            migration_id: migration_id.into(),
            legacy_principal_id: legacy_principal_id.into(),
            principal_id: principal_id.into(),
            reason: reason.into(),
            migrated_at_unix_ms,
            migration_digest: String::new(),
        };
        migration.migration_digest = migration.digest();
        migration.validate()?;
        Ok(migration)
    }

    /// 校验 migration DTO 自身的字段和自摘要。
    ///
    /// 不验证迁移审批、事件持久化或新 principal 的身份来源。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != IDENTITY_MIGRATION_SCHEMA || self.migrated_at_unix_ms == 0 {
            return Err("identity_migration_header_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.migration_id, "identity_migration_id", 256),
            (&self.legacy_principal_id, "identity_migration_legacy", 256),
            (&self.principal_id, "identity_migration_principal", 256),
            (&self.reason, "identity_migration_reason", 1_024),
        ] {
            required(value, field, max)?;
        }
        if self.legacy_principal_id == self.principal_id {
            return Err("identity_migration_identity_unchanged".to_owned());
        }
        digest(&self.migration_digest, "identity_migration_digest")?;
        if self.migration_digest != self.digest() {
            return Err("identity_migration_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 清空自摘要后计算迁移事实 JSON 摘要。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("migration_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

impl AuthoritySnapshot {
    /// 将 authority resolver 收集的各项 ID、策略和 revision 组为快照并计算自摘要。
    ///
    /// 【调用者】`kiana-ports::IdentityResolver::resolve_authority()` 声明返回此 DTO；当前没有找到该 trait 的生产实现，
    /// Domain fixtures 可直接构造。运行中的 Daemon/Core 另有 `SecurityAuthoritySnapshot` 类型。
    /// 【输入/输出】调用方提供主体/组织/项目/owner/role/department/policy/data boundary/epoch/assignment IDs/trust revision。
    /// 【失败情况】字段无效、assignment IDs 顺序非规范、epoch 为零或摘要错误时返回稳定错误。
    /// 【副作用】纯数据构造，不查询 membership/assignment store，也不作最终授权。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        principal_id: PrincipalId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        session_owner: impl Into<String>,
        role_id: impl Into<String>,
        department_id: impl Into<String>,
        policy_profile: impl Into<String>,
        data_boundary: impl Into<String>,
        authority_epoch: u64,
        assignment_ids: Vec<AssignmentId>,
        trust_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: AUTHORITY_SNAPSHOT_SCHEMA.to_owned(),
            principal_id,
            organization_id,
            project_id,
            session_owner: session_owner.into(),
            role_id: role_id.into(),
            department_id: department_id.into(),
            policy_profile: policy_profile.into(),
            data_boundary: data_boundary.into(),
            authority_epoch,
            assignment_ids,
            trust_revision: trust_revision.into(),
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// 校验快照的必填字段、assignment ID 顺序、epoch 和自摘要。
    ///
    /// assignment IDs 必须严格递增，因此重复值和逆序都会以 `authority_assignment_ids_noncanonical` 拒绝。
    /// 该方法不会核验主体、项目、组织、membership 或 role 在权威存储中的真实关系。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTHORITY_SNAPSHOT_SCHEMA
            || self.authority_epoch == 0
            || self.assignment_ids.is_empty()
        {
            return Err("authority_snapshot_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.session_owner, "authority_session_owner"),
            (&self.role_id, "authority_role"),
            (&self.department_id, "authority_department"),
            (&self.policy_profile, "authority_policy_profile"),
            (&self.data_boundary, "authority_data_boundary"),
        ] {
            required(value, field, 512)?;
        }
        if self
            .assignment_ids
            .windows(2)
            .any(|pair| pair[0].as_uuid() >= pair[1].as_uuid())
        {
            return Err("authority_assignment_ids_noncanonical".to_owned());
        }
        digest(&self.trust_revision, "authority_trust_revision")?;
        digest(&self.snapshot_digest, "authority_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("authority_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 先校验快照，再将它与 caller 提供的当前 epoch 做精确比较。
    ///
    /// `current_epoch < snapshot_epoch` 返回 rollback，`current_epoch > snapshot_epoch` 返回 stale；
    /// 两类错误都表示调用方应重新取 authority facts，而不是继续复用这份旧快照。
    pub fn validate_current_epoch(&self, current_epoch: u64) -> Result<(), String> {
        self.validate()?;
        if current_epoch < self.authority_epoch {
            return Err("authority_epoch_rollback".to_owned());
        }
        if current_epoch != self.authority_epoch {
            return Err("authority_epoch_stale".to_owned());
        }
        Ok(())
    }

    /// 对 authority 字段计算无密钥摘要，不包含自身 `snapshot_digest`。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "principal_id": self.principal_id,
            "organization_id": self.organization_id,
            "project_id": self.project_id,
            "session_owner": self.session_owner,
            "role_id": self.role_id,
            "department_id": self.department_id,
            "policy_profile": self.policy_profile,
            "data_boundary": self.data_boundary,
            "authority_epoch": self.authority_epoch,
            "assignment_ids": self.assignment_ids,
            "trust_revision": self.trust_revision,
        }))
    }
}

/// 校验外部 assignment ID 列表已去重且按 UUID 严格递增。
///
/// 稳定排序让同一集合有唯一序列化形式。当前 AuthoritySnapshot 自己重复了窗口顺序检查，
/// 仓内未发现此 helper 的其他调用点；它只校验顺序，不读取 assignment 内容或授权状态。
pub fn validate_assignment_id_order(ids: &[AssignmentId]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    if ids.iter().any(|id| !seen.insert(id.as_uuid())) {
        return Err("assignment_ids_duplicate".to_owned());
    }
    if ids
        .windows(2)
        .any(|pair| pair[0].as_uuid() >= pair[1].as_uuid())
    {
        return Err("assignment_ids_noncanonical".to_owned());
    }
    Ok(())
}
