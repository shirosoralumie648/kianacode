use kiana_domain::{RequestId, RuntimeEvent};
use kiana_quality::{
    standard_uuid_rule_paths, ArrayPolicy, DurableEvent, TraceNormalizer, VolatileError,
    VolatileKind, VolatilePolicy, VolatileRule,
};
use serde_json::json;

fn canonical_trace(data: serde_json::Value) -> kiana_quality::CanonicalEventTrace {
    let request_id = RequestId::new();
    let event = RuntimeEvent::new(
        request_id,
        1,
        "run.capability_requested",
        json!({
            "run_id": "run-1",
            "capability_request_id": "capability-1",
            "arguments": data
        }),
    )
    .expect("valid runtime event")
    .with_stream_metadata("run", "run-1", 1)
    .with_identity_links(None, Some(request_id), None, None);
    TraceNormalizer::new()
        .canonicalize(&[DurableEvent::new(1, event)], ArrayPolicy::Ordered)
        .expect("canonical trace")
}

fn fixture_uuid_rules() -> Vec<VolatileRule> {
    // This valid event has three UUID values; its optional causal/parent links are null.
    // Declaring a null as a UUID string would intentionally fail strict volatile validation.
    standard_uuid_rule_paths()
        .into_iter()
        .filter(|rule| {
            matches!(
                rule.path.as_str(),
                "event_id" | "request_id" | "correlation_id"
            )
        })
        .collect()
}

fn policy() -> VolatilePolicy {
    let mut rules = fixture_uuid_rules();
    rules.extend([
        VolatileRule::new(
            "data.arguments.cancellation_at_unix_ms",
            VolatileKind::Timestamp,
        )
        .unwrap(),
        VolatileRule::new("data.arguments.cancel_actor_id", VolatileKind::Actor).unwrap(),
        VolatileRule::new("data.arguments.project_root", VolatileKind::TempPath).unwrap(),
    ]);
    VolatilePolicy::new(rules).unwrap()
}

#[test]
fn declared_timestamp_uuid_temp_path_and_actor_are_replaced_with_counts() {
    let trace = canonical_trace(json!({
        "cancellation_at_unix_ms": 123,
        "cancel_actor_id": "operator-1",
        "project_root": "/tmp/eval-run-1",
        "arguments": null
    }));
    let normalized = kiana_quality::normalize_volatile(&trace, &policy()).unwrap();
    assert_eq!(normalized.replacement_count, 6);
    assert_eq!(normalized.events[0].value["event_id"], "<UUID>");
    assert_eq!(normalized.events[0].value["request_id"], "<UUID>");
    assert_eq!(
        normalized.events[0].value["data"]["arguments"]["cancellation_at_unix_ms"],
        "<TS>"
    );
    assert_eq!(
        normalized.events[0].value["data"]["arguments"]["cancel_actor_id"],
        "<ACTOR>"
    );
    assert_eq!(
        normalized.events[0].value["data"]["arguments"]["project_root"],
        "<TEMP_PATH>"
    );
    assert!(normalized.canonical_bytes().is_ok());
}

#[test]
fn undeclared_volatile_value_is_not_silently_normalized() {
    let trace = canonical_trace(json!({
        "cancellation_at_unix_ms": 123,
        "cancel_actor_id": "operator-1",
        "project_root": "/tmp/eval-run-1",
        "arguments": "550e8400-e29b-41d4-a716-446655440000"
    }));
    let error = kiana_quality::normalize_volatile(
        &trace,
        &VolatilePolicy::new(fixture_uuid_rules()).unwrap(),
    )
    .expect_err("undeclared timestamp/actor/path must fail closed");
    assert!(matches!(error, VolatileError::UndeclaredVolatile { .. }));
    assert!(error.to_string().contains("volatile_undeclared"));
}

#[test]
fn wildcard_rules_cover_array_members_and_replacement_budget_is_bounded() {
    let trace = canonical_trace(json!({
        "cancellation_at_unix_ms": 123,
        "cancel_actor_id": "operator-1",
        "project_root": "/tmp/eval-run-1",
        "arguments": [{"actor_id": "a"}, {"actor_id": "b"}]
    }));
    let mut rules = fixture_uuid_rules();
    rules.push(
        VolatileRule::new("data.arguments.arguments.*.actor_id", VolatileKind::Actor).unwrap(),
    );
    rules.extend([
        VolatileRule::new(
            "data.arguments.cancellation_at_unix_ms",
            VolatileKind::Timestamp,
        )
        .unwrap(),
        VolatileRule::new("data.arguments.cancel_actor_id", VolatileKind::Actor).unwrap(),
        VolatileRule::new("data.arguments.project_root", VolatileKind::TempPath).unwrap(),
    ]);
    let normalized =
        kiana_quality::normalize_volatile(&trace, &VolatilePolicy::new(rules.clone()).unwrap())
            .unwrap();
    assert_eq!(normalized.replacement_count, 8);
    assert_eq!(
        normalized.events[0].value["data"]["arguments"]["arguments"][0]["actor_id"],
        "<ACTOR>"
    );

    let limited = VolatilePolicy::with_max_replacements(rules, 1).unwrap();
    assert_eq!(
        kiana_quality::normalize_volatile(&trace, &limited).unwrap_err(),
        VolatileError::ReplacementLimitExceeded
    );
}
