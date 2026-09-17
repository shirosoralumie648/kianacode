//! Strict, deterministic evaluation fixture manifest loader.
//!
//! This compatibility/test adapter opens only declared files beneath a caller-supplied canonical
//! root. It never reads the operator home implicitly and never starts an evaluator or provider.
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const EVAL_FIXTURE_MANIFEST_SCHEMA: &str = "kiana.eval-fixture-manifest.v1";
pub const EVAL_RUNTIME_FIXTURE_SCHEMA: &str = "kiana.runtime-event-fixture.v1";
pub const EVAL_PROVIDER_FIXTURE_SCHEMA: &str = "kiana.provider-fixture.v1";
pub const EVAL_MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
pub const EVAL_MAX_CASE_FIXTURE_BYTES: u64 = 16 * 1024 * 1024;
pub const EVAL_MAX_MANIFEST_CASES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalFixtureManifest {
    pub schema: String,
    pub suite_id: String,
    #[serde(default)]
    pub description: String,
    pub cases: Vec<EvalFixtureCase>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalFixtureCase {
    pub case_id: String,
    pub fixture_ref: String,
    pub fixture_schema: String,
    pub max_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedFixture {
    pub case_id: String,
    pub fixture_ref: String,
    pub fixture_schema: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedFixtureManifest {
    pub manifest: EvalFixtureManifest,
    pub root: PathBuf,
    pub manifest_sha256: String,
    pub fixtures: Vec<LoadedFixture>,
}

pub fn load_fixture_manifest(root: &Path, manifest_path: &Path) -> Result<LoadedFixtureManifest> {
    let root = root
        .canonicalize()
        .with_context(|| format!("fixture root cannot be resolved: {}", root.display()))?;
    if !root.is_dir() {
        return Err(anyhow!("fixture root is not a directory"));
    }
    let manifest_path = resolve_declared_path(&root, manifest_path, "fixture_manifest_path")?;
    if !manifest_path.is_file() {
        return Err(anyhow!("fixture_manifest_not_regular_file"));
    }
    let manifest_bytes = fs::read(&manifest_path).with_context(|| {
        format!(
            "fixture manifest cannot be read: {}",
            manifest_path.display()
        )
    })?;
    if manifest_bytes.len() as u64 > EVAL_MAX_MANIFEST_BYTES {
        return Err(anyhow!("fixture_manifest_too_large"));
    }
    let manifest: EvalFixtureManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| anyhow!("fixture_manifest_invalid_json"))?;
    validate_manifest(&manifest)?;
    let mut fixtures = Vec::with_capacity(manifest.cases.len());
    let mut cases = manifest.cases.clone();
    cases.sort_by(|left, right| left.case_id.cmp(&right.case_id));
    for case in cases {
        let path = resolve_declared_path(&root, Path::new(&case.fixture_ref), "fixture_path")?;
        if !path.is_file() {
            return Err(anyhow!("fixture_not_regular_file:{}", case.case_id));
        }
        let metadata = fs::metadata(&path)
            .with_context(|| format!("fixture metadata unavailable: {}", path.display()))?;
        if metadata.len() > case.max_bytes || metadata.len() > EVAL_MAX_CASE_FIXTURE_BYTES {
            return Err(anyhow!("fixture_too_large:{}", case.case_id));
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("fixture cannot be read: {}", path.display()))?;
        let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
        fixtures.push(LoadedFixture {
            case_id: case.case_id,
            fixture_ref: case.fixture_ref,
            fixture_schema: case.fixture_schema,
            bytes,
            sha256,
        });
    }
    Ok(LoadedFixtureManifest {
        manifest,
        root,
        manifest_sha256: format!("sha256:{:x}", Sha256::digest(&manifest_bytes)),
        fixtures,
    })
}

fn validate_manifest(manifest: &EvalFixtureManifest) -> Result<()> {
    if manifest.schema != EVAL_FIXTURE_MANIFEST_SCHEMA {
        return Err(anyhow!("fixture_manifest_schema_unknown"));
    }
    if manifest.suite_id.trim().is_empty()
        || manifest.suite_id.len() > 256
        || manifest.suite_id.contains('\0')
    {
        return Err(anyhow!("fixture_manifest_suite_invalid"));
    }
    if manifest.description.len() > 2_048 || manifest.description.contains('\0') {
        return Err(anyhow!("fixture_manifest_description_invalid"));
    }
    if manifest.cases.is_empty() || manifest.cases.len() > EVAL_MAX_MANIFEST_CASES {
        return Err(anyhow!("fixture_manifest_case_limit"));
    }
    let mut ids = BTreeSet::new();
    for case in &manifest.cases {
        if case.case_id.trim().is_empty()
            || case.case_id.len() > 256
            || case.case_id.contains('\0')
            || !ids.insert(case.case_id.clone())
        {
            return Err(anyhow!("fixture_manifest_case_id_invalid"));
        }
        if case.fixture_ref.trim().is_empty() || case.fixture_ref.contains('\0') {
            return Err(anyhow!("fixture_path_invalid:{}", case.case_id));
        }
        if case.max_bytes == 0 || case.max_bytes > EVAL_MAX_CASE_FIXTURE_BYTES {
            return Err(anyhow!("fixture_max_bytes_invalid:{}", case.case_id));
        }
        if !matches!(
            case.fixture_schema.as_str(),
            EVAL_RUNTIME_FIXTURE_SCHEMA | EVAL_PROVIDER_FIXTURE_SCHEMA
        ) {
            return Err(anyhow!("fixture_schema_unknown:{}", case.case_id));
        }
    }
    Ok(())
}

fn resolve_declared_path(root: &Path, declared: &Path, field: &str) -> Result<PathBuf> {
    let relative = if declared.is_absolute()
        || declared.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
        return Err(anyhow!("{field}_escape"));
    } else {
        declared
    };
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|_| anyhow!("{field}_not_found"))?;
    if !path.starts_with(root) {
        return Err(anyhow!("{field}_escape"));
    }
    Ok(path)
}
