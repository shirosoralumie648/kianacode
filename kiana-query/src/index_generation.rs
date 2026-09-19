//! Atomic publication of one validated index manifest.
//!
//! Component builders remain outside this adapter. They must first produce a single domain
//! IndexManifest; this module writes a temporary file, syncs it, then renames it into place.
//! Readers validate only the published Ready manifest and never discover components separately.

use anyhow::{anyhow, Context, Result};
use kiana_domain::IndexManifest;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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
