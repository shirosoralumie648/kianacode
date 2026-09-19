//! Incremental index invalidation and cache-key contracts.
//!
//! Changes are derived from source snapshot identity/content evidence, not mtime alone. Rename,
//! delete and revoke paths produce explicit invalidation/tombstone evidence for the next index
//! generation; they do not mutate an index or create an execution authority.

use crate::{
    json_digest, SchemaVersion, WorkspaceFileSnapshot, WorkspaceReadDisposition, WorkspaceSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const INDEX_CACHE_KEY_SCHEMA: &str = "kiana.index-cache-key.v1";
pub const INDEX_INVALIDATION_SCHEMA: &str = "kiana.index-invalidation-plan.v1";
pub const INDEX_INVALIDATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_INDEX_CHANGES: usize = 4_096;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn path(value: &str, field: &str) -> Result<(), String> {
    required(value, field, 4_096)?;
    if value.starts_with('/')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceChangeKind {
    Added,
    Changed,
    Removed,
    Renamed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceChange {
    pub schema: String,
    pub kind: WorkspaceChangeKind,
    pub path: String,
    #[serde(default)]
    pub previous_path: Option<String>,
    #[serde(default)]
    pub before_digest: Option<String>,
    #[serde(default)]
    pub after_digest: Option<String>,
    pub change_digest: String,
}

impl WorkspaceChange {
    fn new(
        kind: WorkspaceChangeKind,
        path: String,
        previous_path: Option<String>,
        before_digest: Option<String>,
        after_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut change = Self {
            schema: INDEX_INVALIDATION_SCHEMA.to_owned(),
            kind,
            path,
            previous_path,
            before_digest,
            after_digest,
            change_digest: String::new(),
        };
        change.change_digest = change.digest();
        change.validate()?;
        Ok(change)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INDEX_INVALIDATION_SCHEMA {
            return Err("workspace_change_schema_invalid".to_owned());
        }
        path(&self.path, "workspace_change_path")?;
        if let Some(previous_path) = &self.previous_path {
            path(previous_path, "workspace_change_previous_path")?;
        }
        for (value, field) in [
            (&self.before_digest, "workspace_change_before_digest"),
            (&self.after_digest, "workspace_change_after_digest"),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        match self.kind {
            WorkspaceChangeKind::Added if self.after_digest.is_none() => {
                return Err("workspace_change_after_digest_required".to_owned())
            }
            WorkspaceChangeKind::Changed
                if self.before_digest.is_none() || self.after_digest.is_none() =>
            {
                return Err("workspace_change_digest_pair_required".to_owned())
            }
            WorkspaceChangeKind::Removed if self.before_digest.is_none() => {
                return Err("workspace_change_before_digest_required".to_owned())
            }
            WorkspaceChangeKind::Renamed
                if self.previous_path.is_none()
                    || self.before_digest.is_none()
                    || self.after_digest.is_none() =>
            {
                return Err("workspace_change_rename_evidence_required".to_owned())
            }
            _ => {}
        }
        if self.kind == WorkspaceChangeKind::Renamed
            && self.previous_path.as_deref() == Some(self.path.as_str())
        {
            return Err("workspace_change_rename_same_path".to_owned());
        }
        digest(&self.change_digest, "workspace_change_digest")?;
        if self.change_digest != self.digest() {
            return Err("workspace_change_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "kind": self.kind,
            "path": self.path,
            "previous_path": self.previous_path,
            "before_digest": self.before_digest,
            "after_digest": self.after_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexCacheKey {
    pub schema: String,
    pub root_digest: String,
    pub worktree_digest: String,
    pub branch: String,
    pub dirty_manifest_digest: String,
    pub parser_digest: String,
    pub chunker_digest: String,
    pub config_digest: String,
    pub key_digest: String,
}

impl IndexCacheKey {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        root_digest: impl Into<String>,
        worktree_digest: impl Into<String>,
        branch: impl Into<String>,
        dirty_manifest_digest: impl Into<String>,
        parser_digest: impl Into<String>,
        chunker_digest: impl Into<String>,
        config_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut key = Self {
            schema: INDEX_CACHE_KEY_SCHEMA.to_owned(),
            root_digest: root_digest.into(),
            worktree_digest: worktree_digest.into(),
            branch: branch.into(),
            dirty_manifest_digest: dirty_manifest_digest.into(),
            parser_digest: parser_digest.into(),
            chunker_digest: chunker_digest.into(),
            config_digest: config_digest.into(),
            key_digest: String::new(),
        };
        key.key_digest = key.digest();
        key.validate()?;
        Ok(key)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INDEX_CACHE_KEY_SCHEMA {
            return Err("index_cache_key_schema_invalid".to_owned());
        }
        for (value, field) in [
            (&self.root_digest, "index_cache_root_digest"),
            (&self.worktree_digest, "index_cache_worktree_digest"),
            (
                &self.dirty_manifest_digest,
                "index_cache_dirty_manifest_digest",
            ),
            (&self.parser_digest, "index_cache_parser_digest"),
            (&self.chunker_digest, "index_cache_chunker_digest"),
            (&self.config_digest, "index_cache_config_digest"),
        ] {
            digest(value, field)?;
        }
        required(&self.branch, "index_cache_branch", 256)?;
        digest(&self.key_digest, "index_cache_key_digest")?;
        if self.key_digest != self.digest() {
            return Err("index_cache_key_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "root_digest": self.root_digest,
            "worktree_digest": self.worktree_digest,
            "branch": self.branch,
            "dirty_manifest_digest": self.dirty_manifest_digest,
            "parser_digest": self.parser_digest,
            "chunker_digest": self.chunker_digest,
            "config_digest": self.config_digest,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexInvalidationPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_generation: u64,
    pub target_generation: u64,
    #[serde(default)]
    pub source_snapshot_digest: Option<String>,
    pub target_snapshot_digest: String,
    pub changes: Vec<WorkspaceChange>,
    pub invalidated_paths: Vec<String>,
    pub tombstones: Vec<String>,
    pub plan_digest: String,
}

impl IndexInvalidationPlan {
    pub fn from_snapshots(
        previous: Option<&WorkspaceSnapshot>,
        current: &WorkspaceSnapshot,
        source_generation: u64,
        target_generation: u64,
    ) -> Result<Self, String> {
        current.validate()?;
        if target_generation <= source_generation {
            return Err("index_invalidation_generation_order_invalid".to_owned());
        }
        let previous_files = previous
            .map(|snapshot| {
                snapshot
                    .files
                    .iter()
                    .map(|file| (file.path.clone(), file))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let current_files = current
            .files
            .iter()
            .map(|file| (file.path.clone(), file))
            .collect::<BTreeMap<_, _>>();
        let mut removed = previous_files
            .iter()
            .filter(|(path, _)| !current_files.contains_key(*path))
            .map(|(path, file)| (path.to_string(), effective_digest(file)))
            .collect::<Vec<_>>();
        let mut added = current_files
            .iter()
            .filter(|(path, _)| !previous_files.contains_key(*path))
            .map(|(path, file)| (path.to_string(), effective_digest(file)))
            .collect::<Vec<_>>();
        removed.sort_by(|left, right| left.0.cmp(&right.0));
        added.sort_by(|left, right| left.0.cmp(&right.0));
        let mut changes = Vec::new();
        let mut consumed_added = BTreeSet::new();
        for (old_path, before_digest) in &removed {
            if let Some((new_path, after_digest)) = added
                .iter()
                .find(|(path, digest)| !consumed_added.contains(path) && digest == before_digest)
            {
                consumed_added.insert(new_path.clone());
                changes.push(WorkspaceChange::new(
                    WorkspaceChangeKind::Renamed,
                    new_path.clone(),
                    Some(old_path.clone()),
                    Some(before_digest.clone()),
                    Some(after_digest.clone()),
                )?);
            } else {
                changes.push(WorkspaceChange::new(
                    WorkspaceChangeKind::Removed,
                    old_path.clone(),
                    None,
                    Some(before_digest.clone()),
                    None,
                )?);
            }
        }
        for (new_path, after_digest) in &added {
            if !consumed_added.contains(new_path) {
                changes.push(WorkspaceChange::new(
                    WorkspaceChangeKind::Added,
                    new_path.clone(),
                    None,
                    None,
                    Some(after_digest.clone()),
                )?);
            }
        }
        for (path, before) in &previous_files {
            if let Some(after) = current_files.get(path) {
                let before_digest = effective_digest(before);
                let after_digest = effective_digest(after);
                if before_digest != after_digest
                    || before.disposition != after.disposition
                    || before.instruction_safe != after.instruction_safe
                {
                    changes.push(WorkspaceChange::new(
                        WorkspaceChangeKind::Changed,
                        path.clone(),
                        None,
                        Some(before_digest),
                        Some(after_digest),
                    )?);
                }
            }
        }
        changes.sort_by(|left, right| left.path.cmp(&right.path).then(left.kind.cmp(&right.kind)));
        if changes.len() > MAX_INDEX_CHANGES {
            return Err("index_invalidation_change_limit".to_owned());
        }
        let mut invalidated = BTreeSet::new();
        let mut tombstones = BTreeSet::new();
        for change in &changes {
            invalidated.insert(change.path.clone());
            if let Some(previous_path) = &change.previous_path {
                invalidated.insert(previous_path.clone());
                tombstones.insert(previous_path.clone());
            }
            if change.kind == WorkspaceChangeKind::Removed {
                tombstones.insert(change.path.clone());
            }
        }
        let mut plan = Self {
            schema: INDEX_INVALIDATION_SCHEMA.to_owned(),
            version: INDEX_INVALIDATION_VERSION,
            source_generation,
            target_generation,
            source_snapshot_digest: previous.map(|snapshot| snapshot.snapshot_digest.clone()),
            target_snapshot_digest: current.snapshot_digest.clone(),
            changes,
            invalidated_paths: invalidated.into_iter().collect(),
            tombstones: tombstones.into_iter().collect(),
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INDEX_INVALIDATION_SCHEMA
            || self.version != INDEX_INVALIDATION_VERSION
            || self.target_generation <= self.source_generation
            || self.changes.len() > MAX_INDEX_CHANGES
        {
            return Err("index_invalidation_header_invalid".to_owned());
        }
        if let Some(source) = &self.source_snapshot_digest {
            digest(source, "index_invalidation_source_snapshot_digest")?;
        }
        digest(
            &self.target_snapshot_digest,
            "index_invalidation_target_snapshot_digest",
        )?;
        let mut change_digests = BTreeSet::new();
        for change in &self.changes {
            change.validate()?;
            if !change_digests.insert(change.change_digest.clone()) {
                return Err("index_invalidation_duplicate_change".to_owned());
            }
        }
        validate_paths(&self.invalidated_paths, "index_invalidation_path")?;
        validate_paths(&self.tombstones, "index_invalidation_tombstone")?;
        digest(&self.plan_digest, "index_invalidation_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("index_invalidation_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "source_generation": self.source_generation,
            "target_generation": self.target_generation,
            "source_snapshot_digest": self.source_snapshot_digest,
            "target_snapshot_digest": self.target_snapshot_digest,
            "changes": self.changes,
            "invalidated_paths": self.invalidated_paths,
            "tombstones": self.tombstones,
        }))
    }
}

fn validate_paths(paths: &[String], field: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for value in paths {
        path(value, field)?;
        if !seen.insert(value) {
            return Err(format!("{field}_duplicate"));
        }
    }
    Ok(())
}

fn effective_digest(file: &WorkspaceFileSnapshot) -> String {
    file.identity_after
        .as_ref()
        .and_then(|identity| identity.content_digest.clone())
        .unwrap_or_else(|| file.file_digest.clone())
}

// Keep the import in this module tied to the snapshot's read disposition contract for future
// adapters; untrusted metadata rows remain invalidation evidence but never indexable content.
#[allow(dead_code)]
fn is_indexable(file: &WorkspaceFileSnapshot) -> bool {
    file.disposition == WorkspaceReadDisposition::Indexed && file.instruction_safe
}
