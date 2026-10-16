//! # 文件职责
//!
//! 本文件定义“把原始文本整理成可用于后续检索/模型上下文的带版本结果”这一领域转换：校验转换策略，
//! 规范化少数明确列出的字符形式，按策略拒绝/脱敏/不保留敏感输入，并为结果、索引词元（token）和元数据生成摘要。
//! 这里不执行网络向量嵌入（embedding）、不调用大语言模型（Large Language Model，LLM）、不读写数据库，
//! 也不负责决定文本能否进入某个业务索引。
//!
//! # 在系统中的位置与当前接入情况
//!
//! ```text
//! 调用方提供 TextNormalizationProfile + 原始 &str
//!        ↓
//! 【本文件：校验策略 → 规范化 → 敏感标记处理 → 生成词元 / 元数据 / 摘要】
//!        ↓
//! NormalizedText（包含可选文本和校验摘要的领域值）
//!        ↓
//! 设计上供检索、索引或模型上下文相关流程使用
//! ```
//!
//! `kiana-domain/src/lib.rs` 将这些公共类型重新导出，协议 crate 也重导出部分 schema 常量；当前全仓源码检索
//! 找到的是本文件的领域测试在调用 `NormalizedText::normalize()`，尚未找到生产路径调用它。因此本文件下面
//! 描述的是可调用的领域契约与实际转换语义，不能据此推断它已经过滤了生产请求、保护了某个已接通的索引，
//! 或已经向外部 embedding/LLM 发送数据。
//!
//! # 相关模块与数据来源
//!
//! - `memory::memory_tokens` 提供通用 Unicode/CJK 分词；本文件额外拆分常见 ASCII 标识符片段。
//! - `redaction::redact_text` 对命中敏感规则的文本尝试脱敏；`redact_pii` 再处理空白分隔的邮箱/长数字片段。
//! - `contracts::json_digest` 对 JSON 结构计算确定性摘要；`journal::journal_sha256` 对原始/规范化字节计算摘要。
//! - 调用方负责提供 profile 和文本，并负责决定返回的 `NormalizedText` 后续交给谁。
//!
//! 统一码（Unicode）策略故意保持有限且显式：只去掉字符串开头的 BOM、统一 CRLF/CR 行尾，并折叠常见全角 ASCII；
//! 这不是完整的 NFKC（Unicode 兼容分解与组合规范化），也不会把所有视觉相似字符转换成同一形式。敏感信息处理是基于当前启发式规则，
//! 不是对任意凭证格式的完整证明；调用者不能把 `secret_free` 标记单独当成安全认证。

use crate::{
    journal_sha256, json_digest, memory_tokens, redact_text, redaction::contains_json_secret_key,
    SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// profile、结果以及两类下游元数据各自使用的 wire（线上/跨模块数据）schema（数据结构标识）。
///
/// 即使它们属于同一条规范化流程，也分开版本化：调用者可以判断收到的是哪种记录，
/// 而修改 profile、规范化结果或元数据的字段形状时不必假装它们是同一种对象。
pub const TEXT_NORMALIZATION_PROFILE_SCHEMA: &str = "kiana.text-normalization-profile.v1";
pub const NORMALIZED_TEXT_SCHEMA: &str = "kiana.normalized-text.v1";
pub const EMBEDDING_METADATA_SCHEMA: &str = "kiana.embedding-metadata.v1";
pub const LLM_TEXT_METADATA_SCHEMA: &str = "kiana.llm-text-metadata.v1";
/// 当前实现支持的固定规范化版本；目前只有 `1.0`。
pub const TEXT_NORMALIZATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 原始输入和输出文本的最大字节数（256 KiB）；这里按 UTF-8 字节数限制，而非字符数。
pub const MAX_NORMALIZED_BYTES: usize = 256 * 1024;
/// 规范化后允许保留的 token 数上限，用于限制后续索引输入规模。
pub const MAX_NORMALIZED_TOKENS: usize = 16_384;

/// 检查配置/元数据中的必填文本字段是否非空、未超长且不含 NUL 字节。
///
/// 这是结构形状校验，不会验证字段对应的业务对象是否存在，也不会净化或修改传入字符串。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 校验一个带 `sha256:` 前缀的摘要字符串是否具有固定格式。
///
/// 本函数只检查摘要文本长 64 位且由十六进制字符组成；它没有原始数据，不能确认摘要内容正确。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 命中敏感内容时的处理策略。策略由 profile 提供，`normalize()` 不会自行替调用方选择。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveHandling {
    /// 发现当前规则识别的敏感输入时直接返回错误，不产出结果。
    Reject,
    /// 尝试移除当前规则识别的凭证和个人身份信息（Personally Identifiable Information，PII），再保留处理后的文本。
    Redact,
    /// 不保留文本或 token，只返回以摘要为基础的引用型元数据。
    ReferenceOnly,
}

/// 本次结果对敏感输入采取的处理结果；它记录实际分支，不是权限或安全认证结论。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveDisposition {
    /// 当前启发式检查没有命中敏感标记。
    None,
    /// 检查命中后执行了文本脱敏分支。
    Redacted,
    /// 检查命中后不在结果中保留原文。
    ReferenceOnly,
}

/// 一次规范化转换所使用的显式策略快照。
///
/// profile 由调用方传入 `normalize()`；其策略版本、字符处理策略和资源上限都参与摘要，
/// 因而结果可以记录自己是按哪份策略生成的。`serde` 的派生实现支持 JSON 等格式的读写；
/// `deny_unknown_fields` 会让反序列化拒绝未知字段，避免把新旧 profile 的形状悄悄混用。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextNormalizationProfile {
    /// 预期固定为 `TEXT_NORMALIZATION_PROFILE_SCHEMA`。
    pub schema: String,
    /// 与本文件支持的 `TEXT_NORMALIZATION_VERSION` 精确匹配的版本。
    pub version: SchemaVersion,
    /// Unicode 处理策略标签；当前必须为 `nfkc-lite-fullwidth-v1`，但实际处理范围见 `canonical_text()`。
    pub unicode_policy: String,
    /// 换行与 BOM 策略标签；当前必须为 `lf-bom-v1`。
    pub newline_policy: String,
    /// 标识符拆分策略标签；当前必须为 `camel-snake-cjk-v1`。
    pub identifier_policy: String,
    /// 检出敏感内容后采用的拒绝、脱敏或仅留引用策略。
    pub sensitive_handling: SensitiveHandling,
    /// 本次允许的原始文本字节上限；校验时不能超过模块硬上限。
    pub max_bytes: u64,
    /// 本次允许保留的 token 数上限；校验时不能超过模块硬上限。
    pub max_tokens: u32,
    /// 除本字段自身外整个 profile 内容的确定性摘要，用于发现策略字段变化。
    pub profile_digest: String,
}

impl TextNormalizationProfile {
    /// 按当前固定策略构造 profile，仅让调用方选择敏感内容处理方式。
    ///
    /// 【调用者与数据流】调用者通常先创建 profile，再把它和待处理字符串交给 `NormalizedText::normalize()`；
    /// 本构造器不接收文本，也不执行规范化。
    ///
    /// 【核心流程】填入当前 schema/version/策略标签和模块允许的最大字节/token 数 → 计算 profile 摘要
    /// → 调用 `validate()` 自检。摘要不含自身的当前值，避免计算结果被它自己递归改变。
    ///
    /// 【失败】当前内置字段应当通过校验；若未来修改常量或摘要规则导致自检失败，则返回具体校验错误。
    pub fn new(sensitive_handling: SensitiveHandling) -> Result<Self, String> {
        let mut profile = Self {
            schema: TEXT_NORMALIZATION_PROFILE_SCHEMA.to_owned(),
            version: TEXT_NORMALIZATION_VERSION,
            unicode_policy: "nfkc-lite-fullwidth-v1".to_owned(),
            newline_policy: "lf-bom-v1".to_owned(),
            identifier_policy: "camel-snake-cjk-v1".to_owned(),
            sensitive_handling,
            max_bytes: MAX_NORMALIZED_BYTES as u64,
            max_tokens: MAX_NORMALIZED_TOKENS as u32,
            profile_digest: String::new(),
        };
        profile.profile_digest = profile.digest();
        profile.validate()?;
        Ok(profile)
    }

    /// 检查 profile 是否恰好属于本实现支持的版本、策略和上限，并重新计算摘要核对内容。
    ///
    /// 先拒绝任何不支持的 schema/version/策略标签和越界限制，再检查摘要格式，最后比较摘要值。
    /// 这使调用方不能仅靠改一个字段而继续沿用旧摘要；但摘要不带密钥，不能证明 profile 来自可信签发者。
    /// `normalize()` 在消费 profile 前会调用本方法，因此失败会阻止后续文本处理。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TEXT_NORMALIZATION_PROFILE_SCHEMA
            || self.version != TEXT_NORMALIZATION_VERSION
            || self.unicode_policy != "nfkc-lite-fullwidth-v1"
            || self.newline_policy != "lf-bom-v1"
            || self.identifier_policy != "camel-snake-cjk-v1"
            || self.max_bytes == 0
            || self.max_bytes > MAX_NORMALIZED_BYTES as u64
            || self.max_tokens == 0
            || self.max_tokens as usize > MAX_NORMALIZED_TOKENS
        {
            return Err("text_normalization_profile_invalid".to_owned());
        }
        digest(&self.profile_digest, "text_normalization_profile_digest")?;
        if self.profile_digest != self.digest() {
            return Err("text_normalization_profile_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 profile 字段计算确定性摘要。
    ///
    /// 先把结构序列化为 JSON，再把 `profile_digest` 暂时替换为空字符串，最后交给 `json_digest()`。
    /// 如果 serde 转换意外失败，当前实现把值退化为 JSON `null` 后仍计算摘要；该兜底不等于序列化成功。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "profile_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

/// 默认采用脱敏策略创建内置 profile。
///
/// `expect` 表明默认 profile 是程序员维护的常量组合；若代码改动使它不再通过自检，构造会 panic，
/// 而不是把无效默认配置交给每个调用者继续使用。
impl Default for TextNormalizationProfile {
    fn default() -> Self {
        Self::new(SensitiveHandling::Redact).expect("built-in normalization profile is valid")
    }
}

/// 为后续 embedding/token 索引保留的输入摘要和计数元数据。
///
/// 当前 `model_id` 固定为 `kiana.tokens.no-network.v1`，实际记录的是本地 token 化输入的摘要与数量，
/// 不包含向量，也不发起任何 embedding 网络请求。`secret_free` 是由构造器写入的布尔声明；
/// 它本身不是脱敏证明，结果级文本检查仍由 `NormalizedText::validate()` 执行。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingMetadata {
    /// 元数据形状版本。
    pub schema: String,
    /// 当前固定的无网络 token 处理器标识，不是远程模型名称。
    pub model_id: String,
    /// 规范化文本的摘要；ReferenceOnly 情况下是原文摘要文本再次计算得到的摘要。
    pub input_digest: String,
    /// `normalize()` 从最终保留文本生成的 token 数量；ReferenceOnly 构造结果为 0。
    pub token_count: u32,
    /// 当前构造器恒写 `true`，仅为契约字段，不独立扫描或证明输入无密钥。
    pub secret_free: bool,
    /// 本对象除自身摘要字段外的 JSON 内容摘要。
    pub metadata_digest: String,
}

impl EmbeddingMetadata {
    /// 由规范化摘要和 token 数构造当前版本的本地索引元数据。
    fn new(input_digest: String, token_count: u32) -> Self {
        let mut metadata = Self {
            schema: EMBEDDING_METADATA_SCHEMA.to_owned(),
            model_id: "kiana.tokens.no-network.v1".to_owned(),
            input_digest,
            token_count,
            secret_free: true,
            metadata_digest: String::new(),
        };
        metadata.metadata_digest = metadata.digest();
        metadata
    }

    /// 校验固定 schema/model、secret_free 标志、数量上限和两个摘要字段的格式/内容。
    ///
    /// 失败返回稳定字符串错误码。这个方法只校验本对象，不读取文本，也不检查 `input_digest` 是否
    /// 与外层 `NormalizedText::normalized_digest` 相等；外层调用者若需要该绑定必须另行比较。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EMBEDDING_METADATA_SCHEMA
            || self.model_id != "kiana.tokens.no-network.v1"
            || !self.secret_free
            || self.token_count as usize > MAX_NORMALIZED_TOKENS
        {
            return Err("embedding_metadata_invalid".to_owned());
        }
        digest(&self.input_digest, "embedding_input_digest")?;
        digest(&self.metadata_digest, "embedding_metadata_digest")?;
        if self.metadata_digest != self.digest() {
            return Err("embedding_metadata_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算 embedding 元数据自身的摘要；不把 `metadata_digest` 字段纳入输入。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "model_id": self.model_id,
            "input_digest": self.input_digest,
            "token_count": self.token_count,
            "secret_free": self.secret_free,
        }))
    }
}

/// 为模型上下文保留的文本来源与处理后内容元数据。
///
/// 这里存的是摘要、语言提示和处理策略摘要，不存原文。`source_digest` 仍可用于比对输入是否相同，
/// 因为它是原始字节的哈希，但哈希不是加密，也不能替代访问控制。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmTextMetadata {
    /// 元数据形状版本。
    pub schema: String,
    /// 规范化前原始 UTF-8 字节的摘要。
    pub source_digest: String,
    /// 结果中保留文本的摘要；ReferenceOnly 分支使用原始摘要字符串作为再次哈希的输入。
    pub normalized_digest: String,
    /// 根据最终文本中字符范围生成的粗粒度语言类别，不是语言识别模型的结论。
    pub language: String,
    /// 发生拒绝以外的敏感处理时记录所用 profile 摘要；没有命中敏感内容时为 `None`。
    #[serde(default)]
    pub redaction_profile_digest: Option<String>,
    /// 当前构造器恒写 `true` 的契约声明，不是独立的秘密扫描结果。
    pub secret_free: bool,
    /// 本对象除自身摘要字段外的 JSON 内容摘要。
    pub metadata_digest: String,
}

impl LlmTextMetadata {
    /// 用来源、规范化结果、语言提示和可选策略摘要构造模型侧元数据。
    fn new(
        source_digest: String,
        normalized_digest: String,
        language: String,
        redaction_profile_digest: Option<String>,
    ) -> Self {
        let mut metadata = Self {
            schema: LLM_TEXT_METADATA_SCHEMA.to_owned(),
            source_digest,
            normalized_digest,
            language,
            redaction_profile_digest,
            secret_free: true,
            metadata_digest: String::new(),
        };
        metadata.metadata_digest = metadata.digest();
        metadata
    }

    /// 校验元数据形状、摘要格式以及元数据自身摘要。
    ///
    /// `redaction_profile_digest = None` 允许表示没有执行拒绝以外的敏感处理；本方法没有 profile 实例，
    /// 因而不能重跑规范化或证明原文与规范化文本之间的转换正确。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LLM_TEXT_METADATA_SCHEMA || !self.secret_free {
            return Err("llm_text_metadata_invalid".to_owned());
        }
        digest(&self.source_digest, "llm_source_digest")?;
        digest(&self.normalized_digest, "llm_normalized_digest")?;
        required(&self.language, "llm_language", 32)?;
        if let Some(profile) = &self.redaction_profile_digest {
            digest(profile, "llm_redaction_profile_digest")?;
        }
        digest(&self.metadata_digest, "llm_metadata_digest")?;
        if self.metadata_digest != self.digest() {
            return Err("llm_text_metadata_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算模型侧元数据自身的摘要；不把 `metadata_digest` 字段纳入输入。
    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source_digest": self.source_digest,
            "normalized_digest": self.normalized_digest,
            "language": self.language,
            "redaction_profile_digest": self.redaction_profile_digest,
            "secret_free": self.secret_free,
        }))
    }
}

/// `normalize()` 的完整产物：可选文本、检索 token、策略/语言信息和可核对摘要。
///
/// 调用方通常应把它视为一个不可拆散的结果对象；若只保留 `text` 而丢掉 disposition/profile/digest，
/// 下游就无法知道内容是否经过敏感处理、采用了哪份策略以及摘要对应什么输入。
/// `Deserialize` 可从外部格式重建对象，但重建本身不自动验证内容；消费不可信数据前需要显式调用 `validate()`。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedText {
    /// 结果对象的 schema 标识。
    pub schema: String,
    /// 生成结果所用的规范化算法版本。
    pub version: SchemaVersion,
    /// 原始输入字节摘要，用于来源比对；不保存原始字节。
    pub source_digest: String,
    /// `text` 存在时对应最终文本的摘要；引用模式按实现规则从 source digest 派生。
    pub normalized_digest: String,
    /// 脱敏后或原样规范化的文本；ReferenceOnly 时为 `None`，不会把敏感原文放进结果。
    #[serde(default)]
    pub text: Option<String>,
    /// `normalize()` 从最终保留文本生成的搜索词元；ReferenceOnly 构造结果为空。
    pub tokens: Vec<String>,
    /// 依据最终文本字符范围得出的 `ascii`、`unicode` 或 `cjk` 提示。
    pub language: String,
    /// 记录当前结果走的是无命中、脱敏还是仅引用分支。
    pub disposition: SensitiveDisposition,
    /// 生成结果所用 profile 的摘要，供结果与策略版本关联。
    pub profile_digest: String,
    /// 与本地 token 输入关联的摘要和数量元数据。
    pub embedding: EmbeddingMetadata,
    /// 与模型上下文来源/结果关联的摘要元数据。
    pub llm: LlmTextMetadata,
    /// 除本字段自身外整个结果对象的摘要。
    pub result_digest: String,
}

impl NormalizedText {
    /// 按 profile 把调用方给出的原始文本转换为 `NormalizedText`。
    ///
    /// 【作用与当前调用者】这是领域层纯计算入口，接收 profile 和 UTF-8 文本，返回可校验的规范化结果。
    /// 当前全仓只找到 `kiana-domain/tests/cm09_text_normalization.rs` 的测试调用，尚未找到生产调用点；
    /// 它不会自行获取文件、事件、用户输入或远程内容。
    ///
    /// 【核心数据流】
    /// 1. 校验 profile，并按原始 UTF-8 字节数执行 profile 上限；
    /// 2. 对原文计算 `source_digest`，再去 BOM、统一行尾并转换有限范围的全角 ASCII；
    /// 3. 用本模块启发式规则检查敏感标记，按 Reject/Redact/ReferenceOnly 决定失败、脱敏或丢弃文本；
    /// 4. 对最终可保留文本生成 identifier/CJK tokens，并执行 token 数上限；
    /// 5. 生成语言提示、embedding/LLM 元数据和结果摘要，最后调用 `validate()` 自检。
    ///
    /// 【输出与副作用】成功返回一个值对象；函数不写数据库、不记录事件、不发网络请求，也不改变输入字符串。
    /// ReferenceOnly 的 `normalized_digest` 不是对明文规范化文本的摘要，而是对 `source_digest` 文本再哈希，
    /// 以便结果不携带明文；该摘要仍会稳定地区分不同来源输入。
    ///
    /// 【失败情况】profile 无效、原文超过字节上限、Reject 策略命中敏感标记、token 超限或最终对象自检失败时
    /// 返回字符串错误码；这里不触发重试、禁用账号或其他系统状态变化。敏感识别规则有限，不能把未命中理解为
    /// 输入已被完整证明不含任何秘密。
    pub fn normalize(profile: &TextNormalizationProfile, source: &str) -> Result<Self, String> {
        profile.validate()?;
        if source.as_bytes().len() > profile.max_bytes as usize {
            return Err("text_normalization_input_too_large".to_owned());
        }
        let source_digest = digest_bytes(source.as_bytes());
        // 先在有限字符规则上得到统一文本，再做敏感识别；因此 source_digest 保留原始字节身份，
        // 而后面的 normalized_digest 对应规范化/脱敏后的值，两者用途不同。
        let canonical = canonical_text(source);
        let sensitive = contains_sensitive(&canonical);
        let (text, disposition) = if sensitive {
            match profile.sensitive_handling {
                // 拒绝发生在 token 和下游元数据生成之前；错误交由调用者决定如何反馈或记录。
                SensitiveHandling::Reject => {
                    return Err("normalization_sensitive_input_rejected".to_owned())
                }
                // 先按凭证文本规则处理，再把空白分隔的邮箱/长数字字段替换为统一占位符。
                SensitiveHandling::Redact => (
                    Some(redact_pii(&redact_text(&canonical))),
                    SensitiveDisposition::Redacted,
                ),
                // 不把原文放入结果；下方用 source digest 的文本形式派生摘要，且 token 列表保持为空。
                SensitiveHandling::ReferenceOnly => (None, SensitiveDisposition::ReferenceOnly),
            }
        } else {
            (Some(canonical), SensitiveDisposition::None)
        };
        let normalized_digest = digest_bytes(
            text.as_deref()
                .unwrap_or_else(|| source_digest.as_str())
                .as_bytes(),
        );
        let tokens = text.as_deref().map(identifier_tokens).unwrap_or_default();
        if tokens.len() > profile.max_tokens as usize {
            return Err("text_normalization_token_limit".to_owned());
        }
        let language = language_hint(text.as_deref().unwrap_or_default());
        let redaction_profile_digest =
            (disposition != SensitiveDisposition::None).then(|| profile.profile_digest.clone());
        let embedding = EmbeddingMetadata::new(normalized_digest.clone(), tokens.len() as u32);
        let llm = LlmTextMetadata::new(
            source_digest.clone(),
            normalized_digest.clone(),
            language.clone(),
            redaction_profile_digest,
        );
        let mut result = Self {
            schema: NORMALIZED_TEXT_SCHEMA.to_owned(),
            version: TEXT_NORMALIZATION_VERSION,
            source_digest,
            normalized_digest,
            text,
            tokens,
            language,
            disposition,
            profile_digest: profile.profile_digest.clone(),
            embedding,
            llm,
            result_digest: String::new(),
        };
        result.result_digest = result.digest();
        result.validate()?;
        Ok(result)
    }

    /// 检查一个已构造或反序列化的结果对象是否满足当前格式和摘要约束。
    ///
    /// 检查包括 schema/version、文本与 disposition 的空值关系、摘要格式/内容、文本大小与敏感规则、
    /// token 形状、语言字段，以及两类内嵌元数据的自校验。失败返回稳定字符串错误码。
    /// 本方法不会重放 `normalize()`，也没有原始 profile/原始文本可供比对；各字段摘要可以校验自洽，
    /// 但不能证明输入来源可信、检测器覆盖所有秘密，或 embedding 元数据与外层摘要字段彼此绑定。
    /// `normalize()` 成功返回前会调用它；反序列化的外部对象也应由消费方显式调用。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NORMALIZED_TEXT_SCHEMA
            || self.version != TEXT_NORMALIZATION_VERSION
            || self.tokens.len() > MAX_NORMALIZED_TOKENS
            || (self.disposition == SensitiveDisposition::ReferenceOnly && self.text.is_some())
            || (self.disposition != SensitiveDisposition::ReferenceOnly && self.text.is_none())
        {
            return Err("normalized_text_header_invalid".to_owned());
        }
        digest(&self.source_digest, "normalized_source_digest")?;
        digest(&self.normalized_digest, "normalized_digest")?;
        digest(&self.profile_digest, "normalized_profile_digest")?;
        if let Some(text) = &self.text {
            if text.as_bytes().len() > MAX_NORMALIZED_BYTES || contains_sensitive(text) {
                return Err("normalized_text_sensitive_or_oversize".to_owned());
            }
            if digest_bytes(text.as_bytes()) != self.normalized_digest {
                return Err("normalized_text_digest_mismatch".to_owned());
            }
        }
        if self
            .tokens
            .iter()
            .any(|token| token.trim().is_empty() || token.len() > 256)
        {
            return Err("normalized_text_tokens_invalid".to_owned());
        }
        required(&self.language, "normalized_language", 32)?;
        self.embedding.validate()?;
        self.llm.validate()?;
        digest(&self.result_digest, "normalized_result_digest")?;
        if self.result_digest != self.digest() {
            return Err("normalized_result_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对整个结果对象计算确定性摘要，先把 `result_digest` 置空以避免自我引用。
    ///
    /// 摘要覆盖 schema、文本、token、元数据及其他字段；它用于发现内容变化，不是带密钥的签名。
    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "result_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

/// 将原始文本变成当前 profile 所定义的有限规范形式。
///
/// 【转换顺序】只剥除字符串开头的一个 BOM；之后从左到右把 CRLF 作为一个换行、孤立 CR 也作为换行，
/// 其余字符传给 `fullwidth_ascii()`。普通 LF 保持不变。
///
/// 【为什么不是完整 Unicode NFKC】当前算法要稳定、范围可审阅，故只折叠明确列出的全角 ASCII 字符和全角空格；
/// 组合字符、兼容汉字、其他视觉相似字符不会被统一。改变这一规则会改变规范化文本、tokens 和摘要，必须视为
/// profile/version 语义变更，而非无行为影响的格式清理。
pub fn canonical_text(source: &str) -> String {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut output = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            output.push('\n');
        } else {
            output.push(fullwidth_ascii(character));
        }
    }
    output
}

/// 将全角 ASCII 标点/字母/数字和全角空格映射到对应半角字符，其余字符原样返回。
///
/// 这里使用显式 Unicode 码点范围而不是完整 NFKC 库，避免悄悄改变其他字符类别。
fn fullwidth_ascii(character: char) -> char {
    match character as u32 {
        0xff01..=0xff5e => char::from_u32(character as u32 - 0xfee0).unwrap_or(character),
        0x3000 => ' ',
        _ => character,
    }
}

/// 生成供检索使用的 token 列表：先添加通用 Unicode/CJK token，再拆出常见 ASCII 标识符片段。
///
/// ASCII 标识符在 `_`、非字母数字字符和大写字母边界处拆分；新增的小写片段追加在 `memory_tokens()`
/// 的结果之后，因此列表可能包含有意重叠的词项。上限由调用者 `normalize()` 在得到列表后统一检查。
fn identifier_tokens(text: &str) -> Vec<String> {
    let mut tokens = memory_tokens(text);
    let mut identifier = String::new();
    let flush = |identifier: &mut String, tokens: &mut Vec<String>| {
        if !identifier.is_empty() {
            tokens.push(identifier.to_ascii_lowercase());
            identifier.clear();
        }
    };
    for character in text.chars() {
        if character == '_' {
            flush(&mut identifier, &mut tokens);
        } else if character.is_ascii_alphanumeric() {
            if character.is_ascii_uppercase() && !identifier.is_empty() {
                flush(&mut identifier, &mut tokens);
            }
            identifier.push(character);
        } else {
            flush(&mut identifier, &mut tokens);
        }
    }
    flush(&mut identifier, &mut tokens);
    tokens
}

/// 根据最终文本中是否有汉字区字符、其他非 ASCII 字符，给出粗粒度语言标签。
///
/// 汉字区命中时优先返回 `cjk`；没有汉字但有非 ASCII 返回 `unicode`；纯 ASCII 或空文本返回 `ascii`。
/// 这只是可重复的字符范围提示，不负责检测具体自然语言，也不联网调用识别器。
fn language_hint(text: &str) -> String {
    let has_cjk = text.chars().any(|character| {
        matches!(character as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0x20000..=0x3134f)
    });
    let has_non_ascii = text.chars().any(|character| !character.is_ascii());
    match (has_cjk, has_non_ascii) {
        (true, _) => "cjk".to_owned(),
        (false, true) => "unicode".to_owned(),
        (false, false) => "ascii".to_owned(),
    }
}

/// 使用本模块当前的启发式规则判断文本是否包含需要特殊处理的敏感标记。
///
/// 先做 ASCII 小写副本，再检查部分凭证键/头部标记、特定形式的邮箱和包含至少 10 位数字的空白字段。
/// 对已经由 `[redacted]` 紧接着遮蔽的 marker 会跳过该次匹配。它不解析任意文本中的全部凭证格式，
/// 也不是通用 DLP（数据泄漏防护）扫描器；未命中仅表示这些规则没有发现目标形状。
fn contains_sensitive(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    if contains_json_secret_key(text) {
        return true;
    }
    if contains_malformed_json_secret_key(text, &lowered) {
        return true;
    }
    let markers = [
        "token=",
        "token:",
        "password=",
        "password:",
        "api_key=",
        "api_key:",
        "access_key=",
        "access_key:",
        "access_token=",
        "access_token:",
        "private_key=",
        "private_key:",
        "refresh_token=",
        "refresh_token:",
        "id_token=",
        "id_token:",
        "api-key=",
        "api-key:",
        "client_secret=",
        "client_secret:",
        "proxy-authorization:",
        "secret=",
        "secret:",
        "bearer ",
        "authorization: bearer ",
        "authorization: basic ",
        "x-api-key:",
        "-----begin ",
    ];
    markers
        .iter()
        .any(|marker| contains_unredacted_marker(&lowered, marker))
        || text.split_whitespace().any(looks_like_email_or_phone)
        || has_formatted_phone(text)
}

/// 针对“开头看起来像 JSON、但 JSON 解析失败”的文本扫描带引号的敏感键。
///
/// ⚠ 这个 helper 的范围比名称容易让人误解：若原文首字节不是 `{` 或 `[`，或原文是合法 JSON，
/// 它立即返回 `false`；只扫描其余 malformed JSON 中形如 `"token":` 的键名。前导空格会令首字节检查
/// 失败。合法 JSON 路径没有在此检查键名，而普通 marker 规则也不匹配带引号键名如 `"token":"value"`；
/// 因此这段代码不能被描述成完整的 JSON 凭证扫描。相关缺口只记录，注释任务不改变其实现。
fn contains_malformed_json_secret_key(original: &str, lowered: &str) -> bool {
    if !matches!(original.as_bytes().first(), Some(b'{') | Some(b'['))
        || serde_json::from_str::<serde_json::Value>(original).is_ok()
    {
        return false;
    }
    [
        "token",
        "password",
        "api_key",
        "api-key",
        "access_key",
        "access_token",
        "refresh_token",
        "id_token",
        "private_key",
        "secret",
        "client_secret",
        "proxy-authorization",
    ]
    .iter()
    .any(|key| {
        let quoted = format!("\"{key}\"");
        let mut offset = 0;
        while let Some(found) = lowered[offset..].find(&quoted) {
            let key_end = offset + found + quoted.len();
            if lowered[key_end..].trim_start().starts_with(':') {
                return true;
            }
            offset = key_end;
            if offset >= lowered.len() {
                break;
            }
        }
        false
    })
}

/// 搜索文本 marker，并允许它后面紧跟一个边界明确的 `[redacted]` 占位符。
///
/// 占位符后必须是文本结尾、空白或当前列出的分隔符之一，避免把 `[redacted]suffix` 误当成已脱敏值。
/// `search_from` 在每轮推进到当前匹配值之后，防止相同 marker 被无限重复匹配。
fn contains_unredacted_marker(text: &str, marker: &str) -> bool {
    let mut search_from = 0;
    while let Some(relative) = text[search_from..].find(marker) {
        let marker_start = search_from + relative;
        let value_start = marker_start + marker.len();
        let value = text[value_start..].trim_start();
        let redacted = value.strip_prefix("[redacted]").is_some_and(|suffix| {
            suffix.is_empty()
                || suffix.chars().next().is_some_and(|character| {
                    character.is_whitespace() || ",;)]}".contains(character)
                })
        });
        if !redacted {
            return true;
        }
        search_from = value_start + value.len().saturating_sub(value.trim_start().len());
        if search_from <= marker_start {
            search_from = value_start.saturating_add("[redacted]".len());
        }
    }
    false
}

/// 粗略识别一个空白分隔字段是否像邮箱地址或长数字串。
///
/// 邮箱判定只要求同时含 `@` 和位于末段的点；电话号码判定只数 ASCII 数字且阈值为 10。
/// 这类形状检查可能误报或漏报，所以这里只供启发式遮蔽流程使用。
fn looks_like_email_or_phone(value: &str) -> bool {
    let email = value.contains('@') && value.rsplit_once('.').is_some();
    let digits = value
        .chars()
        .filter(|character| character.is_ascii_digit())
        .count();
    email || digits >= 10
}

/// Detect a conservative phone-shaped run split into three or four digit groups.
///
/// Requiring bounded all-digit groups avoids treating arbitrary prose as a phone number while
/// covering common forms such as `138 0013 8000`. This remains heuristic PII detection, not a
/// locale-aware telephone parser.
fn formatted_phone_run(tokens: &[&str], start: usize) -> Option<usize> {
    let first = *tokens.get(start)?;
    if !(3..=4).contains(&first.len()) || !first.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut index = start;
    let mut groups = 0;
    let mut digits = 0;
    while index < tokens.len() && groups < 4 {
        let token = tokens[index];
        if !(2..=4).contains(&token.len()) || !token.bytes().all(|byte| byte.is_ascii_digit()) {
            break;
        }
        groups += 1;
        digits += token.len();
        index += 1;
    }
    (groups >= 3 && (10..=15).contains(&digits)).then_some(index)
}

fn has_formatted_phone(text: &str) -> bool {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    (0..tokens.len()).any(|start| formatted_phone_run(&tokens, start).is_some())
}

/// 按空白切分文本，把形状像邮箱或长数字串的整个片段替换为统一 PII 占位符。
///
/// 因为按 `split_whitespace()` 重组，连续空白会变成单个空格；其他片段保持原样。这是简单的第二道处理，
/// 不解析句子或带空格的地址/号码格式。
fn redact_pii(text: &str) -> String {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    let mut redacted = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        if let Some(end) = formatted_phone_run(&tokens, index) {
            redacted.push("[PII_REDACTED]");
            index = end;
        } else {
            redacted.push(if looks_like_email_or_phone(tokens[index]) {
                "[PII_REDACTED]"
            } else {
                tokens[index]
            });
            index += 1;
        }
    }
    redacted.join(" ")
}

/// 对一段字节计算 SHA-256，并返回带算法前缀的字符串形式。
///
/// 原始来源摘要和规范化结果摘要都通过此 helper 生成；不同调用点传入的字节内容不同，不能把两个 digest
/// 混为同一类值。摘要用于稳定比较，不提供保密或来源认证。
fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", journal_sha256(bytes))
}
