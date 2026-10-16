//! `kiana-domain` 中与持久化安全有关的值对象和纯校验函数。
//!
//! # 文件职责与调用位置
//!
//! 本文件不打开文件、不读写存储、不加密或解密内容；它定义上层适配器（adapter）应传递的安全元数据，
//! 并提供确定性的字段校验：载荷是否已不含明文凭证（secret-free）、加密密钥是否是作用域绑定的不透明引用（opaque reference）、
//! 文件路径身份是否符合约定，以及当前编译平台声明了哪些存储保护能力。
//!
//! ```text
//! Event / Artifact / ACP action
//!     → validate_secret_free() 或 validate_secret_free_text()
//!     → 发现脱敏会改变值时返回错误，调用方拒绝写出
//!
//! JsonlEventLog 提供平台能力声明 ─┐
//!                                 ├→ StoragePreflight / Port 能力矩阵
//! 路径 adapter 提供 StorageFileIdentity ┘
//!
//! 加密绑定（StorageEncryptionBinding）
//!     → 当前源码中只找到领域测试直接构造，尚未找到生产执行调用
//! ```
//!
//! EventStore 的 `validate_event_for_storage()` 使用 `validate_secret_free()`；这个检查通过比较
//! `redact_value(value)` 与原值来拒绝需要脱敏的数据，**不调用 `scan_secret_sentinels()`**。
//! 因此本文件校验和脱敏扫描器是不同层次的检查，不能把通用持久化校验说成覆盖了所有 secret 形状。
//!
//! 文件身份和加密绑定都是普通可序列化元数据。摘要（digest）可以发现字段变化，但没有密钥，不能证明
//! 文件来自可信路径、密文真实存在、密钥对调用者可用或底层文件操作已达到某种物理保证；这些责任仍由
//! Core/Port/文件系统适配器在实际打开、替换或解密时承担。

use crate::{json_digest, redact_text, redact_value, SchemaVersion, SecretRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path};

pub const STORAGE_SECURITY_SCHEMA: &str = "kiana.storage-security.v1";
/// 本模块的兼容版本。
pub const STORAGE_SECURITY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 文件身份记录的 schema 标识。
pub const STORAGE_FILE_IDENTITY_SCHEMA: &str = "kiana.storage-file-identity.v1";
/// 加密元数据绑定的 schema 标识。
pub const STORAGE_ENCRYPTION_BINDING_SCHEMA: &str = "kiana.storage-encryption-binding.v1";
/// 存储安全能力声明的 schema 标识。
pub const STORAGE_SECURITY_CAPABILITIES_SCHEMA: &str = "kiana.storage-security-capabilities.v1";
/// 本模块接受的普通文本字段最大长度：4 KiB，按 UTF-8 字节数计算。
pub const MAX_STORAGE_SECURITY_TEXT_BYTES: usize = 4_096;
/// 存储能力声明最多记录 16 条已知限制。
pub const MAX_STORAGE_SECURITY_LIMITATIONS: usize = 16;

/// 可选的加密算法标识；本枚举只描述密文使用的方案，不执行加密或验密。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageCipher {
    /// AES 使用 256 位密钥并以 Galois/Counter Mode（GCM）提供认证加密。
    Aes256Gcm,
    /// XChaCha20 与 Poly1305 组合的认证加密方案。
    XChaCha20Poly1305,
}

/// 描述一份密文、加密方案与不透明密钥引用（opaque `SecretRef`）之间关系的可验证元数据。
///
/// 它用于把密钥用途、所有者范围、命名空间和密文摘要绑定在一个对象内；不包含密钥字节，也不会解密载荷。
/// 当前源码中直接使用该类型的生产调用尚未找到，不能从类型存在推断加密存储已启用。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageEncryptionBinding {
    /// 绑定结构的 schema 标识。
    pub schema: String,
    /// schema 兼容版本。
    pub version: SchemaVersion,
    /// 密文声明使用的认证加密方案。
    pub cipher: StorageCipher,
    /// 密钥的不透明引用句柄（opaque reference）；字段中不保存密钥材料。
    pub key_ref: SecretRef,
    /// 密钥允许用于的具体目的，并且必须与 `key_ref.purpose` 一致。
    pub key_purpose: String,
    /// 所有者/授权范围摘要；用于防止把同一密钥绑定跨所有者复用。
    pub owner_scope_digest: String,
    /// 密文所在的相对命名空间路径。
    pub namespace: String,
    /// 实际密文字节的 SHA-256 摘要；本对象只校验摘要格式，不读取密文重算。
    pub ciphertext_digest: String,
    /// 除自身外以上绑定字段的 SHA-256 摘要。
    pub binding_digest: String,
}

impl StorageEncryptionBinding {
    /// 创建绑定值、计算自摘要并立即校验字段关系。
    ///
    /// 输入来自上层密文/密钥管理流程；本方法不生成密文、不访问 SecretStore，也不证明引用句柄可解析。
    /// namespace 必须是安全的相对路径，摘要必须带 `sha256:` 前缀且长度正确，校验失败返回错误码。
    pub fn new(
        cipher: StorageCipher,
        key_ref: SecretRef,
        key_purpose: impl Into<String>,
        owner_scope_digest: impl Into<String>,
        namespace: impl Into<String>,
        ciphertext_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: STORAGE_ENCRYPTION_BINDING_SCHEMA.to_owned(),
            version: STORAGE_SECURITY_SCHEMA_VERSION,
            cipher,
            key_ref,
            key_purpose: key_purpose.into(),
            owner_scope_digest: owner_scope_digest.into(),
            namespace: namespace.into(),
            ciphertext_digest: ciphertext_digest.into(),
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    /// 将绑定重新核对到“即将解密”的所有者（owner）、命名空间和密钥用途（key purpose）上下文。
    ///
    /// 先检查对象自身，再逐项比较调用方传入的当前上下文。即使 `SecretRef` 自身格式有效，
    /// owner、namespace 或用途不同仍会拒绝；通过本方法也不代表 SecretStore（密钥存储服务）已授权或完成解密。
    /// 不匹配时分别返回 scope、namespace 或 purpose 错误，不会自动改写绑定。
    pub fn validate_for_scope(
        &self,
        owner_scope_digest: &str,
        namespace: &str,
        key_purpose: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if self.owner_scope_digest != owner_scope_digest {
            return Err("storage_encryption_owner_scope_mismatch".to_owned());
        }
        if self.namespace != namespace {
            return Err("storage_encryption_namespace_mismatch".to_owned());
        }
        if self.key_purpose != key_purpose || self.key_ref.purpose != key_purpose {
            return Err("storage_encryption_key_purpose_mismatch".to_owned());
        }
        Ok(())
    }

    /// 检查 schema、SecretRef、敏感文本、相对命名空间、摘要格式和绑定自摘要。
    ///
    /// 此处不读取文件或密文、不验证密码学 tag、不访问密钥服务。任一字段无效时返回稳定错误字符串，
    /// 上层应阻止后续使用，而不是把元数据错误当成可重试的加密失败。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_ENCRYPTION_BINDING_SCHEMA
            || !STORAGE_SECURITY_SCHEMA_VERSION.is_compatible_with(&self.version)
        {
            return Err("storage_encryption_header_invalid".to_owned());
        }
        self.key_ref.validate()?;
        validate_secret_free_text(&self.key_ref.key, "storage_encryption_key")?;
        required(&self.key_purpose, "storage_encryption_key_purpose")?;
        required(&self.owner_scope_digest, "storage_encryption_owner_scope")?;
        validate_digest(&self.owner_scope_digest, "storage_encryption_owner_scope")?;
        required(&self.namespace, "storage_encryption_namespace")?;
        validate_relative_path(&self.namespace, "storage_encryption_namespace")?;
        validate_digest(&self.ciphertext_digest, "storage_encryption_ciphertext")?;
        validate_digest(&self.binding_digest, "storage_encryption_binding")?;
        if self.binding_digest != self.digest() {
            return Err("storage_encryption_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对算法、SecretRef、用途、owner 范围、命名空间和密文摘要计算 SHA-256 指纹。
    /// `binding_digest` 自身不参与计算，以避免摘要定义循环；此值不是数字签名。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "cipher": self.cipher,
            "key_ref": self.key_ref,
            "key_purpose": self.key_purpose,
            "owner_scope_digest": self.owner_scope_digest,
            "namespace": self.namespace,
            "ciphertext_digest": self.ciphertext_digest,
        }))
    }
}

/// 路径 adapter 在打开/替换文件前捕获的元数据身份快照。
///
/// 它把存储根摘要、相对路径、目标类型标志和可选权限位绑定起来，供 Domain/Core 做一致性与风险检查；
/// 自身不访问文件系统，也不固定文件描述符。实际路径是否仍指向同一 inode/文件，必须由 adapter 在
/// 文件操作边界重新确认。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageFileIdentity {
    /// 身份记录的 schema 标识。
    pub schema: String,
    /// schema 兼容版本。
    pub version: SchemaVersion,
    /// 所属存储根的 SHA-256 摘要，避免跨根复用相同相对路径。
    pub root_digest: String,
    /// 相对存储根的路径；本对象做词法形状校验，不解析符号链接（symlink）目标。
    pub relative_path: String,
    /// 捕获时目标是否存在。
    pub exists: bool,
    /// 捕获时目标是否为普通文件。
    pub regular_file: bool,
    /// 捕获时目标是否为符号链接（symlink）。
    pub symlink: bool,
    /// 捕获时目标是否为硬链接（hard link）。
    pub hardlink: bool,
    #[serde(default)]
    /// 若平台能读取，保存 Unix mode 权限位；`None` 表示没有提供该项观测。
    pub mode: Option<u32>,
    /// 除本字段自身外快照字段的 SHA-256 摘要。
    pub identity_digest: String,
}

impl StorageFileIdentity {
    #[allow(clippy::too_many_arguments)]
    /// 从 adapter 已观察到的根/路径/文件属性构造身份快照并计算自摘要。
    ///
    /// 构造成功只说明传入标志彼此满足领域规则，不会替 adapter 检查这些标志是否来自真实文件系统。
    pub fn new(
        root_digest: impl Into<String>,
        relative_path: impl Into<String>,
        exists: bool,
        regular_file: bool,
        symlink: bool,
        hardlink: bool,
        mode: Option<u32>,
    ) -> Result<Self, String> {
        let mut identity = Self {
            schema: STORAGE_FILE_IDENTITY_SCHEMA.to_owned(),
            version: STORAGE_SECURITY_SCHEMA_VERSION,
            root_digest: root_digest.into(),
            relative_path: relative_path.into(),
            exists,
            regular_file,
            symlink,
            hardlink,
            mode,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    /// 检查 schema、摘要、相对路径、存在性与文件类型标志之间的不变量。
    ///
    /// 存在的目标必须是普通文件且不能是符号链接（symlink）/硬链接（hard link）；不存在时这些类型标志
    /// 必须全部为 false。若报告 mode，则八进制 `0o077`（组用户与其他用户的权限位）必须为 0。
    /// 任何失败返回结构化错误字符串。这只是快照自校验，不会再次 stat 文件或消除检查后使用
    /// （Time-of-Check to Time-of-Use，TOCTOU）竞态，也不证明 adapter 真正采用了安全打开方式。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_FILE_IDENTITY_SCHEMA
            || !STORAGE_SECURITY_SCHEMA_VERSION.is_compatible_with(&self.version)
        {
            return Err("storage_file_identity_header_invalid".to_owned());
        }
        validate_digest(&self.root_digest, "storage_file_identity_root")?;
        validate_relative_path(&self.relative_path, "storage_file_identity_path")?;
        if self.exists && (!self.regular_file || self.symlink || self.hardlink) {
            return Err("storage_file_identity_unsafe_target".to_owned());
        }
        if !self.exists && (self.regular_file || self.symlink || self.hardlink) {
            return Err("storage_file_identity_missing_flags".to_owned());
        }
        if self.mode.is_some_and(|mode| mode & 0o077 != 0) {
            return Err("storage_file_permissions_too_broad".to_owned());
        }
        validate_digest(&self.identity_digest, "storage_file_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("storage_file_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对根、相对路径、存在性、文件类型和可选 mode 计算快照摘要，不包括 `identity_digest` 自身。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "root_digest": self.root_digest,
            "relative_path": self.relative_path,
            "exists": self.exists,
            "regular_file": self.regular_file,
            "symlink": self.symlink,
            "hardlink": self.hardlink,
            "mode": self.mode,
        }))
    }
}

/// 平台/adapter 声明可提供的存储安全能力和已知限制。
///
/// `current()` 依据 Rust 编译目标的 `cfg!` 分支填入声明值，不会探测当前挂载点或执行文件操作来验证能力。
/// 消费方应将此记录作为平台声明输入预检/能力矩阵；它不是“已通过物理验证”的证明。未支持项要放入
/// `limitations`，避免调用方把缺失能力误当作等价保证。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageSecurityCapabilities {
    /// 能力声明的 schema 标识。
    pub schema: String,
    /// schema 兼容版本。
    pub version: SchemaVersion,
    /// 目标平台标签；由编译目标分支决定。
    pub platform: String,
    /// adapter 声称能防止沿符号链接替换/跟随的能力。
    pub symlink_guard: bool,
    /// adapter 声称能识别或拒绝硬链接目标的能力。
    pub hardlink_guard: bool,
    /// adapter 声称能施加/核对文件权限的能力。
    pub permission_guard: bool,
    /// adapter 声称能原子替换文件的能力。
    pub atomic_replace: bool,
    /// 密钥引用是否保持不透明（opaque）、不把密钥材料直接放进绑定元数据。
    pub opaque_encryption_refs: bool,
    #[serde(default)]
    /// 对当前声明中缺失保护能力的说明；向后兼容时缺省为空列表。
    pub limitations: Vec<String>,
    /// 除本字段自身外能力声明的 SHA-256 摘要。
    pub capability_digest: String,
}

impl StorageSecurityCapabilities {
    /// 按编译目标生成一份当前平台能力声明，并为未声明支持的保护项附上 limitation。
    ///
    /// Linux 和其他 Unix target 当前都填入四项文件保护能力为 `true`；非 Unix target 填 `false`，
    /// 并分别记录 symlink、hardlink、权限和原子替换限制。这个分支是编译时平台分类，不会询问具体
    /// EventStore adapter 是否实现了每项能力；调用方仍需结合 Port 能力矩阵和实际 adapter 证据判断。
    /// 返回对象带自摘要，但没有文件系统或密钥服务副作用。
    pub fn current() -> Self {
        let (platform, symlink_guard, hardlink_guard, permission_guard, atomic_replace) =
            if cfg!(target_os = "linux") {
                ("linux".to_owned(), true, true, true, true)
            } else if cfg!(unix) {
                ("unix".to_owned(), true, true, true, true)
            } else {
                ("non_unix".to_owned(), false, false, false, false)
            };
        let mut capabilities = Self {
            schema: STORAGE_SECURITY_CAPABILITIES_SCHEMA.to_owned(),
            version: STORAGE_SECURITY_SCHEMA_VERSION,
            platform,
            symlink_guard,
            hardlink_guard,
            permission_guard,
            atomic_replace,
            opaque_encryption_refs: true,
            limitations: Vec::new(),
            capability_digest: String::new(),
        };
        if !capabilities.symlink_guard {
            capabilities
                .limitations
                .push("symlink_guard_requires_adapter_check".to_owned());
        }
        if !capabilities.hardlink_guard {
            capabilities
                .limitations
                .push("hardlink_identity_requires_adapter_check".to_owned());
        }
        if !capabilities.permission_guard {
            capabilities
                .limitations
                .push("permission_mode_unavailable".to_owned());
        }
        if !capabilities.atomic_replace {
            capabilities
                .limitations
                .push("atomic_replace_semantics_unproven".to_owned());
        }
        capabilities.capability_digest = capabilities.digest();
        capabilities
    }

    /// 校验声明版本、限制条数/文本、缺失能力说明和自摘要。
    ///
    /// 最多允许 16 条 limitation；若 symlink/hardlink/权限/原子替换任一项为 false，至少要提供一条说明。
    /// 校验不会证明声明为 true 的能力实际可用，也不会核对 limitation 是否逐项对应 false 的字段；
    /// 它也不验证 platform 是否非空，或要求 `opaque_encryption_refs` 为 true。
    /// 失败返回稳定错误字符串，供预检流程拒绝结构不完整或自摘要不一致的声明。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_SECURITY_CAPABILITIES_SCHEMA
            || !STORAGE_SECURITY_SCHEMA_VERSION.is_compatible_with(&self.version)
        {
            return Err("storage_security_capabilities_header_invalid".to_owned());
        }
        if self.limitations.len() > MAX_STORAGE_SECURITY_LIMITATIONS {
            return Err("storage_security_capabilities_limitations_exceeded".to_owned());
        }
        for limitation in &self.limitations {
            required(limitation, "storage_security_limitation")?;
        }
        if (!self.symlink_guard
            || !self.hardlink_guard
            || !self.permission_guard
            || !self.atomic_replace)
            && self.limitations.is_empty()
        {
            return Err("storage_security_capability_limitation_missing".to_owned());
        }
        validate_digest(
            &self.capability_digest,
            "storage_security_capability_digest",
        )?;
        if self.capability_digest != self.digest() {
            return Err("storage_security_capability_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对平台标签、四项文件保护能力、opaque 引用能力和 limitation 列表计算摘要。
    /// 摘要不包含自身，也不等同于平台签名或物理安全证明。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "platform": self.platform,
            "symlink_guard": self.symlink_guard,
            "hardlink_guard": self.hardlink_guard,
            "permission_guard": self.permission_guard,
            "atomic_replace": self.atomic_replace,
            "opaque_encryption_refs": self.opaque_encryption_refs,
            "limitations": self.limitations,
        }))
    }
}

/// 拒绝含有会被 `redact_value()` 改写内容的结构化 JSON。
///
/// 【真实调用者】EventStore 的事件载荷、Core operator evidence / artifact，以及其他显式验证点会调用它。
/// 输入是待保存的 JSON；成功返回 `Ok(())` 后调用方才继续自己的持久化流程。
///
/// 【为什么不在这里自动改写】本函数是 validator，不是转换器：如果脱敏前后不同便返回
/// `storage_secret_sentinel_detected`，原值留给调用方决定拒绝/另行处理，不会返回一个悄悄变化的对象。
/// 【扫描范围】它只做 `redact_value(value) == value` 比较，不调用 `scan_secret_value()` 或
/// `scan_secret_sentinels()`。因此像 `sk-...` 这类未被 `redact_text()` 改写的形状不会被本函数额外识别；
/// 若边界需要哨兵扫描，必须由调用方显式添加 scanner 校验。这里不写日志、不重试，也不修改存储状态。
pub fn validate_secret_free(value: &Value) -> Result<(), String> {
    if redact_value(value) != *value {
        return Err("storage_secret_sentinel_detected".to_owned());
    }
    Ok(())
}

/// 拒绝过长文本或会被 `redact_text()` 改写的文本字段。
///
/// `field` 用于构造调用方可区分的错误码；输入按 UTF-8 字节计长，最大 4 KiB。超限返回
/// `{field}_too_long`，脱敏比较不相等返回 `{field}_secret_sentinel`。这仍是差异比较，不运行独立哨兵扫描；
/// 不通过时上层应拒绝使用该字段，本函数不截断或返回替代文本。
pub fn validate_secret_free_text(value: &str, field: &str) -> Result<(), String> {
    if value.len() > MAX_STORAGE_SECURITY_TEXT_BYTES {
        return Err(format!("{field}_too_long"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_secret_sentinel"));
    }
    Ok(())
}

/// 检查必填文本非空、长度受限且不会被脱敏函数改写。
///
/// 空白字符串和超长字符串当前都返回 `{field}_required`；其他情况继续交给
/// `validate_secret_free_text()`，所以敏感 marker 会返回 `{field}_secret_sentinel`。
fn required(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_STORAGE_SECURITY_TEXT_BYTES {
        return Err(format!("{field}_required"));
    }
    validate_secret_free_text(value, field)
}

/// 对路径做相对路径的词法检查，拒绝绝对路径、父目录跳转、根目录和平台前缀。
///
/// 先调用 `required()` 做空值/长度/脱敏检查，再使用当前平台的 `std::path::Path` 解析 component。
/// 它不 canonicalize（规范化绝对路径）、不访问磁盘，也不解析符号链接目标；真实打开文件时仍需 adapter
/// 做路径包含性（containment）和文件身份核对，不能仅靠这项字符串校验防止 TOCTOU 或 symlink 攻击。
fn validate_relative_path(value: &str, field: &str) -> Result<(), String> {
    required(value, field)?;
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 检查摘要是否是 `sha256:` 加 64 个十六进制字符的文本形状。
///
/// 此处允许大小写混合的十六进制字符；函数不根据原数据重算摘要，也不认证签发者。
fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
