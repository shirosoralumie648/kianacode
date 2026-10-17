use kiana_daemon::eval_runtime::{self, EvalInitialStateBundle, EvalInitialStateStore};
use kiana_ports::FixtureStore;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

static SANDBOX_SEQ: AtomicU64 = AtomicU64::new(0);

fn sandbox() -> eval_runtime::EvalRuntimeSandbox {
    let seed = SANDBOX_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let sandbox = eval_runtime::EvalRuntimeSandbox::create("eq13", 1_700_000_000_000, 1_000, seed);
    sandbox.unwrap()
}

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
    assert_eq!(store.fixture_names().len(), 6);
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
    for name in store.fixture_names() {
        let digest = store.fixture_digests().get(&name).unwrap();
        assert_eq!(digest.len(), 71);
    }
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
    assert!(store
        .read_fixture("does-not-exist", &bundle.initial_state_digest)
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

    let mut wrong_schema = bundle();
    wrong_schema.schema = "kiana.eval-initial-state.v2".to_owned();
    assert!(wrong_schema.validate().is_err());

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
    assert!(EvalInitialStateBundle::new(
        "eq13-oversize",
        json!({"blob": "x".repeat(140 * 1024)}),
        json!({}),
        json!({}),
        json!({}),
        json!({}),
    )
    .is_err());
}

#[test]
fn initial_state_store_materializes_into_isolated_sandbox() {
    let bundle = bundle();
    let store = EvalInitialStateStore::from_bundle(&bundle).unwrap();
    let sandbox = sandbox();
    let manifest = sandbox.materialize_initial_state(&store).unwrap();
    manifest.validate().unwrap();
    let expected_schema = kiana_daemon::eval_runtime::EVAL_INITIAL_STATE_MATERIALIZATION_SCHEMA;
    assert_eq!(manifest.schema, expected_schema);
    assert_eq!(manifest.scope_digest, bundle.initial_state_digest);
    assert_eq!(manifest.entries.len(), store.fixture_names().len());
    let expected_base = format!("eval-initial-state/{}", bundle.initial_state_digest);
    assert_eq!(manifest.base_relative_path, expected_base);
    for entry in &manifest.entries {
        assert!(entry.relative_path.starts_with("eval-initial-state/"));
        let on_disk = sandbox.kiana_home().join(&entry.relative_path);
        assert!(on_disk.starts_with(sandbox.kiana_home()));
        assert!(on_disk.starts_with(sandbox.root()));
        let metadata = std::fs::metadata(&on_disk).unwrap();
        assert_eq!(metadata.len() as usize, entry.byte_len);
        let stored_digest = store.fixture_digests().get(&entry.name).unwrap();
        assert_eq!(stored_digest, &entry.fixture_digest);
    }
    assert!(!sandbox.workspace().join("eval-initial-state").exists());
}

#[test]
fn initial_state_materialization_rejects_path_escape_and_digest_drift() {
    let store = EvalInitialStateStore::from_bundle(&bundle()).unwrap();
    let sandbox = sandbox();
    let manifest = sandbox.materialize_initial_state(&store).unwrap();

    let mut escaped = manifest.clone();
    escaped.entries[0].relative_path = "../outside.json".to_owned();
    escaped.manifest_digest = escaped.digest();
    let code = escaped.validate().unwrap_err();
    assert_eq!(code, "eval_initial_state_materialization_entry_invalid");

    let mut renamed = manifest.clone();
    renamed.entries[0].name = "not allow listed".to_owned();
    renamed.manifest_digest = renamed.digest();
    assert!(renamed.validate().is_err());

    let mut drifted = manifest.clone();
    drifted.entries[0].fixture_digest = "sha256:deadbeef".to_owned();
    drifted.manifest_digest = drifted.digest();
    let code = drifted.validate().unwrap_err();
    assert_eq!(code, "eval_initial_state_materialization_entry_invalid");

    let mut wrong_base = manifest.clone();
    wrong_base.base_relative_path = "eval-initial-state/other".to_owned();
    wrong_base.manifest_digest = wrong_base.digest();
    let code = wrong_base.validate().unwrap_err();
    assert_eq!(code, "eval_initial_state_materialization_base_invalid");

    let mut bad_scope = manifest;
    bad_scope.scope_digest = "sha256:not-hex".to_owned();
    bad_scope.manifest_digest = bad_scope.digest();
    assert!(bad_scope.validate().is_err());
}
