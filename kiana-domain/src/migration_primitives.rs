//! Pure bounded migration phase and checkpoint primitives.
//!
//! These values describe an allowed migration transition; they do not mutate storage or call an
//! adapter. The later runner may execute a validated plan, but it must first commit the returned
//! checkpoint through its own lock/fence and EventLog boundary.

use crate::{json_digest, MigrationRegistry, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const MIGRATION_PRIMITIVE_SCHEMA: &str = "kiana.migration-primitive.v1";
pub const MIGRATION_PRIMITIVE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_MIGRATION_BATCH: u32 = 1_024;
pub const MAX_MIGRATION_ITEMS: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPrimitivePhase {
    Expand,
    Backfill,
    Verify,
    Switch,
    Contract,
}

impl MigrationPrimitivePhase {
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Expand => Some(Self::Backfill),
            Self::Backfill => Some(Self::Verify),
            Self::Verify => Some(Self::Switch),
            Self::Switch => Some(Self::Contract),
            Self::Contract => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPrimitiveTarget {
    Store,
    Projection,
    Artifact,
    Config,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationCheckpoint {
    pub schema: String,
    pub version: SchemaVersion,
    pub registry_digest: String,
    pub step_id: String,
    pub phase: MigrationPrimitivePhase,
    pub source_cursor: u64,
    pub generation: u64,
    pub phase_complete: bool,
    pub verified: bool,
    pub last_batch_key: Option<String>,
    pub checkpoint_digest: String,
}

impl MigrationCheckpoint {
    pub fn initial(registry: &MigrationRegistry, step_id: &str) -> Result<Self, String> {
        registry.validate()?;
        if !registry
            .ordered_steps()
            .iter()
            .any(|step| step.step_id == step_id)
        {
            return Err("migration_step_unknown".to_owned());
        }
        let mut checkpoint = Self {
            schema: MIGRATION_PRIMITIVE_SCHEMA.to_owned(),
            version: MIGRATION_PRIMITIVE_VERSION,
            registry_digest: registry.registry_digest.clone(),
            step_id: step_id.to_owned(),
            phase: MigrationPrimitivePhase::Expand,
            source_cursor: 0,
            generation: 1,
            phase_complete: false,
            verified: false,
            last_batch_key: None,
            checkpoint_digest: String::new(),
        };
        checkpoint.checkpoint_digest = checkpoint.digest();
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MIGRATION_PRIMITIVE_SCHEMA
            || self.version != MIGRATION_PRIMITIVE_VERSION
            || self.registry_digest.is_empty()
            || self.step_id.trim().is_empty()
            || self.step_id.len() > 256
            || self.generation == 0
            || (self.phase == MigrationPrimitivePhase::Expand && self.verified)
        {
            return Err("migration_checkpoint_invalid".to_owned());
        }
        validate_digest(
            &self.registry_digest,
            "migration_checkpoint_registry_digest",
        )?;
        if let Some(key) = &self.last_batch_key {
            validate_digest(key, "migration_checkpoint_batch_key")?;
        }
        validate_digest(&self.checkpoint_digest, "migration_checkpoint_digest")?;
        if self.checkpoint_digest != self.digest() {
            return Err("migration_checkpoint_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn advance(
        &self,
        registry: &MigrationRegistry,
        plan: &MigrationBatchPlan,
    ) -> Result<Self, String> {
        self.validate()?;
        plan.validate(registry)?;
        if self.registry_digest != plan.registry_digest || self.step_id != plan.step_id {
            return Err("migration_checkpoint_plan_binding_mismatch".to_owned());
        }
        if plan.idempotency_key == self.last_batch_key.as_deref().unwrap_or_default() {
            return Ok(self.clone());
        }
        if plan.source_cursor < self.source_cursor {
            return Err("migration_source_cursor_rollback".to_owned());
        }
        let phase_allowed = (plan.phase == self.phase && !self.phase_complete)
            || (self.phase_complete && self.phase.next() == Some(plan.phase));
        if !phase_allowed {
            return Err("migration_phase_order_invalid".to_owned());
        }
        if plan.phase == MigrationPrimitivePhase::Switch && !self.verified {
            return Err("migration_verify_required_before_switch".to_owned());
        }
        if plan.phase == MigrationPrimitivePhase::Contract && !self.verified {
            return Err("migration_verify_required_before_contract".to_owned());
        }
        let mut next = self.clone();
        next.phase = plan.phase;
        next.source_cursor = plan.source_cursor;
        next.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "migration_checkpoint_generation_overflow".to_owned())?;
        next.phase_complete = plan.phase_complete;
        next.verified =
            self.verified || (plan.phase == MigrationPrimitivePhase::Verify && plan.phase_complete);
        next.last_batch_key = Some(plan.idempotency_key.clone());
        if plan.phase_complete && plan.phase == MigrationPrimitivePhase::Contract {
            next.verified = true;
        }
        next.checkpoint_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "registry_digest": self.registry_digest,
            "step_id": self.step_id,
            "phase": self.phase,
            "source_cursor": self.source_cursor,
            "generation": self.generation,
            "phase_complete": self.phase_complete,
            "verified": self.verified,
            "last_batch_key": self.last_batch_key,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationBatchPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub registry_digest: String,
    pub step_id: String,
    pub phase: MigrationPrimitivePhase,
    pub target: MigrationPrimitiveTarget,
    pub source_cursor: u64,
    pub batch_size: u32,
    pub item_digests: Vec<String>,
    pub phase_complete: bool,
    pub idempotency_key: String,
    pub plan_digest: String,
}

impl MigrationBatchPlan {
    pub fn new(
        registry: &MigrationRegistry,
        checkpoint: &MigrationCheckpoint,
        phase: MigrationPrimitivePhase,
        target: MigrationPrimitiveTarget,
        source_cursor: u64,
        batch_size: u32,
        item_digests: Vec<String>,
        phase_complete: bool,
    ) -> Result<Self, String> {
        registry.validate()?;
        checkpoint.validate()?;
        let step = registry
            .ordered_steps()
            .iter()
            .find(|step| step.step_id == checkpoint.step_id)
            .ok_or_else(|| "migration_step_unknown".to_owned())?;
        if checkpoint.registry_digest != registry.registry_digest
            || source_cursor < checkpoint.source_cursor
            || batch_size == 0
            || batch_size > MAX_MIGRATION_BATCH
            || item_digests.is_empty()
            || item_digests.len() > MAX_MIGRATION_ITEMS
            || item_digests.len() as u32 > batch_size
        {
            return Err("migration_batch_bounds_or_binding_invalid".to_owned());
        }
        for digest in &item_digests {
            validate_digest(digest, "migration_item_digest")?;
        }
        if phase == MigrationPrimitivePhase::Switch && !checkpoint.verified {
            return Err("migration_verify_required_before_switch".to_owned());
        }
        if phase == MigrationPrimitivePhase::Contract && !checkpoint.verified {
            return Err("migration_verify_required_before_contract".to_owned());
        }
        let idempotency_key = json_digest(&serde_json::json!({
            "registry_digest": registry.registry_digest,
            "step_id": step.step_id,
            "phase": phase,
            "target": target,
            "source_cursor": source_cursor,
            "batch_size": batch_size,
            "item_digests": item_digests,
            "phase_complete": phase_complete,
        }));
        let mut plan = Self {
            schema: MIGRATION_PRIMITIVE_SCHEMA.to_owned(),
            version: MIGRATION_PRIMITIVE_VERSION,
            registry_digest: registry.registry_digest.clone(),
            step_id: checkpoint.step_id.clone(),
            phase,
            target,
            source_cursor,
            batch_size,
            item_digests,
            phase_complete,
            idempotency_key,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate(registry)?;
        Ok(plan)
    }

    pub fn validate(&self, registry: &MigrationRegistry) -> Result<(), String> {
        registry.validate()?;
        if self.schema != MIGRATION_PRIMITIVE_SCHEMA
            || self.version != MIGRATION_PRIMITIVE_VERSION
            || self.registry_digest != registry.registry_digest
            || self.step_id.trim().is_empty()
            || self.source_cursor == 0
            || self.batch_size == 0
            || self.batch_size > MAX_MIGRATION_BATCH
            || self.item_digests.is_empty()
            || self.item_digests.len() > MAX_MIGRATION_ITEMS
            || self.item_digests.len() as u32 > self.batch_size
        {
            return Err("migration_batch_plan_invalid".to_owned());
        }
        let step = registry
            .ordered_steps()
            .iter()
            .find(|step| step.step_id == self.step_id)
            .ok_or_else(|| "migration_step_unknown".to_owned())?;
        for digest in &self.item_digests {
            validate_digest(digest, "migration_item_digest")?;
        }
        let expected_key = json_digest(&serde_json::json!({
            "registry_digest": self.registry_digest,
            "step_id": step.step_id,
            "phase": self.phase,
            "target": self.target,
            "source_cursor": self.source_cursor,
            "batch_size": self.batch_size,
            "item_digests": self.item_digests,
            "phase_complete": self.phase_complete,
        }));
        if self.idempotency_key != expected_key {
            return Err("migration_idempotency_key_mismatch".to_owned());
        }
        validate_digest(&self.idempotency_key, "migration_idempotency_key")?;
        validate_digest(&self.plan_digest, "migration_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("migration_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "registry_digest": self.registry_digest,
            "step_id": self.step_id,
            "phase": self.phase,
            "target": self.target,
            "source_cursor": self.source_cursor,
            "batch_size": self.batch_size,
            "item_digests": self.item_digests,
            "phase_complete": self.phase_complete,
            "idempotency_key": self.idempotency_key,
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
