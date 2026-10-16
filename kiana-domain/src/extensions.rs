//! # 文件职责
//!
//! 本文件定义本地扩展（Extension）的领域数据结构和纯校验/摘要函数，包括扩展清单、运行时执行 scope、
//! 依赖图解析结果，以及由父 scope 限定的扩展绑定快照。它不读取插件目录、不验证公钥签名、不加载扩展代码，
//! 也不执行任何能力；类型里的声明本身不会授予权限。
//!
//! 这里的“能力（capability）”指 Broker 可以调度的有限操作；“权限范围（scope）”是对操作、路径、命名空间
//! 或数量等维度的限制集合。scope 中的 SHA-256 摘要是内容指纹，不是签名或授权票据。
//!
//! # Skill 能力请求中的真实调用链
//!
//! ```text
//! Daemon 从 registry/EventLog 读取已启用 Skill
//!   → 验证 manifest 签名与 package 内容
//!   → 创建短期 ExtensionExecutionScope
//!   → KianaHarness 把 scope 放入 CapabilityRequest.arguments["_extension_scopes"]
//!   → CapabilityBroker 解析 scope、检查请求形状/风险/操作
//!   → Daemon ExtensionAdmission 复查 registry generation、启用状态、包和当前 manifest
//!   → 准入通过后才调用具体 CapabilityHandler
//! ```
//!
//! 注册表代次（registry generation）由 Daemon 从 `extension_registry` 事件流折叠得到；每追加一个 registry
//! 事件，流版本递增，之前生成的 scope 会因代次不匹配而失效。
//!
//! `ExtensionManifest::signing_bytes()` 只生成待签/待验的规范字节；签名密钥信任和 Edwards 曲线数字签名算法
//! Ed25519 的验证由 `kiana-daemon/src/extensions.rs` 负责。`ExtensionExecutionScope` 与执行 contract 中的 `*_digest`
//! 是无密钥摘要，用于发现字段变化，不是签名或认证凭据。Scope 仍需与 Daemon 当前读取的 registry 状态和
//! manifest 匹配，不能仅凭调用方传入的 JSON 自行建立信任。
//!
//! # 依赖图与绑定数据
//!
//! `ExtensionDependencyGraph`、`ExtensionGraphResolution` 和 `ExtensionBindingSnapshot` 描述扩展之间的
//! 版本依赖、范围交集和受父 scope 限制的绑定。它们在这里仅是领域值与确定性校验/摘要；本文件不安装、
//! 不调度这些依赖，也不把它们直接转换成 Broker 权限。是否接入生产路径须看实际调用者，而不能从类型存在推断。
//!
//! `serde` 的 `Serialize`/`Deserialize` 派生用于把这些值与 JSON 等外部表示互转；`deny_unknown_fields` 会在
//! 反序列化时拒绝未声明字段，减少版本错配被静默忽略的机会。JSON canonicalization（规范化）在本文件中
//! 只用于生成确定性签名字节/摘要，不改变运行时权限。

use crate::{
    json_digest, CapabilityKind, CapabilityRequest, RiskLevel, RoleSpec, SchemaVersion, ScopeLimit,
    ScopeSet,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// 扩展清单、包、管理操作与 registry 事件流使用的稳定协议标识。
pub const EXTENSION_MANIFEST_SCHEMA: &str = "kiana.extension-manifest.v1";
pub const EXTENSION_PACKAGE_SCHEMA: &str = "kiana.extension-package.v1";
pub const EXTENSION_MANAGE_OPERATION: &str = "extension.manage";
pub const EXTENSION_STREAM: &str = "extension_registry";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
/// 扩展声明的效果级别。它只是一项受信任清单中的上限声明，实际 handler 效果还由 Host 分类。
pub enum ExtensionEffect {
    /// 声明仅允许只读效果；若具体 Host handler 可能写入，准入必须拒绝。
    ReadOnly,
    /// 声明允许具有读写效果的能力；这仍不绕过 ControlPlane 和 Broker 的其他检查。
    ReadWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 扩展提供的功能类别标签；该标签用于描述和筛选，不单独创建对应运行能力。
pub enum ExtensionType {
    /// 为模型上下文提供提示材料的 Skill。
    Skill,
    /// 声明 capability 集成的扩展。
    Capability,
    /// 声明确定性步骤的 workflow 扩展。
    Workflow,
    /// 与 Memory 集成的扩展。
    Memory,
    /// 与模型 provider 集成的扩展。
    Provider,
    /// 提供界面组件的扩展。
    Ui,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
/// 清单声明的网络策略。Allowlist 只表达允许的 host 名单；当前 contract 对 Network 能力仍会拒绝，
/// 直到经审查的 Host 网络 adapter 真正实现并执行对应隔离。
pub enum ExtensionNetworkPolicy {
    /// 不允许扩展请求网络能力。
    Deny,
    /// 清单声明的 host 白名单；白名单字符串本身不是网络连接限制。
    Allowlist { hosts: BTreeSet<String> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 扩展清单的 Ed25519 签名三元组；本类型只承载签名文本，不验证密钥是否可信或签名是否有效。
pub struct ExtensionSignature {
    /// 当前验证实现要求为 `ed25519`。
    pub algorithm: String,
    /// 指向 Daemon 信任密钥表中公钥的标识。
    pub key_id: String,
    /// 对已规范化且移除 signature 字段的 manifest JSON 所做 Ed25519 签名，以小写十六进制保存。
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 扩展安装/启用前声明的运行环境依赖。该类型保存精确版本和能力特征，不执行依赖安装或升级。
pub struct ExtensionRequires {
    /// 需要的 Kiana 版本；使用精确版本字符串，不解释尚未实现的范围语法。
    pub kiana_version: String,
    /// 需要的 versioned protocol 版本。
    pub protocol_version: String,
    #[serde(default)]
    /// 扩展要求存在的 capability 名称及其精确版本。
    pub capability_versions: BTreeMap<String, String>,
    #[serde(default)]
    /// 扩展要求启用的策略特性名称。
    pub policy_features: BTreeSet<String>,
    #[serde(default)]
    /// 扩展可能依赖的 Memory 集合名称。
    pub memory_collections: BTreeSet<String>,
    #[serde(default)]
    /// 扩展声明支持的平台标签。
    pub supported_platforms: BTreeSet<String>,
    #[serde(default)]
    /// 扩展 ID 到所需精确版本的依赖映射。
    pub extensions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 扩展包中描述发布者、内容、所需能力和兼容条件的签名清单。
///
/// `validate()` 只做字段形状和允许范围检查；真正的签名验证由 Daemon 完成。Manifest 中的
/// `required_capabilities`、网络/secret 声明只会参与后续准入边界，不能仅凭清单就执行对应能力。
pub struct ExtensionManifest {
    /// 清单格式标识，必须等于 `EXTENSION_MANIFEST_SCHEMA`。
    pub schema: String,
    /// 扩展在 registry 中的稳定名称。
    pub extension_id: String,
    /// 发布版本字符串；当前格式校验只限制字符，不解析语义版本范围。
    pub version: String,
    /// 发布者稳定标识，与签名公钥的 key id 是不同概念。
    pub publisher: String,
    /// 发布许可证标识或文本；当前只检查非空及长度上限。
    pub license: String,
    /// 扩展内容摘要的裸 SHA-256 十六进制文本；Daemon 负责将其与包文件实际内容核对。
    pub content_hash: String,
    /// 对本清单规范字节的签名载荷；签名字段自身不会包含在签名字节中。
    pub signature: ExtensionSignature,
    /// 供 registry/UI 等描述扩展用途的类别。
    pub extension_type: ExtensionType,
    /// 扩展声明的最大效果级别；实际具体 handler 还会独立分类。
    pub effect: ExtensionEffect,
    #[serde(default)]
    /// 清单声称可提供的能力名称；声明本身不会注册 Broker handler。
    pub provided_capabilities: BTreeSet<String>,
    #[serde(default)]
    /// 扩展运行所需的 capability operation 名称；准入时会逐项核对当前请求操作。
    pub required_capabilities: BTreeSet<String>,
    /// 可以使用该扩展的 Kiana 角色 ID 集合。
    pub supported_roles: BTreeSet<String>,
    #[serde(default)]
    /// 扩展声明接触的数据分类标签。
    pub data_classes: BTreeSet<String>,
    /// 扩展声明的网络主机策略。
    pub network_policy: ExtensionNetworkPolicy,
    #[serde(default)]
    /// 清单引用的 secret handle 名称；这里不包含 secret 明文。
    pub secret_refs: BTreeSet<String>,
    #[serde(default)]
    /// 供扩展配置验证使用的 JSON schema 文档；本类型本身不实例化配置 UI。
    pub configuration_schema: Value,
    #[serde(default)]
    /// 可选迁移说明引用；当前值只做相对路径形状检查。
    pub migration_ref: Option<String>,
    #[serde(default)]
    /// 可选回滚说明引用；当前值只做相对路径形状检查。
    pub rollback_ref: Option<String>,
    /// 安装/启用时应由 Daemon 检查的依赖声明。
    pub requires: ExtensionRequires,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 扩展传输/缓存包：一个 manifest 加上“相对 POSIX 路径 → 十六进制文件字节”的映射。
///
/// 这里不包含 tar/zip 解包逻辑。单独构造此结构不代表路径、摘要、签名或包大小已通过 Daemon 验证。
pub struct ExtensionPackage {
    /// 包结构版本标识。
    pub schema: String,
    /// 描述包身份、能力声明、效果和兼容条件的 manifest。
    pub manifest: ExtensionManifest,
    /// 相对 POSIX 路径到小写十六进制文件字节的映射；本类型不执行压缩包解包。
    pub files: BTreeMap<String, String>,
}

impl ExtensionManifest {
    /// 做清单字段的局部形状校验，不执行 Ed25519 密钥验证。
    ///
    /// 【调用链位置】Daemon 在接受 registry 包时先调用此方法，随后用可信密钥表验证 `signing_bytes()`，
    /// 并实际重算包内容摘要。其他调用者若只调用 `validate()`，只能证明字段符合本地格式规则。
    ///
    /// 【主要拒绝条件】schema/标识符/许可证/摘要格式无效；签名算法、key id 或 64 字节签名对应的
    /// 128 个小写十六进制字符不合规；
    /// 角色/能力声明不合法或超量；allowlist host 形状错误；迁移/回滚引用不是安全的相对路径。
    /// 返回静态错误码供上层映射到 Port/Core 错误；本函数无 I/O 和状态副作用。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != EXTENSION_MANIFEST_SCHEMA
            || !valid_extension_identifier(&self.extension_id)
            || !valid_extension_identifier(&self.publisher)
            || !valid_extension_identifier(&self.version)
            || self.license.trim().is_empty()
            || self.license.len() > 256
            || !is_sha256_hex(&self.content_hash)
        {
            return Err("extension_manifest_invalid");
        }
        if self.signature.algorithm != "ed25519"
            || !valid_extension_identifier(&self.signature.key_id)
            || !is_hex_bytes(&self.signature.value, 64)
        {
            return Err("extension_signature_invalid");
        }
        if self.supported_roles.is_empty()
            || self
                .supported_roles
                .iter()
                .any(|role| RoleSpec::lookup(role).is_none())
            || self.required_capabilities.len() > 64
            || self.provided_capabilities.len() > 64
            || self.requires.extensions.len() > 64
            || self
                .required_capabilities
                .iter()
                .chain(self.provided_capabilities.iter())
                .any(|value| !valid_extension_identifier(value))
        {
            return Err("extension_capability_declaration_invalid");
        }
        if let ExtensionNetworkPolicy::Allowlist { hosts } = &self.network_policy {
            if hosts.is_empty()
                || hosts.len() > 32
                || hosts.iter().any(|host| {
                    host.is_empty()
                        || host.len() > 253
                        || host.starts_with('.')
                        || host.ends_with('.')
                        || host.contains("..")
                        || !host.bytes().all(|b| {
                            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-')
                        })
                })
            {
                return Err("extension_network_policy_invalid");
            }
        }
        for path in [&self.migration_ref, &self.rollback_ref]
            .into_iter()
            .flatten()
        {
            if !valid_extension_path(path) {
                return Err("extension_reference_invalid");
            }
        }
        Ok(())
    }

    /// 生成 Ed25519 签名/验签使用的 manifest 字节：移除签名对象、递归排序 JSON 对象键、再序列化。
    ///
    /// 这样同一份逻辑 manifest 不会因 JSON map 键顺序不同而得到不同签名输入；数组次序保留。
    /// 内容摘要、publisher、效果、依赖和所有其他 manifest 字段仍参与签名。此方法只生成字节，
    /// 不访问私钥、不签名，也不验证 `signature.value`；调用者负责把结果交给加密库。
    pub fn signing_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        value
            .as_object_mut()
            .expect("manifest is an object")
            .remove("signature");
        serde_json::to_vec(&canonical_json(value))
    }

    /// 生成当前 manifest 相对上一版本的差异展示对象。
    ///
    /// `previous = None` 时旧能力集合为空，因此当前的所有 required capability 都列入 added。
    /// 输出还带上前后 effect、network policy、license 和 secret_refs；它是 JSON 差异数据，
    /// 不是策略判定、安装动作或签名证明。
    pub fn capability_diff(&self, previous: Option<&Self>) -> Value {
        let empty = BTreeSet::new();
        let old = previous.map(|p| &p.required_capabilities).unwrap_or(&empty);
        serde_json::json!({
            "added": self.required_capabilities.difference(old).collect::<Vec<_>>(),
            "removed": old.difference(&self.required_capabilities).collect::<Vec<_>>(),
            "effect_before": previous.map(|p| p.effect), "effect_after": self.effect,
            "network_before": previous.map(|p| &p.network_policy), "network_after": self.network_policy,
            "license_before": previous.map(|p| &p.license), "license_after": self.license,
            "secret_refs_before": previous.map(|p| &p.secret_refs), "secret_refs_after": self.secret_refs,
        })
    }
}

/// Host 根据已检查的 registry 状态生成的短期执行 scope，绝不能由模型参数或 Skill frontmatter 自行授予。
///
/// 它把扩展包摘要、当前 registry generation、角色、能力声明、策略摘要和有效时间绑定在一起，随后由
/// `KianaHarness` 附加到能力请求。`scope_digest` 是无密钥摘要：篡改后可以被检测，但本身不能验证签发者；
/// Broker 还要调用 `ExtensionAdmission`，由 Daemon 对照当前 registry 和 manifest 复查。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionExecutionScope {
    /// 当前 scope 数据结构的 schema 标识。
    pub schema: String,
    /// 与 schema 配套的显式版本号。
    pub version: SchemaVersion,
    /// 与已启用 manifest 对应的扩展标识。
    pub extension_id: String,
    /// Manifest 声明的发布者。
    pub publisher: String,
    /// 当前 registry 记录的缓存包 SHA-256 摘要，用于把 scope 绑定到某一份包。
    pub package_sha256: String,
    /// Manifest 所声明的扩展内容摘要。
    pub content_hash: String,
    /// 创建 scope 时观察到的 `extension_registry` stream version；新增 registry 事件后旧 scope 应失效。
    pub registry_generation: u64,
    /// 本次 scope 绑定的 Kiana 角色。
    pub role_id: String,
    /// 扩展 manifest 声明的效果上限。
    pub effect: ExtensionEffect,
    /// 此扩展请求可使用的 operation 名称集合；Broker 还会把当前请求 operation 与之核对。
    pub required_capabilities: BTreeSet<String>,
    /// Manifest 的网络策略声明；当前声明 allowlist 不会单独启用网络 adapter。
    pub network_policy: ExtensionNetworkPolicy,
    /// Manifest 允许使用该扩展的角色集合。
    pub supported_roles: BTreeSet<String>,
    /// `capability_diff(None)` 的摘要；新 scope 中它描述当前 required capability 相对空集合的差异。
    pub capability_diff_digest: String,
    /// 当前 network policy JSON 的摘要。
    pub network_policy_digest: String,
    /// Manifest secret handle 引用集合的摘要；不含 secret 明文。
    pub secret_policy_digest: String,
    /// Host 签发该 scope 的 Unix 毫秒时间。
    pub issued_at_unix_ms: u64,
    /// scope 的排他过期时间；now 等于该值时即视为过期。
    pub expires_at_unix_ms: u64,
    /// 除本字段自身外 scope 内容的 SHA-256 摘要，不是数字签名。
    pub scope_digest: String,
}

impl ExtensionExecutionScope {
    /// 此 host-owned scope 的结构版本；修改纳入摘要的字段时须同步考虑版本兼容。
    pub const SCHEMA: &'static str = "kiana.extension-execution-scope.v2";
    pub const VERSION: SchemaVersion = SchemaVersion::new(2, 0);

    /// 将 Daemon 已检查的 manifest 快照整理为适合附加到 `CapabilityRequest` 的 scope。
    ///
    /// 【输入】manifest、包摘要、创建时的 registry generation、角色，以及有效期起止时间。
    /// 本函数会检查 manifest 的字段形状并确认角色属于 `supported_roles`，但不验证发布者签名、
    /// 不读取 registry，也不核对包摘要对应的磁盘文件；真实调用链由 Daemon 先完成这些信任检查。
    ///
    /// 【核心流程】复制 manifest 中影响准入的字段 → 为能力差异、网络策略和 secret 引用计算摘要
    /// → 计算整个 scope 的摘要 → 调用 `validate()` 自检。所有摘要都是无密钥 hash。
    ///
    /// 【失败与副作用】无效 manifest/角色/时间/包摘要或策略 JSON 转换失败时返回错误字符串；
    /// 不读写 EventLog、不延长 registry 生命周期，也不注册能力 handler。
    pub fn new(
        manifest: &ExtensionManifest,
        package_sha256: impl Into<String>,
        registry_generation: u64,
        role_id: impl Into<String>,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        manifest.validate().map_err(str::to_owned)?;
        let mut scope = Self {
            schema: Self::SCHEMA.to_owned(),
            version: Self::VERSION,
            extension_id: manifest.extension_id.clone(),
            publisher: manifest.publisher.clone(),
            package_sha256: package_sha256.into(),
            content_hash: manifest.content_hash.clone(),
            registry_generation,
            role_id: role_id.into(),
            effect: manifest.effect,
            required_capabilities: manifest.required_capabilities.clone(),
            network_policy: manifest.network_policy.clone(),
            supported_roles: manifest.supported_roles.clone(),
            capability_diff_digest: json_digest(&manifest.capability_diff(None)),
            network_policy_digest: json_digest(
                &serde_json::to_value(&manifest.network_policy)
                    .map_err(|_| "extension_scope_policy_serialize".to_owned())?,
            ),
            secret_policy_digest: json_digest(&json!({
                "secret_refs": manifest.secret_refs,
            })),
            issued_at_unix_ms,
            expires_at_unix_ms,
            scope_digest: String::new(),
        };
        scope.scope_digest = scope.digest();
        scope.validate()?;
        Ok(scope)
    }

    /// 检查 scope 的 schema、标识符、非零 generation/时间、摘要格式和自摘要。
    ///
    /// 这是局部结构校验；它不能证明签名有效、扩展仍启用、generation 仍是最新值或时间已生效。
    /// 这些上下文检查分别由 `validate_at()` 和 Daemon 的 `ExtensionAdmission` 负责。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA
            || self.version != Self::VERSION
            || !valid_extension_identifier(&self.extension_id)
            || !valid_extension_identifier(&self.publisher)
            || !valid_extension_identifier(&self.role_id)
            || self.registry_generation == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || !is_sha256_hex(&self.package_sha256)
            || !is_sha256_hex(&self.content_hash)
        {
            return Err("extension_scope_header_invalid".to_owned());
        }
        if !self.supported_roles.contains(&self.role_id) {
            return Err("extension_scope_role_not_supported".to_owned());
        }
        for (value, field) in [
            (
                &self.capability_diff_digest,
                "extension_scope_capability_diff_digest",
            ),
            (
                &self.network_policy_digest,
                "extension_scope_network_policy_digest",
            ),
            (
                &self.secret_policy_digest,
                "extension_scope_secret_policy_digest",
            ),
            (&self.scope_digest, "extension_scope_digest"),
        ] {
            if !value.starts_with("sha256:")
                || value.len() != 71
                || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(format!("{field}_invalid"));
            }
        }
        if self.scope_digest != self.digest() {
            return Err("extension_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 在当前时间、registry generation 和角色上下文中检查这个 scope 是否仍可用。
    ///
    /// 先调用 `validate()`，再拒绝尚未到签发时间、到期（now >= expires）、generation 变化或角色不符。
    /// 时间/generation/role 任一不匹配都会返回分类错误，调用方应停止准入；本方法不自动刷新 scope。
    /// 它仍不查询 registry，也不与 manifest 内容比较，生产准入需由 Daemon 继续调用 admission。
    pub fn validate_at(
        &self,
        now_unix_ms: u64,
        registry_generation: u64,
        role_id: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if now_unix_ms < self.issued_at_unix_ms {
            return Err("extension_scope_not_yet_valid".to_owned());
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("extension_scope_expired".to_owned());
        }
        if self.registry_generation != registry_generation {
            return Err("extension_scope_generation_stale".to_owned());
        }
        if self.role_id != role_id {
            return Err("extension_scope_role_mismatch".to_owned());
        }
        Ok(())
    }

    /// 将 scope 中记录的策略字段与一个 manifest 做逐项比较。
    ///
    /// 该比较用于发现签发 scope 后 manifest 的身份、内容摘要、效果、能力、网络/角色/secret 声明发生变化。
    /// 本方法先检查 manifest 的结构，再比较字段；它不验证 manifest 的 Ed25519 签名，也不检查 scope 自身的
    /// digest、过期时间或 registry generation，调用者应先执行对应校验。
    pub fn matches_manifest(&self, manifest: &ExtensionManifest) -> Result<(), String> {
        manifest.validate().map_err(str::to_owned)?;
        if self.extension_id != manifest.extension_id
            || self.publisher != manifest.publisher
            || self.content_hash != manifest.content_hash
            || self.effect != manifest.effect
            || self.required_capabilities != manifest.required_capabilities
            || self.network_policy != manifest.network_policy
            || self.supported_roles != manifest.supported_roles
            || self.capability_diff_digest != json_digest(&manifest.capability_diff(None))
            || self.network_policy_digest
                != json_digest(
                    &serde_json::to_value(&manifest.network_policy)
                        .map_err(|_| "extension_scope_policy_serialize".to_owned())?,
                )
            || self.secret_policy_digest
                != json_digest(&json!({"secret_refs": manifest.secret_refs}))
        {
            return Err("extension_scope_policy_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 把可序列化的 host scope 转成 Broker 内部检查用 contract，并附上 Broker 判定的 handler 效果。
    ///
    /// `handler_effect` 不来自 manifest 或模型请求，而由 Broker 根据实际 capability/operation 分类。
    /// 此方法只是字段复制，不会调用 `validate()` 或产生准入决定；转换后仍须调用 contract 的 `check()`
    /// 和上层 `ExtensionAdmission`。
    pub fn contract(&self, handler_effect: ExtensionEffect) -> ExtensionExecutionContract {
        ExtensionExecutionContract {
            extension_id: self.extension_id.clone(),
            publisher: self.publisher.clone(),
            package_sha256: self.package_sha256.clone(),
            content_hash: self.content_hash.clone(),
            registry_generation: self.registry_generation,
            role_id: self.role_id.clone(),
            effect: self.effect,
            required_capabilities: self.required_capabilities.clone(),
            network_policy: self.network_policy.clone(),
            supported_roles: self.supported_roles.clone(),
            capability_diff_digest: self.capability_diff_digest.clone(),
            network_policy_digest: self.network_policy_digest.clone(),
            secret_policy_digest: self.secret_policy_digest.clone(),
            issued_at_unix_ms: self.issued_at_unix_ms,
            expires_at_unix_ms: self.expires_at_unix_ms,
            scope_digest: self.scope_digest.clone(),
            handler_effect,
        }
    }

    /// 对参与执行准入的 scope 字段计算确定性摘要，忽略 `scope_digest` 自身。
    ///
    /// 这是 `json_digest()` 产生的无密钥完整性指纹；信任仍来自 Daemon 验证的 registry/manifest，
    /// 不能把此摘要写成数字签名或独立授权凭证。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "extension_id": self.extension_id,
            "publisher": self.publisher,
            "package_sha256": self.package_sha256,
            "content_hash": self.content_hash,
            "registry_generation": self.registry_generation,
            "role_id": self.role_id,
            "effect": self.effect,
            "required_capabilities": self.required_capabilities,
            "network_policy": self.network_policy,
            "supported_roles": self.supported_roles,
            "capability_diff_digest": self.capability_diff_digest,
            "network_policy_digest": self.network_policy_digest,
            "secret_policy_digest": self.secret_policy_digest,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

/// 从 `CapabilityRequest.arguments` 中读取 Harness 附加的扩展 scope 列表。
///
/// 【调用者与链路】KianaHarness 把 Host 生成的 scope 放到保留字段 `_extension_scopes`；Broker 在调用
/// handler 前使用本函数反序列化并做结构自检，之后创建 contract 并调用 Daemon `ExtensionAdmission`。
///
/// 【结果】字段不存在返回 `Ok(None)`，表示这是普通能力请求；字段存在时返回已解码 scope 列表。
/// 列表超过 64 项、JSON 形状不符或任一 scope 无效都返回固定错误码。此函数没有当前时间、registry 或
/// manifest 入参，所以仅验证结构，不能证明 scope 可信、未过期或扩展仍 enabled。
pub fn request_extension_scopes(
    request: &CapabilityRequest,
) -> Result<Option<Vec<ExtensionExecutionScope>>, &'static str> {
    request
        .arguments
        .get("_extension_scopes")
        .map(|value| {
            let scopes: Vec<ExtensionExecutionScope> =
                serde_json::from_value(value.clone()).map_err(|_| "extension_scope_invalid")?;
            if scopes.len() > 64 {
                return Err("extension_scope_limit_exceeded");
            }
            for scope in &scopes {
                scope.validate().map_err(|_| "extension_scope_invalid")?;
            }
            Ok(scopes)
        })
        .transpose()
}

/// Broker 在执行扩展包装 handler 前使用的内部准入合同。
///
/// 与可放入请求参数的 `ExtensionExecutionScope` 相比，它多出 `handler_effect`：实际处理器的效果分类由
/// Broker 根据 concrete capability/operation 决定。Broker 先调用 `check()` 对照当前请求，Daemon admission
/// 再回读 registry 并检查 manifest，只有两步都通过才会进入 handler。
#[derive(Clone, Debug)]
pub struct ExtensionExecutionContract {
    /// 当前扩展稳定身份。
    pub extension_id: String,
    /// 清单中的 publisher 身份。
    pub publisher: String,
    /// 被 registry 选中的缓存包摘要。
    pub package_sha256: String,
    /// 扩展内容摘要。
    pub content_hash: String,
    /// 创建 contract 时观察到的 `extension_registry` stream version。
    pub registry_generation: u64,
    /// 本次请求绑定的角色。
    pub role_id: String,
    /// 清单声明的效果上限。
    pub effect: ExtensionEffect,
    /// 可调用的 operation 名称。
    pub required_capabilities: BTreeSet<String>,
    /// 清单中的网络策略声明。
    pub network_policy: ExtensionNetworkPolicy,
    /// 清单允许的角色集合。
    pub supported_roles: BTreeSet<String>,
    /// 能力声明差异摘要。
    pub capability_diff_digest: String,
    /// 网络策略摘要。
    pub network_policy_digest: String,
    /// secret handle 声明摘要。
    pub secret_policy_digest: String,
    /// contract 生效起始时间。
    pub issued_at_unix_ms: u64,
    /// contract 排他过期时间。
    pub expires_at_unix_ms: u64,
    /// 对共享 scope 字段的无密钥摘要。
    pub scope_digest: String,
    /// 由 Broker 对具体 Host adapter 分类的效果级别，与请求调用方提供的 risk 标签分开。
    pub handler_effect: ExtensionEffect,
}

impl ExtensionExecutionContract {
    /// 对照当前时间、registry generation、role 和 scope 摘要检查 contract 新鲜度。
    ///
    /// 该方法不读取 registry，也不校验 manifest 签名；它只使用调用方传入的上下文和 contract 自身字段。
    pub fn validate_at(
        &self,
        now_unix_ms: u64,
        registry_generation: u64,
        role_id: &str,
    ) -> Result<(), String> {
        if now_unix_ms < self.issued_at_unix_ms {
            return Err("extension_scope_not_yet_valid".to_owned());
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("extension_scope_expired".to_owned());
        }
        if self.registry_generation != registry_generation {
            return Err("extension_scope_generation_stale".to_owned());
        }
        if self.role_id != role_id {
            return Err("extension_scope_role_mismatch".to_owned());
        }
        if !is_prefixed_sha256_hex(&self.capability_diff_digest)
            || !is_prefixed_sha256_hex(&self.network_policy_digest)
            || !is_prefixed_sha256_hex(&self.secret_policy_digest)
            || !is_prefixed_sha256_hex(&self.scope_digest)
        {
            return Err("extension_execution_contract_invalid".to_owned());
        }
        if self.scope_digest != self.canonical_scope_digest() {
            return Err("extension_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 将 contract 中捕获的扩展策略字段与 Daemon 当前 registry 中的 manifest 核对。
    ///
    /// 先检查 manifest 形状和 contract 自摘要，再逐项比对身份、内容、效果、capability、网络/角色/secret
    /// 声明及其摘要。包签名和包字节的真实性由 Daemon 在 registry 加载/缓存校验阶段负责；本函数自身不读包。
    pub fn matches_manifest(&self, manifest: &ExtensionManifest) -> Result<(), String> {
        manifest.validate().map_err(str::to_owned)?;
        if self.scope_digest != self.canonical_scope_digest() {
            return Err("extension_scope_digest_mismatch".to_owned());
        }
        if self.extension_id != manifest.extension_id
            || self.publisher != manifest.publisher
            || self.content_hash != manifest.content_hash
            || self.effect != manifest.effect
            || self.required_capabilities != manifest.required_capabilities
            || self.network_policy != manifest.network_policy
            || self.supported_roles != manifest.supported_roles
            || self.capability_diff_digest != json_digest(&manifest.capability_diff(None))
            || self.network_policy_digest
                != json_digest(
                    &serde_json::to_value(&manifest.network_policy)
                        .map_err(|_| "extension_scope_policy_serialize".to_owned())?,
                )
            || self.secret_policy_digest
                != json_digest(&json!({"secret_refs": manifest.secret_refs}))
        {
            return Err("extension_scope_policy_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 检查一个 Broker 请求是否满足当前扩展 execution contract 的局部准入条件。
    ///
    /// 【调用者与数据流】Broker 从 Harness 附加的 scope 构造 contract，并先用此方法检查将要调用的
    /// `CapabilityRequest`；本方法通过后，仍需 Daemon `ExtensionAdmission` 回读 registry/manifest，
    /// 两层都通过后 Broker 才能进入具体 handler。
    ///
    /// 【检查顺序】验证扩展身份、generation、有效时间字段和摘要格式 → 重算无密钥 scope 摘要 →
    /// 确认请求 `role_id` 与 contract 绑定角色完全相同 → 若清单声明 ReadOnly，则同时要求实际
    /// `handler_effect` 和请求 `risk` 都是 ReadOnly，且 capability 不是 Process/Secret/Computer →
    /// 检查 operation 在 `required_capabilities` 中 → 确认角色在清单支持集合内 → 检查 Network 能力。
    ///
    /// Network=Deny 返回 `extension_network_denied`；即使是 Allowlist，目前也返回
    /// `extension_network_adapter_unavailable`，因为清单名单尚未接入真正的 Host 网络隔离 adapter。
    /// 任一错误都应让 Broker 停止调用 handler；本方法不执行请求、不重试，也不改变 registry。
    /// 它没有当前时间或当前 registry generation 入参，过期/代次新鲜度应由 `validate_at()` 另行检查。
    pub fn check(&self, request: &CapabilityRequest) -> Result<(), &'static str> {
        if !valid_extension_identifier(&self.extension_id)
            || !valid_extension_identifier(&self.publisher)
            || !valid_extension_identifier(&self.role_id)
            || self.registry_generation == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || !is_sha256_hex(&self.content_hash)
            || !is_sha256_hex(&self.package_sha256)
            || !is_prefixed_sha256_hex(&self.capability_diff_digest)
            || !is_prefixed_sha256_hex(&self.network_policy_digest)
            || !is_prefixed_sha256_hex(&self.secret_policy_digest)
            || !is_prefixed_sha256_hex(&self.scope_digest)
        {
            return Err("extension_execution_contract_invalid");
        }
        if self.scope_digest != self.canonical_scope_digest() {
            return Err("extension_scope_digest_mismatch");
        }
        if request.arguments.get("role_id").and_then(Value::as_str) != Some(self.role_id.as_str()) {
            return Err("extension_scope_role_mismatch");
        }
        if self.effect == ExtensionEffect::ReadOnly
            && (self.handler_effect != ExtensionEffect::ReadOnly
                || request.risk != RiskLevel::ReadOnly
                || matches!(
                    request.capability,
                    CapabilityKind::Process | CapabilityKind::Secret | CapabilityKind::Computer
                ))
        {
            return Err("extension_read_only_write_denied");
        }
        if !self.required_capabilities.contains(&request.operation) {
            return Err("extension_required_capability_missing");
        }
        if request
            .arguments
            .get("role_id")
            .and_then(Value::as_str)
            .is_none_or(|role| !self.supported_roles.contains(role))
        {
            return Err("extension_role_denied");
        }
        if request.capability == CapabilityKind::Network {
            match &self.network_policy {
                ExtensionNetworkPolicy::Deny => return Err("extension_network_denied"),
                // 真实网络隔离必须由经过审查的 Host adapter 执行。
                // 在该 adapter 接入前，清单中的 host 白名单不能启用网络访问。
                ExtensionNetworkPolicy::Allowlist { .. } => {
                    return Err("extension_network_adapter_unavailable")
                }
            }
        }
        Ok(())
    }

    /// 重新计算共享 scope 字段的 SHA-256 摘要，不纳入 Broker 的 handler 效果分类。
    ///
    /// `handler_effect` 是 Host 对具体 adapter 的分类，不属于传入的 ExtensionExecutionScope。
    /// 这里所谓“重新计算”只是校验无密钥摘要，不执行签名；真正的信任需要 registry/manifest admission。
    pub fn canonical_scope_digest(&self) -> String {
        json_digest(&json!({
            "schema": ExtensionExecutionScope::SCHEMA,
            "version": ExtensionExecutionScope::VERSION,
            "extension_id": self.extension_id,
            "publisher": self.publisher,
            "package_sha256": self.package_sha256,
            "content_hash": self.content_hash,
            "registry_generation": self.registry_generation,
            "role_id": self.role_id,
            "effect": self.effect,
            "required_capabilities": self.required_capabilities,
            "network_policy": self.network_policy,
            "supported_roles": self.supported_roles,
            "capability_diff_digest": self.capability_diff_digest,
            "network_policy_digest": self.network_policy_digest,
            "secret_policy_digest": self.secret_policy_digest,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

/// 递归排序 JSON 对象的键，生成适合稳定序列化/摘要计算的规范 JSON 值。
///
/// 对象键收集到 `BTreeMap` 后按字典序排列；数组顺序和标量值原样保留。本函数只规范结构顺序，
/// 不验证扩展字段，也不执行签名。
pub fn canonical_json(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, canonical_json(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(canonical_json).collect()),
        other => other,
    }
}

/// 检查扩展 ID、发布者 ID、版本标签和图节点 ID 使用的有限 ASCII 字符集。
///
/// 允许 ASCII 字母/数字、点、连字符和下划线，最多 128 字节，并拒绝 `.` 与 `..`；
/// 只证明标识符形状合规，不证明 registry 中存在该对象。
pub fn valid_extension_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// 对扩展包内部文件引用执行词法相对路径检查。
///
/// 拒绝绝对路径、反斜线、控制字符、空路径段、`.` / `..` 和含冒号的路径段，最多 512 字节。
/// 这不是 filesystem canonicalize，也不检查符号链接目标；真正读取文件时仍由 Daemon 的受限目录 API 管理。
pub fn valid_extension_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains(':'))
}

/// 检查不带 `sha256:` 前缀的裸 SHA-256 摘要格式，即 64 个小写十六进制字符。
///
/// 本函数只校验形状；上层必须把实际数据重新计算出的摘要与该值比较，才能判断内容是否匹配。
pub fn is_sha256_hex(value: &str) -> bool {
    is_hex_bytes(value, 32)
}

/// 校验带 `sha256:` 前缀、总长 71 字节的 SHA-256 文本；不重算摘要或认证其来源。
fn is_prefixed_sha256_hex(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// 校验表示 `length` 个原始字节的小写十六进制字符串；每个字节对应两个字符。
pub fn is_hex_bytes(value: &str, length: usize) -> bool {
    value.len() == length * 2
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// 下面的依赖图/绑定类型目前由领域测试构造，尚未找到生产调用点。它们只校验元数据关系；
// 即使图或绑定有效，也不会安装依赖、启动扩展或授予 capability。实际副作用仍归 ControlPlane/Broker。
/// 依赖图、解析结果、绑定和绑定快照各自使用的 schema 标识。
pub const EXTENSION_DEPENDENCY_GRAPH_SCHEMA: &str = "kiana.extension-dependency-graph.v1";
pub const EXTENSION_GRAPH_RESOLUTION_SCHEMA: &str = "kiana.extension-graph-resolution.v1";
pub const EXTENSION_BINDING_SCHEMA: &str = "kiana.extension-binding.v1";
pub const EXTENSION_BINDING_SNAPSHOT_SCHEMA: &str = "kiana.extension-binding-snapshot.v1";
pub const EXTENSION_DEPENDENCY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 单张依赖图最多包含的节点数，限制验证与环检测的输入规模。
pub const MAX_EXTENSION_GRAPH_NODES: usize = 256;
/// 单个节点最多声明的依赖数。
pub const MAX_EXTENSION_DEPENDENCIES: usize = 64;
/// 一个绑定快照最多容纳的绑定条目数。
pub const MAX_EXTENSION_BINDINGS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 扩展依赖图中的节点类别；用于要求依赖的 kind 与目标节点一致。
pub enum ExtensionGraphKind {
    /// Skill 指令材料节点。
    Skill,
    /// Hook 节点。
    Hook,
    /// MCP 节点。
    Mcp,
    /// capability 节点。
    Capability,
    /// Memory 节点。
    Memory,
    /// provider 节点。
    Provider,
    /// UI 组件节点。
    UiComponent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一条对另一个扩展节点的版本/类别依赖声明。
pub struct ExtensionDependency {
    /// 目标节点的稳定 ID。
    pub dependency_id: String,
    /// 必须完全匹配的依赖版本；当前不解析范围表达式。
    pub version: String,
    /// 目标扩展必须属于的类型。
    pub kind: ExtensionGraphKind,
    #[serde(default)]
    /// 目标缺失时是否允许继续验证图；如果目标存在，仍会检查版本、类型和 scope。
    pub optional: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 可选的额外 scope 要求；若提供，必须同时与依赖源和目标 scope 有交集。
    pub required_scope: Option<ScopeSet>,
}

impl ExtensionDependency {
    /// 检查依赖 ID/版本的字符形状，并校验可选 scope 的内部结构。
    fn validate(&self) -> Result<(), String> {
        if !valid_extension_identifier(&self.dependency_id)
            || !valid_extension_identifier(&self.version)
        {
            return Err("extension_dependency_identity_invalid".to_owned());
        }
        if let Some(scope) = &self.required_scope {
            scope.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 依赖图中的一个扩展节点及其版本、来源摘要、平台、依赖列表和能力范围。
pub struct ExtensionGraphNode {
    /// 本张图中的节点键；构图时同时作为 `nodes` map 的 key。
    pub node_id: String,
    /// 节点对应的扩展 ID，可与同一扩展的不同版本节点区分。
    pub extension_id: String,
    /// 节点表示的精确扩展版本。
    pub version: String,
    /// 节点所属的扩展类别。
    pub kind: ExtensionGraphKind,
    /// 对源材料计算的裸 SHA-256 十六进制摘要，不带 `sha256:` 前缀。
    pub source_digest: String,
    #[serde(default)]
    /// 此节点依赖的其他节点；构图时会按 ID/版本/类别排序。
    pub dependencies: Vec<ExtensionDependency>,
    #[serde(default)]
    /// 节点声称支持的平台集合；空集合在图校验中表示不限定平台。
    pub supported_platforms: BTreeSet<String>,
    /// 此节点可用的范围上限；边校验只检查与依赖方有非空交集，不扩大任一方范围。
    pub scope: ScopeSet,
}

impl ExtensionGraphNode {
    /// 校验单个节点、scope、依赖排序/重复项和平台名称，不访问节点源文件。
    fn validate(&self) -> Result<(), String> {
        if !valid_extension_identifier(&self.node_id)
            || !valid_extension_identifier(&self.extension_id)
            || !valid_extension_identifier(&self.version)
            || !is_sha256_hex(&self.source_digest)
            || self.dependencies.len() > MAX_EXTENSION_DEPENDENCIES
            || self.supported_platforms.len() > 16
        {
            return Err("extension_graph_node_invalid".to_owned());
        }
        self.scope.validate()?;
        for platform in &self.supported_platforms {
            if !valid_extension_identifier(platform) {
                return Err("extension_graph_platform_invalid".to_owned());
            }
        }
        for dependency in &self.dependencies {
            dependency.validate()?;
        }
        if self.dependencies.windows(2).any(|pair| {
            (&pair[0].dependency_id, &pair[0].version, pair[0].kind)
                > (&pair[1].dependency_id, &pair[1].version, pair[1].kind)
        }) {
            return Err("extension_dependency_order_invalid".to_owned());
        }
        if self
            .dependencies
            .windows(2)
            .any(|pair| pair[0].dependency_id == pair[1].dependency_id)
        {
            return Err("extension_dependency_duplicate".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 针对一个目标平台整理好的扩展依赖有向图。
///
/// `nodes` 使用有序 map 让摘要输入和遍历顺序稳定；图本身只是一份依赖关系快照，
/// 不代表图中节点已安装或可执行。当前源码中直接构造/解析该类型的调用仅见于领域测试。
pub struct ExtensionDependencyGraph {
    /// 依赖图格式标识。
    pub schema: String,
    /// 依赖图格式版本。
    pub version: SchemaVersion,
    /// 本图所针对的单个平台。
    pub platform: String,
    /// 以 `node_id` 为键的节点集合。
    pub nodes: BTreeMap<String, ExtensionGraphNode>,
    /// 除本字段自身外图内容的 SHA-256 摘要。
    pub graph_digest: String,
}

impl ExtensionDependencyGraph {
    /// 从节点列表创建有序依赖图，并在返回前运行全图校验。
    ///
    /// 【核心流程】先对每个节点内的 dependency 列表排序 → 按 node_id 放入 BTreeMap 并拒绝重复节点
    /// → 计算图摘要 → 调用 `validate()` 检查缺失依赖、版本/kind、平台、scope 交集和循环。
    ///
    /// 【为什么先排序】JSON 数组顺序会参与摘要；构造器固定依赖顺序，确保同一组输入节点得到稳定结果。
    /// 输入节点列表本身按 map key 归一，重复的 node_id 会失败而不是由后一个静默覆盖。
    /// 本方法只返回值对象，无文件、网络或安装副作用。
    pub fn new(
        platform: impl Into<String>,
        nodes: Vec<ExtensionGraphNode>,
    ) -> Result<Self, String> {
        let mut indexed = BTreeMap::new();
        for mut node in nodes {
            node.dependencies.sort_by(|left, right| {
                (&left.dependency_id, &left.version, left.kind).cmp(&(
                    &right.dependency_id,
                    &right.version,
                    right.kind,
                ))
            });
            node.validate()?;
            if indexed.insert(node.node_id.clone(), node).is_some() {
                return Err("extension_graph_node_duplicate".to_owned());
            }
        }
        let mut graph = Self {
            schema: EXTENSION_DEPENDENCY_GRAPH_SCHEMA.to_owned(),
            version: EXTENSION_DEPENDENCY_VERSION,
            platform: platform.into(),
            nodes: indexed,
            graph_digest: String::new(),
        };
        graph.graph_digest = graph.digest();
        graph.validate()?;
        Ok(graph)
    }

    /// 校验图的格式、节点及所有依赖边，再核对循环和图摘要。
    ///
    /// 必需但缺失的目标返回 `extension_dependency_unsatisfied`；optional 缺失目标被跳过。
    /// 目标存在时仍要精确匹配版本与 kind，满足平台限制，且两端 scope 必须有交集；
    /// `required_scope` 若存在，也必须分别与两端有交集。这里仅验证交集非空，不计算/写回收窄后的 scope。
    /// 深度优先搜索（Depth-First Search，DFS）使用 `visiting` 识别回边、`visited` 避免重复遍历，
    /// 发现循环会返回错误。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_DEPENDENCY_GRAPH_SCHEMA
            || self.version != EXTENSION_DEPENDENCY_VERSION
            || !valid_extension_identifier(&self.platform)
            || self.nodes.len() > MAX_EXTENSION_GRAPH_NODES
            || !is_prefixed_digest(&self.graph_digest)
        {
            return Err("extension_dependency_graph_header_invalid".to_owned());
        }
        for node in self.nodes.values() {
            node.validate()?;
            if !node.supported_platforms.is_empty()
                && !node.supported_platforms.contains(&self.platform)
            {
                return Err("extension_dependency_platform_mismatch".to_owned());
            }
            for dependency in &node.dependencies {
                let Some(target) = self.nodes.get(&dependency.dependency_id) else {
                    if dependency.optional {
                        continue;
                    }
                    return Err("extension_dependency_unsatisfied".to_owned());
                };
                if target.version != dependency.version {
                    return Err("extension_dependency_version_unavailable".to_owned());
                }
                if target.kind != dependency.kind {
                    return Err("extension_dependency_kind_mismatch".to_owned());
                }
                if !target.supported_platforms.is_empty()
                    && !target.supported_platforms.contains(&self.platform)
                {
                    return Err("extension_dependency_platform_mismatch".to_owned());
                }
                node.scope
                    .intersect(&target.scope)
                    .map_err(|_| "extension_dependency_scope_disjoint".to_owned())?;
                if let Some(required_scope) = &dependency.required_scope {
                    required_scope
                        .intersect(&node.scope)
                        .map_err(|_| "extension_dependency_scope_disjoint".to_owned())?;
                    required_scope
                        .intersect(&target.scope)
                        .map_err(|_| "extension_dependency_scope_disjoint".to_owned())?;
                }
            }
        }
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for node_id in self.nodes.keys() {
            visit_extension_node(node_id, &self.nodes, &mut visiting, &mut visited)?;
        }
        if self.graph_digest != self.digest() {
            return Err("extension_dependency_graph_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 把已验证的图转换成按 source/target/version/kind 排序的依赖边记录。
    ///
    /// 缺失的 optional 目标不会出现在输出中；存在的目标会生成 `ResolvedExtensionDependency`，
    /// 并保留原始 required scope 的摘要。结果只描述解析关系，不下载、安装或启动目标扩展。
    /// 图错误、依赖不满足、循环或结果自检失败都会作为 `Err(String)` 返回。
    pub fn resolve(&self) -> Result<ExtensionGraphResolution, String> {
        self.validate()?;
        let mut resolved_dependencies = Vec::new();
        for node in self.nodes.values() {
            for dependency in &node.dependencies {
                if let Some(target) = self.nodes.get(&dependency.dependency_id) {
                    resolved_dependencies.push(ResolvedExtensionDependency {
                        source_node_id: node.node_id.clone(),
                        target_node_id: target.node_id.clone(),
                        version: target.version.clone(),
                        kind: target.kind,
                        required_scope_digest: dependency
                            .required_scope
                            .as_ref()
                            .map(ScopeSet::digest),
                    });
                }
            }
        }
        resolved_dependencies.sort_by(|left, right| {
            (
                &left.source_node_id,
                &left.target_node_id,
                &left.version,
                left.kind,
            )
                .cmp(&(
                    &right.source_node_id,
                    &right.target_node_id,
                    &right.version,
                    right.kind,
                ))
        });
        let mut resolution = ExtensionGraphResolution {
            schema: EXTENSION_GRAPH_RESOLUTION_SCHEMA.to_owned(),
            version: EXTENSION_DEPENDENCY_VERSION,
            graph_digest: self.graph_digest.clone(),
            platform: self.platform.clone(),
            resolved_dependencies,
            resolution_digest: String::new(),
        };
        resolution.resolution_digest = resolution.digest();
        resolution.validate()?;
        Ok(resolution)
    }

    /// 对 schema、版本、平台和节点集合计算稳定摘要；摘要本身不是签名。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "platform": self.platform,
            "nodes": self.nodes,
        }))
    }
}

/// 用深度优先搜索（Depth-First Search，DFS）遍历依赖边，区分“正在访问”和“已完成访问”的节点。
///
/// 节点再次进入 `visiting` 表示沿依赖边回到了当前递归路径，形成循环；进入 `visited` 则是共享依赖，
/// 可以直接返回。此 helper 只校验图结构，不执行依赖。
fn visit_extension_node(
    node_id: &str,
    nodes: &BTreeMap<String, ExtensionGraphNode>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) -> Result<(), String> {
    if visited.contains(node_id) {
        return Ok(());
    }
    if !visiting.insert(node_id.to_owned()) {
        return Err("extension_dependency_cycle".to_owned());
    }
    if let Some(node) = nodes.get(node_id) {
        for dependency in &node.dependencies {
            if nodes.contains_key(&dependency.dependency_id) {
                visit_extension_node(&dependency.dependency_id, nodes, visiting, visited)?;
            }
        }
    }
    visiting.remove(node_id);
    visited.insert(node_id.to_owned());
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 依赖图解析出的单条边；不包含扩展内容或执行结果。
pub struct ResolvedExtensionDependency {
    /// 发起依赖的源节点 ID。
    pub source_node_id: String,
    /// 解析到的目标节点 ID。
    pub target_node_id: String,
    /// 目标节点的精确版本。
    pub version: String,
    /// 目标节点类别。
    pub kind: ExtensionGraphKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 输入依赖声明中的 scope 摘要；`None` 表示该边未另行声明 required scope。
    pub required_scope_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 某张依赖图在指定平台上的确定性解析结果。
pub struct ExtensionGraphResolution {
    /// 解析结果的 schema 标识。
    pub schema: String,
    /// 解析结果格式版本。
    pub version: SchemaVersion,
    /// 产生此结果的原始图摘要，用于绑定输入图。
    pub graph_digest: String,
    /// 解析针对的平台。
    pub platform: String,
    /// 排序后的依赖边列表。
    pub resolved_dependencies: Vec<ResolvedExtensionDependency>,
    /// 除本字段自身外解析结果内容的摘要。
    pub resolution_digest: String,
}

impl ExtensionGraphResolution {
    /// 内部自检 schema、摘要、边排序和每条边的字段形状。
    fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_GRAPH_RESOLUTION_SCHEMA
            || self.version != EXTENSION_DEPENDENCY_VERSION
            || !is_prefixed_digest(&self.graph_digest)
            || !valid_extension_identifier(&self.platform)
            || !is_prefixed_digest(&self.resolution_digest)
        {
            return Err("extension_graph_resolution_invalid".to_owned());
        }
        if self.resolved_dependencies.windows(2).any(|pair| {
            (
                &pair[0].source_node_id,
                &pair[0].target_node_id,
                &pair[0].version,
                pair[0].kind,
            ) > (
                &pair[1].source_node_id,
                &pair[1].target_node_id,
                &pair[1].version,
                pair[1].kind,
            )
        }) {
            return Err("extension_graph_resolution_order_invalid".to_owned());
        }
        for dependency in &self.resolved_dependencies {
            if !valid_extension_identifier(&dependency.source_node_id)
                || !valid_extension_identifier(&dependency.target_node_id)
                || !valid_extension_identifier(&dependency.version)
                || dependency
                    .required_scope_digest
                    .as_deref()
                    .is_some_and(|digest| !is_prefixed_digest(digest))
            {
                return Err("extension_graph_resolution_dependency_invalid".to_owned());
            }
        }
        if self.resolution_digest != self.digest() {
            return Err("extension_graph_resolution_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算解析结果摘要，不包含 `resolution_digest` 自身。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "graph_digest": self.graph_digest,
            "platform": self.platform,
            "resolved_dependencies": self.resolved_dependencies,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 将依赖图节点的父 scope 收窄后，绑定到目标、WorkPacket、角色和资源限制的记录。
/// 创建时要求子 `scope` 是父节点 `ScopeSet` 的子集；绑定本身不会启动扩展或独立授予权限。
pub struct ExtensionBinding {
    /// 绑定记录的 schema 标识。
    pub schema: String,
    /// 绑定记录的 schema 版本。
    pub version: SchemaVersion,
    /// 当前绑定的稳定唯一 ID。
    pub binding_id: String,
    /// 所属 `ExtensionBindingSnapshot` 的 ID。
    pub snapshot_id: String,
    /// 提供父 scope 的依赖图节点 ID。
    pub source_node_id: String,
    /// 绑定目标的调用方定义标识；这里只校验文本形状，不把它解释成文件路径。
    pub destination: String,
    /// 此绑定关联的 WorkPacket ID。
    pub packet_id: String,
    /// 此绑定适用的 Kiana 角色 ID。
    pub role_id: String,
    /// 此绑定允许接触的数据分类集合。
    pub data_classes: BTreeSet<String>,
    /// 网络策略声明，不会在本类型中配置 socket/HTTP 访问控制。
    pub network_policy: ExtensionNetworkPolicy,
    /// 可引用的 secret handle 名称集合，不包含 secret 明文。
    pub secret_handles: BTreeSet<String>,
    /// 此绑定对应的资源预算限制；`NotApplicable` 在校验中被拒绝。
    pub budget: ScopeLimit,
    /// 排他过期时间；now 等于此毫秒值时即已过期。
    pub expires_at_unix_ms: u64,
    /// 创建绑定时父 scope 的摘要，父范围变化后用来识别旧绑定。
    pub parent_scope_digest: String,
    /// 此绑定实际获得的范围，必须是父 scope 的子集。
    pub scope: ScopeSet,
    /// 除本字段自身外整个绑定内容的 SHA-256 摘要。
    pub binding_digest: String,
}

impl ExtensionBinding {
    #[allow(clippy::too_many_arguments)]
    /// 创建一个不扩大父 ScopeSet 的绑定，并计算父摘要和绑定摘要。
    ///
    /// 调用方提供目标、WorkPacket、角色、数据/网络/secret/预算限制，同时传入当前父 scope 与准备绑定的
    /// 更窄 scope。先校验两份 scope，再用 `is_subset_of()` 拒绝任何超出父级范围的子绑定，之后构造并自检。
    /// 失败返回字段/范围错误字符串；本方法不写事件、不创建外部资源，也不验证目标是否存在。
    /// 子集检查不能删掉：只分别验证两个 scope 的格式，并不能保证子绑定没有自行扩大权限。
    pub fn new(
        binding_id: impl Into<String>,
        snapshot_id: impl Into<String>,
        source_node_id: impl Into<String>,
        destination: impl Into<String>,
        packet_id: impl Into<String>,
        role_id: impl Into<String>,
        data_classes: BTreeSet<String>,
        network_policy: ExtensionNetworkPolicy,
        secret_handles: BTreeSet<String>,
        budget: ScopeLimit,
        expires_at_unix_ms: u64,
        parent_scope: &ScopeSet,
        scope: ScopeSet,
    ) -> Result<Self, String> {
        parent_scope.validate()?;
        scope.validate()?;
        if !scope.is_subset_of(parent_scope)? {
            return Err("extension_binding_scope_widened".to_owned());
        }
        let mut binding = Self {
            schema: EXTENSION_BINDING_SCHEMA.to_owned(),
            version: EXTENSION_DEPENDENCY_VERSION,
            binding_id: binding_id.into(),
            snapshot_id: snapshot_id.into(),
            source_node_id: source_node_id.into(),
            destination: destination.into(),
            packet_id: packet_id.into(),
            role_id: role_id.into(),
            data_classes,
            network_policy,
            secret_handles,
            budget,
            expires_at_unix_ms,
            parent_scope_digest: parent_scope.digest(),
            scope,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    /// 检查 schema、ID/字符串、预算、网络策略、ScopeSet 和绑定摘要的内部形状。
    ///
    /// 这里只能校验对象自身；父 scope 是否仍是当前版本、绑定是否到期，要通过 `is_valid_against()`
    /// 传入最新上下文检查。校验成功也不代表下游 handler 已经获准执行。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_BINDING_SCHEMA
            || self.version != EXTENSION_DEPENDENCY_VERSION
            || !valid_extension_identifier(&self.binding_id)
            || !valid_extension_identifier(&self.snapshot_id)
            || !valid_extension_identifier(&self.source_node_id)
            || !valid_extension_identifier(&self.packet_id)
            || !valid_extension_identifier(&self.role_id)
            || self.destination.trim().is_empty()
            || self.destination.len() > 512
            || self.destination.contains('\0')
            || self.data_classes.len() > 32
            || self.secret_handles.len() > 32
            || self.expires_at_unix_ms == 0
            || matches!(self.budget, ScopeLimit::NotApplicable)
            || !is_prefixed_digest(&self.parent_scope_digest)
            || !is_prefixed_digest(&self.binding_digest)
        {
            return Err("extension_binding_invalid".to_owned());
        }
        validate_string_set(&self.data_classes, "extension_binding_data_class")?;
        validate_string_set(&self.secret_handles, "extension_binding_secret_handle")?;
        validate_network_policy(&self.network_policy)?;
        self.scope.validate()?;
        if self.binding_digest != self.digest() {
            return Err("extension_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 将绑定与当前父 scope 和当前时间比较，拒绝过期或扩权的旧绑定。
    ///
    /// 先验证绑定和父范围，再比较父摘要、确认子范围仍被父范围包含，最后检查 now 是否早于过期时间。
    /// 父范围变化、子范围扩张或已到期分别返回对应错误码；调用方应停止使用不匹配的绑定。
    pub fn is_valid_against(
        &self,
        current_parent_scope: &ScopeSet,
        now_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        current_parent_scope.validate()?;
        if self.parent_scope_digest != current_parent_scope.digest() {
            return Err("extension_binding_parent_scope_changed".to_owned());
        }
        if !self.scope.is_subset_of(current_parent_scope)? {
            return Err("extension_binding_scope_widened".to_owned());
        }
        if now_ms >= self.expires_at_unix_ms {
            return Err("extension_binding_expired".to_owned());
        }
        Ok(())
    }

    /// 对绑定所有业务字段计算确定性摘要，不包含 `binding_digest` 自身。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "binding_id": self.binding_id,
            "snapshot_id": self.snapshot_id,
            "source_node_id": self.source_node_id,
            "destination": self.destination,
            "packet_id": self.packet_id,
            "role_id": self.role_id,
            "data_classes": self.data_classes,
            "network_policy": self.network_policy,
            "secret_handles": self.secret_handles,
            "budget": self.budget,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "parent_scope_digest": self.parent_scope_digest,
            "scope": self.scope,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 把多条扩展绑定与同一依赖图、解析结果和授权代次标签关联起来的快照。
///
/// 它是可序列化的领域值，不是 EventLog 记录或签名凭证。当前源码中直接构造/重验它的调用位于领域测试，
/// 尚未找到生产执行链调用。
pub struct ExtensionBindingSnapshot {
    /// 快照的 schema 标识。
    pub schema: String,
    /// 快照格式版本。
    pub version: SchemaVersion,
    /// 此快照的稳定 ID。
    pub snapshot_id: String,
    /// 创建快照时依赖图的摘要。
    pub graph_digest: String,
    /// 创建快照时解析结果的摘要。
    pub resolution_digest: String,
    /// 调用方提供的授权代次标签；`revalidate()` 没有当前 epoch 入参，不会比较其新鲜度。
    pub authority_epoch: u64,
    /// 0 到 256 条按 binding_id 严格升序排列的绑定条目。
    pub bindings: Vec<ExtensionBinding>,
    /// 除本字段自身外整个快照内容的摘要。
    pub snapshot_digest: String,
}

impl ExtensionBindingSnapshot {
    /// 基于依赖图、解析结果和绑定列表生成一个可校验的快照。
    ///
    /// 先验证 graph/resolution 并确认 resolution 引用同一 graph digest；再验证快照 ID 和非零 epoch，
    /// 按 binding_id 排序，检查每条绑定的 source node、snapshot ID、父 scope 摘要和父子范围关系，
    /// 拒绝重复 ID，最后计算摘要并自检。排序发生在摘要之前，不能在摘要后随意重排绑定列表。
    /// 方法只构造领域值，不安装依赖、写 EventLog 或创建运行权限。
    pub fn new(
        graph: &ExtensionDependencyGraph,
        resolution: &ExtensionGraphResolution,
        snapshot_id: impl Into<String>,
        authority_epoch: u64,
        mut bindings: Vec<ExtensionBinding>,
    ) -> Result<Self, String> {
        graph.validate()?;
        resolution.validate()?;
        if resolution.graph_digest != graph.graph_digest {
            return Err("extension_binding_graph_changed".to_owned());
        }
        let snapshot_id = snapshot_id.into();
        if !valid_extension_identifier(&snapshot_id) || authority_epoch == 0 {
            return Err("extension_binding_snapshot_header_invalid".to_owned());
        }
        bindings.sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
        for binding in &bindings {
            binding.validate()?;
            let Some(parent) = graph.nodes.get(&binding.source_node_id) else {
                return Err("extension_binding_parent_missing".to_owned());
            };
            if binding.snapshot_id != snapshot_id
                || binding.parent_scope_digest != parent.scope.digest()
                || !binding.scope.is_subset_of(&parent.scope)?
            {
                return Err("extension_binding_snapshot_mismatch".to_owned());
            }
        }
        if bindings
            .windows(2)
            .any(|pair| pair[0].binding_id == pair[1].binding_id)
        {
            return Err("extension_binding_duplicate".to_owned());
        }
        let mut snapshot = Self {
            schema: EXTENSION_BINDING_SNAPSHOT_SCHEMA.to_owned(),
            version: EXTENSION_DEPENDENCY_VERSION,
            snapshot_id,
            graph_digest: graph.graph_digest.clone(),
            resolution_digest: resolution.resolution_digest.clone(),
            authority_epoch,
            bindings,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// 校验 schema/摘要/上限以及每条绑定的快照身份和严格排序。
    ///
    /// 当前最多接受 256 条绑定，且不允许重复 binding_id；本方法没有当前 graph/epoch 入参，
    /// 因而只证明快照内部结构自洽。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_BINDING_SNAPSHOT_SCHEMA
            || self.version != EXTENSION_DEPENDENCY_VERSION
            || !valid_extension_identifier(&self.snapshot_id)
            || !is_prefixed_digest(&self.graph_digest)
            || !is_prefixed_digest(&self.resolution_digest)
            || self.authority_epoch == 0
            || self.bindings.len() > MAX_EXTENSION_BINDINGS
            || !is_prefixed_digest(&self.snapshot_digest)
        {
            return Err("extension_binding_snapshot_invalid".to_owned());
        }
        for binding in &self.bindings {
            binding.validate()?;
            if binding.snapshot_id != self.snapshot_id {
                return Err("extension_binding_snapshot_identity_mismatch".to_owned());
            }
        }
        if self
            .bindings
            .windows(2)
            .any(|pair| pair[0].binding_id >= pair[1].binding_id)
        {
            return Err("extension_binding_snapshot_order_invalid".to_owned());
        }
        if self.snapshot_digest != self.digest() {
            return Err("extension_binding_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 用当前依赖图和时间重新核验快照的图摘要、父节点、父 scope 与到期状态。
    ///
    /// 若图变化、节点消失、父范围变化、子范围扩大或绑定过期则返回错误。函数没有接收当前 authority epoch，
    /// 所以仅由 `validate()` 确认存储的 epoch 非零，并不判断授权代次变化是否使绑定失效；需要此判断时，
    /// 调用方还必须单独比较当前 epoch。
    pub fn revalidate(&self, graph: &ExtensionDependencyGraph, now_ms: u64) -> Result<(), String> {
        self.validate()?;
        graph.validate()?;
        if self.graph_digest != graph.graph_digest {
            return Err("extension_binding_graph_changed".to_owned());
        }
        for binding in &self.bindings {
            let Some(parent) = graph.nodes.get(&binding.source_node_id) else {
                return Err("extension_binding_parent_missing".to_owned());
            };
            binding.is_valid_against(&parent.scope, now_ms)?;
        }
        Ok(())
    }

    /// 对快照头、图/解析摘要、epoch 和已排序绑定列表计算摘要。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "snapshot_id": self.snapshot_id,
            "graph_digest": self.graph_digest,
            "resolution_digest": self.resolution_digest,
            "authority_epoch": self.authority_epoch,
            "bindings": self.bindings,
        }))
    }
}

/// 检查字符串集合中的每个值都非空、最长 256 字节且不含 NUL/控制字符。
///
/// 错误码以调用方传入的字段名前缀返回，便于区分 data class 与 secret handle 的失败来源。
fn validate_string_set(values: &BTreeSet<String>, field: &str) -> Result<(), String> {
    if values.iter().any(|value| {
        value.trim().is_empty()
            || value.len() > 256
            || value.contains('\0')
            || value.chars().any(char::is_control)
    }) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 对绑定网络策略中的 allowlist 做主机名文本形状检查。
///
/// 限制名单数量和字符串长度，并只接受 ASCII 小写字母/数字/点/连字符；不解析 DNS、端口或 wildcard，
/// 也不创建网络访问规则。真实连接限制仍必须由上层受审查的网络 adapter 执行。
fn validate_network_policy(policy: &ExtensionNetworkPolicy) -> Result<(), String> {
    if let ExtensionNetworkPolicy::Allowlist { hosts } = policy {
        if hosts.is_empty()
            || hosts.len() > 32
            || hosts.iter().any(|host| {
                host.is_empty()
                    || host.len() > 253
                    || host.starts_with('.')
                    || host.ends_with('.')
                    || host.contains("..")
                    || !host.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'.' | b'-')
                    })
            })
        {
            return Err("extension_binding_network_policy_invalid".to_owned());
        }
    }
    Ok(())
}

/// 检查带 `sha256:` 前缀的摘要字段格式；只检查形状，不使用原文重算或认证来源。
fn is_prefixed_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
