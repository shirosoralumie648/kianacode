//! 本文件属于 `kiana-domain`，提供可复用的脱敏（redaction，替换已识别敏感值）与
//! 凭证哨兵扫描（secret sentinel scan，按已知格式拒绝疑似凭证）基础函数。
//!
//! # 在系统中的位置
//!
//! ```text
//! 业务对象 / 上游服务（Provider）错误 / 日志等原始值
//!     ├─ 结构化数据 → redact_value / encode_bounded_value
//!     ├─ 完整文本   → redact_text / encode_bounded_text
//!     └─ 流式文本   → StreamingRedactor::push → finish
//!                         ↓
//!              调用方显式选择是否再调用扫描器
//!                         ↓
//!       事件、运行收据（Receipt）、用户界面（UI）、进程输出或其他下游边界
//! ```
//!
//! 这里实现的是可被上层调用的领域工具，不是自动套在所有输出上的中间件。
//! `encode_bounded_*` 会组合结构校验、脱敏、哨兵检查和大小限制；其他调用点可能只调用其中一部分。
//! 例如，`kiana-eventlog/src/event_store_core.rs` 的通用事件存储（EventStore）校验先调用
//! `storage_security::validate_secret_free()`（比较结构化脱敏前后的值），再额外调用本文件的
//! `scan_secret_value_with_limit(SecretScanChannel::Event, ...)` 补上脱敏不会重写的形状
//! （`sk-` token 前缀、`proxy-authorization:` 文本标记、NUL/超限输入）。
//! 但仍不能把本文件或 `SecretScanChannel` 的存在理解为所有事件日志（EventLog）写入都自动经过扫描：
//! 覆盖范围取决于每个写入边界是否显式调用了这些 API。
//!
//! 主要调用方包括 Domain 的审计/模型事件/遥测验证、Core 的审计与 Trace 导出、Runner 的输出处理、
//! Daemon 的子进程输出，以及 Entrypoints 的 Web/流式展示。它们各自在自己的数据边界调用这些工具；
//! 修改脱敏行为时，应沿具体调用点确认其输入格式和失败处理，而不能只根据目标通道名称推断覆盖范围。
//!
//! # 脱敏与扫描的差别
//!
//! 脱敏尝试替换它认识的字段名和文本标记；扫描器只报告命中的固定标记、凭证字段、URL 用户信息或
//! 类 JWT 形状，不会替调用方改写输入。两者都是有限规则：无标记的任意随机字符串不一定能识别，
//! 所以 `Ok(())` 只表示本次规则没有命中，不能证明文本绝对不含凭证（secret）。
//!
//! 通常在允许写出数据的边界先做脱敏，再对脱敏结果扫描；遇到不可安全投影的原始 Provider 错误时，
//! 使用 `project_redacted_error()` 返回不包含原始错误文本的摘要对象。扫描规则是否生效，取决于调用方
//! 是否确实调用了相应 API。
use crate::{check_schema_compatibility, json_digest, DataClass, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// 替换后的占位文本。
///
/// 【为什么是固定的常量，而不是每次现场生成】
/// 替换标记必须**稳定**：如果它每次都不一样（比如带时间戳），
/// 那么「同一段内容在两次记录里是否相同」这个问题就无法回答，
/// 而 diff、比对、幂等判断都依赖这一点。
///
/// 同时它必须**显眼到不会误认成真数据**——`[REDACTED]` 在日志里一眼可辨，
/// 不会和某个真实的字段值混淆。
const REDACTED: &str = "[REDACTED]";

pub const REDACTION_PROFILE_SCHEMA: &str = "kiana.redaction-profile.v1";
/// 当前脱敏配置结构的兼容版本。
pub const REDACTION_PROFILE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// profile 编码器允许的最大嵌套深度：32 层。
///
/// `encode_bounded_value()` 在脱敏前后都会检查深度；超过配置上限时返回错误，
/// 不会把未继续脱敏的深层子树原样作为成功结果交出去。单独调用 `redact_value()` 不使用此上限。
/// 上限用于限制递归遍历的工作量，也使有界编码器能明确拒绝超出约定形状的输入。
///
pub const MAX_REDACTION_DEPTH: usize = 32;
/// 单个 profile 可接受的最大 JSON 编码字节数；字节数按 UTF-8 序列化结果计算。
pub const MAX_REDACTION_VALUE_BYTES: usize = 64 * 1024;
/// 脱敏结果序列化值/文本结果的额外硬上限（256 KiB）。当前 profile 的 `max_bytes` 最多为 64 KiB，
/// 所以正常有效 profile 通常会先触及更严格的配置上限；此常量是额外的绝对边界。
pub const MAX_REDACTION_PROFILE_BYTES: usize = 256 * 1024;

/// 哨兵扫描结果使用的 schema 标识；该扫描由调用方显式触发，不会自动覆盖所有输出通道。
pub const SECRET_SENTINEL_SCAN_SCHEMA: &str = "kiana.secret-sentinel-scan.v1";
/// 单次文本哨兵扫描最多检查 64 KiB；超限输入会以结构化 finding 拒绝。
pub const MAX_SECRET_SENTINEL_SCAN_BYTES: usize = 64 * 1024;
/// `scan_secret_channels()` 接受的回显哨兵字符串数量上限；它不限制传入的 channel 条数。
pub const MAX_SECRET_SENTINELS: usize = 64;
/// Provider / transport 错误安全投影对象所使用的 schema 标识。
pub const REDACTED_ERROR_PROJECTION_SCHEMA: &str = "kiana.redacted-error-projection.v1";

/// 调用方正在扫描的目标类别，用于让错误 finding 指明扫描发生在哪个边界。
///
/// 这是跨 crate 共用的名称集合，不会自行连接到 Prompt、EventLog、进程或缓存，也不会强制这些出口调用扫描器。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretScanChannel {
    /// 即将交给模型或其他生成器的提示内容。
    Prompt,
    /// 面向用户展示或持久化的对话文本。
    Transcript,
    /// EventLog 中的事件数据或事件相关文本。
    Event,
    /// 从事件或运行结果投影出的收据数据。
    Receipt,
    /// 子进程标准输出。
    Stdout,
    /// 子进程标准错误。
    Stderr,
    /// 即将传给子进程的参数。
    Argv,
    /// 即将传给子进程的环境变量。
    Env,
    /// 缓存键或缓存值。
    Cache,
}

impl SecretScanChannel {
    /// 返回该通道稳定的序列化/错误显示名称；不是文件名或运行时路由。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Transcript => "transcript",
            Self::Event => "event",
            Self::Receipt => "receipt",
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Argv => "argv",
            Self::Env => "env",
            Self::Cache => "cache",
        }
    }
}

/// 哨兵扫描命中的规则类别；finding（扫描结果）只记录类别，不保存待检查的原始值。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretSentinelKind {
    /// 以 token 字段或文本标记形式出现的凭证。
    Token,
    /// 应用程序接口密钥（API key）或密钥样式值。
    ApiKey,
    /// Authorization 等 HTTP 认证头。
    Header,
    /// 符合本模块启发式形状的 JSON Web Token（JWT）。
    Jwt,
    /// URL authority 部分包含用户名/密码信息。
    UrlUserinfo,
    /// 结构化对象含有非空敏感字段。
    CredentialLease,
    /// 输入超限、包含 NUL 或无法满足扫描前提；不表示已识别某个具体 Provider 错误。
    ProviderRawError,
    /// 输入中出现调用方提供的回显哨兵。
    Echo,
}

impl SecretSentinelKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::ApiKey => "api_key",
            Self::Header => "header",
            Self::Jwt => "jwt",
            Self::UrlUserinfo => "url_userinfo",
            Self::CredentialLease => "credential_lease",
            Self::ProviderRawError => "provider_raw_error",
            Self::Echo => "echo",
        }
    }
}

/// 哨兵扫描拒绝结果，不直接携带命中的原文。
///
/// 通常 `marker_digest` 对规则标识（例如 `jwt_shape`、`json_secret_key` 或 `sk-`）求摘要；
/// `Echo` finding 则对调用方提供的回显哨兵求摘要。摘要用于聚合诊断，但低熵或已知的哨兵仍可能被猜测，
/// 因而不能把它当成对任意输入都无泄漏风险的匿名化证明。Display 只暴露通道和类别。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretSentinelFinding {
    /// finding 的 schema 标识。
    pub schema: String,
    /// 调用方声明的扫描目标类别。
    pub channel: SecretScanChannel,
    /// 首个命中的规则类别。
    pub kind: SecretSentinelKind,
    /// 规则标识摘要，不含原始输入或 secret 值。
    pub marker_digest: String,
}

impl SecretSentinelFinding {
    /// 构造不包含原始文本字段的 finding，并对规则标识或 Echo 哨兵计算摘要。
    fn new(channel: SecretScanChannel, kind: SecretSentinelKind, marker: &str) -> Self {
        Self {
            schema: SECRET_SENTINEL_SCAN_SCHEMA.to_owned(),
            channel,
            kind,
            marker_digest: json_digest(&serde_json::json!({
                "channel": channel.as_str(),
                "kind": kind.as_str(),
                "marker": marker,
            })),
        }
    }
}

impl fmt::Display for SecretSentinelFinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "secret_sentinel_detected:{}:{}",
            self.channel.as_str(),
            self.kind.as_str()
        )
    }
}

impl std::error::Error for SecretSentinelFinding {}

/// 上游服务（Provider）/传输层（transport）错误的安全投影；不保存原始错误文本，只留下分类码和脱敏后内容的摘要。
///
/// 调用方可把这个小对象写入 Event/Receipt 或返回给 UI。摘要仍会随脱敏后的错误文本变化，
/// 但对象本身不包含该文本；`validate()` 只检查格式，不能验证错误来源或摘要真实性。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactedErrorProjection {
    /// 投影结构的 schema 标识。
    pub schema: String,
    /// 原始错误所属的扫描通道。
    pub channel: SecretScanChannel,
    /// 稳定、有限字符集的错误分类码，不是 Provider 原始消息。
    pub code: String,
    /// 对通道及脱敏文本组合求出的 SHA-256 摘要。
    pub error_digest: String,
    /// 原始错误是否命中哨兵扫描规则；不等同于 `redact_text()` 是否实际替换了内容。
    pub redacted: bool,
}

impl RedactedErrorProjection {
    /// 检查 schema、错误码字符集和 SHA-256 摘要的文本形状。
    ///
    /// 出错时返回 `redacted_error_projection_invalid`；这里不读取 Provider、不写日志，也不重算摘要。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REDACTED_ERROR_PROJECTION_SCHEMA
            || self.code.trim().is_empty()
            || self.code.len() > 128
            || !self
                .code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            || !self.error_digest.starts_with("sha256:")
            || self.error_digest.len() != 71
        {
            return Err("redacted_error_projection_invalid".to_owned());
        }
        Ok(())
    }
}

/// 流式脱敏保留已输出文本末尾的重叠区上限：64 字节。
///
/// 重叠区用于识别被网络/模型分片拆开的敏感标记；常量必须大于 `SENSITIVE_MARKERS` 中最长标记，
/// 否则标记前半段可能在下一分片到来前被丢弃。这里的字节上限不代表完整 secret 值会缓存 64 字节。
pub const STREAM_REDACTION_BUFFER_LIMIT: usize = 64;

/// 选择有界脱敏配置默认字节限制的信号类别。
///
/// `encode_bounded_*()` 实际接收完整 profile；这个枚举只帮助创建内置默认 profile，
/// 不会自动拦截或路由日志、指标、Trace、审计记录或导出数据。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionSignal {
    /// 普通日志文本，默认最多 16 KiB。
    Log,
    /// 指标标签/属性，默认最多 2 KiB。
    Metric,
    /// Trace 属性，默认最多 8 KiB。
    Trace,
    /// 审计信号，默认最多 16 KiB。
    Audit,
    /// 导出信号，默认最多 64 KiB。
    Export,
}

impl RedactionSignal {
    /// 返回此类别内置 profile 的默认 UTF-8 字节上限。
    fn default_limit(self) -> usize {
        match self {
            Self::Log => 16 * 1024,
            Self::Metric => 2 * 1024,
            Self::Trace => 8 * 1024,
            Self::Audit => 16 * 1024,
            Self::Export => 64 * 1024,
        }
    }

    /// 选择治理某个发布通道的 profile 类别。
    ///
    /// Prompt/Transcript/Stdout/Stderr/Argv/Env/Cache 都是文本发布通道，使用日志档位；
    /// Event/Receipt 是记录通道，使用审计档位。该映射只决定 `encode_bounded_*_for()` 与
    /// `StreamingRedactionSink` 采用的字节上限和 profile 摘要，本身不会自动拦截、路由或拒绝任一通道。
    pub const fn for_scan_channel(channel: SecretScanChannel) -> Self {
        match channel {
            SecretScanChannel::Prompt
            | SecretScanChannel::Transcript
            | SecretScanChannel::Stdout
            | SecretScanChannel::Stderr
            | SecretScanChannel::Argv
            | SecretScanChannel::Env
            | SecretScanChannel::Cache => Self::Log,
            SecretScanChannel::Event | SecretScanChannel::Receipt => Self::Audit,
        }
    }
}

/// 一次有界脱敏调用使用的版本化配置。
///
/// 创建配置时将信号类型、数据分类、最大编码字节数和最大嵌套深度纳入摘要；消费者可检查
/// `profile_digest` 是否与这些字段一致。摘要没有密钥，不是签名或授权凭证；`data_class` 在本文件中
/// 被保存并参与摘要，但不会驱动额外的分类策略分支。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionProfile {
    /// 配置结构的 schema 标识。
    pub schema: String,
    /// schema 兼容版本。
    pub version: SchemaVersion,
    /// 该配置描述的信号类别。
    pub signal: RedactionSignal,
    /// 调用方提供的数据分类标签；本编码器会绑定它，但不会据此改变脱敏规则。
    pub data_class: DataClass,
    /// 输出 JSON 编码或文本 UTF-8 字节数的最大值。
    pub max_bytes: usize,
    /// 允许的最大数组/对象嵌套层数。
    pub max_depth: usize,
    /// 除本字段自身外配置内容的 SHA-256 摘要。
    pub profile_digest: String,
}

impl RedactionProfile {
    /// 按信号类别创建默认配置，数据分类设为 `Internal`，深度上限设为 32。
    ///
    /// 仅使用枚举内置的固定值，因此内部构造失败会触发 `expect`；新增或修改默认值时应保持其满足
    /// `new()` 的边界校验。它只返回配置，不会让对应信号自动经过编码器。
    pub fn for_signal(signal: RedactionSignal) -> Self {
        Self::new(
            signal,
            DataClass::Internal,
            signal.default_limit(),
            MAX_REDACTION_DEPTH,
        )
        .expect("built-in redaction profile is valid")
    }

    /// 按发布通道创建默认配置，等价于对该通道 [`RedactionSignal::for_scan_channel`] 档位的
    /// [`RedactionProfile::for_signal`]。
    ///
    /// 它让同一份边界（例如 stdout 预览、transcript 事件）在读取处和写入处都能算出一致的
    /// `profile_digest`，便于下游在记录里定位实际使用的配置。
    pub fn for_channel(channel: SecretScanChannel) -> Self {
        Self::for_signal(RedactionSignal::for_scan_channel(channel))
    }

    /// 创建并立即校验一份配置，再计算包含策略字段的自摘要。
    ///
    /// 字节上限必须在 1 到 64 KiB 之间，深度必须在 1 到 32 之间；越界分别返回稳定错误码。
    /// `data_class` 作为策略元数据保存，当前文件不根据其值选择不同规则。无外部 I/O 或状态副作用。
    pub fn new(
        signal: RedactionSignal,
        data_class: DataClass,
        max_bytes: usize,
        max_depth: usize,
    ) -> Result<Self, String> {
        if max_bytes == 0 || max_bytes > MAX_REDACTION_VALUE_BYTES {
            return Err("redaction_profile_bytes_invalid".to_owned());
        }
        if max_depth == 0 || max_depth > MAX_REDACTION_DEPTH {
            return Err("redaction_profile_depth_invalid".to_owned());
        }
        let mut profile = Self {
            schema: REDACTION_PROFILE_SCHEMA.to_owned(),
            version: REDACTION_PROFILE_SCHEMA_VERSION,
            signal,
            data_class,
            max_bytes,
            max_depth,
            profile_digest: String::new(),
        };
        profile.profile_digest = profile.digest();
        profile.validate()?;
        Ok(profile)
    }

    /// 检查 schema 兼容性、固定 schema 名、字节/深度边界和自摘要。
    ///
    /// 输入的配置可能来自序列化数据，因此每次使用前重新校验；字段被改动却保留旧摘要时返回
    /// `redaction_profile_digest_mismatch`。格式正确的摘要不证明配置来自可信签发者。
    pub fn validate(&self) -> Result<(), String> {
        check_schema_compatibility(&self.schema, &self.version)
            .map_err(|_| "redaction_profile_schema_incompatible".to_owned())?;
        if self.schema != REDACTION_PROFILE_SCHEMA {
            return Err("redaction_profile_schema_mismatch".to_owned());
        }
        if self.max_bytes == 0 || self.max_bytes > MAX_REDACTION_VALUE_BYTES {
            return Err("redaction_profile_bytes_invalid".to_owned());
        }
        if self.max_depth == 0 || self.max_depth > MAX_REDACTION_DEPTH {
            return Err("redaction_profile_depth_invalid".to_owned());
        }
        let Some(hex) = self.profile_digest.strip_prefix("sha256:") else {
            return Err("redaction_profile_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("redaction_profile_digest_invalid".to_owned());
        }
        if self.profile_digest != self.digest() {
            return Err("redaction_profile_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算配置摘要时先把 `profile_digest` 自身置空，避免摘要包含自身造成循环定义。
    /// 序列化失败时当前实现以 JSON `null` 继续计算；正常的这些字段均可序列化。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert("profile_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

impl Default for RedactionProfile {
    /// 默认配置等同于普通日志信号的内置 profile。
    fn default() -> Self {
        Self::for_signal(RedactionSignal::Log)
    }
}

#[derive(Clone, Debug, PartialEq)]
/// 成功完成结构化脱敏后的 JSON 值，以及本次编码大小和所用 profile 摘要。
pub struct BoundedRedactedValue {
    /// 脱敏后的 JSON；只有完整通过编码器的所有检查才会返回。
    pub value: Value,
    /// 脱敏结果序列化成 JSON 后的 UTF-8 字节数。
    pub encoded_bytes: usize,
    /// 本次使用配置的摘要，方便调用方记录结果对应的边界。
    pub profile_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// 成功完成文本脱敏后的内容，以及 UTF-8 字节数和所用 profile 摘要。
pub struct BoundedRedactedText {
    /// 通过脱敏、扫描与大小检查的文本。
    pub text: String,
    /// 返回文本的 UTF-8 字节数，而非字符数。
    pub encoded_bytes: usize,
    /// 本次使用配置的摘要。
    pub profile_digest: String,
}

/// 对一个 JSON（JavaScript Object Notation，常用的数据交换文本格式）值执行脱敏配置（profile）约束下的
/// 脱敏、哨兵检查和大小检查。
///
/// 【调用者与数据流】Domain 的审计事件、Core 的审计/Trace 导出等调用方，将准备输出或持久化的
/// `serde_json::Value` 传入；成功结果携带脱敏后的值、序列化字节数和 profile 摘要，再由调用方决定写到哪里。
/// 本函数不会自行写 EventLog、日志或 Receipt，也不会触发重试、账号禁用或网络请求。
///
/// 【核心流程】先验证配置 → 检查原值的递归深度与 NUL → 按字段名递归脱敏 → 再检查脱敏结果形状
/// → 检查残留敏感字段/文本标记 → 以 `Event` 通道递归运行哨兵扫描 → 序列化为 JSON → 核对字节上限。
/// 其中字段名规则和固定文本哨兵互补，因此不能只保留其中一层检查。
///
/// 【失败情况】任一校验失败都会返回字符串错误码，且不会把原始 `value` 放进错误对象或成功结果；
/// 上层负责把错误转成自己的领域错误或拒绝写出。超深、NUL、疑似凭证、编码失败或超限均是拒绝，
/// 不是自动修复后继续执行的提示。成功只说明当前这组有限规则通过，不是对任意随机 secret 的证明。
pub fn encode_bounded_value(
    profile: &RedactionProfile,
    value: &Value,
) -> Result<BoundedRedactedValue, String> {
    encode_bounded_value_for(profile, SecretScanChannel::Event, value)
}

/// 与 [`encode_bounded_value`] 相同，但用调用方给出的通道标签做残余哨兵扫描。
///
/// 用于结构化的信号值进入具体通道（如 Receipt、Prompt 的 JSON 载荷）之前的统一检查：通道标签
/// 只影响 finding 的来源标注，不改变脱敏规则；失败仍然是不返回原文的错误码。
pub fn encode_bounded_value_for(
    profile: &RedactionProfile,
    channel: SecretScanChannel,
    value: &Value,
) -> Result<BoundedRedactedValue, String> {
    profile.validate()?;
    validate_value_shape(value, 0, profile.max_depth)?;
    let redacted = redact_value(value);
    validate_value_shape(&redacted, 0, profile.max_depth)?;
    if contains_unredacted_secret(&redacted) {
        return Err("redaction_secret_sentinel_detected".to_owned());
    }
    scan_secret_value(channel, &redacted)
        .map_err(|_| "redaction_secret_sentinel_detected".to_owned())?;
    let encoded =
        serde_json::to_vec(&redacted).map_err(|_| "redaction_encode_failed".to_owned())?;
    if encoded.len() > profile.max_bytes || encoded.len() > MAX_REDACTION_PROFILE_BYTES {
        return Err("redaction_value_too_large".to_owned());
    }
    Ok(BoundedRedactedValue {
        value: redacted,
        encoded_bytes: encoded.len(),
        profile_digest: profile.profile_digest.clone(),
    })
}

/// 对文本执行 profile 约束下的脱敏、哨兵扫描和 UTF-8 字节数检查。
///
/// 处理次序是：验证 profile/NUL → 对有限范围内的畸形 JSON 凭证键做额外拒绝检查 → 调用
/// `redact_text()` → 检查脱敏幂等性与基础 marker → 用 `Transcript` 通道扫描 → 检查最终字节数。
/// profile 的 `signal` 决定默认配置，不会改变这里固定使用的 `Transcript` 扫描通道。
///
/// 畸形结构预检只在去除首尾空白后以 `{` 或 `[` 开始、且去空白文本不超过配置上限时运行；
/// 它是启发式拒绝检查，不是通用 JSON 修复器。任何错误都以错误码返回而不返回输入文本；
/// 调用方决定是否拒绝对应日志/导出操作，本函数没有持久化或重试副作用。
pub fn encode_bounded_text(
    profile: &RedactionProfile,
    text: &str,
) -> Result<BoundedRedactedText, String> {
    encode_bounded_text_for(profile, SecretScanChannel::Transcript, text)
}

/// 与 [`encode_bounded_text`] 相同，但用调用方给出的通道标签做残余哨兵扫描。
///
/// 用于文本信号进入具体通道（如 Stdout/Stderr 预览、Prompt、Env 派生文本）之前的统一检查；
/// 通道标签只影响 finding 的来源标注和调用方记录，不改变 `redact_text` 的替换规则。任何失败都
/// 以错误码返回而不返回输入文本，调用方决定是否阻断该次发布。
pub fn encode_bounded_text_for(
    profile: &RedactionProfile,
    channel: SecretScanChannel,
    text: &str,
) -> Result<BoundedRedactedText, String> {
    profile.validate()?;
    if text.as_bytes().contains(&0) {
        return Err("redaction_nul_forbidden".to_owned());
    }
    let trimmed = text.trim();
    let malformed_structured_secret = (trimmed.starts_with('{') || trimmed.starts_with('['))
        && trimmed.len() <= profile.max_bytes
        && trimmed.len() <= MAX_REDACTION_PROFILE_BYTES
        && serde_json::from_str::<Value>(trimmed).is_err()
        && contains_malformed_json_secret_key(trimmed);
    if malformed_structured_secret {
        return Err("redaction_secret_sentinel_detected".to_owned());
    }
    let redacted = redact_text(text);
    if redact_text(&redacted) != redacted || contains_text_secret_marker(&redacted) {
        return Err("redaction_secret_sentinel_detected".to_owned());
    }
    scan_secret_sentinels(channel, &redacted)
        .map_err(|_| "redaction_secret_sentinel_detected".to_owned())?;
    let encoded_bytes = redacted.len();
    if encoded_bytes > profile.max_bytes || encoded_bytes > MAX_REDACTION_PROFILE_BYTES {
        return Err("redaction_text_too_large".to_owned());
    }
    Ok(BoundedRedactedText {
        text: redacted,
        encoded_bytes,
        profile_digest: profile.profile_digest.clone(),
    })
}

/// 结构化值编码器的便捷包装：保留 profile 校验与所有拒绝条件，只返回脱敏后的 JSON。
///
/// 调用方拿不到本次 `encoded_bytes` 元数据；需要记录实际大小时应调用 `encode_bounded_value()`。
/// 该包装不会将结果自动发送到某个信号出口。
pub fn redact_with_profile(profile: &RedactionProfile, value: &Value) -> Result<Value, String> {
    encode_bounded_value(profile, value).map(|encoded| encoded.value)
}

/// 文本编码器的便捷包装：保留 profile 校验与所有拒绝条件，只返回脱敏后的字符串。
/// 需要同时使用字节数或 profile 摘要时应调用 `encode_bounded_text()`。
pub fn redact_text_with_profile(profile: &RedactionProfile, text: &str) -> Result<String, String> {
    encode_bounded_text(profile, text).map(|encoded| encoded.text)
}

/// 按已知标记扫描一段完整文本；命中或输入超限时返回不含原始文本的 finding。
///
/// 【调用者】Runner、Daemon、Entrypoints、Domain/Core 的多个显式输出校验点会把自己的文本和
/// `SecretScanChannel` 传进来。通道只是诊断标签，真正决定是否扫描的是调用方是否调用了本函数。
/// `EventStore` 的 `validate_secret_free()` 路径不调用文本扫描；`kiana-eventlog` 的存储校验另外
/// 调用 `scan_secret_value_with_limit(SecretScanChannel::Event, ...)` 补上脱敏不重写的残留形状。
///
/// 【检查内容】有界/NUL 输入、常见凭证与认证头 marker、合法 JSON 中的非空敏感字段、URL 用户信息、
/// 有限形状的 JWT，以及 `sk-` token 前缀。规则面向常见泄漏形状，可能漏掉未覆盖的凭证格式，也可能
/// 因形状相似而拒绝普通文本；`Ok(())` 只代表这些规则没有命中。
///
/// 对已经脱敏的候选文本通常先检查替换结果；原始 Provider 错误应调用 `project_redacted_error()`，
/// 让调用方得到不包含错误明文的投影。此函数只返回 `Err(finding)`，不会自行清理、记录、重试或禁用账号。
pub fn scan_secret_sentinels(
    channel: SecretScanChannel,
    text: &str,
) -> Result<(), SecretSentinelFinding> {
    if text.len() > MAX_SECRET_SENTINEL_SCAN_BYTES || text.as_bytes().contains(&0) {
        return Err(SecretSentinelFinding::new(
            channel,
            SecretSentinelKind::ProviderRawError,
            "bounded_or_nul_input",
        ));
    }
    let lowered = text.to_ascii_lowercase();
    for (kind, marker) in [
        (SecretSentinelKind::Header, "authorization: bearer "),
        (SecretSentinelKind::Header, "authorization: basic "),
        (SecretSentinelKind::Header, "proxy-authorization: "),
        (SecretSentinelKind::Header, "x-api-key:"),
        (SecretSentinelKind::ApiKey, "api_key="),
        (SecretSentinelKind::ApiKey, "api_key:"),
        (SecretSentinelKind::ApiKey, "api-key="),
        (SecretSentinelKind::ApiKey, "api-key:"),
        (SecretSentinelKind::ApiKey, "access_key="),
        (SecretSentinelKind::ApiKey, "access_key:"),
        (SecretSentinelKind::ApiKey, "client_secret="),
        (SecretSentinelKind::ApiKey, "client_secret:"),
        (SecretSentinelKind::Token, "access_token="),
        (SecretSentinelKind::Token, "access_token:"),
        (SecretSentinelKind::Token, "refresh_token="),
        (SecretSentinelKind::Token, "refresh_token:"),
        (SecretSentinelKind::Token, "id_token="),
        (SecretSentinelKind::Token, "id_token:"),
        (SecretSentinelKind::Token, "token="),
        (SecretSentinelKind::Token, "token:"),
        (SecretSentinelKind::Token, "password="),
        (SecretSentinelKind::Token, "password:"),
        (SecretSentinelKind::Token, "secret="),
        (SecretSentinelKind::Token, "secret:"),
        (SecretSentinelKind::Token, "private_key="),
        (SecretSentinelKind::Token, "private_key:"),
    ] {
        if marker_has_unredacted_value(&lowered, marker) {
            return Err(SecretSentinelFinding::new(channel, kind, marker));
        }
    }
    if contains_json_secret_key(text) {
        return Err(SecretSentinelFinding::new(
            channel,
            SecretSentinelKind::CredentialLease,
            "json_secret_key",
        ));
    }
    if contains_url_userinfo(text) {
        return Err(SecretSentinelFinding::new(
            channel,
            SecretSentinelKind::UrlUserinfo,
            "url_userinfo",
        ));
    }
    if contains_jwt_shape(text) {
        return Err(SecretSentinelFinding::new(
            channel,
            SecretSentinelKind::Jwt,
            "jwt_shape",
        ));
    }
    // OpenAI / Anthropic 风格的部分 API key 使用 `sk-` 前缀，也可能嵌在逻辑引用中，
    // 例如 `artifact/sk-live-...`。按 token 边界检查可覆盖“不是整段文本开头”的情况；
    // 这仍只是启发式前缀规则，且只有实际调用本 scanner 的输出才会执行此检查。
    if lowered
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '-')
        .any(|token| token.starts_with("sk-"))
    {
        return Err(SecretSentinelFinding::new(
            channel,
            SecretSentinelKind::ApiKey,
            "sk-",
        ));
    }
    Ok(())
}

/// 递归扫描一个结构化 JSON 值，常用于序列化 Event/Receipt/cache 之前的显式检查。
///
/// 先将整个值编码成 JSON，超过 64 KiB 或无法编码时拒绝；随后递归对象、数组和字符串。敏感字段名下
/// 的非空值只有恰好等于 `[REDACTED]` 的字符串例外；`secret_ref` / `credential_ref` 仅免于字段名
/// 规则，内部值仍会继续递归检查。失败返回不含原文的 `SecretSentinelFinding`。
/// 此 API 不会自动挂接到每个 EventStore 或 Receipt 写入路径。
pub fn scan_secret_value(
    channel: SecretScanChannel,
    value: &Value,
) -> Result<(), SecretSentinelFinding> {
    scan_secret_value_with_limit(channel, value, MAX_SECRET_SENTINEL_SCAN_BYTES)
}

/// 扫描结构化值，同时允许调用方为整个 JSON 树指定一个更大的聚合上限。
///
/// 文本节点仍使用 `MAX_SECRET_SENTINEL_SCAN_BYTES` 的单节点上限；较大的上限只适用于
/// 已经由更高层协议（例如 1 MiB EventLog 载荷）约束的多个小字段组成的树。这样不会把
/// 大对象误判成 Provider 原始错误，也不会因为放宽聚合大小而允许超长单字段绕过扫描。
pub fn scan_secret_value_with_limit(
    channel: SecretScanChannel,
    value: &Value,
    max_encoded_bytes: usize,
) -> Result<(), SecretSentinelFinding> {
    let encoded = serde_json::to_vec(value).map_err(|_| {
        SecretSentinelFinding::new(
            channel,
            SecretSentinelKind::ProviderRawError,
            "encode_failed",
        )
    })?;
    if encoded.len() > max_encoded_bytes {
        return Err(SecretSentinelFinding::new(
            channel,
            SecretSentinelKind::ProviderRawError,
            "value_too_large",
        ));
    }
    scan_secret_value_inner(channel, value)
}

/// 对调用方列出的多个文本通道执行扫描，并检查这些文本是否回显了给定哨兵。
///
/// 【当前用途】源码中直接调用本函数的位置是 Domain 的红队/集成测试 fixture；它接收的通道列表是
/// 测试或调用方提供的样本，不会自动发现生产系统中的所有 sink。
/// `echo_sentinels` 最多 64 项，每项非空且不超过 512 字节；通道列表本身没有条数上限。
/// 任一通道扫描失败或出现不区分大小写的哨兵子串时立即返回 finding。finding 不保存哨兵明文，
/// 但 `Echo` 类别的摘要是对传入哨兵求出的，低熵值仍可能被猜测；本函数不会改写输入或替换命中内容。
pub fn scan_secret_channels(
    channels: &[(SecretScanChannel, &str)],
    echo_sentinels: &[&str],
) -> Result<(), SecretSentinelFinding> {
    if echo_sentinels.len() > MAX_SECRET_SENTINELS
        || echo_sentinels
            .iter()
            .any(|sentinel| sentinel.is_empty() || sentinel.len() > 512)
    {
        return Err(SecretSentinelFinding::new(
            SecretScanChannel::Cache,
            SecretSentinelKind::Echo,
            "sentinel_set_invalid",
        ));
    }
    for &(channel, text) in channels {
        scan_secret_sentinels(channel, text)?;
        let lowered = text.to_ascii_lowercase();
        for sentinel in echo_sentinels {
            if lowered.contains(&sentinel.to_ascii_lowercase()) {
                return Err(SecretSentinelFinding::new(
                    channel,
                    SecretSentinelKind::Echo,
                    sentinel,
                ));
            }
        }
    }
    Ok(())
}

/// 将 Provider/transport 原始错误投影为不含原文的诊断对象。
///
/// 【调用关系与输出】调用本函数的一方提供目标通道和原始错误；函数用哨兵扫描决定 `code` 与
/// `redacted`，再对 `redact_text(error)` 的结果和通道求摘要。返回值只含 schema、通道、有限错误码、
/// 摘要和命中布尔值，调用方可持久化该对象，而不会把原始 Provider 消息带到下游。
///
/// `redacted == true` 表示原始错误命中了本模块扫描器，不保证 `redact_text()` 实际替换了某个字符；
/// `false` 也不能证明错误不含未知格式的凭证。这里不记录日志、不写 EventLog，也不发起重试。
pub fn project_redacted_error(channel: SecretScanChannel, error: &str) -> RedactedErrorProjection {
    let original_scan = scan_secret_sentinels(channel, error).is_err();
    let redacted = redact_text(error);
    let error_digest = json_digest(&serde_json::json!({
        "channel": channel.as_str(),
        "error": redacted,
    }));
    let projection = RedactedErrorProjection {
        schema: REDACTED_ERROR_PROJECTION_SCHEMA.to_owned(),
        channel,
        code: if original_scan {
            "provider_error_redacted".to_owned()
        } else {
            "provider_error".to_owned()
        },
        error_digest,
        redacted: original_scan,
    };
    debug_assert!(projection.validate().is_ok());
    projection
}

/// 在递归处理 JSON 前后统一检查深度和 NUL，避免有界编码器处理超出约定形状的值。
///
/// 根值从深度 0 开始；数组元素和对象字段值各增加一层。对象字段名及字符串值都不能含 NUL。
/// 超过 `max_depth` 时向上传播 `redaction_value_depth_exceeded`，不会截断子树或部分返回结果。
fn validate_value_shape(value: &Value, depth: usize, max_depth: usize) -> Result<(), String> {
    if depth > max_depth {
        return Err("redaction_value_depth_exceeded".to_owned());
    }
    match value {
        Value::Array(items) => {
            for item in items {
                validate_value_shape(item, depth + 1, max_depth)?;
            }
        }
        Value::Object(object) => {
            for (key, item) in object {
                if key.as_bytes().contains(&0) {
                    return Err("redaction_key_nul_forbidden".to_owned());
                }
                validate_value_shape(item, depth + 1, max_depth)?;
            }
        }
        Value::String(text) if text.as_bytes().contains(&0) => {
            return Err("redaction_nul_forbidden".to_owned());
        }
        _ => {}
    }
    Ok(())
}

/// 判断文本 marker 后面是否不是完整的 `[REDACTED]` 占位值。
///
/// 调用方已将文本转为小写；这里仅略过空格和制表符，再让占位符校验器确认替换值后面有合理边界。
/// 它是 marker 检查的一部分，不是通用的凭证格式解析器。
fn marker_has_unredacted_value(lowered: &str, marker: &str) -> bool {
    let mut offset = 0;
    while let Some(relative) = lowered[offset..].find(marker) {
        let value_start = offset + relative + marker.len();
        let value = lowered[value_start..].trim_start_matches([' ', '\t']);
        if !redacted_placeholder_is_complete(value) {
            return true;
        }
        offset = value_start;
    }
    false
}

/// 确认 marker 后紧接完整占位符，且占位符之后是文本结尾或字段分隔符。
///
/// 这样 `token=[REDACTED]suffix` 不会因为包含占位符就被误判为已完全清理。
fn redacted_placeholder_is_complete(value: &str) -> bool {
    let Some(remainder) = value.strip_prefix("[redacted]") else {
        return false;
    };
    remainder.is_empty()
        || remainder.chars().next().is_some_and(|character| {
            character.is_whitespace()
                || matches!(character, '&' | ',' | ';' | '"' | '}' | ']' | '\\')
        })
}

/// 对疑似 JSON 的畸形文本做轻量凭证键预检，供 `encode_bounded_text()` 在脱敏前调用。
///
/// 它寻找双引号包围、后面跟冒号的字符串，并复用 `secret_field_key()` 判断键名；它不是真正的 JSON
/// 解析器，不处理转义引号、嵌套语法或合法 JSON 的值语义。调用方只在文本以 `{` / `[` 开始、JSON
/// 解析失败且输入大小受限时使用它；命中后采取拒绝策略，避免把可疑畸形凭证文本当普通文字放行。
fn contains_malformed_json_secret_key(text: &str) -> bool {
    let mut cursor = 0;
    while let Some(relative_start) = text[cursor..].find('"') {
        let start = cursor + relative_start + 1;
        let Some(relative_end) = text[start..].find('"') else {
            break;
        };
        let end = start + relative_end;
        let key = &text[start..end];
        let remainder = &text[end + 1..];
        if remainder.trim_start().starts_with(':')
            && secret_field_key(key, &Value::String(String::new()))
        {
            return true;
        }
        cursor = end + 1;
    }
    false
}

/// 使用 serde_json 解析完整 JSON，并递归查找非空、未被 `[REDACTED]` 替换的敏感字段。
///
/// 非法 JSON 在这里返回 `false`，由文本扫描的 marker 规则或有界编码器的畸形 JSON 预检处理；
/// 因此不能单独调用本 helper 来证明一段任意文本没有凭证。
pub(crate) fn contains_json_secret_key(text: &str) -> bool {
    let trimmed = text.trim();
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return false;
    };
    fn visit(value: &Value) -> bool {
        match value {
            Value::Array(items) => items.iter().any(visit),
            Value::Object(fields) => fields.iter().any(|(key, value)| {
                (secret_field_key(key, value)
                    && !value.is_null()
                    && !matches!(value, Value::String(text) if text == REDACTED))
                    || visit(value)
            }),
            _ => false,
        }
    }
    visit(&value)
}

/// 扫描已通过总编码大小限制的 JSON 树；敏感字段先拒绝，再递归检查字段值和文本字符串。
///
/// 这里返回第一个 finding，避免错误对象收集或暴露全部原始敏感字段。
fn scan_secret_value_inner(
    channel: SecretScanChannel,
    value: &Value,
) -> Result<(), SecretSentinelFinding> {
    match value {
        Value::Array(items) => {
            for item in items {
                scan_secret_value_inner(channel, item)?;
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                let normalized = normalize_secret_key(key);
                if secret_field_key(key, item)
                    && !item.is_null()
                    && !matches!(item, Value::String(text) if text == REDACTED)
                {
                    return Err(SecretSentinelFinding::new(
                        channel,
                        SecretSentinelKind::CredentialLease,
                        &normalized,
                    ));
                }
                scan_secret_value_inner(channel, item)?;
            }
        }
        Value::String(text) => scan_secret_sentinels(channel, text)?,
        _ => {}
    }
    Ok(())
}

/// 统一字段名大小写及连字符，令 `api-key` 与 `api_key` 等形式共用匹配规则。
fn normalize_secret_key(key: &str) -> String {
    key.to_ascii_lowercase().replace('-', "_")
}

/// 判断字段是否是 secret 的不透明引用句柄，而非 secret 明文本身。
///
/// 引用字段名本身不触发敏感字段规则；对象内其他字段仍会继续递归检查。
fn is_opaque_reference_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("secret_ref") || key.eq_ignore_ascii_case("credential_ref")
}

/// 按字段名和 JSON 值类型判断是否应把该字段视为敏感。
///
/// `secret_ref`/`credential_ref` 是可保存的句柄；一组明确允许的数值 token 计量字段是统计数据。
/// 其他含 token/password/key/secret/credential 等语义的字段按敏感字段处理。
fn secret_field_key(key: &str, value: &Value) -> bool {
    let normalized = normalize_secret_key(key);
    if is_opaque_reference_key(key) {
        return false;
    }
    !is_token_metric(key, value)
        && (normalized.contains("token")
            || normalized.contains("password")
            || normalized.contains("api_key")
            || normalized.contains("access_key")
            || normalized.contains("private_key")
            || normalized == "authorization"
            || normalized == "proxy_authorization"
            || normalized == "x_api_key"
            || normalized == "secret"
            || normalized == "credential"
            || (normalized.contains("credential") && !normalized.ends_with("_generation")))
}

/// 识别允许保留的 token 数量统计字段，避免把用量计数误当成身份凭证。
///
/// 只有名单中的字段名且值为 Number、Bool 或 Null 时才例外；同名字符串仍按敏感内容处理。
/// 脱敏与残留检查共用此规则，增删名单会同时改变持久化内容和扫描结果。
fn is_token_metric(key: &str, value: &Value) -> bool {
    let normalized = normalize_secret_key(key);
    matches!(
        normalized.as_str(),
        "reserved_tokens"
            | "tokens"
            | "charged_tokens"
            | "reported_tokens"
            | "charged_and_reserved_tokens"
            | "tokens_before"
            | "tokens_after"
            | "input_tokens"
            | "output_tokens"
            | "max_output_tokens"
            | "total_tokens"
            | "tokens_used"
            | "cached_tokens"
            | "reasoning_tokens"
            | "max_tokens"
            | "min_tokens"
            | "token_count"
            | "token_budget"
            | "estimated_tokens"
            | "token_overlap"
    ) && matches!(value, Value::Number(_) | Value::Bool(_) | Value::Null)
}

/// 检查 `scheme://user:password@host` 形式的 URL authority 中是否含用户信息。
///
/// 每次从 `://` 后扫描到路径、查询、片段或空白处，再检查 authority 最后一个 `@` 前是否有内容；
/// 这是文本形状识别，不做 URL 完整解析。已替换为 `[redacted]@` 的凭证前缀视为脱敏标记。
fn contains_url_userinfo(text: &str) -> bool {
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find("://") {
        let scheme_end = cursor + relative;
        let authority_start = scheme_end + 3;
        let authority_end = text[authority_start..]
            .find(|character: char| matches!(character, '/' | '?' | '#' | ' ' | '\n' | '\r'))
            .map_or(text.len(), |offset| authority_start + offset);
        let authority = &text[authority_start..authority_end];
        if let Some(at) = authority.rfind('@') {
            if at > 0 && !authority[..at].eq_ignore_ascii_case("[redacted]") {
                return true;
            }
        }
        cursor = authority_end;
    }
    false
}

/// 按常见文本分隔符切分候选 token，再检查每段是否符合本模块采用的 JWT 启发式形状。
///
/// JSON Web Token（JWT）是一种常见的身份令牌，通常由三段以点号分隔的编码文本组成。此处只识别形状，不验证签名、
/// issuer、过期时间或 token 内容，因此命中只能作为拒绝/清理信号，不能作为认证结论。
fn contains_jwt_shape(text: &str) -> bool {
    text.split(|character: char| {
        character.is_whitespace()
            || matches!(character, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']')
    })
    .any(looks_like_jwt)
}

/// 用三段结构、`eyJ` 常见 header 前缀和字符集作轻量 JWT 候选判断。
///
/// 每段至少 8 个字节只是降低普通短字符串误报的启发式门槛；它既可能漏掉变体，也可能把形状相似的
/// 普通文本判为 JWT。这里没有密码学验签，也不检查 token 是否有效。
fn looks_like_jwt(candidate: &str) -> bool {
    let candidate = candidate
        .rsplit_once('=')
        .map_or(candidate, |(_, value)| value);
    let candidate = candidate
        .rsplit_once(':')
        .map_or(candidate, |(_, value)| value);
    let candidate = candidate.trim_matches(|character: char| {
        matches!(character, '.' | ':' | '=' | '&' | '?' | '/' | '{' | '}')
    });
    let mut segments = candidate.split('.');
    let (Some(header), Some(payload), Some(signature), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return false;
    };
    header.starts_with("eyJ")
        && header.len() >= 8
        && payload.len() >= 8
        && signature.len() >= 8
        && [header, payload, signature].iter().all(|segment| {
            segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
}

/// 检查结构化脱敏结果中是否仍存在敏感字段值或已知文本 marker。
///
/// 它用于编码器的第二道本地检查；SecretRef 句柄和数值 token 计量字段按专门规则放行，
/// 其他敏感字段只有为空或恰好替换为 `[REDACTED]` 才视为清理完成。
fn contains_unredacted_secret(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(contains_unredacted_secret),
        Value::Object(object) => object.iter().any(|(key, value)| {
            let normalized = normalize_secret_key(key);
            let sensitive = !is_opaque_reference_key(key)
                && ((normalized.contains("token") && !is_token_metric(key, value))
                    || normalized.contains("password")
                    || normalized.contains("api_key")
                    || normalized.contains("access_key")
                    || normalized.contains("private_key")
                    || normalized == "authorization"
                    || normalized == "proxy_authorization"
                    || normalized == "x_api_key"
                    || (normalized.contains("credential") && !normalized.ends_with("_generation"))
                    || normalized.contains("secret"));
            (sensitive
                && !value.is_null()
                && !matches!(value, Value::String(text) if text == REDACTED))
                || contains_unredacted_secret(value)
        }),
        Value::String(text) => contains_text_secret_marker(text),
        _ => false,
    }
}

/// 检查一组常见的未脱敏文本标记，不负责识别未带这些标记的随机凭证。
///
/// 当前实现只要整段文本含有 `[REDACTED]` 就不再将名单里的 marker 判为未脱敏；这是较粗的快速检查，
/// 不能独立证明同一段文本中的其他 marker 都已被替换。精确占位边界和更全面的形状检查由其他 helper 处理。
fn contains_text_secret_marker(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "token=",
        "password=",
        "api_key=",
        "access_key=",
        "private_key=",
        "secret=",
        "bearer ",
        "authorization: bearer ",
        "authorization: basic ",
        "x-api-key:",
    ]
    .iter()
    .any(|marker| lowered.contains(marker) && !lowered.contains("[redacted]"))
}

#[derive(Clone, Copy, Debug)]
/// 一种可识别的文本敏感标记，以及其值是否由双引号界定。
struct SensitiveMarker {
    /// 匹配时不区分 ASCII 大小写的前缀。
    literal: &'static str,
    /// `true` 表示值延伸到下一个双引号；否则延伸到空白或配置的标点分隔符。
    quoted_value: bool,
}

/// `redact_text()` 和 `StreamingRedactor` 共用的文本前缀清单。
///
/// 新增 marker 会改变日志、事件、Provider 错误和跨分片输出的替换边界；完整文本与流式路径都依赖
/// 这里的条目，但 URL 用户信息/JWT 仅由完整文本 `redact_text()` 的额外 helper 处理。
const SENSITIVE_MARKERS: &[SensitiveMarker] = &[
    SensitiveMarker {
        literal: "token=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "token:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "access_token:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "refresh_token:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "id_token:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "password=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "password:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "api_key=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "api_key:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "api-key=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "api-key:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "access_key=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "access_key:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "private_key=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "private_key:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "secret=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "secret:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "bearer ",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "authorization: bearer ",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "authorization: basic ",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "x-api-key:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "\"token\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"password\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"api_key\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"access_key\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"private_key\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"secret\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"authorization\":\"bearer ",
        quoted_value: true,
    },
];

/// 对一段完整文本执行尽力而为的脱敏。
///
/// 【它做两件事，顺序不能换】
/// 1. 如果去掉首尾空白后**看起来像 JSON**（首字符是 `{` 或 `[`）且能解析成功，
///    就走 [ `redact_value` ] 做**结构化**脱敏——按字段名判断，而不是按文本模式；
/// 2. 否则按标记做**文本扫描**：替换 `api_key`、`token` 等已知前缀后面的值，
///    再处理 URL 用户信息和 JWT 候选形状。
///
/// 【为什么 JSON 要走结构化】
/// 因为结构化脱敏能按**字段名**判断敏感度，而文本扫描只能按**出现位置**猜。
/// 一段 `{"notes": "the api_key is abc"}` 里，结构化路径知道 `notes` 不是敏感字段，
/// 而文本路径会把 `abc` 也一起替换掉——过度脱敏会让日志失去诊断价值。
///
/// 【返回值边界】返回 `String` 而非 `Result`，所以它不会报告“发现但无法安全处理”的状态。
/// JSON 解析失败时退回文本 marker 扫描；没有已知 marker 的随机凭证可能原样保留。要求任何异常都
/// 必须拒绝输出的调用点应使用 `redact_text_with_profile()`，并继续处理它返回的错误。
///
/// JSON 成功解析时结果会重新序列化为紧凑 JSON，因此原来的缩进和空白布局可能改变；普通文本路径则
/// 在原字符串上替换。此函数只转换文本，不记录日志或写入任何存储。
pub fn redact_text(text: &str) -> String {
    let trimmed = text.trim();
    if matches!(trimmed.as_bytes().first(), Some(b'{') | Some(b'[')) {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return redact_value(&value).to_string();
        }
    }

    let mut redacted = text.to_owned();
    for marker in SENSITIVE_MARKERS {
        let marker_lower = marker.literal.to_ascii_lowercase();
        let mut search_from = 0;
        while search_from < redacted.len() {
            let lower = redacted.to_ascii_lowercase();
            let Some(relative_start) = lower[search_from..].find(&marker_lower) else {
                break;
            };
            let start = search_from + relative_start + marker.literal.len();
            let value_start = if marker.quoted_value {
                start
            } else {
                start
                    + redacted[start..]
                        .chars()
                        .take_while(|character| character.is_whitespace())
                        .map(char::len_utf8)
                        .sum::<usize>()
            };
            let end = if marker.quoted_value {
                redacted[value_start..]
                    .find('"')
                    .map_or(redacted.len(), |relative_end| value_start + relative_end)
            } else {
                redacted[value_start..]
                    .find(|character: char| {
                        character.is_whitespace()
                            || matches!(character, '&' | ',' | ';' | '"' | '}')
                    })
                    .map_or(redacted.len(), |relative_end| value_start + relative_end)
            };
            if end <= value_start {
                // A streaming producer can flush a marker prefix before its value arrives.
                // Close that dangling marker with the same placeholder used for a complete
                // value so the event boundary never persists a bare Authorization/token header.
                let marker_start = search_from + relative_start;
                let marker_prefix = redacted[marker_start..value_start].to_owned();
                redacted.replace_range(
                    marker_start..redacted.len(),
                    &format!("{marker_prefix}{REDACTED}"),
                );
                break;
            }
            redacted.replace_range(value_start..end, REDACTED);
            search_from = value_start + REDACTED.len();
        }
    }
    redacted = redact_url_userinfo(&redacted);
    redacted = redact_jwt_shapes(&redacted);
    redacted
}

/// 替换完整文本 URL authority 中 `@` 之前的用户名/密码片段。
///
/// 只按 `://` 和 URL 常见分隔符寻找 authority，不做完整 URL 校验；脱敏文本可能不是规范化 URL。
fn redact_url_userinfo(text: &str) -> String {
    let mut output = text.to_owned();
    let mut cursor = 0;
    while let Some(relative) = output[cursor..].find("://") {
        let scheme_end = cursor + relative;
        let authority_start = scheme_end + 3;
        let authority_end = output[authority_start..]
            .find(|character: char| matches!(character, '/' | '?' | '#' | ' ' | '\n' | '\r'))
            .map_or(output.len(), |offset| authority_start + offset);
        let authority = &output[authority_start..authority_end];
        let Some(at) = authority.rfind('@') else {
            cursor = authority_end;
            continue;
        };
        if at > 0 && !authority[..at].eq_ignore_ascii_case("[redacted]") {
            output.replace_range(authority_start..authority_start + at, "[REDACTED]");
            cursor = authority_start + "[REDACTED]".len();
        } else {
            cursor = authority_end;
        }
    }
    output
}

/// 将按常见分隔符独立出现、且符合 JWT 启发式形状的 token 替换为固定占位符。
///
/// 这里与 scanner 使用相同形状识别，不验签；被保留的边界字符仍原样写入输出。
fn redact_jwt_shapes(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for (index, character) in text.char_indices() {
        let boundary = character.is_whitespace()
            || matches!(character, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']');
        if !boundary {
            continue;
        }
        if index > cursor {
            let token = &text[cursor..index];
            if looks_like_jwt(token) {
                output.push_str("[REDACTED]");
            } else {
                output.push_str(token);
            }
        }
        output.push(character);
        cursor = index + character.len_utf8();
    }
    if cursor < text.len() {
        let token = &text[cursor..];
        if looks_like_jwt(token) {
            output.push_str("[REDACTED]");
        } else {
            output.push_str(token);
        }
    }
    output
}

/// 按对象字段名递归脱敏 JSON，同时对普通字符串值调用 `redact_text()`。
///
/// 数组逐项递归；对象中的敏感字段只替换其非空值为 `[REDACTED]`，保留字段名和其他结构；
/// 允许的 token 数量统计与不透明引用句柄保持原值，但其内部内容仍按普通递归规则检查。
/// 本函数不验证深度/字节上限，也不返回错误；需要边界保证时由 `encode_bounded_value()` 包装。
pub fn redact_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let normalized = normalize_secret_key(key);
                    let token_metric = is_token_metric(key, value);
                    let sensitive = !is_opaque_reference_key(key)
                        && ((normalized.contains("token") && !token_metric)
                            || normalized.contains("password")
                            || normalized.contains("api_key")
                            || normalized.contains("access_key")
                            || normalized.contains("private_key")
                            || normalized == "authorization"
                            || normalized == "proxy_authorization"
                            || normalized == "x_api_key"
                            || (normalized.contains("credential")
                                && !normalized.ends_with("_generation"))
                            || normalized.contains("secret"));
                    let value = if sensitive && !value.is_null() {
                        Value::String(REDACTED.to_owned())
                    } else {
                        redact_value(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::String(text) => Value::String(redact_text(text)),
        _ => value.clone(),
    }
}

#[derive(Clone, Copy, Debug)]
/// `StreamingRedactor` 在处理一个可能跨分片的敏感值时所处的状态。
enum Suppression {
    /// 已识别带引号 marker，跳过值直到下一个双引号。
    Quoted,
    /// 已识别非引号 marker；先保留 marker 后的空白，再开始丢弃值。
    UnquotedAwaitingValue,
    /// 正在丢弃非引号值，直到空白或配置的标点边界。
    UnquotedValue,
}

#[derive(Clone, Copy, Debug)]
enum CandidateKind {
    UrlUserinfo,
    Jwt,
}

#[derive(Clone, Copy, Debug)]
struct CandidateStart {
    kind: CandidateKind,
}

impl CandidateStart {
    fn into_deferred(self, prefix: &str) -> DeferredCandidate {
        let mut candidate = DeferredCandidate {
            kind: self.kind,
            buffer: String::new(),
            overflowed: false,
        };
        append_candidate_bytes(&mut candidate, prefix);
        candidate
    }
}

#[derive(Debug)]
struct DeferredCandidate {
    kind: CandidateKind,
    buffer: String,
    overflowed: bool,
}

impl DeferredCandidate {
    fn buffered_len(&self) -> usize {
        self.buffer.len()
    }
}

enum EitherSensitive {
    Marker(SensitiveMarker),
    Candidate(CandidateStart),
}

fn candidate_delimiter(kind: CandidateKind, character: char) -> bool {
    match kind {
        CandidateKind::UrlUserinfo => {
            matches!(character, '/' | '?' | '#' | ' ' | '\n' | '\r')
        }
        CandidateKind::Jwt => {
            character.is_whitespace()
                || matches!(character, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']')
        }
    }
}

fn append_candidate_bytes(candidate: &mut DeferredCandidate, text: &str) {
    if candidate.overflowed || text.is_empty() {
        return;
    }
    let available = STREAM_REDACTION_BUFFER_LIMIT.saturating_sub(candidate.buffer.len());
    if text.len() <= available {
        candidate.buffer.push_str(text);
        return;
    }
    let mut end = available;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    candidate.buffer.push_str(&text[..end]);
    candidate.overflowed = true;
}

fn finish_candidate(candidate: DeferredCandidate) -> String {
    if candidate.overflowed {
        return REDACTED.to_owned();
    }
    match candidate.kind {
        CandidateKind::UrlUserinfo => redact_url_authority(&candidate.buffer),
        CandidateKind::Jwt => {
            if looks_like_jwt(&candidate.buffer) {
                REDACTED.to_owned()
            } else {
                candidate.buffer
            }
        }
    }
}

fn redact_url_authority(authority: &str) -> String {
    let Some(at) = authority.rfind('@') else {
        return authority.to_owned();
    };
    if at == 0 || authority[..at].eq_ignore_ascii_case(REDACTED) {
        authority.to_owned()
    } else {
        format!("{REDACTED}{}", &authority[at..])
    }
}

fn consume_candidate(candidate: &mut DeferredCandidate, text: &str) -> (usize, String, bool) {
    let delimiter = text
        .char_indices()
        .find(|(_, character)| candidate_delimiter(candidate.kind, *character));
    let (body, consumed) =
        delimiter.map_or((text, text.len()), |(index, _)| (&text[..index], index));
    append_candidate_bytes(candidate, body);
    if delimiter.is_none() {
        return (consumed, String::new(), false);
    }
    (
        consumed,
        finish_candidate(std::mem::replace(
            candidate,
            DeferredCandidate {
                kind: candidate.kind,
                buffer: String::new(),
                overflowed: false,
            },
        )),
        true,
    )
}

impl Suppression {
    /// 判断当前状态下某个字符是否结束敏感值；返回分隔符由 `push()` 原样输出。
    fn is_delimiter(self, character: char) -> bool {
        match self {
            Self::Quoted => character == '"',
            Self::UnquotedValue => {
                character.is_whitespace() || matches!(character, '&' | ',' | ';' | '"' | '}')
            }
            Self::UnquotedAwaitingValue => false,
        }
    }
}

/// 对分片到达的文本进行状态化脱敏，例如模型或子进程逐段产生的输出。
///
/// 【调用者与数据流】Runner、Domain model-event、Daemon execution-control、Web 和 stream-render
/// 在持续收到文本片段时调用 `push()`，把返回的安全片段继续交给下游；结束时必须调用 `finish()` 刷新尾部状态。
/// 它处理 `SENSITIVE_MARKERS` 中的文本前缀和值边界，并对跨分片的 URL userinfo/JWT 做有界形状检查；
/// 它仍不解析 JSON，也不调用 secret sentinel scanner。
///
/// 普通文本会尽快返回，只在内部复制最多 64 字节的尾部重叠区，以便识别跨片段拆开的 marker。
/// URL authority 和 JWT 候选会暂存最多 64 字节，直到边界到达后再决定是否替换；超出上限的候选
/// 直接进入封闭状态并丢弃后续字节，因此不会把无界流或完整超长凭证保存在状态字段中。
///
/// ⚠ 调用方若遗漏 `finish()`，最后一个尚未遇到分隔符的敏感值可能不会产生占位符；若在 finish 后
/// 再调用 `push()`，新文本会作为独立完整文本交给 `redact_text()`，不会重新打开原流状态。
#[derive(Debug)]
pub struct StreamingRedactor {
    /// 已输出文本尾部的有限副本，用于与下一片段拼接并识别被拆开的 marker。
    overlap: String,
    /// 当前是否正在跳过一个已识别的敏感值，以及该值的边界模式。
    suppression: Option<Suppression>,
    /// 尚未遇到边界、可能是 URL 用户信息或 JWT 的有界候选。
    candidate: Option<DeferredCandidate>,
    /// 当前敏感值是否至少丢弃过一个字符；只保存布尔状态，不保存值本身。
    suppressed_any: bool,
    /// `finish()` 是否已结束这条流；之后的 `push()` 改为独立完整文本处理。
    finished: bool,
}

impl Default for StreamingRedactor {
    /// 默认构造与 `new()` 相同的空流状态。
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingRedactor {
    /// 创建没有输入、没有待隐藏值且尚未结束的流式脱敏器。
    ///
    /// debug 构建中额外核对所有 marker 都不超过重叠缓冲上限；否则跨 chunk 匹配可能丢失前缀。
    pub fn new() -> Self {
        debug_assert!(SENSITIVE_MARKERS
            .iter()
            .all(|marker| marker.literal.len() <= STREAM_REDACTION_BUFFER_LIMIT));
        Self {
            overlap: String::new(),
            suppression: None,
            candidate: None,
            suppressed_any: false,
            finished: false,
        }
    }

    /// 处理一个新片段，并返回当前已经可以安全交给下游的文本。
    ///
    /// 如果 marker 的开头在 `overlap`、结尾在当前分片，先拼接两者搜索；找到后保留可见 marker，
    /// 丢弃后续敏感值直至对应分隔符。非引号值前的空格/制表等空白按原样输出；分隔符也会保留。
    /// 这个状态机要跨调用保存 `suppression`，否则把 token 与值拆成两个 chunk 时，后一片会失去上下文。
    /// 片段不会写入存储或网络；返回值由调用方继续传递。对空片段返回空字符串。
    pub fn push(&mut self, text: &str) -> String {
        if self.finished {
            return redact_text(text);
        }
        let mut output = String::new();
        let mut remaining = text;
        while !remaining.is_empty() {
            if let Some(suppression) = self.suppression {
                if matches!(suppression, Suppression::UnquotedAwaitingValue) {
                    let whitespace_len = remaining
                        .chars()
                        .take_while(|character| character.is_whitespace())
                        .map(char::len_utf8)
                        .sum::<usize>();
                    output.push_str(&remaining[..whitespace_len]);
                    remaining = &remaining[whitespace_len..];
                    if remaining.is_empty() {
                        break;
                    }
                    self.suppression = Some(Suppression::UnquotedValue);
                    continue;
                }
                if let Some(index) = remaining.char_indices().find_map(|(index, character)| {
                    suppression.is_delimiter(character).then_some(index)
                }) {
                    if self.suppressed_any || index > 0 {
                        output.push_str(REDACTED);
                    }
                    self.suppression = None;
                    self.suppressed_any = false;
                    remaining = &remaining[index..];
                    continue;
                }
                self.suppressed_any = true;
                break;
            }

            if let Some(mut candidate) = self.candidate.take() {
                let (consumed, candidate_output, complete) =
                    consume_candidate(&mut candidate, remaining);
                output.push_str(&candidate_output);
                remaining = &remaining[consumed..];
                if complete {
                    continue;
                }
                self.candidate = Some(candidate);
                break;
            }

            let combined = format!("{}{}", self.overlap, remaining);
            let overlap_len = self.overlap.len();
            let marker = earliest_marker(&combined);
            let candidate = earliest_candidate(&combined);
            let (start, marker) = match (marker, candidate) {
                (None, None) => {
                    if let Some(partial_start) = partial_jwt_start(&combined, overlap_len) {
                        let emit_len = partial_start.saturating_sub(overlap_len);
                        output.push_str(&remaining[..emit_len]);
                        remaining = &remaining[emit_len..];
                        self.overlap.clear();
                        self.candidate = Some(
                            CandidateStart {
                                kind: CandidateKind::Jwt,
                            }
                            .into_deferred(""),
                        );
                        continue;
                    }
                    output.push_str(remaining);
                    self.retain_overlap(&combined);
                    break;
                }
                (Some((marker_start, marker)), None) => {
                    (marker_start, EitherSensitive::Marker(marker))
                }
                (None, Some((candidate_start, candidate))) => {
                    (candidate_start, EitherSensitive::Candidate(candidate))
                }
                (Some((marker_start, marker)), Some((candidate_start, candidate))) => {
                    if marker_start <= candidate_start {
                        (marker_start, EitherSensitive::Marker(marker))
                    } else {
                        (candidate_start, EitherSensitive::Candidate(candidate))
                    }
                }
            };

            match marker {
                EitherSensitive::Marker(marker) => {
                    let marker_end = start + marker.literal.len();
                    let emit_len = marker_end.saturating_sub(overlap_len).min(remaining.len());
                    output.push_str(&remaining[..emit_len]);
                    remaining = &remaining[emit_len..];
                    self.overlap.clear();
                    self.suppression = Some(if marker.quoted_value {
                        Suppression::Quoted
                    } else {
                        Suppression::UnquotedAwaitingValue
                    });
                    self.suppressed_any = false;
                }
                EitherSensitive::Candidate(candidate) => {
                    let emit_len = start.saturating_sub(overlap_len).min(remaining.len());
                    output.push_str(&remaining[..emit_len]);
                    remaining = &remaining[emit_len..];
                    let prefix = (start < overlap_len).then(|| &combined[start..overlap_len]);
                    self.overlap.clear();
                    self.candidate = Some(candidate.into_deferred(prefix.unwrap_or_default()));
                }
            }
        }
        output
    }

    /// 结束流并刷新尾部状态；若最后一个未完成的敏感值已丢弃过内容，追加 `[REDACTED]`。
    ///
    /// 清除重叠区并将 `finished` 设为 true。重复调用不再输出内容；调用方必须保留本次返回值，
    /// 否则流尾部的占位符也会丢失。
    pub fn finish(&mut self) -> String {
        let mut output = String::new();
        if self.finished {
            return output;
        }
        if self.suppression.is_some() {
            if self.suppressed_any {
                output.push_str(REDACTED);
            }
            self.suppression = None;
            self.suppressed_any = false;
        }
        if let Some(candidate) = self.candidate.take() {
            output.push_str(&finish_candidate(candidate));
        }
        self.overlap.clear();
        self.finished = true;
        output
    }

    /// 当前是否已识别 marker、正在等待敏感值结束。
    ///
    /// `true` 表示仍处于抑制状态，不表示 secret 明文被缓存在内存中；内容已被丢弃。
    pub fn has_pending_secret(&self) -> bool {
        self.suppression.is_some() || self.candidate.is_some()
    }

    /// 返回当前用于跨片段匹配的重叠区字节数，不包括已丢弃的敏感值。
    /// 正常 `push()` 后此数不超过 [`STREAM_REDACTION_BUFFER_LIMIT`]。
    pub fn buffered_len(&self) -> usize {
        self.overlap.len()
            + self
                .candidate
                .as_ref()
                .map_or(0, DeferredCandidate::buffered_len)
    }

    /// 只保留尾部最多 64 个字节，并将起点向前移动到 UTF-8 字符边界。
    ///
    /// UTF-8 字符可能占多个字节，不能直接从任意字节偏移切字符串；若移到合法边界后剩余片段略短于
    /// 上限，这是为了避免切开字符。保留值用于下一次 marker 匹配，不会再次输出。
    fn retain_overlap(&mut self, emitted: &str) {
        let mut start = emitted.len().saturating_sub(STREAM_REDACTION_BUFFER_LIMIT);
        while start < emitted.len() && !emitted.is_char_boundary(start) {
            start += 1;
        }
        self.overlap.clear();
        self.overlap.push_str(&emitted[start..]);
        debug_assert!(self.overlap.len() <= STREAM_REDACTION_BUFFER_LIMIT);
    }
}

/// 一段流式文本 `finish()` 后的尾部结果，以及治理该流的 profile 摘要和累计输出字节数。
///
/// `tail` 只是刷新出的尾部安全文本，不是整条流的拼接结果；调用方已经把此前每次 `push()` 的
/// 返回值发布出去。保留 `profile_digest` 是为了让“这段流由哪份脱敏配置治理”在记录里可定位。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedStreamTail {
    /// `finish()` 刷新出的尾部安全文本。
    pub tail: String,
    /// 该流至今发布的 UTF-8 字节数（累计，不含被抑制的敏感值）。
    pub emitted_bytes: usize,
    /// 治理该流的 profile 摘要。
    pub profile_digest: String,
}

/// 把 [`RedactionProfile`] 绑定到某个发布通道的增量脱敏 sink。
///
/// 【用途与数据流】模型增量、子进程输出预览等运行时 sink 持续收到文本分片；本 sink 在
/// [`StreamingRedactor`] 的有界状态机之上校验 profile、保留发布通道标签，并在出错时进入封闭
/// 状态而不是回退原文。`finish()` 返回的 [`BoundedStreamTail`] 携带 profile 摘要，使下游记录能
/// 定位实际使用的脱敏配置。
///
/// 【边界】它与 [`StreamingRedactor`] 一样只对可识别的 marker、URL 用户信息和有限 JWT 形状脱敏，
/// 不会逐一扫描任意未标记的随机凭证；整条流的最终事实级扫描仍由调用方在写入事件/收据/导出边界
/// 时执行（例如 `scan_secret_value_*`）。本类型不写日志、EventLog 或网络，也不缓存完整流内容。
#[derive(Debug)]
pub struct StreamingRedactionSink {
    /// 治理该流的配置；其摘要会随 `finish()` 结果一起返回。
    profile: RedactionProfile,
    /// 该流所属的发布通道标签，用于记录来源而不改变脱敏规则。
    channel: SecretScanChannel,
    /// 复用的有界流式脱敏状态机。
    redactor: StreamingRedactor,
    /// 至今已发布的 UTF-8 字节数。
    emitted_bytes: usize,
    /// 一旦发生错误契约冲突即封闭，后续调用不再接受新分片。
    failed: bool,
}

impl StreamingRedactionSink {
    /// 用一份已验证的 profile 和发布通道创建 sink。
    ///
    /// profile 校验失败时返回其稳定错误码，不会退化成无 profile 的普通脱敏。
    pub fn new(profile: RedactionProfile, channel: SecretScanChannel) -> Result<Self, String> {
        Self::from_redactor(StreamingRedactor::new(), profile, channel)
    }

    /// 用已有状态机创建 sink，便于调用方复用同一条流的基础脱敏状态。
    pub fn from_redactor(
        redactor: StreamingRedactor,
        profile: RedactionProfile,
        channel: SecretScanChannel,
    ) -> Result<Self, String> {
        profile.validate()?;
        Ok(Self {
            profile,
            channel,
            redactor,
            emitted_bytes: 0,
            failed: false,
        })
    }

    /// 用该通道的内置 profile 创建 sink；内置 profile 始终有效。
    pub fn for_channel(channel: SecretScanChannel) -> Self {
        Self::new(RedactionProfile::for_channel(channel), channel)
            .expect("built-in channel redaction profile is valid")
    }

    /// 返回治理该流的 profile 摘要。
    pub fn profile_digest(&self) -> &str {
        &self.profile.profile_digest
    }

    /// 返回该流的发布通道标签。
    pub fn channel(&self) -> SecretScanChannel {
        self.channel
    }

    /// 返回至今已发布的 UTF-8 字节数。
    pub fn emitted_bytes(&self) -> usize {
        self.emitted_bytes
    }

    /// 是否已封闭（发生过错误或状态越界）。
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// 当前是否仍在等待一个跨分片的敏感值结束。
    pub fn has_pending_secret(&self) -> bool {
        self.redactor.has_pending_secret()
    }

    /// 处理一个新的文本分片并返回可以安全发布的前缀。
    ///
    /// 分片只经过本文件的有界状态机；状态机缓冲越界或计数溢出属于内部契约冲突，会让 sink 进入
    /// 封闭状态并返回错误码，之后不再接受新分片，也不会把原文返回给调用方。
    pub fn push(&mut self, chunk: &str) -> Result<String, String> {
        if self.failed {
            return Err("redaction_stream_failed".to_owned());
        }
        let text = self.redactor.push(chunk);
        if self.redactor.buffered_len() > STREAM_REDACTION_BUFFER_LIMIT {
            self.failed = true;
            return Err("redaction_stream_state_unbounded".to_owned());
        }
        match self.emitted_bytes.checked_add(text.len()) {
            Some(total) => self.emitted_bytes = total,
            None => {
                self.failed = true;
                return Err("redaction_stream_bytes_overflow".to_owned());
            }
        }
        Ok(text)
    }

    /// 结束流并返回尾部安全文本、累计字节数和 profile 摘要。
    ///
    /// 已封闭的 sink 返回 `redaction_stream_failed`，不重放缓存内容；重复调用会返回空的
    /// `tail`（底层状态机只刷新一次）。
    pub fn finish(&mut self) -> Result<BoundedStreamTail, String> {
        if self.failed {
            return Err("redaction_stream_failed".to_owned());
        }
        let tail = self.redactor.finish();
        match self.emitted_bytes.checked_add(tail.len()) {
            Some(total) => self.emitted_bytes = total,
            None => {
                self.failed = true;
                return Err("redaction_stream_bytes_overflow".to_owned());
            }
        }
        Ok(BoundedStreamTail {
            tail,
            emitted_bytes: self.emitted_bytes,
            profile_digest: self.profile.profile_digest.clone(),
        })
    }
}

/// 找到文本中最早出现的 marker；同一位置有多个候选时优先选择较长者。
///
/// 例如完整的 `authorization: bearer ` 应覆盖较短的 `bearer ` 子串，从而让 marker 边界与后续
/// 敏感值识别保持一致。返回位置按 UTF-8 字节偏移计算，供 `push()` 与字符串切片共同使用。
fn earliest_marker(text: &str) -> Option<(usize, SensitiveMarker)> {
    let lower = text.to_ascii_lowercase();
    SENSITIVE_MARKERS
        .iter()
        .filter_map(|marker| lower.find(marker.literal).map(|start| (start, *marker)))
        .min_by(|(left_start, left), (right_start, right)| {
            left_start
                .cmp(right_start)
                .then_with(|| right.literal.len().cmp(&left.literal.len()))
        })
}

fn earliest_candidate(text: &str) -> Option<(usize, CandidateStart)> {
    let url = text.find("://").and_then(|scheme_start| {
        let start = scheme_start + 3;
        (start < text.len() && !text[start..].starts_with('/')).then_some((
            start,
            CandidateStart {
                kind: CandidateKind::UrlUserinfo,
            },
        ))
    });
    let jwt = text.match_indices("eyJ").find_map(|(start, _)| {
        let boundary = start == 0
            || text[..start].chars().next_back().is_some_and(|character| {
                character.is_whitespace()
                    || matches!(character, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']')
            });
        boundary.then_some((
            start,
            CandidateStart {
                kind: CandidateKind::Jwt,
            },
        ))
    });
    match (url, jwt) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(candidate), None) | (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

fn partial_jwt_start(text: &str, overlap_len: usize) -> Option<usize> {
    for prefix in ["ey", "e"] {
        let Some(start) = text.len().checked_sub(prefix.len()) else {
            continue;
        };
        if start < overlap_len || !text[start..].eq_ignore_ascii_case(prefix) {
            continue;
        }
        let boundary = start == 0
            || text[..start].chars().next_back().is_some_and(|character| {
                character.is_whitespace()
                    || matches!(character, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']')
            });
        if boundary {
            return Some(start);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        encode_bounded_text_for, encode_bounded_value_for, redact_text, scan_secret_value,
        scan_secret_value_with_limit, RedactionProfile, RedactionSignal, SecretScanChannel,
        StreamingRedactionSink, StreamingRedactor, MAX_SECRET_SENTINEL_SCAN_BYTES,
        STREAM_REDACTION_BUFFER_LIMIT,
    };
    use serde_json::json;

    #[test]
    fn streaming_redactor_masks_marker_and_value_split_across_chunks() {
        let mut redactor = StreamingRedactor::new();
        let mut output = String::new();
        output.push_str(&redactor.push("Authorization: Bear"));
        output.push_str(&redactor.push("er stream-split-sentinel"));
        output.push_str(&redactor.finish());

        assert_eq!(output, "Authorization: Bearer [REDACTED]");
        assert!(!output.contains("stream-split-sentinel"), "{output}");
    }

    #[test]
    fn streaming_redactor_masks_value_split_across_chunks() {
        let mut redactor = StreamingRedactor::new();
        let mut output = String::new();
        output.push_str(&redactor.push("token=first"));
        output.push_str(&redactor.push("second, visible"));
        output.push_str(&redactor.finish());

        assert_eq!(output, "token=[REDACTED], visible");
    }

    #[test]
    fn streaming_redactor_skips_marker_whitespace_across_chunks() {
        let mut redactor = StreamingRedactor::new();
        let mut output = String::new();
        output.push_str(&redactor.push("X-Api-Key:"));
        output.push_str(&redactor.push("  stream-split-sentinel"));
        output.push_str(&redactor.finish());

        assert_eq!(output, "X-Api-Key:  [REDACTED]");
        assert!(!output.contains("stream-split-sentinel"), "{output}");
    }

    #[test]
    fn streaming_redactor_emits_ordinary_text_immediately_and_stays_bounded() {
        let mut redactor = StreamingRedactor::new();
        let output = redactor.push("ordinary text Authorization: Bear");

        assert_eq!(output, "ordinary text Authorization: Bear");
        assert!(redactor.buffered_len() <= STREAM_REDACTION_BUFFER_LIMIT);
        assert!(redactor.finish().is_empty());
    }

    #[test]
    fn complete_redaction_and_streaming_redaction_agree_on_split_secret() {
        let text = "Authorization: Bearer stream-split-sentinel";
        let mut redactor = StreamingRedactor::new();
        let mut streamed = String::new();
        streamed.push_str(&redactor.push("Authorization: Bear"));
        streamed.push_str(&redactor.push("er stream-split-sentinel"));
        streamed.push_str(&redactor.finish());

        assert_eq!(streamed, redact_text(text));
    }

    #[test]
    fn streaming_redactor_masks_url_userinfo_split_across_chunks() {
        let mut redactor = StreamingRedactor::new();
        let mut output = String::new();
        output.push_str(&redactor.push("fetch https://stream-user:stream-pass"));
        output.push_str(&redactor.push("@example.test/path"));
        output.push_str(&redactor.finish());

        assert!(!output.contains("stream-user"), "{output}");
        assert!(!output.contains("stream-pass"), "{output}");
        assert_eq!(output, "fetch https://[REDACTED]@example.test/path");
    }

    #[test]
    fn streaming_redactor_masks_jwt_split_across_chunks() {
        let mut redactor = StreamingRedactor::new();
        let mut output = String::new();
        output.push_str(&redactor.push("response eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ"));
        output.push_str(&redactor.push(".signature123 tail"));
        output.push_str(&redactor.finish());

        assert!(!output.contains("eyJhbGci"), "{output}");
        assert!(!output.contains("signature123"), "{output}");
        assert_eq!(output, "response [REDACTED] tail");
    }

    #[test]
    fn streaming_redactor_masks_jwt_prefix_split_across_chunks() {
        let mut redactor = StreamingRedactor::new();
        let mut output = String::new();
        output.push_str(&redactor.push("response e"));
        output.push_str(&redactor.push("yJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.signature123 tail"));
        output.push_str(&redactor.finish());

        assert_eq!(output, "response [REDACTED] tail");
    }

    #[test]
    fn streaming_redactor_bounds_deferred_candidate_and_fails_closed() {
        let mut redactor = StreamingRedactor::new();
        let mut output = redactor.push(&format!("response eyJ{}", "A".repeat(128)));
        assert!(redactor.buffered_len() <= STREAM_REDACTION_BUFFER_LIMIT);
        output.push_str(&redactor.finish());

        assert_eq!(output, "response [REDACTED]");
    }

    #[test]
    fn structured_event_scan_allows_bounded_large_trees_but_not_large_text_nodes() {
        let value = json!({
            "items": (0..70_000).map(|_| "ok").collect::<Vec<_>>(),
        });
        assert!(scan_secret_value(SecretScanChannel::Event, &value).is_err());
        scan_secret_value_with_limit(SecretScanChannel::Event, &value, 1024 * 1024)
            .expect("aggregate event payload is within its higher protocol bound");

        let large_text = json!("x".repeat(MAX_SECRET_SENTINEL_SCAN_BYTES + 1));
        assert!(
            scan_secret_value_with_limit(SecretScanChannel::Event, &large_text, 1024 * 1024)
                .is_err()
        );
    }

    #[test]
    fn scanner_accepts_redacted_markers_inside_json_arrays() {
        let value = json!({
            "arguments": ["api_key=[REDACTED]", "Authorization: Bearer [REDACTED]"]
        });
        scan_secret_value(SecretScanChannel::Event, &value)
            .expect("complete placeholders remain safe at JSON array boundaries");
    }

    #[test]
    fn scanner_accepts_redacted_markers_inside_escaped_json_summaries() {
        let value = json!({
            "summary": r#"{\"stdout\":\"api_key=[REDACTED]\\n\"}"#,
        });
        scan_secret_value(SecretScanChannel::Event, &value)
            .expect("escaped JSON summaries preserve the redacted placeholder boundary");
    }

    #[test]
    fn complete_redaction_closes_a_dangling_stream_marker() {
        assert_eq!(
            redact_text("Authorization: Bearer "),
            "Authorization: Bearer [REDACTED]"
        );
    }

    #[test]
    fn channel_profiles_bind_signal_and_share_digest_across_text_channels() {
        let prompt = RedactionProfile::for_channel(SecretScanChannel::Prompt);
        assert_eq!(prompt.signal, RedactionSignal::Log);
        assert_eq!(
            prompt.profile_digest,
            RedactionProfile::for_signal(RedactionSignal::Log).profile_digest
        );
        for channel in [
            SecretScanChannel::Transcript,
            SecretScanChannel::Stdout,
            SecretScanChannel::Stderr,
            SecretScanChannel::Argv,
            SecretScanChannel::Env,
            SecretScanChannel::Cache,
        ] {
            assert_eq!(
                RedactionProfile::for_channel(channel).profile_digest,
                prompt.profile_digest,
                "{channel:?}"
            );
        }
        let receipt = RedactionProfile::for_channel(SecretScanChannel::Receipt);
        assert_eq!(receipt.signal, RedactionSignal::Audit);
        assert_eq!(
            receipt.profile_digest,
            RedactionProfile::for_channel(SecretScanChannel::Event).profile_digest
        );
    }

    #[test]
    fn channel_aware_encoders_block_residual_secrets_without_raw_fallback() {
        let profile = RedactionProfile::for_channel(SecretScanChannel::Env);
        let error = encode_bounded_text_for(&profile, SecretScanChannel::Env, "prefix sk-oa03chan")
            .unwrap_err();
        assert_eq!(error, "redaction_secret_sentinel_detected");
        // The value is redacted rather than returned raw, so the encoder succeeds with a placeholder.
        let secret = encode_bounded_value_for(
            &profile,
            SecretScanChannel::Prompt,
            &json!({"api_key": "oa03-prompt-sentinel"}),
        )
        .unwrap();
        assert_eq!(
            secret.value["api_key"],
            serde_json::Value::String("[REDACTED]".to_owned())
        );
        let raw = json!({"authorization": "Bearer oa03-raw-sentinel"});
        let encoded = encode_bounded_value_for(&profile, SecretScanChannel::Prompt, &raw).unwrap();
        assert!(!encoded.value.to_string().contains("oa03-raw-sentinel"));
        assert_eq!(encoded.profile_digest, profile.profile_digest);
    }

    #[test]
    fn streaming_sink_binds_profile_and_masks_split_markers() {
        let mut sink = StreamingRedactionSink::for_channel(SecretScanChannel::Stderr);
        let mut output = String::new();
        output.push_str(&sink.push("Authorization: Bear").unwrap());
        output.push_str(&sink.push("er oa03-sink-sentinel").unwrap());
        let tail = sink.finish().unwrap();
        output.push_str(&tail.tail);

        assert_eq!(output, "Authorization: Bearer [REDACTED]");
        assert!(!output.contains("oa03-sink-sentinel"), "{output}");
        assert_eq!(
            tail.profile_digest,
            RedactionProfile::for_channel(SecretScanChannel::Stderr).profile_digest
        );
        assert_eq!(tail.emitted_bytes, output.len());
        assert!(!sink.failed());
        assert!(!sink.has_pending_secret());
        assert_eq!(sink.channel(), SecretScanChannel::Stderr);
    }

    #[test]
    fn streaming_sink_rejects_invalid_profile_and_never_reopens_after_finish() {
        let mut forged = RedactionProfile::for_signal(RedactionSignal::Log);
        forged.max_bytes = forged.max_bytes.saturating_sub(1);
        assert_eq!(
            StreamingRedactionSink::new(forged, SecretScanChannel::Transcript).unwrap_err(),
            "redaction_profile_digest_mismatch"
        );

        let mut sink = StreamingRedactionSink::for_channel(SecretScanChannel::Stdout);
        assert!(sink.finish().unwrap().tail.is_empty());
        // After finish, new pushes run as independent complete text; a marker plus value is closed
        // with the same placeholder rather than emitted raw.
        assert_eq!(
            sink.push("token=oa03-after-finish").unwrap(),
            "token=[REDACTED]"
        );
        assert!(!sink.failed());
    }
}
