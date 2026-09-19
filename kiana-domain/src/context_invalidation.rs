//! Resume, cache and source-revocation invalidation contracts for the context view.
//!
//! These are deterministic projections over the existing dependency graph, prepared-request
//! digest and ContextCheckpoint. They do not delete facts or make a cache authoritative.

use crate::{
    json_digest, ContextCheckpoint, ContextCheckpointState, DependencyNodeKind, RunId,
    SchemaVersion, SourceDependencyGraph, SourceInvalidation,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTEXT_INVALIDATION_SCHEMA: &str = "kiana.context-invalidation.v1";
pub const CONTEXT_CACHE_BINDING_SCHEMA: &str = "kiana.context-cache-binding.v1";
pub const CONTEXT_RESUME_VIEW_SCHEMA: &str = "kiana.context-resume-view.v1";
pub const CONTEXT_INVALIDATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn required_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextInvalidationTarget {
    Summary,
    Selection,
    Cache,
    Checkpoint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextInvalidationPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_id: String,
    pub previous_epoch: u64,
    pub data_epoch: u64,
    pub affected_node_ids: Vec<String>,
    pub targets: Vec<ContextInvalidationTarget>,
    pub source_invalidation_digest: String,
    pub plan_digest: String,
}

impl ContextInvalidationPlan {
    pub fn from_source_invalidation(
        graph: &SourceDependencyGraph,
        invalidation: &SourceInvalidation,
    ) -> Result<Self, String> {
        graph.validate()?;
        if invalidation.schema != crate::SOURCE_INVALIDATION_SCHEMA
            || invalidation.data_epoch != graph.data_epoch
            || invalidation.data_epoch <= invalidation.previous_epoch
            || invalidation.graph_digest != graph.graph_digest
            || invalidation.source_id.trim().is_empty()
        {
            return Err("context_invalidation_source_binding_invalid".to_owned());
        }
        let mut ids = invalidation.affected_node_ids.clone();
        ids.sort();
        ids.dedup();
        if ids.len() != invalidation.affected_node_ids.len()
            || ids.iter().any(|id| !graph.nodes.contains_key(id))
        {
            return Err("context_invalidation_nodes_invalid".to_owned());
        }
        let mut targets = BTreeSet::new();
        for id in &ids {
            match graph.nodes[id].kind {
                DependencyNodeKind::Summary => {
                    targets.insert(ContextInvalidationTarget::Summary);
                    targets.insert(ContextInvalidationTarget::Checkpoint);
                }
                DependencyNodeKind::ContextPlan | DependencyNodeKind::Index => {
                    targets.insert(ContextInvalidationTarget::Selection);
                }
                DependencyNodeKind::Plan => {
                    targets.insert(ContextInvalidationTarget::Cache);
                }
                DependencyNodeKind::Artifact => {
                    targets.insert(ContextInvalidationTarget::Checkpoint);
                }
                DependencyNodeKind::Memory
                | DependencyNodeKind::Evidence
                | DependencyNodeKind::Event
                | DependencyNodeKind::File => {}
            }
        }
        if !targets.is_empty() {
            targets.insert(ContextInvalidationTarget::Cache);
        }
        let source_invalidation_digest = json_digest(invalidation);
        let mut plan = Self {
            schema: CONTEXT_INVALIDATION_SCHEMA.to_owned(),
            version: CONTEXT_INVALIDATION_VERSION,
            source_id: invalidation.source_id.clone(),
            previous_epoch: invalidation.previous_epoch,
            data_epoch: invalidation.data_epoch,
            affected_node_ids: ids,
            targets: targets.into_iter().collect(),
            source_invalidation_digest,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_INVALIDATION_SCHEMA
            || !self
                .version
                .is_compatible_with(&CONTEXT_INVALIDATION_VERSION)
            || self.source_id.trim().is_empty()
            || self.previous_epoch == 0
            || self.data_epoch <= self.previous_epoch
            || self
                .affected_node_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.targets.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("context_invalidation_header_invalid".to_owned());
        }
        required_digest(
            &self.source_invalidation_digest,
            "context_invalidation_source_digest",
        )?;
        required_digest(&self.plan_digest, "context_invalidation_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("context_invalidation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn invalidates(&self, target: ContextInvalidationTarget) -> bool {
        self.targets.binary_search(&target).is_ok()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "source_id": self.source_id,
            "previous_epoch": self.previous_epoch,
            "data_epoch": self.data_epoch,
            "affected_node_ids": self.affected_node_ids,
            "targets": self.targets,
            "source_invalidation_digest": self.source_invalidation_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCacheBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub prepared_request_digest: String,
    pub cache_key_digest: String,
    pub data_epoch: u64,
    pub valid: bool,
    pub invalidated_by: Option<String>,
    pub binding_digest: String,
}

impl ContextCacheBinding {
    pub fn new(
        prepared_request_digest: impl Into<String>,
        cache_key_digest: impl Into<String>,
        data_epoch: u64,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: CONTEXT_CACHE_BINDING_SCHEMA.to_owned(),
            version: CONTEXT_INVALIDATION_VERSION,
            prepared_request_digest: prepared_request_digest.into(),
            cache_key_digest: cache_key_digest.into(),
            data_epoch,
            valid: true,
            invalidated_by: None,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn invalidate(&self, plan: &ContextInvalidationPlan) -> Result<Self, String> {
        self.validate()?;
        plan.validate()?;
        if !plan.invalidates(ContextInvalidationTarget::Cache) {
            return Err("context_cache_invalidation_target_missing".to_owned());
        }
        let mut invalidated = self.clone();
        invalidated.valid = false;
        invalidated.data_epoch = plan.data_epoch;
        invalidated.invalidated_by = Some(plan.plan_digest.clone());
        invalidated.binding_digest = invalidated.digest();
        invalidated.validate()?;
        Ok(invalidated)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_CACHE_BINDING_SCHEMA
            || !self
                .version
                .is_compatible_with(&CONTEXT_INVALIDATION_VERSION)
            || self.data_epoch == 0
            || self.valid && self.invalidated_by.is_some()
            || !self.valid && self.invalidated_by.is_none()
        {
            return Err("context_cache_binding_header_invalid".to_owned());
        }
        required_digest(
            &self.prepared_request_digest,
            "context_cache_prepared_request_digest",
        )?;
        required_digest(&self.cache_key_digest, "context_cache_key_digest")?;
        if let Some(value) = &self.invalidated_by {
            required_digest(value, "context_cache_invalidation_digest")?;
        }
        required_digest(&self.binding_digest, "context_cache_binding_digest")?;
        if self.binding_digest != self.digest() {
            return Err("context_cache_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "prepared_request_digest": self.prepared_request_digest,
            "cache_key_digest": self.cache_key_digest,
            "data_epoch": self.data_epoch,
            "valid": self.valid,
            "invalidated_by": self.invalidated_by,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextResumeView {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub checkpoint_digest: String,
    pub prepared_request_digest: String,
    pub context_revision: u64,
    pub source_cursor: u64,
    pub inbox_digest: String,
    pub data_epoch: u64,
    pub view_digest: String,
}

impl ContextResumeView {
    pub fn new(
        checkpoint: &ContextCheckpoint,
        prepared_request_digest: impl Into<String>,
        data_epoch: u64,
    ) -> Result<Self, String> {
        checkpoint.validate()?;
        if checkpoint.state != ContextCheckpointState::Committed || data_epoch == 0 {
            return Err("context_resume_checkpoint_unavailable".to_owned());
        }
        let mut view = Self {
            schema: CONTEXT_RESUME_VIEW_SCHEMA.to_owned(),
            version: CONTEXT_INVALIDATION_VERSION,
            run_id: checkpoint.run_id,
            checkpoint_digest: checkpoint.checkpoint_digest.clone(),
            prepared_request_digest: prepared_request_digest.into(),
            context_revision: checkpoint.active_context_revision,
            source_cursor: checkpoint.source_cursor,
            inbox_digest: checkpoint.inbox_digest.clone(),
            data_epoch,
            view_digest: String::new(),
        };
        view.view_digest = view.digest();
        view.validate_against(checkpoint, &view.prepared_request_digest, data_epoch)?;
        Ok(view)
    }

    pub fn rebuild_after_restart(
        checkpoint: &ContextCheckpoint,
        prepared_request_digest: impl Into<String>,
        data_epoch: u64,
    ) -> Result<Self, String> {
        Self::new(checkpoint, prepared_request_digest, data_epoch)
    }

    pub fn validate_against(
        &self,
        checkpoint: &ContextCheckpoint,
        prepared_request_digest: &str,
        data_epoch: u64,
    ) -> Result<(), String> {
        checkpoint.validate()?;
        if self.schema != CONTEXT_RESUME_VIEW_SCHEMA
            || self.run_id != checkpoint.run_id
            || checkpoint.state != ContextCheckpointState::Committed
            || self.checkpoint_digest != checkpoint.checkpoint_digest
            || self.prepared_request_digest != prepared_request_digest
            || self.context_revision != checkpoint.active_context_revision
            || self.source_cursor != checkpoint.source_cursor
            || self.inbox_digest != checkpoint.inbox_digest
            || self.data_epoch != data_epoch
        {
            return Err("context_resume_binding_changed".to_owned());
        }
        required_digest(
            &self.prepared_request_digest,
            "context_resume_prepared_request_digest",
        )?;
        required_digest(&self.inbox_digest, "context_resume_inbox_digest")?;
        required_digest(&self.view_digest, "context_resume_view_digest")?;
        if self.view_digest != self.digest() {
            return Err("context_resume_view_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "checkpoint_digest": self.checkpoint_digest,
            "prepared_request_digest": self.prepared_request_digest,
            "context_revision": self.context_revision,
            "source_cursor": self.source_cursor,
            "inbox_digest": self.inbox_digest,
            "data_epoch": self.data_epoch,
        }))
    }
}
