//! Server-owned OAuth account records and compare-and-swap boundaries.
//!
//! This module deliberately carries only provider/account identity, opaque SecretRef metadata,
//! token metadata digests and lifecycle status. Raw access/refresh tokens remain in the provider
//! SecretStore adapter and never enter account records, EventLog facts or query projections.

use crate::{
    json_digest, OAuthSubject, OAuthTokenMetadata, OAuthTokenStatus, ProviderAccountId, SecretRef,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

pub const OAUTH_ACCOUNT_SCHEMA: &str = "kiana.oauth-account.v1";
pub const OAUTH_ACCOUNT_PROJECTION_SCHEMA: &str = "kiana.oauth-account-projection.v1";
pub const OAUTH_ACCOUNT_MAX_SCOPES: usize = 64;
pub const OAUTH_ACCOUNT_SECRET_PURPOSE: &str = "connector.oauth";

/// Server-owned audience binding for an OAuth account SecretRef.
pub fn oauth_account_audience(provider_id: &str) -> String {
    format!("provider:{provider_id}")
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains(['\0', '\r', '\n'])
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn redirect_uri_valid(value: &str) -> bool {
    let Ok(url) = Url::parse(value.trim()) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let loopback = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && (url.scheme() == "https" || (url.scheme() == "http" && loopback))
}

/// Account lifecycle is server-owned. Provider claims or UI state cannot promote an account.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthAccountStatus {
    Active,
    Expired,
    ReauthRequired,
    Revoked,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthAccountRecord {
    pub schema: String,
    pub account_id: ProviderAccountId,
    pub provider_id: String,
    /// Provider account key used only for server-side route binding; it is not a Kiana principal.
    pub provider_account: String,
    pub subject: OAuthSubject,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    /// Exact callback origin/path that was admitted by the provider adapter.
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    /// Only the opaque reference digest crosses this account record boundary.
    pub secret_ref_digest: String,
    pub credential_generation: u64,
    pub token_metadata: OAuthTokenMetadata,
    pub status: OAuthAccountStatus,
    pub revision: u64,
    pub account_digest: String,
}

impl OAuthAccountRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account_id: ProviderAccountId,
        provider_id: impl Into<String>,
        provider_account: impl Into<String>,
        subject: OAuthSubject,
        redirect_uri: impl Into<String>,
        scopes: Vec<String>,
        secret_ref: &SecretRef,
        token_metadata: OAuthTokenMetadata,
    ) -> Result<Self, String> {
        secret_ref.validate()?;
        let provider_id = provider_id.into();
        let provider_account = provider_account.into();
        if secret_ref.purpose != OAUTH_ACCOUNT_SECRET_PURPOSE
            || secret_ref.audience != oauth_account_audience(&provider_id)
        {
            return Err("oauth_account_secret_ref_binding_mismatch".to_owned());
        }
        if token_metadata.provider_account != provider_account {
            return Err("oauth_account_provider_binding_mismatch".to_owned());
        }
        let mut record = Self {
            schema: OAUTH_ACCOUNT_SCHEMA.to_owned(),
            account_id,
            provider_id,
            provider_account,
            subject,
            external_subject: None,
            tenant: None,
            redirect_uri: redirect_uri.into(),
            scopes,
            secret_ref_digest: secret_ref.reference_digest.clone(),
            credential_generation: secret_ref.generation,
            token_metadata,
            status: OAuthAccountStatus::Active,
            revision: 1,
            account_digest: String::new(),
        };
        record.account_digest = record.digest();
        record.validate_at(record.token_metadata.issued_at_unix_ms)?;
        Ok(record)
    }

    pub fn validate_at(&self, now_unix_ms: u64) -> Result<(), String> {
        if self.schema != OAUTH_ACCOUNT_SCHEMA
            || self.account_id.as_uuid().is_nil()
            || !bounded(&self.provider_id, 256)
            || !bounded(&self.provider_account, 256)
            || !bounded(&self.redirect_uri, 2_048)
            || !redirect_uri_valid(&self.redirect_uri)
            || self.scopes.is_empty()
            || self.scopes.len() > OAUTH_ACCOUNT_MAX_SCOPES
            || self.scopes.windows(2).any(|pair| pair[0] >= pair[1])
            || self.scopes.iter().any(|scope| !bounded(scope, 256))
            || !digest(&self.secret_ref_digest)
            || self.credential_generation == 0
            || self.revision == 0
            || self.token_metadata.provider_account != self.provider_account
            || self.token_metadata.subject != self.subject
            || self.token_metadata.scopes != self.scopes
            || self.token_metadata.generation != self.credential_generation
            || self.account_digest != self.digest()
        {
            return Err("oauth_account_invalid".to_owned());
        }
        self.token_metadata.validate_at(now_unix_ms)?;
        if matches!(self.status, OAuthAccountStatus::Active)
            && matches!(
                self.token_metadata.status,
                OAuthTokenStatus::ReauthRequired | OAuthTokenStatus::Revoked
            )
        {
            return Err("oauth_account_status_mismatch".to_owned());
        }
        if matches!(self.status, OAuthAccountStatus::ReauthRequired)
            && self.token_metadata.status != OAuthTokenStatus::ReauthRequired
        {
            return Err("oauth_account_status_mismatch".to_owned());
        }
        if matches!(self.status, OAuthAccountStatus::Revoked)
            && self.token_metadata.status != OAuthTokenStatus::Revoked
        {
            return Err("oauth_account_status_mismatch".to_owned());
        }
        Ok(())
    }

    /// Expiry is derived from metadata and never silently promoted to `Active`.
    pub fn effective_status(&self, now_unix_ms: u64) -> OAuthAccountStatus {
        if matches!(
            self.status,
            OAuthAccountStatus::Active | OAuthAccountStatus::Expired
        ) && self.token_metadata.expires_at_unix_ms <= now_unix_ms
        {
            OAuthAccountStatus::Expired
        } else {
            self.status
        }
    }

    /// Rotate metadata only when both the account revision and token generation are current.
    /// Existing scopes must remain present, so a provider scope downgrade fails closed.
    pub fn rotate(
        &self,
        observed_revision: u64,
        observed_generation: u64,
        next_metadata: OAuthTokenMetadata,
        now_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate_at(now_unix_ms)?;
        next_metadata.validate_at(now_unix_ms)?;
        if observed_revision != self.revision || observed_generation != self.credential_generation {
            return Err("oauth_account_generation_conflict".to_owned());
        }
        if matches!(
            self.status,
            OAuthAccountStatus::Revoked | OAuthAccountStatus::ReauthRequired
        ) {
            return Err("oauth_account_status_fenced".to_owned());
        }
        if next_metadata.provider_account != self.provider_account
            || next_metadata.subject != self.subject
            || next_metadata.generation != self.credential_generation.saturating_add(1)
        {
            return Err("oauth_account_rotation_binding_mismatch".to_owned());
        }
        if next_metadata.scopes.len() < self.scopes.len()
            || !self
                .scopes
                .iter()
                .all(|scope| next_metadata.scopes.binary_search(scope).is_ok())
        {
            return Err("oauth_scope_downgrade".to_owned());
        }
        let mut next = self.clone();
        next.scopes = next_metadata.scopes.clone();
        next.credential_generation = next_metadata.generation;
        next.token_metadata = next_metadata;
        next.status = OAuthAccountStatus::Active;
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "oauth_account_revision_overflow".to_owned())?;
        next.account_digest = next.digest();
        next.validate_at(now_unix_ms)?;
        Ok(next)
    }

    /// Revocation bumps both fences before persistence; an in-flight old refresh cannot win later.
    pub fn revoke(
        &self,
        observed_revision: u64,
        observed_generation: u64,
        now_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate_at(now_unix_ms)?;
        if observed_revision != self.revision || observed_generation != self.credential_generation {
            return Err("oauth_account_generation_conflict".to_owned());
        }
        let mut next = self.clone();
        next.token_metadata = self
            .token_metadata
            .revoke()
            .map_err(|_| "oauth_account_revision_overflow".to_owned())?;
        next.credential_generation = next.token_metadata.generation;
        next.status = OAuthAccountStatus::Revoked;
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "oauth_account_revision_overflow".to_owned())?;
        next.account_digest = next.digest();
        next.validate_at(now_unix_ms)?;
        Ok(next)
    }

    /// Reauthentication fences an in-flight refresh while retaining no raw token material.
    pub fn require_reauth(
        &self,
        observed_revision: u64,
        observed_generation: u64,
        now_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate_at(now_unix_ms)?;
        if observed_revision != self.revision || observed_generation != self.credential_generation {
            return Err("oauth_account_generation_conflict".to_owned());
        }
        let metadata = self
            .token_metadata
            .reauth()
            .map_err(|_| "oauth_account_revision_overflow".to_owned())?;
        let mut next = self.clone();
        next.token_metadata = metadata;
        next.credential_generation = next.token_metadata.generation;
        next.status = OAuthAccountStatus::ReauthRequired;
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "oauth_account_revision_overflow".to_owned())?;
        next.account_digest = next.digest();
        next.validate_at(now_unix_ms)?;
        Ok(next)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "account_id": self.account_id,
            "provider_id": self.provider_id,
            "provider_account": self.provider_account,
            "subject": self.subject,
            "external_subject": self.external_subject,
            "tenant": self.tenant,
            "redirect_uri": self.redirect_uri,
            "scopes": self.scopes,
            "secret_ref_digest": self.secret_ref_digest,
            "credential_generation": self.credential_generation,
            "token_metadata": self.token_metadata,
            "status": self.status,
            "revision": self.revision,
        }))
    }
}

/// A redaction-safe account view intended for query/UI projections.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthAccountProjection {
    pub schema: String,
    pub account_id: ProviderAccountId,
    pub provider_id: String,
    pub provider_account: String,
    pub subject: OAuthSubject,
    pub external_subject: Option<String>,
    pub tenant: Option<String>,
    pub scopes: Vec<String>,
    pub credential_generation: u64,
    pub status: OAuthAccountStatus,
    pub revision: u64,
    pub account_digest: String,
    pub stale: bool,
    pub proof_level: String,
    pub limitations: Vec<String>,
}

impl OAuthAccountProjection {
    pub fn from_record(record: &OAuthAccountRecord, now_unix_ms: u64) -> Result<Self, String> {
        record.validate_at(now_unix_ms)?;
        Ok(Self {
            schema: OAUTH_ACCOUNT_PROJECTION_SCHEMA.to_owned(),
            account_id: record.account_id,
            provider_id: record.provider_id.clone(),
            provider_account: record.provider_account.clone(),
            subject: record.subject,
            external_subject: record.external_subject.clone(),
            tenant: record.tenant.clone(),
            scopes: record.scopes.clone(),
            credential_generation: record.credential_generation,
            status: record.effective_status(now_unix_ms),
            revision: record.revision,
            account_digest: record.account_digest.clone(),
            stale: false,
            proof_level: "source".to_owned(),
            limitations: vec![
                "provider_account_metadata_only".to_owned(),
                "raw_token_never_projected".to_owned(),
            ],
        })
    }
}
