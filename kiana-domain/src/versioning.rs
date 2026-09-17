//! Version-governance contracts for model routing and drift projections.
//!
//! A drift bucket is a read-only projection of committed `run.model_turn` facts.  The
//! version identity deliberately includes the model profile, prompt, route and budget
//! dimensions so a metric change cannot be attributed to an ambiguous runtime label.
//! These records do not grant authority and never select or switch a model.

use crate::{json_digest, EventId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const ROUTE_DECISION_SCHEMA: &str = "kiana.route-decision.v1";
pub const DRIFT_REPORT_SCHEMA: &str = "kiana.drift-report.v1";
pub const DRIFT_MEASUREMENT_OBSERVED_TURNS: &str = "observed_turns";
pub const DRIFT_COST_UNKNOWN: &str = "unknown";

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn field(data: &Value, paths: &[&str], fallback: &str) -> String {
    paths
        .iter()
        .filter_map(|path| {
            let value = if path.starts_with('/') {
                data.pointer(path)
            } else {
                data.get(*path)
            }?;
            value.as_str()
        })
        .find(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| fallback.to_owned())
}

/// Server-derived identity of one model route and prompt/budget configuration.
///
/// This is an audit/version identity, not a provider request and not an execution permit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteDecision {
    pub schema: String,
    #[serde(rename = "provider")]
    pub provider_id: String,
    #[serde(rename = "model")]
    pub model_id: String,
    pub model_profile: String,
    pub prompt_hash: String,
    pub route_digest: String,
    pub configuration_revision: String,
    pub budget_schema: String,
    pub runtime_version: String,
}

impl RouteDecision {
    /// Build a version identity from the redacted model-turn audit metadata.
    ///
    /// Missing compatibility fields are represented by the bounded `unknown` label rather
    /// than silently dropping a turn from drift accounting.  The route digest is derived from
    /// the audited route when older producers did not provide one.
    pub fn from_model_turn(data: &Value, runtime_version: impl Into<String>) -> Self {
        let provider_id = field(
            data,
            &["provider_id", "/prepared/route/provider_id"],
            "unknown",
        );
        let model_id = field(data, &["model_id", "/prepared/route/model_id"], "unknown");
        let model_profile = field(
            data,
            &["model_profile", "/prepared/route/profile"],
            "unknown",
        );
        let prompt_hash = field(
            data,
            &["prompt_hash", "prompt_version", "/prepared/prompt_version"],
            "unknown",
        );
        let configuration_revision = field(
            data,
            &[
                "configuration_revision",
                "/prepared/route/configuration_revision",
            ],
            "unknown",
        );
        let route_digest = field(data, &["route_digest", "/prepared/route_digest"], "");
        let route_digest = if route_digest.is_empty() {
            json_digest(&json!({
                "provider_id": provider_id,
                "model_id": model_id,
                "profile": model_profile,
                "configuration_revision": configuration_revision,
                "streaming": data
                    .get("streaming")
                    .or_else(|| data.pointer("/prepared/route/streaming"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            }))
        } else {
            route_digest
        };
        Self {
            schema: ROUTE_DECISION_SCHEMA.to_owned(),
            provider_id,
            model_id,
            model_profile,
            prompt_hash,
            route_digest,
            configuration_revision,
            budget_schema: field(data, &["/budget/schema", "budget_schema"], "unknown"),
            runtime_version: runtime_version.into(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != ROUTE_DECISION_SCHEMA
            || !bounded(&self.provider_id, 256)
            || !bounded(&self.model_id, 512)
            || !bounded(&self.model_profile, 256)
            || !bounded(&self.prompt_hash, 512)
            || !bounded(&self.route_digest, 256)
            || !bounded(&self.configuration_revision, 256)
            || !bounded(&self.budget_schema, 256)
            || !bounded(&self.runtime_version, 128)
        {
            return Err("route_decision_invalid");
        }
        Ok(())
    }

    /// Canonical version dimensions used as the drift bucket key.
    pub fn version_key(&self) -> String {
        json_digest(&json!({
            "provider": self.provider_id,
            "model": self.model_id,
            "model_profile": self.model_profile,
            "prompt_hash": self.prompt_hash,
            "route_digest": self.route_digest,
            "configuration_revision": self.configuration_revision,
            "budget_schema": self.budget_schema,
            "runtime_version": self.runtime_version,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftBucket {
    pub version: RouteDecision,
    pub turns: u64,
    pub errors: u64,
    pub elapsed_ms: u64,
    pub event_ids: Vec<EventId>,
}

impl DriftBucket {
    pub fn new(version: RouteDecision) -> Result<Self, &'static str> {
        version.validate()?;
        Ok(Self {
            version,
            turns: 0,
            errors: 0,
            elapsed_ms: 0,
            event_ids: Vec::new(),
        })
    }

    pub fn record(
        &mut self,
        event_id: EventId,
        elapsed_ms: u64,
        error: bool,
    ) -> Result<(), &'static str> {
        if self.event_ids.contains(&event_id) {
            return Err("drift_event_duplicate");
        }
        self.turns = self.turns.checked_add(1).ok_or("drift_turn_overflow")?;
        self.elapsed_ms = self
            .elapsed_ms
            .checked_add(elapsed_ms)
            .ok_or("drift_elapsed_overflow")?;
        if error {
            self.errors = self.errors.checked_add(1).ok_or("drift_error_overflow")?;
        }
        self.event_ids.push(event_id);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        self.version.validate()?;
        if self.version.version_key().is_empty()
            || self.turns != self.event_ids.len() as u64
            || self.errors > self.turns
        {
            return Err("drift_bucket_invalid");
        }
        let mut ids = std::collections::BTreeSet::new();
        if self.event_ids.iter().any(|id| !ids.insert(*id)) {
            return Err("drift_event_duplicate");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftReport {
    pub schema: String,
    pub buckets: BTreeMap<String, DriftBucket>,
    pub automatic_model_switch: bool,
    pub measurement: String,
    pub cost: String,
}

impl Default for DriftReport {
    fn default() -> Self {
        Self {
            schema: DRIFT_REPORT_SCHEMA.to_owned(),
            buckets: BTreeMap::new(),
            automatic_model_switch: false,
            measurement: DRIFT_MEASUREMENT_OBSERVED_TURNS.to_owned(),
            cost: DRIFT_COST_UNKNOWN.to_owned(),
        }
    }
}

impl DriftReport {
    pub fn record(
        &mut self,
        version: RouteDecision,
        event_id: EventId,
        elapsed_ms: u64,
        error: bool,
    ) -> Result<(), &'static str> {
        let key = version.version_key();
        let bucket = self
            .buckets
            .entry(key)
            .or_insert(DriftBucket::new(version)?);
        bucket.record(event_id, elapsed_ms, error)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DRIFT_REPORT_SCHEMA
            || self.automatic_model_switch
            || self.measurement != DRIFT_MEASUREMENT_OBSERVED_TURNS
            || self.cost != DRIFT_COST_UNKNOWN
        {
            return Err("drift_report_invalid");
        }
        for (key, bucket) in &self.buckets {
            bucket.validate()?;
            if key != &bucket.version.version_key() {
                return Err("drift_bucket_key_mismatch");
            }
        }
        Ok(())
    }
}
