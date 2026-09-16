use kiana_domain::{
    json_digest, ComponentId, EvidenceStatus, ExtensionError, ExtensionErrorCode,
    ExtensionSnapshot, ExtensionSnapshotState, HookDecision, HookDecisionKind, HookDescriptor,
    HookPhase, HookRunId, PluginLifecycle, PluginLifecycleState, SkillDescriptor, SkillLifecycle,
    SnapshotId, SourceKind, SourceRef, EXTENSION_ERROR_SCHEMA, EXTENSION_SNAPSHOT_SCHEMA,
    HOOK_DECISION_SCHEMA, HOOK_DESCRIPTOR_SCHEMA, PLUGIN_LIFECYCLE_SCHEMA, SKILL_DESCRIPTOR_SCHEMA,
};
use serde_json::json;
use std::collections::BTreeSet;

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
    let skill = SkillDescriptor {
        schema: SKILL_DESCRIPTOR_SCHEMA.to_owned(),
        component_id: ComponentId::new(),
        extension_id: None,
        name: "review-skill".to_owned(),
        version: "1.0.0".to_owned(),
        description: "Review source changes".to_owned(),
        source: source("skill"),
        content_digest: json_digest(&json!({"body": "review"})),
        lifecycle: SkillLifecycle::Eligible,
        allowed_tools: BTreeSet::from(["memory.search".to_owned()]),
        activation_reason: Some("explicit".to_owned()),
        snapshot_id,
    };
    let hook = HookDescriptor {
        schema: HOOK_DESCRIPTOR_SCHEMA.to_owned(),
        hook_id: "hook.review".to_owned(),
        event: "PreToolUse".to_owned(),
        matcher: "memory.*".to_owned(),
        phase: HookPhase::Guard,
        source: source("hook"),
        snapshot_id,
    };
    let plugin = PluginLifecycle {
        schema: PLUGIN_LIFECYCLE_SCHEMA.to_owned(),
        extension_id: kiana_domain::ExtensionId::new(),
        version: "1.0.0".to_owned(),
        content_hash: "a".repeat(64),
        state: PluginLifecycleState::Enabled,
        revision: 1,
        reason: None,
    };
    ExtensionSnapshot::new(
        snapshot_id,
        1,
        vec![source("manifest")],
        vec![skill],
        vec![hook],
        vec![plugin],
        json_digest(&json!({"trust": "trusted"})),
    )
    .unwrap()
}

#[test]
fn extension_contracts_round_trip_and_reject_unknown_fields() {
    let snapshot = snapshot();
    let encoded = serde_json::to_value(&snapshot).unwrap();
    let decoded: ExtensionSnapshot = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, snapshot);
    assert_eq!(decoded.schema, EXTENSION_SNAPSHOT_SCHEMA);

    let mut forged = encoded;
    forged["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ExtensionSnapshot>(forged).is_err());

    let decision = HookDecision {
        schema: HOOK_DECISION_SCHEMA.to_owned(),
        hook_run_id: HookRunId::new(),
        hook_id: "hook.review".to_owned(),
        decision: HookDecisionKind::Block,
        phase: HookPhase::Guard,
        source: source("decision"),
        snapshot_id: snapshot.snapshot_id,
        input_digest: json_digest(&json!({"input": "memory.write"})),
        output_digest: None,
        reason: Some("policy denied".to_owned()),
    };
    decision.validate().unwrap();
}

#[test]
fn extension_snapshot_digest_and_duplicate_identity_fail_closed() {
    let mut original = snapshot();
    original.validate().unwrap();
    original.generation = 2;
    assert_eq!(
        original.validate().unwrap_err(),
        "extension_snapshot_digest_mismatch"
    );

    let mut duplicate = snapshot();
    duplicate.skills.push(duplicate.skills[0].clone());
    duplicate.snapshot_digest = duplicate.digest();
    assert_eq!(
        duplicate.validate().unwrap_err(),
        "extension_snapshot_skill_duplicate"
    );

    let mut invalidated = snapshot();
    invalidated.invalidate().unwrap();
    assert_eq!(invalidated.state, ExtensionSnapshotState::Invalidated);
    invalidated.validate().unwrap();
}

#[test]
fn extension_error_codes_are_stable_and_bounded() {
    assert_eq!(ExtensionErrorCode::ALL.len(), 14);
    assert_eq!(ExtensionErrorCode::ScopeDenied.as_str(), "scope_denied");
    let error = ExtensionError::new(
        ExtensionErrorCode::UntrustedSource,
        "project extension is not trusted",
        false,
    );
    assert_eq!(error.schema, EXTENSION_ERROR_SCHEMA);
    error.validate().unwrap();
    let mut forged = serde_json::to_value(error).unwrap();
    forged["secret"] = json!("must not pass");
    assert!(serde_json::from_value::<ExtensionError>(forged).is_err());
}
