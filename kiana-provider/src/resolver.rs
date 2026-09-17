//! Single provider configuration resolver.
//!
//! This module owns the product configuration boundary. It resolves explicit/env profiles for
//! `ProviderGateway` and parses optional workspace overlays into strict, secret-free snapshots.
//! Workspace text is never accepted before the caller proves ProjectTrust; the resolver does not
//! perform network I/O or return raw credentials to callers.

use crate::config::{self, Connection};
use crate::ProviderConfig;
use kiana_domain::{json_digest, ConfigSnapshot, ProviderConfigSnapshot, RoleSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const WORKSPACE_CONFIG_SCHEMA: &str = "kiana.provider-config.v1";
pub const WORKSPACE_CONFIG_VERSION: u64 = 1;
pub const MAX_WORKSPACE_CONFIG_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Default)]
pub struct ConfigResolver;

#[derive(Clone)]
pub(crate) struct Resolution {
    pub connections: BTreeMap<String, Connection>,
    pub explicit_profiles: bool,
    pub snapshot: ProviderConfigSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfig {
    pub schema: String,
    pub version: u64,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, WorkspaceProfile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceProfile {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub inherit_default: bool,
}

impl ConfigResolver {
    /// Resolve the only production provider connection/profile parser.
    pub(crate) fn resolve(config: ProviderConfig) -> Result<Resolution, kiana_domain::ModelError> {
        let (connections, explicit_profiles) = config::connections(config)?;
        let snapshot = config::snapshot(&connections)?;
        Ok(Resolution {
            connections,
            explicit_profiles,
            snapshot,
        })
    }

    /// Parse a strict workspace overlay after the caller has established ProjectTrust.
    pub fn parse_workspace(
        raw: &str,
        project_trusted: bool,
    ) -> Result<WorkspaceConfig, kiana_domain::ModelError> {
        if !project_trusted {
            return Err(kiana_domain::ModelError::invalid(
                "config_workspace_untrusted",
            ));
        }
        if raw.len() > MAX_WORKSPACE_CONFIG_BYTES || raw.contains('\0') {
            return Err(kiana_domain::ModelError::invalid(
                "config_workspace_too_large",
            ));
        }
        let config: WorkspaceConfig = serde_json::from_str(raw)
            .map_err(|_| kiana_domain::ModelError::invalid("config_workspace_invalid"))?;
        validate_workspace(&config)?;
        Ok(config)
    }

    /// Parse a trusted overlay and produce the canonical secret-free domain snapshot used for
    /// revision fencing. `project_trust_revision` must already be a digest from the authority
    /// boundary; this function does not infer trust from the config text.
    pub fn workspace_snapshot(
        raw: &str,
        project_trusted: bool,
        project_trust_revision: &str,
    ) -> Result<ConfigSnapshot, kiana_domain::ModelError> {
        let config = Self::parse_workspace(raw, project_trusted)?;
        let effective = serde_json::to_value(&config)
            .map_err(|_| kiana_domain::ModelError::invalid("config_workspace_encode_failed"))?;
        ConfigSnapshot::new(
            vec!["workspace:provider-config".to_owned()],
            effective.clone(),
            json_digest(&effective),
            project_trust_revision.to_owned(),
        )
        .map_err(kiana_domain::ModelError::invalid)
    }
}

fn validate_workspace(config: &WorkspaceConfig) -> Result<(), kiana_domain::ModelError> {
    if config.schema != WORKSPACE_CONFIG_SCHEMA || config.version != WORKSPACE_CONFIG_VERSION {
        return Err(kiana_domain::ModelError::invalid(
            "config_workspace_schema_unsupported",
        ));
    }
    for (value, code, max) in [
        (config.provider.as_deref(), "config_provider_invalid", 128),
        (config.model.as_deref(), "config_model_invalid", 256),
    ] {
        if value.is_some_and(|value| value.trim().is_empty() || value.len() > max) {
            return Err(kiana_domain::ModelError::invalid(code));
        }
    }
    if let Some(base_url) = &config.base_url {
        validate_endpoint(base_url)?;
    }
    validate_env_ref(config.api_key_env.as_deref())?;
    if config.profiles.len() > 64 {
        return Err(kiana_domain::ModelError::invalid(
            "config_profile_limit_exceeded",
        ));
    }
    let allowed = RoleSpec::catalog()
        .into_iter()
        .map(|role| role.model_profile)
        .collect::<std::collections::BTreeSet<_>>();
    for (name, profile) in &config.profiles {
        if !allowed.contains(name)
            || profile.provider.trim().is_empty()
            || profile.provider.len() > 128
            || profile.model.trim().is_empty()
            || profile.model.len() > 256
        {
            return Err(kiana_domain::ModelError::invalid("config_profile_invalid"));
        }
        if let Some(base_url) = &profile.base_url {
            validate_endpoint(base_url)?;
        }
        validate_env_ref(profile.api_key_env.as_deref())?;
        if profile.inherit_default && (profile.base_url.is_some() || profile.api_key_env.is_some())
        {
            return Err(kiana_domain::ModelError::invalid(
                "config_profile_inheritance_conflict",
            ));
        }
    }
    Ok(())
}

fn validate_env_ref(value: Option<&str>) -> Result<(), kiana_domain::ModelError> {
    if value.is_some_and(|value| {
        value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    }) {
        return Err(kiana_domain::ModelError::invalid(
            "config_secret_ref_invalid",
        ));
    }
    Ok(())
}

fn validate_endpoint(value: &str) -> Result<(), kiana_domain::ModelError> {
    let endpoint = reqwest::Url::parse(value.trim())
        .map_err(|_| kiana_domain::ModelError::invalid("config_endpoint_invalid"))?;
    if !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(kiana_domain::ModelError::invalid(
            "config_endpoint_credentials_or_query_denied",
        ));
    }
    let local = endpoint.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && local) {
        return Err(kiana_domain::ModelError::invalid(
            "config_endpoint_requires_tls_or_loopback",
        ));
    }
    Ok(())
}

/// Keep the resolver's effective config shape inspectable without exposing credentials.
pub fn redacted_workspace_value(
    config: &WorkspaceConfig,
) -> Result<Value, kiana_domain::ModelError> {
    serde_json::to_value(config)
        .map_err(|_| kiana_domain::ModelError::invalid("config_workspace_encode_failed"))
}
