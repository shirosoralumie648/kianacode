use kiana_client::{
    compare_surface_traces, ParitySurface, SurfaceParityError, SurfaceTrace, SURFACE_PARITY_SCHEMA,
};

fn trace(surface: ParitySurface) -> SurfaceTrace {
    SurfaceTrace {
        schema: SURFACE_PARITY_SCHEMA.to_owned(),
        surface,
        command_id: "p4-j7-30-run".to_owned(),
        operation: "provider.coding_round_trip".to_owned(),
        disposition: "applied".to_owned(),
        retry: "query_original".to_owned(),
        cursor_epoch: "provider-epoch-30".to_owned(),
        cursor_sequence: 30,
        revision: 1,
        receipt_digest: Some(format!("sha256:{}", "c".repeat(64))),
        error_code: None,
        sensitive_field_count: 0,
        limitations: vec!["loopback_only".to_owned(), "live_unverified".to_owned()],
    }
}

#[test]
fn four_surfaces_agree_on_one_server_owned_terminal() {
    let report = compare_surface_traces(&[
        trace(ParitySurface::Cli),
        trace(ParitySurface::Workbench),
        trace(ParitySurface::Web),
        trace(ParitySurface::Desktop),
    ])
    .expect("four-surface parity");
    assert_eq!(report.command_id, "p4-j7-30-run");
    assert_eq!(
        report.receipt_digest,
        trace(ParitySurface::Cli).receipt_digest
    );
    assert_eq!(report.surfaces.len(), 4);
    assert!(report.limitations.contains(&"loopback_only".to_owned()));
}

#[test]
fn surface_parity_rejects_terminal_drift_and_sensitive_observation() {
    let mut drift = trace(ParitySurface::Web);
    drift.receipt_digest = Some(format!("sha256:{}", "d".repeat(64)));
    assert_eq!(
        compare_surface_traces(&[
            trace(ParitySurface::Cli),
            trace(ParitySurface::Workbench),
            drift,
            trace(ParitySurface::Desktop),
        ])
        .unwrap_err(),
        SurfaceParityError::Mismatch("receipt")
    );

    let mut secret = trace(ParitySurface::Desktop);
    secret.sensitive_field_count = 1;
    assert_eq!(
        compare_surface_traces(&[
            trace(ParitySurface::Cli),
            trace(ParitySurface::Workbench),
            trace(ParitySurface::Web),
            secret,
        ])
        .unwrap_err(),
        SurfaceParityError::SensitiveFields
    );
}

#[test]
fn late_terminal_is_query_original_not_a_second_model_request() {
    let mut late = trace(ParitySurface::Desktop);
    late.retry = "query_original".to_owned();
    late.cursor_sequence = 31;
    assert_eq!(
        compare_surface_traces(&[
            trace(ParitySurface::Cli),
            trace(ParitySurface::Workbench),
            trace(ParitySurface::Web),
            late,
        ])
        .unwrap_err(),
        SurfaceParityError::Mismatch("cursor")
    );
}
