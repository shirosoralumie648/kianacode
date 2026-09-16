//! Immutable extension snapshot cache and explicit invalidation boundaries.
//!
//! A cache hit only reuses a read projection.  It never authorizes a capability or bypasses the
//! resolver/ControlPlane checks.  Invalidation is monotonic: a stale entry is retained for
//! diagnostics but can no longer be returned as the current snapshot.

use kiana_domain::{
    json_digest, SnapshotId, EXTENSION_SNAPSHOT_CACHE_ENTRY_SCHEMA,
    EXTENSION_SNAPSHOT_CACHE_KEY_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub const SNAPSHOT_CACHE_KEY_SCHEMA: &str = EXTENSION_SNAPSHOT_CACHE_KEY_SCHEMA;
pub const SNAPSHOT_CACHE_ENTRY_SCHEMA: &str = EXTENSION_SNAPSHOT_CACHE_ENTRY_SCHEMA;
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

static SNAPSHOT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCacheKey {
    pub schema: String,
    pub cwd_digest: String,
    pub trust_decision: String,
    pub source_roots_digest: String,
    pub package_registry_generation: String,
    pub config_digest: String,
    pub schema_version: u32,
    pub key_digest: String,
}

impl SnapshotCacheKey {
    pub fn new(
        cwd: &str,
        trust_decision: &str,
        source_roots_digest: &str,
        package_registry_generation: &str,
        config_digest: &str,
    ) -> Result<Self, String> {
        if cwd.trim().is_empty()
            || trust_decision.trim().is_empty()
            || source_roots_digest.trim().is_empty()
            || package_registry_generation.trim().is_empty()
            || config_digest.trim().is_empty()
        {
            return Err("snapshot_cache_key_input_required".to_owned());
        }
        let mut key = Self {
            schema: SNAPSHOT_CACHE_KEY_SCHEMA.to_owned(),
            cwd_digest: json_digest(&json!({"cwd": cwd})),
            trust_decision: trust_decision.to_owned(),
            source_roots_digest: source_roots_digest.to_owned(),
            package_registry_generation: package_registry_generation.to_owned(),
            config_digest: config_digest.to_owned(),
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            key_digest: String::new(),
        };
        key.key_digest = key.digest();
        key.validate()?;
        Ok(key)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SNAPSHOT_CACHE_KEY_SCHEMA
            || self.schema_version != SNAPSHOT_SCHEMA_VERSION
            || self.trust_decision.trim().is_empty()
            || !is_digest(&self.cwd_digest)
            || !is_digest(&self.source_roots_digest)
            || !is_digest(&self.config_digest)
            || self.package_registry_generation.trim().is_empty()
            || self.package_registry_generation.len() > 256
            || !is_digest(&self.key_digest)
        {
            return Err("snapshot_cache_key_invalid".to_owned());
        }
        if self.key_digest != self.digest() {
            return Err("snapshot_cache_key_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "key_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotCacheState {
    Prepared,
    Used,
    Invalidated,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCacheEntry {
    pub schema: String,
    pub key: SnapshotCacheKey,
    pub snapshot_id: SnapshotId,
    pub generation: u64,
    pub state: SnapshotCacheState,
    #[serde(default)]
    pub invalidation_reason: Option<String>,
}

impl SnapshotCacheEntry {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SNAPSHOT_CACHE_ENTRY_SCHEMA || self.generation == 0 {
            return Err("snapshot_cache_entry_invalid".to_owned());
        }
        self.key.validate()?;
        if self
            .invalidation_reason
            .as_deref()
            .is_some_and(|reason| reason.trim().is_empty() || reason.len() > 512)
        {
            return Err("snapshot_cache_invalidation_reason_invalid".to_owned());
        }
        if self.state != SnapshotCacheState::Invalidated && self.invalidation_reason.is_some() {
            return Err("snapshot_cache_invalidation_state_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ExtensionSnapshotCache {
    entries: BTreeMap<String, SnapshotCacheEntry>,
    generation: u64,
}

impl Default for ExtensionSnapshotCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtensionSnapshotCache {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            generation: 1,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn insert(
        &mut self,
        key: SnapshotCacheKey,
        snapshot_id: SnapshotId,
    ) -> Result<SnapshotCacheEntry, String> {
        key.validate()?;
        if let Some(existing) = self.entries.get(&key.key_digest) {
            if existing.state != SnapshotCacheState::Invalidated {
                return Ok(existing.clone());
            }
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "snapshot_cache_generation_exhausted".to_owned())?;
        let entry = SnapshotCacheEntry {
            schema: SNAPSHOT_CACHE_ENTRY_SCHEMA.to_owned(),
            key: key.clone(),
            snapshot_id,
            generation: self.generation,
            state: SnapshotCacheState::Prepared,
            invalidation_reason: None,
        };
        entry.validate()?;
        self.entries.insert(key.key_digest, entry.clone());
        Ok(entry)
    }

    pub fn get(&self, key: &SnapshotCacheKey) -> Result<Option<SnapshotCacheEntry>, String> {
        key.validate()?;
        Ok(self.entries.get(&key.key_digest).and_then(|entry| {
            (entry.state != SnapshotCacheState::Invalidated).then(|| entry.clone())
        }))
    }

    pub fn mark_used(
        &mut self,
        key: &SnapshotCacheKey,
    ) -> Result<Option<SnapshotCacheEntry>, String> {
        key.validate()?;
        let Some(entry) = self.entries.get_mut(&key.key_digest) else {
            return Ok(None);
        };
        if entry.state == SnapshotCacheState::Invalidated {
            return Ok(None);
        }
        entry.state = SnapshotCacheState::Used;
        Ok(Some(entry.clone()))
    }

    pub fn invalidate_key(
        &mut self,
        key: &SnapshotCacheKey,
        reason: impl Into<String>,
    ) -> Result<bool, String> {
        key.validate()?;
        let reason = reason.into();
        if reason.trim().is_empty() || reason.len() > 512 {
            return Err("snapshot_cache_invalidation_reason_invalid".to_owned());
        }
        let Some(entry) = self.entries.get_mut(&key.key_digest) else {
            return Ok(false);
        };
        entry.state = SnapshotCacheState::Invalidated;
        entry.invalidation_reason = Some(reason);
        entry.validate()?;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "snapshot_cache_generation_exhausted".to_owned())?;
        Ok(true)
    }

    pub fn invalidate_all(&mut self, reason: impl Into<String>) -> Result<u64, String> {
        let reason = reason.into();
        if reason.trim().is_empty() || reason.len() > 512 {
            return Err("snapshot_cache_invalidation_reason_invalid".to_owned());
        }
        for entry in self.entries.values_mut() {
            entry.state = SnapshotCacheState::Invalidated;
            entry.invalidation_reason = Some(reason.clone());
            entry.validate()?;
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "snapshot_cache_generation_exhausted".to_owned())?;
        Ok(self.generation)
    }

    pub fn entries(&self) -> impl Iterator<Item = &SnapshotCacheEntry> {
        self.entries.values()
    }
}

pub fn snapshot_generation() -> u64 {
    SNAPSHOT_GENERATION.load(Ordering::Acquire)
}

pub fn invalidate_extension_snapshots(_reason: &str) -> u64 {
    SNAPSHOT_GENERATION
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1)
}

fn is_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
