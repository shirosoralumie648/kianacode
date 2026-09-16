use kiana_skills::{
    component_index, normalize_skill_name, parse_hook_manifest, parse_plugin_manifest,
    parse_skill_document, HookEntry, PluginComponentKind, SettingSource, SkillLoadError,
    NORMALIZED_HOOK_MANIFEST_SCHEMA, NORMALIZED_PLUGIN_MANIFEST_SCHEMA,
};
use serde_json::json;
use std::path::Path;

#[test]
fn skill_frontmatter_is_strict_and_names_are_normalized() {
    let content = r#"---
name: "Display Name"
description: "A bounded skill"
version: "1.2.0"
license: MIT
compatibility: "kiana"
allowed-tools: "memory.search memory.write"
metadata:
  owner: platform
---
# Body
"#;
    let skill = parse_skill_document(
        content,
        Path::new("/tmp/my_skill"),
        SettingSource::ProjectSettings,
    )
    .unwrap();
    assert_eq!(skill.name, "my-skill");
    assert_eq!(skill.display_name.as_deref(), Some("Display Name"));
    assert_eq!(
        skill.allowed_tools,
        vec!["memory.search".to_owned(), "memory.write".to_owned()]
    );
    assert_eq!(normalize_skill_name("My Skill").unwrap(), "my-skill");

    let unknown = "---\nunknown: true\n---\nbody\n";
    assert!(matches!(
        parse_skill_document(unknown, "/tmp/strict-skill", SettingSource::UserSettings),
        Err(SkillLoadError::Frontmatter(_))
    ));
    assert!(normalize_skill_name("!!!").is_err());
}

#[test]
fn strict_plugin_manifest_rejects_unknown_and_duplicate_components() {
    let valid = json!({
        "schema": NORMALIZED_PLUGIN_MANIFEST_SCHEMA,
        "name": "review-plugin",
        "version": "1.0.0",
        "components": [
            {"id":"review-skill","kind":"skills","entry":"skills/review"},
            {"id":"review-hook","kind":"hooks","entry":"hooks/review.json"}
        ]
    });
    let manifest = parse_plugin_manifest(&valid.to_string()).unwrap();
    assert!(!manifest.legacy_adapter);
    assert_eq!(manifest.components.len(), 2);
    assert_eq!(component_index(&manifest).len(), 2);

    let mut unknown = valid.clone();
    unknown["unexpected"] = json!(true);
    assert!(parse_plugin_manifest(&unknown.to_string()).is_err());

    let mut duplicate = valid;
    duplicate["components"][1]["id"] = json!("review-skill");
    assert!(parse_plugin_manifest(&duplicate.to_string()).is_err());
}

#[test]
fn legacy_plugin_and_hook_manifests_require_explicit_adapter_and_entry() {
    let legacy = json!({
        "name": "legacy-plugin",
        "version": "0.9.0",
        "skills": ["skills/review"],
        "hooks": ["hooks/review.json"]
    });
    let manifest = parse_plugin_manifest(&legacy.to_string()).unwrap();
    assert!(manifest.legacy_adapter);
    assert!(manifest
        .components
        .iter()
        .any(|component| component.kind == PluginComponentKind::Skills));

    let hook = json!({
        "schema": NORMALIZED_HOOK_MANIFEST_SCHEMA,
        "hooks": [{"id":"review","event":"PreToolUse","matcher":"memory.*","phase":"guard","entry":"hooks/review.json"}]
    });
    let parsed = parse_hook_manifest(&hook.to_string()).unwrap();
    assert!(!parsed.legacy_adapter);
    assert_eq!(parsed.hooks[0].id, "review");

    let missing_entry = json!({
        "schema": NORMALIZED_HOOK_MANIFEST_SCHEMA,
        "hooks": [{"id":"review","event":"PreToolUse","matcher":"memory.*","phase":"guard"}]
    });
    assert!(parse_hook_manifest(&missing_entry.to_string()).is_err());
    let _ = HookEntry {
        id: "fixture".to_owned(),
        event: "PreToolUse".to_owned(),
        matcher: "*".to_owned(),
        phase: "observer".to_owned(),
        entry: "hooks/fixture".to_owned(),
    };
}
