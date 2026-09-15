//! Read-only startup/readiness/liveness health aggregation.
//!
//! Health is deliberately not a second authority. The aggregator reads committed facts and
//! capability declarations, then reports stale, unknown or missing evidence as degraded. It never
//! grants a permit, changes a run, or asks a provider/Broker to self-report readiness.

use kiana_domain::{
    ComponentHealth, ComponentHealthState, EventId, EventStoreCapabilities, HealthProbeKind,
    HealthSnapshot, MetricSnapshot, RuntimeEvent, SignalStatus,
};
use std::collections::{BTreeMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{project_operational_metrics, ControlPlane, CoreError};

const COMPONENT_VERSION: &str = "health.v1";
const MAX_LIMITATIONS: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HealthProjectionError {
    #[error("health_source_empty")]
    SourceEmpty,
    #[error("health_observed_at_invalid")]
    ObservedAtInvalid,
    #[error("health_metrics_unavailable:{0}")]
    MetricsUnavailable(String),
    #[error("health_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
}

fn bounded_source_ids(events: &[RuntimeEvent]) -> Vec<EventId> {
    let mut seen = HashSet::new();
    events
        .iter()
        .filter(|event| seen.insert(event.event_id.to_string()))
        .map(|event| event.event_id)
        .take(kiana_domain::MAX_SOURCE_EVENT_IDS)
        .collect()
}

fn metric_value(snapshot: &MetricSnapshot, name: &str) -> u64 {
    snapshot
        .points
        .iter()
        .find(|point| point.name == name)
        .map(|point| point.value.max(0.0) as u64)
        .unwrap_or_default()
}

fn has_limitation(snapshot: &MetricSnapshot, value: &str) -> bool {
    snapshot
        .limitations
        .iter()
        .any(|limitation| limitation == value)
}

fn add_limitation(limitations: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !value.is_empty()
        && limitations.len() < MAX_LIMITATIONS
        && !limitations.iter().any(|item| item == &value)
    {
        limitations.push(value);
    }
}

fn component(
    name: &'static str,
    state: ComponentHealthState,
    cursor: Option<u64>,
    limitation: Option<&'static str>,
) -> Result<(String, ComponentHealth), HealthProjectionError> {
    let health = ComponentHealth::new(
        name,
        COMPONENT_VERSION,
        state,
        cursor,
        limitation.map(str::to_owned),
    )
    .map_err(HealthProjectionError::SnapshotInvalid)?;
    Ok((name.to_owned(), health))
}

fn readiness_capable(capabilities: &EventStoreCapabilities) -> bool {
    capabilities.atomic_transitions
        && capabilities.durable_commits
        && capabilities.command_receipts
        && capabilities.cursor_reads
}

fn status_for_probe(
    probe: HealthProbeKind,
    metrics: &MetricSnapshot,
    capabilities: &EventStoreCapabilities,
) -> SignalStatus {
    let cursor_gap = has_limitation(metrics, "eventlog_cursor_gap");
    let projector_inferred = has_limitation(metrics, "projector_cursor_inferred");
    let unknown = metric_value(metrics, "kiana.recovery.unknown_total") > 0;
    let orphan = metric_value(metrics, "kiana.recovery.orphan_total") > 0;
    match probe {
        HealthProbeKind::Readiness => {
            if readiness_capable(capabilities)
                && metrics.projector_cursor == metrics.source_cursor
                && !cursor_gap
                && !projector_inferred
                && !unknown
                && !orphan
            {
                SignalStatus::Ok
            } else {
                SignalStatus::Degraded
            }
        }
        HealthProbeKind::Liveness => {
            if cursor_gap {
                SignalStatus::Degraded
            } else {
                SignalStatus::Ok
            }
        }
        HealthProbeKind::Startup => {
            if readiness_capable(capabilities) && !cursor_gap {
                SignalStatus::Ok
            } else {
                SignalStatus::Degraded
            }
        }
        HealthProbeKind::Drain | HealthProbeKind::Maintenance => SignalStatus::Degraded,
    }
}

/// Build a health snapshot from a committed event view and immutable EventStore capabilities.
pub fn project_health_snapshot(
    events: &[RuntimeEvent],
    capabilities: &EventStoreCapabilities,
    probe: HealthProbeKind,
    observed_at_ms: u64,
) -> Result<HealthSnapshot, HealthProjectionError> {
    if events.is_empty() {
        return Err(HealthProjectionError::SourceEmpty);
    }
    if observed_at_ms == 0 {
        return Err(HealthProjectionError::ObservedAtInvalid);
    }
    let metrics = project_operational_metrics(events, None)
        .map_err(|error| HealthProjectionError::MetricsUnavailable(error.to_string()))?;
    let mut limitations = metrics.limitations.clone();
    if !capabilities.durable_commits {
        add_limitation(&mut limitations, "eventlog_durability_unavailable");
    }
    if !capabilities.atomic_transitions {
        add_limitation(&mut limitations, "eventlog_atomic_transitions_unavailable");
    }
    if metric_value(&metrics, "kiana.recovery.unknown_total") > 0 {
        add_limitation(&mut limitations, "effect_unknown_present");
    }
    if metric_value(&metrics, "kiana.recovery.orphan_total") > 0 {
        add_limitation(&mut limitations, "orphan_dispatch_present");
    }
    match probe {
        HealthProbeKind::Drain => add_limitation(&mut limitations, "draining_no_new_admission"),
        HealthProbeKind::Maintenance => {
            add_limitation(&mut limitations, "maintenance_no_new_admission")
        }
        _ => {}
    }

    let status = status_for_probe(probe, &metrics, capabilities);
    let source_cursor = metrics.source_cursor;
    let projector_cursor = metrics.projector_cursor;
    let mut components = BTreeMap::new();
    let eventlog_state =
        if readiness_capable(capabilities) && !has_limitation(&metrics, "eventlog_cursor_gap") {
            ComponentHealthState::Healthy
        } else if capabilities.cursor_reads {
            ComponentHealthState::Degraded
        } else {
            ComponentHealthState::Unavailable
        };
    let (eventlog_name, eventlog) = component(
        "eventlog",
        eventlog_state,
        Some(source_cursor),
        (eventlog_state != ComponentHealthState::Healthy)
            .then_some("eventlog_capability_or_cursor_limit"),
    )?;
    components.insert(eventlog_name, eventlog);
    let projector_state = if has_limitation(&metrics, "eventlog_cursor_gap")
        || projector_cursor < source_cursor
        || has_limitation(&metrics, "projector_cursor_inferred")
    {
        ComponentHealthState::Degraded
    } else {
        ComponentHealthState::Healthy
    };
    let projector_limitation = (projector_state != ComponentHealthState::Healthy)
        .then_some("projector_cursor_or_checkpoint_limit");
    let (projector_name, projector) = component(
        "projector",
        projector_state,
        Some(projector_cursor),
        projector_limitation,
    )?;
    components.insert(projector_name, projector);

    let receipt_failure = metric_value(&metrics, "kiana.receipts.query_failure_total") > 0;
    let (receipt_name, receipt) = component(
        "receipt",
        if receipt_failure {
            ComponentHealthState::Degraded
        } else {
            ComponentHealthState::Healthy
        },
        Some(source_cursor),
        receipt_failure.then_some("receipt_query_failure"),
    )?;
    components.insert(receipt_name, receipt);

    let artifact_failure = metric_value(&metrics, "kiana.artifacts.read_failure_total") > 0;
    let artifact_seen = events
        .iter()
        .any(|event| event.kind.contains("artifact") || event.data.get("artifact_id").is_some());
    let (artifact_name, artifact) = component(
        "artifact",
        if artifact_failure {
            ComponentHealthState::Degraded
        } else if artifact_seen {
            ComponentHealthState::Healthy
        } else {
            ComponentHealthState::Unknown
        },
        artifact_seen.then_some(source_cursor),
        if artifact_failure {
            Some("artifact_read_failure")
        } else if !artifact_seen {
            Some("artifact_not_observed")
        } else {
            None
        },
    )?;
    components.insert(artifact_name, artifact);

    let unknown_or_orphan = metric_value(&metrics, "kiana.recovery.unknown_total") > 0
        || metric_value(&metrics, "kiana.recovery.orphan_total") > 0;
    let (recovery_name, recovery) = component(
        "recovery",
        if unknown_or_orphan {
            ComponentHealthState::Degraded
        } else {
            ComponentHealthState::Healthy
        },
        Some(source_cursor),
        unknown_or_orphan.then_some("unknown_or_orphan_present"),
    )?;
    components.insert(recovery_name, recovery);

    for (name, reason) in [
        ("provider", "provider_health_not_observed"),
        ("broker", "broker_health_not_observed"),
        ("telemetry", "telemetry_exporter_health_not_observed"),
        ("daemon", "daemon_host_probe_not_observed"),
    ] {
        let (name, health) = component(name, ComponentHealthState::Unknown, None, Some(reason))?;
        components.insert(name, health);
    }

    let mut health = HealthSnapshot::new(
        "control_plane",
        status,
        source_cursor,
        bounded_source_ids(events),
        observed_at_ms,
    )
    .map_err(HealthProjectionError::SnapshotInvalid)?;
    health.probe = probe;
    health.capabilities.insert(
        "eventlog.atomic_transitions".to_owned(),
        capabilities.atomic_transitions,
    );
    health.capabilities.insert(
        "eventlog.durable_commits".to_owned(),
        capabilities.durable_commits,
    );
    health.capabilities.insert(
        "eventlog.command_receipts".to_owned(),
        capabilities.command_receipts,
    );
    health.capabilities.insert(
        "eventlog.cursor_reads".to_owned(),
        capabilities.cursor_reads,
    );
    health
        .capabilities
        .insert("projector.checkpoint_durable".to_owned(), false);
    health.components = components;
    health.limitations = limitations;
    health.snapshot_digest = health.digest();
    health
        .validate()
        .map_err(HealthProjectionError::SnapshotInvalid)?;
    Ok(health)
}

fn observed_at_ms() -> Result<u64, HealthProjectionError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HealthProjectionError::ObservedAtInvalid)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .and_then(|value| {
            (value > 0)
                .then_some(value)
                .ok_or(HealthProjectionError::ObservedAtInvalid)
        })
}

impl ControlPlane {
    /// Read a bounded health snapshot without invoking any external component.
    pub async fn health_snapshot(
        &self,
        probe: HealthProbeKind,
    ) -> Result<HealthSnapshot, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("health_read_all_unsupported".to_owned())
        })?;
        let capabilities = self.events.capabilities();
        let observed_at =
            observed_at_ms().map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        project_health_snapshot(&events, &capabilities, probe, observed_at)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }

    pub async fn readiness(&self) -> Result<HealthSnapshot, CoreError> {
        self.health_snapshot(HealthProbeKind::Readiness).await
    }

    pub async fn liveness(&self) -> Result<HealthSnapshot, CoreError> {
        self.health_snapshot(HealthProbeKind::Liveness).await
    }

    pub async fn startup_health(&self) -> Result<HealthSnapshot, CoreError> {
        self.health_snapshot(HealthProbeKind::Startup).await
    }
}
