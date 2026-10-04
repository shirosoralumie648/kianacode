//! 脱敏（redaction）与 secret 哨兵扫描。整个仓库**每一次**要把内容写进事件、
//! 日志、指标、Trace 或 Receipt 的地方，都要经过这里。
//!
//! # 为什么单独一个文件、而且放在最底层
//!
//! 因为脱敏是**唯一一件「漏了就无法挽回」的事**。其它数据写错了还能改；
//! 一个 secret 写进了 EventLog，它就已经在那本账里了——而且那本账按设计是 append-only。
//!
//! 所以这个文件的函数被其它所有层复用，而不是各写各的。
//!
//! # 两条独立的防线
//!
//! ```text
//! redact_text / redact_value    把「已知的敏感形状」替换掉
//!        ↓
//! scan_secret_sentinels         找出「长得像 secret」的东西并报告
//! ```
//!
//! 前者是**替换**，后者是**发现**。两者都需要，因为：
//! 前者能处理已知标记（`api_key = …`），但对「没有标记的一段随机字符串」无能为力；
//! 后者能认出 AWS key、GitHub token 这类**固定形状**，但它只能报告、不能替你决定要不要保留。
//!
//! # ⚠ redaction 是尽力而为，不是保证
//!
//! 这是本文件最需要被理解的一点。按标记扫描意味着：**一个不以已知标记出现的 secret
//! 不会被替换掉**。它会原样通过。
//!
//! 所以正确的用法是「先 redact，再 scan」——两道都过一遍，而不是指望其中一道。
//! 单靠 redact 的代码，安全性等于「你的标记列表有多全」。
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
pub const REDACTION_PROFILE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 脱敏时递归的最大深度：32 层。
///
/// 【为什么需要上限】
/// 恶意或意外构造的深层嵌套 JSON 会让递归脱敏一直往下走。
/// 32 层对真实业务数据（一般 3–5 层）绰绰有余，而它把最坏情况的代价钉住了。
///
/// ⚠ 超过上限的处理是「停止下钻」——那一层以下的内容**原样保留**。
///   所以深度上限是安全与保真的取舍，不是纯粹的加固。
pub const MAX_REDACTION_DEPTH: usize = 32;
pub const MAX_REDACTION_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_REDACTION_PROFILE_BYTES: usize = 256 * 1024;

/// Secret egress scanning is deliberately independent from a producer's redaction profile.
/// Profiles make values safe where possible; this scan is the final deny-first fence for every
/// projection channel (prompt, transcript, EventLog, receipt, process output and cache).
pub const SECRET_SENTINEL_SCAN_SCHEMA: &str = "kiana.secret-sentinel-scan.v1";
pub const MAX_SECRET_SENTINEL_SCAN_BYTES: usize = 64 * 1024;
pub const MAX_SECRET_SENTINELS: usize = 64;
pub const REDACTED_ERROR_PROJECTION_SCHEMA: &str = "kiana.redacted-error-projection.v1";

/// Destination being checked by the shared secret scanner.  Keeping this vocabulary in the
/// domain crate prevents a daemon, event or UI adapter from silently inventing an unscanned sink.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretScanChannel {
    Prompt,
    Transcript,
    Event,
    Receipt,
    Stdout,
    Stderr,
    Argv,
    Env,
    Cache,
}

impl SecretScanChannel {
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

/// Shape which caused a candidate to be rejected.  Findings never carry the value itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretSentinelKind {
    Token,
    ApiKey,
    Header,
    Jwt,
    UrlUserinfo,
    CredentialLease,
    ProviderRawError,
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

/// A bounded, secret-free scan failure.  The marker digest is useful for grouping incidents but
/// is not reversible to the candidate secret.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretSentinelFinding {
    pub schema: String,
    pub channel: SecretScanChannel,
    pub kind: SecretSentinelKind,
    pub marker_digest: String,
}

impl SecretSentinelFinding {
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

/// A projection for raw provider/transport errors.  Deliberately excludes the original message;
/// callers can persist this object in an Event/Receipt or return it to UI safely.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactedErrorProjection {
    pub schema: String,
    pub channel: SecretScanChannel,
    pub code: String,
    pub error_digest: String,
    pub redacted: bool,
}

impl RedactedErrorProjection {
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

/// Streaming redaction retains at most this many bytes of already-emitted text as overlap so a
/// sensitive marker split across chunks can still be recognized. The value is deliberately larger
/// than the longest marker below.
pub const STREAM_REDACTION_BUFFER_LIMIT: usize = 64;

/// Signal boundary whose payload is allowed to pass through the bounded encoder.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionSignal {
    Log,
    Metric,
    Trace,
    Audit,
    Export,
}

impl RedactionSignal {
    fn default_limit(self) -> usize {
        match self {
            Self::Log => 16 * 1024,
            Self::Metric => 2 * 1024,
            Self::Trace => 8 * 1024,
            Self::Audit => 16 * 1024,
            Self::Export => 64 * 1024,
        }
    }
}

/// Versioned, digest-bound policy for one redaction boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionProfile {
    pub schema: String,
    pub version: SchemaVersion,
    pub signal: RedactionSignal,
    pub data_class: DataClass,
    pub max_bytes: usize,
    pub max_depth: usize,
    pub profile_digest: String,
}

impl RedactionProfile {
    pub fn for_signal(signal: RedactionSignal) -> Self {
        Self::new(
            signal,
            DataClass::Internal,
            signal.default_limit(),
            MAX_REDACTION_DEPTH,
        )
        .expect("built-in redaction profile is valid")
    }

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

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert("profile_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

impl Default for RedactionProfile {
    fn default() -> Self {
        Self::for_signal(RedactionSignal::Log)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoundedRedactedValue {
    pub value: Value,
    pub encoded_bytes: usize,
    pub profile_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedRedactedText {
    pub text: String,
    pub encoded_bytes: usize,
    pub profile_digest: String,
}

/// Apply a profile to a structured signal. Errors are terminal and never return the input value.
pub fn encode_bounded_value(
    profile: &RedactionProfile,
    value: &Value,
) -> Result<BoundedRedactedValue, String> {
    profile.validate()?;
    validate_value_shape(value, 0, profile.max_depth)?;
    let redacted = redact_value(value);
    validate_value_shape(&redacted, 0, profile.max_depth)?;
    if contains_unredacted_secret(&redacted) {
        return Err("redaction_secret_sentinel_detected".to_owned());
    }
    scan_secret_value(SecretScanChannel::Event, &redacted)
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

/// Text counterpart for logs, trace attributes and export rows.
pub fn encode_bounded_text(
    profile: &RedactionProfile,
    text: &str,
) -> Result<BoundedRedactedText, String> {
    profile.validate()?;
    if text.as_bytes().contains(&0) {
        return Err("redaction_nul_forbidden".to_owned());
    }
    let redacted = redact_text(text);
    if redact_text(&redacted) != redacted || contains_text_secret_marker(&redacted) {
        return Err("redaction_secret_sentinel_detected".to_owned());
    }
    scan_secret_sentinels(SecretScanChannel::Transcript, &redacted)
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

/// Short aliases used by signal producers so they cannot accidentally bypass the profile.
pub fn redact_with_profile(profile: &RedactionProfile, value: &Value) -> Result<Value, String> {
    encode_bounded_value(profile, value).map(|encoded| encoded.value)
}

pub fn redact_text_with_profile(profile: &RedactionProfile, text: &str) -> Result<String, String> {
    encode_bounded_text(profile, text).map(|encoded| encoded.text)
}

/// Scan one complete text value for secret shapes.  This is a deny-first check: callers must
/// redact first and then call the scanner, while raw provider errors use
/// [`project_redacted_error`] instead of returning their text.
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
    Ok(())
}

/// Structured counterpart used before EventLog/Receipt/cache serialization.  SecretRef metadata
/// remains valid, but raw values under secret-like fields fail closed.
pub fn scan_secret_value(
    channel: SecretScanChannel,
    value: &Value,
) -> Result<(), SecretSentinelFinding> {
    let encoded = serde_json::to_vec(value).map_err(|_| {
        SecretSentinelFinding::new(
            channel,
            SecretSentinelKind::ProviderRawError,
            "encode_failed",
        )
    })?;
    if encoded.len() > MAX_SECRET_SENTINEL_SCAN_BYTES {
        return Err(SecretSentinelFinding::new(
            channel,
            SecretSentinelKind::ProviderRawError,
            "value_too_large",
        ));
    }
    scan_secret_value_inner(channel, value)
}

/// Scan a bounded set of candidate channels and explicit echo sentinels.  It is intended for
/// fixtures and CI gates that replay one secret through every possible projection sink.
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

/// Safe projection for a provider/transport error.  No provider message or URL/header material
/// crosses this return boundary, even when the input was malformed or contained a secret.
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

fn redacted_placeholder_is_complete(value: &str) -> bool {
    let Some(remainder) = value.strip_prefix("[redacted]") else {
        return false;
    };
    remainder.is_empty()
        || remainder.chars().next().is_some_and(|character| {
            character.is_whitespace() || matches!(character, '&' | ',' | ';' | '"' | '}')
        })
}

fn contains_json_secret_key(text: &str) -> bool {
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

fn normalize_secret_key(key: &str) -> String {
    key.to_ascii_lowercase().replace('-', "_")
}

fn is_opaque_reference_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("secret_ref") || key.eq_ignore_ascii_case("credential_ref")
}

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

/// Numeric usage metadata shares one allowlist across redaction and residual-secret checks.
/// Strings and non-allowlisted token fields remain sensitive.
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

fn contains_jwt_shape(text: &str) -> bool {
    text.split(|character: char| {
        character.is_whitespace()
            || matches!(character, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']')
    })
    .any(looks_like_jwt)
}

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
struct SensitiveMarker {
    literal: &'static str,
    quoted_value: bool,
}

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

/// Redact secrets in one complete text value.
/// 脱敏一段文本。**这是全仓库被调用最多的函数之一。**
///
/// 【它做两件事，顺序不能换】
/// 1. 如果这段文本**看起来像 JSON**（首字符是 `{` 或 `[`）且能解析成功，
///    就走 [ `redact_value` ] 做**结构化**脱敏——按字段名判断，而不是按文本模式；
/// 2. 否则按标记做**文本扫描**：找到 `api_key`、`token` 这类词，
///    把它们后面跟的值替换掉。
///
/// 【为什么 JSON 要走结构化】
/// 因为结构化脱敏能按**字段名**判断敏感度，而文本扫描只能按**出现位置**猜。
/// 一段 `{"notes": "the api_key is abc"}` 里，结构化路径知道 `notes` 不是敏感字段，
/// 而文本路径会把 `abc` 也一起替换掉——过度脱敏会让日志失去诊断价值。
///
/// 【⚠ 返回类型是 `String` 而不是 `Result`】
/// 这是个刻意的取舍：它让调用点写起来很轻（我在多个模块里都是直接调用）。
/// 代价是**脱敏失败会被静默吞掉**——解析失败时它退回文本扫描，
/// 而文本扫描对无标记的 secret 无能为力。
/// 需要「失败必须被看见」的地方，应该用 [ `redact_text_with_profile` ]。
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

/// Redact sensitive object keys and text values recursively.
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
enum Suppression {
    Quoted,
    UnquotedAwaitingValue,
    UnquotedValue,
}

impl Suppression {
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

/// Stateful redaction for a text stream.
///
/// The redactor emits ordinary text immediately and retains only a bounded overlap of already
/// emitted text so a sensitive marker split across chunks can still be recognized. Once a marker
/// is recognized, the following value is suppressed until its delimiter; if the stream ends first,
/// [`Self::finish`] emits the replacement marker instead of the buffered secret.
#[derive(Debug)]
pub struct StreamingRedactor {
    overlap: String,
    suppression: Option<Suppression>,
    suppressed_any: bool,
    finished: bool,
}

impl Default for StreamingRedactor {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingRedactor {
    /// Construct an empty streaming redactor.
    pub fn new() -> Self {
        debug_assert!(SENSITIVE_MARKERS
            .iter()
            .all(|marker| marker.literal.len() <= STREAM_REDACTION_BUFFER_LIMIT));
        Self {
            overlap: String::new(),
            suppression: None,
            suppressed_any: false,
            finished: false,
        }
    }

    /// Redact a stream chunk and return the text that is safe to expose now.
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

            let combined = format!("{}{}", self.overlap, remaining);
            let overlap_len = self.overlap.len();
            let Some((start, marker)) = earliest_marker(&combined) else {
                output.push_str(remaining);
                self.retain_overlap(&combined);
                break;
            };

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
        output
    }

    /// Flush the stream tail. A pending sensitive value is replaced with `[REDACTED]`.
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
        self.overlap.clear();
        self.finished = true;
        output
    }

    /// Whether a sensitive marker has been recognized and its value is still buffered.
    pub fn has_pending_secret(&self) -> bool {
        self.suppression.is_some()
    }

    /// Current number of retained overlap bytes. This is bounded by
    /// [`STREAM_REDACTION_BUFFER_LIMIT`] for ordinary stream processing.
    pub fn buffered_len(&self) -> usize {
        self.overlap.len()
    }

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

#[cfg(test)]
mod tests {
    use super::{redact_text, StreamingRedactor, STREAM_REDACTION_BUFFER_LIMIT};

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
}
