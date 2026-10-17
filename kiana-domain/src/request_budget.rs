//! Provider request wire-budget and stable-prefix contracts.
//!
//! Exact tokenizer counts and conservative local estimates are explicit alternatives. The
//! estimate is never presented as provider tokenizer truth. Stable prefix identity excludes
//! dynamic retrieval/clock suffixes but includes the profile, prompt/catalog and data version.

use crate::{json_digest, SchemaVersion, TokenBudget};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const WIRE_BUDGET_SCHEMA: &str = "kiana.wire-budget.v1";
pub const STABLE_PREFIX_SCHEMA: &str = "kiana.stable-prefix.v1";
pub const REQUEST_USAGE_DELTA_SCHEMA: &str = "kiana.request-usage-delta.v1";
pub const REQUEST_BUDGET_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_WIRE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_STABLE_SEGMENTS: usize = 256;

/// 预算必须覆盖的固定请求组成部分（顺序即契约顺序）。
///
/// `system`/`role` 产品文本随编码后的 system prompt 一起计入 `system_bytes`；
/// `history_content_blocks` 是用户/助手/工具消息与其中的 content blocks；
/// `attachments` 是 `WireBudgetInput::attachment_bytes`（inline 附件引用随消息字节计入，
/// 这里只声明适配器额外附加的附件开销）；`provider_framing` 是 provider 包装与头部开销；
/// `output_reserve` 是有效的输出预留，与输出 cap 取自同一份有效配置（[`TokenBudget`]）。
pub const REQUEST_BUDGET_COVERAGE_ITEMS: [&str; 7] = [
    "system",
    "role",
    "history_content_blocks",
    "tool_schemas",
    "attachments",
    "provider_framing",
    "output_reserve",
];

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenAccounting {
    Exact {
        input_tokens: u64,
    },
    /// An exact provider tokenizer count bound to the tokenizer artifact/config digest.
    ExactTokenizer {
        input_tokens: u64,
        tokenizer_digest: String,
    },
    ConservativeUtf8 {
        bytes_per_token: u64,
        safety_margin_tokens: u64,
    },
}

impl TokenAccounting {
    fn input_tokens(&self, input_bytes: u64) -> Result<u64, String> {
        match self {
            Self::Exact { input_tokens } => Ok(*input_tokens),
            Self::ExactTokenizer { input_tokens, .. } => Ok(*input_tokens),
            Self::ConservativeUtf8 {
                bytes_per_token,
                safety_margin_tokens,
            } if *bytes_per_token > 0 => input_bytes
                .saturating_add(bytes_per_token.saturating_sub(1))
                .checked_div(*bytes_per_token)
                .and_then(|value| value.checked_add(*safety_margin_tokens))
                .ok_or_else(|| "wire_budget_token_overflow".to_owned()),
            Self::ConservativeUtf8 { .. } => Err("wire_budget_bytes_per_token_invalid".to_owned()),
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Exact { input_tokens } if *input_tokens > 0 => Ok(()),
            Self::ExactTokenizer {
                input_tokens,
                tokenizer_digest,
            } if *input_tokens > 0 => digest(tokenizer_digest, "wire_budget_tokenizer_digest"),
            Self::ConservativeUtf8 {
                bytes_per_token, ..
            } if *bytes_per_token > 0 => Ok(()),
            Self::Exact { .. } | Self::ExactTokenizer { .. } => {
                Err("wire_budget_exact_tokens_invalid".to_owned())
            }
            Self::ConservativeUtf8 { .. } => Err("wire_budget_bytes_per_token_invalid".to_owned()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBudgetInput {
    pub system_bytes: u64,
    pub message_bytes: u64,
    pub tool_schema_bytes: u64,
    pub attachment_bytes: u64,
    pub provider_framing_bytes: u64,
    pub output_reserved_tokens: u64,
    pub output_cap_tokens: u64,
    pub token_limit: u64,
    pub accounting: TokenAccounting,
}

impl WireBudgetInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.token_limit == 0
            || self.output_cap_tokens == 0
            || self.output_reserved_tokens == 0
            || self.output_reserved_tokens > self.output_cap_tokens
        {
            return Err("wire_budget_output_reserve_invalid".to_owned());
        }
        let bytes = self.input_bytes()?;
        if bytes > MAX_WIRE_BYTES {
            return Err("wire_budget_input_too_large".to_owned());
        }
        self.accounting.validate()?;
        Ok(())
    }

    pub fn input_bytes(&self) -> Result<u64, String> {
        self.system_bytes
            .checked_add(self.message_bytes)
            .and_then(|value| value.checked_add(self.tool_schema_bytes))
            .and_then(|value| value.checked_add(self.attachment_bytes))
            .and_then(|value| value.checked_add(self.provider_framing_bytes))
            .ok_or_else(|| "wire_budget_bytes_overflow".to_owned())
    }

    /// 这份预算输入覆盖的固定组成部分；调用方用它与实际观测字节一起解释预算范围。
    pub fn coverage(&self) -> [&'static str; REQUEST_BUDGET_COVERAGE_ITEMS.len()] {
        REQUEST_BUDGET_COVERAGE_ITEMS
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBudget {
    pub schema: String,
    pub version: SchemaVersion,
    pub input_bytes: u64,
    pub estimated_input_tokens: u64,
    pub output_reserved_tokens: u64,
    pub total_reserved_tokens: u64,
    pub token_limit: u64,
    pub headroom_tokens: u64,
    pub accounting: TokenAccounting,
    pub budget_digest: String,
}

impl WireBudget {
    pub fn from_input(input: WireBudgetInput) -> Result<Self, String> {
        input.validate()?;
        let input_bytes = input.input_bytes()?;
        let estimated_input_tokens = input.accounting.input_tokens(input_bytes)?;
        let total_reserved_tokens = estimated_input_tokens
            .checked_add(input.output_reserved_tokens)
            .ok_or_else(|| "wire_budget_token_overflow".to_owned())?;
        if total_reserved_tokens > input.token_limit {
            return Err("wire_budget_exceeded".to_owned());
        }
        let mut budget = Self {
            schema: WIRE_BUDGET_SCHEMA.to_owned(),
            version: REQUEST_BUDGET_VERSION,
            input_bytes,
            estimated_input_tokens,
            output_reserved_tokens: input.output_reserved_tokens,
            total_reserved_tokens,
            token_limit: input.token_limit,
            headroom_tokens: input.token_limit - total_reserved_tokens,
            accounting: input.accounting,
            budget_digest: String::new(),
        };
        budget.budget_digest = budget.digest();
        budget.validate()?;
        Ok(budget)
    }

    /// Account one already-rendered provider body as a single immutable wire input.
    ///
    /// Callers must pass the final serialized body length; the provider framing reserve and
    /// output reserve are still separate fields in the resulting contract.
    pub fn from_final_wire(
        wire_bytes: u64,
        output_reserved_tokens: u64,
        output_cap_tokens: u64,
        token_limit: u64,
        accounting: TokenAccounting,
    ) -> Result<Self, String> {
        Self::from_input(WireBudgetInput {
            system_bytes: 0,
            message_bytes: wire_bytes,
            tool_schema_bytes: 0,
            attachment_bytes: 0,
            provider_framing_bytes: 0,
            output_reserved_tokens,
            output_cap_tokens,
            token_limit,
            accounting,
        })
    }

    /// 用同一份有效预算配置（[`TokenBudget`]）构造 wire 预算。
    ///
    /// 输出预留与输出 cap 都取自该对象（`reserved_output` / `limit`），发送路径不再另外
    /// 固定一组常量分别计算，避免估算、预留和实际发送各读一份配置。
    pub fn from_effective_wire(
        wire_bytes: u64,
        effective: &TokenBudget,
        accounting: TokenAccounting,
    ) -> Result<Self, String> {
        Self::from_final_wire(
            wire_bytes,
            effective.reserved_output,
            effective.limit,
            effective.limit,
            accounting,
        )
    }

    /// 这份 wire 预算覆盖的固定组成部分；见 [`REQUEST_BUDGET_COVERAGE_ITEMS`]。
    pub fn coverage_items(&self) -> [&'static str; REQUEST_BUDGET_COVERAGE_ITEMS.len()] {
        REQUEST_BUDGET_COVERAGE_ITEMS
    }

    pub fn validate(&self) -> Result<(), String> {
        self.accounting.validate()?;
        if self.schema != WIRE_BUDGET_SCHEMA
            || !self.version.is_compatible_with(&REQUEST_BUDGET_VERSION)
            || self.token_limit == 0
            || self.output_reserved_tokens == 0
            || self.total_reserved_tokens
                != self
                    .estimated_input_tokens
                    .checked_add(self.output_reserved_tokens)
                    .unwrap_or(u64::MAX)
            || self.total_reserved_tokens > self.token_limit
            || self.headroom_tokens != self.token_limit - self.total_reserved_tokens
        {
            return Err("wire_budget_invalid".to_owned());
        }
        if self.accounting.input_tokens(self.input_bytes)? != self.estimated_input_tokens {
            return Err("wire_budget_accounting_drift".to_owned());
        }
        digest(&self.budget_digest, "wire_budget_digest")?;
        if self.budget_digest != self.digest() {
            return Err("wire_budget_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "input_bytes": self.input_bytes,
            "estimated_input_tokens": self.estimated_input_tokens,
            "output_reserved_tokens": self.output_reserved_tokens,
            "total_reserved_tokens": self.total_reserved_tokens,
            "token_limit": self.token_limit,
            "accounting": self.accounting,
        }))
    }
}

/// 估算与实际用量的差值记录。
///
/// `usage_complete=false` 表示 provider 未回报用量：此时差值保持 `None`，不能把输出预留
/// 或整份预留当作实际消费。它是审计解释，不是计费凭证。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestUsageDelta {
    pub schema: String,
    pub estimated_input_tokens: u64,
    pub output_reserved_tokens: u64,
    pub actual_input_tokens: Option<u64>,
    pub actual_output_tokens: Option<u64>,
    pub input_delta_tokens: Option<i64>,
    pub output_delta_tokens: Option<i64>,
    pub usage_complete: bool,
}

impl RequestUsageDelta {
    /// 从同一份 wire 预算的估算值与 provider 回报用量构造差值。
    pub fn from_usage(
        estimated_input_tokens: u64,
        output_reserved_tokens: u64,
        usage: Option<&crate::ModelUsage>,
    ) -> Self {
        let (actual_input_tokens, actual_output_tokens) = match usage {
            Some(usage) => (Some(usage.input_tokens), Some(usage.output_tokens)),
            None => (None, None),
        };
        Self {
            schema: REQUEST_USAGE_DELTA_SCHEMA.to_owned(),
            estimated_input_tokens,
            output_reserved_tokens,
            actual_input_tokens,
            actual_output_tokens,
            input_delta_tokens: actual_input_tokens
                .map(|actual| token_delta(actual, estimated_input_tokens)),
            output_delta_tokens: actual_output_tokens
                .map(|actual| token_delta(actual, output_reserved_tokens)),
            usage_complete: usage.is_some(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REQUEST_USAGE_DELTA_SCHEMA
            || self.estimated_input_tokens == 0
            || self.output_reserved_tokens == 0
        {
            return Err("request_usage_delta_invalid".to_owned());
        }
        let complete = self.actual_input_tokens.is_some()
            && self.actual_output_tokens.is_some()
            && self.input_delta_tokens.is_some()
            && self.output_delta_tokens.is_some();
        if self.usage_complete != complete {
            return Err("request_usage_delta_incomplete".to_owned());
        }
        Ok(())
    }
}

fn token_delta(actual: u64, baseline: u64) -> i64 {
    let delta = actual as i128 - baseline as i128;
    delta.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StablePrefixSegment {
    pub name: String,
    pub order: u32,
    pub content_digest: String,
}

impl StablePrefixSegment {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.name, "stable_prefix_segment_name", 256)?;
        digest(&self.content_digest, "stable_prefix_segment_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StablePrefix {
    pub schema: String,
    pub version: SchemaVersion,
    pub profile: String,
    pub prompt_bundle_digest: String,
    pub catalog_digest: String,
    pub configuration_revision: String,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub segments: Vec<StablePrefixSegment>,
    pub dynamic_suffix_digest: String,
    pub prefix_digest: String,
    pub cache_key_digest: String,
}

impl StablePrefix {
    pub fn new(
        profile: impl Into<String>,
        prompt_bundle_digest: impl Into<String>,
        catalog_digest: impl Into<String>,
        configuration_revision: impl Into<String>,
        authority_epoch: u64,
        data_epoch: u64,
        mut segments: Vec<StablePrefixSegment>,
        dynamic_suffix_digest: impl Into<String>,
    ) -> Result<Self, String> {
        segments.sort_by_key(|segment| (segment.order, segment.name.clone()));
        let mut prefix = Self {
            schema: STABLE_PREFIX_SCHEMA.to_owned(),
            version: REQUEST_BUDGET_VERSION,
            profile: profile.into(),
            prompt_bundle_digest: prompt_bundle_digest.into(),
            catalog_digest: catalog_digest.into(),
            configuration_revision: configuration_revision.into(),
            authority_epoch,
            data_epoch,
            segments,
            dynamic_suffix_digest: dynamic_suffix_digest.into(),
            prefix_digest: String::new(),
            cache_key_digest: String::new(),
        };
        prefix.prefix_digest = prefix.compute_prefix_digest();
        prefix.cache_key_digest = prefix.compute_cache_key_digest();
        prefix.validate()?;
        Ok(prefix)
    }

    fn compute_prefix_digest(&self) -> String {
        json_digest(&serde_json::json!({"segments": self.segments}))
    }

    fn compute_cache_key_digest(&self) -> String {
        Self::cache_key_digest_for(
            &self.profile,
            &self.prompt_bundle_digest,
            &self.catalog_digest,
            &self.configuration_revision,
            self.authority_epoch,
            self.data_epoch,
        )
    }

    fn cache_key_digest_for(
        profile: &str,
        prompt_bundle_digest: &str,
        catalog_digest: &str,
        configuration_revision: &str,
        authority_epoch: u64,
        data_epoch: u64,
    ) -> String {
        json_digest(&serde_json::json!({
            "profile": profile,
            "prompt_bundle_digest": prompt_bundle_digest,
            "catalog_digest": catalog_digest,
            "configuration_revision": configuration_revision,
            "authority_epoch": authority_epoch,
            "data_epoch": data_epoch,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STABLE_PREFIX_SCHEMA
            || !self.version.is_compatible_with(&REQUEST_BUDGET_VERSION)
            || self.data_epoch == 0
            || self.authority_epoch == 0
            || self.segments.len() > MAX_STABLE_SEGMENTS
        {
            return Err("stable_prefix_header_invalid".to_owned());
        }
        required(&self.profile, "stable_prefix_profile", 256)?;
        digest(
            &self.configuration_revision,
            "stable_prefix_configuration_revision",
        )?;
        digest(&self.prompt_bundle_digest, "stable_prefix_prompt_digest")?;
        digest(&self.catalog_digest, "stable_prefix_catalog_digest")?;
        digest(&self.dynamic_suffix_digest, "stable_prefix_suffix_digest")?;
        let mut names = BTreeSet::new();
        for segment in &self.segments {
            segment.validate()?;
            if !names.insert(segment.name.clone()) {
                return Err("stable_prefix_segment_duplicate".to_owned());
            }
        }
        if self
            .segments
            .windows(2)
            .any(|pair| (pair[0].order, &pair[0].name) > (pair[1].order, &pair[1].name))
        {
            return Err("stable_prefix_order_invalid".to_owned());
        }
        digest(&self.prefix_digest, "stable_prefix_digest")?;
        digest(&self.cache_key_digest, "stable_prefix_cache_key")?;
        if self.prefix_digest != self.compute_prefix_digest()
            || self.cache_key_digest != self.compute_cache_key_digest()
        {
            return Err("stable_prefix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// A cache entry is reusable only for the same route/configuration, prompt/tool catalog and
    /// current authority/data epochs. Dynamic suffix content is intentionally not part of this
    /// identity, but revocation epochs always are.
    pub fn cache_hit_allowed(
        &self,
        profile: &str,
        prompt_bundle_digest: &str,
        catalog_digest: &str,
        configuration_revision: &str,
        authority_epoch: u64,
        data_epoch: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if self.profile != profile {
            return Err("stable_prefix_profile_changed".to_owned());
        }
        if self.prompt_bundle_digest != prompt_bundle_digest {
            return Err("stable_prefix_prompt_changed".to_owned());
        }
        if self.catalog_digest != catalog_digest {
            return Err("stable_prefix_catalog_changed".to_owned());
        }
        if self.configuration_revision != configuration_revision {
            return Err("stable_prefix_configuration_changed".to_owned());
        }
        if self.authority_epoch != authority_epoch {
            return Err("stable_prefix_authority_revoked".to_owned());
        }
        if self.data_epoch != data_epoch {
            return Err("stable_prefix_data_epoch_changed".to_owned());
        }
        if self.cache_key_digest
            != Self::cache_key_digest_for(
                profile,
                prompt_bundle_digest,
                catalog_digest,
                configuration_revision,
                authority_epoch,
                data_epoch,
            )
        {
            return Err("stable_prefix_cache_key_mismatch".to_owned());
        }
        Ok(())
    }

    /// 已记录的缓存条目（`self`）能否复用于当前请求身份（`current`）。
    ///
    /// 撤销与数据版本优先于可见前缀比较：权限收紧（authority epoch 变化）或数据删除/内容
    /// 撤销（data epoch 变化）后，即使稳定前缀段完全相同也必须拒绝复用。随后才比较
    /// profile、prompt bundle、工具目录、配置 revision 与缓存 key。返回稳定原因字符串，
    /// 调用方据此记录缓存失效原因，而不是静默沿用旧条目。
    pub fn reuse_for(&self, current: &StablePrefix) -> Result<(), String> {
        self.validate()?;
        current.validate()?;
        if self.authority_epoch != current.authority_epoch {
            return Err("stable_prefix_authority_revoked".to_owned());
        }
        if self.data_epoch != current.data_epoch {
            return Err("stable_prefix_data_epoch_changed".to_owned());
        }
        if self.profile != current.profile {
            return Err("stable_prefix_profile_changed".to_owned());
        }
        if self.prompt_bundle_digest != current.prompt_bundle_digest {
            return Err("stable_prefix_prompt_changed".to_owned());
        }
        if self.catalog_digest != current.catalog_digest {
            return Err("stable_prefix_catalog_changed".to_owned());
        }
        if self.configuration_revision != current.configuration_revision {
            return Err("stable_prefix_configuration_changed".to_owned());
        }
        if self.cache_key_digest != current.cache_key_digest {
            return Err("stable_prefix_cache_key_mismatch".to_owned());
        }
        Ok(())
    }
}
