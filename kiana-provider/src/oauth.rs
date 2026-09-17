//! Bounded OAuth/workload identity lifecycle at the provider boundary.
//!
//! The domain only sees flow/token metadata and digests.  This module is the sole owner of raw
//! access/refresh token strings, and only its effect-scoped `AccessTokenMaterial` may be handed to
//! an outbound adapter.  Refresh, rotation and revoke all use generation compare-and-swap.
#![allow(dead_code)]

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use kiana_domain::{
    canonical_scopes, json_digest, OAuthAuthorizationRequest, OAuthCallback, OAuthSubject,
    OAuthTokenMetadata, OAuthTokenStatus, RequestId, OAUTH_MAX_TOKEN_TTL_MS,
};
use ring::{
    digest,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

pub(crate) const OAUTH_FLOW_TTL_MS: u64 = 10 * 60 * 1_000;
pub(crate) const OAUTH_REFRESH_SKEW_MS: u64 = 5 * 60 * 1_000;
const OAUTH_REFRESH_COOLDOWN_MS: u64 = 1_000;
const MAX_TOKEN_BYTES: usize = 8 * 1024;
const MAX_TOKEN_RESPONSE_BYTES: usize = 16 * 1024;
const MAX_PENDING_FLOWS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OAuthClientConfig {
    pub provider_account: String,
    pub subject: OAuthSubject,
    pub client_id: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

impl OAuthClientConfig {
    fn validate(&self) -> Result<(), OAuthError> {
        if self.provider_account.trim().is_empty()
            || self.provider_account.len() > 256
            || self.provider_account.contains(['\0', '\r', '\n'])
            || self.client_id.trim().is_empty()
            || self.client_id.len() > 256
            || self.client_id.contains(['\0', '\r', '\n'])
            || self.redirect_uri.trim().is_empty()
            || self.redirect_uri.len() > 2_048
            || self.redirect_uri.contains(['\0', '\r', '\n'])
        {
            return Err(OAuthError::Invalid("oauth_client_config_invalid"));
        }
        canonical_endpoint(&self.authorization_endpoint)?;
        canonical_endpoint(&self.token_endpoint)?;
        let redirect = reqwest::Url::parse(self.redirect_uri.trim())
            .map_err(|_| OAuthError::Invalid("oauth_redirect_uri_invalid"))?;
        if !redirect.username().is_empty()
            || redirect.password().is_some()
            || redirect.query().is_some()
            || redirect.fragment().is_some()
        {
            return Err(OAuthError::Invalid("oauth_redirect_uri_invalid"));
        }
        canonical_scopes(self.scopes.clone())
            .map_err(|_| OAuthError::Invalid("oauth_scope_invalid"))?;
        Ok(())
    }
}

fn canonical_endpoint(value: &str) -> Result<reqwest::Url, OAuthError> {
    let endpoint = reqwest::Url::parse(value.trim())
        .map_err(|_| OAuthError::Invalid("oauth_endpoint_invalid"))?;
    let local = endpoint.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if endpoint.username().is_empty()
        && endpoint.password().is_none()
        && (endpoint.scheme() == "https" || endpoint.scheme() == "http" && local)
    {
        Ok(endpoint)
    } else {
        Err(OAuthError::Invalid(
            "oauth_endpoint_requires_tls_or_loopback",
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OAuthError {
    Missing,
    Expired,
    Invalid(&'static str),
    Transient,
    ReauthRequired,
    Revoked,
    GenerationConflict,
    ScopeInsufficient,
    Storage,
}

impl OAuthError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Missing => "oauth_token_missing",
            Self::Expired => "oauth_token_expired",
            Self::Invalid(code) => code,
            Self::Transient => "oauth_refresh_transient",
            Self::ReauthRequired => "oauth_reauth_required",
            Self::Revoked => "oauth_credential_revoked",
            Self::GenerationConflict => "oauth_generation_conflict",
            Self::ScopeInsufficient => "oauth_scope_insufficient",
            Self::Storage => "oauth_token_storage_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RefreshFailure {
    Transient,
    Permanent,
    Revoked,
}

#[derive(Clone, Debug)]
pub(crate) struct RawTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub scope: Option<String>,
    pub token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTokenWire {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
}

fn decode_token_response(bytes: &[u8]) -> Result<RawTokenResponse, OAuthError> {
    if bytes.len() > MAX_TOKEN_RESPONSE_BYTES {
        return Err(OAuthError::Invalid("oauth_token_response_too_large"));
    }
    let value: RawTokenWire = serde_json::from_slice(bytes)
        .map_err(|_| OAuthError::Invalid("oauth_token_response_invalid"))?;
    Ok(RawTokenResponse {
        access_token: value.access_token,
        refresh_token: value.refresh_token,
        expires_in: value.expires_in,
        scope: value.scope,
        token_type: value.token_type,
    })
}

#[derive(Clone, Debug)]
struct TokenMaterial {
    access_token: String,
    refresh_token: Option<String>,
    expires_at_unix_ms: u64,
    scopes: Vec<String>,
}

#[derive(Clone, Debug)]
struct StoredTokens {
    metadata: OAuthTokenMetadata,
    access_token: String,
    refresh_token: Option<String>,
}

struct PendingFlow {
    request: OAuthAuthorizationRequest,
    state: String,
    verifier: String,
}

#[derive(Default)]
struct OAuthState {
    pending: std::collections::BTreeMap<RequestId, PendingFlow>,
    tokens: Option<StoredTokens>,
    refreshing: bool,
    refresh_cooldown_until: u64,
}

#[derive(Clone)]
pub(crate) struct OAuthManager {
    config: OAuthClientConfig,
    path: Option<PathBuf>,
    state: Arc<Mutex<OAuthState>>,
    refresh_notify: Arc<Notify>,
}

/// Raw access material scoped to one provider effect.  Drop clears the owned buffer; no domain or
/// EventLog contract can serialize this type.
pub(crate) struct AccessTokenMaterial {
    pub(crate) value: String,
    pub(crate) metadata: OAuthTokenMetadata,
}

impl Drop for AccessTokenMaterial {
    fn drop(&mut self) {
        self.value.clear();
    }
}

impl OAuthManager {
    pub(crate) fn new(
        mut config: OAuthClientConfig,
        path: Option<PathBuf>,
    ) -> Result<Self, OAuthError> {
        config.validate()?;
        config.scopes = canonical_scopes(config.scopes.clone())
            .map_err(|_| OAuthError::Invalid("oauth_scope_invalid"))?;
        Ok(Self {
            config,
            path,
            state: Arc::new(Mutex::new(OAuthState::default())),
            refresh_notify: Arc::new(Notify::new()),
        })
    }

    pub(crate) fn load(
        config: OAuthClientConfig,
        path: impl Into<PathBuf>,
    ) -> Result<Self, OAuthError> {
        let manager = Self::new(config, Some(path.into()))?;
        let path = manager.path.as_deref().ok_or(OAuthError::Storage)?;
        if !path.exists() {
            return Ok(manager);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path)
                .map_err(|_| OAuthError::Storage)?
                .permissions()
                .mode()
                & 0o777;
            if mode & 0o077 != 0 {
                return Err(OAuthError::Invalid("oauth_token_file_permissions"));
            }
        }
        let bytes = std::fs::read(path).map_err(|_| OAuthError::Storage)?;
        if bytes.len() > MAX_TOKEN_RESPONSE_BYTES {
            return Err(OAuthError::Invalid("oauth_token_file_too_large"));
        }
        let file: TokenFile = serde_json::from_slice(&bytes)
            .map_err(|_| OAuthError::Invalid("oauth_token_file_invalid"))?;
        let stored = file.into_stored(&manager.config)?;
        manager.state.lock().unwrap().tokens = Some(stored);
        Ok(manager)
    }

    pub(crate) fn start_authorization(
        &self,
        now_unix_ms: u64,
    ) -> Result<(OAuthAuthorizationRequest, String), OAuthError> {
        let csrf_state = random_urlsafe(32)?;
        let verifier = random_urlsafe(32)?;
        let request = OAuthAuthorizationRequest::new(
            RequestId::new(),
            self.config.subject,
            self.config.client_id.clone(),
            self.config.authorization_endpoint.clone(),
            self.config.redirect_uri.clone(),
            self.config.scopes.clone(),
            json_digest(&serde_json::json!(csrf_state)),
            pkce_challenge(&verifier)?,
            now_unix_ms,
            OAUTH_FLOW_TTL_MS,
        )
        .map_err(|_| OAuthError::Invalid("oauth_authorization_invalid"))?;
        let mut url = canonical_endpoint(&self.config.authorization_endpoint)?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("response_type", "code")
                .append_pair("client_id", &self.config.client_id)
                .append_pair("redirect_uri", &self.config.redirect_uri)
                .append_pair("scope", &self.config.scopes.join(" "))
                .append_pair("state", &csrf_state)
                .append_pair("code_challenge", &request.code_challenge)
                .append_pair("code_challenge_method", "S256");
        }
        let mut store = self.state.lock().unwrap();
        store.pending.insert(
            request.flow_id,
            PendingFlow {
                request: request.clone(),
                state: csrf_state,
                verifier,
            },
        );
        while store.pending.len() > MAX_PENDING_FLOWS {
            let oldest = store
                .pending
                .iter()
                .min_by_key(|(_, flow)| flow.request.issued_at_unix_ms)
                .map(|(id, _)| *id);
            if let Some(oldest) = oldest {
                store.pending.remove(&oldest);
            } else {
                break;
            }
        }
        Ok((request, url.to_string()))
    }

    fn consume_callback(
        &self,
        callback: &OAuthCallback,
        now_unix_ms: u64,
    ) -> Result<String, OAuthError> {
        callback
            .validate()
            .map_err(|_| OAuthError::Invalid("oauth_callback_invalid"))?;
        let mut state = self.state.lock().unwrap();
        let pending = state
            .pending
            .get(&callback.flow_id)
            .ok_or(OAuthError::Invalid("oauth_flow_unknown"))?;
        pending
            .request
            .validate_at(now_unix_ms)
            .map_err(|error| match error.as_str() {
                "oauth_flow_expired" => OAuthError::Expired,
                _ => OAuthError::Invalid("oauth_authorization_invalid"),
            })?;
        if pending.state != callback.state {
            return Err(OAuthError::Invalid("oauth_state_mismatch"));
        }
        if pending.request.redirect_uri != callback.redirect_uri {
            return Err(OAuthError::Invalid("oauth_redirect_mismatch"));
        }
        let pending = state
            .pending
            .remove(&callback.flow_id)
            .ok_or(OAuthError::Invalid("oauth_flow_replay"))?;
        if pkce_challenge(&pending.verifier)? != pending.request.code_challenge {
            return Err(OAuthError::Invalid("oauth_pkce_mismatch"));
        }
        Ok(pending.verifier)
    }

    pub(crate) fn complete_authorization(
        &self,
        callback: OAuthCallback,
        response: RawTokenResponse,
        now_unix_ms: u64,
    ) -> Result<OAuthTokenMetadata, OAuthError> {
        let _verifier = self.consume_callback(&callback, now_unix_ms)?;
        let material = parse_token_response(response, now_unix_ms, &self.config.scopes, None)?;
        self.store_material(material, now_unix_ms)
    }

    /// Exchange a validated callback through an injected token endpoint.  The closure receives
    /// the one-time authorization code, PKCE verifier and exact redirect URI; the provider module
    /// never exposes the resulting raw response beyond its parser.
    pub(crate) async fn exchange_authorization<F, Fut>(
        &self,
        callback: OAuthCallback,
        now_unix_ms: u64,
        exchange: F,
    ) -> Result<OAuthTokenMetadata, OAuthError>
    where
        F: FnOnce(String, String, String) -> Fut + Send,
        Fut: Future<Output = Result<Vec<u8>, RefreshFailure>> + Send,
    {
        let verifier = self.consume_callback(&callback, now_unix_ms)?;
        let bytes = exchange(callback.code, verifier, callback.redirect_uri)
            .await
            .map_err(|failure| match failure {
                RefreshFailure::Transient => OAuthError::Transient,
                RefreshFailure::Permanent => OAuthError::ReauthRequired,
                RefreshFailure::Revoked => OAuthError::Revoked,
            })?;
        let response = decode_token_response(&bytes)?;
        let material = parse_token_response(response, now_unix_ms, &self.config.scopes, None)?;
        self.store_material(material, now_unix_ms)
    }

    fn store_material(
        &self,
        material: TokenMaterial,
        now_unix_ms: u64,
    ) -> Result<OAuthTokenMetadata, OAuthError> {
        let metadata = OAuthTokenMetadata::new(
            self.config.provider_account.clone(),
            self.config.subject,
            1,
            material.scopes.clone(),
            now_unix_ms,
            material.expires_at_unix_ms,
            json_digest(&serde_json::json!(&material.access_token)),
            material
                .refresh_token
                .as_ref()
                .map(|token| json_digest(&serde_json::json!(token))),
        )
        .map_err(|_| OAuthError::Invalid("oauth_token_metadata_invalid"))?;
        let stored = StoredTokens {
            metadata: metadata.clone(),
            access_token: material.access_token,
            refresh_token: material.refresh_token,
        };
        self.persist(&stored)?;
        self.state.lock().unwrap().tokens = Some(stored);
        Ok(metadata)
    }

    pub(crate) fn complete_authorization_json(
        &self,
        callback: OAuthCallback,
        response: &[u8],
        now_unix_ms: u64,
    ) -> Result<OAuthTokenMetadata, OAuthError> {
        let _verifier = self.consume_callback(&callback, now_unix_ms)?;
        let response = decode_token_response(response)?;
        let material = parse_token_response(response, now_unix_ms, &self.config.scopes, None)?;
        self.store_material(material, now_unix_ms)
    }

    pub(crate) fn install_tokens(
        &self,
        access_token: String,
        refresh_token: Option<String>,
        expires_at_unix_ms: u64,
        now_unix_ms: u64,
    ) -> Result<OAuthTokenMetadata, OAuthError> {
        let material = validate_token_values(
            access_token,
            refresh_token,
            expires_at_unix_ms,
            now_unix_ms,
            self.config.scopes.clone(),
        )?;
        self.store_material(material, now_unix_ms)
    }

    pub(crate) async fn access_token<F, Fut>(
        &self,
        now_unix_ms: u64,
        skew_ms: u64,
        refresh: F,
    ) -> Result<AccessTokenMaterial, OAuthError>
    where
        F: FnOnce(String) -> Fut + Send,
        Fut: Future<Output = Result<RawTokenResponse, RefreshFailure>> + Send,
    {
        loop {
            let notified = self.refresh_notify.notified();
            let leader = {
                let mut state = self.state.lock().unwrap();
                let stored = state.tokens.as_ref().ok_or(OAuthError::Missing)?.clone();
                if stored.metadata.status == OAuthTokenStatus::Revoked {
                    return Err(OAuthError::Revoked);
                }
                if matches!(
                    stored.metadata.status,
                    OAuthTokenStatus::ReauthRequired | OAuthTokenStatus::Unknown
                ) {
                    return Err(OAuthError::ReauthRequired);
                }
                if stored.metadata.expires_at_unix_ms > now_unix_ms
                    && (!stored.metadata.needs_refresh(now_unix_ms, skew_ms)
                        || state.refresh_cooldown_until > now_unix_ms)
                {
                    return Ok(AccessTokenMaterial {
                        value: stored.access_token.clone(),
                        metadata: stored.metadata.clone(),
                    });
                }
                let refresh_token = stored.refresh_token.clone().ok_or_else(|| {
                    if stored.metadata.expires_at_unix_ms <= now_unix_ms {
                        OAuthError::Expired
                    } else {
                        OAuthError::ReauthRequired
                    }
                })?;
                if state.refreshing {
                    None
                } else {
                    state.refreshing = true;
                    Some((stored.metadata.generation, refresh_token))
                }
            };
            let Some((generation, refresh_token)) = leader else {
                notified.await;
                continue;
            };
            let result = refresh(refresh_token).await;
            return self.finish_refresh(generation, now_unix_ms, result).await;
        }
    }

    async fn finish_refresh(
        &self,
        generation: u64,
        now_unix_ms: u64,
        result: Result<RawTokenResponse, RefreshFailure>,
    ) -> Result<AccessTokenMaterial, OAuthError> {
        let mut state = self.state.lock().unwrap();
        let current = state.tokens.as_ref().ok_or(OAuthError::Missing)?.clone();
        let outcome = match result {
            Ok(response) => {
                if current.metadata.generation != generation {
                    Err(OAuthError::GenerationConflict)
                } else {
                    let material = parse_token_response(
                        response,
                        now_unix_ms,
                        &self.config.scopes,
                        current.refresh_token.as_deref(),
                    );
                    match material {
                        Err(error) => Err(error),
                        Ok(material) => {
                            let metadata = current
                                .metadata
                                .rotate(
                                    generation,
                                    now_unix_ms,
                                    material.expires_at_unix_ms,
                                    json_digest(&serde_json::json!(&material.access_token)),
                                    material
                                        .refresh_token
                                        .as_ref()
                                        .map(|token| json_digest(&serde_json::json!(token))),
                                    material.scopes.clone(),
                                )
                                .map_err(|error| match error.as_str() {
                                    "oauth_generation_conflict" => OAuthError::GenerationConflict,
                                    "oauth_scope_insufficient" => OAuthError::ScopeInsufficient,
                                    "oauth_token_status_fenced"
                                        if current.metadata.status == OAuthTokenStatus::Revoked =>
                                    {
                                        OAuthError::Revoked
                                    }
                                    "oauth_token_status_fenced" => OAuthError::ReauthRequired,
                                    _ => OAuthError::Invalid("oauth_token_metadata_invalid"),
                                });
                            match metadata {
                                Err(error) => Err(error),
                                Ok(metadata) => {
                                    let stored = StoredTokens {
                                        metadata: metadata.clone(),
                                        access_token: material.access_token,
                                        refresh_token: material.refresh_token,
                                    };
                                    match self.persist(&stored) {
                                        Err(error) => Err(error),
                                        Ok(()) => {
                                            state.tokens = Some(stored.clone());
                                            state.refresh_cooldown_until = 0;
                                            Ok(AccessTokenMaterial {
                                                value: stored.access_token,
                                                metadata,
                                            })
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(RefreshFailure::Transient) => {
                if current.metadata.generation != generation {
                    Err(OAuthError::GenerationConflict)
                } else if current.metadata.expires_at_unix_ms > now_unix_ms {
                    state.refresh_cooldown_until =
                        now_unix_ms.saturating_add(OAUTH_REFRESH_COOLDOWN_MS);
                    Ok(AccessTokenMaterial {
                        value: current.access_token,
                        metadata: current.metadata,
                    })
                } else {
                    Err(OAuthError::Expired)
                }
            }
            Err(RefreshFailure::Permanent) => {
                if current.metadata.generation != generation {
                    Err(OAuthError::GenerationConflict)
                } else {
                    let metadata = current
                        .metadata
                        .with_status(OAuthTokenStatus::ReauthRequired)
                        .map_err(|_| OAuthError::Invalid("oauth_token_metadata_invalid"))?;
                    let stored = StoredTokens {
                        metadata,
                        access_token: current.access_token,
                        refresh_token: None,
                    };
                    match self.persist(&stored) {
                        Err(error) => Err(error),
                        Ok(()) => {
                            state.tokens = Some(stored);
                            Err(OAuthError::ReauthRequired)
                        }
                    }
                }
            }
            Err(RefreshFailure::Revoked) => {
                if current.metadata.generation != generation {
                    Err(OAuthError::GenerationConflict)
                } else {
                    let metadata = current
                        .metadata
                        .revoke()
                        .map_err(|_| OAuthError::Invalid("oauth_token_metadata_invalid"))?;
                    let stored = StoredTokens {
                        metadata,
                        access_token: current.access_token,
                        refresh_token: None,
                    };
                    match self.persist(&stored) {
                        Err(error) => Err(error),
                        Ok(()) => {
                            state.tokens = Some(stored);
                            Err(OAuthError::Revoked)
                        }
                    }
                }
            }
        };
        state.refreshing = false;
        self.refresh_notify.notify_waiters();
        outcome
    }

    pub(crate) async fn rotate(
        &self,
        observed_generation: u64,
        access_token: String,
        refresh_token: Option<String>,
        expires_at_unix_ms: u64,
        scopes: Vec<String>,
        now_unix_ms: u64,
    ) -> Result<OAuthTokenMetadata, OAuthError> {
        let mut state = self.state.lock().unwrap();
        let current = state.tokens.as_ref().ok_or(OAuthError::Missing)?.clone();
        let material = validate_token_values(
            access_token,
            refresh_token,
            expires_at_unix_ms,
            now_unix_ms,
            scopes,
        )?;
        let metadata = current
            .metadata
            .rotate(
                observed_generation,
                now_unix_ms,
                material.expires_at_unix_ms,
                json_digest(&serde_json::json!(&material.access_token)),
                material
                    .refresh_token
                    .as_ref()
                    .map(|token| json_digest(&serde_json::json!(token))),
                material.scopes.clone(),
            )
            .map_err(|error| match error.as_str() {
                "oauth_generation_conflict" => OAuthError::GenerationConflict,
                "oauth_scope_insufficient" => OAuthError::ScopeInsufficient,
                "oauth_token_status_fenced"
                    if current.metadata.status == OAuthTokenStatus::Revoked =>
                {
                    OAuthError::Revoked
                }
                "oauth_token_status_fenced" => OAuthError::ReauthRequired,
                _ => OAuthError::Invalid("oauth_token_metadata_invalid"),
            })?;
        let stored = StoredTokens {
            metadata: metadata.clone(),
            access_token: material.access_token,
            refresh_token: material.refresh_token,
        };
        self.persist(&stored)?;
        state.tokens = Some(stored);
        state.refresh_cooldown_until = 0;
        self.refresh_notify.notify_waiters();
        Ok(metadata)
    }

    pub(crate) async fn revoke(&self) -> Result<OAuthTokenMetadata, OAuthError> {
        let mut state = self.state.lock().unwrap();
        let current = state.tokens.as_ref().ok_or(OAuthError::Missing)?.clone();
        let metadata = current
            .metadata
            .revoke()
            .map_err(|_| OAuthError::Invalid("oauth_token_metadata_invalid"))?;
        let stored = StoredTokens {
            metadata: metadata.clone(),
            access_token: current.access_token,
            refresh_token: None,
        };
        self.persist(&stored)?;
        state.tokens = Some(stored);
        state.refresh_cooldown_until = 0;
        self.refresh_notify.notify_waiters();
        Ok(metadata)
    }

    pub(crate) fn metadata(&self) -> Option<OAuthTokenMetadata> {
        self.state
            .lock()
            .unwrap()
            .tokens
            .as_ref()
            .map(|stored| stored.metadata.clone())
    }

    fn persist(&self, tokens: &StoredTokens) -> Result<(), OAuthError> {
        if let Some(path) = &self.path {
            save_token_file(path, &self.config, tokens)?;
        }
        Ok(())
    }
}

fn random_urlsafe(bytes: usize) -> Result<String, OAuthError> {
    let mut value = vec![0_u8; bytes];
    SystemRandom::new()
        .fill(&mut value)
        .map_err(|_| OAuthError::Invalid("oauth_randomness_unavailable"))?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

fn pkce_challenge(verifier: &str) -> Result<String, OAuthError> {
    if !(43..=128).contains(&verifier.len())
        || !verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
    {
        return Err(OAuthError::Invalid("oauth_pkce_verifier_invalid"));
    }
    Ok(URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, verifier.as_bytes())))
}

fn validate_token_values(
    access_token: String,
    refresh_token: Option<String>,
    expires_at_unix_ms: u64,
    now_unix_ms: u64,
    scopes: Vec<String>,
) -> Result<TokenMaterial, OAuthError> {
    if access_token.trim().is_empty()
        || access_token.len() > MAX_TOKEN_BYTES
        || access_token.contains(['\0', '\r', '\n'])
        || refresh_token.as_deref().is_some_and(|token| {
            token.trim().is_empty()
                || token.len() > MAX_TOKEN_BYTES
                || token.contains(['\0', '\r', '\n'])
        })
        || expires_at_unix_ms <= now_unix_ms
        || expires_at_unix_ms - now_unix_ms > OAUTH_MAX_TOKEN_TTL_MS
    {
        return Err(OAuthError::Invalid("oauth_token_response_invalid"));
    }
    let scopes =
        canonical_scopes(scopes).map_err(|_| OAuthError::Invalid("oauth_scope_invalid"))?;
    Ok(TokenMaterial {
        access_token,
        refresh_token,
        expires_at_unix_ms,
        scopes,
    })
}

fn parse_token_response(
    response: RawTokenResponse,
    now_unix_ms: u64,
    required_scopes: &[String],
    previous_refresh: Option<&str>,
) -> Result<TokenMaterial, OAuthError> {
    if response
        .token_type
        .as_deref()
        .is_some_and(|kind| !kind.eq_ignore_ascii_case("bearer"))
    {
        return Err(OAuthError::Invalid("oauth_token_type_invalid"));
    }
    let expires_in = response
        .expires_in
        .filter(|seconds| *seconds > 0)
        .ok_or(OAuthError::Invalid("oauth_token_expiry_missing"))?;
    let expires_ms = expires_in
        .checked_mul(1_000)
        .and_then(|ttl| now_unix_ms.checked_add(ttl))
        .ok_or(OAuthError::Invalid("oauth_token_expiry_invalid"))?;
    let refresh_token = response
        .refresh_token
        .or_else(|| previous_refresh.map(str::to_owned));
    let scopes = if let Some(scope) = response.scope.filter(|scope| !scope.trim().is_empty()) {
        canonical_scopes(scope.split_whitespace().map(str::to_owned))
            .map_err(|_| OAuthError::ScopeInsufficient)?
    } else {
        required_scopes.to_vec()
    };
    if !required_scopes
        .iter()
        .all(|scope| scopes.binary_search(scope).is_ok())
    {
        return Err(OAuthError::ScopeInsufficient);
    }
    validate_token_values(
        response.access_token,
        refresh_token,
        expires_ms,
        now_unix_ms,
        scopes,
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenFile {
    schema: String,
    provider_account: String,
    subject: OAuthSubject,
    generation: u64,
    scopes: Vec<String>,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    #[serde(default)]
    status: OAuthTokenStatus,
    access_token: String,
    refresh_token: Option<String>,
}

impl TokenFile {
    fn from_stored(config: &OAuthClientConfig, stored: &StoredTokens) -> Self {
        Self {
            schema: kiana_domain::OAUTH_TOKEN_FILE_SCHEMA.to_owned(),
            provider_account: config.provider_account.clone(),
            subject: config.subject,
            generation: stored.metadata.generation,
            scopes: stored.metadata.scopes.clone(),
            issued_at_unix_ms: stored.metadata.issued_at_unix_ms,
            expires_at_unix_ms: stored.metadata.expires_at_unix_ms,
            status: stored.metadata.status,
            access_token: stored.access_token.clone(),
            refresh_token: stored.refresh_token.clone(),
        }
    }

    fn into_stored(self, config: &OAuthClientConfig) -> Result<StoredTokens, OAuthError> {
        if self.schema != kiana_domain::OAUTH_TOKEN_FILE_SCHEMA
            || self.provider_account != config.provider_account
            || self.subject != config.subject
        {
            return Err(OAuthError::Invalid("oauth_token_file_binding_invalid"));
        }
        let metadata = OAuthTokenMetadata::new(
            self.provider_account,
            self.subject,
            self.generation,
            self.scopes,
            self.issued_at_unix_ms,
            self.expires_at_unix_ms,
            json_digest(&serde_json::json!(&self.access_token)),
            self.refresh_token
                .as_ref()
                .map(|token| json_digest(&serde_json::json!(token))),
        )
        .map_err(|_| OAuthError::Invalid("oauth_token_file_invalid"))?;
        let metadata = if self.status == OAuthTokenStatus::Active {
            metadata
        } else {
            metadata
                .with_status(self.status)
                .map_err(|_| OAuthError::Invalid("oauth_token_file_invalid"))?
        };
        validate_token_values(
            self.access_token.clone(),
            self.refresh_token.clone(),
            self.expires_at_unix_ms,
            self.issued_at_unix_ms,
            metadata.scopes.clone(),
        )?;
        Ok(StoredTokens {
            metadata,
            access_token: self.access_token,
            refresh_token: self.refresh_token,
        })
    }
}

fn save_token_file(
    path: &Path,
    config: &OAuthClientConfig,
    tokens: &StoredTokens,
) -> Result<(), OAuthError> {
    let target_is_symlink = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata.file_type().is_symlink(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    };
    if path.as_os_str().is_empty() || target_is_symlink {
        return Err(OAuthError::Invalid("oauth_token_file_path_invalid"));
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        let existed = parent.exists();
        std::fs::create_dir_all(parent).map_err(|_| OAuthError::Storage)?;
        if !existed {
            restrict_directory(parent)?;
        }
    }
    let temp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("oauth"),
        RequestId::new()
    ));
    let file = TokenFile::from_stored(config, tokens);
    let bytes = serde_json::to_vec(&file).map_err(|_| OAuthError::Storage)?;
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        set_owner_only_mode(&mut options);
        let mut handle = options.open(&temp).map_err(|_| OAuthError::Storage)?;
        use std::io::Write;
        handle.write_all(&bytes).map_err(|_| OAuthError::Storage)?;
        handle.sync_all().map_err(|_| OAuthError::Storage)?;
        drop(handle);
        restrict_file(&temp)?;
        std::fs::rename(&temp, path).map_err(|_| OAuthError::Storage)?;
        restrict_file(path)?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            if let Ok(directory) = std::fs::File::open(parent) {
                let _ = directory.sync_all();
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(unix)]
fn set_owner_only_mode(options: &mut std::fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_owner_only_mode(_options: &mut std::fs::OpenOptions) {}

#[cfg(unix)]
fn restrict_file(path: &Path) -> Result<(), OAuthError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| OAuthError::Storage)
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> Result<(), OAuthError> {
    Ok(())
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<(), OAuthError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| OAuthError::Storage)
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<(), OAuthError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    fn config() -> OAuthClientConfig {
        OAuthClientConfig {
            provider_account: "fake-account".to_owned(),
            subject: OAuthSubject::Workload,
            client_id: "client-ci09".to_owned(),
            authorization_endpoint: "http://127.0.0.1:22109/authorize".to_owned(),
            token_endpoint: "http://127.0.0.1:22109/token".to_owned(),
            redirect_uri: "http://127.0.0.1:22109/callback".to_owned(),
            scopes: vec!["model.use".to_owned(), "model.read".to_owned()],
        }
    }

    fn response(access: &str, refresh: Option<&str>, expires_in: u64) -> RawTokenResponse {
        RawTokenResponse {
            access_token: access.to_owned(),
            refresh_token: refresh.map(str::to_owned),
            expires_in: Some(expires_in),
            scope: Some("model.read model.use".to_owned()),
            token_type: Some("Bearer".to_owned()),
        }
    }

    #[tokio::test]
    async fn pkce_state_and_redirect_are_single_use_and_fail_closed() {
        let manager = OAuthManager::new(config(), None).expect("manager");
        let (request, url) = manager.start_authorization(1_000).expect("flow");
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state="));
        let callback = OAuthCallback::new(
            request.flow_id,
            "wrong-state",
            "authorization-code",
            request.redirect_uri.clone(),
        )
        .expect("callback");
        assert_eq!(
            manager
                .complete_authorization(callback, response("access", Some("refresh"), 3_600), 1_001)
                .unwrap_err()
                .code(),
            "oauth_state_mismatch"
        );
        let state = manager.state.lock().unwrap();
        let expected = state
            .pending
            .get(&request.flow_id)
            .expect("pending flow")
            .state
            .clone();
        drop(state);
        let callback = OAuthCallback::new(
            request.flow_id,
            expected,
            "authorization-code",
            request.redirect_uri.clone(),
        )
        .expect("callback");
        manager
            .complete_authorization(
                callback.clone(),
                response("access", Some("refresh"), 3_600),
                1_001,
            )
            .expect("exchange");
        assert_eq!(
            manager
                .complete_authorization(
                    callback,
                    response("access-2", Some("refresh-2"), 3_600),
                    1_002
                )
                .unwrap_err()
                .code(),
            "oauth_flow_unknown"
        );

        let (request, _) = manager.start_authorization(2_000).expect("second flow");
        let state = manager
            .state
            .lock()
            .unwrap()
            .pending
            .get(&request.flow_id)
            .expect("second pending")
            .state
            .clone();
        let expected_challenge = request.code_challenge.clone();
        let callback = OAuthCallback::new(
            request.flow_id,
            state,
            "authorization-code-2",
            request.redirect_uri.clone(),
        )
        .expect("second callback");
        manager
            .exchange_authorization(
                callback,
                2_001,
                move |code, verifier, redirect| async move {
                    assert_eq!(code, "authorization-code-2");
                    assert_eq!(redirect, "http://127.0.0.1:22109/callback");
                    assert_eq!(
                        pkce_challenge(&verifier).expect("verifier"),
                        expected_challenge
                    );
                    Ok(serde_json::to_vec(&serde_json::json!({
                        "access_token": "exchanged-access",
                        "refresh_token": "exchanged-refresh",
                        "expires_in": 3_600,
                        "scope": "model.read model.use",
                        "token_type": "Bearer"
                    }))
                    .expect("token response"))
                },
            )
            .await
            .expect("PKCE exchange");

        let (request, _) = manager.start_authorization(3_000).expect("redirect flow");
        let state = manager
            .state
            .lock()
            .unwrap()
            .pending
            .get(&request.flow_id)
            .expect("redirect pending")
            .state
            .clone();
        let callback = OAuthCallback::new(
            request.flow_id,
            state,
            "authorization-code-3",
            "http://127.0.0.1:22109/other",
        )
        .expect("redirect callback");
        assert_eq!(
            manager
                .complete_authorization(callback, response("access", Some("refresh"), 3_600), 3_001)
                .unwrap_err()
                .code(),
            "oauth_redirect_mismatch"
        );
    }

    #[test]
    fn malformed_or_insufficient_token_response_is_rejected() {
        assert_eq!(
            decode_token_response(
                br#"{"access_token":"value","expires_in":3600,"unexpected":true}"#
            )
            .unwrap_err()
            .code(),
            "oauth_token_response_invalid"
        );
        assert_eq!(
            decode_token_response(&vec![b'x'; MAX_TOKEN_RESPONSE_BYTES + 1])
                .unwrap_err()
                .code(),
            "oauth_token_response_too_large"
        );
        let manager = OAuthManager::new(config(), None).expect("manager");
        let (request, _) = manager.start_authorization(1_000).expect("flow");
        let state = manager
            .state
            .lock()
            .unwrap()
            .pending
            .get(&request.flow_id)
            .expect("pending")
            .state
            .clone();
        let callback = OAuthCallback::new(request.flow_id, state, "code", request.redirect_uri)
            .expect("callback");
        assert_eq!(
            manager
                .complete_authorization(
                    callback,
                    RawTokenResponse {
                        access_token: "".to_owned(),
                        refresh_token: None,
                        expires_in: Some(3_600),
                        scope: Some("model.read model.use".to_owned()),
                        token_type: Some("Bearer".to_owned()),
                    },
                    1_001,
                )
                .unwrap_err()
                .code(),
            "oauth_token_response_invalid"
        );
        let manager = OAuthManager::new(config(), None).expect("manager");
        let (request, _) = manager.start_authorization(1_000).expect("flow");
        let state = manager
            .state
            .lock()
            .unwrap()
            .pending
            .get(&request.flow_id)
            .expect("pending")
            .state
            .clone();
        let callback = OAuthCallback::new(request.flow_id, state, "code", request.redirect_uri)
            .expect("callback");
        assert_eq!(
            manager
                .complete_authorization(
                    callback,
                    RawTokenResponse {
                        scope: Some("model.read".to_owned()),
                        ..response("access", Some("refresh"), 3_600)
                    },
                    1_001,
                )
                .unwrap_err()
                .code(),
            "oauth_scope_insufficient"
        );
    }

    #[tokio::test]
    async fn concurrent_refresh_is_single_flight_and_transient_keeps_valid_token() {
        let manager = OAuthManager::new(config(), None).expect("manager");
        manager
            .install_tokens(
                "old-access".to_owned(),
                Some("old-refresh".to_owned()),
                2_000,
                1_000,
            )
            .expect("seed");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let manager = manager.clone();
            let calls = calls.clone();
            tasks.push(tokio::spawn(async move {
                manager
                    .access_token(1_500, 1_000, move |_refresh| {
                        let calls = calls.clone();
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            Ok(response("new-access", Some("new-refresh"), 3_600))
                        }
                    })
                    .await
                    .expect("refresh")
                    .value
                    .clone()
            }));
        }
        let values = futures::future::join_all(tasks).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(values
            .into_iter()
            .all(|value| value.expect("task") == "new-access"));

        manager
            .install_tokens(
                "still-valid".to_owned(),
                Some("refresh".to_owned()),
                2_000,
                1_000,
            )
            .expect("seed");
        let value = manager
            .access_token(1_500, 1_000, |_refresh| async {
                Err(RefreshFailure::Transient)
            })
            .await
            .expect("stale valid token")
            .value
            .clone();
        assert_eq!(value, "still-valid");
    }

    #[tokio::test]
    async fn generation_cas_and_permanent_reauth_fence_old_refresh() {
        let manager = OAuthManager::new(config(), None).expect("manager");
        let metadata = manager
            .install_tokens("old".to_owned(), Some("refresh".to_owned()), 2_000, 1_000)
            .expect("seed");
        assert_eq!(
            manager
                .rotate(
                    metadata.generation - 1,
                    "stale".to_owned(),
                    Some("stale-refresh".to_owned()),
                    4_000,
                    config().scopes,
                    1_000,
                )
                .await
                .unwrap_err()
                .code(),
            "oauth_generation_conflict"
        );
        let error = match manager
            .access_token(1_500, 1_000, |_refresh| async {
                Err(RefreshFailure::Permanent)
            })
            .await
        {
            Ok(_) => panic!("permanent refresh must require reauth"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "oauth_reauth_required");
        let error = match manager
            .access_token(1_501, 0, |_refresh| async {
                Ok(response("should-not-run", Some("refresh"), 3_600))
            })
            .await
        {
            Ok(_) => panic!("reauth fenced token must not refresh"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "oauth_reauth_required");
        let revoked = manager.revoke().await.expect("revoke");
        assert_eq!(revoked.status, OAuthTokenStatus::Revoked);
    }

    #[tokio::test]
    async fn rotation_wins_over_an_in_flight_old_refresh() {
        let manager = OAuthManager::new(config(), None).expect("manager");
        let metadata = manager
            .install_tokens("old".to_owned(), Some("refresh".to_owned()), 2_000, 1_000)
            .expect("seed");
        let (release, wait) = tokio::sync::oneshot::channel::<()>();
        let (started, started_wait) = tokio::sync::oneshot::channel::<()>();
        let refreshing = manager.clone();
        let task = tokio::spawn(async move {
            refreshing
                .access_token(1_500, 1_000, move |_refresh| async move {
                    let _ = started.send(());
                    let _ = wait.await;
                    Ok(response("stale-access", Some("stale-refresh"), 3_600))
                })
                .await
        });
        started_wait.await.expect("refresh started");
        let rotated = manager
            .rotate(
                metadata.generation,
                "new-access".to_owned(),
                Some("new-refresh".to_owned()),
                5_000,
                config().scopes,
                1_000,
            )
            .await
            .expect("rotation");
        release.send(()).expect("release refresh");
        let result = task.await.expect("refresh task");
        let error = match result {
            Ok(_) => panic!("old refresh must lose the generation race"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "oauth_generation_conflict");
        assert_eq!(
            manager.metadata().expect("metadata").generation,
            rotated.generation
        );
        assert_eq!(
            manager.metadata().expect("metadata").access_digest,
            json_digest(&serde_json::json!("new-access"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_atomic_owner_only_and_strict() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("kiana-ci09-{}", RequestId::new()));
        let path = root.join("oauth.json");
        let manager = OAuthManager::new(config(), Some(path.clone())).expect("manager");
        manager
            .install_tokens(
                "file-access".to_owned(),
                Some("file-refresh".to_owned()),
                5_000,
                1_000,
            )
            .expect("persist");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let entries = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec!["oauth.json"]);
        let loaded = OAuthManager::load(config(), path.clone()).expect("load");
        assert_eq!(loaded.metadata().unwrap().generation, 1);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["unknown"] = serde_json::json!(true);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let error = match OAuthManager::load(config(), path.clone()) {
            Ok(_) => panic!("unknown token file fields must fail closed"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "oauth_token_file_invalid");
        let _ = std::fs::remove_dir_all(root);
    }
}
