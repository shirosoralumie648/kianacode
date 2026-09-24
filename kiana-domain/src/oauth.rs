//! Opaque OAuth/workload identity lifecycle contracts.
//!
//! Authorization state and token material are deliberately split: this module carries only
//! bounded flow metadata, digests, scopes, expiry and generation.  Raw access/refresh tokens
//! belong to a provider-side protected adapter and must never cross this domain boundary.

use crate::{json_digest, RequestId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const OAUTH_AUTHORIZATION_SCHEMA: &str = "kiana.oauth-authorization.v1";
pub const OAUTH_CALLBACK_SCHEMA: &str = "kiana.oauth-callback.v1";
pub const OAUTH_TOKEN_METADATA_SCHEMA: &str = "kiana.oauth-token-metadata.v1";
pub const OAUTH_TOKEN_FILE_SCHEMA: &str = "kiana.oauth-token-file.v1";
pub const OAUTH_MAX_SCOPES: usize = 64;
pub const OAUTH_MAX_SCOPE_BYTES: usize = 256;
pub const OAUTH_MAX_TOKEN_TTL_MS: u64 = 31 * 24 * 60 * 60 * 1_000;

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains(['\0', '\r', '\n'])
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn scopes(values: &[String]) -> bool {
    values.len() <= OAUTH_MAX_SCOPES
        && values.windows(2).all(|pair| pair[0] < pair[1])
        && values
            .iter()
            .all(|scope| bounded(scope, OAUTH_MAX_SCOPE_BYTES))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthSubject {
    User,
    Workload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthAuthorizationRequest {
    pub schema: String,
    pub flow_id: RequestId,
    pub subject: OAuthSubject,
    pub client_id: String,
    pub authorization_endpoint: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub state_digest: String,
    pub code_challenge: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub request_digest: String,
}

impl OAuthAuthorizationRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        flow_id: RequestId,
        subject: OAuthSubject,
        client_id: impl Into<String>,
        authorization_endpoint: impl Into<String>,
        redirect_uri: impl Into<String>,
        scopes: Vec<String>,
        state_digest: impl Into<String>,
        code_challenge: impl Into<String>,
        issued_at_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<Self, String> {
        let expires_at_unix_ms = issued_at_unix_ms
            .checked_add(ttl_ms)
            .ok_or_else(|| "oauth_flow_expiry_invalid".to_owned())?;
        let mut request = Self {
            schema: OAUTH_AUTHORIZATION_SCHEMA.to_owned(),
            flow_id,
            subject,
            client_id: client_id.into(),
            authorization_endpoint: authorization_endpoint.into(),
            redirect_uri: redirect_uri.into(),
            scopes,
            state_digest: state_digest.into(),
            code_challenge: code_challenge.into(),
            issued_at_unix_ms,
            expires_at_unix_ms,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate_at(issued_at_unix_ms)?;
        Ok(request)
    }

    pub fn validate_at(&self, now_unix_ms: u64) -> Result<(), String> {
        if self.schema != OAUTH_AUTHORIZATION_SCHEMA
            || self.flow_id.as_uuid().is_nil()
            || !bounded(&self.client_id, 256)
            || !bounded(&self.authorization_endpoint, 2_048)
            || !bounded(&self.redirect_uri, 2_048)
            || !scopes(&self.scopes)
            || !digest(&self.state_digest)
            || self.code_challenge.len() < 43
            || self.code_challenge.len() > 128
            || !self
                .code_challenge
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || now_unix_ms < self.issued_at_unix_ms
            || now_unix_ms >= self.expires_at_unix_ms
            || self.request_digest != self.digest()
        {
            return Err(if now_unix_ms >= self.expires_at_unix_ms {
                "oauth_flow_expired"
            } else {
                "oauth_authorization_invalid"
            }
            .to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "flow_id": self.flow_id,
            "subject": self.subject,
            "client_id": self.client_id,
            "authorization_endpoint": self.authorization_endpoint,
            "redirect_uri": self.redirect_uri,
            "scopes": self.scopes,
            "state_digest": self.state_digest,
            "code_challenge": self.code_challenge,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthCallback {
    pub schema: String,
    pub flow_id: RequestId,
    pub state: String,
    pub code: String,
    pub redirect_uri: String,
}

impl OAuthCallback {
    pub fn new(
        flow_id: RequestId,
        state: impl Into<String>,
        code: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Result<Self, String> {
        let callback = Self {
            schema: OAUTH_CALLBACK_SCHEMA.to_owned(),
            flow_id,
            state: state.into(),
            code: code.into(),
            redirect_uri: redirect_uri.into(),
        };
        callback.validate()?;
        Ok(callback)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OAUTH_CALLBACK_SCHEMA
            || self.flow_id.as_uuid().is_nil()
            || !bounded(&self.state, 256)
            || !bounded(&self.code, 8_192)
            || !bounded(&self.redirect_uri, 2_048)
        {
            return Err("oauth_callback_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthTokenStatus {
    Active,
    Expiring,
    ReauthRequired,
    Revoked,
    Unknown,
}

impl Default for OAuthTokenStatus {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthTokenMetadata {
    pub schema: String,
    pub provider_account: String,
    pub subject: OAuthSubject,
    pub generation: u64,
    pub scopes: Vec<String>,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub access_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_digest: Option<String>,
    pub status: OAuthTokenStatus,
    pub metadata_digest: String,
}

impl OAuthTokenMetadata {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_account: impl Into<String>,
        subject: OAuthSubject,
        generation: u64,
        scopes: Vec<String>,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        access_digest: impl Into<String>,
        refresh_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut metadata = Self {
            schema: OAUTH_TOKEN_METADATA_SCHEMA.to_owned(),
            provider_account: provider_account.into(),
            subject,
            generation,
            scopes,
            issued_at_unix_ms,
            expires_at_unix_ms,
            access_digest: access_digest.into(),
            refresh_digest,
            status: OAuthTokenStatus::Active,
            metadata_digest: String::new(),
        };
        metadata.metadata_digest = metadata.digest();
        metadata.validate_at(issued_at_unix_ms)?;
        Ok(metadata)
    }

    pub fn validate_at(&self, now_unix_ms: u64) -> Result<(), String> {
        if self.schema != OAUTH_TOKEN_METADATA_SCHEMA
            || !bounded(&self.provider_account, 256)
            || self.generation == 0
            || !scopes(&self.scopes)
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self
                .expires_at_unix_ms
                .saturating_sub(self.issued_at_unix_ms)
                > OAUTH_MAX_TOKEN_TTL_MS
            || !digest(&self.access_digest)
            || self
                .refresh_digest
                .as_deref()
                .is_some_and(|value| !digest(value))
            || self.metadata_digest != self.digest()
        {
            return Err("oauth_token_metadata_invalid".to_owned());
        }
        if now_unix_ms < self.issued_at_unix_ms {
            return Err("oauth_token_clock_regression".to_owned());
        }
        Ok(())
    }

    pub fn needs_refresh(&self, now_unix_ms: u64, skew_ms: u64) -> bool {
        matches!(
            self.status,
            OAuthTokenStatus::Active | OAuthTokenStatus::Expiring
        ) && self.expires_at_unix_ms <= now_unix_ms.saturating_add(skew_ms)
    }

    pub fn rotate(
        &self,
        observed_generation: u64,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        access_digest: impl Into<String>,
        refresh_digest: Option<String>,
        scopes: Vec<String>,
    ) -> Result<Self, String> {
        if observed_generation != self.generation {
            return Err("oauth_generation_conflict".to_owned());
        }
        if matches!(
            self.status,
            OAuthTokenStatus::Revoked | OAuthTokenStatus::ReauthRequired
        ) {
            return Err("oauth_token_status_fenced".to_owned());
        }
        if !self
            .scopes
            .iter()
            .all(|scope| scopes.binary_search(scope).is_ok())
        {
            return Err("oauth_scope_insufficient".to_owned());
        }
        let mut next = Self {
            schema: OAUTH_TOKEN_METADATA_SCHEMA.to_owned(),
            provider_account: self.provider_account.clone(),
            subject: self.subject,
            generation: self
                .generation
                .checked_add(1)
                .ok_or_else(|| "oauth_generation_overflow".to_owned())?,
            scopes,
            issued_at_unix_ms,
            expires_at_unix_ms,
            access_digest: access_digest.into(),
            refresh_digest,
            status: OAuthTokenStatus::Active,
            metadata_digest: String::new(),
        };
        next.metadata_digest = next.digest();
        next.validate_at(issued_at_unix_ms)?;
        Ok(next)
    }

    /// Fence an in-flight refresh and require a new authorization grant.
    pub fn reauth(&self) -> Result<Self, String> {
        let mut next = self.clone();
        next.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "oauth_generation_overflow".to_owned())?;
        next.status = OAuthTokenStatus::ReauthRequired;
        next.metadata_digest = next.digest();
        next.validate_at(self.issued_at_unix_ms)?;
        Ok(next)
    }

    pub fn with_status(&self, status: OAuthTokenStatus) -> Result<Self, String> {
        let mut next = self.clone();
        next.status = status;
        next.metadata_digest = next.digest();
        next.validate_at(self.issued_at_unix_ms)?;
        Ok(next)
    }

    /// Revoke the current generation and advance the fence so an in-flight refresh cannot
    /// reinstall the old credential.
    pub fn revoke(&self) -> Result<Self, String> {
        let mut next = self.clone();
        next.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "oauth_generation_overflow".to_owned())?;
        next.status = OAuthTokenStatus::Revoked;
        next.metadata_digest = next.digest();
        next.validate_at(self.issued_at_unix_ms)?;
        Ok(next)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "provider_account": self.provider_account,
            "subject": self.subject,
            "generation": self.generation,
            "scopes": self.scopes,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "access_digest": self.access_digest,
            "refresh_digest": self.refresh_digest,
            "status": self.status,
        }))
    }
}

pub fn canonical_scopes(values: impl IntoIterator<Item = String>) -> Result<Vec<String>, String> {
    let mut set = BTreeSet::new();
    for value in values {
        if !set.insert(value) {
            return Err("oauth_scope_duplicate".to_owned());
        }
    }
    let values = set.into_iter().collect::<Vec<_>>();
    if !scopes(&values) {
        return Err("oauth_scope_invalid".to_owned());
    }
    Ok(values)
}
