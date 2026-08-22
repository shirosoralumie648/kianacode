use crate::types::{Command, Frontmatter, LoadedFrom, SettingSource};
use gray_matter::{engine::YAML, Matter};
use kiana_types::{project_trust_root, ProjectTrust};
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::fs;
use tracing::{debug, warn};

#[derive(Error, Debug)]
pub enum SkillLoadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Frontmatter parse error: {0}")]
    Frontmatter(String),
    #[error("Invalid skill format")]
    InvalidFormat,
}

pub async fn load_skills_from_dir(
    base_path: impl AsRef<Path>,
    source: SettingSource,
) -> Result<Vec<Command>, SkillLoadError> {
    let base_path = base_path.as_ref();
    let mut skills = Vec::new();

    let mut entries = match fs::read_dir(base_path).await {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(skills),
        Err(e) => return Err(e.into()),
    };

    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        if !metadata.is_dir() {
            continue;
        }

        let skill_dir = entry.path();
        let skill_file = skill_dir.join("SKILL.md");

        let content = match fs::read_to_string(&skill_file).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                warn!("Failed to read {:?}: {}", skill_file, e);
                continue;
            }
        };

        match parse_skill(&content, &skill_dir, source) {
            Ok(skill) => skills.push(skill),
            Err(e) => warn!("Failed to parse skill at {:?}: {}", skill_dir, e),
        }
    }

    debug!("Loaded {} skills from {:?}", skills.len(), base_path);
    Ok(skills)
}

pub(crate) async fn load_skill_from_root(
    skill_dir: impl AsRef<Path>,
    source: SettingSource,
) -> Result<Option<Command>, SkillLoadError> {
    let skill_dir = skill_dir.as_ref();
    let skill_file = skill_dir.join("SKILL.md");
    let content = match fs::read_to_string(&skill_file).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    parse_skill(&content, skill_dir, source).map(Some)
}

pub(crate) fn parse_skill(
    content: &str,
    skill_dir: &Path,
    source: SettingSource,
) -> Result<Command, SkillLoadError> {
    let matter = Matter::<YAML>::new();
    let parsed = matter.parse(content);

    let frontmatter: Frontmatter = if parsed.data.is_some() {
        // Extract the original YAML frontmatter text and parse directly
        if let Some(yaml_start) = content.find("---\n") {
            if let Some(yaml_end) = content[yaml_start + 4..].find("\n---") {
                let yaml_content = &content[yaml_start + 4..yaml_start + 4 + yaml_end];
                serde_yaml::from_str(yaml_content)
                    .map_err(|e| SkillLoadError::Frontmatter(e.to_string()))?
            } else {
                Frontmatter::default()
            }
        } else {
            Frontmatter::default()
        }
    } else {
        Frontmatter::default()
    };

    let skill_name = skill_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(SkillLoadError::InvalidFormat)?
        .to_string();

    let description = frontmatter
        .description
        .clone()
        .or_else(|| extract_description_from_markdown(&parsed.content))
        .unwrap_or_else(|| format!("Skill: {}", skill_name));

    Ok(Command {
        name: skill_name,
        display_name: frontmatter.name.clone(),
        description,
        when_to_use: frontmatter.when_to_use.clone(),
        argument_hint: frontmatter.argument_hint.clone(),
        allowed_tools: frontmatter.parse_allowed_tools(),
        model: frontmatter.model.clone(),
        disable_model_invocation: frontmatter.disable_model_invocation.unwrap_or(false),
        user_invocable: frontmatter.user_invocable.unwrap_or(true),
        source,
        loaded_from: LoadedFrom::Skills,
        skill_root: Some(skill_dir.to_path_buf()),
        context: frontmatter.parse_context(),
        paths: frontmatter.parse_paths(),
        content: parsed.content,
    })
}

fn extract_description_from_markdown(content: &str) -> Option<String> {
    content
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

pub async fn get_skill_dirs(cwd: impl AsRef<Path>) -> Vec<PathBuf> {
    let cwd = cwd.as_ref();
    let project_trust = kiana_types::read_project_trust(cwd)
        .ok()
        .flatten()
        .unwrap_or(ProjectTrust::Unknown);
    get_skill_dirs_with_trust(cwd, project_trust).await
}

pub async fn get_skill_dirs_with_trust(
    cwd: impl AsRef<Path>,
    project_trust: ProjectTrust,
) -> Vec<PathBuf> {
    let cwd = cwd.as_ref();
    let mut dirs = Vec::new();
    push_existing_dir(
        &mut dirs,
        home_dir().map(|home| home.join(".claude").join("skills")),
    );
    push_existing_dir(
        &mut dirs,
        env::var_os("KIANA_HOME").map(|home| PathBuf::from(home).join("skills")),
    );

    let mut project_dirs = Vec::new();
    if project_trust.allows_project_resources() {
        if let Ok(canonical_cwd) = cwd.canonicalize() {
            let project_root = project_trust_root(&canonical_cwd);
            let mut current = canonical_cwd.as_path();

            loop {
                for skills_dir in [
                    current.join(".claude").join("skills"),
                    current.join(".kiana").join("skills"),
                ] {
                    if skills_dir.exists() {
                        project_dirs.push(skills_dir);
                    }
                }
                if current == project_root {
                    break;
                }
                let Some(parent) = current.parent() else {
                    break;
                };
                current = parent;
            }
        }
    }

    project_dirs.reverse();
    dirs.extend(project_dirs);
    dedupe_dirs(dirs)
}

pub(crate) fn setting_source_for_skill_dir(dir: &Path) -> SettingSource {
    let dir_key = canonical_key(dir);
    if user_skill_dirs()
        .into_iter()
        .map(|dir| canonical_key(&dir))
        .any(|user_dir| user_dir == dir_key)
    {
        SettingSource::UserSettings
    } else {
        SettingSource::ProjectSettings
    }
}

fn user_skill_dirs() -> Vec<PathBuf> {
    [
        home_dir().map(|home| home.join(".claude").join("skills")),
        env::var_os("KIANA_HOME").map(|home| PathBuf::from(home).join("skills")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

fn push_existing_dir(dirs: &mut Vec<PathBuf>, dir: Option<PathBuf>) {
    if let Some(dir) = dir {
        if dir.is_dir() {
            dirs.push(dir);
        }
    }
}

fn dedupe_dirs(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    dirs.into_iter()
        .filter(|dir| seen.insert(canonical_key(dir)))
        .collect()
}

fn canonical_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}
