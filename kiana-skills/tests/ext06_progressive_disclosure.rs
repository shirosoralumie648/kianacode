use kiana_skills::{
    activate_skill, list_skill_catalog, load_skill_body, read_skill_resource, search_skill_catalog,
    Command, DisclosureBudget, DisclosureError, LoadedFrom, SettingSource, SkillActivationRequest,
    SkillDisclosureStatus,
};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn skill(root: Option<PathBuf>, name: &str, description: &str, content: &str) -> Command {
    Command {
        name: name.to_owned(),
        display_name: Some(name.to_owned()),
        description: description.to_owned(),
        when_to_use: None,
        argument_hint: None,
        allowed_tools: Vec::new(),
        model: None,
        disable_model_invocation: false,
        user_invocable: true,
        source: SettingSource::ProjectSettings,
        loaded_from: LoadedFrom::Skills,
        skill_root: root,
        context: None,
        paths: None,
        content: content.to_owned(),
    }
}

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-ext06-{label}-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn catalog_and_search_return_metadata_without_body() {
    let commands = vec![skill(
        None,
        "review",
        "Review code changes",
        "private body that must not be exposed by catalog",
    )];
    let listed = list_skill_catalog(&commands);
    assert_eq!(listed.entries.len(), 1);
    assert_eq!(listed.entries[0].status, SkillDisclosureStatus::Eligible);
    let serialized = serde_json::to_string(&listed).unwrap();
    assert!(!serialized.contains("private body"));
    assert_eq!(
        search_skill_catalog(&commands, "review", 10).total_matches,
        1
    );
    assert_eq!(
        search_skill_catalog(&commands, "missing", 10).total_matches,
        0
    );
}

#[test]
fn body_load_is_complete_or_returns_over_budget() {
    let command = skill(None, "review", "Review", "0123456789");
    let body = load_skill_body(
        &command,
        &DisclosureBudget {
            max_bytes: 9,
            max_tokens: 3,
        },
    )
    .unwrap_err();
    assert!(matches!(
        body,
        DisclosureError::OverBudget {
            kind: "skill_body",
            ..
        }
    ));

    let body = load_skill_body(
        &command,
        &DisclosureBudget {
            max_bytes: 10,
            max_tokens: 4,
        },
    )
    .unwrap();
    assert!(body.quota.complete);
    assert_eq!(body.body, "0123456789");
}

#[tokio::test]
async fn resource_read_is_root_relative_and_quota_bound() {
    let root = temp_root("resource");
    fs::write(root.join("README.md"), "resource body").unwrap();
    fs::write(root.join("script.sh"), "#!/bin/sh\nprintf ok\n").unwrap();
    let command = skill(Some(root.clone()), "review", "Review", "body");
    let activation = activate_skill(
        &command,
        &SkillActivationRequest {
            snapshot_generation: 7,
            now_unix_ms: 100,
            expires_at_unix_ms: 200,
            reason: "explicit user activation".to_owned(),
        },
    )
    .unwrap();

    let resource = read_skill_resource(
        &command,
        &activation,
        "README.md",
        &DisclosureBudget {
            max_bytes: 64,
            max_tokens: 16,
        },
        150,
    )
    .await
    .unwrap();
    assert_eq!(resource.relative_path, "README.md");
    assert_eq!(resource.content, b"resource body");
    assert!(resource.package_hash.starts_with("sha256:"));
    assert!(resource.quota.complete);

    let denied = read_skill_resource(
        &command,
        &activation,
        "../outside.txt",
        &DisclosureBudget {
            max_bytes: 64,
            max_tokens: 16,
        },
        150,
    )
    .await
    .unwrap_err();
    assert!(matches!(denied, DisclosureError::ResourceInvalid(_)));

    let over_budget = read_skill_resource(
        &command,
        &activation,
        "script.sh",
        &DisclosureBudget {
            max_bytes: 4,
            max_tokens: 2,
        },
        150,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        over_budget,
        DisclosureError::OverBudget {
            kind: "skill_resource",
            ..
        }
    ));

    let expired = read_skill_resource(
        &command,
        &activation,
        "README.md",
        &DisclosureBudget {
            max_bytes: 64,
            max_tokens: 16,
        },
        200,
    )
    .await
    .unwrap_err();
    assert!(matches!(expired, DisclosureError::ActivationExpired));

    fs::remove_dir_all(Path::new(&root)).unwrap();
}
