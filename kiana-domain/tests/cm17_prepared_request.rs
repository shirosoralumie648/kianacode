use kiana_domain::{
    json_digest, ContextCandidate, ContextMaterialType, ContextPlan, EvidenceStatus, Freshness,
    PreparedModelRequest, PromptAuthority, PromptBundle, RoleSpec, RunId, ScopeSet, SourceKind,
    SourceRef, SourceSnapshot, StepId, StepIdentity, TokenAccounting, TurnId, WireBudget,
};

fn digest(value: &str) -> String {
    json_digest(&serde_json::json!({"value": value}))
}

fn source(id: &str, kind: SourceKind) -> SourceRef {
    SourceRef::new(
        id,
        kind,
        format!("fixture://{id}"),
        "revision:1",
        digest(id),
        None,
        EvidenceStatus::Attributed,
    )
    .unwrap()
}

fn plan() -> ContextPlan {
    ContextPlan::compile(
        &PromptBundle::for_role(&RoleSpec::builder()),
        vec![ContextCandidate {
            name: "workspace".to_owned(),
            text: "the frozen workspace snapshot".to_owned(),
            source: source("workspace", SourceKind::WorkspaceFile),
            authority: PromptAuthority::Context,
            material_type: ContextMaterialType::WorkspaceSnapshot,
            permission_scope: "project:read".to_owned(),
            revision: "revision:1".to_owned(),
            priority: 1,
        }],
        100_000,
    )
    .unwrap()
}

fn prepared() -> PreparedModelRequest {
    let plan = plan();
    let snapshots = vec![SourceSnapshot::new(
        source("workspace", SourceKind::WorkspaceFile),
        Freshness::Current,
        EvidenceStatus::Attributed,
        Some(1),
    )
    .unwrap()];
    let route_digest = digest("route");
    let catalog_digest = digest("tools");
    let wire_budget = WireBudget::from_final_wire(
        plan.rendered_prompt().len() as u64,
        64,
        128,
        100_000,
        TokenAccounting::ConservativeUtf8 {
            bytes_per_token: 1,
            safety_margin_tokens: 1,
        },
    )
    .unwrap();
    PreparedModelRequest::new(
        &plan,
        ScopeSet::unrestricted(),
        StepIdentity::new(RunId::new(), TurnId::new(), StepId::new(), 1).unwrap(),
        "builder-default",
        route_digest,
        catalog_digest,
        snapshots,
        wire_budget,
        4,
        2,
    )
    .unwrap()
}

#[test]
fn same_step_snapshot_renders_same_wire_and_provenance() {
    let prepared = prepared();
    prepared.validate_against(&prepared.context_plan).unwrap();
    assert_eq!(
        prepared.rendered_prompt(),
        prepared.context_plan.rendered_prompt()
    );
    assert_eq!(
        prepared.prompt_bundle_digest,
        prepared.context_plan.prompt_bundle_digest
    );
    assert_eq!(prepared.source_snapshots.len(), 1);
    assert_eq!(prepared.wire_budget, prepared.wire_budget);
}

#[test]
fn route_change_between_prepare_and_send_is_fenced() {
    let prepared = prepared();
    assert_eq!(
        prepared
            .recheck_bindings(
                &digest("new-route"),
                &prepared.catalog_digest,
                &prepared.scope.scope_digest,
                prepared.workspace_revision,
                prepared.data_epoch,
            )
            .unwrap_err(),
        "resolved_context_route_changed"
    );
    assert_eq!(
        prepared
            .recheck_bindings(
                &prepared.route_digest,
                &prepared.catalog_digest,
                &digest("different-scope"),
                prepared.workspace_revision,
                prepared.data_epoch,
            )
            .unwrap_err(),
        "resolved_context_scope_changed"
    );
}
