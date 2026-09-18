//! UTC quota windows and canonical provider/credential/model grouping.

use crate::{json_digest, ClockObservation, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const QUOTA_WINDOW_SCHEMA: &str = "kiana.quota-window.v1";
pub const QUOTA_GROUP_SCHEMA: &str = "kiana.quota-group.v1";
pub const QUOTA_WINDOW_BUDGET_SCHEMA: &str = "kiana.quota-window-budget.v1";
pub const QUOTA_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaWindow {
    pub schema: String,
    pub version: SchemaVersion,
    pub start_unix_ms: u64,
    pub end_unix_ms: u64,
    pub duration_ms: u64,
    pub timezone: String,
    pub window_digest: String,
}

impl QuotaWindow {
    pub fn from_clock(clock: &ClockObservation, duration_ms: u64) -> Result<Self, String> {
        clock.require_trusted()?;
        if duration_ms == 0 || duration_ms > MAX_WINDOW_MS {
            return Err("quota_window_duration_invalid".to_owned());
        }
        let start = (clock.wall_now_unix_ms / duration_ms)
            .checked_mul(duration_ms)
            .ok_or_else(|| "quota_window_overflow".to_owned())?;
        let end = start
            .checked_add(duration_ms)
            .ok_or_else(|| "quota_window_overflow".to_owned())?;
        let mut window = Self {
            schema: QUOTA_WINDOW_SCHEMA.to_owned(),
            version: QUOTA_SCHEMA_VERSION,
            start_unix_ms: start,
            end_unix_ms: end,
            duration_ms,
            timezone: "UTC".to_owned(),
            window_digest: String::new(),
        };
        window.window_digest = window.digest();
        window.validate()?;
        Ok(window)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUOTA_WINDOW_SCHEMA
            || !self.version.is_compatible_with(&QUOTA_SCHEMA_VERSION)
            || self.duration_ms == 0
            || self.duration_ms > MAX_WINDOW_MS
            || self.end_unix_ms <= self.start_unix_ms
            || self.end_unix_ms - self.start_unix_ms != self.duration_ms
            || self.timezone != "UTC"
            || !valid_digest(&self.window_digest)
            || self.window_digest != self.digest()
        {
            return Err("quota_window_invalid".to_owned());
        }
        Ok(())
    }

    pub fn retry_after_ms(&self, now_unix_ms: u64) -> Result<u64, String> {
        self.validate()?;
        if now_unix_ms < self.start_unix_ms {
            return Err("quota_clock_rollback".to_owned());
        }
        Ok(self.end_unix_ms.saturating_sub(now_unix_ms))
    }

    pub fn until_next_window_ms(&self, now_unix_ms: u64) -> Result<u64, String> {
        self.validate()?;
        if now_unix_ms < self.start_unix_ms {
            return Err("quota_clock_rollback".to_owned());
        }
        Ok(self.end_unix_ms.saturating_sub(now_unix_ms))
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "start_unix_ms": self.start_unix_ms,
            "end_unix_ms": self.end_unix_ms,
            "duration_ms": self.duration_ms,
            "timezone": self.timezone,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaGroupKey {
    pub schema: String,
    pub version: SchemaVersion,
    pub provider_id: String,
    pub credential_group: String,
    pub model_id: Option<String>,
    pub alias: Option<String>,
    pub group_digest: String,
}

impl QuotaGroupKey {
    pub fn new(
        provider_id: impl Into<String>,
        credential_group: impl Into<String>,
        model_id: Option<String>,
        alias: Option<String>,
    ) -> Result<Self, String> {
        let mut key = Self {
            schema: QUOTA_GROUP_SCHEMA.to_owned(),
            version: QUOTA_SCHEMA_VERSION,
            provider_id: provider_id.into().trim().to_ascii_lowercase(),
            credential_group: credential_group.into().trim().to_ascii_lowercase(),
            model_id: model_id.map(|value| value.trim().to_ascii_lowercase()),
            alias: alias.map(|value| value.trim().to_ascii_lowercase()),
            group_digest: String::new(),
        };
        key.group_digest = key.digest();
        key.validate()?;
        Ok(key)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUOTA_GROUP_SCHEMA
            || !self.version.is_compatible_with(&QUOTA_SCHEMA_VERSION)
            || !bounded(&self.provider_id, 256)
            || !bounded(&self.credential_group, 256)
            || self
                .model_id
                .as_deref()
                .is_some_and(|value| !bounded(value, 256))
            || self
                .alias
                .as_deref()
                .is_some_and(|value| !bounded(value, 256))
            || !valid_digest(&self.group_digest)
            || self.group_digest != self.digest()
        {
            return Err("quota_group_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "provider_id": self.provider_id,
            "credential_group": self.credential_group,
            "model_id": self.model_id,
            "alias": self.alias,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaWindowBudget {
    pub schema: String,
    pub version: SchemaVersion,
    pub group: QuotaGroupKey,
    pub window: QuotaWindow,
    pub max_requests: u64,
    pub max_tokens: u64,
    pub max_concurrency: u32,
    pub used_requests: u64,
    pub used_tokens: u64,
    pub active_concurrency: u32,
    pub budget_digest: String,
}

impl QuotaWindowBudget {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUOTA_WINDOW_BUDGET_SCHEMA
            || !self.version.is_compatible_with(&QUOTA_SCHEMA_VERSION)
            || self.max_requests == 0
            || self.max_tokens == 0
            || self.max_concurrency == 0
            || self.used_requests > self.max_requests
            || self.used_tokens > self.max_tokens
            || self.active_concurrency > self.max_concurrency
            || !valid_digest(&self.budget_digest)
            || self.budget_digest != self.digest()
        {
            return Err("quota_window_budget_invalid".to_owned());
        }
        self.group.validate()?;
        self.window.validate()?;
        Ok(())
    }

    pub fn check(&self, requests: u64, tokens: u64, concurrency: u32) -> Result<(), String> {
        self.validate()?;
        let retry_after = self
            .window
            .until_next_window_ms(self.window.start_unix_ms)?;
        if requests > self.max_requests.saturating_sub(self.used_requests)
            || tokens > self.max_tokens.saturating_sub(self.used_tokens)
            || concurrency > self.max_concurrency.saturating_sub(self.active_concurrency)
        {
            return Err(format!("quota_exhausted:retry_after_ms={retry_after}"));
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "group": self.group,
            "window": self.window,
            "max_requests": self.max_requests,
            "max_tokens": self.max_tokens,
            "max_concurrency": self.max_concurrency,
            "used_requests": self.used_requests,
            "used_tokens": self.used_tokens,
            "active_concurrency": self.active_concurrency,
        }))
    }
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && !value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
