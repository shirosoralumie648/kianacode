//! Read-only migration preflight matrix.
//!
//! The evaluator consumes server-provided facts and returns a bounded report. It never acquires a
//! migration lock, writes a store, invokes a provider, or changes a projection. A failed axis is
//! represented in the report with a stable reason and remediation instead of being turned into a
//! generic success/skip.

use crate::{json_digest, MigrationRegistry, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const MIGRATION_PREFLIGHT_SCHEMA: &str = "kiana.migration-preflight.v1";
pub const MIGRATION_PREFLIGHT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MIGRATION_PREFLIGHT_AXIS_COUNT: usize = 9;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPreflightAxis {
    Store,
    Schema,
    Projection,
    Workflow,
    Provider,
    Config,
    Space,
    Clock,
    Lease,
}

impl MigrationPreflightAxis {
    pub const ALL: [Self; MIGRATION_PREFLIGHT_AXIS_COUNT] = [
        Self::Store,
        Self::Schema,
        Self::Projection,
        Self::Workflow,
        Self::Provider,
        Self::Config,
        Self::Space,
        Self::Clock,
        Self::Lease,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPreflightStatus {
    Ready,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationDigestFact {
    pub observed_digest: String,
    pub expected_digest: String,
    pub matches: bool,
}

impl MigrationDigestFact {
    pub fn validate(&self, field: &str) -> Result<(), String> {
        validate_digest(&self.observed_digest, field)?;
        validate_digest(&self.expected_digest, field)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationSchemaFact {
    pub current_format_version: u32,
    pub current_major: u32,
    pub target_major: u32,
    pub observed_schema_digest: String,
    pub expected_schema_digest: String,
}

impl MigrationSchemaFact {
    pub fn validate(&self) -> Result<(), String> {
        if self.current_format_version == 0 || self.current_major == 0 || self.target_major == 0 {
            return Err("migration_preflight_schema_fact_invalid".to_owned());
        }
        validate_digest(
            &self.observed_schema_digest,
            "migration_preflight_observed_schema_digest",
        )?;
        validate_digest(
            &self.expected_schema_digest,
            "migration_preflight_expected_schema_digest",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationProjectionFact {
    pub source_cursor: u64,
    pub projection_cursor: u64,
    pub current_generation: u64,
    pub expected_generation: u64,
}

impl MigrationProjectionFact {
    pub fn validate(&self) -> Result<(), String> {
        if self.source_cursor == 0
            || self.projection_cursor == 0
            || self.current_generation == 0
            || self.expected_generation == 0
        {
            return Err("migration_preflight_projection_fact_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationSpaceFact {
    pub available_bytes: u64,
    pub required_bytes: u64,
    pub available_inodes: u64,
    pub required_inodes: u64,
}

impl MigrationSpaceFact {
    pub fn validate(&self) -> Result<(), String> {
        if self.required_bytes == 0 || self.required_inodes == 0 {
            return Err("migration_preflight_space_fact_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationClockFact {
    pub trusted: bool,
    pub revision: u64,
}

impl MigrationClockFact {
    pub fn validate(&self) -> Result<(), String> {
        if self.revision == 0 {
            return Err("migration_preflight_clock_fact_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationLeaseFact {
    pub runner_held: bool,
    pub old_writer_count: u32,
    pub active_unknown_count: u32,
    pub owner_available: bool,
}

impl MigrationLeaseFact {
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPreflightFacts {
    pub store: MigrationDigestFact,
    pub schema: MigrationSchemaFact,
    pub projection: MigrationProjectionFact,
    pub workflow: MigrationDigestFact,
    pub provider: MigrationDigestFact,
    pub config: MigrationDigestFact,
    pub space: MigrationSpaceFact,
    pub clock: MigrationClockFact,
    pub lease: MigrationLeaseFact,
    pub verified_backup: bool,
}

impl MigrationPreflightFacts {
    pub fn validate(&self) -> Result<(), String> {
        self.store.validate("migration_preflight_store_digest")?;
        self.schema.validate()?;
        self.projection.validate()?;
        self.workflow
            .validate("migration_preflight_workflow_digest")?;
        self.provider
            .validate("migration_preflight_provider_digest")?;
        self.config.validate("migration_preflight_config_digest")?;
        self.space.validate()?;
        self.clock.validate()?;
        self.lease.validate()?;
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::to_value(self).unwrap_or_default())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationAxisResult {
    pub axis: MigrationPreflightAxis,
    pub status: MigrationPreflightStatus,
    pub reason: String,
    pub remediation: String,
    pub observed_digest: String,
}

impl MigrationAxisResult {
    fn ready(axis: MigrationPreflightAxis, observed_digest: String) -> Self {
        Self {
            axis,
            status: MigrationPreflightStatus::Ready,
            reason: "ok".to_owned(),
            remediation: "none".to_owned(),
            observed_digest,
        }
    }

    fn blocked(
        axis: MigrationPreflightAxis,
        reason: &str,
        remediation: &str,
        observed_digest: String,
    ) -> Self {
        Self {
            axis,
            status: MigrationPreflightStatus::Blocked,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            observed_digest,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPreflightReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub registry_digest: String,
    pub source_format_version: u32,
    pub target_format_version: u32,
    pub generated_at_unix_ms: u64,
    pub read_only: bool,
    pub effect_calls: u32,
    pub fact_digest: String,
    pub axes: Vec<MigrationAxisResult>,
    pub report_digest: String,
}

impl MigrationPreflightReport {
    pub fn evaluate(
        registry: &MigrationRegistry,
        facts: &MigrationPreflightFacts,
        generated_at_unix_ms: u64,
    ) -> Result<Self, String> {
        registry.validate()?;
        facts.validate()?;
        if generated_at_unix_ms == 0 {
            return Err("migration_preflight_timestamp_invalid".to_owned());
        }
        let source_format_version = registry
            .ordered_steps()
            .first()
            .map(|step| step.from_format_version)
            .ok_or_else(|| "migration_registry_header_invalid".to_owned())?;
        let target_format_version = registry
            .ordered_steps()
            .last()
            .map(|step| step.to_format_version)
            .ok_or_else(|| "migration_registry_header_invalid".to_owned())?;
        let fact_digest = facts.digest();
        let mut axes = Vec::with_capacity(MIGRATION_PREFLIGHT_AXIS_COUNT);
        axes.push(if facts.store.matches {
            MigrationAxisResult::ready(
                MigrationPreflightAxis::Store,
                facts.store.observed_digest.clone(),
            )
        } else {
            MigrationAxisResult::blocked(
                MigrationPreflightAxis::Store,
                "migration_store_digest_mismatch",
                "re-open the store and capture a fresh immutable snapshot",
                facts.store.observed_digest.clone(),
            )
        });
        axes.push(
            if facts.schema.current_major == facts.schema.target_major
                && facts.schema.current_format_version <= source_format_version
                && facts.schema.observed_schema_digest == facts.schema.expected_schema_digest
            {
                MigrationAxisResult::ready(
                    MigrationPreflightAxis::Schema,
                    facts.schema.observed_schema_digest.clone(),
                )
            } else if facts.schema.current_format_version > target_format_version {
                MigrationAxisResult::blocked(
                    MigrationPreflightAxis::Schema,
                    "migration_downgrade_denied",
                    "select a forward migration or restore a verified compatible root",
                    facts.schema.observed_schema_digest.clone(),
                )
            } else if facts.schema.current_major != facts.schema.target_major {
                MigrationAxisResult::blocked(
                    MigrationPreflightAxis::Schema,
                    "migration_schema_major_mismatch",
                    "install a registry with an explicit major-compatible migration",
                    facts.schema.observed_schema_digest.clone(),
                )
            } else {
                MigrationAxisResult::blocked(
                    MigrationPreflightAxis::Schema,
                    "migration_schema_precondition_mismatch",
                    "refresh the source schema snapshot before planning",
                    facts.schema.observed_schema_digest.clone(),
                )
            },
        );
        axes.push(
            if facts.projection.source_cursor == facts.projection.projection_cursor
                && facts.projection.current_generation == facts.projection.expected_generation
            {
                MigrationAxisResult::ready(
                    MigrationPreflightAxis::Projection,
                    json_digest(&serde_json::json!({
                        "source_cursor": facts.projection.source_cursor,
                        "generation": facts.projection.current_generation,
                    })),
                )
            } else {
                MigrationAxisResult::blocked(
                    MigrationPreflightAxis::Projection,
                    "migration_projection_lag",
                    "catch up or rebuild the projection before migration",
                    json_digest(&serde_json::to_value(&facts.projection).unwrap_or_default()),
                )
            },
        );
        axes.push(digest_axis(
            MigrationPreflightAxis::Workflow,
            &facts.workflow,
            "migration_workflow_digest_mismatch",
            "pin the workflow definition and capture a compatible revision",
        ));
        axes.push(digest_axis(
            MigrationPreflightAxis::Provider,
            &facts.provider,
            "migration_provider_digest_mismatch",
            "pin the provider route/configuration before migration",
        ));
        axes.push(digest_axis(
            MigrationPreflightAxis::Config,
            &facts.config,
            "migration_config_digest_mismatch",
            "freeze or reload the approved config snapshot",
        ));
        axes.push(
            if facts.space.available_bytes >= facts.space.required_bytes
                && facts.space.available_inodes >= facts.space.required_inodes
            {
                MigrationAxisResult::ready(
                    MigrationPreflightAxis::Space,
                    json_digest(&serde_json::to_value(&facts.space).unwrap_or_default()),
                )
            } else {
                MigrationAxisResult::blocked(
                    MigrationPreflightAxis::Space,
                    "migration_space_insufficient",
                    "free capacity without deleting facts or retrying the migration",
                    json_digest(&serde_json::to_value(&facts.space).unwrap_or_default()),
                )
            },
        );
        axes.push(if facts.clock.trusted {
            MigrationAxisResult::ready(
                MigrationPreflightAxis::Clock,
                json_digest(&serde_json::to_value(&facts.clock).unwrap_or_default()),
            )
        } else {
            MigrationAxisResult::blocked(
                MigrationPreflightAxis::Clock,
                "migration_clock_untrusted",
                "capture a newer trusted clock observation",
                json_digest(&serde_json::to_value(&facts.clock).unwrap_or_default()),
            )
        });
        axes.push(
            if !facts.lease.runner_held
                && facts.lease.old_writer_count == 0
                && facts.lease.active_unknown_count == 0
                && facts.lease.owner_available
                && facts.verified_backup
            {
                MigrationAxisResult::ready(
                    MigrationPreflightAxis::Lease,
                    json_digest(&serde_json::to_value(&facts.lease).unwrap_or_default()),
                )
            } else if facts.lease.runner_held {
                MigrationAxisResult::blocked(
                    MigrationPreflightAxis::Lease,
                    "migration_runner_concurrent",
                    "wait for the existing migration owner or fence it explicitly",
                    json_digest(&serde_json::to_value(&facts.lease).unwrap_or_default()),
                )
            } else if facts.lease.old_writer_count > 0 {
                MigrationAxisResult::blocked(
                    MigrationPreflightAxis::Lease,
                    "migration_old_writer_active",
                    "drain and fence every old writer before applying a migration",
                    json_digest(&serde_json::to_value(&facts.lease).unwrap_or_default()),
                )
            } else if facts.lease.active_unknown_count > 0 {
                MigrationAxisResult::blocked(
                    MigrationPreflightAxis::Lease,
                    "migration_active_unknown",
                    "reconcile unknown operations before migration",
                    json_digest(&serde_json::to_value(&facts.lease).unwrap_or_default()),
                )
            } else if !facts.verified_backup {
                MigrationAxisResult::blocked(
                    MigrationPreflightAxis::Lease,
                    "migration_backup_unverified",
                    "create and verify the required backup before migration",
                    json_digest(&serde_json::to_value(&facts.lease).unwrap_or_default()),
                )
            } else {
                MigrationAxisResult::blocked(
                    MigrationPreflightAxis::Lease,
                    "migration_owner_unavailable",
                    "acquire the approved migration owner during the apply phase",
                    json_digest(&serde_json::to_value(&facts.lease).unwrap_or_default()),
                )
            },
        );
        let mut report = Self {
            schema: MIGRATION_PREFLIGHT_SCHEMA.to_owned(),
            version: MIGRATION_PREFLIGHT_VERSION,
            registry_digest: registry.registry_digest.clone(),
            source_format_version,
            target_format_version,
            generated_at_unix_ms,
            read_only: true,
            effect_calls: 0,
            fact_digest,
            axes,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MIGRATION_PREFLIGHT_SCHEMA
            || self.version != MIGRATION_PREFLIGHT_VERSION
            || self.registry_digest.is_empty()
            || self.source_format_version == 0
            || self.target_format_version < self.source_format_version
            || self.generated_at_unix_ms == 0
            || !self.read_only
            || self.effect_calls != 0
            || self.axes.len() != MIGRATION_PREFLIGHT_AXIS_COUNT
        {
            return Err("migration_preflight_report_invalid".to_owned());
        }
        validate_digest(&self.registry_digest, "migration_preflight_registry_digest")?;
        validate_digest(&self.fact_digest, "migration_preflight_fact_digest")?;
        validate_digest(&self.report_digest, "migration_preflight_report_digest")?;
        for (expected, actual) in MigrationPreflightAxis::ALL.iter().zip(&self.axes) {
            if actual.axis != *expected
                || actual.reason.trim().is_empty()
                || actual.remediation.trim().is_empty()
                || actual.observed_digest.trim().is_empty()
            {
                return Err("migration_preflight_axis_invalid".to_owned());
            }
        }
        if self.report_digest != self.digest() {
            return Err("migration_preflight_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.axes
            .iter()
            .all(|axis| axis.status == MigrationPreflightStatus::Ready)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "registry_digest": self.registry_digest,
            "source_format_version": self.source_format_version,
            "target_format_version": self.target_format_version,
            "generated_at_unix_ms": self.generated_at_unix_ms,
            "read_only": self.read_only,
            "effect_calls": self.effect_calls,
            "fact_digest": self.fact_digest,
            "axes": self.axes,
        }))
    }
}

fn digest_axis(
    axis: MigrationPreflightAxis,
    fact: &MigrationDigestFact,
    reason: &str,
    remediation: &str,
) -> MigrationAxisResult {
    if fact.matches && fact.observed_digest == fact.expected_digest {
        MigrationAxisResult::ready(axis, fact.observed_digest.clone())
    } else {
        MigrationAxisResult::blocked(axis, reason, remediation, fact.observed_digest.clone())
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
