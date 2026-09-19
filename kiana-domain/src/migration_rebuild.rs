//! Post-migration projection, index and receipt invariant contract.
//!
//! The evaluator proves that a rebuilt read model catches up to the committed source before a
//! ready gate opens. It never rewrites facts, runs a projector, or treats an index generation as
//! authority.

use crate::{json_digest, MigrationRegistry, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const MIGRATION_REBUILD_SCHEMA: &str = "kiana.migration-rebuild.v1";
pub const MIGRATION_REBUILD_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationRebuildStatus {
    Ready,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRebuildFacts {
    pub registry_digest: String,
    pub source_cursor: u64,
    pub projection_cursor: u64,
    pub source_generation: u64,
    pub projection_generation: u64,
    pub expected_generation: u64,
    pub index_generation: u64,
    pub receipt_generation: u64,
    pub source_replay_digest: String,
    pub projection_digest: String,
    pub index_digest: String,
    pub receipt_source_digest: String,
    pub receipt_refs_present: bool,
}

impl MigrationRebuildFacts {
    pub fn validate(&self) -> Result<(), String> {
        if self.source_cursor == 0
            || self.projection_cursor == 0
            || self.source_generation == 0
            || self.projection_generation == 0
            || self.expected_generation == 0
            || self.index_generation == 0
            || self.receipt_generation == 0
        {
            return Err("migration_rebuild_fact_counter_invalid".to_owned());
        }
        for (value, field) in [
            (&self.registry_digest, "migration_rebuild_registry_digest"),
            (
                &self.source_replay_digest,
                "migration_rebuild_source_digest",
            ),
            (
                &self.projection_digest,
                "migration_rebuild_projection_digest",
            ),
            (&self.index_digest, "migration_rebuild_index_digest"),
            (
                &self.receipt_source_digest,
                "migration_rebuild_receipt_source_digest",
            ),
        ] {
            validate_digest(value, field)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::to_value(self).unwrap_or_default())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRebuildReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub registry_digest: String,
    pub status: MigrationRebuildStatus,
    pub ready_gate: bool,
    pub source_cursor: u64,
    pub projection_cursor: u64,
    pub projection_generation: u64,
    pub index_generation: u64,
    pub receipt_generation: u64,
    pub fact_digest: String,
    pub reason: String,
    pub remediation: String,
    pub report_digest: String,
}

impl MigrationRebuildReport {
    pub fn evaluate(
        registry: &MigrationRegistry,
        facts: &MigrationRebuildFacts,
    ) -> Result<Self, String> {
        registry.validate()?;
        facts.validate()?;
        if facts.registry_digest != registry.registry_digest {
            return Err("migration_rebuild_registry_checksum_drift".to_owned());
        }
        let fact_digest = facts.digest();
        let (status, reason, remediation) = if facts.projection_cursor > facts.source_cursor {
            (
                MigrationRebuildStatus::Blocked,
                "migration_projection_over_facts",
                "rebuild from the committed source cursor without inventing projection facts",
            )
        } else if facts.projection_cursor < facts.source_cursor {
            (
                MigrationRebuildStatus::Blocked,
                "migration_projection_cursor_lag",
                "catch up the projector before opening the ready gate",
            )
        } else if facts.projection_generation != facts.expected_generation
            || facts.projection_generation < facts.source_generation
        {
            (
                MigrationRebuildStatus::Blocked,
                "migration_projection_generation_stale",
                "rebuild the projection at the target generation",
            )
        } else if facts.index_generation != facts.projection_generation {
            (
                MigrationRebuildStatus::Blocked,
                "migration_index_generation_stale",
                "rebuild or invalidate the index for the current projection generation",
            )
        } else if facts.receipt_generation != facts.projection_generation {
            (
                MigrationRebuildStatus::Blocked,
                "migration_receipt_generation_mismatch",
                "reproject receipts from the same source/projection generation",
            )
        } else if facts.source_replay_digest != facts.projection_digest {
            (
                MigrationRebuildStatus::Blocked,
                "migration_projection_replay_mismatch",
                "replay committed source facts and quarantine the divergent projection",
            )
        } else if facts.index_digest.is_empty()
            || facts.receipt_source_digest != facts.projection_digest
            || !facts.receipt_refs_present
        {
            (
                MigrationRebuildStatus::Blocked,
                "migration_receipt_source_invariant_failed",
                "restore source event references before publishing the read model",
            )
        } else {
            (
                MigrationRebuildStatus::Ready,
                "ok",
                "publish read-only handles after the source cursor and generation gate",
            )
        };
        let mut report = Self {
            schema: MIGRATION_REBUILD_SCHEMA.to_owned(),
            version: MIGRATION_REBUILD_VERSION,
            registry_digest: facts.registry_digest.clone(),
            status,
            ready_gate: status == MigrationRebuildStatus::Ready,
            source_cursor: facts.source_cursor,
            projection_cursor: facts.projection_cursor,
            projection_generation: facts.projection_generation,
            index_generation: facts.index_generation,
            receipt_generation: facts.receipt_generation,
            fact_digest,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MIGRATION_REBUILD_SCHEMA
            || self.version != MIGRATION_REBUILD_VERSION
            || self.source_cursor == 0
            || self.projection_cursor == 0
            || self.projection_generation == 0
            || self.index_generation == 0
            || self.receipt_generation == 0
            || self.reason.trim().is_empty()
            || self.remediation.trim().is_empty()
            || self.ready_gate != (self.status == MigrationRebuildStatus::Ready)
        {
            return Err("migration_rebuild_report_invalid".to_owned());
        }
        validate_digest(&self.registry_digest, "migration_rebuild_registry_digest")?;
        validate_digest(&self.fact_digest, "migration_rebuild_fact_digest")?;
        validate_digest(&self.report_digest, "migration_rebuild_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("migration_rebuild_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "registry_digest": self.registry_digest,
            "status": self.status,
            "ready_gate": self.ready_gate,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "index_generation": self.index_generation,
            "receipt_generation": self.receipt_generation,
            "fact_digest": self.fact_digest,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
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
