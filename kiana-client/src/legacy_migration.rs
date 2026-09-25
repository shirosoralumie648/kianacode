//! One-way legacy Web/CLI compatibility mapping.
//!
//! Legacy aliases are translated to the typed client operation vocabulary. The adapter never
//! writes facts, starts a second loop, or silently accepts an unknown legacy route.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const LEGACY_MIGRATION_SCHEMA: &str = "kiana.ui-legacy-migration.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacySurface {
    Web,
    Cli,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationDisposition {
    Forward,
    Deprecated,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRouteMapping {
    pub schema: String,
    pub surface: LegacySurface,
    pub legacy_name: String,
    pub canonical_name: Option<String>,
    pub disposition: MigrationDisposition,
    pub deprecation: Option<String>,
    pub read_only: bool,
    pub writes_facts: bool,
    pub requires_typed_client: bool,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum LegacyMigrationError {
    #[error("legacy_migration_schema_invalid")]
    SchemaInvalid,
    #[error("legacy_route_unknown")]
    Unknown,
    #[error("legacy_route_input_invalid")]
    InputInvalid,
    #[error("legacy_route_writes_facts")]
    WritesFacts,
}

pub fn map_legacy_route(
    surface: LegacySurface,
    legacy_name: &str,
) -> Result<LegacyRouteMapping, LegacyMigrationError> {
    if legacy_name.trim().is_empty() || legacy_name.len() > 256 || legacy_name.contains('\0') {
        return Err(LegacyMigrationError::InputInvalid);
    }
    let (canonical_name, disposition, deprecation) = match legacy_name {
        "GET /api/state" | "state" => (
            "ui.snapshot",
            MigrationDisposition::Deprecated,
            Some("use typed snapshot query"),
        ),
        "GET /api/events" | "events" => (
            "ui.feed",
            MigrationDisposition::Deprecated,
            Some("use typed feed subscription"),
        ),
        "run" | "turn" => (
            "turn.prompt",
            MigrationDisposition::Deprecated,
            Some("use typed action client"),
        ),
        "cancel" => (
            "turn.cancel",
            MigrationDisposition::Deprecated,
            Some("use typed action client"),
        ),
        "receipt" => (
            "receipt.query",
            MigrationDisposition::Deprecated,
            Some("use typed receipt query"),
        ),
        _ => return Err(LegacyMigrationError::Unknown),
    };
    let mapping = LegacyRouteMapping {
        schema: LEGACY_MIGRATION_SCHEMA.to_owned(),
        surface,
        legacy_name: legacy_name.to_owned(),
        canonical_name: Some(canonical_name.to_owned()),
        disposition,
        deprecation: deprecation.map(str::to_owned),
        read_only: matches!(canonical_name, "ui.snapshot" | "ui.feed" | "receipt.query"),
        writes_facts: false,
        requires_typed_client: true,
    };
    validate_mapping(&mapping)?;
    Ok(mapping)
}

pub fn validate_mapping(mapping: &LegacyRouteMapping) -> Result<(), LegacyMigrationError> {
    if mapping.schema != LEGACY_MIGRATION_SCHEMA || mapping.legacy_name.trim().is_empty() {
        return Err(LegacyMigrationError::SchemaInvalid);
    }
    if mapping.writes_facts || !mapping.requires_typed_client {
        return Err(LegacyMigrationError::WritesFacts);
    }
    if mapping.disposition == MigrationDisposition::Rejected {
        if mapping.canonical_name.is_some() {
            return Err(LegacyMigrationError::SchemaInvalid);
        }
    } else if mapping.canonical_name.is_none() || mapping.deprecation.is_none() {
        return Err(LegacyMigrationError::SchemaInvalid);
    }
    Ok(())
}
