//! EQ-46 version-bucket drift metrics and alert evidence.
//!
//! The contract consumes the existing read-only [`DriftReport`] projection.  It derives bounded
//! metrics and, when a threshold is exceeded, a `drift.alerted` event payload.  The payload binds
//! the canonical target and source cursor and proves that route and grant snapshots did not
//! change.  It never selects a model, changes a route, or grants authority.

use crate::{json_digest, DriftAlertId, DriftReport};
use serde::{Deserialize, Serialize};

pub const DRIFT_INPUT_SCHEMA: &str = "kiana.quality-drift-input.v1";
pub const DRIFT_METRICS_SCHEMA: &str = "kiana.quality-drift-metrics.v1";
pub const DRIFT_ALERT_SCHEMA: &str = "kiana.quality-drift-alert.v1";
pub const DRIFT_ALERT_EVENT_SCHEMA: &str = "kiana.drift-alerted-event.v1";
pub const DRIFT_ALERT_EVENT_KIND: &str = "drift.alerted";
const MAX_BUCKETS: usize = 256;
const MAX_TEXT_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftVerdict {
    Healthy,
    Alerted,
    NeedsReview,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftThreshold {
    pub schema: String,
    pub minimum_samples: u64,
    pub max_error_rate_milli: Option<u64>,
    pub max_mean_elapsed_ms: Option<u64>,
    pub threshold_digest: String,
}

impl DriftThreshold {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != "kiana.quality-drift-threshold.v1"
            || self.minimum_samples == 0
            || self.minimum_samples > 1_000_000
            || self.max_error_rate_milli.is_some_and(|value| value > 1_000)
            || self.threshold_digest != self.digest()
        {
            return Err("drift_threshold_invalid");
        }
        if self.max_error_rate_milli.is_none() && self.max_mean_elapsed_ms.is_none() {
            return Err("drift_threshold_empty");
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "minimum_samples": self.minimum_samples,
            "max_error_rate_milli": self.max_error_rate_milli,
            "max_mean_elapsed_ms": self.max_mean_elapsed_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftMetricBucket {
    pub version_key: String,
    pub sample_count: u64,
    pub error_count: u64,
    pub elapsed_ms: u64,
    pub error_rate_milli: u64,
    pub mean_elapsed_ms: u64,
}

impl DriftMetricBucket {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_text(&self.version_key)
            || self.sample_count == 0
            || self.error_count > self.sample_count
            || self.error_rate_milli > 1_000
            || self.mean_elapsed_ms > self.elapsed_ms
            || self.error_rate_milli != self.error_count.saturating_mul(1_000) / self.sample_count
            || self.mean_elapsed_ms != self.elapsed_ms / self.sample_count
        {
            return Err("drift_metric_bucket_invalid");
        }
        Ok(())
    }

    fn from_report(version_key: &str, bucket: &crate::DriftBucket) -> Result<Self, &'static str> {
        bucket.validate()?;
        let result = Self {
            version_key: version_key.to_owned(),
            sample_count: bucket.turns,
            error_count: bucket.errors,
            elapsed_ms: bucket.elapsed_ms,
            error_rate_milli: bucket.errors.saturating_mul(1_000) / bucket.turns.max(1),
            mean_elapsed_ms: bucket.elapsed_ms / bucket.turns.max(1),
        };
        result.validate()?;
        Ok(result)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftMetrics {
    pub schema: String,
    pub target_digest: String,
    pub source_cursor: u64,
    pub threshold_digest: String,
    pub buckets: Vec<DriftMetricBucket>,
    pub verdict: DriftVerdict,
    pub metrics_digest: String,
}

impl DriftMetrics {
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "target_digest": self.target_digest,
            "source_cursor": self.source_cursor,
            "threshold_digest": self.threshold_digest,
            "buckets": self.buckets,
            "verdict": self.verdict,
        }))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DRIFT_METRICS_SCHEMA
            || !valid_digest(&self.target_digest)
            || self.source_cursor == 0
            || !valid_digest(&self.threshold_digest)
            || self.buckets.is_empty()
            || self.buckets.len() > MAX_BUCKETS
            || self
                .buckets
                .windows(2)
                .any(|pair| pair[0].version_key >= pair[1].version_key)
            || self.buckets.iter().any(|bucket| bucket.validate().is_err())
            || !valid_digest(&self.metrics_digest)
            || self.metrics_digest != self.digest()
        {
            return Err("drift_metrics_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftAlert {
    pub schema: String,
    pub alert_id: DriftAlertId,
    pub target_digest: String,
    pub metrics_digest: String,
    pub threshold_digest: String,
    pub version_key: String,
    pub reason: String,
    pub sample_count: u64,
    pub route_digest_before: String,
    pub route_digest_after: String,
    pub grant_digest_before: String,
    pub grant_digest_after: String,
    pub authority_changes_applied: bool,
    pub alert_digest: String,
}

impl DriftAlert {
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "alert_id": self.alert_id,
            "target_digest": self.target_digest,
            "metrics_digest": self.metrics_digest,
            "threshold_digest": self.threshold_digest,
            "version_key": self.version_key,
            "reason": self.reason,
            "sample_count": self.sample_count,
            "route_digest_before": self.route_digest_before,
            "route_digest_after": self.route_digest_after,
            "grant_digest_before": self.grant_digest_before,
            "grant_digest_after": self.grant_digest_after,
            "authority_changes_applied": self.authority_changes_applied,
        }))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DRIFT_ALERT_SCHEMA
            || self.alert_id.as_uuid().is_nil()
            || !valid_digest(&self.target_digest)
            || !valid_digest(&self.metrics_digest)
            || !valid_digest(&self.threshold_digest)
            || !valid_text(&self.version_key)
            || !valid_text(&self.reason)
            || self.sample_count == 0
            || !valid_digest(&self.route_digest_before)
            || self.route_digest_before != self.route_digest_after
            || !valid_digest(&self.grant_digest_before)
            || self.grant_digest_before != self.grant_digest_after
            || self.authority_changes_applied
            || !valid_digest(&self.alert_digest)
            || self.alert_digest != self.digest()
        {
            return Err("drift_alert_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftAlertEvent {
    pub schema: String,
    pub kind: String,
    pub source_cursor: u64,
    pub alert: DriftAlert,
    pub event_digest: String,
}

impl DriftAlertEvent {
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "kind": self.kind,
            "source_cursor": self.source_cursor,
            "alert": self.alert,
        }))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DRIFT_ALERT_EVENT_SCHEMA
            || self.kind != DRIFT_ALERT_EVENT_KIND
            || self.source_cursor == 0
            || self.alert.validate().is_err()
            || !valid_digest(&self.event_digest)
            || self.event_digest != self.digest()
        {
            return Err("drift_alert_event_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftEvaluationInput {
    pub schema: String,
    pub target_digest: String,
    pub report: DriftReport,
    pub threshold: DriftThreshold,
    pub route_digest_before: String,
    pub route_digest_after: String,
    pub grant_digest_before: String,
    pub grant_digest_after: String,
    pub source_cursor: u64,
    pub alert_id: DriftAlertId,
}

impl DriftEvaluationInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DRIFT_INPUT_SCHEMA
            || !valid_digest(&self.target_digest)
            || self.report.validate().is_err()
            || self.threshold.validate().is_err()
            || !valid_digest(&self.route_digest_before)
            || self.route_digest_before != self.route_digest_after
            || !valid_digest(&self.grant_digest_before)
            || self.grant_digest_before != self.grant_digest_after
            || self.source_cursor == 0
            || self.alert_id.as_uuid().is_nil()
        {
            return Err("drift_input_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriftEvaluation {
    pub metrics: DriftMetrics,
    pub alert: Option<DriftAlert>,
    pub event: Option<DriftAlertEvent>,
}

pub fn evaluate_drift(input: &DriftEvaluationInput) -> Result<DriftEvaluation, &'static str> {
    input.validate()?;
    let buckets = input
        .report
        .buckets
        .iter()
        .map(|(key, bucket)| DriftMetricBucket::from_report(key, bucket))
        .collect::<Result<Vec<_>, _>>()?;
    if buckets.is_empty() {
        return Err("drift_samples_missing");
    }
    let mut metrics = DriftMetrics {
        schema: DRIFT_METRICS_SCHEMA.to_owned(),
        target_digest: input.target_digest.clone(),
        source_cursor: input.source_cursor,
        threshold_digest: input.threshold.threshold_digest.clone(),
        buckets,
        verdict: DriftVerdict::Healthy,
        metrics_digest: String::new(),
    };
    let mut alert_bucket = None;
    let mut insufficient = false;
    for bucket in &metrics.buckets {
        if bucket.sample_count < input.threshold.minimum_samples {
            insufficient = true;
        }
        if input
            .threshold
            .max_error_rate_milli
            .is_some_and(|limit| bucket.error_rate_milli > limit)
            || input
                .threshold
                .max_mean_elapsed_ms
                .is_some_and(|limit| bucket.mean_elapsed_ms > limit)
        {
            alert_bucket = Some(bucket);
            break;
        }
    }
    if insufficient {
        metrics.verdict = DriftVerdict::NeedsReview;
        metrics.metrics_digest = metrics.digest();
        return Ok(DriftEvaluation {
            metrics,
            alert: None,
            event: None,
        });
    }
    if let Some(bucket) = alert_bucket {
        metrics.verdict = DriftVerdict::Alerted;
        metrics.metrics_digest = metrics.digest();
        let mut alert = DriftAlert {
            schema: DRIFT_ALERT_SCHEMA.to_owned(),
            alert_id: input.alert_id,
            target_digest: input.target_digest.clone(),
            metrics_digest: metrics.metrics_digest.clone(),
            threshold_digest: input.threshold.threshold_digest.clone(),
            version_key: bucket.version_key.clone(),
            reason: "drift_threshold_exceeded".to_owned(),
            sample_count: bucket.sample_count,
            route_digest_before: input.route_digest_before.clone(),
            route_digest_after: input.route_digest_after.clone(),
            grant_digest_before: input.grant_digest_before.clone(),
            grant_digest_after: input.grant_digest_after.clone(),
            authority_changes_applied: false,
            alert_digest: String::new(),
        };
        alert.alert_digest = alert.digest();
        let mut event = DriftAlertEvent {
            schema: DRIFT_ALERT_EVENT_SCHEMA.to_owned(),
            kind: DRIFT_ALERT_EVENT_KIND.to_owned(),
            source_cursor: input.source_cursor,
            alert,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        event.validate()?;
        return Ok(DriftEvaluation {
            metrics,
            alert: Some(event.alert.clone()),
            event: Some(event),
        });
    }
    metrics.verdict = DriftVerdict::Healthy;
    metrics.metrics_digest = metrics.digest();
    Ok(DriftEvaluation {
        metrics,
        alert: None,
        event: None,
    })
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
