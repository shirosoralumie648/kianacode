//! Shared source-manifest and index-generation contracts.
//!
//! Repo map, exact/BM25 and dense artifacts are published as one generation. Readers receive only
//! the last Ready manifest; a failed or incomplete build cannot replace it or mix components from
//! different source manifests.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const INDEX_MANIFEST_SCHEMA: &str = "kiana.index-manifest.v1";
pub const INDEX_GENERATION_STATE_SCHEMA: &str = "kiana.index-generation-state.v1";
pub const INDEX_GENERATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_INDEX_COMPONENTS: usize = 4;

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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexComponentKind {
    RepoMap,
    Exact,
    Bm25,
    Dense,
}

impl IndexComponentKind {
    pub const ALL: [Self; MAX_INDEX_COMPONENTS] =
        [Self::RepoMap, Self::Exact, Self::Bm25, Self::Dense];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RepoMap => "repo_map",
            Self::Exact => "exact",
            Self::Bm25 => "bm25",
            Self::Dense => "dense",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexGenerationStatus {
    Building,
    Ready,
    Failed,
    Retired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexManifest {
    pub schema: String,
    pub version: SchemaVersion,
    pub generation: u64,
    pub source_manifest_digest: String,
    pub components: BTreeMap<String, String>,
    pub status: IndexGenerationStatus,
    #[serde(default)]
    pub failure_reason: Option<String>,
    pub manifest_digest: String,
}

impl IndexManifest {
    pub fn building(
        generation: u64,
        source_manifest_digest: impl Into<String>,
        components: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let mut manifest = Self {
            schema: INDEX_MANIFEST_SCHEMA.to_owned(),
            version: INDEX_GENERATION_VERSION,
            generation,
            source_manifest_digest: source_manifest_digest.into(),
            components,
            status: IndexGenerationStatus::Building,
            failure_reason: None,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn ready(mut self) -> Result<Self, String> {
        if self.status != IndexGenerationStatus::Building {
            return Err("index_manifest_not_building".to_owned());
        }
        self.status = IndexGenerationStatus::Ready;
        self.failure_reason = None;
        self.manifest_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn failed(mut self, reason: impl Into<String>) -> Result<Self, String> {
        if self.status != IndexGenerationStatus::Building {
            return Err("index_manifest_not_building".to_owned());
        }
        self.status = IndexGenerationStatus::Failed;
        self.failure_reason = Some(reason.into());
        self.manifest_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INDEX_MANIFEST_SCHEMA
            || self.version != INDEX_GENERATION_VERSION
            || self.generation == 0
            || self.components.len() != MAX_INDEX_COMPONENTS
        {
            return Err("index_manifest_header_invalid".to_owned());
        }
        digest(&self.source_manifest_digest, "index_source_manifest_digest")?;
        for component in IndexComponentKind::ALL {
            let Some(component_digest) = self.components.get(component.as_str()) else {
                return Err("index_manifest_component_missing".to_owned());
            };
            digest(component_digest, "index_component_digest")?;
        }
        if self.components.keys().any(|key| {
            !IndexComponentKind::ALL
                .iter()
                .any(|kind| kind.as_str() == key)
        }) {
            return Err("index_manifest_component_unknown".to_owned());
        }
        if let Some(reason) = &self.failure_reason {
            required(reason, "index_manifest_failure_reason", 256)?;
        }
        match self.status {
            IndexGenerationStatus::Building | IndexGenerationStatus::Ready => {
                if self.failure_reason.is_some() {
                    return Err("index_manifest_failure_on_active_status".to_owned());
                }
            }
            IndexGenerationStatus::Failed => {
                if self.failure_reason.is_none() {
                    return Err("index_manifest_failure_reason_required".to_owned());
                }
            }
            IndexGenerationStatus::Retired => {}
        }
        digest(&self.manifest_digest, "index_manifest_digest")?;
        if self.manifest_digest != self.digest() {
            return Err("index_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "generation": self.generation,
            "source_manifest_digest": self.source_manifest_digest,
            "components": self.components,
            "status": self.status,
            "failure_reason": self.failure_reason,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexGenerationState {
    pub schema: String,
    pub version: SchemaVersion,
    pub next_generation: u64,
    #[serde(default)]
    pub ready: Option<IndexManifest>,
    #[serde(default)]
    pub building: Option<IndexManifest>,
    #[serde(default)]
    pub last_failure: Option<String>,
    pub state_digest: String,
}

impl Default for IndexGenerationState {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexGenerationState {
    pub fn new() -> Self {
        let mut state = Self {
            schema: INDEX_GENERATION_STATE_SCHEMA.to_owned(),
            version: INDEX_GENERATION_VERSION,
            next_generation: 1,
            ready: None,
            building: None,
            last_failure: None,
            state_digest: String::new(),
        };
        state.state_digest = state.digest();
        state
    }

    pub fn begin(
        &mut self,
        source_manifest_digest: impl Into<String>,
        components: BTreeMap<String, String>,
    ) -> Result<IndexManifest, String> {
        self.validate()?;
        if self.building.is_some() {
            return Err("index_generation_build_already_active".to_owned());
        }
        let manifest =
            IndexManifest::building(self.next_generation, source_manifest_digest, components)?;
        self.building = Some(manifest.clone());
        self.state_digest = self.digest();
        Ok(manifest)
    }

    pub fn commit(&mut self, manifest: IndexManifest) -> Result<IndexManifest, String> {
        self.validate()?;
        manifest.validate()?;
        if manifest.status != IndexGenerationStatus::Building
            || self.building.as_ref() != Some(&manifest)
        {
            return Err("index_generation_commit_fence_mismatch".to_owned());
        }
        let ready = manifest.ready()?;
        self.ready = Some(ready.clone());
        self.building = None;
        self.last_failure = None;
        self.next_generation = self.next_generation.saturating_add(1);
        self.state_digest = self.digest();
        Ok(ready)
    }

    pub fn fail(&mut self, reason: impl Into<String>) -> Result<(), String> {
        self.validate()?;
        let reason = reason.into();
        required(&reason, "index_generation_failure_reason", 256)?;
        if let Some(building) = self.building.take() {
            let _ = building.failed(reason.clone())?;
        }
        self.last_failure = Some(reason);
        self.state_digest = self.digest();
        Ok(())
    }

    pub fn reader(&self) -> Result<&IndexManifest, String> {
        self.validate()?;
        self.ready
            .as_ref()
            .ok_or_else(|| "index_generation_no_ready_manifest".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INDEX_GENERATION_STATE_SCHEMA
            || self.version != INDEX_GENERATION_VERSION
            || self.next_generation == 0
        {
            return Err("index_generation_state_header_invalid".to_owned());
        }
        if let Some(ready) = &self.ready {
            ready.validate()?;
            if ready.status != IndexGenerationStatus::Ready
                || ready.generation >= self.next_generation
            {
                return Err("index_generation_ready_invalid".to_owned());
            }
        }
        if let Some(building) = &self.building {
            building.validate()?;
            if building.status != IndexGenerationStatus::Building
                || building.generation != self.next_generation
            {
                return Err("index_generation_building_invalid".to_owned());
            }
        }
        if let Some(failure) = &self.last_failure {
            required(failure, "index_generation_last_failure", 256)?;
        }
        digest(&self.state_digest, "index_generation_state_digest")?;
        if self.state_digest != self.digest() {
            return Err("index_generation_state_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "next_generation": self.next_generation,
            "ready": self.ready,
            "building": self.building,
            "last_failure": self.last_failure,
        }))
    }
}
