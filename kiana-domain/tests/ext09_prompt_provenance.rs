use kiana_domain::{
    PromptAuthority, PromptBudgetUsage, PromptBundle, PromptSection, RoleSpec,
    SkillPromptProvenance, SKILL_PROMPT_PROVENANCE_SCHEMA,
};

fn provenance(section_name: &str) -> SkillPromptProvenance {
    SkillPromptProvenance {
        schema: SKILL_PROMPT_PROVENANCE_SCHEMA.to_owned(),
        section_name: section_name.to_owned(),
        skill_id: "review".to_owned(),
        version: "legacy-command".to_owned(),
        content_hash: format!("sha256:{}", "a".repeat(64)),
        trust: "trusted".to_owned(),
        activation_reason: Some("catalog_eligible".to_owned()),
        budget: PromptBudgetUsage {
            budget_bytes: 16 * 1024,
            budget_tokens: 4 * 1024,
            used_bytes: 128,
            estimated_tokens: 32,
            truncated: false,
            omission_reason: None,
        },
        snapshot_id: format!("skill-snapshot:{}", "b".repeat(64)),
    }
}

#[test]
fn prompt_bundle_round_trips_skill_provenance_and_budget() {
    let mut bundle = PromptBundle::for_role(&RoleSpec::builder());
    bundle.sections.push(PromptSection {
        name: "skill:review".to_owned(),
        order: 300,
        text: "review body".to_owned(),
        source: "skill:review".to_owned(),
        authority: PromptAuthority::Context,
    });
    bundle.skill_provenance.push(provenance("skill:review"));
    bundle.validate().unwrap();
    let decoded = PromptBundle::decode(&bundle.encode().unwrap()).unwrap();
    assert_eq!(decoded.skill_provenance.len(), 1);
    assert!(decoded
        .provenance()
        .iter()
        .any(|item| item["section_name"] == "skill:review"));
}

#[test]
fn prompt_provenance_cannot_attach_to_product_or_unknown_sections() {
    let mut bundle = PromptBundle::for_role(&RoleSpec::builder());
    bundle.skill_provenance.push(provenance("missing"));
    assert_eq!(
        bundle.validate().unwrap_err(),
        "skill_prompt_provenance_section_invalid"
    );

    let mut bundle = PromptBundle::for_role(&RoleSpec::builder());
    bundle.skill_provenance.push(provenance("role"));
    assert_eq!(
        bundle.validate().unwrap_err(),
        "skill_prompt_provenance_section_invalid"
    );
}
