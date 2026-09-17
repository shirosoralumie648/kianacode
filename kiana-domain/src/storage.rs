//! Storage root, owner scope, namespace and lock contracts.
//!
//! The domain layer only validates/canonicalizes the logical contract. Filesystem creation and
//! process locking live in the daemon adapter; no path in this module is an authorization source.
use crate::{json_digest, ProjectId, StorageLockId, StorageRootId, StoreIdentityId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Component, Path};
use uuid::Uuid;

pub const STORAGE_ROOT_SCHEMA: &str = "kiana.storage-root.v1";
pub const STORAGE_OWNER_SCOPE_SCHEMA: &str = "kiana.storage-owner-scope.v1";
pub const STORE_IDENTITY_SCHEMA: &str = "kiana.store-identity.v1";
pub const STORAGE_LOCK_SCHEMA: &str = "kiana.storage-lock.v1";
pub const MAX_STORAGE_OWNER_BYTES: usize = 256;
pub const MAX_STORAGE_INSTANCE_BYTES: usize = 256;
pub const MAX_STORAGE_PATH_BYTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageNamespace {
    Meta,
    Facts,
    Projections,
    Artifacts,
    Memory,
    Indexes,
    Checkpoints,
    Backups,
    Migrations,
    Quarantine,
    Locks,
}

impl StorageNamespace {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Meta => "meta",
            Self::Facts => "facts",
            Self::Projections => "projections",
            Self::Artifacts => "artifacts",
            Self::Memory => "memory",
            Self::Indexes => "indexes",
            Self::Checkpoints => "checkpoints",
            Self::Backups => "backups",
            Self::Migrations => "migrations",
            Self::Quarantine => "quarantine",
            Self::Locks => "locks",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackend {
    LocalFilesystem,
    NetworkFilesystem,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageOwnerScope {
    pub schema: String,
    pub owner_id: String,
    pub instance_id: String,
    #[serde(default)]
    pub project_id: Option<ProjectId>,
    pub authority_epoch: u64,
    pub scope_digest: String,
}

impl StorageOwnerScope {
    pub fn new(
        owner_id: impl Into<String>,
        instance_id: impl Into<String>,
        project_id: Option<ProjectId>,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut scope = Self {
            schema: STORAGE_OWNER_SCOPE_SCHEMA.to_owned(),
            owner_id: owner_id.into(),
            instance_id: instance_id.into(),
            project_id,
            authority_epoch,
            scope_digest: String::new(),
        };
        scope.scope_digest = scope.digest();
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_OWNER_SCOPE_SCHEMA || self.authority_epoch == 0 {
            return Err("storage_owner_scope_header_invalid".to_owned());
        }
        required(&self.owner_id, "storage_owner", MAX_STORAGE_OWNER_BYTES)?;
        required(
            &self.instance_id,
            "storage_instance",
            MAX_STORAGE_INSTANCE_BYTES,
        )?;
        if self.project_id.is_some_and(|id| id.as_uuid().is_nil()) {
            return Err("storage_project_id_invalid".to_owned());
        }
        validate_digest(&self.scope_digest, "storage_owner_scope_digest")?;
        if self.scope_digest != self.digest() {
            return Err("storage_owner_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "owner_id": self.owner_id,
            "instance_id": self.instance_id,
            "project_id": self.project_id,
            "authority_epoch": self.authority_epoch,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageRoot {
    pub schema: String,
    pub root_id: StorageRootId,
    pub canonical_path: String,
    pub backend: StorageBackend,
    pub owner_scope: StorageOwnerScope,
    pub namespaces: BTreeMap<StorageNamespace, String>,
    pub root_digest: String,
}

impl StorageRoot {
    pub fn new(
        canonical_path: impl Into<String>,
        backend: StorageBackend,
        owner_scope: StorageOwnerScope,
    ) -> Result<Self, String> {
        let canonical_path = canonical_path.into();
        let mut namespaces = BTreeMap::new();
        for namespace in [
            StorageNamespace::Meta,
            StorageNamespace::Facts,
            StorageNamespace::Projections,
            StorageNamespace::Artifacts,
            StorageNamespace::Memory,
            StorageNamespace::Indexes,
            StorageNamespace::Checkpoints,
            StorageNamespace::Backups,
            StorageNamespace::Migrations,
            StorageNamespace::Quarantine,
            StorageNamespace::Locks,
        ] {
            namespaces.insert(
                namespace,
                format!(
                    "{}/{}",
                    canonical_path.trim_end_matches('/'),
                    namespace.as_str()
                ),
            );
        }
        let mut root = Self {
            schema: STORAGE_ROOT_SCHEMA.to_owned(),
            root_id: stable_root_id(&canonical_path),
            canonical_path,
            backend,
            owner_scope,
            namespaces,
            root_digest: String::new(),
        };
        root.root_digest = root.digest();
        root.validate()?;
        Ok(root)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_ROOT_SCHEMA || self.root_id.as_uuid().is_nil() {
            return Err("storage_root_header_invalid".to_owned());
        }
        if self.backend == StorageBackend::NetworkFilesystem {
            return Err("storage_network_filesystem_unsupported".to_owned());
        }
        validate_absolute_path(&self.canonical_path)?;
        self.owner_scope.validate()?;
        if self.namespaces.len() != 11
            || self.namespaces.iter().any(|(namespace, path)| {
                path != &format!(
                    "{}/{}",
                    self.canonical_path.trim_end_matches('/'),
                    namespace.as_str()
                )
            })
        {
            return Err("storage_namespace_map_invalid".to_owned());
        }
        validate_digest(&self.root_digest, "storage_root_digest")?;
        if self.root_digest != self.digest() {
            return Err("storage_root_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn namespace_path(&self, namespace: StorageNamespace) -> Result<&str, String> {
        self.validate()?;
        self.namespaces
            .get(&namespace)
            .map(String::as_str)
            .ok_or_else(|| "storage_namespace_missing".to_owned())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "root_id": self.root_id,
            "canonical_path": self.canonical_path,
            "backend": self.backend,
            "owner_scope": self.owner_scope,
            "namespaces": self.namespaces,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreIdentity {
    pub schema: String,
    pub store_id: StoreIdentityId,
    pub root_id: StorageRootId,
    pub owner_scope_digest: String,
    pub instance_id: String,
    pub format_version: u32,
    pub schema_epoch: u64,
    pub created_at_unix_ms: u64,
    pub identity_digest: String,
}

impl StoreIdentity {
    pub fn new(
        root: &StorageRoot,
        format_version: u32,
        schema_epoch: u64,
        created_at: u64,
    ) -> Result<Self, String> {
        root.validate()?;
        let mut identity = Self {
            schema: STORE_IDENTITY_SCHEMA.to_owned(),
            store_id: StoreIdentityId::new(),
            root_id: root.root_id,
            owner_scope_digest: root.owner_scope.scope_digest.clone(),
            instance_id: root.owner_scope.instance_id.clone(),
            format_version,
            schema_epoch,
            created_at_unix_ms: created_at,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate(root)?;
        Ok(identity)
    }

    pub fn validate(&self, root: &StorageRoot) -> Result<(), String> {
        root.validate()?;
        if self.schema != STORE_IDENTITY_SCHEMA
            || self.store_id.as_uuid().is_nil()
            || self.root_id != root.root_id
            || self.owner_scope_digest != root.owner_scope.scope_digest
            || self.instance_id != root.owner_scope.instance_id
            || self.format_version == 0
            || self.schema_epoch == 0
        {
            return Err("store_identity_mismatch".to_owned());
        }
        validate_digest(&self.identity_digest, "store_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("store_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "store_id": self.store_id,
            "root_id": self.root_id,
            "owner_scope_digest": self.owner_scope_digest,
            "instance_id": self.instance_id,
            "format_version": self.format_version,
            "schema_epoch": self.schema_epoch,
            "created_at_unix_ms": self.created_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageLockRecord {
    pub schema: String,
    pub lock_id: StorageLockId,
    pub store_id: StoreIdentityId,
    pub owner_id: String,
    pub instance_id: String,
    pub acquired_at_unix_ms: u64,
    pub lock_digest: String,
}

impl StorageLockRecord {
    pub fn new(
        identity: &StoreIdentity,
        owner_scope: &StorageOwnerScope,
        acquired_at: u64,
    ) -> Result<Self, String> {
        let mut record = Self {
            schema: STORAGE_LOCK_SCHEMA.to_owned(),
            lock_id: StorageLockId::new(),
            store_id: identity.store_id,
            owner_id: owner_scope.owner_id.clone(),
            instance_id: owner_scope.instance_id.clone(),
            acquired_at_unix_ms: acquired_at,
            lock_digest: String::new(),
        };
        record.lock_digest = record.digest();
        record.validate(identity, owner_scope)?;
        Ok(record)
    }

    pub fn validate(
        &self,
        identity: &StoreIdentity,
        owner_scope: &StorageOwnerScope,
    ) -> Result<(), String> {
        if self.schema != STORAGE_LOCK_SCHEMA
            || self.lock_id.as_uuid().is_nil()
            || self.store_id != identity.store_id
            || self.owner_id != owner_scope.owner_id
            || self.instance_id != owner_scope.instance_id
        {
            return Err("storage_lock_owner_mismatch".to_owned());
        }
        validate_digest(&self.lock_digest, "storage_lock_digest")?;
        if self.lock_digest != self.digest() {
            return Err("storage_lock_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "lock_id": self.lock_id,
            "store_id": self.store_id,
            "owner_id": self.owner_id,
            "instance_id": self.instance_id,
            "acquired_at_unix_ms": self.acquired_at_unix_ms,
        }))
    }
}

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn validate_absolute_path(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_STORAGE_PATH_BYTES
        || !Path::new(value).is_absolute()
        || Path::new(value)
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err("storage_root_path_invalid".to_owned());
    }
    Ok(())
}

fn stable_root_id(path: &str) -> StorageRootId {
    let digest = Sha256::digest(path.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    StorageRootId::from_uuid(Uuid::from_bytes(bytes))
}
