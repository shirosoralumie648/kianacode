use kiana_domain::{prompt_hash, PromptBundle, RoleSpec};

#[test]
fn role_prompt_loads_from_the_role_pack() {
    let cases = [
        (
            RoleSpec::builder(),
            include_str!("../role-packs/builder.md"),
        ),
        (
            RoleSpec::analyst(),
            include_str!("../role-packs/analyst.md"),
        ),
        (RoleSpec::pm(), include_str!("../role-packs/pm.md")),
        (
            RoleSpec::architect(),
            include_str!("../role-packs/architect.md"),
        ),
        (
            RoleSpec::reviewer(),
            include_str!("../role-packs/reviewer.md"),
        ),
        (RoleSpec::qa(), include_str!("../role-packs/qa.md")),
        (
            RoleSpec::sponsor(),
            include_str!("../role-packs/sponsor.md"),
        ),
        (RoleSpec::closer(), include_str!("../role-packs/closer.md")),
        (
            RoleSpec::librarian(),
            include_str!("../role-packs/librarian.md"),
        ),
    ];
    for (role, pack) in cases {
        role.validate().unwrap();
        assert_eq!(role.prompt, pack, "role pack drift: {}", role.role_id);
        assert_eq!(
            role.prompt_hash,
            prompt_hash(pack),
            "hash drift: {}",
            role.role_id
        );
        let bundle = PromptBundle::for_role(&role);
        bundle.validate().unwrap();
        assert_eq!(bundle.role_prompt_hash, role.prompt_hash);
    }
}

#[test]
fn role_pack_hash_is_stable_across_bundle_round_trip() {
    let role = RoleSpec::builder();
    let encoded = PromptBundle::for_role(&role).encode().unwrap();
    let decoded = PromptBundle::decode(&encoded).unwrap();
    assert_eq!(decoded.role_prompt_hash, role.prompt_hash);
    assert_eq!(decoded.provenance()[1]["prompt_hash"], role.prompt_hash);
}
