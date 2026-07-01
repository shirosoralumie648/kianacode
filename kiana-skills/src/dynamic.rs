use crate::types::Command;
use ignore::gitignore::GitignoreBuilder;
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::RwLock;

static DYNAMIC_SKILLS: Lazy<RwLock<HashMap<String, Command>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

static CONDITIONAL_SKILLS: Lazy<RwLock<HashMap<String, Command>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

static ACTIVATED_SKILL_NAMES: Lazy<RwLock<HashSet<String>>> =
    Lazy::new(|| RwLock::new(HashSet::new()));

pub fn add_dynamic_skill(skill: Command) {
    let name = skill.name.clone();
    DYNAMIC_SKILLS.write().unwrap().insert(name, skill);
}

pub fn get_dynamic_skills() -> Vec<Command> {
    DYNAMIC_SKILLS.read().unwrap().values().cloned().collect()
}

pub fn store_conditional_skill(skill: Command) {
    let name = skill.name.clone();
    CONDITIONAL_SKILLS.write().unwrap().insert(name, skill);
}

pub fn activate_conditional_skills_for_paths(
    file_paths: &[impl AsRef<Path>],
    cwd: impl AsRef<Path>,
) -> Vec<String> {
    let cwd = cwd.as_ref();
    let mut activated = Vec::new();
    let mut conditional = CONDITIONAL_SKILLS.write().unwrap();
    let mut dynamic = DYNAMIC_SKILLS.write().unwrap();
    let mut activated_names = ACTIVATED_SKILL_NAMES.write().unwrap();

    let skills_to_check: Vec<_> = conditional
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    for (name, skill) in skills_to_check {
        let Some(patterns) = &skill.paths else {
            continue;
        };

        let mut builder = GitignoreBuilder::new(cwd);
        for pattern in patterns {
            let _ = builder.add_line(None, pattern);
        }

        let matcher = match builder.build() {
            Ok(m) => m,
            Err(_) => continue,
        };

        for file_path in file_paths {
            let path = file_path.as_ref();
            let rel_path = if path.is_absolute() {
                path.strip_prefix(cwd).unwrap_or(path)
            } else {
                path
            };

            if matcher.matched(rel_path, false).is_ignore() {
                if let Some(skill) = conditional.remove(&name) {
                    dynamic.insert(name.clone(), skill);
                    activated_names.insert(name.clone());
                    activated.push(name.clone());
                }
                break;
            }
        }
    }

    activated
}

pub fn clear_dynamic_skills() {
    DYNAMIC_SKILLS.write().unwrap().clear();
    CONDITIONAL_SKILLS.write().unwrap().clear();
    ACTIVATED_SKILL_NAMES.write().unwrap().clear();
}
