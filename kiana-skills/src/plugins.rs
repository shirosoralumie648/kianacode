use crate::loader::{load_skill_from_root, load_skills_from_dir};
use crate::types::{Command, LoadedFrom, SettingSource};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::warn;

pub async fn load_plugin_skills() -> Vec<Command> {
    load_plugin_skills_for_roots(kiana_types::plugin::installed_plugin_roots()).await
}

pub async fn load_plugin_skills_for_cwd(
    cwd: impl AsRef<Path>,
    project_trust: kiana_types::ProjectTrust,
) -> Vec<Command> {
    load_plugin_skills_for_roots(enabled_plugin_roots_for_cwd(cwd, project_trust)).await
}

async fn load_plugin_skills_for_roots(plugin_roots: Vec<PathBuf>) -> Vec<Command> {
    let mut all = Vec::new();
    for plugin in discover_plugin_skill_sources(plugin_roots).await {
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
    get_plugin_skill_dirs_for_roots(kiana_types::plugin::installed_plugin_roots()).await
}

pub async fn get_plugin_skill_dirs_for_cwd(
    cwd: impl AsRef<Path>,
    project_trust: kiana_types::ProjectTrust,
) -> Vec<PathBuf> {
    get_plugin_skill_dirs_for_roots(enabled_plugin_roots_for_cwd(cwd, project_trust)).await
}

async fn get_plugin_skill_dirs_for_roots(plugin_roots: Vec<PathBuf>) -> Vec<PathBuf> {
    discover_plugin_skill_sources(plugin_roots)
        .await
        .into_iter()
        .flat_map(|plugin| plugin.skill_dirs)
        .collect()
}

pub async fn plugin_skill_load_audit() -> PluginSkillLoadAudit {
    plugin_skill_load_audit_for_roots(kiana_types::plugin::installed_plugin_roots()).await
}

pub async fn plugin_skill_load_audit_for_cwd(
    cwd: impl AsRef<Path>,
    project_trust: kiana_types::ProjectTrust,
) -> PluginSkillLoadAudit {
    plugin_skill_load_audit_for_roots(enabled_plugin_roots_for_cwd(cwd, project_trust)).await
}

async fn plugin_skill_load_audit_for_roots(plugin_roots: Vec<PathBuf>) -> PluginSkillLoadAudit {
    let mut entries = Vec::new();
    let mut total_skills_loaded = 0usize;

    for entry in discover_plugin_skill_audit_entries(plugin_roots).await {
        if entry.status == PluginSkillLoadStatus::Discovered {
            let mut loaded_count = 0usize;
            let plugin_name = entry.plugin.clone();
            for skills_dir in &entry.skill_dirs {
                match load_plugin_skills_from_path(&plugin_name, skills_dir).await {
                    Ok(skills) => loaded_count += skills.len(),
                    Err(error) => entries.push(entry.with_load_error(skills_dir, &error)),
                }
            }
            let status = if loaded_count > 0 {
                PluginSkillLoadStatus::Loaded
            } else {
                PluginSkillLoadStatus::NoSkills
            };
            total_skills_loaded += loaded_count;
            entries.push(entry.with_loaded_count(status, loaded_count));
        } else {
            entries.push(entry);
        }
    }

    entries.sort_by(|a, b| a.plugin.cmp(&b.plugin).then_with(|| a.root.cmp(&b.root)));
    PluginSkillLoadAudit {
        schema: "kiana.plugin-skill-load-audit.v1",
        total_plugins: entries.len(),
        total_skills_loaded,
        entries,
    }
}

pub fn plugin_skill_cache_key_for_cwd(
    cwd: impl AsRef<Path>,
    project_trust: kiana_types::ProjectTrust,
) -> String {
    enabled_plugin_roots_for_cwd(cwd, project_trust)
        .into_iter()
        .map(|path| {
            path.canonicalize()
                .unwrap_or(path)
                .to_string_lossy()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("|")
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

async fn discover_plugin_skill_sources(plugin_roots: Vec<PathBuf>) -> Vec<PluginSkillSource> {
    let mut plugins = Vec::new();
    for plugin_root in plugin_roots {
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

async fn discover_plugin_skill_audit_entries(
    plugin_roots: Vec<PathBuf>,
) -> Vec<PluginSkillLoadAuditEntry> {
    let mut entries = Vec::new();
    for plugin_root in plugin_roots {
        entries.push(read_plugin_skill_audit_entry(plugin_root).await);
    }
    entries
}

fn enabled_plugin_roots_for_cwd(
    cwd: impl AsRef<Path>,
    project_trust: kiana_types::ProjectTrust,
) -> Vec<PathBuf> {
    let cwd = cwd.as_ref();
    let mut plugin_roots = Vec::new();
    let mut seen = BTreeSet::new();
    let roots = scoped_plugin_root_dirs(cwd, project_trust);
    for root in roots {
        for plugin_root in kiana_types::plugin::enabled_plugin_roots_in(&root) {
            let key = plugin_root
                .canonicalize()
                .unwrap_or_else(|_| plugin_root.clone())
                .to_string_lossy()
                .to_string();
            if seen.insert(key) {
                plugin_roots.push(plugin_root);
            }
        }
    }
    plugin_roots
}

fn scoped_plugin_root_dirs(cwd: &Path, project_trust: kiana_types::ProjectTrust) -> Vec<PathBuf> {
    let mut roots = kiana_types::plugin::user_plugins_dir()
        .into_iter()
        .collect::<Vec<_>>();
    if project_trust.allows_project_resources() {
        roots.push(cwd.join(".kiana").join("plugins"));
        roots.push(cwd.join(".kiana").join("plugins.local"));
    }
    roots
}

async fn read_plugin_skill_source(plugin_root: PathBuf) -> Option<PluginSkillSource> {
    let manifest_path = find_manifest_path(&plugin_root)?;
    let manifest = fs::read_to_string(&manifest_path)
        .await
        .ok()
        .and_then(|contents| serde_json::from_str::<Value>(&contents).ok())?;
    let name = manifest
        .get("name")
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
    push_manifest_skill_dirs(&mut skill_dirs, &plugin_root, &manifest);
    dedupe_paths(&mut skill_dirs);
    Some(PluginSkillSource { name, skill_dirs })
}

async fn read_plugin_skill_audit_entry(plugin_root: PathBuf) -> PluginSkillLoadAuditEntry {
    let root = path_string(&plugin_root);
    let fallback_plugin = fallback_plugin_name(&plugin_root);
    let Some(manifest_path) = find_manifest_path(&plugin_root) else {
        return PluginSkillLoadAuditEntry {
            plugin: fallback_plugin,
            root,
            manifest_path: None,
            status: PluginSkillLoadStatus::Error,
            skills_loaded: 0,
            skill_dirs: Vec::new(),
            warnings: Vec::new(),
            error: Some(PluginSkillLoadError {
                error_type: "manifest-not-found".to_string(),
                manifest_path: None,
                message: "plugin manifest was not found".to_string(),
                skill_dir: None,
            }),
        };
    };
    let manifest_path_string = path_string(&manifest_path);
    let contents = match fs::read_to_string(&manifest_path).await {
        Ok(contents) => contents,
        Err(error) => {
            return PluginSkillLoadAuditEntry {
                plugin: fallback_plugin,
                root,
                manifest_path: Some(manifest_path_string.clone()),
                status: PluginSkillLoadStatus::Error,
                skills_loaded: 0,
                skill_dirs: Vec::new(),
                warnings: Vec::new(),
                error: Some(PluginSkillLoadError {
                    error_type: "manifest-read-error".to_string(),
                    manifest_path: Some(manifest_path_string),
                    message: error.to_string(),
                    skill_dir: None,
                }),
            };
        }
    };
    let manifest = match serde_json::from_str::<Value>(&contents) {
        Ok(manifest) => manifest,
        Err(error) => {
            return PluginSkillLoadAuditEntry {
                plugin: fallback_plugin,
                root,
                manifest_path: Some(manifest_path_string.clone()),
                status: PluginSkillLoadStatus::Error,
                skills_loaded: 0,
                skill_dirs: Vec::new(),
                warnings: Vec::new(),
                error: Some(PluginSkillLoadError {
                    error_type: "manifest-parse-error".to_string(),
                    manifest_path: Some(manifest_path_string),
                    message: error.to_string(),
                    skill_dir: None,
                }),
            };
        }
    };

    let plugin = manifest
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or(fallback_plugin);
    let mut skill_dirs = Vec::new();
    push_existing_skill_dir(&mut skill_dirs, plugin_root.join("skills"));
    push_manifest_skill_dirs(&mut skill_dirs, &plugin_root, &manifest);
    dedupe_paths(&mut skill_dirs);
    let mut warnings = Vec::new();
    if skill_dirs.is_empty() {
        warnings.push("no skill directories found".to_string());
    }

    PluginSkillLoadAuditEntry {
        plugin,
        root,
        manifest_path: Some(manifest_path_string),
        status: PluginSkillLoadStatus::Discovered,
        skills_loaded: 0,
        skill_dirs,
        warnings,
        error: None,
    }
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

fn fallback_plugin_name(plugin_root: &Path) -> String {
    plugin_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown-plugin")
        .to_string()
}

fn path_string(path: &Path) -> String {
    let value = path.to_string_lossy();
    let normalized = if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        value.into_owned()
    };
    normalized.replace('\\', "/")
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginSkillLoadAudit {
    pub schema: &'static str,
    pub total_plugins: usize,
    pub total_skills_loaded: usize,
    pub entries: Vec<PluginSkillLoadAuditEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginSkillLoadAuditEntry {
    pub plugin: String,
    pub root: String,
    pub manifest_path: Option<String>,
    pub status: PluginSkillLoadStatus,
    pub skills_loaded: usize,
    #[serde(skip_serializing)]
    pub skill_dirs: Vec<PathBuf>,
    pub warnings: Vec<String>,
    pub error: Option<PluginSkillLoadError>,
}

impl PluginSkillLoadAuditEntry {
    fn with_loaded_count(mut self, status: PluginSkillLoadStatus, skills_loaded: usize) -> Self {
        self.status = status;
        self.skills_loaded = skills_loaded;
        self
    }

    fn with_load_error(
        &self,
        skill_dir: &Path,
        error: &crate::loader::SkillLoadError,
    ) -> PluginSkillLoadAuditEntry {
        let mut entry = self.clone();
        entry.status = PluginSkillLoadStatus::Error;
        entry.skills_loaded = 0;
        entry.error = Some(PluginSkillLoadError {
            error_type: "skill-load-error".to_string(),
            manifest_path: entry.manifest_path.clone(),
            message: error.to_string(),
            skill_dir: Some(path_string(skill_dir)),
        });
        entry
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginSkillLoadStatus {
    Discovered,
    Loaded,
    NoSkills,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginSkillLoadError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub manifest_path: Option<String>,
    pub message: String,
    pub skill_dir: Option<String>,
}
