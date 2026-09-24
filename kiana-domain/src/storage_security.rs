//! Shared fail-closed contracts for persisted bytes and file identities.
//!
//! The domain layer never opens files or decrypts bytes. It only validates the metadata that a
//! core/adapter boundary must carry: secret-free payloads, owner-scoped opaque key references,
//! and a file identity that explicitly records platform limitations.

use crate::{json_digest, redact_text, redact_value, SchemaVersion, SecretRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path};

pub const STORAGE_SECURITY_SCHEMA: &str = "kiana.storage-security.v1";
pub const STORAGE_SECURITY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const STORAGE_FILE_IDENTITY_SCHEMA: &str = "kiana.storage-file-identity.v1";
pub const STORAGE_ENCRYPTION_BINDING_SCHEMA: &str = "kiana.storage-encryption-binding.v1";
pub const STORAGE_SECURITY_CAPABILITIES_SCHEMA: &str = "kiana.storage-security-capabilities.v1";
pub const MAX_STORAGE_SECURITY_TEXT_BYTES: usize = 4_096;
pub const MAX_STORAGE_SECURITY_LIMITATIONS: usize = 16;

/// Optional encryption metadata. The key is always an opaque SecretRef; this contract never
/// carries key material or decrypts a payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageCipher {
    Aes256Gcm,
    XChaCha20Poly1305,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageEncryptionBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub cipher: StorageCipher,
    pub key_ref: SecretRef,
    pub key_purpose: String,
    pub owner_scope_digest: String,
    pub namespace: String,
    pub ciphertext_digest: String,
    pub binding_digest: String,
}

impl StorageEncryptionBinding {
    pub fn new(
        cipher: StorageCipher,
        key_ref: SecretRef,
        key_purpose: impl Into<String>,
        owner_scope_digest: impl Into<String>,
        namespace: impl Into<String>,
        ciphertext_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: STORAGE_ENCRYPTION_BINDING_SCHEMA.to_owned(),
            version: STORAGE_SECURITY_SCHEMA_VERSION,
            cipher,
            key_ref,
            key_purpose: key_purpose.into(),
            owner_scope_digest: owner_scope_digest.into(),
            namespace: namespace.into(),
            ciphertext_digest: ciphertext_digest.into(),
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    /// Recheck the key against the scope that is about to decrypt bytes. A valid SecretRef alone
    /// does not grant access to another owner, namespace or purpose.
    pub fn validate_for_scope(
        &self,
        owner_scope_digest: &str,
        namespace: &str,
        key_purpose: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if self.owner_scope_digest != owner_scope_digest {
            return Err("storage_encryption_owner_scope_mismatch".to_owned());
        }
        if self.namespace != namespace {
            return Err("storage_encryption_namespace_mismatch".to_owned());
        }
        if self.key_purpose != key_purpose || self.key_ref.purpose != key_purpose {
            return Err("storage_encryption_key_purpose_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_ENCRYPTION_BINDING_SCHEMA
            || !STORAGE_SECURITY_SCHEMA_VERSION.is_compatible_with(&self.version)
        {
            return Err("storage_encryption_header_invalid".to_owned());
        }
        self.key_ref.validate()?;
        validate_secret_free_text(&self.key_ref.key, "storage_encryption_key")?;
        required(&self.key_purpose, "storage_encryption_key_purpose")?;
        required(&self.owner_scope_digest, "storage_encryption_owner_scope")?;
        validate_digest(&self.owner_scope_digest, "storage_encryption_owner_scope")?;
        required(&self.namespace, "storage_encryption_namespace")?;
        validate_relative_path(&self.namespace, "storage_encryption_namespace")?;
        validate_digest(&self.ciphertext_digest, "storage_encryption_ciphertext")?;
        validate_digest(&self.binding_digest, "storage_encryption_binding")?;
        if self.binding_digest != self.digest() {
            return Err("storage_encryption_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "cipher": self.cipher,
            "key_ref": self.key_ref,
            "key_purpose": self.key_purpose,
            "owner_scope_digest": self.owner_scope_digest,
            "namespace": self.namespace,
            "ciphertext_digest": self.ciphertext_digest,
        }))
    }
}

/// A metadata-only identity captured by a path adapter before opening or replacing a file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageFileIdentity {
    pub schema: String,
    pub version: SchemaVersion,
    pub root_digest: String,
    pub relative_path: String,
    pub exists: bool,
    pub regular_file: bool,
    pub symlink: bool,
    pub hardlink: bool,
    #[serde(default)]
    pub mode: Option<u32>,
    pub identity_digest: String,
}

impl StorageFileIdentity {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        root_digest: impl Into<String>,
        relative_path: impl Into<String>,
        exists: bool,
        regular_file: bool,
        symlink: bool,
        hardlink: bool,
        mode: Option<u32>,
    ) -> Result<Self, String> {
        let mut identity = Self {
            schema: STORAGE_FILE_IDENTITY_SCHEMA.to_owned(),
            version: STORAGE_SECURITY_SCHEMA_VERSION,
            root_digest: root_digest.into(),
            relative_path: relative_path.into(),
            exists,
            regular_file,
            symlink,
            hardlink,
            mode,
            identity_digest: String::new(),
        };
        identity.identity_digest = identity.digest();
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_FILE_IDENTITY_SCHEMA
            || !STORAGE_SECURITY_SCHEMA_VERSION.is_compatible_with(&self.version)
        {
            return Err("storage_file_identity_header_invalid".to_owned());
        }
        validate_digest(&self.root_digest, "storage_file_identity_root")?;
        validate_relative_path(&self.relative_path, "storage_file_identity_path")?;
        if self.exists && (!self.regular_file || self.symlink || self.hardlink) {
            return Err("storage_file_identity_unsafe_target".to_owned());
        }
        if !self.exists && (self.regular_file || self.symlink || self.hardlink) {
            return Err("storage_file_identity_missing_flags".to_owned());
        }
        if self.mode.is_some_and(|mode| mode & 0o077 != 0) {
            return Err("storage_file_permissions_too_broad".to_owned());
        }
        validate_digest(&self.identity_digest, "storage_file_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("storage_file_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "root_digest": self.root_digest,
            "relative_path": self.relative_path,
            "exists": self.exists,
            "regular_file": self.regular_file,
            "symlink": self.symlink,
            "hardlink": self.hardlink,
            "mode": self.mode,
        }))
    }
}

/// Explicitly report what a platform adapter can prove. Unsupported capabilities stay visible
/// instead of being silently treated as equivalent durable guarantees.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageSecurityCapabilities {
    pub schema: String,
    pub version: SchemaVersion,
    pub platform: String,
    pub symlink_guard: bool,
    pub hardlink_guard: bool,
    pub permission_guard: bool,
    pub atomic_replace: bool,
    pub opaque_encryption_refs: bool,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub capability_digest: String,
}

impl StorageSecurityCapabilities {
    pub fn current() -> Self {
        let (platform, symlink_guard, hardlink_guard, permission_guard, atomic_replace) = if cfg!(
            target_os = "linux"
        ) {
            ("linux".to_owned(), true, true, true, true)
        } else if cfg!(unix) {
            ("unix".to_owned(), true, true, true, true)
        } else {
            ("non_unix".to_owned(), false, false, false, false)
        };
        let mut capabilities = Self {
            schema: STORAGE_SECURITY_CAPABILITIES_SCHEMA.to_owned(),
            version: STORAGE_SECURITY_SCHEMA_VERSION,
            platform,
            symlink_guard,
            hardlink_guard,
            permission_guard,
            atomic_replace,
            opaque_encryption_refs: true,
            limitations: Vec::new(),
            capability_digest: String::new(),
        };
        if !capabilities.symlink_guard {
            capabilities
                .limitations
                .push("symlink_guard_requires_adapter_check".to_owned());
        }
        if !capabilities.hardlink_guard {
            capabilities
                .limitations
                .push("hardlink_identity_requires_adapter_check".to_owned());
        }
        if !capabilities.permission_guard {
            capabilities
                .limitations
                .push("permission_mode_unavailable".to_owned());
        }
        if !capabilities.atomic_replace {
            capabilities
                .limitations
                .push("atomic_replace_semantics_unproven".to_owned());
        }
        capabilities.capability_digest = capabilities.digest();
        capabilities
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_SECURITY_CAPABILITIES_SCHEMA
            || !STORAGE_SECURITY_SCHEMA_VERSION.is_compatible_with(&self.version)
        {
            return Err("storage_security_capabilities_header_invalid".to_owned());
        }
        if self.limitations.len() > MAX_STORAGE_SECURITY_LIMITATIONS {
            return Err("storage_security_capabilities_limitations_exceeded".to_owned());
        }
        for limitation in &self.limitations {
            required(limitation, "storage_security_limitation")?;
        }
        if (!self.symlink_guard
            || !self.hardlink_guard
            || !self.permission_guard
            || !self.atomic_replace)
            && self.limitations.is_empty()
        {
            return Err("storage_security_capability_limitation_missing".to_owned());
        }
        validate_digest(&self.capability_digest, "storage_security_capability_digest")?;
        if self.capability_digest != self.digest() {
            return Err("storage_security_capability_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "platform": self.platform,
            "symlink_guard": self.symlink_guard,
            "hardlink_guard": self.hardlink_guard,
            "permission_guard": self.permission_guard,
            "atomic_replace": self.atomic_replace,
            "opaque_encryption_refs": self.opaque_encryption_refs,
            "limitations": self.limitations,
        }))
    }
}

/// Persisted JSON, event and diagnostic values must be checked before crossing a storage boundary.
pub fn validate_secret_free(value: &Value) -> Result<(), String> {
    if redact_value(value) != *value {
        return Err("storage_secret_sentinel_detected".to_owned());
    }
    Ok(())
}

pub fn validate_secret_free_text(value: &str, field: &str) -> Result<(), String> {
    if value.len() > MAX_STORAGE_SECURITY_TEXT_BYTES {
        return Err(format!("{field}_too_long"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_secret_sentinel"));
    }
    Ok(())
}

fn required(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_STORAGE_SECURITY_TEXT_BYTES {
        return Err(format!("{field}_required"));
    }
    validate_secret_free_text(value, field)
}

fn validate_relative_path(value: &str, field: &str) -> Result<(), String> {
    required(value, field)?;
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
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
