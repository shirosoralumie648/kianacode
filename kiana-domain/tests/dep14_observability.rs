use kiana_domain::{
    schema_contract, LifecycleEvidenceBundle, LifecycleMetricFact, DEPLOYMENT_OBSERVABILITY_SCHEMA,
};

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn observability_bundle_binds_operation_revision_cursor_and_schema() {
    let contract = schema_contract(DEPLOYMENT_OBSERVABILITY_SCHEMA).unwrap();
    assert_eq!(contract.owner_crate, "kiana-domain");
    assert!(!contract.allow_unknown_fields);
    let metric =
        LifecycleMetricFact::new("kiana.operation.duration_ms", 42, "ms", "operation:1", D, 9)
            .unwrap();
    let bundle =
        LifecycleEvidenceBundle::new("operation:1", D, 9, "trace:1", D, D, vec![metric]).unwrap();
    bundle.validate().unwrap();
    assert_eq!(
        serde_json::from_value::<LifecycleEvidenceBundle>(serde_json::to_value(&bundle).unwrap())
            .unwrap(),
        bundle
    );
}

#[test]
fn observability_rejects_cross_operation_metrics_and_secret_markers() {
    let metric = LifecycleMetricFact::new(
        "kiana.operation.duration_ms",
        42,
        "ms",
        "operation:other",
        D,
        9,
    )
    .unwrap();
    assert_eq!(
        LifecycleEvidenceBundle::new("operation:1", D, 9, "trace:1", D, D, vec![metric])
            .unwrap_err(),
        "deployment_observability_metric_binding_invalid"
    );
    assert_eq!(
        LifecycleEvidenceBundle::new("operation:1", D, 9, "trace:secret=raw", D, D, Vec::new())
            .unwrap_err(),
        "deployment_observability_secret_marker"
    );
}

#[test]
fn observability_tamper_and_duplicate_metrics_fail_closed() {
    let metric =
        LifecycleMetricFact::new("kiana.operation.duration_ms", 42, "ms", "operation:1", D, 9)
            .unwrap();
    let duplicate = metric.clone();
    assert_eq!(
        LifecycleEvidenceBundle::new(
            "operation:1",
            D,
            9,
            "trace:1",
            D,
            D,
            vec![metric, duplicate]
        )
        .unwrap_err(),
        "deployment_observability_metric_binding_invalid"
    );
    let mut bundle =
        LifecycleEvidenceBundle::new("operation:1", D, 9, "trace:1", D, D, Vec::new()).unwrap();
    bundle.source_cursor = 10;
    assert_eq!(
        bundle.validate().unwrap_err(),
        "deployment_observability_bundle_digest_mismatch"
    );
}
