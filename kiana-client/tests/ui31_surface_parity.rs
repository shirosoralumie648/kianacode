use kiana_client::{
    compare_surface_traces, ParitySurface, SurfaceParityError, SurfaceTrace, SURFACE_PARITY_SCHEMA,
};
use std::fs;
use std::path::Path;

fn trace(surface: ParitySurface) -> SurfaceTrace {
    SurfaceTrace {
        schema: SURFACE_PARITY_SCHEMA.to_owned(),
        surface,
        command_id: "command-31".to_owned(),
        operation: "run.prompt".to_owned(),
        disposition: "applied".to_owned(),
        retry: "query_original".to_owned(),
        cursor_epoch: "epoch-31".to_owned(),
        cursor_sequence: 31,
        revision: 4,
        receipt_digest: Some(format!("sha256:{}", "a".repeat(64))),
        error_code: None,
        sensitive_field_count: 0,
        limitations: vec!["browser_visual_not_proven".to_owned()],
    }
}

#[test]
fn fixture_declares_four_surface_parity_contract() {
    let value: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui31-parity.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.surface-parity-fixture.v1");
    assert_eq!(value["surfaces"].as_array().unwrap().len(), 4);
}

#[test]
fn same_server_trace_compares_across_all_surfaces() {
    let report = compare_surface_traces(&[
        trace(ParitySurface::Cli),
        trace(ParitySurface::Workbench),
        trace(ParitySurface::Web),
        trace(ParitySurface::Desktop),
    ])
    .unwrap();
    assert_eq!(report.command_id, "command-31");
    assert_eq!(report.surfaces.len(), 4);
    assert_eq!(report.limitations, vec!["browser_visual_not_proven"]);
}

#[test]
fn parity_rejects_drift_missing_surface_and_sensitive_fields() {
    let mut drift = trace(ParitySurface::Web);
    drift.retry = "safe_retry".to_owned();
    let error = compare_surface_traces(&[
        trace(ParitySurface::Cli),
        trace(ParitySurface::Workbench),
        drift,
        trace(ParitySurface::Desktop),
    ])
    .unwrap_err();
    assert_eq!(error, SurfaceParityError::Mismatch("retry"));

    let missing = compare_surface_traces(&[
        trace(ParitySurface::Cli),
        trace(ParitySurface::Workbench),
        trace(ParitySurface::Web),
    ])
    .unwrap_err();
    assert_eq!(missing, SurfaceParityError::SurfaceMissing);

    let mut secret = trace(ParitySurface::Desktop);
    secret.sensitive_field_count = 1;
    let secret_error = compare_surface_traces(&[
        trace(ParitySurface::Cli),
        trace(ParitySurface::Workbench),
        trace(ParitySurface::Web),
        secret,
    ])
    .unwrap_err();
    assert_eq!(secret_error, SurfaceParityError::SensitiveFields);
}
