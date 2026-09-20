use kiana_skills::{
    activate_skill, read_skill_resource, Command, DisclosureBudget, DisclosureError, LoadedFrom,
    SettingSource, SkillActivationRequest, SkillActivationStatus,
};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

fn temp_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-ext07-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn skill(root: PathBuf) -> Command {
    Command {
        name: "review".to_owned(),
        display_name: Some("Review".to_owned()),
        description: "Review code".to_owned(),
        when_to_use: None,
        argument_hint: None,
        allowed_tools: Vec::new(),
        model: None,
        disable_model_invocation: false,
        user_invocable: true,
        source: SettingSource::ProjectSettings,
        loaded_from: LoadedFrom::Skills,
        skill_root: Some(root),
        context: None,
        paths: None,
        content: "body".to_owned(),
    }
}

#[tokio::test]
async fn resource_read_requires_live_activation_and_exact_package_hash() {
    let root = temp_root();
    fs::write(root.join("README.md"), "resource").unwrap();
    let command = skill(root.clone());
    let activation = activate_skill(
        &command,
        &SkillActivationRequest {
            snapshot_generation: 3,
            now_unix_ms: 10,
            expires_at_unix_ms: 100,
            reason: "explicit activation".to_owned(),
        },
    )
    .unwrap();

    let mut tampered = activation.clone();
    tampered.package_hash = "sha256:tampered".to_owned();
    let result = read_skill_resource(
        &command,
        &tampered,
        "README.md",
        &DisclosureBudget {
            max_bytes: 64,
            max_tokens: 16,
        },
        20,
    )
    .await
    .unwrap_err();
    assert!(matches!(result, DisclosureError::ActivationInvalid));

    let mut revoked = activation.clone();
    revoked.status = SkillActivationStatus::Revoked;
    let result = read_skill_resource(
        &command,
        &revoked,
        "README.md",
        &DisclosureBudget {
            max_bytes: 64,
            max_tokens: 16,
        },
        20,
    )
    .await
    .unwrap_err();
    assert!(matches!(result, DisclosureError::ActivationInvalid));

    fs::remove_dir_all(root).unwrap();
}
