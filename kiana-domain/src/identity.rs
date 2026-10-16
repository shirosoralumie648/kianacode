//! 服务端持有的主体引用、项目身份和会话指派快照，属于 `kiana-domain`。
//!
//! 【核心调用链】
//! ```text
//! Daemon ingress 根据本地身份/ProjectTrust 构造 principal 与 project
//!     -> RequestContext 交给 ControlPlane
//!     -> Core 把 principal + project + role/department + epoch 组为 SessionAssignment
//!     -> assignment 随 session.assigned 事件以 CAS 方式持久化
//! ```
//! 本文件提供不可变 typed DTO 与自校验，不自行读取 OS 凭据、canonicalize 路径、查询 ProjectTrust 或写 EventLog。
//! wire actor、role、trust 字段不能直接当作已认证主体；调用方必须在服务端边界构造这些值，ControlPlane 还需在 effect 边界授权。
//! `principal_digest`、`identity_digest` 和 `assignment_digest` 都是可重算的无密钥摘要，只检测字段变化，不是签名。
//! 当前本地路径的实际构造点位于 Daemon/Core；`AuthenticatedPrincipalRef::local()` 只是返回固定本地身份标签，不执行系统登录认证。

use crate::{canonical_journal_bytes, json_digest, ProjectId, SchemaVersion, SessionId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// 主体引用 DTO 的稳定 schema 名称。
pub const AUTHENTICATED_PRINCIPAL_SCHEMA: &str = "kiana.authenticated-principal.v1";
/// 项目身份 DTO 的稳定 schema 名称。
pub const PROJECT_IDENTITY_SCHEMA: &str = "kiana.project-identity.v1";
/// 会话指派 DTO 的稳定 schema 名称。
pub const SESSION_ASSIGNMENT_SCHEMA: &str = "kiana.session-assignment.v1";
/// 本文件身份 DTO 的当前 schema 版本。
pub const IDENTITY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 校验主体、角色等必填标识的非空和字节上限。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 检查 SHA-256 文本外形；不会验证摘要来源或其授权含义。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 规范化 trust revision 输入：已有 `sha256:` 标签保持原样，其他文本按 JSON 字符串散列。
///
/// 已带前缀的值仍由后续 validate() 检查长度和十六进制字符；此函数不掩盖格式错误。
fn clean_digest(value: &str) -> String {
    if value.starts_with("sha256:") {
        value.to_owned()
    } else {
        json_digest(&serde_json::json!(value))
    }
}

/// 由 canonical root 派生稳定 ProjectId。
///
/// 取 SHA-256 的前 16 字节并设置 UUID version/variant 位，因此同一 canonical root 可重现相同 ID；
/// 这不是随机 ID，也不检查 root 是否真实存在或由本函数完成 canonicalize。
fn stable_project_id(canonical_root: &str) -> ProjectId {
    let digest = Sha256::digest(canonical_root.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // 将位型调整为 UUID 约定的 version 5 与 RFC variant；其余摘要字节仍提供按 root 的稳定区分。
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    ProjectId::from_uuid(Uuid::from_bytes(bytes))
}

/// 已认证主体的服务端引用；不携带密码、token 或 raw credential。
///
/// 该结构记录认证结论的身份标签、方法、发行者、凭据代数与有效期；它本身不是认证流程或签名凭证。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedPrincipalRef {
    /// 固定 schema 名称。
    pub schema: String,
    /// 向前兼容的 DTO 版本。
    pub version: SchemaVersion,
    /// 服务端使用的主体标识；local helper 当前固定为 `local-user`。
    pub principal_id: String,
    /// 主体如何被上层认定，例如本地 daemon 身份方式标签。
    pub authentication_method: String,
    /// 声明该身份来源的 issuer 标签，不等同于可验证证书。
    pub issuer: String,
    /// 凭据/身份代数；轮换时应递增以区分旧引用。
    pub credential_generation: u64,
    /// Unix epoch 毫秒过期时间；validate() 只检查非零，不读取当前时钟。
    pub expires_at_unix_ms: u64,
    /// 除自身字段外其余主体数据的无密钥 SHA-256 摘要。
    pub principal_digest: String,
}

impl AuthenticatedPrincipalRef {
    /// 构造兼容用的固定本地主体引用并填入摘要。
    ///
    /// 调用者是 Daemon/Core 本地运行路径和 fixtures。返回值使用 `local-user`、`local_os`、`kiana-local`、generation 1、
    /// `u64::MAX` 作为不过期哨兵；这里没有 OS credential lookup，所以不能单凭此函数声称用户已完成系统认证。
    pub fn local() -> Self {
        let mut principal = Self {
            schema: AUTHENTICATED_PRINCIPAL_SCHEMA.to_owned(),
            version: IDENTITY_SCHEMA_VERSION,
            principal_id: "local-user".to_owned(),
            authentication_method: "local_os".to_owned(),
            issuer: "kiana-local".to_owned(),
            credential_generation: 1,
            expires_at_unix_ms: u64::MAX,
            principal_digest: String::new(),
        };
        principal.principal_digest = principal.digest();
        principal
    }

    /// 校验 schema/version、必填主体元数据、非零 generation/expiry 和自摘要。
    ///
    /// 失败返回 `authenticated_principal_header_invalid`、字段格式错误或 `principal_digest_mismatch`。
    /// 不读取当前时间、不验证 issuer/authentication_method，也不证明调用者有权代表该 principal。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTHENTICATED_PRINCIPAL_SCHEMA
            || !self.version.is_compatible_with(&IDENTITY_SCHEMA_VERSION)
            || self.credential_generation == 0
            || self.expires_at_unix_ms == 0
        {
            return Err("authenticated_principal_header_invalid".to_owned());
        }
        required(&self.principal_id, "principal_id", 256)?;
        required(&self.authentication_method, "authentication_method", 64)?;
        required(&self.issuer, "principal_issuer", 256)?;
        digest(&self.principal_digest, "principal_digest")?;
        if self.principal_digest != self.digest() {
            return Err("principal_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 将自身摘要字段清空后，对完整序列化主体引用计算 JSON SHA-256。
    ///
    /// 清空自身字段是为避免摘要递归依赖；serde 无法序列化时当前实现以 JSON null 作为输入。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "principal_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

/// 绑定项目稳定 ID、调用方 root 展示路径、canonical root 和信任版本的快照。
///
/// Daemon 负责先 canonicalize/读取文件系统身份/查询 ProjectTrust，再调用构造器；Domain 不自行执行这些 I/O。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectIdentity {
    /// 固定 schema 名称。
    pub schema: String,
    /// DTO 版本。
    pub version: SchemaVersion,
    /// 从 canonical_root 稳定派生的项目标识。
    pub project_id: ProjectId,
    /// 调用方最初使用的 root 表达，可能是别名。
    pub root: String,
    /// 调用方已经 canonicalize 的根目录字符串；本类型不会替调用方验证它。
    pub canonical_root: String,
    /// 文件系统 device 元数据；跨平台可能不可用。
    #[serde(default)]
    pub device: Option<u64>,
    /// 文件系统 inode 元数据；跨平台可能不可用。
    #[serde(default)]
    pub inode: Option<u64>,
    /// 上层 ProjectTrust 版本/状态摘要，用于把信任变化带入身份快照。
    pub trust_revision: String,
    /// 对项目身份字段计算的无密钥摘要。
    pub identity_digest: String,
}

impl ProjectIdentity {
    /// 根据调用方解析好的项目路径和信任 revision 构造 ProjectIdentity。
    ///
    /// 【调用者】Daemon 的 `project_identity()`；测试也可直接提供 fixture。
    /// 【输入/输出】保留 root 展示路径、canonical_root、可选 device/inode 与 trust revision，返回带稳定 ProjectId 和摘要的 DTO。
    /// 【失败情况】root/canonical_root/revision 为空、字段过长、revision 格式无效或摘要校验失败时返回领域错误。
    /// 【副作用/设计原因】纯数据构造，不访问磁盘；以 canonical_root 派生 ID，让路径别名指向相同项目标识。
    /// 若 revision 是普通文本会先散列；若已经带 `sha256:` 前缀则直接进入校验。
    pub fn new(
        root: impl Into<String>,
        canonical_root: impl Into<String>,
        device: Option<u64>,
        inode: Option<u64>,
        trust_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let canonical_root = canonical_root.into();
        let project_id = stable_project_id(&canonical_root);
        let mut identity = Self {
            schema: PROJECT_IDENTITY_SCHEMA.to_owned(),
            version: IDENTITY_SCHEMA_VERSION,
            project_id,
            root: root.into(),
            canonical_root,
            device,
            inode,
            trust_revision: clean_digest(&trust_revision.into()),
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    /// 校验 schema/version、路径文本、trust revision 和自摘要。
    ///
    /// ⚠ 此方法不再 canonicalize 路径、不回读 device/inode/ProjectTrust，也没有确认 `project_id` 等于 canonical_root 的派生值；
    /// 反序列化值的这些跨字段/外部事实须由 Daemon authority boundary 重新构造或绑定。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_IDENTITY_SCHEMA
            || !self.version.is_compatible_with(&IDENTITY_SCHEMA_VERSION)
        {
            return Err("project_identity_header_invalid".to_owned());
        }
        required(&self.root, "project_root", 4_096)?;
        required(&self.canonical_root, "project_canonical_root", 4_096)?;
        required(&self.trust_revision, "project_trust_revision", 256)?;
        digest(&self.trust_revision, "project_trust_revision")?;
        digest(&self.identity_digest, "project_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("project_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 把显示用 root 归一为 canonical_root 后，对项目身份字段计算摘要。
    ///
    /// 所以仅改变别名 root 不会改变 identity digest；canonical root、device/inode、trust revision 和 project_id 仍参与摘要。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            // root 可能是展示别名；身份摘要以 canonical_root 为准，避免相同项目因不同入口路径产生不同摘要。
            object.insert(
                "root".to_owned(),
                serde_json::Value::String(self.canonical_root.clone()),
            );
            object.insert(
                "identity_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

/// 将主体、项目、会话与角色/部门声明固定在同一份 versioned assignment 中。
///
/// Core 首次绑定 session 时构造它，并把它放进 `session.assigned` EventLog 事件；后续请求不能仅靠改 wire metadata 换角色。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionAssignment {
    /// 固定 schema 名称。
    pub schema: String,
    /// DTO 版本。
    pub version: SchemaVersion,
    /// 经服务端边界产生的主体引用。
    pub principal: AuthenticatedPrincipalRef,
    /// 与本会话关联的项目身份快照。
    pub project: ProjectIdentity,
    /// 此 assignment 所属 session ID。
    pub session_id: SessionId,
    /// 被指派的角色 ID；实际是否允许由 Core role catalog/policy 决定。
    pub role_id: String,
    /// 被指派的部门 ID；与 role 的一致性应由上游查目录并由 session 事件固定。
    pub department_id: String,
    /// 本次 assignment revision；本合同只要求非零，CAS/持久化层负责处理修订竞争。
    pub assignment_revision: u64,
    /// 产生 assignment 时的 authority epoch，用于识别旧授权上下文。
    pub authority_epoch: u64,
    /// 对 assignment 全部字段计算的无密钥摘要。
    pub assignment_digest: String,
}

impl SessionAssignment {
    /// 组装并校验主体/项目/会话/角色/部门绑定，随后计算 assignment 摘要。
    ///
    /// 【调用者】`ControlPlane::bind_session_assignment()`；只应在服务端已确定 RequestContext 后调用。
    /// 【失败情况】嵌套身份无效、session/epoch/revision 为零或角色/部门文本不合法时返回领域错误。
    /// 【副作用】仅构造内存 DTO；真正的 append/CAS、重复 session 冲突处理在 Core EventLog 调用方。
    /// 此构造器不查角色目录、membership 或当前 authority epoch，也不单独授权角色。
    pub fn new(
        principal: AuthenticatedPrincipalRef,
        project: ProjectIdentity,
        session_id: impl Into<String>,
        role_id: impl Into<String>,
        department_id: impl Into<String>,
        assignment_revision: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut assignment = Self {
            schema: SESSION_ASSIGNMENT_SCHEMA.to_owned(),
            version: IDENTITY_SCHEMA_VERSION,
            principal,
            project,
            session_id: SessionId::new(session_id),
            role_id: role_id.into(),
            department_id: department_id.into(),
            assignment_revision,
            authority_epoch,
            assignment_digest: String::new(),
        };
        assignment.assignment_digest = assignment.digest();
        assignment.validate()?;
        Ok(assignment)
    }

    /// 递归校验主体和项目 DTO，再检查本 assignment 的 schema、序号、角色和自摘要。
    ///
    /// 失败时返回具体嵌套错误或 `session_assignment_*` 错误；成功只说明结构自洽，不能证明 assignment 已持久化或仍有效。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SESSION_ASSIGNMENT_SCHEMA
            || !self.version.is_compatible_with(&IDENTITY_SCHEMA_VERSION)
            || self.assignment_revision == 0
            || self.authority_epoch == 0
            || self.session_id.is_empty()
        {
            return Err("session_assignment_header_invalid".to_owned());
        }
        self.principal.validate()?;
        self.project.validate()?;
        required(&self.role_id, "assignment_role", 128)?;
        required(&self.department_id, "assignment_department", 128)?;
        digest(&self.assignment_digest, "assignment_digest")?;
        if self.assignment_digest != self.digest() {
            return Err("session_assignment_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 生成 EventLog/journal 用的 canonical JSON bytes，使同一 typed assignment 得到可稳定比较的编码。
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// 清空自引用摘要字段后计算 assignment 内容摘要。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "assignment_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
