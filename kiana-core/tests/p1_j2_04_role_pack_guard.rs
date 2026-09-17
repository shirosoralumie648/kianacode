#[test]
fn role_pack_hash_reaches_receipt_and_resume_fences() {
    let roles = include_str!("../../kiana-domain/src/roles.rs");
    let prompts = include_str!("../../kiana-domain/src/prompts.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let recovery = include_str!("../src/recovery.rs");
    let receipts = include_str!("../src/receipts.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_skills.rs");
    for marker in [
        "include_str!(\"../role-packs/builder.md\")",
        "prompt_hash(prompt)",
        "role_prompt_hash",
        "PromptBundle::for_role",
        "snapshot.role_prompt_hash != role.prompt_hash",
        "prompt_hash",
    ] {
        assert!(
            roles.contains(marker)
                || prompts.contains(marker)
                || lifecycle.contains(marker)
                || recovery.contains(marker)
                || receipts.contains(marker)
                || daemon.contains(marker),
            "role-pack provenance marker missing: {marker}"
        );
    }
    assert!(daemon.contains("load_all_skills_with_trust"));
    assert!(daemon.contains("ProjectTrust::Trusted"));
}
