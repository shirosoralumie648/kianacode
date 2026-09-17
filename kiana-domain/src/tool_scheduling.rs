//! Deterministic planning metadata for bounded tool groups.
//!
//! The planner never authorizes or executes a capability. It only partitions one assistant batch
//! into parallel-read groups and exclusive barriers. Core still admits every request separately;
//! this prevents a group-level allow from widening the authority of a sibling call.

use crate::{current_tool_catalog, ModelToolCall, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const TOOL_BATCH_PLAN_SCHEMA: &str = "kiana.tool-batch-plan.v1";
pub const TOOL_BATCH_PLAN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const TOOL_BATCH_MAX_PARALLELISM: u32 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSchedulingMode {
    ParallelRead,
    Exclusive,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchGroup {
    pub group_id: u32,
    pub mode: ToolSchedulingMode,
    pub resources: Vec<String>,
    pub max_parallelism: u32,
    pub call_ids: Vec<String>,
}

impl ToolBatchGroup {
    pub fn validate(&self) -> Result<(), String> {
        if self.group_id == 0
            || self.resources.is_empty()
            || self
                .resources
                .iter()
                .any(|resource| resource.trim().is_empty())
            || self.call_ids.is_empty()
            || self
                .call_ids
                .iter()
                .any(|call_id| call_id.trim().is_empty())
            || self.max_parallelism == 0
            || self.max_parallelism > TOOL_BATCH_MAX_PARALLELISM
            || self.call_ids.len() as u32 > self.max_parallelism
                && self.mode == ToolSchedulingMode::ParallelRead
            || self.call_ids.len() != 1 && self.mode == ToolSchedulingMode::Exclusive
        {
            return Err("tool_batch_group_invalid".to_owned());
        }
        let mut resources = BTreeSet::new();
        if self
            .resources
            .iter()
            .any(|resource| !resources.insert(resource.clone()))
        {
            return Err("tool_batch_group_resources_duplicate".to_owned());
        }
        let mut calls = BTreeSet::new();
        if self
            .call_ids
            .iter()
            .any(|call_id| !calls.insert(call_id.clone()))
        {
            return Err("tool_batch_group_calls_duplicate".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBatchPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub groups: Vec<ToolBatchGroup>,
}

impl ToolBatchPlan {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_BATCH_PLAN_SCHEMA
            || self.version != TOOL_BATCH_PLAN_VERSION
            || self.groups.is_empty()
        {
            return Err("tool_batch_plan_invalid".to_owned());
        }
        let mut expected = 1u32;
        let mut calls = BTreeSet::new();
        for group in &self.groups {
            if group.group_id != expected {
                return Err("tool_batch_plan_group_order_invalid".to_owned());
            }
            group.validate()?;
            if group
                .call_ids
                .iter()
                .any(|call_id| !calls.insert(call_id.clone()))
            {
                return Err("tool_batch_plan_call_duplicate".to_owned());
            }
            expected = expected.saturating_add(1);
        }
        Ok(())
    }

    pub fn parallel_groups(&self) -> impl Iterator<Item = &ToolBatchGroup> {
        self.groups
            .iter()
            .filter(|group| group.mode == ToolSchedulingMode::ParallelRead)
    }
}

/// Partition calls in source order. Parallel-read calls are grouped only when they are adjacent;
/// an exclusive call always drains the preceding group and forms a barrier of its own.
pub fn plan_tool_batch(calls: &[ModelToolCall]) -> Result<ToolBatchPlan, String> {
    if calls.is_empty() {
        return Err("tool_batch_empty".to_owned());
    }
    let catalog = current_tool_catalog();
    catalog.validate()?;
    let mut groups = Vec::new();
    for call in calls {
        if call.id.trim().is_empty() {
            return Err("tool_batch_call_id_required".to_owned());
        }
        let descriptor = catalog
            .descriptor(&call.name)
            .ok_or_else(|| format!("tool_unsupported:{}", call.name))?;
        let mode = if descriptor.scheduling == "parallel_read" {
            ToolSchedulingMode::ParallelRead
        } else {
            ToolSchedulingMode::Exclusive
        };
        let max_parallelism = descriptor.max_parallelism.min(TOOL_BATCH_MAX_PARALLELISM);
        let can_append = mode == ToolSchedulingMode::ParallelRead
            && groups.last().is_some_and(|group: &ToolBatchGroup| {
                group.mode == ToolSchedulingMode::ParallelRead
                    && group.call_ids.len() < group.max_parallelism as usize
                    && group.resources == descriptor.resources
            });
        if can_append {
            groups
                .last_mut()
                .expect("parallel group was checked above")
                .call_ids
                .push(call.id.clone());
        } else {
            let group_id = groups.len() as u32 + 1;
            groups.push(ToolBatchGroup {
                group_id,
                mode,
                resources: descriptor.resources.clone(),
                max_parallelism,
                call_ids: vec![call.id.clone()],
            });
        }
    }
    let plan = ToolBatchPlan {
        schema: TOOL_BATCH_PLAN_SCHEMA.to_owned(),
        version: TOOL_BATCH_PLAN_VERSION,
        groups,
    };
    plan.validate()?;
    Ok(plan)
}
