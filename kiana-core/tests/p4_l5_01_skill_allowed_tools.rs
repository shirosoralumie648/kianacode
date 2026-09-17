#[test]
fn skill_allowed_tools_cannot_grant_shell() {
    let domain = include_str!("../../kiana-domain/src/extensions.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_skills.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let skills = include_str!("../../kiana-skills/src/types.rs");
    let baseline = include_str!("../../docs/roadmap/skills-plugins-hooks-baseline.md");
    let step = include_str!("../../docs/roadmap/p4-l5-01-extension-skills-baseline.md");

    for marker in [
        "allowed_tools",
        "allowed-tools",
        "display only",
        "does not grant tools or permission",
        "ExtensionExecutionContract",
        "extension_read_only_write_denied",
        "handler_effect",
        "required_capabilities",
        "request_extension_scopes",
        "admit_extensions",
        "extension_admission",
        "ControlPlane",
        "Broker",
        "skill_allowed_tools_cannot_grant_shell",
    ] {
        assert!(
            domain.contains(marker)
                || daemon.contains(marker)
                || broker.contains(marker)
                || skills.contains(marker)
                || baseline.contains(marker)
                || step.contains(marker),
            "skill authorization marker missing: {marker}"
        );
    }
    assert!(daemon.contains("PromptAuthority::Context"));
    assert!(daemon.contains("serde_json::to_string(&skill.allowed_tools)"));
    assert!(domain.contains("self.effect == ExtensionEffect::ReadOnly"));
    assert!(domain.contains("handler_effect != ExtensionEffect::ReadOnly"));
    assert!(broker.contains("self.admit_extensions(&request).await?"));
    assert!(!daemon.contains("allowed_tools.*authorize"));
    assert!(!broker.contains("allowed_tools"));
}
