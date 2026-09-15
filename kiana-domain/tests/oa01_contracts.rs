use kiana_domain::{
    check_schema_compatibility, schema_contract, AuditActionKind, AuditDecision, AuditRecord,
    DataClass, EventId, MetricCatalog, MetricDefinition, MetricKind, MetricPoint, MetricSource,
    ObservabilityRecord, SchemaVersion, SignalKind, SignalStatus, TraceStatus, TraceSummary,
    AUDIT_RECORD_SCHEMA, METRIC_CATALOG_SCHEMA, OBSERVABILITY_SCHEMA, TRACE_SUMMARY_SCHEMA,
};
use std::collections::BTreeMap;

fn source_events() -> Vec<EventId> {
    vec![EventId::new()]
}

#[test]
fn registered_contracts_round_trip_and_minor_versions_are_compatible() {
    for schema in [
        OBSERVABILITY_SCHEMA,
        AUDIT_RECORD_SCHEMA,
        METRIC_CATALOG_SCHEMA,
        TRACE_SUMMARY_SCHEMA,
    ] {
        let contract = schema_contract(schema).expect("OA-01 schema registration");
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert_eq!(contract.version, SchemaVersion::new(1, 0));
        assert!(!contract.allow_unknown_fields);
        check_schema_compatibility(schema, &SchemaVersion::new(1, 1))
            .expect("minor additive version remains compatible");
        assert!(check_schema_compatibility(schema, &SchemaVersion::new(2, 0)).is_err());
    }

    let version = SchemaVersion::new(1, 2);
    let encoded = serde_json::to_string(&version).expect("schema version serialize");
    assert_eq!(
        serde_json::from_str::<SchemaVersion>(&encoded).expect("schema version deserialize"),
        version
    );
    assert!(
        serde_json::from_str::<SchemaVersion>(r#"{"major":1,"minor":0,"extra":true}"#).is_err()
    );
    assert!(serde_json::from_str::<SignalStatus>(r#""partial""#).is_err());
}

#[test]
fn observability_audit_and_trace_contracts_reject_empty_cursor_and_bad_digest() {
    let mut attributes = BTreeMap::new();
    attributes.insert("component".to_owned(), "core".to_owned());
    let record = ObservabilityRecord::new(
        "signal-1",
        SignalKind::Audit,
        SignalStatus::Unknown,
        "control-plane",
        1,
        source_events(),
        attributes,
    )
    .expect("valid observability record");
    assert_eq!(
        record,
        serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap()
    );
    assert_eq!(
        record.canonical_bytes().unwrap(),
        record.canonical_bytes().unwrap()
    );

    let mut empty_cursor = record.clone();
    empty_cursor.source_cursor = 0;
    assert_eq!(
        empty_cursor.validate().unwrap_err(),
        "observability_source_cursor_required"
    );
    let mut bad_digest = record.clone();
    bad_digest.payload_digest = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        bad_digest.validate().unwrap_err(),
        "observability_payload_digest_mismatch"
    );

    let audit = AuditRecord::new(
        "audit-1",
        AuditActionKind::Authorization,
        AuditDecision::Accepted,
        "principal:local-user",
        "run",
        "run-1",
        2,
        source_events(),
        1,
        1,
        DataClass::Internal,
        "audit",
    )
    .expect("valid audit record");
    assert!(audit.validate().is_ok());
    let mut bad_audit = audit.clone();
    bad_audit.record_digest = format!("sha256:{}", "f".repeat(64));
    assert_eq!(
        bad_audit.validate().unwrap_err(),
        "audit_record_digest_mismatch"
    );

    let trace = TraceSummary::new(
        "trace-1",
        Some("span-1".to_owned()),
        TraceStatus::Degraded,
        1,
        2,
        3,
        source_events(),
    )
    .expect("valid trace summary");
    assert!(trace.validate().is_ok());
    let mut bad_trace = trace.clone();
    bad_trace.summary_digest = format!("sha256:{}", "f".repeat(64));
    assert_eq!(
        bad_trace.validate().unwrap_err(),
        "trace_summary_digest_mismatch"
    );
    assert!(TraceSummary::new("trace-1", None, TraceStatus::Ok, 0, 0, 3, source_events()).is_err());
}

#[test]
fn metric_points_require_catalog_membership_and_bound_attributes() {
    let mut definition = MetricDefinition::counter("kiana.test.accepted_total", "count");
    definition.allowed_labels = vec!["component".to_owned()];
    let catalog = MetricCatalog::new(vec![definition]).expect("valid metric catalog");
    assert_eq!(catalog.schema, METRIC_CATALOG_SCHEMA);
    assert!(
        serde_json::from_str::<MetricCatalog>(&serde_json::to_string(&catalog).unwrap())
            .unwrap()
            .validate()
            .is_ok()
    );

    let mut labels = BTreeMap::new();
    labels.insert("component".to_owned(), "core".to_owned());
    let point = MetricPoint::new(
        "kiana.test.accepted_total",
        MetricKind::Counter,
        1.0,
        "count",
        labels,
        MetricSource::EventReducer,
        4,
        source_events(),
    )
    .expect("valid metric point");
    point.validate_with_catalog(&catalog).unwrap();

    let unknown = MetricPoint::new(
        "kiana.test.unknown_total",
        MetricKind::Counter,
        1.0,
        "count",
        BTreeMap::new(),
        MetricSource::EventReducer,
        5,
        source_events(),
    )
    .unwrap();
    assert_eq!(
        unknown.validate_with_catalog(&catalog).unwrap_err(),
        "metric_unregistered"
    );

    let mut too_long = BTreeMap::new();
    too_long.insert("component".to_owned(), "x".repeat(257));
    assert_eq!(
        ObservabilityRecord::new(
            "signal-2",
            SignalKind::Metric,
            SignalStatus::Ok,
            "core",
            1,
            source_events(),
            too_long,
        )
        .unwrap_err(),
        "observability_attribute_value_too_long"
    );
    assert!(MetricPoint::new(
        "kiana.test.accepted_total",
        MetricKind::Counter,
        1.0,
        "count",
        BTreeMap::new(),
        MetricSource::EventReducer,
        0,
        source_events(),
    )
    .is_err());
}
