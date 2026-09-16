//! Versioned, secret-free provider configuration snapshots.

use crate::{json_digest, ModelCapabilities, ModelRoute};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const PROVIDER_CONFIG_SNAPSHOT_SCHEMA: &str = "kiana.provider-config-snapshot.v1";
pub const PROVIDER_PROFILE_SNAPSHOT_SCHEMA: &str = "kiana.provider-profile-snapshot.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSelectionMode {
    Live,
    Cassette,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConfigSource {
    BuiltinDefault,
    Environment,
    Explicit,
    Profile,
    LegacyCassette,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProfileSnapshot {
    pub schema: String,
    pub profile: String,
    pub profile_version: u64,
    pub route: ModelRoute,
    pub capabilities: ModelCapabilities,
    pub credential_ref: Option<String>,
    pub source: ProviderConfigSource,
    pub snapshot_digest: String,
}

impl ProviderProfileSnapshot {
    pub fn new(
        route: ModelRoute,
        capabilities: ModelCapabilities,
        credential_ref: Option<String>,
        source: ProviderConfigSource,
    ) -> Result<Self, String> {
        let profile = route.profile.clone();
        if profile.trim().is_empty() || route.provider_id.trim().is_empty() {
            return Err("provider_profile_snapshot_invalid".to_owned());
        }
        let mut snapshot = Self {
            schema: PROVIDER_PROFILE_SNAPSHOT_SCHEMA.to_owned(),
            profile,
            profile_version: 1,
            route,
            capabilities,
            credential_ref,
            source,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_PROFILE_SNAPSHOT_SCHEMA
            || self.profile_version == 0
            || self.profile.trim().is_empty()
            || self.route.profile != self.profile
            || self.snapshot_digest != self.digest()
        {
            return Err("provider_profile_snapshot_invalid".to_owned());
        }
        if let Some(reference) = &self.credential_ref {
            if !reference.starts_with("sha256:") || reference.len() != 71 {
                return Err("provider_credential_ref_invalid".to_owned());
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "profile": self.profile,
            "profile_version": self.profile_version,
            "route": self.route,
            "capabilities": self.capabilities,
            "credential_ref": self.credential_ref,
            "source": self.source,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfigSnapshot {
    pub schema: String,
    pub version: u64,
    pub selection_mode: ProviderSelectionMode,
    pub profiles: Vec<ProviderProfileSnapshot>,
    pub snapshot_digest: String,
}

impl ProviderConfigSnapshot {
    pub fn new(
        selection_mode: ProviderSelectionMode,
        profiles: Vec<ProviderProfileSnapshot>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: PROVIDER_CONFIG_SNAPSHOT_SCHEMA.to_owned(),
            version: 1,
            selection_mode,
            profiles,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CONFIG_SNAPSHOT_SCHEMA
            || self.version == 0
            || self.profiles.is_empty()
            || self.profiles.len() > 256
            || self.snapshot_digest != self.digest()
        {
            return Err("provider_config_snapshot_invalid".to_owned());
        }
        let mut profiles = std::collections::BTreeSet::new();
        for profile in &self.profiles {
            profile.validate()?;
            if !profiles.insert(profile.profile.clone()) {
                return Err("provider_config_profile_duplicate".to_owned());
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "selection_mode": self.selection_mode,
            "profiles": self.profiles,
        }))
    }
}
