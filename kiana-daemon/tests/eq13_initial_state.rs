use kiana_daemon::eval_runtime::{EvalInitialStateBundle, EvalInitialStateStore};
use kiana_ports::FixtureStore;
use serde_json::json;

fn bundle() -> EvalInitialStateBundle {
    EvalInitialStateBundle::new(
        "eq13-experiment",
        json!({"policy": "deny-by-default", "revision": 3}),
        json!({"role": "reviewer", "project": "fixture-project"}),
        json!({"collection": "lessons", "records": []}),
        json!({"workflow": "fixture-workflow", "version": 1}),
        json!({"artifact": "fixture-output", "sha256": "sha256:fixture"}),
    )
    .unwrap()
}

#[tokio::test]
async fn initial_state_digest_is_bound_to_experiment() {
    let bundle = bundle();
    let store = EvalInitialStateStore::from_bundle(&bundle).unwrap();
    assert_eq!(store.scope_digest(), bundle.initial_state_digest);
    assert_eq!(
        store.fixture_names(),
        vec![
            "artifact_fixture",
            "initial_state",
            "memory_fixture",
            "policy_snapshot",
            "role_assignment",
            "workflow_fixture"
        ]
    );
    let initial = store
        .read_fixture("initial_state", &bundle.initial_state_digest)
        .await
        .unwrap();
    assert!(!initial.is_empty());
    assert!(store
        .read_fixture("initial_state", "sha256:wrong-scope")
        .await
        .is_err());
    assert!(store
        .read_fixture("../initial_state", &bundle.initial_state_digest)
        .await
        .is_err());
}

#[test]
fn initial_state_bundle_rejects_drift_and_secret_values() {
    let mut tampered = bundle();
    tampered.policy_snapshot["revision"] = json!(4);
    assert_eq!(
        tampered.validate().unwrap_err(),
        "eval_initial_state_digest_mismatch"
    );

    assert!(EvalInitialStateBundle::new(
        "eq13-secret",
        json!({"api_key": "raw-secret"}),
        json!({"role": "reviewer"}),
        json!({}),
        json!({}),
        json!({}),
    )
    .is_err());
    assert!(EvalInitialStateBundle::new(
        "eq13-invalid",
        json!([]),
        json!({}),
        json!({}),
        json!({}),
        json!({}),
    )
    .is_err());
}
