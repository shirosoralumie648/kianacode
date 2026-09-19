use kiana_domain::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn source(id: &str) -> SourceRef {
    SourceRef::new(
        id,
        SourceKind::UserImport,
        format!("fixture://{id}"),
        "revision:1",
        json_digest(&json!({"source": id})),
        Some(1),
        EvidenceStatus::Attributed,
    )
    .unwrap()
}

fn snapshot() -> ExtensionSnapshot {
    let snapshot_id = SnapshotId::new();
    let hook = HookDescriptor {
        schema: HOOK_DESCRIPTOR_SCHEMA.to_owned(),
        hook_id: "hook.lifecycle".to_owned(),
        event: "before.tool".to_owned(),
        matcher: "shell".to_owned(),
        phase: HookPhase::Guard,
        source: source("hook"),
        snapshot_id,
    };
    ExtensionSnapshot::new(
        snapshot_id,
        1,
        vec![source("manifest")],
        Vec::new(),
        vec![hook],
        Vec::new(),
        json_digest(&json!({"trust":"trusted"})),
    )
    .unwrap()
}

fn binding(
    snapshot: &ExtensionSnapshot,
    role: HookExecutionRole,
    revalidate: bool,
) -> HookLifecycleBinding {
    HookLifecycleBinding::new(
        &snapshot.hooks[0],
        &snapshot,
        HookLifecyclePoint::BeforeTool,
        role,
        1,
        BTreeSet::from([
            "context.user_message".to_owned(),
            "tool.arguments".to_owned(),
        ]),
        if role == HookExecutionRole::Observer {
            BTreeSet::new()
        } else {
            BTreeSet::from(["context.additional_context".to_owned()])
        },
        revalidate,
        HookFailurePolicy::Block,
        HookBudget {
            timeout_ms: 500,
            max_output_bytes: 8 * 1024,
            max_feedback_bytes: 1024,
        },
    )
    .unwrap()
}

#[test]
fn trusted_snapshot_orders_lifecycle_bindings_deterministically() {
    let snapshot = snapshot();
    let first = binding(&snapshot, HookExecutionRole::Observer, false);
    let mut second = first.clone();
    second.hook_id = "hook.lifecycle.2".to_owned();
    second.binding_digest = second.digest();
    second.validate().unwrap();
    let plan = HookLifecyclePlan::new(&snapshot, vec![second, first]).unwrap();
    plan.validate().unwrap();
    assert_eq!(plan.bindings[0].sequence, 1);
    assert_eq!(plan.snapshot_digest, snapshot.snapshot_digest);
}

#[test]
fn observer_cannot_write_and_transformer_writes_are_bounded() {
    let snapshot = snapshot();
    let observer = binding(&snapshot, HookExecutionRole::Observer, false);
    let input = HookLifecycleInput::new(
        &observer,
        BTreeMap::from([("context.user_message".to_owned(), json!("hello"))]),
    )
    .unwrap();
    assert_eq!(
        observer
            .apply_updates(
                &input,
                BTreeMap::from([("context.user_message".to_owned(), json!("changed"))]),
            )
            .unwrap_err(),
        "hook_observer_write_denied"
    );

    let transformer = binding(&snapshot, HookExecutionRole::Transformer, false);
    let input = HookLifecycleInput::new(
        &transformer,
        BTreeMap::from([("context.user_message".to_owned(), json!("hello"))]),
    )
    .unwrap();
    let result = transformer
        .apply_updates(
            &input,
            BTreeMap::from([("context.additional_context".to_owned(), json!("bounded"))]),
        )
        .unwrap();
    assert!(result.changed_fields.contains("context.additional_context"));
    assert!(!result.requires_revalidation);
}

#[test]
fn tool_argument_change_requires_revalidation_and_foreign_fields_are_denied() {
    let snapshot = snapshot();
    let mut transformer = binding(&snapshot, HookExecutionRole::Transformer, false);
    transformer
        .writable_fields
        .insert("tool.arguments".to_owned());
    transformer.binding_digest = transformer.digest();
    assert_eq!(
        transformer.validate().unwrap_err(),
        "hook_tool_change_revalidation_required"
    );

    let observer = binding(&snapshot, HookExecutionRole::Observer, false);
    let mut fields = BTreeMap::new();
    fields.insert("capability.grant".to_owned(), json!("forged"));
    assert_eq!(
        HookLifecycleInput::new(&observer, fields).unwrap_err(),
        "hook_lifecycle_input_invalid"
    );
}
