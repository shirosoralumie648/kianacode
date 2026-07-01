use crate::loader::{load_skill_from_root, load_skills_from_dir};
use crate::types::{Command, LoadedFrom, SettingSource};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::warn;

pub async fn load_plugin_skills() -> Vec<Command> {
    let mut all = Vec::new();
    for plugin in discover_plugins().await {
        for skills_dir in plugin.skill_dirs {
            match load_plugin_skills_from_path(&plugin.name, &skills_dir).await {
                Ok(skills) => all.extend(skills),
                Err(error) => warn!(
                    "Failed to load plugin skills from {:?}: {}",
                    skills_dir, error
                ),
            }
        }
    }
    all
}

pub async fn get_plugin_skill_dirs() -> Vec<PathBuf> {
    discover_plugins()
        .await
        .into_iter()
        .flat_map(|plugin| plugin.skill_dirs)
        .collect()
}

async fn load_plugin_skills_from_path(
    plugin_name: &str,
    skills_path: &Path,
) -> Result<Vec<Command>, crate::loader::SkillLoadError> {
    let mut skills = if skills_path.join("SKILL.md").is_file() {
        load_skill_from_root(skills_path, SettingSource::UserSettings)
            .await?
            .into_iter()
            .collect()
    } else {
        load_skills_from_dir(skills_path, SettingSource::UserSettings).await?
    };
    for skill in &mut skills {
        skill.name = format!("{}:{}", plugin_name, skill.name);
        skill.loaded_from = LoadedFrom::Plugin;
    }
    Ok(skills)
}

#[derive(Debug)]
struct PluginSkillSource {
    name: String,
    skill_dirs: Vec<PathBuf>,
}

async fn discover_plugins() -> Vec<PluginSkillSource> {
    let mut plugins = Vec::new();
    for plugin_root in kiana_types::plugin::installed_plugin_roots() {
        let Some(plugin) = read_plugin_skill_source(plugin_root).await else {
            continue;
        };
        if !plugin.skill_dirs.is_empty() {
            plugins.push(plugin);
        }
    }
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins
}

async fn read_plugin_skill_source(plugin_root: PathBuf) -> Option<PluginSkillSource> {
    let manifest_path = find_manifest_path(&plugin_root)?;
    let manifest = match fs::read_to_string(&manifest_path).await {
        Ok(contents) => serde_json::from_str::<Value>(&contents).ok(),
        Err(_) => None,
    };
    let name = manifest
        .as_ref()
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            plugin_root
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })?;
    let mut skill_dirs = Vec::new();
    push_existing_skill_dir(&mut skill_dirs, plugin_root.join("skills"));
    if let Some(manifest) = manifest.as_ref() {
        push_manifest_skill_dirs(&mut skill_dirs, &plugin_root, manifest);
    }
    dedupe_paths(&mut skill_dirs);
    Some(PluginSkillSource { name, skill_dirs })
}

fn push_manifest_skill_dirs(dirs: &mut Vec<PathBuf>, plugin_root: &Path, manifest: &Value) {
    let Some(skills) = manifest.get("skills") else {
        return;
    };
    match skills {
        Value::String(path) => push_existing_skill_dir(dirs, plugin_root.join(path)),
        Value::Array(paths) => {
            for path in paths {
                if let Some(path) = path.as_str() {
                    push_existing_skill_dir(dirs, plugin_root.join(path));
                }
            }
        }
        _ => {}
    }
}

fn push_existing_skill_dir(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if dir.join("SKILL.md").is_file() || dir.is_dir() {
        dirs.push(dir);
    }
}

fn dedupe_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|path| {
        seen.insert(
            path.canonicalize()
                .unwrap_or_else(|_| path.to_path_buf())
                .to_string_lossy()
                .to_string(),
        )
    });
}

fn find_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    [
        plugin_root.join(".codex-plugin").join("plugin.json"),
        plugin_root.join(".claude-plugin").join("plugin.json"),
        plugin_root.join("plugin.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}
