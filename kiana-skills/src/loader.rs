//! Loads bounded `SKILL.md` documents into in-memory [`Command`] values.
//!
//! This layer does not decide whether a project root is trusted. The usual discovery path first
//! asks [`SourceResolver`] for roots allowed by `ProjectTrust`, then passes those roots here. The
//! `SettingSource` argument labels provenance in the returned command; it is not an authorization.
//!
//! Directory loading enumerates only direct child directories. Individual missing, unreadable or
//! malformed skill files can be skipped, while root validation, directory iteration and selected
//! resolver errors are returned. The single-root helper has a different contract: missing or
//! invalid `SKILL.md` is `Ok(None)`, but other read/parse failures remain errors.
//!
//! Parsing stores the skill body in `Command.content`; it does not execute the text or read
//! resources referenced by that text. Later disclosure and activation are separate operations.

use crate::source_resolver::{validate_root, SourceResolveError, SourceResolver};
use crate::types::{Command, Frontmatter, LoadedFrom, SettingSource};
use gray_matter::{engine::YAML, Matter};
use kiana_types::ProjectTrust;
use std::env;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::fs;
use tracing::{debug, warn};

/// Errors which the selected loader entry point does not skip locally.
#[derive(Error, Debug)]
pub enum SkillLoadError {
    /// Directory enumeration/entry metadata or single-root file I/O failed.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Frontmatter YAML or a bounded metadata field did not satisfy its schema.
    #[error("Frontmatter parse error: {0}")]
    Frontmatter(String),
    /// The document exceeded its byte limit or the directory had no usable UTF-8 basename.
    #[error("Invalid skill format")]
    InvalidFormat,
    /// Root or resource validation rejected the source path.
    #[error("Source resolution error: {0}")]
    Source(String),
}

/// Load skills from the direct child directories of `base_path`.
///
/// `source` is copied into each resulting `Command`; this function does not make a ProjectTrust
/// decision. The usual crate facade obtains allowed roots through `get_skill_dirs_with_trust` first.
/// A missing root returns an empty vector. A root symlink or any symlink among its direct entries
/// returns `Source`; non-directory entries are skipped and nested directories are not traversed.
///
/// Within a skill directory, missing/invalid `SKILL.md` entries are skipped. Non-NotFound read
/// errors and per-skill parse errors are logged and skipped so other entries can still load.
/// Directory iteration I/O and non-`ResourceInvalid` resolver errors propagate as `Err`.
pub async fn load_skills_from_dir(
    base_path: impl AsRef<Path>,
    source: SettingSource,
) -> Result<Vec<Command>, SkillLoadError> {
    let base_path = base_path.as_ref();
    let mut skills = Vec::new();

    match fs::symlink_metadata(base_path).await {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(SkillLoadError::Source("source_root_symlink".to_owned()));
        }
        Ok(_) => {
            validate_root(base_path).map_err(|error| SkillLoadError::Source(error.to_string()))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(skills),
        Err(error) => return Err(error.into()),
    }

    let mut entries = match fs::read_dir(base_path).await {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(skills),
        Err(e) => return Err(e.into()),
    };

    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        if file_type.is_symlink() {
            return Err(SkillLoadError::Source("source_resource_symlink".to_owned()));
        }
        if !file_type.is_dir() {
            continue;
        }

        let skill_dir = validate_root(&entry.path())
            .map_err(|error| SkillLoadError::Source(error.to_string()))?;
        let skill_file = match SourceResolver::resolve_resource(&skill_dir, "SKILL.md") {
            Ok(path) => path,
            Err(SourceResolveError::ResourceInvalid(_)) => continue,
            Err(error) => return Err(SkillLoadError::Source(error.to_string())),
        };

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

/// Load one skill from a caller-selected directory.
///
/// `source` labels the resulting command but does not verify the caller's trust decision. Invalid
/// roots and non-NotFound I/O/parse failures are errors. A missing root resource or a
/// `ResourceInvalid` `SKILL.md` resolves to `Ok(None)`, allowing plugin discovery to skip that entry.
pub(crate) async fn load_skill_from_root(
    skill_dir: impl AsRef<Path>,
    source: SettingSource,
) -> Result<Option<Command>, SkillLoadError> {
    let skill_dir = validate_root(skill_dir.as_ref())
        .map_err(|error| SkillLoadError::Source(error.to_string()))?;
    let skill_file = match SourceResolver::resolve_resource(&skill_dir, "SKILL.md") {
        Ok(path) => path,
        Err(SourceResolveError::ResourceInvalid(_)) => return Ok(None),
        Err(error) => return Err(SkillLoadError::Source(error.to_string())),
    };
    let content = match fs::read_to_string(&skill_file).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    parse_skill(&content, &skill_dir, source).map(Some)
}

/// Parse one already-read skill document into a `Command`; this function performs no file I/O or trust check.
///
/// The directory basename supplies `Command.name`; a validated `frontmatter.name` is retained only
/// as `display_name`. Missing frontmatter uses `Frontmatter::default()`, so the invocation flags
/// default to `disable_model_invocation = false` and `user_invocable = true`. When gray_matter detects
/// frontmatter, the original YAML block is deserialized into the strict `Frontmatter` struct, then
/// the parsed Markdown body is stored in `Command.content`. Description falls back to the first
/// non-empty body line and then to `Skill: {name}`.
pub(crate) fn parse_skill(
    content: &str,
    skill_dir: &Path,
    source: SettingSource,
) -> Result<Command, SkillLoadError> {
    if content.len() > 64 * 1024 {
        return Err(SkillLoadError::InvalidFormat);
    }
    let trimmed = content.trim_start();
    if trimmed.starts_with("---") && !trimmed[3..].contains("\n---") {
        return Err(SkillLoadError::Frontmatter(
            "frontmatter_terminator_missing".to_owned(),
        ));
    }
    let matter = Matter::<YAML>::new();
    let parsed = matter.parse(content);

    let frontmatter: Frontmatter = if parsed.data.is_some() {
        // Reparse the original YAML block as Frontmatter so Serde's deny_unknown_fields schema is enforced.
        if let Some(yaml_start) = content.find("---\n") {
            if let Some(yaml_end) = content[yaml_start + 4..].find("\n---") {
                let yaml_content = &content[yaml_start + 4..yaml_start + 4 + yaml_end];
                serde_yaml::from_str(yaml_content)
                    .map_err(|e| SkillLoadError::Frontmatter(e.to_string()))?
            } else {
                return Err(SkillLoadError::Frontmatter(
                    "frontmatter_terminator_missing".to_owned(),
                ));
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
    let skill_name = normalize_skill_name(&skill_name)?;
    validate_frontmatter(&frontmatter, parsed.content.as_bytes().len())?;

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

/// Public, side-effect-free text parser used by protocol/CI fixtures.
///
/// It does not inspect or validate whether `skill_dir` exists; its basename is used to create
/// `Command.name`, and the supplied path is copied to `Command.skill_root` for later consumers.
pub fn parse_skill_document(
    content: &str,
    skill_dir: impl AsRef<Path>,
    source: SettingSource,
) -> Result<Command, SkillLoadError> {
    parse_skill(content, skill_dir.as_ref(), source)
}

/// Normalize one directory or display-label string to the stable Agent Skills slug format.
///
/// `parse_skill` uses the normalized directory basename as `Command.name`; frontmatter labels are
/// checked with this helper but remain unchanged in `Command.display_name`.
pub fn normalize_skill_name(raw: &str) -> Result<String, SkillLoadError> {
    let mut normalized = String::new();
    let mut separator = false;
    for character in raw.trim().chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            if separator && !normalized.is_empty() {
                normalized.push('-');
            }
            normalized.push(character);
            separator = false;
        } else if matches!(character, '-' | '_' | ' ' | '.') || !character.is_ascii() {
            separator = true;
        } else {
            separator = true;
        }
    }
    if normalized.is_empty() || normalized.len() > 64 {
        return Err(SkillLoadError::Frontmatter("skill_name_invalid".to_owned()));
    }
    Ok(normalized)
}

/// Enforce byte, field-count and path constraints after deserialization; this validates metadata
/// shape only and does not decide whether the skill source is trusted or authorized.
fn validate_frontmatter(
    frontmatter: &Frontmatter,
    body_bytes: usize,
) -> Result<(), SkillLoadError> {
    if body_bytes > 64 * 1024 {
        return Err(SkillLoadError::InvalidFormat);
    }
    if let Some(name) = frontmatter.name.as_deref() {
        // Directory identity and the human-facing frontmatter label are separate. Validate both
        // forms, but do not require a display label to equal the directory slug.
        let _normalized = normalize_skill_name(name)?;
    }
    for (value, field, limit) in [
        (frontmatter.description.as_deref(), "description", 4_096),
        (frontmatter.when_to_use.as_deref(), "when_to_use", 2_048),
        (frontmatter.argument_hint.as_deref(), "argument_hint", 1_024),
        (frontmatter.model.as_deref(), "model", 128),
        (frontmatter.license.as_deref(), "license", 256),
        (frontmatter.compatibility.as_deref(), "compatibility", 512),
    ] {
        if value.is_some_and(|value| value.len() > limit || value.contains('\0')) {
            return Err(SkillLoadError::Frontmatter(format!(
                "skill_{field}_invalid"
            )));
        }
    }
    if frontmatter
        .context
        .as_deref()
        .is_some_and(|context| !matches!(context, "inline" | "fork"))
    {
        return Err(SkillLoadError::Frontmatter(
            "skill_context_invalid".to_owned(),
        ));
    }
    if frontmatter.allowed_tools.as_ref().is_some_and(|tools| {
        tools.len() > 32
            || tools
                .iter()
                .any(|tool| tool.trim().is_empty() || tool.len() > 128 || tool.contains('\0'))
    }) {
        return Err(SkillLoadError::Frontmatter(
            "skill_allowed_tools_invalid".to_owned(),
        ));
    }
    for values in [frontmatter.triggers.as_ref(), frontmatter.tools.as_ref()]
        .into_iter()
        .flatten()
    {
        if values.len() > 32
            || values
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 128 || value.contains('\0'))
        {
            return Err(SkillLoadError::Frontmatter(
                "skill_legacy_metadata_invalid".to_owned(),
            ));
        }
    }
    if let Some(paths) = frontmatter.paths.as_deref() {
        if paths.len() > 4_096
            || paths.lines().any(|path| {
                let path = path.trim();
                path.is_empty()
                    || path.starts_with('/')
                    || path.contains('\\')
                    || path.contains('\0')
                    || path.split('/').any(|part| part == "..")
            })
        {
            return Err(SkillLoadError::Frontmatter(
                "skill_paths_invalid".to_owned(),
            ));
        }
    }
    if let Some(metadata) = &frontmatter.metadata {
        if metadata.len() > 32
            || metadata
                .keys()
                .any(|key| key.trim().is_empty() || key.len() > 128 || key.contains('\0'))
        {
            return Err(SkillLoadError::Frontmatter(
                "skill_metadata_invalid".to_owned(),
            ));
        }
        for value in metadata.values() {
            validate_yaml_value(value, 0)?;
        }
    }
    Ok(())
}

/// Bound nested YAML metadata independently of serde's general parser limits: sequences/maps are
/// limited to 32 entries, strings to 1024 bytes, and recursive containers to depth 8.
fn validate_yaml_value(value: &serde_yaml::Value, depth: usize) -> Result<(), SkillLoadError> {
    if depth > 8 {
        return Err(SkillLoadError::Frontmatter(
            "skill_metadata_depth_invalid".to_owned(),
        ));
    }
    match value {
        serde_yaml::Value::String(value) if value.len() > 1_024 || value.contains('\0') => Err(
            SkillLoadError::Frontmatter("skill_metadata_value_invalid".to_owned()),
        ),
        serde_yaml::Value::Sequence(values) => {
            if values.len() > 32 {
                return Err(SkillLoadError::Frontmatter(
                    "skill_metadata_value_invalid".to_owned(),
                ));
            }
            values
                .iter()
                .try_for_each(|value| validate_yaml_value(value, depth + 1))
        }
        serde_yaml::Value::Mapping(values) => {
            if values.len() > 32 {
                return Err(SkillLoadError::Frontmatter(
                    "skill_metadata_value_invalid".to_owned(),
                ));
            }
            values
                .values()
                .try_for_each(|value| validate_yaml_value(value, depth + 1))
        }
        _ => Ok(()),
    }
}

/// Use the first non-empty parsed Markdown line as the fallback description; preserve it trimmed.
fn extract_description_from_markdown(content: &str) -> Option<String> {
    content
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

/// Discover skill roots using the project trust record stored for `cwd`.
///
/// Missing or unreadable trust data becomes `ProjectTrust::Unknown` before resolution; this helper
/// therefore does not treat an absent record as trusted. Resolver errors are collapsed to an empty
/// directory list by `get_skill_dirs_with_trust` below.
pub async fn get_skill_dirs(cwd: impl AsRef<Path>) -> Vec<PathBuf> {
    let cwd = cwd.as_ref();
    let project_trust = kiana_types::read_project_trust(cwd)
        .ok()
        .flatten()
        .unwrap_or(ProjectTrust::Unknown);
    get_skill_dirs_with_trust(cwd, project_trust).await
}

/// Return only roots accepted by `SourceResolver` for the supplied trust snapshot.
///
/// The API returns `Vec<PathBuf>` rather than a `Result`; any resolver error is converted to an
/// empty list here, so callers cannot distinguish a denied/invalid source tree from no skill roots.
pub async fn get_skill_dirs_with_trust(
    cwd: impl AsRef<Path>,
    project_trust: ProjectTrust,
) -> Vec<PathBuf> {
    SourceResolver::new(cwd, project_trust)
        .resolve()
        .map(|resolution| resolution.trusted_paths())
        .unwrap_or_default()
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

fn canonical_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}
