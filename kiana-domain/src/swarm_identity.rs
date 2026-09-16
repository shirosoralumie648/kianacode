//! Typed Swarm identity and lineage contracts.
//!
//! Legacy SwarmPlan/SwarmChild fields remain readable for migration, but new lineage facts use
//! these opaque IDs so a partition, attempt or dispatch intent cannot be confused across swarms.
use crate::{
    json_digest, AttemptId, ChildCellId, DispatchIntentId, EventId, MergeDecisionId, PartitionId,
    QueueEntryId, RequestId, RunId, SwarmPlanId,
};
use serde::{Deserialize, Serialize};

pub const SWARM_LINEAGE_SCHEMA: &str = "kiana.swarm-lineage.v1";

fn digest(value: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err("swarm_lineage_digest_invalid".to_owned());
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("swarm_lineage_digest_invalid".to_owned());
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmLineage {
    pub schema: String,
    pub swarm_plan_id: SwarmPlanId,
    pub partition_id: PartitionId,
    pub child_cell_id: ChildCellId,
    pub attempt_id: AttemptId,
    pub dispatch_intent_id: DispatchIntentId,
    pub queue_entry_id: QueueEntryId,
    pub merge_decision_id: MergeDecisionId,
    #[serde(default)]
    pub workflow_instance_id: Option<String>,
    #[serde(default)]
    pub parent_swarm_plan_id: Option<SwarmPlanId>,
    #[serde(default)]
    pub root_run_id: Option<RunId>,
    pub correlation_id: RequestId,
    #[serde(default)]
    pub causation_event_id: Option<EventId>,
    pub authority_epoch: u64,
    pub revision: u64,
    pub lineage_digest: String,
}

impl SwarmLineage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        swarm_plan_id: SwarmPlanId,
        partition_id: PartitionId,
        child_cell_id: ChildCellId,
        attempt_id: AttemptId,
        dispatch_intent_id: DispatchIntentId,
        queue_entry_id: QueueEntryId,
        merge_decision_id: MergeDecisionId,
        workflow_instance_id: Option<String>,
        parent_swarm_plan_id: Option<SwarmPlanId>,
        root_run_id: Option<RunId>,
        correlation_id: RequestId,
        causation_event_id: Option<EventId>,
        authority_epoch: u64,
        revision: u64,
    ) -> Result<Self, String> {
        let mut lineage = Self {
            schema: SWARM_LINEAGE_SCHEMA.to_owned(),
            swarm_plan_id,
            partition_id,
            child_cell_id,
            attempt_id,
            dispatch_intent_id,
            queue_entry_id,
            merge_decision_id,
            workflow_instance_id,
            parent_swarm_plan_id,
            root_run_id,
            correlation_id,
            causation_event_id,
            authority_epoch,
            revision,
            lineage_digest: String::new(),
        };
        lineage.lineage_digest = lineage.digest();
        lineage.validate()?;
        Ok(lineage)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SWARM_LINEAGE_SCHEMA || self.authority_epoch == 0 || self.revision == 0 {
            return Err("swarm_lineage_header_invalid".to_owned());
        }
        if [
            self.swarm_plan_id.as_uuid(),
            self.partition_id.as_uuid(),
            self.child_cell_id.as_uuid(),
            self.attempt_id.as_uuid(),
            self.dispatch_intent_id.as_uuid(),
            self.queue_entry_id.as_uuid(),
            self.merge_decision_id.as_uuid(),
            self.correlation_id.as_uuid(),
        ]
        .iter()
        .any(|value| value.is_nil())
            || self
                .root_run_id
                .is_some_and(|value| value.as_uuid().is_nil())
            || self
                .causation_event_id
                .is_some_and(|value| value.as_uuid().is_nil())
            || self
                .parent_swarm_plan_id
                .is_some_and(|value| value.as_uuid().is_nil())
        {
            return Err("swarm_lineage_id_invalid".to_owned());
        }
        if self
            .workflow_instance_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
        {
            return Err("swarm_workflow_instance_invalid".to_owned());
        }
        if let Some(parent) = self.parent_swarm_plan_id {
            if parent == self.swarm_plan_id {
                return Err("swarm_lineage_self_parent".to_owned());
            }
        }
        digest(&self.lineage_digest)?;
        if self.lineage_digest != self.digest() {
            return Err("swarm_lineage_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Validate a new fact against the previously committed fact for this lineage stream.
    /// Equality is rejected as a duplicate revision; authority epochs may advance but may never
    /// move backwards. The method is pure so EventLog/CAS adapters can invoke it before append.
    pub fn validate_against(&self, previous: &Self) -> Result<(), String> {
        previous.validate()?;
        self.validate()?;
        if self.swarm_plan_id != previous.swarm_plan_id {
            return Err("swarm_lineage_swarm_mismatch".to_owned());
        }
        if self.revision <= previous.revision {
            return Err("swarm_lineage_revision_regression".to_owned());
        }
        if self.authority_epoch < previous.authority_epoch {
            return Err("swarm_lineage_epoch_regression".to_owned());
        }
        Ok(())
    }

    pub fn validate_for_swarm(&self, expected: SwarmPlanId) -> Result<(), String> {
        self.validate()?;
        if self.swarm_plan_id != expected {
            return Err("swarm_lineage_swarm_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "swarm_plan_id": self.swarm_plan_id,
            "partition_id": self.partition_id,
            "child_cell_id": self.child_cell_id,
            "attempt_id": self.attempt_id,
            "dispatch_intent_id": self.dispatch_intent_id,
            "queue_entry_id": self.queue_entry_id,
            "merge_decision_id": self.merge_decision_id,
            "workflow_instance_id": self.workflow_instance_id,
            "parent_swarm_plan_id": self.parent_swarm_plan_id,
            "root_run_id": self.root_run_id,
            "correlation_id": self.correlation_id,
            "causation_event_id": self.causation_event_id,
            "authority_epoch": self.authority_epoch,
            "revision": self.revision,
        }))
    }
}
