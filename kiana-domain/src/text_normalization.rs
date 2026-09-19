//! Deterministic text normalization and sensitive-data boundaries for context indexing.
//!
//! The profile is deliberately explicit about its limited Unicode policy. It normalizes BOM,
//! line endings and common full-width ASCII without pretending to be a complete Unicode standard
//! implementation. Sensitive material is rejected, redacted, or represented only by a digest;
//! embedding metadata and LLM metadata are separate strict records.

use crate::{journal_sha256, json_digest, memory_tokens, redact_text, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const TEXT_NORMALIZATION_PROFILE_SCHEMA: &str = "kiana.text-normalization-profile.v1";
pub const NORMALIZED_TEXT_SCHEMA: &str = "kiana.normalized-text.v1";
pub const EMBEDDING_METADATA_SCHEMA: &str = "kiana.embedding-metadata.v1";
pub const LLM_TEXT_METADATA_SCHEMA: &str = "kiana.llm-text-metadata.v1";
pub const TEXT_NORMALIZATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_NORMALIZED_BYTES: usize = 256 * 1024;
pub const MAX_NORMALIZED_TOKENS: usize = 16_384;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveHandling {
    Reject,
    Redact,
    ReferenceOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveDisposition {
    None,
    Redacted,
    ReferenceOnly,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextNormalizationProfile {
    pub schema: String,
    pub version: SchemaVersion,
    pub unicode_policy: String,
    pub newline_policy: String,
    pub identifier_policy: String,
    pub sensitive_handling: SensitiveHandling,
    pub max_bytes: u64,
    pub max_tokens: u32,
    pub profile_digest: String,
}

impl TextNormalizationProfile {
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

impl Default for TextNormalizationProfile {
    fn default() -> Self {
        Self::new(SensitiveHandling::Redact).expect("built-in normalization profile is valid")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingMetadata {
    pub schema: String,
    pub model_id: String,
    pub input_digest: String,
    pub token_count: u32,
    pub secret_free: bool,
    pub metadata_digest: String,
}

impl EmbeddingMetadata {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmTextMetadata {
    pub schema: String,
    pub source_digest: String,
    pub normalized_digest: String,
    pub language: String,
    #[serde(default)]
    pub redaction_profile_digest: Option<String>,
    pub secret_free: bool,
    pub metadata_digest: String,
}

impl LlmTextMetadata {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedText {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_digest: String,
    pub normalized_digest: String,
    #[serde(default)]
    pub text: Option<String>,
    pub tokens: Vec<String>,
    pub language: String,
    pub disposition: SensitiveDisposition,
    pub profile_digest: String,
    pub embedding: EmbeddingMetadata,
    pub llm: LlmTextMetadata,
    pub result_digest: String,
}

impl NormalizedText {
    pub fn normalize(profile: &TextNormalizationProfile, source: &str) -> Result<Self, String> {
        profile.validate()?;
        if source.as_bytes().len() > profile.max_bytes as usize {
            return Err("text_normalization_input_too_large".to_owned());
        }
        let source_digest = digest_bytes(source.as_bytes());
        let canonical = canonical_text(source);
        let sensitive = contains_sensitive(&canonical);
        let (text, disposition) = if sensitive {
            match profile.sensitive_handling {
                SensitiveHandling::Reject => {
                    return Err("normalization_sensitive_input_rejected".to_owned())
                }
                SensitiveHandling::Redact => (
                    Some(redact_pii(&redact_text(&canonical))),
                    SensitiveDisposition::Redacted,
                ),
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

fn fullwidth_ascii(character: char) -> char {
    match character as u32 {
        0xff01..=0xff5e => char::from_u32(character as u32 - 0xfee0).unwrap_or(character),
        0x3000 => ' ',
        _ => character,
    }
}

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

fn contains_sensitive(text: &str) -> bool {
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
        "-----begin ",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
        || text.split_whitespace().any(looks_like_email_or_phone)
}

fn looks_like_email_or_phone(value: &str) -> bool {
    let email = value.contains('@') && value.rsplit_once('.').is_some();
    let digits = value
        .chars()
        .filter(|character| character.is_ascii_digit())
        .count();
    email || digits >= 10
}

fn redact_pii(text: &str) -> String {
    text.split_whitespace()
        .map(|part| {
            if looks_like_email_or_phone(part) {
                "[PII_REDACTED]"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", journal_sha256(bytes))
}
