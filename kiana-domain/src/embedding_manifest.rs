//! Offline embedding package manifest and generation-rotation contracts.
//!
//! The manifest is a pinned description of an already-installed local package. It never downloads
//! weights, and an index reader must bind the exact manifest digest and generation before reading.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const EMBEDDING_MANIFEST_SCHEMA: &str = "kiana.embedding-manifest.v1";
pub const EMBEDDING_INDEX_BINDING_SCHEMA: &str = "kiana.embedding-index-binding.v1";
pub const EMBEDDING_ROTATION_SCHEMA: &str = "kiana.embedding-rotation.v1";
pub const MAX_EMBEDDING_DIMENSIONS: u32 = 16_384;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingPooling {
    Mean,
    Cls,
    LastToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingProvider {
    Cpu,
    Cuda,
    CoreMl,
    DirectMl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingDevice {
    Cpu,
    Gpu,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingManifest {
    pub schema: String,
    pub model_id: String,
    pub package_digest: String,
    pub weights_digest: String,
    pub tokenizer_digest: String,
    pub config_digest: String,
    pub dimensions: u32,
    pub pooling: EmbeddingPooling,
    pub provider: EmbeddingProvider,
    pub device: EmbeddingDevice,
    pub normalized: bool,
    pub network_allowed: bool,
    pub manifest_digest: String,
}

impl EmbeddingManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        model_id: impl Into<String>,
        package_digest: impl Into<String>,
        weights_digest: impl Into<String>,
        tokenizer_digest: impl Into<String>,
        config_digest: impl Into<String>,
        dimensions: u32,
        pooling: EmbeddingPooling,
        provider: EmbeddingProvider,
        device: EmbeddingDevice,
        normalized: bool,
    ) -> Result<Self, String> {
        let mut manifest = Self {
            schema: EMBEDDING_MANIFEST_SCHEMA.to_owned(),
            model_id: model_id.into(),
            package_digest: package_digest.into(),
            weights_digest: weights_digest.into(),
            tokenizer_digest: tokenizer_digest.into(),
            config_digest: config_digest.into(),
            dimensions,
            pooling,
            provider,
            device,
            normalized,
            network_allowed: false,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EMBEDDING_MANIFEST_SCHEMA
            || self.dimensions == 0
            || self.dimensions > MAX_EMBEDDING_DIMENSIONS
            || !self.normalized
            || self.network_allowed
        {
            return Err("embedding_manifest_header_invalid".to_owned());
        }
        required(&self.model_id, "embedding_model_id", 256)?;
        for (value, field) in [
            (&self.package_digest, "embedding_package_digest"),
            (&self.weights_digest, "embedding_weights_digest"),
            (&self.tokenizer_digest, "embedding_tokenizer_digest"),
            (&self.config_digest, "embedding_config_digest"),
            (&self.manifest_digest, "embedding_manifest_digest"),
        ] {
            digest(value, field)?;
        }
        if self.manifest_digest != self.digest() {
            return Err("embedding_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "model_id": self.model_id,
            "package_digest": self.package_digest,
            "weights_digest": self.weights_digest,
            "tokenizer_digest": self.tokenizer_digest,
            "config_digest": self.config_digest,
            "dimensions": self.dimensions,
            "pooling": self.pooling,
            "provider": self.provider,
            "device": self.device,
            "normalized": self.normalized,
            "network_allowed": self.network_allowed,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingIndexBinding {
    pub schema: String,
    pub generation: u64,
    pub source_manifest_digest: String,
    pub embedding_manifest_digest: String,
    pub components: BTreeMap<String, String>,
    pub binding_digest: String,
}

impl EmbeddingIndexBinding {
    pub fn new(
        generation: u64,
        source_manifest_digest: impl Into<String>,
        manifest: &EmbeddingManifest,
        components: BTreeMap<String, String>,
    ) -> Result<Self, String> {
        manifest.validate()?;
        let mut binding = Self {
            schema: EMBEDDING_INDEX_BINDING_SCHEMA.to_owned(),
            generation,
            source_manifest_digest: source_manifest_digest.into(),
            embedding_manifest_digest: manifest.manifest_digest.clone(),
            components,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate_against(manifest)?;
        Ok(binding)
    }

    pub fn validate_against(&self, manifest: &EmbeddingManifest) -> Result<(), String> {
        manifest.validate()?;
        if self.schema != EMBEDDING_INDEX_BINDING_SCHEMA
            || self.generation == 0
            || self.embedding_manifest_digest != manifest.manifest_digest
            || self.components.is_empty()
        {
            return Err("embedding_index_binding_invalid".to_owned());
        }
        digest(
            &self.source_manifest_digest,
            "embedding_index_source_manifest_digest",
        )?;
        for component in self.components.values() {
            digest(component, "embedding_index_component_digest")?;
        }
        digest(&self.binding_digest, "embedding_index_binding_digest")?;
        if self.binding_digest != self.digest() {
            return Err("embedding_index_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "generation": self.generation,
            "source_manifest_digest": self.source_manifest_digest,
            "embedding_manifest_digest": self.embedding_manifest_digest,
            "components": self.components,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingRotationPlan {
    pub schema: String,
    pub old_generation: u64,
    pub new_generation: u64,
    pub source_manifest_digest: String,
    pub old_embedding_manifest_digest: String,
    pub new_embedding_manifest_digest: String,
    pub old_read_only: bool,
    pub plan_digest: String,
}

impl EmbeddingRotationPlan {
    pub fn new(
        old_generation: u64,
        new_generation: u64,
        source_manifest_digest: impl Into<String>,
        old_manifest: &EmbeddingManifest,
        new_manifest: &EmbeddingManifest,
    ) -> Result<Self, String> {
        old_manifest.validate()?;
        new_manifest.validate()?;
        let mut plan = Self {
            schema: EMBEDDING_ROTATION_SCHEMA.to_owned(),
            old_generation,
            new_generation,
            source_manifest_digest: source_manifest_digest.into(),
            old_embedding_manifest_digest: old_manifest.manifest_digest.clone(),
            new_embedding_manifest_digest: new_manifest.manifest_digest.clone(),
            old_read_only: true,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EMBEDDING_ROTATION_SCHEMA
            || self.old_generation == 0
            || self.new_generation <= self.old_generation
            || !self.old_read_only
            || self.old_embedding_manifest_digest == self.new_embedding_manifest_digest
        {
            return Err("embedding_rotation_plan_invalid".to_owned());
        }
        digest(
            &self.source_manifest_digest,
            "embedding_rotation_source_manifest_digest",
        )?;
        digest(
            &self.old_embedding_manifest_digest,
            "embedding_rotation_old_manifest_digest",
        )?;
        digest(
            &self.new_embedding_manifest_digest,
            "embedding_rotation_new_manifest_digest",
        )?;
        digest(&self.plan_digest, "embedding_rotation_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("embedding_rotation_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn reader_allowed(&self, generation: u64, manifest_digest: &str) -> bool {
        (generation == self.old_generation && manifest_digest == self.old_embedding_manifest_digest)
            || (generation == self.new_generation
                && manifest_digest == self.new_embedding_manifest_digest)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "old_generation": self.old_generation,
            "new_generation": self.new_generation,
            "source_manifest_digest": self.source_manifest_digest,
            "old_embedding_manifest_digest": self.old_embedding_manifest_digest,
            "new_embedding_manifest_digest": self.new_embedding_manifest_digest,
            "old_read_only": self.old_read_only,
        }))
    }
}
