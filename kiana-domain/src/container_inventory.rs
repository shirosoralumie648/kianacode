//! Server-owned container inventory and restart-recovery contract.
//!
//! This record is the durable shape a future EventLog-backed inventory projection must satisfy.
//! It does not inspect or mutate a runtime; missing/stale/unknown identity remains unavailable or
//! quarantined rather than recovered from a container name.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const CONTAINER_INVENTORY_SCHEMA: &str = "kiana.container-inventory.v1";
pub const CONTAINER_INVENTORY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerInventoryState {
    Active,
    Quiesced,
    Unknown,
    Quarantined,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerInventoryRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub environment_id: String,
    pub owner_id: String,
    pub scope_digest: String,
    pub plan_digest: String,
    pub root_identity_digest: String,
    pub runtime_identity_digest: String,
    pub lease_digest: String,
    pub lease_epoch: u64,
    pub restart_epoch: u64,
    pub source_cursor: u64,
    pub state: ContainerInventoryState,
    pub runtime_identity_verified: bool,
    pub old_process_fenced: bool,
    pub record_digest: String,
}

impl ContainerInventoryRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        environment_id: impl Into<String>,
        owner_id: impl Into<String>,
        scope_digest: impl Into<String>,
        plan_digest: impl Into<String>,
        root_identity_digest: impl Into<String>,
        runtime_identity_digest: impl Into<String>,
        lease_digest: impl Into<String>,
        lease_epoch: u64,
        restart_epoch: u64,
        source_cursor: u64,
        state: ContainerInventoryState,
        runtime_identity_verified: bool,
        old_process_fenced: bool,
    ) -> Result<Self, String> {
        let mut record = Self {
            schema: CONTAINER_INVENTORY_SCHEMA.to_owned(),
            version: CONTAINER_INVENTORY_VERSION,
            environment_id: environment_id.into(),
            owner_id: owner_id.into(),
            scope_digest: scope_digest.into(),
            plan_digest: plan_digest.into(),
            root_identity_digest: root_identity_digest.into(),
            runtime_identity_digest: runtime_identity_digest.into(),
            lease_digest: lease_digest.into(),
            lease_epoch,
            restart_epoch,
            source_cursor,
            state,
            runtime_identity_verified,
            old_process_fenced,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTAINER_INVENTORY_SCHEMA
            || self.version != CONTAINER_INVENTORY_VERSION
            || self.environment_id.trim().is_empty()
            || self.environment_id.len() > 256
            || self.owner_id.trim().is_empty()
            || self.owner_id.len() > 256
            || self.lease_epoch == 0
            || self.restart_epoch == 0
            || self.source_cursor == 0
        {
            return Err("container_inventory_record_invalid".to_owned());
        }
        for (value, field) in [
            (&self.scope_digest, "container_inventory_scope_digest"),
            (&self.plan_digest, "container_inventory_plan_digest"),
            (
                &self.root_identity_digest,
                "container_inventory_root_digest",
            ),
            (
                &self.runtime_identity_digest,
                "container_inventory_runtime_digest",
            ),
            (&self.lease_digest, "container_inventory_lease_digest"),
            (&self.record_digest, "container_inventory_record_digest"),
        ] {
            validate_digest(value, field)?;
        }
        if self.state == ContainerInventoryState::Active
            && (!self.runtime_identity_verified || !self.old_process_fenced)
        {
            return Err("container_inventory_active_evidence_missing".to_owned());
        }
        if self.record_digest != self.digest() {
            return Err("container_inventory_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Re-admit a recorded environment only when all server-owned identity and restart fences
    /// match. This is a decision contract, not a runtime attach operation.
    pub fn recover(
        &self,
        owner_id: &str,
        scope_digest: &str,
        plan_digest: &str,
        root_identity_digest: &str,
        current_lease_epoch: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if self.state != ContainerInventoryState::Active {
            return Err("container_inventory_requires_reconcile".to_owned());
        }
        if self.owner_id != owner_id
            || self.scope_digest != scope_digest
            || self.plan_digest != plan_digest
            || self.root_identity_digest != root_identity_digest
        {
            return Err("container_inventory_identity_mismatch".to_owned());
        }
        if current_lease_epoch == 0 || self.lease_epoch < current_lease_epoch {
            return Err("container_inventory_lease_stale".to_owned());
        }
        if !self.runtime_identity_verified || !self.old_process_fenced {
            return Err("result_unknown:container_inventory_runtime_unverified".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "environment_id": self.environment_id,
            "owner_id": self.owner_id,
            "scope_digest": self.scope_digest,
            "plan_digest": self.plan_digest,
            "root_identity_digest": self.root_identity_digest,
            "runtime_identity_digest": self.runtime_identity_digest,
            "lease_digest": self.lease_digest,
            "lease_epoch": self.lease_epoch,
            "restart_epoch": self.restart_epoch,
            "source_cursor": self.source_cursor,
            "state": self.state,
            "runtime_identity_verified": self.runtime_identity_verified,
            "old_process_fenced": self.old_process_fenced,
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
