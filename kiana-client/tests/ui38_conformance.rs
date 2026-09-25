use kiana_client::{
    compare_conformance, ConformanceError, ConformanceTrace, ParitySurface, UI_CONFORMANCE_SCHEMA,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn trace(surface: ParitySurface) -> ConformanceTrace {
    ConformanceTrace {
        schema: UI_CONFORMANCE_SCHEMA.to_owned(),
        surface,
        protocol_schema: "kiana.protocol.v1".to_owned(),
        ui_schema: "kiana.ui.v1".to_owned(),
        capability_schema: "kiana.ui-capability.v1".to_owned(),
        command_id: "command-38".to_owned(),
        cursor_epoch: "epoch-38".to_owned(),
        feed_sequence: 38,
        action_disposition: "unknown".to_owned(),
        retry: "query_original".to_owned(),
        artifact_digest: Some(format!("sha256:{}", "a".repeat(64))),
        receipt_digest: Some(format!("sha256:{}", "b".repeat(64))),
        unknown_visible: true,
        sensitive_field_count: 0,
    }
}

#[test]
fn fixture_declares_conformance_deny_first_contract() {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui38-conformance.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.ui-conformance-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("Unknown")));
}

#[test]
fn four_surface_unknown_trace_conforms_without_retry_or_effect() {
    let report = compare_conformance(&[
        trace(ParitySurface::Cli),
        trace(ParitySurface::Workbench),
        trace(ParitySurface::Web),
        trace(ParitySurface::Desktop),
    ])
    .unwrap();
    assert_eq!(report.action_disposition, "unknown");
    assert_eq!(report.retry, "query_original");
    assert!(report.unknown_visible);
}

#[test]
fn conformance_rejects_hidden_unknown_sensitive_and_schema_drift() {
    let mut hidden = trace(ParitySurface::Desktop);
    hidden.unknown_visible = false;
    assert_eq!(
        compare_conformance(&[
            trace(ParitySurface::Cli),
            trace(ParitySurface::Workbench),
            trace(ParitySurface::Web),
            hidden,
        ]),
        Err(ConformanceError::UnknownHidden)
    );
    let mut sensitive = trace(ParitySurface::Desktop);
    sensitive.sensitive_field_count = 1;
    assert_eq!(
        compare_conformance(&[
            trace(ParitySurface::Cli),
            trace(ParitySurface::Workbench),
            trace(ParitySurface::Web),
            sensitive,
        ]),
        Err(ConformanceError::SensitiveFields)
    );
    let mut drift = trace(ParitySurface::Web);
    drift.capability_schema = "kiana.ui-capability.v2".to_owned();
    assert_eq!(
        compare_conformance(&[
            trace(ParitySurface::Cli),
            trace(ParitySurface::Workbench),
            drift,
            trace(ParitySurface::Desktop),
        ]),
        Err(ConformanceError::Mismatch("capability_schema"))
    );
}
