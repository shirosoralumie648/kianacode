//! Atomic publication of one validated index manifest.
//!
//! Component builders remain outside this adapter. They must first produce a single domain
//! IndexManifest; this module writes a temporary file, syncs it, then renames it into place.
//! Readers validate only the published Ready manifest and never discover components separately.

use crate::index::{build_context_index, ContextIndex, ContextIndexOptions};
use anyhow::{anyhow, Context, Result};
use kiana_domain::{
    json_digest, Freshness, IndexComponentKind, IndexGenerationState, IndexManifest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const CONTEXT_INDEX_SOURCE_MANIFEST_SCHEMA: &str = "kiana.context-index-source-manifest.v1";
pub const CONTEXT_INDEX_GENERATION_ENVELOPE_SCHEMA: &str =
    "kiana.context-index-generation-envelope.v1";

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextIndexSourceManifest {
    pub schema: String,
    pub root: String,
    pub generation: u64,
    pub source_fingerprint: String,
    pub ignore_rules_digest: String,
    pub config_digest: String,
    pub model_id: String,
    pub freshness: Freshness,
    pub manifest_digest: String,
}

impl ContextIndexSourceManifest {
    pub fn from_index(
        index: &ContextIndex,
        generation: u64,
        options: &ContextIndexOptions,
        ignore_rules_digest: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Result<Self, String> {
        let ignore_rules_digest = ignore_rules_digest.into();
        let model_id = model_id.into();
        if generation == 0 {
            return Err("context_index_generation_invalid".to_owned());
        }
        required(&index.root, "context_index_root", 4096)?;
        digest(&ignore_rules_digest, "context_index_ignore_rules_digest")?;
        required(&model_id, "context_index_model_id", 256)?;
        let mut files = index
            .files
            .iter()
            .map(|file| (&file.path, &file.bytes, &file.content_hash))
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(right.0));
        let config_digest = json_digest(&json!({
            "max_bytes_per_file": options.max_bytes_per_file,
            "model_id": &model_id,
        }));
        let source_fingerprint = json_digest(&json!({
            "root": &index.root,
            "files": files,
            "ignore_rules_digest": &ignore_rules_digest,
            "config_digest": &config_digest,
        }));
        let mut manifest = Self {
            schema: CONTEXT_INDEX_SOURCE_MANIFEST_SCHEMA.to_owned(),
            root: index.root.clone(),
            generation,
            source_fingerprint,
            ignore_rules_digest,
            config_digest,
            model_id,
            freshness: Freshness::Current,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn compare(&self, other: &Self) -> Result<Freshness, String> {
        self.validate()?;
        other.validate()?;
        Ok(
            if self.root == other.root
                && self.source_fingerprint == other.source_fingerprint
                && self.ignore_rules_digest == other.ignore_rules_digest
                && self.config_digest == other.config_digest
                && self.model_id == other.model_id
            {
                Freshness::Current
            } else {
                Freshness::Stale
            },
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_INDEX_SOURCE_MANIFEST_SCHEMA
            || self.generation == 0
            || self.freshness != Freshness::Current
        {
            return Err("context_index_source_manifest_invalid".to_owned());
        }
        required(&self.root, "context_index_root", 4096)?;
        digest(&self.source_fingerprint, "context_index_source_fingerprint")?;
        digest(
            &self.ignore_rules_digest,
            "context_index_ignore_rules_digest",
        )?;
        digest(&self.config_digest, "context_index_config_digest")?;
        required(&self.model_id, "context_index_model_id", 256)?;
        digest(&self.manifest_digest, "context_index_manifest_digest")?;
        if self.manifest_digest != self.digest() {
            return Err("context_index_source_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "root": self.root,
            "generation": self.generation,
            "source_fingerprint": self.source_fingerprint,
            "ignore_rules_digest": self.ignore_rules_digest,
            "config_digest": self.config_digest,
            "model_id": self.model_id,
            "freshness": self.freshness,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextIndexGenerationEnvelope {
    pub schema: String,
    pub source: ContextIndexSourceManifest,
    pub index: ContextIndex,
    pub index_manifest: IndexManifest,
    pub envelope_digest: String,
}

impl ContextIndexGenerationEnvelope {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_INDEX_GENERATION_ENVELOPE_SCHEMA {
            return Err("context_index_generation_envelope_schema_invalid".to_owned());
        }
        self.source.validate()?;
        self.index_manifest.validate()?;
        if self.index_manifest.status != kiana_domain::IndexGenerationStatus::Ready
            || self.index_manifest.generation != self.source.generation
            || self.index.root != self.source.root
        {
            return Err("context_index_generation_envelope_binding_invalid".to_owned());
        }
        digest(&self.envelope_digest, "context_index_envelope_digest")?;
        if self.envelope_digest != self.digest() {
            return Err("context_index_generation_envelope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source": self.source,
            "index": self.index,
            "index_manifest": self.index_manifest,
        }))
    }
}

pub fn build_context_index_generation(
    root: impl AsRef<Path>,
    options: ContextIndexOptions,
    previous: &IndexGenerationState,
    ignore_rules_digest: impl Into<String>,
    model_id: impl Into<String>,
) -> Result<(ContextIndexGenerationEnvelope, IndexGenerationState), String> {
    previous.validate()?;
    let index = build_context_index(root, options).map_err(|error| error.to_string())?;
    let generation = previous.next_generation;
    let source = ContextIndexSourceManifest::from_index(
        &index,
        generation,
        &options,
        ignore_rules_digest,
        model_id,
    )?;
    let index_digest = json_digest(
        &serde_json::to_value(&index).map_err(|_| "context_index_encode_failed".to_owned())?,
    );
    let components = IndexComponentKind::ALL
        .into_iter()
        .map(|kind| {
            (
                kind.as_str().to_owned(),
                json_digest(&json!({
                    "kind": kind,
                    "source_fingerprint": &source.source_fingerprint,
                    "index_digest": &index_digest,
                })),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut next = previous.clone();
    let building = next.begin(source.manifest_digest.clone(), components)?;
    let ready = next.commit(building)?;
    if ready.generation != source.generation {
        return Err("context_index_generation_binding_invalid".to_owned());
    }
    let mut envelope = ContextIndexGenerationEnvelope {
        schema: CONTEXT_INDEX_GENERATION_ENVELOPE_SCHEMA.to_owned(),
        source,
        index,
        index_manifest: ready,
        envelope_digest: String::new(),
    };
    envelope.envelope_digest = envelope.digest();
    envelope.validate()?;
    Ok((envelope, next))
}

pub fn write_context_index_generation_atomic(
    path: impl AsRef<Path>,
    envelope: &ContextIndexGenerationEnvelope,
) -> Result<(), String> {
    envelope.validate()?;
    atomic_write_json(path.as_ref(), envelope, "context_index_generation")
}

pub fn read_context_index_generation(
    path: impl AsRef<Path>,
) -> Result<ContextIndexGenerationEnvelope> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)
        .with_context(|| format!("context index generation open failed: {}", path.display()))?;
    let envelope: ContextIndexGenerationEnvelope = serde_json::from_str(&contents)
        .map_err(|_| anyhow!("context index generation decode failed"))?;
    envelope.validate().map_err(|error| anyhow!(error))?;
    Ok(envelope)
}

pub fn write_index_manifest_atomic(
    path: impl AsRef<Path>,
    manifest: &IndexManifest,
) -> Result<(), String> {
    manifest.validate()?;
    if manifest.status != kiana_domain::IndexGenerationStatus::Ready {
        return Err("index_manifest_publish_requires_ready".to_owned());
    }
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or_else(|| "index_manifest_parent_missing".to_owned())?;
    fs::create_dir_all(parent).map_err(|_| "index_manifest_parent_create_failed".to_owned())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "index_manifest_filename_missing".to_owned())?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|_| "index_manifest_encode_failed".to_owned())?;
    let result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "index_manifest_temp_create_failed".to_owned())?;
        file.write_all(&bytes)
            .map_err(|_| "index_manifest_temp_write_failed".to_owned())?;
        file.sync_all()
            .map_err(|_| "index_manifest_temp_sync_failed".to_owned())?;
        fs::rename(&temporary, path)
            .map_err(|_| "index_manifest_atomic_rename_failed".to_owned())?;
        sync_parent(parent).map_err(|_| "index_manifest_parent_sync_failed".to_owned())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn atomic_write_json<T: Serialize>(
    path: &Path,
    value: &T,
    label: &str,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{label}_parent_missing"))?;
    fs::create_dir_all(parent).map_err(|_| format!("{label}_parent_create_failed"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("{label}_filename_missing"))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| format!("{label}_encode_failed"))?;
    let result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| format!("{label}_temp_create_failed"))?;
        file.write_all(&bytes)
            .map_err(|_| format!("{label}_temp_write_failed"))?;
        file.sync_all()
            .map_err(|_| format!("{label}_temp_sync_failed"))?;
        fs::rename(&temporary, path).map_err(|_| format!("{label}_atomic_rename_failed"))?;
        sync_parent(parent).map_err(|_| format!("{label}_parent_sync_failed"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn read_index_manifest(path: impl AsRef<Path>) -> Result<IndexManifest> {
    let path = path.as_ref();
    let mut contents = String::new();
    File::open(path)
        .with_context(|| format!("index manifest open failed: {}", path.display()))?
        .read_to_string(&mut contents)
        .with_context(|| format!("index manifest read failed: {}", path.display()))?;
    let manifest: IndexManifest =
        serde_json::from_str(&contents).map_err(|_| anyhow!("index manifest decode failed"))?;
    manifest.validate().map_err(|error| anyhow!(error))?;
    if manifest.status != kiana_domain::IndexGenerationStatus::Ready {
        return Err(anyhow!("index manifest is not ready"));
    }
    Ok(manifest)
}

fn sync_parent(parent: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}

/// Resolve a manifest path relative to an explicit root without changing the root authority.
pub fn manifest_path(root: impl AsRef<Path>, relative: impl AsRef<Path>) -> PathBuf {
    let relative = relative.as_ref();
    if relative.is_absolute() {
        relative.to_path_buf()
    } else {
        root.as_ref().join(relative)
    }
}
