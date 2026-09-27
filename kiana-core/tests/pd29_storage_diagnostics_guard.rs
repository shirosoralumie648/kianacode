//! PD-29 source guard: the storage diagnostic contract displays and never authorizes.
//!
//! The card's three rejections are asserted against the real source text, not against behaviour:
//! a health report must not be an authority, stale/unknown/corrupt must not be renderable as
//! healthy, and the output must stay redacted with logs/metrics/traces separate from facts.

#[test]
fn pd29_diagnostic_never_becomes_an_authority() {
    let source = include_str!("../src/storage_diagnostics.rs");
    for marker in [
        "pub const STORAGE_DIAGNOSTIC_DISPLAY_ONLY: bool = true;",
        "pub const fn authorizes(&self) -> bool",
        "display_only: true",
        "storage_diagnostics_health_is_display_only",
        "storage_diagnostics_view_authorizes",
        "authorizes: false",
    ] {
        assert!(source.contains(marker), "PD-29 marker missing: {marker}");
    }
    // The refusal must be structural: the accessor is a `const fn` that can only return `false`,
    // and no parameter lets a caller talk it into authorizing.
    let authorizes = source
        .find("pub const fn authorizes(&self) -> bool")
        .expect("authorizes accessor");
    let body = &source[authorizes..authorizes + 120];
    assert!(
        body.contains("false"),
        "PD-29 authorizes must be hard-wired to false: {body}"
    );
    // A diagnostic path may not acquire a permit, a gate or a broker in the first place.
    for forbidden in [
        "admission_allowed",
        "permit",
        "CapabilityBroker",
        "GateEngine",
        "ControlPlane",
    ] {
        assert!(
            !source.contains(forbidden),
            "PD-29 diagnostic crossed an authority boundary: {forbidden}"
        );
    }
}

#[test]
fn pd29_stale_unknown_and_corrupt_cannot_render_as_healthy() {
    let source = include_str!("../src/storage_diagnostics.rs");
    for marker in [
        "storage_diagnostics_unhealthy_reported_healthy",
        "storage_diagnostic_checkpoint_state_contradicts_cursor",
        "storage_diagnostics_projection_cursor_ahead",
        "storage_diagnostics_report_binding_mismatch",
        "StorageCheckpointState::Unknown",
        "StorageHealthStatus::Unknown",
        "StorageHealthStatus::Corrupt",
        "StorageIntegrityIncidentClass::ResultUnknown",
        "worst_status(",
        "if self.display_status == SignalStatus::Ok && !self.limitations.is_empty()",
    ] {
        assert!(source.contains(marker), "PD-29 marker missing: {marker}");
    }
    // The decision order is fixed and written down, corrupt first.
    let order = source
        .find("fn store_status(input: &StorageDiagnosticInput) -> SignalStatus")
        .expect("store_status");
    let body = &source[order..];
    let corrupt = body
        .find("StorageHealthStatus::Corrupt")
        .expect("corrupt branch");
    let unavailable = body
        .find("StorageHealthStatus::Unavailable")
        .expect("unavailable branch");
    let unknown = body
        .find("StorageHealthStatus::Unknown")
        .expect("unknown branch");
    assert!(
        corrupt < unavailable && unavailable < unknown,
        "PD-29 decision order must be corrupt, then unavailable, then unknown"
    );
}

#[test]
fn pd29_diagnostic_output_is_redacted_and_carries_no_payload_or_path() {
    let source = include_str!("../src/storage_diagnostics.rs");
    for marker in [
        "redact_text",
        "scan_secret_sentinels(SecretScanChannel::Receipt, text).is_err()",
        "storage_diagnostic_note_payload",
        "storage_diagnostic_note_secret",
        "storage_diagnostic_note_path",
        "MAX_DIAGNOSTIC_NOTE_BYTES",
    ] {
        assert!(source.contains(marker), "PD-29 marker missing: {marker}");
    }
    // The incident `code` is adapter-supplied text. Only counts and classes may cross, so the
    // diagnostic must never copy an incident code into a limitation or a signal.
    for field in [".code", "incident.code"] {
        assert!(
            !source.contains(field),
            "PD-29 must not copy adapter incident text into the diagnostic: {field}"
        );
    }
}

#[test]
fn pd29_logs_metrics_and_traces_stay_separate_from_facts() {
    let source = include_str!("../src/storage_diagnostics.rs");
    for marker in [
        "pub enum StorageSignalChannel",
        "StorageSignalChannel::Logs",
        "StorageSignalChannel::Metrics",
        "StorageSignalChannel::Traces",
        "STORAGE_DIAGNOSTIC_METRIC_CATALOG",
        "STORAGE_DIAGNOSTIC_FACT_RESERVED_PREFIXES",
        "storage_diagnostics_fact_signal_rejected",
        "storage_diagnostics_metric_not_in_catalog",
        "storage_diagnostics_channel_mismatch",
    ] {
        assert!(source.contains(marker), "PD-29 marker missing: {marker}");
    }
    // The catalog is closed and the channel is part of the catalog entry, so a name cannot be
    // quietly relocated onto a different observability channel.
    let entry = source
        .find("pub const STORAGE_DIAGNOSTIC_METRIC_CATALOG")
        .expect("catalog");
    let signature = &source[entry..source.len().min(entry + 220)];
    assert!(
        signature.contains("StorageMetricUnit, StorageSignalChannel"),
        "PD-29 catalog must bind unit and channel per name: {signature}"
    );
}

#[test]
fn pd29_ui_view_exposes_cursor_generation_and_limits() {
    let source = include_str!("../src/storage_diagnostics.rs");
    for marker in [
        "pub struct StorageDiagnosticUiView",
        "pub projection_lag: u64",
        "pub projection_generation: u64",
        "pub data_epoch: u64",
        "pub authority_epoch: u64",
        "pub max_frame_bytes: u64",
        "pub max_batch_events: u64",
        "pub fn storage_diagnostic_ui_view(",
        "pub fn validate_storage_diagnostic_ui_view(",
    ] {
        assert!(source.contains(marker), "PD-29 marker missing: {marker}");
    }
}

#[test]
fn pd29_module_stays_a_pure_reducer_and_is_exported() {
    let source = include_str!("../src/storage_diagnostics.rs");
    let lib = include_str!("../src/lib.rs");
    assert!(lib.contains("mod storage_diagnostics;"));
    assert!(lib.contains("pub use storage_diagnostics::{"));
    for marker in [
        "pub fn evaluate_storage_diagnostics(",
        "pub fn validate_storage_diagnostic_report(",
        "pub fn validate_against(&self, input: &StorageDiagnosticInput)",
        "pub fn sealed(mut self) -> Result<Self, String>",
        "pub fn evaluate(input: &StorageDiagnosticInput) -> Result<Self, String>",
    ] {
        assert!(source.contains(marker), "PD-29 marker missing: {marker}");
    }
    // Effect-free: the diagnostic opens nothing, reads no byte and schedules no maintenance.
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "PathBuf",
        "read_to_string",
        "tokio::",
        "EventStore",
    ] {
        assert!(
            !source.contains(forbidden),
            "PD-29 diagnostic crossed an effect boundary: {forbidden}"
        );
    }
}

#[test]
fn pd29_baseline_states_the_proof_ceiling() {
    let baseline = include_str!("../../docs/roadmap/pd29-storage-diagnostics-baseline.md");
    for marker in ["source", "does not", "PD-29", "display", "authoriz"] {
        assert!(
            baseline.contains(marker),
            "PD-29 baseline missing: {marker}"
        );
    }
}
