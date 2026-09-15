use kiana_domain::{
    check_event_schema_version, event_kind_spec, event_migration, unknown_event_policy,
    validate_event_payload, validate_runtime_event, EventKindSpec, RequestId, RuntimeEvent,
    SchemaVersion, UnknownEventPolicy, EVENT_KIND_SPECS, RUNTIME_EVENT_SCHEMA,
};
use serde_json::json;

#[test]
fn event_kind_registry_is_machine_readable_and_bounded() {
    assert!(!EVENT_KIND_SPECS.is_empty());
    for spec in EVENT_KIND_SPECS {
        assert_eq!(spec.schema, RUNTIME_EVENT_SCHEMA);
        assert_eq!(spec.owner_crate, "kiana-domain");
        assert!(!spec.kind.is_empty());
        assert!(!spec.aggregate_type.is_empty());
        assert!(!spec.secret_policy.is_empty());
        let _: EventKindSpec = *spec;
    }
    assert!(event_kind_spec("run.completed").is_some_and(|spec| spec.terminal));
    assert!(event_kind_spec("run.prompt").is_some_and(|spec| !spec.terminal));
}

#[test]
fn unknown_required_kind_and_schema_downgrade_fail_closed() {
    assert_eq!(
        unknown_event_policy("future.opaque").unwrap(),
        UnknownEventPolicy::PreserveOpaqueWithoutExecution
    );
    assert_eq!(
        unknown_event_policy("run.future").unwrap_err(),
        "unknown_required_event_kind"
    );
    assert_eq!(
        check_event_schema_version("run.completed", &SchemaVersion::new(0, 0)).unwrap_err(),
        "event_schema_version_incompatible"
    );
    assert_eq!(
        event_migration("run.completed", 0, 1),
        Some("legacy_run_event_v0_to_v1")
    );
    assert!(event_migration("future.opaque", 0, 1).is_none());
}

#[test]
fn payload_unknown_field_is_not_silently_dropped() {
    let run_id = kiana_domain::RunId::new();
    let valid = json!({"run_id":run_id,"error":"failed","turn_id":kiana_domain::TurnId::new()});
    validate_event_payload("run.failed", &valid).unwrap();
    let mut unknown = valid;
    unknown["unexpected"] = json!(true);
    assert_eq!(
        validate_event_payload("run.failed", &unknown).unwrap_err(),
        "event_payload_unknown_field"
    );
    let event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "future.opaque",
        json!({"untrusted":true}),
    )
    .unwrap();
    validate_runtime_event(&event).unwrap();
    let required = RuntimeEvent::new(RequestId::new(), 1, "run.future", json!({})).unwrap();
    assert_eq!(
        validate_runtime_event(&required).unwrap_err(),
        "unknown_required_event_kind"
    );
}
