//! Persistent source manifest for RepoMap, context artifacts and dependency graphs.
//!
//! The manifest is a read/rebuild boundary. It records bounded derived summaries and digests;
//! it never authorizes a capability or replaces the EventLog/ControlPlane authority.

use crate::index::{build_context_artifact_store, ContextArtifactOptions, ContextArtifactStore};
use crate::index_generation::atomic_write_json;
use crate::repo_map::{build_repo_map, RepoMap, RepoMapOptions};
use anyhow::{anyhow, Context, Result};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

pub const CONTEXT_MATERIAL_MANIFEST_SCHEMA: &str = "kiana.context-material-manifest.v1";

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMaterialManifest {
    pub schema: String,
    pub root: String,
    pub source_cursor: u64,
    pub tool_version: String,
    pub repo_map: RepoMap,
    pub artifact_store: ContextArtifactStore,
    pub manifest_digest: String,
}

impl ContextMaterialManifest {
    pub fn build(
        root: impl AsRef<Path>,
        repo_options: RepoMapOptions,
        artifact_options: ContextArtifactOptions,
        source_cursor: u64,
        tool_version: impl Into<String>,
    ) -> Result<Self, String> {
        if source_cursor == 0 {
            return Err("context_material_source_cursor_invalid".to_owned());
        }
        let tool_version = tool_version.into();
        required(&tool_version, "context_material_tool_version", 128)?;
        let repo_map = build_repo_map(&root, repo_options).map_err(|error| error.to_string())?;
        let artifact_store = build_context_artifact_store(&root, artifact_options)
            .map_err(|error| error.to_string())?;
        if repo_map.root != artifact_store.root {
            return Err("context_material_root_mismatch".to_owned());
        }
        let mut manifest = Self {
            schema: CONTEXT_MATERIAL_MANIFEST_SCHEMA.to_owned(),
            root: repo_map.root.clone(),
            source_cursor,
            tool_version,
            repo_map,
            artifact_store,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_MATERIAL_MANIFEST_SCHEMA || self.source_cursor == 0 {
            return Err("context_material_manifest_header_invalid".to_owned());
        }
        required(&self.root, "context_material_root", 4096)?;
        required(&self.tool_version, "context_material_tool_version", 128)?;
        if self.repo_map.root != self.root || self.artifact_store.root != self.root {
            return Err("context_material_root_mismatch".to_owned());
        }
        for file in &self.repo_map.files {
            if file.path.starts_with('/') || file.path.contains("..") {
                return Err("context_material_repo_path_invalid".to_owned());
            }
            digest(&file.content_hash, "context_material_repo_content_hash")?;
        }
        for artifact in &self.artifact_store.artifacts.artifacts {
            if artifact.path.starts_with('/') || artifact.path.contains("..") {
                return Err("context_material_artifact_path_invalid".to_owned());
            }
            digest(
                &artifact.content_hash,
                "context_material_artifact_content_hash",
            )?;
        }
        digest(&self.manifest_digest, "context_material_manifest_digest")?;
        if self.manifest_digest != self.digest() {
            return Err("context_material_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "root": self.root,
            "source_cursor": self.source_cursor,
            "tool_version": self.tool_version,
            "repo_map": self.repo_map,
            "artifact_store": self.artifact_store,
        }))
    }
}

pub fn write_context_material_manifest_atomic(
    path: impl AsRef<Path>,
    manifest: &ContextMaterialManifest,
) -> Result<(), String> {
    manifest.validate()?;
    atomic_write_json(path.as_ref(), manifest, "context_material_manifest")
}

pub fn read_context_material_manifest(path: impl AsRef<Path>) -> Result<ContextMaterialManifest> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)
        .with_context(|| format!("context material manifest open failed: {}", path.display()))?;
    let manifest: ContextMaterialManifest = serde_json::from_str(&contents)
        .map_err(|_| anyhow!("context material manifest decode failed"))?;
    manifest.validate().map_err(|error| anyhow!(error))?;
    Ok(manifest)
}
