//! Read-only observability alert/incident projection.
//!
//! Rules consume committed metric and failure facts. They deduplicate by a stable fingerprint and
//! attach a reconciliation-safe recovery plan; no rule can approve, retry, close an unknown
//! effect, or mutate the EventLog.

use kiana_domain::{
    json_digest, AlertSeverity, AlertState, EventId, MetricSnapshot, ObservabilityAlert,
    ObservabilityIncident, ObservabilityIncidentCategory, ObservabilityIncidentSnapshot,
    ObservabilityIncidentState, RuntimeEvent,
};
use std::collections::{BTreeMap, BTreeSet};

use super::{project_operational_metrics, ControlPlane, CoreError};

const MAX_RULE_EVENTS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IncidentProjectionError {
    #[error("observability_incident_source_empty")]
    SourceEmpty,
    #[error("observability_incident_metrics_invalid:{0}")]
    MetricsInvalid(String),
    #[error("observability_incident_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
}

struct Cause {
    category: Option<ObservabilityIncidentCategory>,
    severity: AlertSeverity,
    rule: &'static str,
    message: &'static str,
    requires_reconciliation: bool,
    event_ids: BTreeSet<String>,
}

impl Cause {
    fn add_event(&mut self, event: &RuntimeEvent) {
        if self.event_ids.len() < MAX_RULE_EVENTS {
            self.event_ids.insert(event.event_id.to_string());
        }
    }
}

fn cause_for<'a>(
    causes: &'a mut BTreeMap<&'static str, Cause>,
    category: ObservabilityIncidentCategory,
    severity: AlertSeverity,
    rule: &'static str,
    message: &'static str,
    requires_reconciliation: bool,
) -> &'a mut Cause {
    causes.entry(rule).or_insert_with(|| Cause {
        category: Some(category),
        severity,
        rule,
        message,
        requires_reconciliation,
        event_ids: BTreeSet::new(),
    })
}

fn metric_value(metrics: &MetricSnapshot, name: &str) -> u64 {
    metrics
        .points
        .iter()
        .find(|point| point.name == name)
        .map(|point| point.value.max(0.0) as u64)
        .unwrap_or_default()
}

fn category_name(category: ObservabilityIncidentCategory) -> &'static str {
    match category {
        ObservabilityIncidentCategory::ProjectorLag => "projector_lag",
        ObservabilityIncidentCategory::AuditLoss => "audit_loss",
        ObservabilityIncidentCategory::RedactionFailure => "redaction_failure",
        ObservabilityIncidentCategory::QueueOverflow => "queue_overflow",
        ObservabilityIncidentCategory::JournalCorruption => "journal_corruption",
        ObservabilityIncidentCategory::EffectUnknown => "effect_unknown",
        ObservabilityIncidentCategory::OrphanDispatch => "orphan_dispatch",
    }
}

fn source_ids(metrics: &MetricSnapshot, cause: &Cause) -> Vec<EventId> {
    if cause.event_ids.is_empty() {
        return metrics.source_event_ids.clone();
    }
    let filtered = metrics
        .source_event_ids
        .iter()
        .filter(|event_id| cause.event_ids.contains(&event_id.to_string()))
        .copied()
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        metrics.source_event_ids.clone()
    } else {
        filtered
    }
}

fn rule_for_event(
    event: &RuntimeEvent,
) -> Option<(
    &'static str,
    ObservabilityIncidentCategory,
    AlertSeverity,
    &'static str,
    &'static str,
    bool,
)> {
    let kind = event.kind.to_ascii_lowercase();
    if kind.contains("redaction") && (kind.contains("fail") || event.data.get("error").is_some()) {
        return Some((
            "redaction_failure",
            ObservabilityIncidentCategory::RedactionFailure,
            AlertSeverity::Critical,
            "redaction_failure",
            "Redaction failed; candidate telemetry was withheld",
            true,
        ));
    }
    if kind.contains("queue") || kind.contains("overflow") {
        return Some((
            "queue_overflow",
            ObservabilityIncidentCategory::QueueOverflow,
            AlertSeverity::Error,
            "queue_overflow",
            "Observability queue capacity was exceeded",
            false,
        ));
    }
    if kind.contains("journal") && (kind.contains("corrupt") || kind.contains("fail")) {
        return Some((
            "journal_corruption",
            ObservabilityIncidentCategory::JournalCorruption,
            AlertSeverity::Critical,
            "journal_corruption",
            "Journal integrity or commit evidence requires inspection",
            true,
        ));
    }
    if kind.starts_with("audit.") && (kind.contains("fail") || kind.contains("unknown")) {
        return Some((
            "audit_loss",
            ObservabilityIncidentCategory::AuditLoss,
            AlertSeverity::Critical,
            "audit_loss",
            "Required audit evidence is missing or unknown",
            true,
        ));
    }
    None
}

/// Derive deduplicated alerts/incidents from committed facts and the OA-10 metrics snapshot.
pub fn project_observability_incidents(
    events: &[RuntimeEvent],
    metrics: &MetricSnapshot,
) -> Result<ObservabilityIncidentSnapshot, IncidentProjectionError> {
    if events.is_empty() {
        return Err(IncidentProjectionError::SourceEmpty);
    }
    metrics
        .validate()
        .map_err(IncidentProjectionError::MetricsInvalid)?;
    let mut causes: BTreeMap<&'static str, Cause> = BTreeMap::new();
    if metrics.projector_cursor < metrics.source_cursor
        || metrics
            .limitations
            .iter()
            .any(|limitation| limitation == "eventlog_cursor_gap")
    {
        cause_for(
            &mut causes,
            ObservabilityIncidentCategory::ProjectorLag,
            AlertSeverity::Warning,
            "projector_lag",
            "Projector cursor is behind or has a source gap",
            true,
        );
    }
    if metric_value(metrics, "kiana.recovery.unknown_total") > 0 {
        cause_for(
            &mut causes,
            ObservabilityIncidentCategory::EffectUnknown,
            AlertSeverity::Critical,
            "effect_unknown",
            "An external effect is unknown and remains fenced",
            true,
        );
    }
    if metric_value(metrics, "kiana.recovery.orphan_total") > 0 {
        cause_for(
            &mut causes,
            ObservabilityIncidentCategory::OrphanDispatch,
            AlertSeverity::Error,
            "orphan_dispatch",
            "A dispatch has no committed terminal result",
            true,
        );
    }
    if metric_value(metrics, "kiana.receipts.query_failure_total") > 0 {
        cause_for(
            &mut causes,
            ObservabilityIncidentCategory::AuditLoss,
            AlertSeverity::Error,
            "audit_query_failure",
            "Receipt/audit query failed and requires re-read",
            true,
        );
    }
    if metric_value(metrics, "kiana.artifacts.read_failure_total") > 0 {
        cause_for(
            &mut causes,
            ObservabilityIncidentCategory::AuditLoss,
            AlertSeverity::Error,
            "artifact_read_failure",
            "An evidence artifact could not be read or verified",
            true,
        );
    }
    for event in events {
        if let Some((rule, category, severity, message, plan, requires)) = rule_for_event(event) {
            cause_for(&mut causes, category, severity, rule, message, requires).add_event(event);
            if plan == "redaction_failure" {
                cause_for(
                    &mut causes,
                    ObservabilityIncidentCategory::RedactionFailure,
                    severity,
                    rule,
                    message,
                    requires,
                )
                .add_event(event);
            }
        }
    }

    let mut alerts = Vec::new();
    let mut incidents = Vec::new();
    for cause in causes.values() {
        let category = cause
            .category
            .ok_or(IncidentProjectionError::MetricsInvalid(
                "category_missing".to_owned(),
            ))?;
        let fingerprint = json_digest(&serde_json::json!({
            "component": "observability",
            "rule": cause.rule,
            "category": category_name(category),
        }));
        let incident_id = format!("observability-incident:{fingerprint}");
        let alert_id = format!("observability-alert:{fingerprint}");
        let ids = source_ids(metrics, cause);
        let alert = ObservabilityAlert::new(
            alert_id.clone(),
            fingerprint.clone(),
            cause.rule,
            cause.severity,
            AlertState::Open,
            metrics.source_cursor,
            ids.clone(),
            cause.message,
            Some(incident_id.clone()),
        )
        .map_err(IncidentProjectionError::SnapshotInvalid)?;
        let recovery_plan = vec![
            "Inspect committed source facts and current cursor".to_owned(),
            "Fence unknown effects; do not retry or approve automatically".to_owned(),
            "Require an explicit reconciliation decision before closure".to_owned(),
        ];
        let incident = ObservabilityIncident::new(
            incident_id,
            fingerprint,
            category,
            cause.severity,
            ObservabilityIncidentState::Open,
            metrics.source_cursor,
            ids,
            cause.requires_reconciliation,
            recovery_plan,
            vec![alert_id],
        )
        .map_err(IncidentProjectionError::SnapshotInvalid)?;
        alerts.push(alert);
        incidents.push(incident);
    }
    let mut limitations = metrics.limitations.clone();
    if !causes.is_empty() && limitations.len() < kiana_domain::MAX_HEALTH_LIMITATIONS {
        limitations.push("observability_incident_projection_is_diagnostic".to_owned());
    }
    ObservabilityIncidentSnapshot::new(
        metrics.source_cursor,
        metrics.source_event_ids.clone(),
        alerts,
        incidents,
        limitations,
    )
    .map_err(IncidentProjectionError::SnapshotInvalid)
}

pub fn project_incidents(
    events: &[RuntimeEvent],
) -> Result<ObservabilityIncidentSnapshot, IncidentProjectionError> {
    let metrics = project_operational_metrics(events, None)
        .map_err(|error| IncidentProjectionError::MetricsInvalid(error.to_string()))?;
    project_observability_incidents(events, &metrics)
}

impl ControlPlane {
    pub async fn observability_incidents(
        &self,
    ) -> Result<ObservabilityIncidentSnapshot, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable(
                "observability_incidents_read_all_unsupported".to_owned(),
            )
        })?;
        let metrics = project_operational_metrics(&events, None)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        project_observability_incidents(&events, &metrics)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }
}
