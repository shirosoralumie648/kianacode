//! Orchestrated canary/blue-green/rainbow rollout design contract.
//!
//! This module is a pure decision boundary. It binds every worker route to an immutable
//! ExecutionRevisionPin, requires one active writer and a progress deadline, and exposes the
//! decisions a future orchestrator adapter may simulate. It does not talk to Kubernetes, change
//! traffic, fence a process or promote a release.

use crate::{json_digest, ExecutionRevisionPin, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const ORCHESTRATED_ROLLOUT_SCHEMA: &str = "kiana.orchestrated-rollout.v1";
pub const ORCHESTRATED_ROLLOUT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_ORCHESTRATED_ROUTES: usize = 8;
pub const MAX_ROLLOUT_REASON: usize = 512;
pub const FULL_TRAFFIC_BPS: u16 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestratedRolloutProfile {
    Canary,
    BlueGreen,
    Rainbow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestratedBackend {
    Simulation,
    KubernetesTarget,
    GenericOrchestratorTarget,
}

impl OrchestratedBackend {
    pub const fn is_target(self) -> bool {
        matches!(
            self,
            Self::KubernetesTarget | Self::GenericOrchestratorTarget
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestratedRolloutAction {
    Pause,
    Resume,
    Promote,
    Rollback,
}

/// One worker build and its traffic/write role. The revision pin is the routing authority input;
/// a build ID or worker label without the rest of the pin cannot enter a route table.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerBuildRoute {
    pub pin: ExecutionRevisionPin,
    pub traffic_weight_bps: u16,
    pub accepts_writes: bool,
}

impl WorkerBuildRoute {
    pub fn validate(&self) -> Result<(), String> {
        self.pin.validate()?;
        if self.traffic_weight_bps > FULL_TRAFFIC_BPS {
            return Err("rollout_route_weight_invalid".to_owned());
        }
        Ok(())
    }
}

/// Routing evidence for one rollout observation. Exactly one route may accept writes, even while
/// multiple pinned revisions serve read traffic during a canary or blue/green transition.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerBuildRoutingTable {
    pub routes: Vec<WorkerBuildRoute>,
    pub active_writer_revision_id: String,
    pub writer_fence_digest: String,
    pub routing_digest: String,
}

impl WorkerBuildRoutingTable {
    pub fn new(
        routes: Vec<WorkerBuildRoute>,
        active_writer_revision_id: impl Into<String>,
        writer_fence_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut table = Self {
            routes,
            active_writer_revision_id: active_writer_revision_id.into(),
            writer_fence_digest: writer_fence_digest.into(),
            routing_digest: String::new(),
        };
        table.routing_digest = table.digest();
        table.validate()?;
        Ok(table)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.routes.is_empty() || self.routes.len() > MAX_ORCHESTRATED_ROUTES {
            return Err("rollout_route_count_invalid".to_owned());
        }
        bounded_text(
            &self.active_writer_revision_id,
            "rollout_active_writer_revision",
        )?;
        validate_digest(&self.writer_fence_digest, "rollout_writer_fence_digest")?;
        validate_digest(&self.routing_digest, "rollout_routing_digest")?;

        let mut revisions = BTreeSet::new();
        let mut builds = BTreeSet::new();
        let mut total_weight = 0u32;
        let mut writer_revision = None;
        for route in &self.routes {
            route.validate()?;
            if !revisions.insert(route.pin.revision_id.as_str()) {
                return Err("rollout_route_revision_duplicate".to_owned());
            }
            if !builds.insert(route.pin.build_digest.as_str()) {
                return Err("rollout_route_build_duplicate".to_owned());
            }
            total_weight = total_weight
                .checked_add(u32::from(route.traffic_weight_bps))
                .ok_or_else(|| "rollout_route_weight_overflow".to_owned())?;
            if route.accepts_writes {
                if writer_revision.is_some() {
                    return Err("rollout_multiple_active_writers".to_owned());
                }
                writer_revision = Some(route.pin.revision_id.as_str());
            }
        }
        if total_weight != u32::from(FULL_TRAFFIC_BPS) {
            return Err("rollout_route_weight_not_full".to_owned());
        }
        if writer_revision != Some(self.active_writer_revision_id.as_str()) {
            return Err("rollout_writer_fence_route_mismatch".to_owned());
        }
        if self.routing_digest != self.digest() {
            return Err("rollout_routing_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn route(&self, revision_id: &str) -> Option<&WorkerBuildRoute> {
        self.routes
            .iter()
            .find(|route| route.pin.revision_id == revision_id)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "routes": self.routes,
            "active_writer_revision_id": self.active_writer_revision_id,
            "writer_fence_digest": self.writer_fence_digest,
        }))
    }
}

/// Bounded canary evidence. The thresholds and measurements are explicit facts; this type never
/// samples traffic itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanaryObservation {
    pub sample_count: u64,
    pub error_rate_bps: u16,
    pub availability_bps: u16,
    pub max_error_rate_bps: u16,
    pub min_availability_bps: u16,
    pub window_ms: u64,
    pub measured_at_unix_ms: u64,
    pub passed: bool,
    pub observation_digest: String,
}

impl CanaryObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sample_count: u64,
        error_rate_bps: u16,
        availability_bps: u16,
        max_error_rate_bps: u16,
        min_availability_bps: u16,
        window_ms: u64,
        measured_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut observation = Self {
            sample_count,
            error_rate_bps,
            availability_bps,
            max_error_rate_bps,
            min_availability_bps,
            window_ms,
            measured_at_unix_ms,
            passed: sample_count > 0
                && error_rate_bps <= max_error_rate_bps
                && availability_bps >= min_availability_bps,
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.sample_count == 0
            || self.error_rate_bps > FULL_TRAFFIC_BPS
            || self.availability_bps > FULL_TRAFFIC_BPS
            || self.max_error_rate_bps > FULL_TRAFFIC_BPS
            || self.min_availability_bps > FULL_TRAFFIC_BPS
            || self.window_ms == 0
            || self.measured_at_unix_ms == 0
        {
            return Err("rollout_canary_observation_invalid".to_owned());
        }
        let expected = self.error_rate_bps <= self.max_error_rate_bps
            && self.availability_bps >= self.min_availability_bps;
        if self.passed != expected {
            return Err("rollout_canary_pass_mismatch".to_owned());
        }
        validate_digest(
            &self.observation_digest,
            "rollout_canary_observation_digest",
        )?;
        if self.observation_digest != self.digest() {
            return Err("rollout_canary_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "sample_count": self.sample_count,
            "error_rate_bps": self.error_rate_bps,
            "availability_bps": self.availability_bps,
            "max_error_rate_bps": self.max_error_rate_bps,
            "min_availability_bps": self.min_availability_bps,
            "window_ms": self.window_ms,
            "measured_at_unix_ms": self.measured_at_unix_ms,
            "passed": self.passed,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestratedRolloutPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub rollout_id: String,
    pub profile: OrchestratedRolloutProfile,
    pub backend: OrchestratedBackend,
    pub target_revision_id: String,
    pub rollback_revision_id: String,
    pub routing: WorkerBuildRoutingTable,
    pub canary: CanaryObservation,
    pub planned_at_unix_ms: u64,
    pub progress_deadline_unix_ms: u64,
    pub plan_digest: String,
}

impl OrchestratedRolloutPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rollout_id: impl Into<String>,
        profile: OrchestratedRolloutProfile,
        backend: OrchestratedBackend,
        target_revision_id: impl Into<String>,
        rollback_revision_id: impl Into<String>,
        routing: WorkerBuildRoutingTable,
        canary: CanaryObservation,
        planned_at_unix_ms: u64,
        progress_deadline_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut plan = Self {
            schema: ORCHESTRATED_ROLLOUT_SCHEMA.to_owned(),
            version: ORCHESTRATED_ROLLOUT_VERSION,
            rollout_id: rollout_id.into(),
            profile,
            backend,
            target_revision_id: target_revision_id.into(),
            rollback_revision_id: rollback_revision_id.into(),
            routing,
            canary,
            planned_at_unix_ms,
            progress_deadline_unix_ms,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ORCHESTRATED_ROLLOUT_SCHEMA
            || self.version != ORCHESTRATED_ROLLOUT_VERSION
            || self.planned_at_unix_ms == 0
            || self.progress_deadline_unix_ms <= self.planned_at_unix_ms
        {
            return Err("rollout_plan_header_invalid".to_owned());
        }
        bounded_text(&self.rollout_id, "rollout_id")?;
        bounded_text(&self.target_revision_id, "rollout_target_revision")?;
        bounded_text(&self.rollback_revision_id, "rollout_rollback_revision")?;
        if self.target_revision_id == self.rollback_revision_id {
            return Err("rollout_target_rollback_revision_same".to_owned());
        }
        self.routing.validate()?;
        self.canary.validate()?;
        validate_profile(self.profile, &self.routing.routes)?;
        if self.routing.route(&self.target_revision_id).is_none() {
            return Err("rollout_target_route_missing".to_owned());
        }
        if self.routing.route(&self.rollback_revision_id).is_none() {
            return Err("rollout_rollback_route_missing".to_owned());
        }
        validate_digest(&self.plan_digest, "rollout_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("rollout_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Evaluate a future adapter action against the pinned, source-supplied facts.
    pub fn decide(
        &self,
        action: OrchestratedRolloutAction,
        now_unix_ms: u64,
        reason: Option<&str>,
    ) -> Result<OrchestratedRolloutDecision, String> {
        self.validate()?;
        let (allowed, decision_reason) = match action {
            OrchestratedRolloutAction::Pause => {
                if reason.is_none_or(|value| value.trim().is_empty()) {
                    (false, "rollout_pause_reason_missing")
                } else {
                    (true, "pause_requested")
                }
            }
            OrchestratedRolloutAction::Resume | OrchestratedRolloutAction::Promote => {
                if now_unix_ms == 0 || now_unix_ms > self.progress_deadline_unix_ms {
                    (false, "rollout_progress_deadline_exceeded")
                } else if !self.canary.passed {
                    (false, "rollout_canary_gate_blocked")
                } else if self.routing.active_writer_revision_id != self.target_revision_id {
                    (false, "rollout_old_worker_not_fenced")
                } else if self
                    .routing
                    .route(&self.target_revision_id)
                    .is_none_or(|route| route.traffic_weight_bps == 0)
                {
                    (false, "rollout_target_route_not_serving")
                } else {
                    (true, "ok")
                }
            }
            OrchestratedRolloutAction::Rollback => {
                if now_unix_ms == 0 {
                    (false, "rollout_time_invalid")
                } else if self.routing.route(&self.rollback_revision_id).is_none() {
                    (false, "rollout_rollback_route_missing")
                } else {
                    (true, "rollback_requested")
                }
            }
        };
        OrchestratedRolloutDecision::new(action, allowed, decision_reason)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "rollout_id": self.rollout_id,
            "profile": self.profile,
            "backend": self.backend,
            "target_revision_id": self.target_revision_id,
            "rollback_revision_id": self.rollback_revision_id,
            "routing": self.routing,
            "canary": self.canary,
            "planned_at_unix_ms": self.planned_at_unix_ms,
            "progress_deadline_unix_ms": self.progress_deadline_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestratedRolloutDecision {
    pub action: OrchestratedRolloutAction,
    pub allowed: bool,
    pub reason: String,
    pub decision_digest: String,
}

impl OrchestratedRolloutDecision {
    fn new(
        action: OrchestratedRolloutAction,
        allowed: bool,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut decision = Self {
            action,
            allowed,
            reason: reason.into(),
            decision_digest: String::new(),
        };
        bounded_text(&decision.reason, "rollout_decision_reason")?;
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        bounded_text(&self.reason, "rollout_decision_reason")?;
        validate_digest(&self.decision_digest, "rollout_decision_digest")?;
        if self.decision_digest != self.digest() {
            return Err("rollout_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "action": self.action,
            "allowed": self.allowed,
            "reason": self.reason,
        }))
    }
}

fn validate_profile(
    profile: OrchestratedRolloutProfile,
    routes: &[WorkerBuildRoute],
) -> Result<(), String> {
    let valid = match profile {
        OrchestratedRolloutProfile::Canary => routes.len() >= 2,
        OrchestratedRolloutProfile::BlueGreen => routes.len() == 2,
        OrchestratedRolloutProfile::Rainbow => routes.len() >= 3,
    };
    if valid {
        Ok(())
    } else {
        Err("rollout_profile_route_count_invalid".to_owned())
    }
}

fn bounded_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_ROLLOUT_REASON || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
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
