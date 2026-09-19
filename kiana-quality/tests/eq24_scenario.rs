use kiana_domain::RequestId;
use kiana_quality::{
    CleanupReceipt, ScenarioArtifact, ScenarioOutcome, ScenarioRunner, ScenarioSpec,
    ScopePredicate, ScrubbedEnvironment, VolatileEvent, VolatileEventTrace,
    CANONICAL_NORMALIZATION_VERSION, CLEANUP_RECEIPT_SCHEMA, SCENARIO_SCHEMA,
    SCRUBBED_ENVIRONMENT_SCHEMA, VOLATILE_NORMALIZATION_VERSION,
};
use serde_json::json;
use std::collections::BTreeMap;

fn environment(network_profile: &str) -> ScrubbedEnvironment {
    ScrubbedEnvironment {
        schema: SCRUBBED_ENVIRONMENT_SCHEMA.to_owned(),
        workspace_token: "<WORKSPACE>".to_owned(),
        home_token: "<KIANA_HOME>".to_owned(),
        variables: BTreeMap::from([
            ("CI".to_owned(), "true".to_owned()),
            ("SECRET".to_owned(), "<REDACTED>".to_owned()),
        ]),
        network_profile: network_profile.to_owned(),
    }
}

fn trace(state: &str) -> VolatileEventTrace {
    VolatileEventTrace {
        normalization_version: VOLATILE_NORMALIZATION_VERSION.to_owned(),
        source_normalization_version: CANONICAL_NORMALIZATION_VERSION.to_owned(),
        array_policy: kiana_quality::ArrayPolicy::Ordered,
        source_cursor_start: 1,
        source_cursor_end: 1,
        correlation_id: RequestId::new(),
        events: vec![VolatileEvent {
            source_cursor: 1,
            kind: "run.completed".to_owned(),
            value: json!({
                "kind": "run.completed",
                "source_cursor": 1,
                "data": {"state": state}
            }),
        }],
        terminal_event_indexes: vec![0],
        replacement_count: 0,
        replacements: Vec::new(),
    }
}

fn artifact(state: &str, network_profile: &str) -> ScenarioArtifact {
    ScenarioArtifact {
        scenario_id: "scenario-1".to_owned(),
        source_ref: "fixture:scenario-1".to_owned(),
        trace: trace(state),
        environment: environment(network_profile),
    }
}

fn spec(scope: ScopePredicate, cleanup_required: bool) -> ScenarioSpec {
    ScenarioSpec {
        schema: SCENARIO_SCHEMA.to_owned(),
        scenario_id: "scenario-1".to_owned(),
        scope,
        cleanup_required,
    }
}

fn cleanup(completed: bool) -> CleanupReceipt {
    CleanupReceipt {
        schema: CLEANUP_RECEIPT_SCHEMA.to_owned(),
        workspace_ref: "workspace:scenario-1".to_owned(),
        home_ref: "home:scenario-1".to_owned(),
        completed,
        orphan_count: 0,
    }
}

#[test]
fn candidate_diff_uses_scrubbed_environment_and_scope_report() {
    let report = ScenarioRunner::compare(
        &spec(
            ScopePredicate::FieldPrefixes(vec!["events[0].data.state".to_owned()]),
            true,
        ),
        &artifact("reference", "network:none"),
        &artifact("candidate", "network:none"),
        cleanup(true),
    )
    .expect("scenario comparison");
    assert_eq!(report.outcome, ScenarioOutcome::InScope);
    assert!(report.environment_match);
    assert_eq!(report.scope_reason, "first_divergence_in_scope");
    assert_ne!(report.reference_trace_digest, report.candidate_trace_digest);
}

#[test]
fn environment_mismatch_cleanup_and_out_of_scope_are_visible() {
    let environment_mismatch = ScenarioRunner::compare(
        &spec(ScopePredicate::AllowAll, true),
        &artifact("same", "network:none"),
        &artifact("same", "network:fixture-only"),
        cleanup(true),
    )
    .unwrap();
    assert_eq!(environment_mismatch.outcome, ScenarioOutcome::NotComparable);
    assert!(!environment_mismatch.environment_match);

    let cleanup_required = ScenarioRunner::compare(
        &spec(ScopePredicate::AllowAll, true),
        &artifact("same", "network:none"),
        &artifact("changed", "network:none"),
        cleanup(false),
    )
    .unwrap();
    assert_eq!(cleanup_required.outcome, ScenarioOutcome::CleanupRequired);

    let out_of_scope = ScenarioRunner::compare(
        &spec(
            ScopePredicate::FieldPrefixes(vec!["events[0].data.allowed".to_owned()]),
            false,
        ),
        &artifact("reference", "network:none"),
        &artifact("candidate", "network:none"),
        cleanup(true),
    )
    .unwrap();
    assert_eq!(out_of_scope.outcome, ScenarioOutcome::OutOfScope);
}

#[test]
fn raw_secret_or_absolute_environment_value_is_rejected() {
    let mut unsafe_environment = environment("network:none");
    unsafe_environment.variables.insert(
        "TOKEN".to_owned(),
        "Authorization: Bearer secret".to_owned(),
    );
    assert!(unsafe_environment.validate().is_err());
    unsafe_environment
        .variables
        .insert("TOKEN".to_owned(), "/tmp/raw".to_owned());
    assert!(unsafe_environment.validate().is_err());
}
