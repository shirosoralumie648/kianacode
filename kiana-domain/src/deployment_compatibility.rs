//! Version-axis compatibility matrix for deployment admission.
//!
//! The matrix compares server-owned snapshots. It does not migrate data, acquire a lease, start
//! a revision or decide an external provider outcome. Unknown major versions, downgrades and
//! route/workflow/extension drift remain explicit blocked reasons.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const DEPLOYMENT_COMPATIBILITY_SCHEMA: &str = "kiana.deployment-compatibility.v1";
pub const DEPLOYMENT_COMPATIBILITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_COMPATIBILITY_REASONS: usize = 16;
pub const MAX_COMPATIBILITY_TEXT: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentVersionAxes {
    pub schema: String,
    pub version: SchemaVersion,
    pub app_build: String,
    pub protocol_version: SchemaVersion,
    pub domain_schema: SchemaVersion,
    pub store_format: u32,
    pub projection_version: SchemaVersion,
    pub workflow_definition_digest: String,
    pub provider_route_digest: String,
    pub extension_digest: String,
    pub config_revision: String,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub generation: u64,
    pub axes_digest: String,
}

impl DeploymentVersionAxes {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app_build: impl Into<String>,
        protocol_version: SchemaVersion,
        domain_schema: SchemaVersion,
        store_format: u32,
        projection_version: SchemaVersion,
        workflow_definition_digest: impl Into<String>,
        provider_route_digest: impl Into<String>,
        extension_digest: impl Into<String>,
        config_revision: impl Into<String>,
        authority_epoch: u64,
        data_epoch: u64,
        generation: u64,
    ) -> Result<Self, String> {
        let mut axes = Self {
            schema: DEPLOYMENT_COMPATIBILITY_SCHEMA.to_owned(),
            version: DEPLOYMENT_COMPATIBILITY_VERSION,
            app_build: app_build.into(),
            protocol_version,
            domain_schema,
            store_format,
            projection_version,
            workflow_definition_digest: workflow_definition_digest.into(),
            provider_route_digest: provider_route_digest.into(),
            extension_digest: extension_digest.into(),
            config_revision: config_revision.into(),
            authority_epoch,
            data_epoch,
            generation,
            axes_digest: String::new(),
        };
        axes.axes_digest = axes.digest();
        axes.validate()?;
        Ok(axes)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_COMPATIBILITY_SCHEMA
            || self.version != DEPLOYMENT_COMPATIBILITY_VERSION
            || self.store_format == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.generation == 0
            || self.protocol_version.major == 0
            || self.domain_schema.major == 0
            || self.projection_version.major == 0
        {
            return Err("deployment_compatibility_axes_invalid".to_owned());
        }
        bounded(&self.app_build, "deployment_compatibility_app_build")?;
        bounded(
            &self.config_revision,
            "deployment_compatibility_config_revision",
        )?;
        for (value, field) in [
            (
                &self.workflow_definition_digest,
                "deployment_compatibility_workflow_digest",
            ),
            (
                &self.provider_route_digest,
                "deployment_compatibility_provider_digest",
            ),
            (
                &self.extension_digest,
                "deployment_compatibility_extension_digest",
            ),
            (&self.axes_digest, "deployment_compatibility_axes_digest"),
        ] {
            valid_digest(value, field)?;
        }
        if self.axes_digest != self.digest() {
            return Err("deployment_compatibility_axes_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "app_build": self.app_build,
            "protocol_version": self.protocol_version,
            "domain_schema": self.domain_schema,
            "store_format": self.store_format,
            "projection_version": self.projection_version,
            "workflow_definition_digest": self.workflow_definition_digest,
            "provider_route_digest": self.provider_route_digest,
            "extension_digest": self.extension_digest,
            "config_revision": self.config_revision,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "generation": self.generation,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentCompatibilityStatus {
    Compatible,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentCompatibilityMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub current: DeploymentVersionAxes,
    pub candidate: DeploymentVersionAxes,
    pub status: DeploymentCompatibilityStatus,
    pub reasons: Vec<String>,
    pub matrix_digest: String,
}

impl DeploymentCompatibilityMatrix {
    pub fn evaluate(
        current: DeploymentVersionAxes,
        candidate: DeploymentVersionAxes,
    ) -> Result<Self, String> {
        current.validate()?;
        candidate.validate()?;
        let reasons = compatibility_reasons(&current, &candidate);
        let mut matrix = Self {
            schema: DEPLOYMENT_COMPATIBILITY_SCHEMA.to_owned(),
            version: DEPLOYMENT_COMPATIBILITY_VERSION,
            current,
            candidate,
            status: if reasons.is_empty() {
                DeploymentCompatibilityStatus::Compatible
            } else {
                DeploymentCompatibilityStatus::Blocked
            },
            reasons,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_COMPATIBILITY_SCHEMA
            || self.version != DEPLOYMENT_COMPATIBILITY_VERSION
            || self.reasons.len() > MAX_COMPATIBILITY_REASONS
        {
            return Err("deployment_compatibility_matrix_invalid".to_owned());
        }
        self.current.validate()?;
        self.candidate.validate()?;
        let expected = compatibility_reasons(&self.current, &self.candidate);
        if self.reasons != expected {
            return Err("deployment_compatibility_reasons_mismatch".to_owned());
        }
        let expected_status = if expected.is_empty() {
            DeploymentCompatibilityStatus::Compatible
        } else {
            DeploymentCompatibilityStatus::Blocked
        };
        if self.status != expected_status {
            return Err("deployment_compatibility_status_mismatch".to_owned());
        }
        for reason in &self.reasons {
            bounded(reason, "deployment_compatibility_reason")?;
        }
        valid_digest(
            &self.matrix_digest,
            "deployment_compatibility_matrix_digest",
        )?;
        if self.matrix_digest != self.digest() {
            return Err("deployment_compatibility_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "current": self.current,
            "candidate": self.candidate,
            "status": self.status,
            "reasons": self.reasons,
        }))
    }
}

fn compatibility_reasons(
    current: &DeploymentVersionAxes,
    candidate: &DeploymentVersionAxes,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if current.protocol_version.major != candidate.protocol_version.major {
        reasons.push("protocol_major_mismatch".to_owned());
    }
    if current.domain_schema.major != candidate.domain_schema.major {
        reasons.push("domain_schema_major_mismatch".to_owned());
    }
    if current.projection_version.major != candidate.projection_version.major {
        reasons.push("projection_major_mismatch".to_owned());
    }
    if candidate.store_format < current.store_format {
        reasons.push("store_format_downgrade".to_owned());
    }
    if candidate.authority_epoch < current.authority_epoch {
        reasons.push("authority_epoch_rollback".to_owned());
    }
    if candidate.data_epoch < current.data_epoch {
        reasons.push("data_epoch_rollback".to_owned());
    }
    if candidate.generation < current.generation {
        reasons.push("generation_rollback".to_owned());
    }
    if candidate.workflow_definition_digest != current.workflow_definition_digest {
        reasons.push("workflow_definition_drift".to_owned());
    }
    if candidate.provider_route_digest != current.provider_route_digest {
        reasons.push("provider_route_drift".to_owned());
    }
    if candidate.extension_digest != current.extension_digest {
        reasons.push("extension_digest_drift".to_owned());
    }
    reasons
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_COMPATIBILITY_TEXT
        || value.contains(['\0', '\r', '\n'])
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
