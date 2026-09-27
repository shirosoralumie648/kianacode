//! Deployment profile, environment and revision contracts.
//!
//! These values describe an already resolved deployment decision.  The domain layer performs
//! deterministic value validation only: it does not inspect the filesystem, resolve ProjectTrust,
//! create a root, start a process or retain secret material.  Daemon and policy adapters supply
//! the trust and root evidence, and later deployment steps decide whether an operation may run.

use crate::{
    json_digest, InstanceId, ProjectId, SchemaVersion, SecretRefId, StorageBackend, StorageRoot,
    StorageRootId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Component, Path};

pub const DEPLOYMENT_PROFILE_SCHEMA: &str = "kiana.deployment-profile.v1";
pub const DEPLOYMENT_REVISION_SCHEMA: &str = "kiana.deployment-revision.v1";
pub const DEPLOYMENT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_DEPLOYMENT_TEXT_BYTES: usize = 512;
pub const MAX_DEPLOYMENT_CAPABILITIES: usize = 64;
pub const MAX_DEPLOYMENT_SECRET_REFS: usize = 64;

/// The four deployment shapes share the same DaemonHost and ControlPlane spine.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentProfile {
    EmbeddedLocal,
    ManagedLocal,
    Container,
    Orchestrated,
}

impl DeploymentProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmbeddedLocal => "embedded-local",
            Self::ManagedLocal => "managed-local",
            Self::Container => "container",
            Self::Orchestrated => "orchestrated",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentPhase {
    Discovered,
    Preflight,
    Quiescing,
    Draining,
    BackedUp,
    Migrated,
    Starting,
    Ready,
    Serving,
    Maintenance,
    Stopped,
    Degraded,
    NeedsRecovery,
}

/// Immutable environment input used by ControlPlane deployment decisions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentProfile {
    pub schema: String,
    pub version: SchemaVersion,
    pub environment_id: String,
    pub profile: DeploymentProfile,
    pub storage_root: StorageRootId,
    pub storage_root_path: String,
    pub storage_project_id: Option<ProjectId>,
    pub project_id: Option<ProjectId>,
    pub project_root: Option<String>,
    pub project_root_digest: Option<String>,
    pub project_trusted: bool,
    pub config_revision: String,
    pub secret_refs: Vec<SecretRefId>,
    pub allowed_capabilities: Vec<String>,
    pub platform: String,
    pub filesystem: StorageBackend,
    pub maintenance_policy: String,
    pub profile_digest: String,
}

impl EnvironmentProfile {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        environment_id: impl Into<String>,
        profile: DeploymentProfile,
        storage_root: StorageRootId,
        storage_root_path: impl Into<String>,
        storage_project_id: Option<ProjectId>,
        project_id: Option<ProjectId>,
        project_root: Option<String>,
        project_root_digest: Option<String>,
        project_trusted: bool,
        config_revision: impl Into<String>,
        secret_refs: Vec<SecretRefId>,
        allowed_capabilities: Vec<String>,
        platform: impl Into<String>,
        filesystem: StorageBackend,
        maintenance_policy: impl Into<String>,
    ) -> Result<Self, String> {
        let mut environment = Self {
            schema: DEPLOYMENT_PROFILE_SCHEMA.to_owned(),
            version: DEPLOYMENT_SCHEMA_VERSION,
            environment_id: environment_id.into(),
            profile,
            storage_root,
            storage_root_path: storage_root_path.into(),
            storage_project_id,
            project_id,
            project_root,
            project_root_digest,
            project_trusted,
            config_revision: config_revision.into(),
            secret_refs,
            allowed_capabilities,
            platform: platform.into(),
            filesystem,
            maintenance_policy: maintenance_policy.into(),
            profile_digest: String::new(),
        };
        environment.profile_digest = environment.digest();
        environment.validate()?;
        Ok(environment)
    }

    /// Build a profile from an already validated domain storage root.
    pub fn from_storage_root(
        root: &StorageRoot,
        environment_id: impl Into<String>,
        profile: DeploymentProfile,
        project_id: Option<ProjectId>,
        project_root: Option<String>,
        project_root_digest: Option<String>,
        project_trusted: bool,
        config_revision: impl Into<String>,
        secret_refs: Vec<SecretRefId>,
        allowed_capabilities: Vec<String>,
        platform: impl Into<String>,
        maintenance_policy: impl Into<String>,
    ) -> Result<Self, String> {
        root.validate()?;
        Self::new(
            environment_id,
            profile,
            root.root_id,
            root.canonical_path.clone(),
            root.owner_scope.project_id,
            project_id,
            project_root,
            project_root_digest,
            project_trusted,
            config_revision,
            secret_refs,
            allowed_capabilities,
            platform,
            root.backend,
            maintenance_policy,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_PROFILE_SCHEMA
            || self.version != DEPLOYMENT_SCHEMA_VERSION
            || self.storage_root.as_uuid().is_nil()
        {
            return Err("deployment_profile_header_invalid".to_owned());
        }
        bounded(&self.environment_id, "deployment_environment_id")?;
        bounded(&self.config_revision, "deployment_config_revision")?;
        bounded(&self.platform, "deployment_platform")?;
        bounded(&self.maintenance_policy, "deployment_maintenance_policy")?;
        absolute_path(&self.storage_root_path, "deployment_storage_root")?;
        if self.filesystem == StorageBackend::NetworkFilesystem {
            return Err("deployment_network_filesystem_unsupported".to_owned());
        }
        if self.storage_project_id != self.project_id {
            return Err("deployment_project_root_mismatch".to_owned());
        }
        match (
            &self.project_id,
            &self.project_root,
            &self.project_root_digest,
        ) {
            (Some(_), Some(root), Some(digest)) => {
                absolute_path(root, "deployment_project_root")?;
                valid_digest(digest, "deployment_project_root_digest")?;
                if !self.project_trusted {
                    return Err("deployment_project_untrusted".to_owned());
                }
            }
            (None, None, None) => {}
            _ => return Err("deployment_project_binding_invalid".to_owned()),
        }
        let mut secret_refs = BTreeSet::new();
        if self.secret_refs.len() > MAX_DEPLOYMENT_SECRET_REFS
            || self
                .secret_refs
                .iter()
                .any(|value| value.as_uuid().is_nil() || !secret_refs.insert(*value))
        {
            return Err("deployment_secret_refs_invalid".to_owned());
        }
        if self.allowed_capabilities.len() > MAX_DEPLOYMENT_CAPABILITIES {
            return Err("deployment_capabilities_invalid".to_owned());
        }
        let mut capabilities = BTreeSet::new();
        for capability in &self.allowed_capabilities {
            bounded(capability, "deployment_capability")?;
            if !capabilities.insert(capability) {
                return Err("deployment_capabilities_duplicate".to_owned());
            }
        }
        valid_digest(&self.profile_digest, "deployment_profile_digest")?;
        if self.profile_digest != self.digest() {
            return Err("deployment_profile_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "environment_id": self.environment_id,
            "profile": self.profile,
            "storage_root": self.storage_root,
            "storage_root_path": self.storage_root_path,
            "storage_project_id": self.storage_project_id,
            "project_id": self.project_id,
            "project_root": self.project_root,
            "project_root_digest": self.project_root_digest,
            "project_trusted": self.project_trusted,
            "config_revision": self.config_revision,
            "secret_refs": self.secret_refs,
            "allowed_capabilities": self.allowed_capabilities,
            "platform": self.platform,
            "filesystem": self.filesystem,
            "maintenance_policy": self.maintenance_policy,
        }))
    }
}

/// A server-owned deployment revision.  It records lifecycle facts but never starts or stops a
/// process; operation admission and EventLog persistence belong to later deployment steps.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentRevision {
    pub schema: String,
    pub version: SchemaVersion,
    pub revision_id: String,
    pub release_id: String,
    pub profile: DeploymentProfile,
    pub instance_id: InstanceId,
    pub storage_root: StorageRootId,
    pub build_id: String,
    pub data_epoch: u64,
    pub authority_epoch: u64,
    pub phase: DeploymentPhase,
    pub health: String,
    pub started_at_unix_ms: u64,
    pub drain_started_at_unix_ms: Option<u64>,
    pub retired_at_unix_ms: Option<u64>,
    pub revision_digest: String,
}

impl DeploymentRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        revision_id: impl Into<String>,
        release_id: impl Into<String>,
        profile: DeploymentProfile,
        instance_id: InstanceId,
        storage_root: StorageRootId,
        build_id: impl Into<String>,
        data_epoch: u64,
        authority_epoch: u64,
        phase: DeploymentPhase,
        health: impl Into<String>,
        started_at_unix_ms: u64,
        drain_started_at_unix_ms: Option<u64>,
        retired_at_unix_ms: Option<u64>,
    ) -> Result<Self, String> {
        let mut revision = Self {
            schema: DEPLOYMENT_REVISION_SCHEMA.to_owned(),
            version: DEPLOYMENT_SCHEMA_VERSION,
            revision_id: revision_id.into(),
            release_id: release_id.into(),
            profile,
            instance_id,
            storage_root,
            build_id: build_id.into(),
            data_epoch,
            authority_epoch,
            phase,
            health: health.into(),
            started_at_unix_ms,
            drain_started_at_unix_ms,
            retired_at_unix_ms,
            revision_digest: String::new(),
        };
        revision.revision_digest = revision.digest();
        revision.validate()?;
        Ok(revision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_REVISION_SCHEMA
            || self.version != DEPLOYMENT_SCHEMA_VERSION
            || self.instance_id.as_uuid().is_nil()
            || self.storage_root.as_uuid().is_nil()
            || self.data_epoch == 0
            || self.authority_epoch == 0
            || self.started_at_unix_ms == 0
        {
            return Err("deployment_revision_header_invalid".to_owned());
        }
        bounded(&self.revision_id, "deployment_revision_id")?;
        bounded(&self.release_id, "deployment_release_id")?;
        bounded(&self.build_id, "deployment_build_id")?;
        bounded(&self.health, "deployment_health")?;
        if matches!(
            self.phase,
            DeploymentPhase::Ready | DeploymentPhase::Serving
        ) && self.health.trim().is_empty()
        {
            return Err("deployment_revision_health_required".to_owned());
        }
        if self
            .drain_started_at_unix_ms
            .is_some_and(|value| value < self.started_at_unix_ms)
            || self
                .retired_at_unix_ms
                .is_some_and(|value| value < self.started_at_unix_ms)
            || self
                .retired_at_unix_ms
                .zip(self.drain_started_at_unix_ms)
                .is_some_and(|(retired, drain)| retired < drain)
        {
            return Err("deployment_revision_timestamps_invalid".to_owned());
        }
        if self.phase == DeploymentPhase::Stopped && self.retired_at_unix_ms.is_none() {
            return Err("deployment_revision_retired_at_required".to_owned());
        }
        valid_digest(&self.revision_digest, "deployment_revision_digest")?;
        if self.revision_digest != self.digest() {
            return Err("deployment_revision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "revision_id": self.revision_id,
            "release_id": self.release_id,
            "profile": self.profile,
            "instance_id": self.instance_id,
            "storage_root": self.storage_root,
            "build_id": self.build_id,
            "data_epoch": self.data_epoch,
            "authority_epoch": self.authority_epoch,
            "phase": self.phase,
            "health": self.health,
            "started_at_unix_ms": self.started_at_unix_ms,
            "drain_started_at_unix_ms": self.drain_started_at_unix_ms,
            "retired_at_unix_ms": self.retired_at_unix_ms,
        }))
    }
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_DEPLOYMENT_TEXT_BYTES
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn absolute_path(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > 4_096
        || !Path::new(value).is_absolute()
        || Path::new(value)
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
