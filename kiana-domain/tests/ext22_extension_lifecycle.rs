use kiana_domain::{
    ExtensionConfigEntry, ExtensionConfigScope, ExtensionConfigSnapshot, ExtensionLifecycleAction,
    ExtensionLifecycleMutation,
};

fn sha(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

#[test]
fn config_snapshot_records_sources_and_uses_narrowest_scope_precedence() {
    let snapshot = ExtensionConfigSnapshot::new(
        "plugin.alpha",
        4,
        vec![
            ExtensionConfigEntry {
                key: "endpoint".to_owned(),
                scope: ExtensionConfigScope::Host,
                source_ref: "host:defaults".to_owned(),
                value_digest: sha('a'),
                secret_handle: None,
            },
            ExtensionConfigEntry {
                key: "endpoint".to_owned(),
                scope: ExtensionConfigScope::Project,
                source_ref: "project:config".to_owned(),
                value_digest: sha('b'),
                secret_handle: None,
            },
            ExtensionConfigEntry {
                key: "endpoint".to_owned(),
                scope: ExtensionConfigScope::Run,
                source_ref: "run:override".to_owned(),
                value_digest: sha('c'),
                secret_handle: Some("secret.api".to_owned()),
            },
        ],
    )
    .expect("snapshot");
    let effective = snapshot.effective_entries().expect("effective config");
    assert_eq!(effective.len(), 1);
    assert_eq!(effective[0].scope, ExtensionConfigScope::Run);
    assert_eq!(effective[0].source_ref, "run:override");
    assert_eq!(effective[0].secret_handle.as_deref(), Some("secret.api"));
}

#[test]
fn mutation_contract_requires_server_snapshots_for_enable() {
    let mutation = ExtensionLifecycleMutation::new(
        ExtensionLifecycleAction::Enable,
        "plugin.alpha",
        "/workspace/project",
        Some("a".repeat(64)),
        4,
        "idem-4",
        "operator:local",
        "enable verified package",
        "trust:revision-4",
        Some(sha('d')),
        Some(sha('e')),
        Some("approval.alpha".to_owned()),
    )
    .expect("enable mutation");
    mutation.validate().expect("valid enable mutation");

    let missing_approval = ExtensionLifecycleMutation::new(
        ExtensionLifecycleAction::Enable,
        "plugin.alpha",
        "/workspace/project",
        Some("a".repeat(64)),
        4,
        "idem-5",
        "operator:local",
        "enable verified package",
        "trust:revision-4",
        Some(sha('d')),
        Some(sha('e')),
        None,
    )
    .expect_err("enable without approval must fail closed");
    assert_eq!(
        missing_approval,
        "extension_enable_approval_and_config_required"
    );
}

#[test]
fn mutation_contract_keeps_stage_inert_and_rejects_unknown_fields() {
    let staged = ExtensionLifecycleMutation::new(
        ExtensionLifecycleAction::Stage,
        "plugin.alpha",
        "/workspace/project",
        Some("a".repeat(64)),
        4,
        "idem-stage",
        "operator:local",
        "stage verified package",
        "trust:revision-4",
        Some(sha('d')),
        None,
        None,
    )
    .expect("stage mutation");
    staged.validate().expect("valid stage mutation");
    assert!(staged.approval_id.is_none());

    let unknown = serde_json::json!({
        "schema": "kiana.extension-lifecycle-mutation.v1",
        "version": {"major": 1, "minor": 0},
        "action": "inspect",
        "extension_id": "plugin.alpha",
        "project_root": "/workspace/project",
        "expected_registry_version": 4,
        "idempotency_key": "idem-inspect",
        "actor_ref": "operator:local",
        "reason": "inspect",
        "trust_revision": "trust:revision-4",
        "mutation_digest": sha('f'),
        "unexpected": true
    });
    assert!(serde_json::from_value::<ExtensionLifecycleMutation>(unknown).is_err());
}
