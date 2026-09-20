use kiana_skills::{
    Command, DynamicSkillError, DynamicSkillScope, DynamicSkillStore, LoadedFrom, SettingSource,
    SkillInvocationRequest,
};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

fn temp_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-ext08-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
    root
}

fn conditional_skill() -> Command {
    Command {
        name: "review".to_owned(),
        display_name: Some("Review".to_owned()),
        description: "Review source".to_owned(),
        when_to_use: None,
        argument_hint: Some("--format <value>".to_owned()),
        allowed_tools: Vec::new(),
        model: None,
        disable_model_invocation: false,
        user_invocable: true,
        source: SettingSource::ProjectSettings,
        loaded_from: LoadedFrom::Skills,
        skill_root: None,
        context: None,
        paths: Some(vec!["src/**".to_owned()]),
        content: "body".to_owned(),
    }
}

#[test]
fn path_trigger_is_compiled_and_scoped_by_session_and_snapshot() {
    let root = temp_root();
    let store = DynamicSkillStore::default();
    let first = DynamicSkillScope::new("session-a", 7).unwrap();
    let second = DynamicSkillScope::new("session-b", 7).unwrap();
    let ast = store
        .store_conditional_skill_for_scope(&first, conditional_skill())
        .unwrap();
    assert!(ast.pattern_digest.starts_with("sha256:"));

    let receipts = store
        .activate_conditional_skills_for_paths_for_scope(
            &first,
            &[root.join("src/lib.rs")],
            &root,
            100,
            200,
        )
        .unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].reason, "path_trigger");
    assert_eq!(receipts[0].trigger_paths, vec!["src/lib.rs"]);
    assert_eq!(store.get_dynamic_skills_for_scope(&first).len(), 1);
    assert!(store.get_dynamic_skills_for_scope(&second).is_empty());

    let invocation = store
        .prepare_invocation(&SkillInvocationRequest {
            scope: first.clone(),
            skill_name: "review".to_owned(),
            argv: vec!["--format=json".to_owned()],
            now_unix_ms: 150,
        })
        .unwrap();
    assert!(invocation.args_digest.starts_with("sha256:"));

    let unknown = store
        .prepare_invocation(&SkillInvocationRequest {
            scope: first.clone(),
            skill_name: "review".to_owned(),
            argv: vec!["--unknown=value".to_owned()],
            now_unix_ms: 150,
        })
        .unwrap_err();
    assert!(matches!(
        unknown,
        DynamicSkillError::ArgumentsInvalid("unknown_argument")
    ));

    let revoked = store
        .revoke_skill_for_scope(&first, "review", "snapshot_invalidated")
        .unwrap();
    assert_eq!(
        revoked.status,
        kiana_skills::DynamicActivationStatus::Revoked
    );
    assert!(store.get_dynamic_skills_for_scope(&first).is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_path_patterns_fail_closed_before_storage() {
    let store = DynamicSkillStore::default();
    let scope = DynamicSkillScope::new("session", 1).unwrap();
    let mut skill = conditional_skill();
    skill.paths = Some(vec!["../escape".to_owned()]);
    assert!(matches!(
        store.store_conditional_skill_for_scope(&scope, skill),
        Err(DynamicSkillError::PathPatternInvalid)
    ));
}
