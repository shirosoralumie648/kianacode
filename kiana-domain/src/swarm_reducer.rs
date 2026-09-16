//! Deterministic Swarm/Partition/Attempt status reducer and typed transition facts.
//!
//! The reducer is pure: it can be used by live admission and replay, but it never claims a
//! broker effect. EventLog/CAS remains the only durable authority and legacy SwarmEvent remains
//! readable while typed transitions are introduced.
use crate::{
    json_digest, AttemptId, ChildCellId, EventId, PartitionId, PartitionStatus, RequestId,
    SwarmPlanId, SwarmStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const SWARM_TRANSITION_EVENT_SCHEMA: &str = "kiana.swarm-transition-event.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "entity", deny_unknown_fields)]
pub enum SwarmTransitionEntity {
    Swarm {
        from: SwarmStatus,
        to: SwarmStatus,
    },
    Partition {
        partition_id: PartitionId,
        from: PartitionStatus,
        to: PartitionStatus,
    },
    Attempt {
        partition_id: PartitionId,
        attempt_id: AttemptId,
        child_cell_id: ChildCellId,
        from: AttemptStatus,
        to: AttemptStatus,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    Proposed,
    Dispatched,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmTransitionEvent {
    pub schema: String,
    pub event_kind: String,
    pub swarm_plan_id: SwarmPlanId,
    pub entity: SwarmTransitionEntity,
    pub revision: u64,
    pub authority_epoch: u64,
    pub correlation_id: RequestId,
    #[serde(default)]
    pub causation_event_id: Option<EventId>,
    #[serde(default)]
    pub review_complete: bool,
    pub event_digest: String,
}

impl SwarmTransitionEvent {
    pub fn new(
        swarm_plan_id: SwarmPlanId,
        entity: SwarmTransitionEntity,
        revision: u64,
        authority_epoch: u64,
        correlation_id: RequestId,
        causation_event_id: Option<EventId>,
        review_complete: bool,
    ) -> Result<Self, String> {
        let mut event = Self {
            schema: SWARM_TRANSITION_EVENT_SCHEMA.to_owned(),
            event_kind: event_kind(&entity).to_owned(),
            swarm_plan_id,
            entity,
            revision,
            authority_epoch,
            correlation_id,
            causation_event_id,
            review_complete,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SWARM_TRANSITION_EVENT_SCHEMA
            || self.swarm_plan_id.as_uuid().is_nil()
            || self.revision == 0
            || self.authority_epoch == 0
            || self.correlation_id.as_uuid().is_nil()
        {
            return Err("swarm_transition_header_invalid".to_owned());
        }
        if self
            .causation_event_id
            .is_some_and(|value| value.as_uuid().is_nil())
        {
            return Err("swarm_transition_causation_invalid".to_owned());
        }
        if self.event_kind != event_kind(&self.entity) {
            return Err("swarm_transition_kind_invalid".to_owned());
        }
        match &self.entity {
            SwarmTransitionEntity::Swarm { from, to } => {
                if !swarm_transition_allowed(*from, *to, self.review_complete) {
                    return Err("swarm_transition_illegal".to_owned());
                }
            }
            SwarmTransitionEntity::Partition {
                partition_id,
                from,
                to,
            } => {
                if partition_id.as_uuid().is_nil() || !partition_transition_allowed(*from, *to) {
                    return Err("swarm_partition_transition_illegal".to_owned());
                }
            }
            SwarmTransitionEntity::Attempt {
                partition_id,
                attempt_id,
                child_cell_id,
                from,
                to,
            } => {
                if partition_id.as_uuid().is_nil()
                    || attempt_id.as_uuid().is_nil()
                    || child_cell_id.as_uuid().is_nil()
                    || !attempt_transition_allowed(*from, *to)
                {
                    return Err("swarm_attempt_transition_illegal".to_owned());
                }
            }
        }
        let digest = self.event_digest.strip_prefix("sha256:");
        if digest
            .is_none_or(|value| value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("swarm_transition_digest_invalid".to_owned());
        }
        if self.event_digest != self.digest() {
            return Err("swarm_transition_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "event_kind": self.event_kind,
            "swarm_plan_id": self.swarm_plan_id,
            "entity": self.entity,
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
            "correlation_id": self.correlation_id,
            "causation_event_id": self.causation_event_id,
            "review_complete": self.review_complete,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmTransitionReducer {
    pub schema: String,
    pub swarm_plan_id: SwarmPlanId,
    pub swarm_status: SwarmStatus,
    pub partition_statuses: BTreeMap<String, PartitionStatus>,
    pub attempt_statuses: BTreeMap<String, AttemptStatus>,
    pub revision: u64,
    pub authority_epoch: u64,
}

impl SwarmTransitionReducer {
    pub fn new(swarm_plan_id: SwarmPlanId, authority_epoch: u64) -> Result<Self, String> {
        if swarm_plan_id.as_uuid().is_nil() || authority_epoch == 0 {
            return Err("swarm_reducer_header_invalid".to_owned());
        }
        Ok(Self {
            schema: SWARM_TRANSITION_EVENT_SCHEMA.to_owned(),
            swarm_plan_id,
            swarm_status: SwarmStatus::Reserved,
            partition_statuses: BTreeMap::new(),
            attempt_statuses: BTreeMap::new(),
            revision: 0,
            authority_epoch,
        })
    }

    pub fn apply(&mut self, event: &SwarmTransitionEvent) -> Result<(), String> {
        event.validate()?;
        if event.swarm_plan_id != self.swarm_plan_id {
            return Err("swarm_transition_swarm_mismatch".to_owned());
        }
        if event.revision != self.revision.saturating_add(1) {
            return Err("swarm_transition_revision_gap_or_regression".to_owned());
        }
        if event.authority_epoch < self.authority_epoch {
            return Err("swarm_transition_epoch_regression".to_owned());
        }
        match &event.entity {
            SwarmTransitionEntity::Swarm { from, to } => {
                if *from != self.swarm_status
                    || !swarm_transition_allowed(*from, *to, event.review_complete)
                {
                    return Err("swarm_transition_illegal".to_owned());
                }
                self.swarm_status = *to;
            }
            SwarmTransitionEntity::Partition {
                partition_id,
                from,
                to,
            } => {
                let key = partition_id.to_string();
                let current = self
                    .partition_statuses
                    .get(&key)
                    .copied()
                    .unwrap_or(PartitionStatus::Pending);
                if current != *from || !partition_transition_allowed(*from, *to) {
                    return Err("swarm_partition_transition_illegal".to_owned());
                }
                self.partition_statuses.insert(key, *to);
            }
            SwarmTransitionEntity::Attempt {
                attempt_id,
                from,
                to,
                ..
            } => {
                let key = attempt_id.to_string();
                let current = self
                    .attempt_statuses
                    .get(&key)
                    .copied()
                    .unwrap_or(AttemptStatus::Proposed);
                if current != *from || !attempt_transition_allowed(*from, *to) {
                    return Err("swarm_attempt_transition_illegal".to_owned());
                }
                self.attempt_statuses.insert(key, *to);
            }
        }
        self.revision = event.revision;
        self.authority_epoch = event.authority_epoch;
        Ok(())
    }

    pub fn replay(
        swarm_plan_id: SwarmPlanId,
        authority_epoch: u64,
        events: &[SwarmTransitionEvent],
    ) -> Result<Self, String> {
        let mut reducer = Self::new(swarm_plan_id, authority_epoch)?;
        for event in events {
            reducer.apply(event)?;
        }
        Ok(reducer)
    }
}

fn event_kind(entity: &SwarmTransitionEntity) -> &'static str {
    match entity {
        SwarmTransitionEntity::Swarm { .. } => "delegation.swarm_state",
        SwarmTransitionEntity::Partition { .. } => "delegation.partition_state",
        SwarmTransitionEntity::Attempt { .. } => "delegation.attempt_state",
    }
}

fn swarm_transition_allowed(from: SwarmStatus, to: SwarmStatus, review_complete: bool) -> bool {
    if from == to || swarm_is_terminal(from) {
        return false;
    }
    match (from, to) {
        (SwarmStatus::Reserved, SwarmStatus::Ready)
        | (
            SwarmStatus::Ready,
            SwarmStatus::Running | SwarmStatus::Failed | SwarmStatus::CancelRequested,
        )
        | (
            SwarmStatus::Running,
            SwarmStatus::ReadyToMerge
            | SwarmStatus::Failed
            | SwarmStatus::CancelRequested
            | SwarmStatus::ResultUnknown,
        )
        | (SwarmStatus::CancelRequested, SwarmStatus::Cancelled | SwarmStatus::ResultUnknown)
        | (
            SwarmStatus::ReadyToMerge,
            SwarmStatus::Completed | SwarmStatus::Failed | SwarmStatus::ResultUnknown,
        ) => to != SwarmStatus::Completed || review_complete,
        _ => false,
    }
}

fn partition_transition_allowed(from: PartitionStatus, to: PartitionStatus) -> bool {
    if from == to
        || matches!(
            from,
            PartitionStatus::Succeeded
                | PartitionStatus::Failed
                | PartitionStatus::Cancelled
                | PartitionStatus::ResultUnknown
        )
    {
        return false;
    }
    matches!(
        (from, to),
        (
            PartitionStatus::Pending,
            PartitionStatus::Ready
                | PartitionStatus::Running
                | PartitionStatus::Failed
                | PartitionStatus::Cancelled
        ) | (
            PartitionStatus::Ready,
            PartitionStatus::Running | PartitionStatus::Failed | PartitionStatus::Cancelled
        ) | (
            PartitionStatus::Running,
            PartitionStatus::Succeeded
                | PartitionStatus::Failed
                | PartitionStatus::Cancelled
                | PartitionStatus::ResultUnknown
        )
    )
}

fn attempt_transition_allowed(from: AttemptStatus, to: AttemptStatus) -> bool {
    if from == to
        || matches!(
            from,
            AttemptStatus::Succeeded
                | AttemptStatus::Failed
                | AttemptStatus::Cancelled
                | AttemptStatus::ResultUnknown
        )
    {
        return false;
    }
    matches!(
        (from, to),
        (
            AttemptStatus::Proposed,
            AttemptStatus::Dispatched | AttemptStatus::Cancelled | AttemptStatus::Failed
        ) | (
            AttemptStatus::Dispatched,
            AttemptStatus::Running
                | AttemptStatus::Cancelled
                | AttemptStatus::Failed
                | AttemptStatus::ResultUnknown
        ) | (
            AttemptStatus::Running,
            AttemptStatus::Succeeded
                | AttemptStatus::Failed
                | AttemptStatus::Cancelled
                | AttemptStatus::ResultUnknown
        )
    )
}

fn swarm_is_terminal(status: SwarmStatus) -> bool {
    matches!(
        status,
        SwarmStatus::Completed
            | SwarmStatus::Failed
            | SwarmStatus::Cancelled
            | SwarmStatus::ResultUnknown
    )
}
