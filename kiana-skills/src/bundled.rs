use crate::types::{Command, ExecutionContext, LoadedFrom, SettingSource};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;

static BUNDLED_SKILLS: Lazy<RwLock<HashMap<String, BundledSkill>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub struct BundledSkill {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub argument_hint: Option<String>,
    pub allowed_tools: Vec<String>,
    pub model: Option<String>,
    pub disable_model_invocation: bool,
    pub user_invocable: bool,
    pub context: Option<ExecutionContext>,
    pub content: String,
}

pub fn register_bundled_skill(skill: BundledSkill) {
    let name = skill.name.clone();
    BUNDLED_SKILLS.write().unwrap().insert(name, skill);
}

pub fn get_bundled_skills() -> Vec<Command> {
    BUNDLED_SKILLS
        .read()
        .unwrap()
        .values()
        .map(|skill| Command {
            name: skill.name.clone(),
            display_name: None,
            description: skill.description.clone(),
            when_to_use: skill.when_to_use.clone(),
            argument_hint: skill.argument_hint.clone(),
            allowed_tools: skill.allowed_tools.clone(),
            model: skill.model.clone(),
            disable_model_invocation: skill.disable_model_invocation,
            user_invocable: skill.user_invocable,
            source: SettingSource::UserSettings,
            loaded_from: LoadedFrom::Bundled,
            skill_root: None,
            context: skill.context.clone(),
            paths: None,
            content: skill.content.clone(),
        })
        .collect()
}

pub fn clear_bundled_skills() {
    BUNDLED_SKILLS.write().unwrap().clear();
}
