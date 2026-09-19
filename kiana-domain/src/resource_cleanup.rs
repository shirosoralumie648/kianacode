//! Explicit run-resource cleanup and Unknown isolation contracts.
//!
//! This is a reducer over observed cleanup evidence.  It never kills a process or releases an
//! OS lock itself; adapters report stop/effect evidence, and only confirmed resources become
//! releasable.  Missing evidence is retained as Unknown for reconciliation.

use crate::{json_digest, RunId, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const RESOURCE_CLEANUP_PLAN_SCHEMA: &str = "kiana.resource-cleanup-plan.v1";
pub const RESOURCE_CLEANUP_REPORT_SCHEMA: &str = "kiana.resource-cleanup-report.v1";
pub const RESOURCE_RETENTION_SCHEMA: &str = "kiana.resource-retention.v1";
pub const RESOURCE_CLEANUP_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_CLEANUP_RESOURCES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupCause {
    Normal,
    Cancel,
    Panic,
    LogFailure,
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupResourceKind {
    ModelTask,
    ToolFuture,
    McpConnection,
    TemporaryArtifact,
    JobHandle,
    PathLock,
    BudgetLease,
    Subscription,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupResourceStatus {
    Pending,
    Released,
    IsolatedUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupResource {
    pub resource_id: String,
    pub owner_run_id: RunId,
    pub kind: CleanupResourceKind,
    pub requires_stop_confirmation: bool,
    pub stop_confirmed: bool,
    pub effect_known: bool,
    pub status: CleanupResourceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl CleanupResource {
    pub fn validate(&self) -> Result<(), String> {
        if !bounded(&self.resource_id, 256)
            || self.owner_run_id.as_uuid().is_nil()
            || (self.status == CleanupResourceStatus::Released
                && (self.requires_stop_confirmation && !self.stop_confirmed || !self.effect_known))
            || (self.status == CleanupResourceStatus::IsolatedUnknown
                && self.stop_confirmed
                && self.effect_known)
            || self
                .reason
                .as_deref()
                .is_some_and(|reason| !bounded(reason, 1_024))
        {
            return Err("cleanup_resource_invalid".to_owned());
        }
        Ok(())
    }

    pub fn observe(
        mut self,
        stop_confirmed: bool,
        effect_known: bool,
        reason: impl Into<String>,
    ) -> Self {
        self.stop_confirmed = stop_confirmed;
        self.effect_known = effect_known;
        self.reason = Some(reason.into());
        self.status = if (!self.requires_stop_confirmation || stop_confirmed) && effect_known {
            CleanupResourceStatus::Released
        } else {
            CleanupResourceStatus::IsolatedUnknown
        };
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceCleanupPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub cause: CleanupCause,
    pub resources: Vec<CleanupResource>,
    pub plan_digest: String,
}

impl ResourceCleanupPlan {
    pub fn new(
        run_id: RunId,
        cause: CleanupCause,
        resources: Vec<CleanupResource>,
    ) -> Result<Self, String> {
        let mut plan = Self {
            schema: RESOURCE_CLEANUP_PLAN_SCHEMA.to_owned(),
            version: RESOURCE_CLEANUP_VERSION,
            run_id,
            cause,
            resources,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESOURCE_CLEANUP_PLAN_SCHEMA
            || self.version != RESOURCE_CLEANUP_VERSION
            || self.run_id.as_uuid().is_nil()
            || self.resources.len() > MAX_CLEANUP_RESOURCES
            || !digest(&self.plan_digest)
            || self.plan_digest != self.digest()
        {
            return Err("cleanup_plan_invalid".to_owned());
        }
        let mut ids = std::collections::HashSet::new();
        for resource in &self.resources {
            resource.validate()?;
            if resource.owner_run_id != self.run_id || !ids.insert(resource.resource_id.clone()) {
                return Err("cleanup_resource_identity_invalid".to_owned());
            }
        }
        Ok(())
    }

    pub fn observe_all(
        &self,
        observations: &[(String, bool, bool, String)],
    ) -> Result<ResourceCleanupReport, String> {
        self.validate()?;
        let mut resources = self.resources.clone();
        for (resource_id, stop_confirmed, effect_known, reason) in observations {
            let Some(resource) = resources
                .iter_mut()
                .find(|resource| &resource.resource_id == resource_id)
            else {
                return Err("cleanup_observation_resource_unknown".to_owned());
            };
            *resource = resource
                .clone()
                .observe(*stop_confirmed, *effect_known, reason.clone());
        }
        let report = ResourceCleanupReport::from_plan(self, resources)?;
        report.validate()?;
        Ok(report)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "cause": self.cause,
            "resources": self.resources,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceCleanupReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub cause: CleanupCause,
    pub resources: Vec<CleanupResource>,
    pub released_count: u32,
    pub unknown_count: u32,
    pub pending_count: u32,
    pub report_digest: String,
}

impl ResourceCleanupReport {
    fn from_plan(
        plan: &ResourceCleanupPlan,
        resources: Vec<CleanupResource>,
    ) -> Result<Self, String> {
        let released_count = resources
            .iter()
            .filter(|resource| resource.status == CleanupResourceStatus::Released)
            .count() as u32;
        let unknown_count = resources
            .iter()
            .filter(|resource| resource.status == CleanupResourceStatus::IsolatedUnknown)
            .count() as u32;
        let pending_count = resources
            .iter()
            .filter(|resource| resource.status == CleanupResourceStatus::Pending)
            .count() as u32;
        let mut report = Self {
            schema: RESOURCE_CLEANUP_REPORT_SCHEMA.to_owned(),
            version: RESOURCE_CLEANUP_VERSION,
            run_id: plan.run_id,
            cause: plan.cause,
            resources,
            released_count,
            unknown_count,
            pending_count,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESOURCE_CLEANUP_REPORT_SCHEMA
            || self.version != RESOURCE_CLEANUP_VERSION
            || self.run_id.as_uuid().is_nil()
            || self.resources.len() > MAX_CLEANUP_RESOURCES
            || self.released_count + self.unknown_count + self.pending_count
                != self.resources.len() as u32
            || !digest(&self.report_digest)
            || self.report_digest != self.digest()
        {
            return Err("cleanup_report_invalid".to_owned());
        }
        for resource in &self.resources {
            resource.validate()?;
        }
        Ok(())
    }

    pub fn safe_to_release_all(&self) -> bool {
        self.unknown_count == 0 && self.pending_count == 0
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "cause": self.cause,
            "resources": self.resources,
            "released_count": self.released_count,
            "unknown_count": self.unknown_count,
            "pending_count": self.pending_count,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRetention {
    pub schema: String,
    pub version: SchemaVersion,
    pub max_retained_runs: usize,
    pub max_retained_frames: usize,
    pub max_history_bytes: usize,
    pub retention_digest: String,
}

impl ResourceRetention {
    pub fn new(
        max_retained_runs: usize,
        max_retained_frames: usize,
        max_history_bytes: usize,
    ) -> Result<Self, String> {
        let mut retention = Self {
            schema: RESOURCE_RETENTION_SCHEMA.to_owned(),
            version: RESOURCE_CLEANUP_VERSION,
            max_retained_runs,
            max_retained_frames,
            max_history_bytes,
            retention_digest: String::new(),
        };
        retention.retention_digest = retention.digest();
        retention.validate()?;
        Ok(retention)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESOURCE_RETENTION_SCHEMA
            || self.version != RESOURCE_CLEANUP_VERSION
            || self.max_retained_runs == 0
            || self.max_retained_frames == 0
            || self.max_history_bytes == 0
            || !digest(&self.retention_digest)
            || self.retention_digest != self.digest()
        {
            return Err("resource_retention_invalid".to_owned());
        }
        Ok(())
    }

    pub fn within(&self, runs: usize, frames: usize, history_bytes: usize) -> Result<(), String> {
        self.validate()?;
        if runs > self.max_retained_runs
            || frames > self.max_retained_frames
            || history_bytes > self.max_history_bytes
        {
            return Err("resource_retention_exceeded".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "max_retained_runs": self.max_retained_runs,
            "max_retained_frames": self.max_retained_frames,
            "max_history_bytes": self.max_history_bytes,
        }))
    }
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
