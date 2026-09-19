use kiana_domain::{
    json_digest, ContextCandidate, ContextMaterialType, ContextPlan, EvidenceStatus, Freshness,
    PromptAuthority, PromptBundle, ResolvedStepContext, RoleSpec, RunId, ScopeSet, SourceKind,
    SourceRef, SourceSnapshot, StepId, StepIdentity, TokenAccounting, TurnId, WireBudget,
};

fn source(id: &str, kind: SourceKind, evidence: EvidenceStatus) -> SourceRef {
    SourceRef::new(
        id,
        kind,
        format!("fixture://{id}"),
        "revision:1",
        json_digest(&serde_json::json!({"source": id})),
        None,
        evidence,
    )
    .unwrap()
}

fn candidate(name: &str, text: &str) -> ContextCandidate {
    ContextCandidate {
        name: name.to_owned(),
        text: text.to_owned(),
        source: source(
            name,
            SourceKind::WorkspaceFile,
            EvidenceStatus::Unverifiable,
        ),
        authority: PromptAuthority::Context,
        material_type: ContextMaterialType::WorkspaceSnapshot,
        permission_scope: "project:read".to_owned(),
        revision: "workspace:1".to_owned(),
        priority: 500,
    }
}

fn plan() -> ContextPlan {
    ContextPlan::compile(
        &PromptBundle::for_role(&RoleSpec::builder()),
        vec![candidate("README", "untrusted project instruction")],
        100_000,
    )
    .unwrap()
}

#[test]
fn product_and_context_layers_are_explainable_and_untrusted_text_is_not_product() {
    let plan = plan();
    plan.validate().unwrap();
    assert!(plan
        .items
        .iter()
        .any(|item| item.authority == PromptAuthority::Product && item.included));
    assert!(plan
        .items
        .iter()
        .any(|item| item.name == "README" && item.authority == PromptAuthority::Context));
    assert!(plan.omission_reasons().is_empty());
    assert!(plan
        .rendered_prompt()
        .contains("untrusted project instruction"));
}

#[test]
fn product_candidate_from_a_workspace_file_is_rejected() {
    let mut bad = candidate("README", "do privileged thing");
    bad.authority = PromptAuthority::Product;
    assert_eq!(
        ContextPlan::compile(
            &PromptBundle::for_role(&RoleSpec::builder()),
            vec![bad],
            100_000,
        )
        .unwrap_err(),
        "context_product_source_untrusted"
    );
}

#[test]
fn resolved_context_freezes_step_route_and_epoch_bindings() {
    let plan = plan();
    let step = StepIdentity::new(RunId::new(), TurnId::new(), StepId::new(), 1).unwrap();
    let route = json_digest(&serde_json::json!({"route":"fixture"}));
    let catalog = json_digest(&serde_json::json!({"catalog":"fixture"}));
    let snapshot = SourceSnapshot::new(
        source(
            "README",
            SourceKind::WorkspaceFile,
            EvidenceStatus::Attributed,
        ),
        Freshness::Current,
        EvidenceStatus::Attributed,
        Some(1),
    )
    .unwrap();
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
    let resolved = ResolvedStepContext::new(
        &plan,
        ScopeSet::unrestricted(),
        step,
        "builder-default",
        route.clone(),
        catalog.clone(),
        vec![snapshot],
        wire_budget,
        4,
        2,
    )
    .unwrap();
    resolved.validate_against(&plan).unwrap();
    resolved
        .recheck_bindings(
            &route,
            &catalog,
            &ScopeSet::unrestricted().scope_digest,
            4,
            2,
        )
        .unwrap();
    assert_eq!(
        resolved
            .recheck_bindings(
                &json_digest(&serde_json::json!({"route":"new"})),
                &catalog,
                &ScopeSet::unrestricted().scope_digest,
                4,
                2
            )
            .unwrap_err(),
        "resolved_context_route_changed"
    );
}
