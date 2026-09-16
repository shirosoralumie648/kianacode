//! One deny-first source/root resolver for Skills, Plugins and Hook resources.
//!
//! The resolver is intentionally separate from parsing and execution. It establishes a
//! canonical root, applies the project trust decision, and only then permits a relative resource
//! lookup. A `SourceRef` is a provenance value; it is never an authorization grant.

use kiana_domain::{
    json_digest, EvidenceStatus, SourceKind, SourceRef, EXTENSION_SOURCE_RESOLUTION_SCHEMA,
};
use kiana_types::{project_trust_root, ProjectTrust};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const SOURCE_RESOLUTION_SCHEMA: &str = EXTENSION_SOURCE_RESOLUTION_SCHEMA;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceRootKind {
    User,
    KianaHome,
    Project,
    Bundled,
    SignedPackage,
    PluginComponent,
    External,
}

impl SourceRootKind {
    fn source_kind(self) -> SourceKind {
        match self {
            Self::Project => SourceKind::WorkspaceFile,
            Self::Bundled | Self::SignedPackage | Self::PluginComponent => SourceKind::Artifact,
            Self::User | Self::KianaHome | Self::External => SourceKind::UserImport,
        }
    }

    fn precedence(self) -> u8 {
        match self {
            Self::Bundled => 10,
            Self::SignedPackage => 20,
            Self::PluginComponent => 30,
            Self::KianaHome => 40,
            Self::User => 50,
            Self::Project => 60,
            Self::External => 70,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTrust {
    Trusted,
    Untrusted,
    Unknown,
    Denied,
}

impl SourceTrust {
    fn for_kind(kind: SourceRootKind, project_trust: ProjectTrust) -> Self {
        if kind == SourceRootKind::Project {
            return match project_trust {
                ProjectTrust::Trusted => Self::Trusted,
                ProjectTrust::Untrusted => Self::Untrusted,
                ProjectTrust::Unknown => Self::Unknown,
            };
        }
        match kind {
            SourceRootKind::SignedPackage | SourceRootKind::Bundled => Self::Trusted,
            SourceRootKind::User | SourceRootKind::KianaHome | SourceRootKind::PluginComponent => {
                Self::Trusted
            }
            SourceRootKind::External => Self::Unknown,
            SourceRootKind::Project => unreachable!(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRootSummary {
    pub source_id: String,
    pub kind: SourceRootKind,
    pub trust: SourceTrust,
    pub precedence: u8,
    pub root_digest: String,
}

impl SourceRootSummary {
    pub fn validate(&self) -> Result<(), SourceResolveError> {
        if self.source_id.trim().is_empty()
            || self.source_id.len() > 256
            || self.source_id.contains('/')
            || !self.root_digest.starts_with("sha256:")
            || self.root_digest.len() != 71
        {
            return Err(SourceResolveError::RootInvalid(self.source_id.clone()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceResolutionSummary {
    pub schema: String,
    pub project_trust: SourceTrust,
    pub roots: Vec<SourceRootSummary>,
    pub root_set_digest: String,
}

impl SourceResolutionSummary {
    pub fn validate(&self) -> Result<(), SourceResolveError> {
        if self.schema != SOURCE_RESOLUTION_SCHEMA
            || self.roots.len() > 256
            || !self.root_set_digest.starts_with("sha256:")
            || self.root_set_digest.len() != 71
        {
            return Err(SourceResolveError::RootInvalid(
                "source_resolution_summary".to_owned(),
            ));
        }
        let mut ids = BTreeSet::new();
        for root in &self.roots {
            root.validate()?;
            if !ids.insert(root.source_id.clone()) {
                return Err(SourceResolveError::DuplicateRoot);
            }
        }
        let expected = json_digest(&json!({
            "project_trust": self.project_trust,
            "roots": self.roots.clone(),
        }));
        if expected != self.root_set_digest {
            return Err(SourceResolveError::RootInvalid(
                "source_resolution_digest".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedSourceRoot {
    pub summary: SourceRootSummary,
    canonical_root: PathBuf,
    source_ref: SourceRef,
}

impl ResolvedSourceRoot {
    pub fn path(&self) -> &Path {
        &self.canonical_root
    }

    pub fn source_ref(&self) -> &SourceRef {
        &self.source_ref
    }

    pub fn is_usable(&self) -> bool {
        self.summary.trust == SourceTrust::Trusted
    }
}

#[derive(Clone, Debug)]
pub struct SourceResolution {
    pub summary: SourceResolutionSummary,
    pub roots: Vec<ResolvedSourceRoot>,
}

impl SourceResolution {
    pub fn trusted_paths(&self) -> Vec<PathBuf> {
        self.roots
            .iter()
            .filter(|root| root.is_usable())
            .map(|root| root.canonical_root.clone())
            .collect()
    }

    pub fn source_refs(&self) -> Vec<SourceRef> {
        self.roots
            .iter()
            .filter(|root| root.is_usable())
            .map(|root| root.source_ref.clone())
            .collect()
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SourceResolveError {
    #[error("source_root_invalid:{0}")]
    RootInvalid(String),
    #[error("source_root_symlink:{0}")]
    RootSymlink(String),
    #[error("source_root_duplicate")]
    DuplicateRoot,
    #[error("source_project_untrusted")]
    ProjectUntrusted,
    #[error("source_resource_invalid:{0}")]
    ResourceInvalid(String),
    #[error("source_resource_symlink")]
    ResourceSymlink,
    #[error("source_resource_outside_root")]
    ResourceOutsideRoot,
}

#[derive(Clone, Debug)]
pub struct SourceResolver {
    cwd: PathBuf,
    project_trust: ProjectTrust,
    extra_roots: Vec<(SourceRootKind, PathBuf)>,
}

impl SourceResolver {
    pub fn new(cwd: impl AsRef<Path>, project_trust: ProjectTrust) -> Self {
        Self {
            cwd: cwd.as_ref().to_path_buf(),
            project_trust,
            extra_roots: Vec::new(),
        }
    }

    pub fn with_root(mut self, kind: SourceRootKind, root: impl Into<PathBuf>) -> Self {
        self.extra_roots.push((kind, root.into()));
        self
    }

    pub fn resolve(&self) -> Result<SourceResolution, SourceResolveError> {
        let cwd = canonical_directory(&self.cwd)?;
        let project_root = canonical_directory(&project_trust_root(&cwd))?;
        let mut candidates = Vec::new();
        if let Some(home) = home_dir() {
            candidates.push((SourceRootKind::User, home.join(".claude").join("skills")));
        }
        if let Some(home) = std::env::var_os("KIANA_HOME") {
            candidates.push((
                SourceRootKind::KianaHome,
                PathBuf::from(home).join("skills"),
            ));
        }
        let mut current = cwd.as_path();
        loop {
            for path in [
                current.join(".claude").join("skills"),
                current.join(".kiana").join("skills"),
            ] {
                if path.exists() {
                    candidates.push((SourceRootKind::Project, path));
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
        candidates.extend(self.extra_roots.iter().cloned());

        let mut roots = Vec::new();
        let mut seen = BTreeSet::new();
        for (kind, path) in candidates {
            if !path.exists() {
                continue;
            }
            let canonical = validate_root(&path)?;
            if !seen.insert(canonical.clone()) {
                return Err(SourceResolveError::DuplicateRoot);
            }
            let trust = SourceTrust::for_kind(kind, self.project_trust);
            let content_digest = root_content_digest(&canonical)?;
            let root_digest = json_digest(&json!({
                "kind": kind,
                "canonical_root": canonical.to_string_lossy(),
                "content": content_digest,
            }));
            // The public summary uses a digest-derived identity; the absolute root only stays in
            // the private resolver/source reference used by the loader.
            let source_id = format!(
                "extension-root:{kind:?}:{}",
                root_digest.trim_start_matches("sha256:")
            );
            let source_ref = SourceRef::new(
                source_id.clone(),
                kind.source_kind(),
                format!("root:{root_digest}"),
                "root:v1",
                root_digest.clone(),
                None,
                if trust == SourceTrust::Trusted {
                    EvidenceStatus::Attributed
                } else {
                    EvidenceStatus::Unverifiable
                },
            )
            .map_err(SourceResolveError::RootInvalid)?;
            roots.push(ResolvedSourceRoot {
                summary: SourceRootSummary {
                    source_id,
                    kind,
                    trust,
                    precedence: kind.precedence(),
                    root_digest,
                },
                canonical_root: canonical,
                source_ref,
            });
        }
        roots.sort_by(|left, right| {
            left.summary
                .precedence
                .cmp(&right.summary.precedence)
                .then_with(|| left.summary.source_id.cmp(&right.summary.source_id))
        });
        let summaries = roots
            .iter()
            .map(|root| root.summary.clone())
            .collect::<Vec<_>>();
        let project_trust = match self.project_trust {
            ProjectTrust::Trusted => SourceTrust::Trusted,
            ProjectTrust::Untrusted => SourceTrust::Untrusted,
            ProjectTrust::Unknown => SourceTrust::Unknown,
        };
        let summary = SourceResolutionSummary {
            schema: SOURCE_RESOLUTION_SCHEMA.to_owned(),
            project_trust,
            root_set_digest: json_digest(
                &json!({"project_trust": project_trust, "roots": summaries.clone()}),
            ),
            roots: summaries,
        };
        summary.validate()?;
        Ok(SourceResolution { summary, roots })
    }

    pub fn resolve_resource(
        root: impl AsRef<Path>,
        relative: &str,
    ) -> Result<PathBuf, SourceResolveError> {
        if relative.trim().is_empty()
            || Path::new(relative).is_absolute()
            || relative.contains('\\')
            || relative.chars().any(char::is_control)
            || Path::new(relative)
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        {
            return Err(SourceResolveError::ResourceInvalid(relative.to_owned()));
        }
        let root = validate_root(root.as_ref())?;
        let candidate = root.join(relative);
        let mut current = root.clone();
        for component in Path::new(relative).components() {
            let Component::Normal(name) = component else {
                return Err(SourceResolveError::ResourceInvalid(relative.to_owned()));
            };
            current.push(name);
            let metadata = std::fs::symlink_metadata(&current)
                .map_err(|_| SourceResolveError::ResourceInvalid(relative.to_owned()))?;
            if metadata.file_type().is_symlink() {
                return Err(SourceResolveError::ResourceSymlink);
            }
        }
        let resolved = candidate
            .canonicalize()
            .map_err(|_| SourceResolveError::ResourceInvalid(relative.to_owned()))?;
        if !resolved.starts_with(&root) {
            return Err(SourceResolveError::ResourceOutsideRoot);
        }
        Ok(resolved)
    }
}

pub fn validate_root(path: &Path) -> Result<PathBuf, SourceResolveError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(SourceResolveError::RootInvalid(path.display().to_string()));
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| SourceResolveError::RootInvalid(path.display().to_string()))?;
    if metadata.file_type().is_symlink() {
        return Err(SourceResolveError::RootSymlink(path.display().to_string()));
    }
    if !metadata.is_dir() {
        return Err(SourceResolveError::RootInvalid(path.display().to_string()));
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| SourceResolveError::RootInvalid(path.display().to_string()))?;
    if !canonical.is_dir() {
        return Err(SourceResolveError::RootInvalid(path.display().to_string()));
    }
    Ok(canonical)
}

fn canonical_directory(path: &Path) -> Result<PathBuf, SourceResolveError> {
    let canonical = path
        .canonicalize()
        .map_err(|_| SourceResolveError::RootInvalid(path.display().to_string()))?;
    if !canonical.is_dir() {
        return Err(SourceResolveError::RootInvalid(path.display().to_string()));
    }
    Ok(canonical)
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn root_content_digest(root: &Path) -> Result<String, SourceResolveError> {
    let mut files = Vec::new();
    collect_root_files(root, root, 0, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(json_digest(&json!(files)))
}

fn collect_root_files(
    root: &Path,
    current: &Path,
    depth: usize,
    files: &mut Vec<(String, String)>,
) -> Result<(), SourceResolveError> {
    if depth > 3 || files.len() > 2_048 {
        return Err(SourceResolveError::RootInvalid(root.display().to_string()));
    }
    let mut entries = std::fs::read_dir(current)
        .map_err(|_| SourceResolveError::RootInvalid(current.display().to_string()))?
        .flatten()
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| SourceResolveError::RootInvalid(path.display().to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(SourceResolveError::RootSymlink(path.display().to_string()));
        }
        if metadata.is_dir() {
            collect_root_files(root, &path, depth + 1, files)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| SourceResolveError::RootInvalid(path.display().to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        let digest = if metadata.len() <= 256 * 1024 {
            let mut bytes = Vec::new();
            std::fs::File::open(&path)
                .and_then(|mut file| file.read_to_end(&mut bytes))
                .map_err(|_| SourceResolveError::RootInvalid(path.display().to_string()))?;
            json_digest(&json!({"bytes": bytes}))
        } else {
            json_digest(&json!({"len": metadata.len()}))
        };
        files.push((relative, digest));
    }
    Ok(())
}
