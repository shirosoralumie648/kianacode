//! Operation-bound lifecycle metrics, structured log and audit evidence contract.
//!
//! This is a redacted, read-only evidence bundle. It does not append an audit event, emit a
//! metric, export a trace or infer a successful effect from an observation.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DEPLOYMENT_OBSERVABILITY_SCHEMA: &str = "kiana.deployment-observability.v1";
pub const DEPLOYMENT_OBSERVABILITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_OBSERVABILITY_ITEMS: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleMetricFact {
    pub name: String,
    pub value: u64,
    pub unit: String,
    pub operation_ref: String,
    pub revision_digest: String,
    pub source_cursor: u64,
    pub metric_digest: String,
}

impl LifecycleMetricFact {
    pub fn new(
        name: impl Into<String>,
        value: u64,
        unit: impl Into<String>,
        operation_ref: impl Into<String>,
        revision_digest: impl Into<String>,
        source_cursor: u64,
    ) -> Result<Self, String> {
        let mut fact = Self {
            name: name.into(),
            value,
            unit: unit.into(),
            operation_ref: operation_ref.into(),
            revision_digest: revision_digest.into(),
            source_cursor,
            metric_digest: String::new(),
        };
        fact.metric_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || !self.name.starts_with("kiana.")
            || self.unit.trim().is_empty()
            || self.unit.len() > 32
            || self.operation_ref.trim().is_empty()
            || self.operation_ref.len() > 256
            || self.source_cursor == 0
        {
            return Err("deployment_metric_fact_header_invalid".to_owned());
        }
        valid_digest(&self.revision_digest, "deployment_metric_revision_digest")?;
        valid_digest(&self.metric_digest, "deployment_metric_digest")?;
        if self.metric_digest != self.digest() {
            return Err("deployment_metric_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "name": self.name,
            "value": self.value,
            "unit": self.unit,
            "operation_ref": self.operation_ref,
            "revision_digest": self.revision_digest,
            "source_cursor": self.source_cursor,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleEvidenceBundle {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation_ref: String,
    pub revision_digest: String,
    pub source_cursor: u64,
    pub trace_ref: String,
    pub structured_log_digest: String,
    pub audit_event_digest: String,
    pub metrics: Vec<LifecycleMetricFact>,
    pub secret_free: bool,
    pub bundle_digest: String,
}

impl LifecycleEvidenceBundle {
    pub fn new(
        operation_ref: impl Into<String>,
        revision_digest: impl Into<String>,
        source_cursor: u64,
        trace_ref: impl Into<String>,
        structured_log_digest: impl Into<String>,
        audit_event_digest: impl Into<String>,
        metrics: Vec<LifecycleMetricFact>,
    ) -> Result<Self, String> {
        let mut bundle = Self {
            schema: DEPLOYMENT_OBSERVABILITY_SCHEMA.to_owned(),
            version: DEPLOYMENT_OBSERVABILITY_VERSION,
            operation_ref: operation_ref.into(),
            revision_digest: revision_digest.into(),
            source_cursor,
            trace_ref: trace_ref.into(),
            structured_log_digest: structured_log_digest.into(),
            audit_event_digest: audit_event_digest.into(),
            metrics,
            secret_free: true,
            bundle_digest: String::new(),
        };
        bundle.bundle_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_OBSERVABILITY_SCHEMA
            || self.version != DEPLOYMENT_OBSERVABILITY_VERSION
            || !self.secret_free
            || self.operation_ref.trim().is_empty()
            || self.operation_ref.len() > 256
            || self.source_cursor == 0
            || self.trace_ref.trim().is_empty()
            || self.trace_ref.len() > 256
            || self.metrics.len() > MAX_OBSERVABILITY_ITEMS
        {
            return Err("deployment_observability_header_invalid".to_owned());
        }
        valid_digest(
            &self.revision_digest,
            "deployment_observability_revision_digest",
        )?;
        valid_digest(
            &self.structured_log_digest,
            "deployment_observability_log_digest",
        )?;
        valid_digest(
            &self.audit_event_digest,
            "deployment_observability_audit_digest",
        )?;
        valid_digest(
            &self.bundle_digest,
            "deployment_observability_bundle_digest",
        )?;
        if self.trace_ref.contains(['\0', '\r', '\n'])
            || self.trace_ref.to_ascii_lowercase().contains("secret")
            || self.trace_ref.to_ascii_lowercase().contains("token=")
        {
            return Err("deployment_observability_secret_marker".to_owned());
        }
        let mut names = BTreeSet::new();
        for metric in &self.metrics {
            metric.validate()?;
            if metric.operation_ref != self.operation_ref
                || metric.revision_digest != self.revision_digest
                || metric.source_cursor != self.source_cursor
                || !names.insert(metric.name.as_str())
            {
                return Err("deployment_observability_metric_binding_invalid".to_owned());
            }
        }
        if self.bundle_digest != self.digest() {
            return Err("deployment_observability_bundle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_ref": self.operation_ref,
            "revision_digest": self.revision_digest,
            "source_cursor": self.source_cursor,
            "trace_ref": self.trace_ref,
            "structured_log_digest": self.structured_log_digest,
            "audit_event_digest": self.audit_event_digest,
            "metrics": self.metrics,
            "secret_free": self.secret_free,
        }))
    }
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
