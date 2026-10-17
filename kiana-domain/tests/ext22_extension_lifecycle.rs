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

/// The typed mutation is re-derivable from the shared `extension.manage` argument object: the
/// ControlPlane calls this *after* injecting server-owned fields, and the daemon calls it again
/// before any registry CAS. Both derivations must agree, so the digest cannot be client-minted.
#[test]
fn mutation_rebuilds_from_command_arguments_and_requires_every_binding() {
    let base = serde_json::json!({
        "action": "stage",
        "extension_id": "plugin.alpha",
        "project_root": "/workspace/project",
        "package_sha256": "a".repeat(64),
        "expected_registry_version": 0,
        "idempotency_key": "idem-stage",
        "actor_ref": "operator:local",
        "reason": "stage verified package",
        "trust_revision": "trust:revision-1",
        "dependency_snapshot_digest": sha('d'),
    });
    // A fresh registry has generation 0: the first stage/install of a project must be
    // representable, and the daemon still compares the declared value with the folded one.
    let mutation = ExtensionLifecycleMutation::from_command_arguments(&base)
        .expect("stage mutation from command arguments");
    assert_eq!(mutation.action, ExtensionLifecycleAction::Stage);
    assert_eq!(mutation.expected_registry_version, 0);
    assert_eq!(
        mutation.package_sha256.as_deref(),
        Some("a".repeat(64).as_str())
    );
    assert_eq!(mutation.mutation_digest, mutation.digest());
    ExtensionLifecycleMutation::from_command_arguments(&base)
        .expect("re-derivation")
        .validate()
        .expect("re-derived mutation validates");

    // The server-owned actor alias is accepted when `actor_ref` is absent.
    let mut alias = base.clone();
    alias
        .as_object_mut()
        .expect("arguments object")
        .remove("actor_ref");
    alias["actor_id"] = serde_json::json!("operator:local");
    ExtensionLifecycleMutation::from_command_arguments(&alias).expect("actor_id alias");

    let mutate = |change: fn(&mut serde_json::Map<String, serde_json::Value>)| {
        let mut value = base.clone();
        change(value.as_object_mut().expect("arguments object"));
        value
    };
    let cases: Vec<(serde_json::Value, &str)> = vec![
        (
            mutate(|object| {
                object.remove("action");
            }),
            "extension_lifecycle_action_invalid",
        ),
        (
            mutate(|object| {
                object.insert("action".to_owned(), serde_json::json!("promote"));
            }),
            "extension_lifecycle_action_invalid",
        ),
        (
            mutate(|object| {
                object.remove("extension_id");
            }),
            "extension_lifecycle_extension_id_invalid",
        ),
        (
            mutate(|object| {
                object.remove("project_root");
            }),
            "extension_lifecycle_project_root_required",
        ),
        (
            mutate(|object| {
                object.remove("expected_registry_version");
            }),
            "extension_lifecycle_expected_registry_version_required",
        ),
        (
            mutate(|object| {
                object.remove("idempotency_key");
            }),
            "extension_lifecycle_idempotency_key_required",
        ),
        (
            mutate(|object| {
                object.remove("actor_ref");
            }),
            "extension_lifecycle_actor_required",
        ),
        (
            mutate(|object| {
                object.remove("reason");
            }),
            "extension_lifecycle_reason_required",
        ),
        (
            mutate(|object| {
                object.remove("trust_revision");
            }),
            "extension_lifecycle_trust_revision_required",
        ),
        (
            mutate(|object| {
                object.remove("package_sha256");
            }),
            "extension_lifecycle_package_required",
        ),
        (
            mutate(|object| {
                object.remove("dependency_snapshot_digest");
            }),
            "extension_lifecycle_dependency_snapshot_required",
        ),
    ];
    for (arguments, expected) in cases {
        assert_eq!(
            ExtensionLifecycleMutation::from_command_arguments(&arguments)
                .expect_err("must fail closed"),
            expected
        );
    }

    // An enable additionally needs both the approval and the config snapshot digest.
    let enable = serde_json::json!({
        "action": "enable",
        "extension_id": "plugin.alpha",
        "project_root": "/workspace/project",
        "package_sha256": "a".repeat(64),
        "expected_registry_version": 4,
        "idempotency_key": "idem-enable",
        "actor_ref": "operator:local",
        "reason": "enable verified package",
        "trust_revision": "trust:revision-4",
        "dependency_snapshot_digest": sha('d'),
    });
    assert_eq!(
        ExtensionLifecycleMutation::from_command_arguments(&enable).expect_err("enable binding"),
        "extension_enable_approval_and_config_required"
    );

    // A self-consistent mutation whose digest was tampered with fails validation.
    let mut tampered = serde_json::to_value(&mutation).expect("mutation json");
    tampered["mutation_digest"] = serde_json::json!(sha('9'));
    let tampered: ExtensionLifecycleMutation =
        serde_json::from_value(tampered).expect("deserialize");
    assert_eq!(
        tampered.validate().expect_err("tampered digest"),
        "extension_lifecycle_mutation_digest_mismatch"
    );
}

/// A config snapshot binds to exactly one enable mutation: same extension, same registry
/// generation, same digest. Replaying it onto another mutation or a non-enable action fails.
#[test]
fn config_snapshot_binds_only_to_its_enable_mutation() {
    let entries = |source: &str| {
        vec![ExtensionConfigEntry {
            key: "endpoint".to_owned(),
            scope: ExtensionConfigScope::Project,
            source_ref: source.to_owned(),
            value_digest: sha('a'),
            secret_handle: None,
        }]
    };
    let snapshot = ExtensionConfigSnapshot::new("plugin.alpha", 4, entries("project:config"))
        .expect("snapshot");
    let enable = ExtensionLifecycleMutation::new(
        ExtensionLifecycleAction::Enable,
        "plugin.alpha",
        "/workspace/project",
        Some("a".repeat(64)),
        4,
        "idem-enable",
        "operator:local",
        "enable verified package",
        "trust:revision-4",
        Some(sha('d')),
        Some(snapshot.snapshot_digest.clone()),
        Some("approval.alpha".to_owned()),
    )
    .expect("enable mutation");
    snapshot.bind_to_mutation(&enable).expect("bound snapshot");

    let other_extension = ExtensionConfigSnapshot::new("plugin.beta", 4, entries("project:config"))
        .expect("other snapshot");
    assert_eq!(
        other_extension
            .bind_to_mutation(&enable)
            .expect_err("extension mismatch"),
        "extension_config_extension_mismatch"
    );

    let stale_generation =
        ExtensionConfigSnapshot::new("plugin.alpha", 5, entries("project:config"))
            .expect("stale snapshot");
    assert_eq!(
        stale_generation
            .bind_to_mutation(&enable)
            .expect_err("generation mismatch"),
        "extension_config_registry_version_mismatch"
    );

    let drifted = ExtensionConfigSnapshot::new("plugin.alpha", 4, entries("project:other"))
        .expect("drifted snapshot");
    assert_eq!(
        drifted
            .bind_to_mutation(&enable)
            .expect_err("digest mismatch"),
        "extension_config_snapshot_digest_mismatch"
    );

    let stage = ExtensionLifecycleMutation::new(
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
    assert_eq!(
        snapshot
            .bind_to_mutation(&stage)
            .expect_err("non-enable binding"),
        "extension_config_snapshot_binding_invalid"
    );

    let mut tampered = serde_json::to_value(&snapshot).expect("snapshot json");
    tampered["snapshot_digest"] = serde_json::json!(sha('9'));
    let tampered: ExtensionConfigSnapshot = serde_json::from_value(tampered).expect("deserialize");
    assert_eq!(
        tampered
            .bind_to_mutation(&enable)
            .expect_err("self digest mismatch"),
        "extension_config_snapshot_digest_mismatch"
    );
}
