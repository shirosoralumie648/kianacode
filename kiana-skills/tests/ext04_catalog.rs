use kiana_skills::{build_skill_catalog, Command, LoadedFrom, SettingSource};
use std::path::PathBuf;

fn command(
    name: &str,
    body: &str,
    source: SettingSource,
    loaded_from: LoadedFrom,
    root: &str,
) -> Command {
    Command {
        name: name.to_owned(),
        display_name: None,
        description: "fixture".to_owned(),
        when_to_use: None,
        argument_hint: None,
        allowed_tools: Vec::new(),
        model: None,
        disable_model_invocation: false,
        user_invocable: true,
        source,
        loaded_from,
        skill_root: Some(PathBuf::from(root)),
        context: None,
        paths: None,
        content: body.to_owned(),
    }
}

#[test]
fn command_catalog_selection_is_stable_and_reports_shadowed_candidates() {
    let user = command(
        "same-skill",
        "user body",
        SettingSource::UserSettings,
        LoadedFrom::Skills,
        "/tmp/user-skills/same-skill",
    );
    let project = command(
        "same-skill",
        "project body",
        SettingSource::ProjectSettings,
        LoadedFrom::Skills,
        "/tmp/project-skills/same-skill",
    );
    let left = build_skill_catalog(&[project.clone(), user.clone()], 1).unwrap();
    let right = build_skill_catalog(&[user, project], 1).unwrap();
    assert_eq!(left.catalog, right.catalog);
    assert_eq!(left.selected.len(), 1);
    assert_eq!(left.selected[0].content, "user body");
    assert_eq!(left.shadowed.len(), 1);
    assert_eq!(left.shadowed[0].reason, "duplicate_identity_shadowed");
}
