//! Read-only recovery projection for pending approvals and resource ownership.
//!
//! The snapshot is intentionally an inspectable optimization. It is rebuilt from committed
//! EventLog facts and cannot mint approval, budget, lease or Cell authority.

use crate::{
    json_digest, ApprovalId, BudgetLeaseId, CellId, EventCursor, EventId, SchemaVersion,
    StorageLockId, MAX_SOURCE_EVENT_IDS,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const RECOVERY_RESOURCE_SNAPSHOT_SCHEMA: &str = "kiana.recovery-resource-snapshot.v1";
pub const RECOVERY_RESOURCE_SNAPSHOT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RECOVERY_RESOURCE_ENTRIES: usize = 1_024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryResourceSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_cursor: EventCursor,
    pub source_event_ids: Vec<EventId>,
    pub pending_approval_ids: Vec<ApprovalId>,
    pub reserved_budget_lease_ids: Vec<BudgetLeaseId>,
    pub active_resource_lease_ids: Vec<StorageLockId>,
    pub active_cell_ids: Vec<CellId>,
    pub fenced_cell_ids: Vec<CellId>,
    pub snapshot_digest: String,
}

impl RecoveryResourceSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_cursor: EventCursor,
        source_event_ids: Vec<EventId>,
        pending_approval_ids: Vec<ApprovalId>,
        reserved_budget_lease_ids: Vec<BudgetLeaseId>,
        active_resource_lease_ids: Vec<StorageLockId>,
        active_cell_ids: Vec<CellId>,
        fenced_cell_ids: Vec<CellId>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: RECOVERY_RESOURCE_SNAPSHOT_SCHEMA.to_owned(),
            version: RECOVERY_RESOURCE_SNAPSHOT_VERSION,
            source_cursor,
            source_event_ids: canonical_ids(source_event_ids),
            pending_approval_ids: canonical_ids(pending_approval_ids),
            reserved_budget_lease_ids: canonical_ids(reserved_budget_lease_ids),
            active_resource_lease_ids: canonical_ids(active_resource_lease_ids),
            active_cell_ids: canonical_ids(active_cell_ids),
            fenced_cell_ids: canonical_ids(fenced_cell_ids),
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let snapshot: Self = serde_json::from_value(value.clone())
            .map_err(|_| "recovery_resource_snapshot_decode_failed".to_owned())?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self)
            .map_err(|_| "recovery_resource_snapshot_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RECOVERY_RESOURCE_SNAPSHOT_SCHEMA
            || !self
                .version
                .is_compatible_with(&RECOVERY_RESOURCE_SNAPSHOT_VERSION)
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || !valid_ids(&self.source_event_ids)
            || !valid_ids(&self.pending_approval_ids)
            || !valid_ids(&self.reserved_budget_lease_ids)
            || !valid_ids(&self.active_resource_lease_ids)
            || !valid_ids(&self.active_cell_ids)
            || !valid_ids(&self.fenced_cell_ids)
            || !valid_digest(&self.snapshot_digest)
        {
            return Err("recovery_resource_snapshot_header_invalid".to_owned());
        }
        if self.snapshot_digest != self.digest() {
            return Err("recovery_resource_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "pending_approval_ids": self.pending_approval_ids,
            "reserved_budget_lease_ids": self.reserved_budget_lease_ids,
            "active_resource_lease_ids": self.active_resource_lease_ids,
            "active_cell_ids": self.active_cell_ids,
            "fenced_cell_ids": self.fenced_cell_ids,
        }))
    }
}

fn canonical_ids<T: Ord>(mut values: Vec<T>) -> Vec<T> {
    values.sort();
    values.dedup();
    values
}

trait RecoveryId {
    fn is_nil(&self) -> bool;
}

macro_rules! recovery_id {
    ($($ty:ty),+ $(,)?) => {
        $(impl RecoveryId for $ty {
            fn is_nil(&self) -> bool {
                self.as_uuid().is_nil()
            }
        })+
    };
}

recovery_id!(EventId, ApprovalId, BudgetLeaseId, StorageLockId, CellId);

fn valid_ids<T: Ord + RecoveryId>(values: &[T]) -> bool {
    values.len() <= MAX_RECOVERY_RESOURCE_ENTRIES
        && values.windows(2).all(|pair| pair[0] < pair[1])
        && values.iter().all(|value| !value.is_nil())
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
