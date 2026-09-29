//! PD-29 failure-first fixture for the storage diagnostic contract.
//!
//! One test per card rejection in
//! [`PD-29`](../../../docs/roadmap/persistence-data-layer.md#step-pd-29):
//! a diagnostic may never authorize, stale/unknown/corrupt may never render as healthy, and a
//! diagnostic may never emit a payload, a secret or a path.

use kiana_core::{
    evaluate_storage_diagnostics, storage_diagnostic_ui_view, validate_storage_diagnostic_report,
    validate_storage_diagnostic_ui_view, StorageCheckpointState, StorageDiagnosticInput,
    StorageDiagnosticNote, StorageMaintenanceCounters, StorageMaintenanceObservation,
    StorageMaintenanceSubject, StorageProjectionLag, StorageSignalChannel,
    STORAGE_DIAGNOSTIC_DISPLAY_ONLY, STORAGE_DIAGNOSTIC_INPUT_SCHEMA,
    STORAGE_DIAGNOSTIC_METRIC_CATALOG, STORAGE_DIAGNOSTIC_VERSION,
};
use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, SecretScanChannel, SignalStatus,
    StorageCapabilities, StorageHealth, StorageHealthStatus, StorageIntegrityIncident,
    StorageIntegrityIncidentClass, StoreIdentityId,
};

const NOW_MS: u64 = 5_000;
const SOURCE_CURSOR: u64 = 12;

fn store_id() -> StoreIdentityId {
    StoreIdentityId::new()
}

fn durable_capabilities() -> StorageCapabilities {
    StorageCapabilities::new(true, true, true, true, true, 4_096, 256).unwrap()
}

fn health(store_id: StoreIdentityId, status: StorageHealthStatus) -> StorageHealth {
    StorageHealth::new(
        store_id,
        status,
        durable_capabilities(),
        SOURCE_CURSOR,
        9,
        NOW_MS - 1,
        Vec::new(),
    )
    .unwrap()
}

fn maintenance(
    subject: StorageMaintenanceSubject,
    status: SignalStatus,
    reason: Option<&str>,
) -> StorageMaintenanceObservation {
    StorageMaintenanceObservation::new(
        subject,
        status,
        Some(NOW_MS - 100),
        None,
        StorageMaintenanceCounters::default(),
        reason.map(StorageDiagnosticNote::new).transpose().unwrap(),
    )
    .unwrap()
}

fn incident(
    store_id: StoreIdentityId,
    class: StorageIntegrityIncidentClass,
    resolved: bool,
) -> StorageIntegrityIncident {
    let mut incident =
        StorageIntegrityIncident::new(store_id, class, "torn_frame", NOW_MS - 200, Some(7))
            .unwrap();
    if resolved {
        incident.resolved_at_unix_ms = Some(NOW_MS - 10);
        incident.incident_digest = incident.digest();
    }
    incident
}

/// 【为什么需要显式传入 store_id】
/// `store_id()` 每调用一次都生成一个**全新的随机 id**。此前 `input()` 在内部自行调用
/// `store_id()`，于是测试里 `let store = store_id();` 之后拿去构造 incident 的那个 id，
/// 与 `input()` 内部生成的并不是同一个——`validate()` 随即以
/// `storage_diagnostic_input_incident_store_mismatch` 拒绝，而这条检查本身是对的：
/// 一份诊断里，incident 描述的 store 必须就是被诊断的那个 store。
/// 所以修的是夹具：把 store 显式串下去，而不是放宽那条一致性检查。
fn input_for(
    store_id: StoreIdentityId,
    health_status: StorageHealthStatus,
    projection: StorageProjectionLag,
    incidents: Vec<StorageIntegrityIncident>,
) -> StorageDiagnosticInput {
    StorageDiagnosticInput {
        schema: STORAGE_DIAGNOSTIC_INPUT_SCHEMA.to_owned(),
        version: STORAGE_DIAGNOSTIC_VERSION,
        store_id,
        health: health(store_id, health_status),
        projection,
        backup: maintenance(StorageMaintenanceSubject::Backup, SignalStatus::Ok, None),
        migration: maintenance(StorageMaintenanceSubject::Migration, SignalStatus::Ok, None),
        retention: maintenance(StorageMaintenanceSubject::Retention, SignalStatus::Ok, None),
        incidents,
        unknown_observations: 0,
        observed_at_unix_ms: NOW_MS,
        input_digest: String::new(),
    }
    .sealed()
    .unwrap()
}

/// 不需要绑定 incident 的用例直接用它，内部自取一个 store id。
fn input(
    health_status: StorageHealthStatus,
    projection: StorageProjectionLag,
    incidents: Vec<StorageIntegrityIncident>,
) -> StorageDiagnosticInput {
    input_for(store_id(), health_status, projection, incidents)
}

fn caught_up() -> StorageProjectionLag {
    StorageProjectionLag::new(
        "authoritative_read_model",
        SOURCE_CURSOR,
        SOURCE_CURSOR,
        3,
        7,
        StorageCheckpointState::CaughtUp,
    )
    .unwrap()
}

fn lagging() -> StorageProjectionLag {
    StorageProjectionLag::new(
        "authoritative_read_model",
        SOURCE_CURSOR,
        SOURCE_CURSOR - 4,
        3,
        7,
        StorageCheckpointState::Lagging,
    )
    .unwrap()
}

// ---------------------------------------------------------------- healthy baseline

#[test]
fn a_fully_observed_healthy_store_renders_ok_and_still_authorizes_nothing() {
    let input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    let report = evaluate_storage_diagnostics(&input).unwrap();
    assert_eq!(report.display_status, SignalStatus::Ok);
    assert!(report.limitations.is_empty());
    validate_storage_diagnostic_report(&input, &report).unwrap();

    // The whole point of the card: a green report still grants nothing.
    const { assert!(STORAGE_DIAGNOSTIC_DISPLAY_ONLY) };
    assert!(report.display_only);
    assert!(!report.authorizes());
    assert!(!report.view.authorizes);

    // Every catalog signal is present and bound to its channel.
    assert_eq!(
        report.signals.len(),
        STORAGE_DIAGNOSTIC_METRIC_CATALOG.len()
    );
    let channels = report
        .signals
        .iter()
        .map(|signal| signal.channel)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(channels.contains(&StorageSignalChannel::Logs));
    assert!(channels.contains(&StorageSignalChannel::Traces));
    assert!(channels.contains(&StorageSignalChannel::Metrics));
}

#[test]
fn the_ui_view_carries_cursor_generation_epochs_and_declared_limits() {
    let input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    let report = evaluate_storage_diagnostics(&input).unwrap();
    let view = storage_diagnostic_ui_view(&report).unwrap();
    validate_storage_diagnostic_ui_view(&view).unwrap();
    assert_eq!(view.source_cursor, SOURCE_CURSOR);
    assert_eq!(view.projection_cursor, SOURCE_CURSOR);
    assert_eq!(view.projection_lag, 0);
    assert_eq!(view.projection_generation, 3);
    assert_eq!(view.data_epoch, 7);
    assert_eq!(view.authority_epoch, 9);
    assert!(view.durable_commits);
    assert_eq!(view.max_frame_bytes, 4_096);
    assert_eq!(view.max_batch_events, 256);
    assert!(!view.authorizes);
}

// ------------------------------------------------- rejection 1: health never authorizes

#[test]
fn a_report_cannot_claim_to_authorize() {
    let input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    let mut report = evaluate_storage_diagnostics(&input).unwrap();
    report.display_only = false;
    report.report_digest = report.digest();
    assert_eq!(
        report.validate().unwrap_err(),
        "storage_diagnostics_health_is_display_only"
    );
}

#[test]
fn a_ui_view_cannot_claim_to_authorize() {
    let input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    let report = evaluate_storage_diagnostics(&input).unwrap();
    let mut view = storage_diagnostic_ui_view(&report).unwrap();
    view.authorizes = true;
    view.view_digest = view.digest();
    assert_eq!(
        view.validate().unwrap_err(),
        "storage_diagnostics_view_authorizes"
    );
}

#[test]
fn an_authorized_view_cannot_be_rebound_to_a_report() {
    let input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    let mut report = evaluate_storage_diagnostics(&input).unwrap();
    report.view.authorizes = true;
    report.view.view_digest = report.view.digest();
    report.report_digest = report.digest();
    assert_eq!(
        report.validate().unwrap_err(),
        "storage_diagnostics_view_authorizes"
    );
}

// ------------------------------------- rejection 2: stale / unknown / corrupt ≠ healthy

#[test]
fn a_lagging_projection_is_never_rendered_healthy() {
    let input = input(StorageHealthStatus::Ready, lagging(), Vec::new());
    let report = evaluate_storage_diagnostics(&input).unwrap();
    assert_eq!(report.display_status, SignalStatus::Degraded);
    assert_eq!(report.view.projection_lag, 4);
    assert!(report
        .limitations
        .iter()
        .any(|note| note.text == "projection_lag_present"));
    let lag = report
        .signals
        .iter()
        .find(|signal| signal.name == "kiana.storage.projection_lag_events")
        .unwrap();
    assert_eq!(lag.value, 4);
    assert_eq!(lag.status, SignalStatus::Degraded);
}

#[test]
fn a_checkpoint_state_that_contradicts_the_cursor_is_rejected() {
    // Claiming "caught up" while the projection cursor is behind would let a UI show no lag.
    let mut projection = caught_up();
    projection.projection_cursor = SOURCE_CURSOR - 2;
    projection.lag_digest = projection.digest();
    assert_eq!(
        projection.validate().unwrap_err(),
        "storage_diagnostic_checkpoint_state_contradicts_cursor"
    );
    // And the mirror image: "lagging" with nothing outstanding.
    let mut projection = caught_up();
    projection.checkpoint_state = StorageCheckpointState::Lagging;
    projection.lag_digest = projection.digest();
    assert_eq!(
        projection.validate().unwrap_err(),
        "storage_diagnostic_checkpoint_state_contradicts_cursor"
    );
}

#[test]
fn an_unknown_checkpoint_never_renders_healthy() {
    let input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    let mut projection = input.projection.clone();
    projection.checkpoint_state = StorageCheckpointState::Unknown;
    projection.lag_digest = projection.digest();
    projection.validate().unwrap();
    let mut unknown_input = input;
    unknown_input.projection = projection;
    let report = evaluate_storage_diagnostics(&unknown_input.sealed().unwrap()).unwrap();
    assert_eq!(report.display_status, SignalStatus::Degraded);
    assert!(report
        .limitations
        .iter()
        .any(|note| note.text == "projection_checkpoint_not_caught_up"));
}

#[test]
fn an_unknown_store_status_is_never_rendered_healthy() {
    let input = input(StorageHealthStatus::Unknown, caught_up(), Vec::new());
    let report = evaluate_storage_diagnostics(&input).unwrap();
    assert_eq!(report.store_status, SignalStatus::Unknown);
    assert_eq!(report.display_status, SignalStatus::Unknown);
    assert!(report
        .limitations
        .iter()
        .any(|note| note.text == "storage_health_unknown"));
}

#[test]
fn an_open_unknown_incident_is_never_rendered_healthy() {
    let store = store_id();
    let input = input_for(
        store,
        StorageHealthStatus::Ready,
        caught_up(),
        vec![incident(
            store,
            StorageIntegrityIncidentClass::ResultUnknown,
            false,
        )],
    );
    let report = evaluate_storage_diagnostics(&input).unwrap();
    assert_eq!(report.display_status, SignalStatus::Unknown);
    assert!(report
        .limitations
        .iter()
        .any(|note| note.text == "integrity_incident_unknown_open"));
    let open = report
        .signals
        .iter()
        .find(|signal| signal.name == "kiana.storage.integrity_incidents_open")
        .unwrap();
    assert_eq!(open.value, 1);
    assert_eq!(open.status, SignalStatus::Unknown);
}

#[test]
fn a_corrupt_store_or_corrupt_incident_is_reported_as_error() {
    let store = store_id();
    let corrupt_store = input_for(store, StorageHealthStatus::Corrupt, caught_up(), Vec::new());
    let report = evaluate_storage_diagnostics(&corrupt_store).unwrap();
    assert_eq!(report.display_status, SignalStatus::Error);
    assert!(report
        .limitations
        .iter()
        .any(|note| note.text == "storage_health_corrupt"));

    // A store that claims `Ready` while a corrupt frame is open is still an error.
    let corrupt_incident = input_for(
        store,
        StorageHealthStatus::Ready,
        caught_up(),
        vec![incident(
            store,
            StorageIntegrityIncidentClass::Corrupt,
            false,
        )],
    );
    let report = evaluate_storage_diagnostics(&corrupt_incident).unwrap();
    assert_eq!(report.store_status, SignalStatus::Error);
    assert_eq!(report.display_status, SignalStatus::Error);
}

#[test]
fn a_resolved_incident_does_not_keep_degrading_the_store() {
    let store = store_id();
    let input = input_for(
        store,
        StorageHealthStatus::Ready,
        caught_up(),
        vec![incident(
            store,
            StorageIntegrityIncidentClass::Corrupt,
            true,
        )],
    );
    let report = evaluate_storage_diagnostics(&input).unwrap();
    assert_eq!(report.display_status, SignalStatus::Ok);
    let open = report
        .signals
        .iter()
        .find(|signal| signal.name == "kiana.storage.integrity_incidents_open")
        .unwrap();
    assert_eq!(open.value, 0);
}

#[test]
fn an_aggregate_cannot_be_healthier_than_its_worst_component() {
    // The store is lagging, so several signals are Degraded. Flipping both the store and the
    // aggregate to Ok must be refused even with a freshly re-sealed digest.
    let input = input(StorageHealthStatus::Ready, lagging(), Vec::new());
    let mut report = evaluate_storage_diagnostics(&input).unwrap();
    assert_eq!(report.display_status, SignalStatus::Degraded);
    report.store_status = SignalStatus::Ok;
    report.display_status = SignalStatus::Ok;
    report.view.display_status = SignalStatus::Ok;
    report.view.view_digest = report.view.digest();
    report.report_digest = report.digest();
    assert_eq!(
        report.validate().unwrap_err(),
        "storage_diagnostics_unhealthy_reported_healthy"
    );
}

#[test]
fn ok_with_a_limitation_is_not_representable() {
    // Start from an unhealthy store, then force the whole report green while keeping a caveat.
    let input = input(StorageHealthStatus::Ready, lagging(), Vec::new());
    let mut report = evaluate_storage_diagnostics(&input).unwrap();
    report.store_status = SignalStatus::Ok;
    report.display_status = SignalStatus::Ok;
    report.signals = report
        .signals
        .iter()
        .map(|signal| {
            let mut signal = signal.clone();
            signal.status = SignalStatus::Ok;
            signal.reason = None;
            signal.signal_digest = signal.digest();
            signal
        })
        .collect();
    report
        .limitations
        .push(StorageDiagnosticNote::new("storage_health_degraded").unwrap());
    report.view.display_status = SignalStatus::Ok;
    report.view.limitations = vec!["storage_health_degraded".to_owned()];
    report.view.view_digest = report.view.digest();
    report.report_digest = report.digest();
    assert_eq!(
        report.validate().unwrap_err(),
        "storage_diagnostics_unhealthy_reported_healthy"
    );
}

#[test]
fn a_non_durable_adapter_is_never_reported_ok() {
    let store = store_id();
    let mut input = input_for(store, StorageHealthStatus::Ready, caught_up(), Vec::new());
    input.health = StorageHealth::new(
        store,
        StorageHealthStatus::Ready,
        StorageCapabilities::new(false, false, true, true, false, 4_096, 256).unwrap(),
        SOURCE_CURSOR,
        9,
        NOW_MS - 1,
        Vec::new(),
    )
    .unwrap();
    let input = input.sealed().unwrap();
    let report = evaluate_storage_diagnostics(&input).unwrap();
    assert_eq!(report.display_status, SignalStatus::Degraded);
    assert!(report
        .limitations
        .iter()
        .any(|note| note.text == "storage_durability_unavailable"));
    assert!(!report.view.durable_commits);
}

// -------------------------------------- rejection 3: no payload, no secret, no path

#[test]
fn a_diagnostic_note_refuses_a_serialized_payload() {
    for payload in [r#"{"command":"rm -rf /"}"#, r#"["a","b"]"#, r#""quoted""#] {
        assert_eq!(
            StorageDiagnosticNote::new(payload).unwrap_err(),
            "storage_diagnostic_note_payload",
            "payload leaked into a diagnostic note: {payload}"
        );
    }
}

#[test]
fn a_diagnostic_note_refuses_a_secret() {
    for secret in [
        "authorization: bearer abcdef",
        "api_key: live-9f8e7d",
        "token=abc123",
        "password=hunter2",
    ] {
        assert_eq!(
            StorageDiagnosticNote::new(secret).unwrap_err(),
            "storage_diagnostic_note_secret",
            "secret leaked into a diagnostic note: {secret}"
        );
    }
}

#[test]
fn a_diagnostic_note_refuses_a_path() {
    for path in [
        "/home/user/project/facts.jsonl",
        "..\\state\\store",
        "~/facts",
        "file:///tmp/store",
        "backups/set-1",
    ] {
        assert_eq!(
            StorageDiagnosticNote::new(path).unwrap_err(),
            "storage_diagnostic_note_path",
            "path leaked into a diagnostic note: {path}"
        );
    }
}

#[test]
fn a_projector_id_is_held_to_the_same_redaction_rule() {
    assert!(StorageDiagnosticNote::new("authoritative_read_model").is_ok());
    assert_eq!(
        StorageProjectionLag::new(
            "/var/lib/kiana/projector",
            SOURCE_CURSOR,
            SOURCE_CURSOR,
            1,
            1,
            StorageCheckpointState::CaughtUp,
        )
        .unwrap_err(),
        "storage_diagnostic_note_path"
    );
    assert_eq!(
        StorageProjectionLag::new(
            "api_key: leak",
            SOURCE_CURSOR,
            SOURCE_CURSOR,
            1,
            1,
            StorageCheckpointState::CaughtUp,
        )
        .unwrap_err(),
        "storage_diagnostic_note_secret"
    );
}

#[test]
fn adapter_incident_code_never_reaches_the_diagnostic_output() {
    // The incident `code` is adapter-supplied text. Only counts and classes may cross.
    let store = store_id();
    let incident = StorageIntegrityIncident::new(
        store,
        StorageIntegrityIncidentClass::Corrupt,
        "api_key: leaked-into-code",
        NOW_MS - 200,
        Some(7),
    )
    .unwrap();
    let input = input_for(
        store,
        StorageHealthStatus::Ready,
        caught_up(),
        vec![incident],
    );
    let report = evaluate_storage_diagnostics(&input).unwrap();
    let rendered = serde_json::to_string(&report).unwrap();
    assert!(!rendered.contains("leaked-into-code"), "{rendered}");
    for limitation in &report.limitations {
        assert!(redact_text(&limitation.text) == limitation.text);
        assert!(scan_secret_sentinels(SecretScanChannel::Receipt, &limitation.text).is_ok());
    }
}

#[test]
fn a_metric_from_the_fact_ledger_is_refused_on_the_observability_channels() {
    for name in [
        "kiana.fact.committed_total",
        "kiana.receipt.delivered_total",
        "eventlog.commits",
    ] {
        let signal = kiana_core::StorageDiagnosticSignal::new(
            StorageSignalChannel::Metrics,
            name,
            1,
            SignalStatus::Ok,
            None,
        );
        assert_eq!(
            signal.unwrap_err(),
            "storage_diagnostics_fact_signal_rejected",
            "fact metric crossed the observability boundary: {name}"
        );
    }
}

#[test]
fn an_uncatalogued_metric_and_a_channel_swap_are_refused() {
    assert_eq!(
        kiana_core::StorageDiagnosticSignal::new(
            StorageSignalChannel::Metrics,
            "kiana.storage.made_up_gauge",
            1,
            SignalStatus::Ok,
            None,
        )
        .unwrap_err(),
        "storage_diagnostics_metric_not_in_catalog"
    );
    assert_eq!(
        kiana_core::StorageDiagnosticSignal::new(
            StorageSignalChannel::Logs,
            "kiana.storage.projection_lag_events",
            1,
            SignalStatus::Ok,
            None,
        )
        .unwrap_err(),
        "storage_diagnostics_channel_mismatch"
    );
}

// ---------------------------------------------- maintenance subjects are not authority

#[test]
fn a_never_verified_backup_is_never_reported_ok() {
    let never = StorageMaintenanceObservation::new(
        StorageMaintenanceSubject::Backup,
        SignalStatus::Ok,
        None,
        None,
        StorageMaintenanceCounters::default(),
        None,
    )
    .unwrap_err();
    assert_eq!(never, "storage_diagnostic_maintenance_not_observed");

    let mut input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    input.backup = StorageMaintenanceObservation::new(
        StorageMaintenanceSubject::Backup,
        SignalStatus::Unknown,
        None,
        None,
        StorageMaintenanceCounters::default(),
        Some(StorageDiagnosticNote::new("backup_never_verified").unwrap()),
    )
    .unwrap();
    let input = input.sealed().unwrap();
    let report = evaluate_storage_diagnostics(&input).unwrap();
    assert_eq!(report.display_status, SignalStatus::Unknown);
    let never = report
        .signals
        .iter()
        .find(|signal| signal.name == "kiana.storage.backup_never_verified")
        .unwrap();
    assert_eq!(never.value, 1);
    assert!(report
        .limitations
        .iter()
        .any(|note| note.text == "backup_never_verified"));
}

#[test]
fn an_ok_maintenance_status_with_a_newer_failure_is_rejected() {
    let error = StorageMaintenanceObservation::new(
        StorageMaintenanceSubject::Retention,
        SignalStatus::Ok,
        Some(1_000),
        Some(2_000),
        StorageMaintenanceCounters::default(),
        None,
    )
    .unwrap_err();
    assert_eq!(error, "storage_diagnostic_maintenance_stale_status");
}

#[test]
fn a_degraded_maintenance_subject_always_carries_a_reason() {
    let error = StorageMaintenanceObservation::new(
        StorageMaintenanceSubject::Migration,
        SignalStatus::Degraded,
        Some(1_000),
        None,
        StorageMaintenanceCounters::default(),
        None,
    )
    .unwrap_err();
    assert_eq!(error, "storage_diagnostic_maintenance_reason_required");
}

#[test]
fn a_future_dated_backup_observation_is_rejected_against_the_input_clock() {
    let mut input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    input.backup = StorageMaintenanceObservation::new(
        StorageMaintenanceSubject::Backup,
        SignalStatus::Ok,
        Some(NOW_MS + 1_000),
        None,
        StorageMaintenanceCounters::default(),
        None,
    )
    .unwrap();
    let sealed = input.sealed();
    assert_eq!(
        sealed.unwrap_err(),
        "storage_diagnostic_maintenance_header_invalid"
    );
}

#[test]
fn a_blocked_retention_sweep_surfaces_as_a_bounded_counter() {
    let mut input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    input.retention = StorageMaintenanceObservation::new(
        StorageMaintenanceSubject::Retention,
        SignalStatus::Degraded,
        Some(1_000),
        None,
        StorageMaintenanceCounters {
            pending: 1,
            failed: 2,
            blocked: 3,
        },
        Some(StorageDiagnosticNote::new("legal_hold_active").unwrap()),
    )
    .unwrap();
    let input = input.sealed().unwrap();
    let report = evaluate_storage_diagnostics(&input).unwrap();
    assert_eq!(report.display_status, SignalStatus::Degraded);
    for (name, expected) in [
        ("kiana.storage.retention_blocked_total", 3),
        ("kiana.storage.retention_failed_total", 2),
        ("kiana.storage.retention_sweep_incomplete", 1),
    ] {
        let metric = report
            .signals
            .iter()
            .find(|signal| signal.name == name)
            .unwrap();
        assert_eq!(metric.value, expected, "{name}");
    }
    assert!(report
        .limitations
        .iter()
        .any(|note| note.text == "legal_hold_active"));
}

// ------------------------------------------------------------- input binding fences

#[test]
fn an_evidence_set_from_two_different_cursors_is_rejected() {
    let mut input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    input.projection = StorageProjectionLag::new(
        "authoritative_read_model",
        SOURCE_CURSOR + 5,
        SOURCE_CURSOR + 5,
        3,
        7,
        StorageCheckpointState::CaughtUp,
    )
    .unwrap();
    assert_eq!(
        input.sealed().unwrap_err(),
        "storage_diagnostic_input_cursor_mismatch"
    );
}

#[test]
fn a_projection_cursor_ahead_of_the_facts_is_rejected() {
    let error = StorageProjectionLag::new(
        "authoritative_read_model",
        SOURCE_CURSOR,
        SOURCE_CURSOR + 1,
        3,
        7,
        StorageCheckpointState::CaughtUp,
    )
    .unwrap_err();
    assert_eq!(error, "storage_diagnostics_projection_cursor_ahead");
}

#[test]
fn a_duplicate_incident_is_rejected() {
    let store = store_id();
    let mut input = input_for(store, StorageHealthStatus::Ready, caught_up(), Vec::new());
    let incident = StorageIntegrityIncident::new(
        store,
        StorageIntegrityIncidentClass::Unknown,
        "torn_frame",
        NOW_MS - 200,
        Some(7),
    )
    .unwrap();
    input.incidents = vec![incident.clone(), incident];
    assert_eq!(
        input.sealed().unwrap_err(),
        "storage_diagnostic_input_incident_duplicate"
    );
}

#[test]
fn an_unsealed_or_tampered_input_is_rejected() {
    let mut input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    input.unknown_observations = 4;
    assert_eq!(
        input.validate().unwrap_err(),
        "storage_diagnostic_input_digest_mismatch"
    );
    assert_eq!(
        evaluate_storage_diagnostics(&input).unwrap_err(),
        "storage_diagnostic_input_digest_mismatch"
    );
}

#[test]
fn a_report_rebuilt_from_different_evidence_is_rejected() {
    let healthy = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    let mut report = evaluate_storage_diagnostics(&healthy).unwrap();
    // Swap in a different input digest without re-deriving the report.
    report.input_digest = json_digest(&serde_json::json!({"other": true}));
    report.report_digest = report.digest();
    assert_eq!(
        report.validate_against(&healthy).unwrap_err(),
        "storage_diagnostics_report_binding_mismatch"
    );
}

#[test]
fn the_diagnostic_dto_round_trips_without_gaining_authority() {
    let input = input(StorageHealthStatus::Ready, caught_up(), Vec::new());
    let report = evaluate_storage_diagnostics(&input).unwrap();
    let encoded = serde_json::to_string(&report).unwrap();
    let decoded: kiana_core::StorageDiagnosticReport = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, report);
    decoded.validate_against(&input).unwrap();
    assert!(!decoded.authorizes());
}
