//! Immutable deployment configuration source and redaction contracts.
//!
//! This module records source precedence and server-owned trust/allowlist evidence without
//! reading files, environment variables or secret material. The daemon/config adapter supplies
//! already redacted values and opaque `SecretRef`s; later startup steps decide when a revision may
//! be published or require restart/migration.

use crate::{json_digest, redact_value, ProjectTrustSnapshot, SchemaVersion, SecretRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const DEPLOYMENT_CONFIG_SOURCE_SCHEMA: &str = "kiana.deployment-config-source.v1";
pub const DEPLOYMENT_CONFIG_SNAPSHOT_SCHEMA: &str = "kiana.deployment-config-snapshot.v1";
pub const DEPLOYMENT_CONFIG_DIFF_SCHEMA: &str = "kiana.deployment-config-diff.v1";
pub const DEPLOYMENT_CONFIG_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_CONFIG_SOURCES: usize = 16;
pub const MAX_CONFIG_ENV_KEYS: usize = 128;
pub const MAX_CONFIG_SECRET_REFS: usize = 64;
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentConfigSourceKind {
    CompiledDefault,
    UserFile,
    KianaHomeFile,
    ProjectFile,
    Environment,
    CommandLine,
}

impl DeploymentConfigSourceKind {
    pub const fn precedence(self) -> u8 {
        match self {
            Self::CompiledDefault => 10,
            Self::UserFile => 20,
            Self::KianaHomeFile => 25,
            Self::ProjectFile => 30,
            Self::Environment => 40,
            Self::CommandLine => 50,
        }
    }

    const fn prefix(self) -> &'static str {
        match self {
            Self::CompiledDefault => "builtin:",
            Self::UserFile => "user:",
            Self::KianaHomeFile => "kiana-home:",
            Self::ProjectFile => "project:",
            Self::Environment => "env:",
            Self::CommandLine => "cli:",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentConfigSource {
    pub schema: String,
    pub version: SchemaVersion,
    pub kind: DeploymentConfigSourceKind,
    pub reference: String,
    pub precedence: u8,
    pub trusted: bool,
    pub trust_digest: Option<String>,
    pub source_digest: String,
}

impl DeploymentConfigSource {
    pub fn new(
        kind: DeploymentConfigSourceKind,
        reference: impl Into<String>,
        trust_digest: Option<String>,
        source_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut source = Self {
            schema: DEPLOYMENT_CONFIG_SOURCE_SCHEMA.to_owned(),
            version: DEPLOYMENT_CONFIG_VERSION,
            kind,
            reference: reference.into(),
            precedence: kind.precedence(),
            trusted: matches!(
                kind,
                DeploymentConfigSourceKind::CompiledDefault
                    | DeploymentConfigSourceKind::UserFile
                    | DeploymentConfigSourceKind::KianaHomeFile
                    | DeploymentConfigSourceKind::ProjectFile
            ),
            trust_digest,
            source_digest: source_digest.into(),
        };
        source.validate()?;
        Ok(source)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_CONFIG_SOURCE_SCHEMA
            || self.version != DEPLOYMENT_CONFIG_VERSION
            || self.precedence != self.kind.precedence()
        {
            return Err("deployment_config_source_header_invalid".to_owned());
        }
        bounded(&self.reference, "deployment_config_source_reference")?;
        if !self.reference.starts_with(self.kind.prefix()) {
            return Err("deployment_config_source_reference_kind_mismatch".to_owned());
        }
        validate_source_reference(&self.reference, self.kind)?;
        match self.kind {
            DeploymentConfigSourceKind::CompiledDefault
            | DeploymentConfigSourceKind::UserFile
            | DeploymentConfigSourceKind::KianaHomeFile => {
                if !self.trusted || self.trust_digest.is_some() {
                    return Err("deployment_config_source_trust_invalid".to_owned());
                }
            }
            DeploymentConfigSourceKind::ProjectFile => {
                if !self.trusted || self.trust_digest.is_none() {
                    return Err("deployment_config_project_trust_required".to_owned());
                }
            }
            DeploymentConfigSourceKind::Environment | DeploymentConfigSourceKind::CommandLine => {
                if self.trusted || self.trust_digest.is_some() {
                    return Err("deployment_config_external_trust_invalid".to_owned());
                }
            }
        }
        if let Some(digest) = &self.trust_digest {
            valid_digest(digest, "deployment_config_source_trust_digest")?;
        }
        valid_digest(&self.source_digest, "deployment_config_source_digest")
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "kind": self.kind,
            "reference": self.reference,
            "precedence": self.precedence,
            "trusted": self.trusted,
            "trust_digest": self.trust_digest,
            "source_digest": self.source_digest,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigChangeImpact {
    pub restart_required: bool,
    pub migration_required: bool,
    pub lease_reacquire_required: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentConfigSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_trust_digest: String,
    pub project_trusted: bool,
    pub allowed_environment_keys: Vec<String>,
    pub sources: Vec<DeploymentConfigSource>,
    pub effective_non_secret_config: Value,
    pub secret_refs: Vec<SecretRef>,
    pub impact: ConfigChangeImpact,
    pub redacted_digest: String,
    pub config_revision: String,
    pub snapshot_digest: String,
}

impl DeploymentConfigSnapshot {
    pub fn new(
        project_trust: &ProjectTrustSnapshot,
        mut allowed_environment_keys: Vec<String>,
        mut sources: Vec<DeploymentConfigSource>,
        effective_non_secret_config: Value,
        mut secret_refs: Vec<SecretRef>,
    ) -> Result<Self, String> {
        project_trust.validate()?;
        allowed_environment_keys.sort();
        allowed_environment_keys.dedup();
        sources.sort_by_key(|source| source.precedence);
        secret_refs.sort_by(|left, right| left.reference_digest.cmp(&right.reference_digest));
        let impact = derive_impact(&sources, &effective_non_secret_config);
        let mut snapshot = Self {
            schema: DEPLOYMENT_CONFIG_SNAPSHOT_SCHEMA.to_owned(),
            version: DEPLOYMENT_CONFIG_VERSION,
            project_trust_digest: project_trust.trust_digest.clone(),
            project_trusted: project_trust.trusted,
            allowed_environment_keys,
            sources,
            effective_non_secret_config,
            secret_refs,
            impact,
            redacted_digest: String::new(),
            config_revision: String::new(),
            snapshot_digest: String::new(),
        };
        snapshot.config_revision = snapshot.config_revision_digest();
        snapshot.redacted_digest =
            json_digest(&redact_value(&snapshot.effective_non_secret_config));
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_CONFIG_SNAPSHOT_SCHEMA
            || self.version != DEPLOYMENT_CONFIG_VERSION
        {
            return Err("deployment_config_snapshot_header_invalid".to_owned());
        }
        valid_digest(
            &self.project_trust_digest,
            "deployment_config_project_trust_digest",
        )?;
        if self.allowed_environment_keys.len() > MAX_CONFIG_ENV_KEYS
            || self
                .allowed_environment_keys
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("deployment_config_environment_allowlist_invalid".to_owned());
        }
        for key in &self.allowed_environment_keys {
            validate_environment_key(key)?;
        }
        if self.sources.is_empty()
            || self.sources.len() > MAX_CONFIG_SOURCES
            || self
                .sources
                .windows(2)
                .any(|pair| pair[0].precedence >= pair[1].precedence)
        {
            return Err("deployment_config_source_order_invalid".to_owned());
        }
        let mut kinds = BTreeSet::new();
        for source in &self.sources {
            source.validate()?;
            if !kinds.insert(source.kind) {
                return Err("deployment_config_source_duplicate".to_owned());
            }
            if source.kind == DeploymentConfigSourceKind::ProjectFile
                && (!self.project_trusted
                    || source.trust_digest.as_deref() != Some(self.project_trust_digest.as_str()))
            {
                return Err("deployment_config_project_trust_mismatch".to_owned());
            }
            if source.kind == DeploymentConfigSourceKind::Environment {
                let key = source
                    .reference
                    .strip_prefix("env:")
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "deployment_config_environment_reference_invalid".to_owned())?;
                if !self
                    .allowed_environment_keys
                    .iter()
                    .any(|allowed| allowed == key)
                {
                    return Err("deployment_config_environment_not_allowlisted".to_owned());
                }
            }
            if source.kind == DeploymentConfigSourceKind::CommandLine && source.reference == "cli:"
            {
                return Err("deployment_config_command_line_reference_invalid".to_owned());
            }
        }
        if serde_json::to_vec(&self.effective_non_secret_config)
            .map_or(true, |bytes| bytes.len() > MAX_CONFIG_BYTES)
            || contains_raw_secret(&self.effective_non_secret_config)
        {
            return Err("deployment_config_secret_or_size_invalid".to_owned());
        }
        if self.impact != derive_impact(&self.sources, &self.effective_non_secret_config) {
            return Err("deployment_config_impact_mismatch".to_owned());
        }
        if self.secret_refs.len() > MAX_CONFIG_SECRET_REFS
            || self
                .secret_refs
                .windows(2)
                .any(|pair| pair[0].reference_digest >= pair[1].reference_digest)
        {
            return Err("deployment_config_secret_refs_invalid".to_owned());
        }
        for secret_ref in &self.secret_refs {
            secret_ref.validate()?;
        }
        if self.redacted_digest != json_digest(&redact_value(&self.effective_non_secret_config)) {
            return Err("deployment_config_redacted_digest_mismatch".to_owned());
        }
        if self.config_revision != self.config_revision_digest() {
            return Err("deployment_config_revision_mismatch".to_owned());
        }
        valid_digest(&self.snapshot_digest, "deployment_config_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("deployment_config_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn source_order(&self) -> impl Iterator<Item = &DeploymentConfigSource> {
        self.sources.iter()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "project_trust_digest": self.project_trust_digest,
            "project_trusted": self.project_trusted,
            "allowed_environment_keys": self.allowed_environment_keys,
            "sources": self.sources,
            "effective_non_secret_config": self.effective_non_secret_config,
            "secret_refs": self.secret_refs,
            "impact": self.impact,
            "redacted_digest": self.redacted_digest,
            "config_revision": self.config_revision,
        }))
    }

    pub fn config_revision_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "project_trust_digest": self.project_trust_digest,
            "project_trusted": self.project_trusted,
            "allowed_environment_keys": self.allowed_environment_keys,
            "sources": self.sources,
            "effective_non_secret_config": self.effective_non_secret_config,
            "secret_refs": self.secret_refs,
            "impact": derive_impact(&self.sources, &self.effective_non_secret_config),
        }))
    }

    pub fn diff(&self, next: &Self) -> Result<ConfigDiff, String> {
        self.validate()?;
        next.validate()?;
        let mut changed_paths = changed_paths(
            &self.effective_non_secret_config,
            &next.effective_non_secret_config,
        );
        if self.secret_refs != next.secret_refs || self.sources != next.sources {
            changed_paths.push("<sources>".to_owned());
        }
        changed_paths.sort();
        changed_paths.dedup();
        let changed_digests = changed_paths
            .iter()
            .map(|path| {
                if path == "<sources>" {
                    json_digest(&serde_json::json!({
                        "path": path,
                        "old": self.sources,
                        "new": next.sources,
                    }))
                } else {
                    json_digest(&serde_json::json!({
                        "path": path,
                        "old": self.effective_non_secret_config.get(path),
                        "new": next.effective_non_secret_config.get(path),
                    }))
                }
            })
            .collect::<Vec<_>>();
        let impact = if changed_paths.is_empty() {
            ConfigChangeImpact {
                restart_required: false,
                migration_required: false,
                lease_reacquire_required: false,
            }
        } else {
            derive_impact(&next.sources, &next.effective_non_secret_config)
        };
        ConfigDiff::new(
            &self.config_revision,
            &next.config_revision,
            changed_paths,
            changed_digests,
            impact,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDiff {
    pub schema: String,
    pub version: SchemaVersion,
    pub from_revision: String,
    pub to_revision: String,
    pub changed_paths: Vec<String>,
    pub changed_digests: Vec<String>,
    pub impact: ConfigChangeImpact,
    pub diff_digest: String,
}

impl ConfigDiff {
    fn new(
        from_revision: &str,
        to_revision: &str,
        changed_paths: Vec<String>,
        changed_digests: Vec<String>,
        impact: ConfigChangeImpact,
    ) -> Result<Self, String> {
        let mut diff = Self {
            schema: DEPLOYMENT_CONFIG_DIFF_SCHEMA.to_owned(),
            version: DEPLOYMENT_CONFIG_VERSION,
            from_revision: from_revision.to_owned(),
            to_revision: to_revision.to_owned(),
            changed_paths,
            changed_digests,
            impact,
            diff_digest: String::new(),
        };
        diff.diff_digest = diff.digest();
        diff.validate()?;
        Ok(diff)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_CONFIG_DIFF_SCHEMA
            || self.version != DEPLOYMENT_CONFIG_VERSION
            || self.changed_paths.len() != self.changed_digests.len()
            || self.changed_paths.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("deployment_config_diff_invalid".to_owned());
        }
        valid_digest(&self.from_revision, "deployment_config_diff_from_revision")?;
        valid_digest(&self.to_revision, "deployment_config_diff_to_revision")?;
        for digest in &self.changed_digests {
            valid_digest(digest, "deployment_config_diff_changed_digest")?;
        }
        valid_digest(&self.diff_digest, "deployment_config_diff_digest")?;
        if self.diff_digest != self.digest() {
            return Err("deployment_config_diff_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "from_revision": self.from_revision,
            "to_revision": self.to_revision,
            "changed_paths": self.changed_paths,
            "changed_digests": self.changed_digests,
            "impact": self.impact,
        }))
    }
}

fn derive_impact(sources: &[DeploymentConfigSource], effective: &Value) -> ConfigChangeImpact {
    let mut impact = ConfigChangeImpact {
        restart_required: sources.iter().any(|source| {
            matches!(
                source.kind,
                DeploymentConfigSourceKind::ProjectFile
                    | DeploymentConfigSourceKind::Environment
                    | DeploymentConfigSourceKind::CommandLine
            )
        }),
        migration_required: false,
        lease_reacquire_required: false,
    };
    for path in changed_paths(&Value::Object(Default::default()), effective) {
        let lower = path.to_ascii_lowercase();
        impact.restart_required = true;
        if lower.contains("migration") || lower.contains("schema") || lower.contains("store") {
            impact.migration_required = true;
        }
        if lower.contains("authority") || lower.contains("root") || lower.contains("lease") {
            impact.lease_reacquire_required = true;
        }
    }
    impact
}

fn changed_paths(old: &Value, new: &Value) -> Vec<String> {
    let (Some(old), Some(new)) = (old.as_object(), new.as_object()) else {
        return if old == new {
            Vec::new()
        } else {
            vec!["<root>".to_owned()]
        };
    };
    let mut keys = BTreeSet::new();
    keys.extend(old.keys().cloned());
    keys.extend(new.keys().cloned());
    keys.into_iter()
        .filter(|key| old.get(key) != new.get(key))
        .collect()
}

fn validate_source_reference(
    reference: &str,
    kind: DeploymentConfigSourceKind,
) -> Result<(), String> {
    let suffix = reference
        .strip_prefix(kind.prefix())
        .ok_or_else(|| "deployment_config_source_reference_kind_mismatch".to_owned())?;
    if suffix.is_empty()
        || suffix.contains("..")
        || suffix.contains('\\')
        || suffix.starts_with('/')
        || contains_raw_marker(suffix)
    {
        return Err("deployment_config_source_reference_unsafe".to_owned());
    }
    if kind == DeploymentConfigSourceKind::Environment
        && (!suffix
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            || suffix.len() > 128)
    {
        return Err("deployment_config_environment_key_invalid".to_owned());
    }
    if kind == DeploymentConfigSourceKind::CommandLine && !suffix.starts_with('-') {
        return Err("deployment_config_command_line_reference_invalid".to_owned());
    }
    Ok(())
}

fn validate_environment_key(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err("deployment_config_environment_key_invalid".to_owned());
    }
    Ok(())
}

fn contains_raw_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "token=",
        "password=",
        "api_key=",
        "secret=",
        "private_key",
        "client_secret",
        "bearer ",
        "authorization:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn contains_raw_secret(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            let normalized = key.to_ascii_lowercase();
            let secret_key = matches!(
                normalized.as_str(),
                "secret"
                    | "token"
                    | "password"
                    | "api_key"
                    | "access_token"
                    | "refresh_token"
                    | "authorization"
            );
            let reference_key = normalized.ends_with("_ref")
                || normalized.ends_with("_id")
                || normalized.ends_with("_digest");
            (secret_key && !reference_key) || contains_raw_secret(value)
        }),
        Value::Array(values) => values.iter().any(contains_raw_secret),
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            lower.contains("bearer ") || lower.starts_with("sk-")
        }
        _ => false,
    }
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 512 || value.contains(['\0', '\r', '\n']) {
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
