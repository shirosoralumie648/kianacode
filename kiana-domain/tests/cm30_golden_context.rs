use kiana_domain::{
    json_digest, rank_retrieval, ContextPlan, GoldenContextCase, GoldenContextFixture,
    PromptBundle, RetrievalItem, RetrievalPack, RetrievalProfile, RetrievalRequest, RoleSpec,
};
use serde_json::json;
use std::collections::BTreeSet;

fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

fn fixture(plan: &ContextPlan, result: &kiana_domain::RetrievalResult) -> GoldenContextFixture {
    let case = GoldenContextCase::new(
        "cm30-all-languages",
        "登录 auth_token src/main.rs 2026-09-20",
        digest("scope"),
        BTreeSet::from([
            "english".to_owned(),
            "chinese".to_owned(),
            "cjk".to_owned(),
            "identifier".to_owned(),
            "path".to_owned(),
            "time".to_owned(),
            "acl".to_owned(),
        ]),
        result.hits.iter().map(|hit| hit.id.clone()).collect(),
        vec!["private:denied".to_owned()],
    )
    .unwrap();
    GoldenContextFixture::new(
        "fixture:cm30",
        digest("source-snapshot"),
        4,
        9,
        12,
        digest("algorithm:exact+bm25-cjk+dense+rrf60+mmr.v1"),
        digest("embedding:fixture-vectors@1"),
        plan.plan_digest.clone(),
        result.result_digest.clone(),
        vec![case],
    )
    .unwrap()
}

fn retrieval() -> (ContextPlan, kiana_domain::RetrievalResult) {
    let role = RoleSpec::pm();
    let bundle = PromptBundle::for_role(&role);
    let pack = RetrievalPack::new(digest("query"), 9, Vec::new()).unwrap();
    let plan = pack.compile_context(&bundle, 16_384).unwrap();
    let profile = RetrievalProfile::unified();
    let scope = digest("scope");
    let request = RetrievalRequest::new(
        "登录 auth_token src/main.rs 2026-09-20",
        scope.clone(),
        None,
    )
    .unwrap();
    let items = vec![
        RetrievalItem {
            id: "path:src/main.rs".to_owned(),
            text: "登录 auth_token src/main.rs 2026-09-20".to_owned(),
            source_digest: digest("main"),
            permission_scope_digest: scope.clone(),
            allowed: true,
            created_at_ms: 10,
            dense_vector: None,
        },
        RetrievalItem {
            id: "path:README.md".to_owned(),
            text: "English README with login guidance".to_owned(),
            source_digest: digest("readme"),
            permission_scope_digest: scope,
            allowed: true,
            created_at_ms: 11,
            dense_vector: None,
        },
    ];
    let result = rank_retrieval(&profile, &request, &items, 2).unwrap();
    (plan, result)
}

#[test]
fn golden_context_plan_is_byte_stable() {
    let (plan, result) = retrieval();
    let fixture = fixture(&plan, &result);
    fixture.validate_context_plan(&plan).unwrap();
    assert_eq!(
        fixture.canonical_bytes().unwrap(),
        fixture.canonical_bytes().unwrap()
    );
    fixture.validate().unwrap();
}

#[test]
fn golden_retrieval_has_reproducible_ranks() {
    let (plan, first) = retrieval();
    let fixture = fixture(&plan, &first);
    let (_, second) = retrieval();
    assert_eq!(first.result_digest, second.result_digest);
    fixture
        .validate_retrieval("cm30-all-languages", &second)
        .unwrap();
}
