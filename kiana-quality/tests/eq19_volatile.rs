use kiana_domain::{RequestId, RuntimeEvent};
use kiana_quality::{
    standard_uuid_rule_paths, ArrayPolicy, DurableEvent, TraceNormalizer, VolatileError,
    VolatileKind, VolatilePolicy, VolatileRule, MAX_VOLATILE_REPLACEMENTS, MAX_VOLATILE_RULES,
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

fn policy_with(rules: impl IntoIterator<Item = VolatileRule>) -> VolatilePolicy {
    let mut all = fixture_uuid_rules();
    all.extend(rules);
    VolatilePolicy::new(all).unwrap()
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
    // Every volatile-looking value must fail closed unless a rule declares its path.
    let cases = [
        (
            json!({"arguments": "550e8400-e29b-41d4-a716-446655440000"}),
            VolatileKind::Uuid,
            "events[0].data.arguments",
        ),
        (
            json!({"cancellation_at_unix_ms": 123}),
            VolatileKind::Timestamp,
            "events[0].data.arguments.cancellation_at_unix_ms",
        ),
        (
            json!({"project_root": "/tmp/eval-run-1"}),
            VolatileKind::TempPath,
            "events[0].data.arguments.project_root",
        ),
        (
            json!({"cancel_actor_id": "operator-1"}),
            VolatileKind::Actor,
            "events[0].data.arguments.cancel_actor_id",
        ),
    ];
    let policy = VolatilePolicy::new(fixture_uuid_rules()).unwrap();
    for (data, kind, path) in cases {
        let trace = canonical_trace(data);
        let error = kiana_quality::normalize_volatile(&trace, &policy)
            .expect_err("undeclared timestamp/actor/path/uuid must fail closed");
        let message = error.to_string();
        assert_eq!(
            error,
            VolatileError::UndeclaredVolatile {
                path: path.to_owned(),
                kind,
            }
        );
        assert!(message.starts_with("volatile_undeclared:"));
    }
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

#[test]
fn wildcard_array_rule_does_not_normalize_undeclared_siblings() {
    // A wildcard rule covers exactly its declared path; the same field name elsewhere
    // stays undeclared and fails closed instead of being rewritten.
    let trace = canonical_trace(json!({
        "arguments": {
            "items": [{"actor_id": "operator-1"}],
            "other": {"actor_id": "operator-2"}
        }
    }));
    let rules = [VolatileRule::new(
        "data.arguments.arguments.items.*.actor_id",
        VolatileKind::Actor,
    )
    .unwrap()];
    let error = kiana_quality::normalize_volatile(&trace, &policy_with(rules))
        .expect_err("a wildcard rule must not cover sibling paths");
    assert_eq!(
        error,
        VolatileError::UndeclaredVolatile {
            path: "events[0].data.arguments.arguments.other.actor_id".to_owned(),
            kind: VolatileKind::Actor,
        }
    );
}

#[test]
fn invalid_rule_and_policy_shapes_are_rejected() {
    for path in [
        "",
        ".data",
        "data.",
        "data..x",
        "data/sub",
        "data/arguments",
    ] {
        assert_eq!(
            VolatileRule::new(path, VolatileKind::Actor).unwrap_err(),
            VolatileError::RuleInvalid {
                path: path.to_owned(),
            }
        );
    }
    let oversized = "a".repeat(513);
    assert_eq!(
        VolatileRule::new(oversized.clone(), VolatileKind::Uuid).unwrap_err(),
        VolatileError::RuleInvalid { path: oversized }
    );

    let rule = VolatileRule::new("data.arguments.actor_id", VolatileKind::Actor).unwrap();
    assert_eq!(
        VolatilePolicy::new(vec![rule.clone(), rule.clone()]).unwrap_err(),
        VolatileError::DuplicateRule {
            path: "data.arguments.actor_id".to_owned(),
        }
    );
    // One path may not silently carry two kinds: first-match would hide the second rule.
    assert_eq!(
        VolatilePolicy::new(vec![
            rule.clone(),
            VolatileRule::new("data.arguments.actor_id", VolatileKind::Uuid).unwrap(),
        ])
        .unwrap_err(),
        VolatileError::AmbiguousRule {
            path: "data.arguments.actor_id".to_owned(),
        }
    );
    assert_eq!(
        VolatilePolicy::with_max_replacements(vec![rule.clone()], 0).unwrap_err(),
        VolatileError::PolicyInvalid
    );
    assert_eq!(
        VolatilePolicy::with_max_replacements(vec![rule], MAX_VOLATILE_REPLACEMENTS + 1)
            .unwrap_err(),
        VolatileError::PolicyInvalid
    );
    let too_many = (0..=MAX_VOLATILE_RULES)
        .map(|index| VolatileRule::new(format!("field_{index}"), VolatileKind::Timestamp).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        VolatilePolicy::new(too_many).unwrap_err(),
        VolatileError::PolicyInvalid
    );
}

#[test]
fn declared_kind_must_match_the_value_type() {
    let cases = [
        (
            json!({"cancel_actor_id": {"nested": "operator-1"}}),
            VolatileRule::new("data.arguments.cancel_actor_id", VolatileKind::Actor).unwrap(),
            VolatileKind::Actor,
        ),
        (
            json!({"project_root": 17}),
            VolatileRule::new("data.arguments.project_root", VolatileKind::TempPath).unwrap(),
            VolatileKind::TempPath,
        ),
        (
            json!({"cancellation_at_unix_ms": false}),
            VolatileRule::new(
                "data.arguments.cancellation_at_unix_ms",
                VolatileKind::Timestamp,
            )
            .unwrap(),
            VolatileKind::Timestamp,
        ),
    ];
    for (data, rule, kind) in cases {
        let path = rule.path.clone();
        let trace = canonical_trace(data);
        let error = kiana_quality::normalize_volatile(&trace, &policy_with([rule]))
            .expect_err("a declared kind must match the value type");
        assert_eq!(
            error,
            VolatileError::ValueTypeMismatch {
                path: format!("events[0].{path}"),
                kind,
            }
        );
    }
}

#[test]
fn source_normalization_version_must_match_before_any_replacement() {
    let mut trace = canonical_trace(json!({"arguments": null}));
    trace.normalization_version = "eq18.canonical-events.v2".to_owned();
    let error = kiana_quality::normalize_volatile(&trace, &policy())
        .expect_err("foreign source normalization version must fail closed");
    assert_eq!(error, VolatileError::SourceVersionInvalid);
}

#[test]
fn replacement_ledger_records_paths_kinds_and_per_kind_counts() {
    let trace = canonical_trace(json!({
        "cancellation_at_unix_ms": 123,
        "cancel_actor_id": "operator-1",
        "project_root": "/tmp/eval-run-1",
        "arguments": null
    }));
    let normalized = kiana_quality::normalize_volatile(&trace, &policy()).unwrap();
    assert_eq!(normalized.replacement_count, normalized.replacements.len());
    assert_eq!(normalized.replacement_count_for(VolatileKind::Uuid), 3);
    assert_eq!(normalized.replacement_count_for(VolatileKind::Timestamp), 1);
    assert_eq!(normalized.replacement_count_for(VolatileKind::TempPath), 1);
    assert_eq!(normalized.replacement_count_for(VolatileKind::Actor), 1);

    let mut recorded = normalized
        .replacements
        .iter()
        .map(|replacement| (replacement.path.clone(), replacement.kind))
        .collect::<Vec<_>>();
    recorded.sort();
    let mut expected = vec![
        ("events[0].correlation_id".to_owned(), VolatileKind::Uuid),
        (
            "events[0].data.arguments.cancel_actor_id".to_owned(),
            VolatileKind::Actor,
        ),
        (
            "events[0].data.arguments.cancellation_at_unix_ms".to_owned(),
            VolatileKind::Timestamp,
        ),
        (
            "events[0].data.arguments.project_root".to_owned(),
            VolatileKind::TempPath,
        ),
        ("events[0].event_id".to_owned(), VolatileKind::Uuid),
        ("events[0].request_id".to_owned(), VolatileKind::Uuid),
    ];
    expected.sort();
    assert_eq!(recorded, expected);

    let bytes = String::from_utf8(normalized.canonical_bytes().expect("canonical bytes")).unwrap();
    for token in ["<UUID>", "<TS>", "<TEMP_PATH>", "<ACTOR>"] {
        assert!(bytes.contains(token), "missing token: {token}");
    }
    for raw in [
        "operator-1",
        "/tmp/eval-run-1",
        "\"cancellation_at_unix_ms\":123",
    ] {
        assert!(!bytes.contains(raw), "raw volatile value leaked: {raw}");
    }
}
