//! Manifest-bound capability intersection for extensions.
//!
//! An extension declaration is input to this catalog, never an authorization grant. The
//! effective set is the intersection of host, parent grant and approval capabilities, and the
//! manifest digest binds the exact signed metadata used for the decision. `provided_capabilities`
//! and skill `allowed-tools` remain descriptive declarations and never enter the grant set.
//!
//! A consumer that already holds a catalog resolves the freshly observed manifest through
//! [`CapabilityCatalog::authorize_manifest`]. Identity, version, capability-set and digest drift
//! fail closed before any capability is handed out, and a denied entry hands out nothing while
//! keeping its requested set and deny reason for audit.

use crate::{json_digest, ExtensionEffect, ExtensionManifest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const CAPABILITY_CATALOG_SCHEMA: &str = "kiana.capability-catalog.v1";
pub const CAPABILITY_CATALOG_ENTRY_SCHEMA: &str = "kiana.capability-catalog-entry.v1";
pub const MAX_CAPABILITY_CATALOG_ENTRIES: usize = 256;
pub const MAX_EXTENSION_CAPABILITIES: usize = 64;

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn bounded_capabilities(values: &BTreeSet<String>, field: &str) -> Result<(), String> {
    if values.len() > MAX_EXTENSION_CAPABILITIES
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 256 || value.contains('\0'))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn read_only_capability(capability: &str) -> bool {
    matches!(
        capability,
        "memory.search" | "context.query" | "extension.inspect"
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityCatalogDecision {
    Allowed,
    Denied,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityCatalogEntry {
    pub schema: String,
    pub extension_id: String,
    pub version: String,
    pub manifest_digest: String,
    pub effect: ExtensionEffect,
    pub requested_capabilities: BTreeSet<String>,
    pub effective_capabilities: BTreeSet<String>,
    pub decision: CapabilityCatalogDecision,
    pub reason: String,
    pub entry_digest: String,
}

impl CapabilityCatalogEntry {
    fn new(
        manifest: &ExtensionManifest,
        available: &BTreeSet<String>,
        grant: &BTreeSet<String>,
        approval: &BTreeSet<String>,
    ) -> Result<Self, String> {
        manifest
            .validate()
            .map_err(|error| format!("extension_manifest:{error}"))?;
        let manifest_digest = extension_manifest_digest(manifest)?;
        let requested_capabilities = manifest.required_capabilities.clone();
        bounded_capabilities(&requested_capabilities, "capability_requested")?;
        let mut effective_capabilities = requested_capabilities
            .intersection(available)
            .filter(|capability| grant.contains(*capability) && approval.contains(*capability))
            .cloned()
            .collect::<BTreeSet<_>>();
        let missing_intersection = requested_capabilities
            .difference(&effective_capabilities)
            .next()
            .cloned();
        let read_only_write = manifest.effect == ExtensionEffect::ReadOnly
            && requested_capabilities
                .iter()
                .any(|capability| !read_only_capability(capability));
        let (decision, reason) = if read_only_write {
            // A denied read-only manifest must not retain an executable write capability in the
            // projected effective set. Keep the requested set for audit, but make the deny result
            // inert before it is digested or handed to any caller.
            effective_capabilities.clear();
            (
                CapabilityCatalogDecision::Denied,
                "extension_read_only_write_denied".to_owned(),
            )
        } else if let Some(missing) = missing_intersection {
            (
                CapabilityCatalogDecision::Denied,
                format!("capability_intersection_missing:{missing}"),
            )
        } else {
            (
                CapabilityCatalogDecision::Allowed,
                "capability_intersection_allowed".to_owned(),
            )
        };
        let mut entry = Self {
            schema: CAPABILITY_CATALOG_ENTRY_SCHEMA.to_owned(),
            extension_id: manifest.extension_id.clone(),
            version: manifest.version.clone(),
            manifest_digest,
            effect: manifest.effect,
            requested_capabilities,
            effective_capabilities,
            decision,
            reason,
            entry_digest: String::new(),
        };
        entry.entry_digest = entry.digest();
        entry.validate()?;
        Ok(entry)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CAPABILITY_CATALOG_ENTRY_SCHEMA
            || self.extension_id.trim().is_empty()
            || self.version.trim().is_empty()
        {
            return Err("capability_catalog_entry_header_invalid".to_owned());
        }
        digest(&self.manifest_digest, "capability_manifest_digest")?;
        bounded_capabilities(&self.requested_capabilities, "capability_requested")?;
        bounded_capabilities(&self.effective_capabilities, "capability_effective")?;
        if !self
            .effective_capabilities
            .is_subset(&self.requested_capabilities)
        {
            return Err("capability_effective_set_widened".to_owned());
        }
        if self.decision == CapabilityCatalogDecision::Allowed
            && self.reason != "capability_intersection_allowed"
        {
            return Err("capability_catalog_allow_reason_invalid".to_owned());
        }
        if self.decision == CapabilityCatalogDecision::Denied
            && self.reason == "capability_intersection_allowed"
        {
            return Err("capability_catalog_deny_reason_invalid".to_owned());
        }
        digest(&self.entry_digest, "capability_entry_digest")?;
        if self.entry_digest != self.digest() {
            return Err("capability_catalog_entry_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "extension_id": self.extension_id,
            "version": self.version,
            "manifest_digest": self.manifest_digest,
            "effect": self.effect,
            "requested_capabilities": self.requested_capabilities,
            "effective_capabilities": self.effective_capabilities,
            "decision": self.decision,
            "reason": self.reason,
        }))
    }

    /// Hand out one capability, and only one that this sealed entry actually made effective.
    ///
    /// The entry validates itself first, so a widened or resealed snapshot cannot be consumed.
    /// A denied entry hands out nothing and reports its deny reason for audit.
    pub fn authorize(&self, capability: &str) -> Result<(), String> {
        self.validate()?;
        if capability.trim().is_empty() || capability.len() > 256 || capability.contains('\0') {
            return Err("capability_catalog_request_invalid".to_owned());
        }
        if self.decision != CapabilityCatalogDecision::Allowed {
            return Err(format!("capability_catalog_entry_denied:{}", self.reason));
        }
        if !self.effective_capabilities.contains(capability) {
            return Err(format!("capability_not_effective:{capability}"));
        }
        Ok(())
    }

    /// Compare a freshly observed manifest against the exact metadata this entry was sealed on.
    ///
    /// Identity, version, capability-set or digest drift means the entry no longer describes the
    /// manifest; every drift class fails closed before a caller can reuse the old decision.
    pub fn check_manifest_drift(&self, manifest: &ExtensionManifest) -> Result<(), String> {
        self.validate()?;
        manifest
            .validate()
            .map_err(|error| format!("extension_manifest:{error}"))?;
        if manifest.extension_id != self.extension_id {
            return Err(format!(
                "capability_catalog_extension_identity_drift:{}",
                manifest.extension_id
            ));
        }
        if manifest.version != self.version {
            return Err(format!(
                "capability_catalog_manifest_version_drift:{}",
                manifest.version
            ));
        }
        if manifest.required_capabilities != self.requested_capabilities {
            let drifted = manifest
                .required_capabilities
                .difference(&self.requested_capabilities)
                .chain(
                    self.requested_capabilities
                        .difference(&manifest.required_capabilities),
                )
                .next()
                .cloned()
                .unwrap_or_default();
            return Err(format!(
                "capability_catalog_manifest_capability_drift:{drifted}"
            ));
        }
        if extension_manifest_digest(manifest)? != self.manifest_digest {
            return Err("capability_catalog_manifest_digest_drift".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityCatalog {
    pub schema: String,
    pub generation: u64,
    pub entries: Vec<CapabilityCatalogEntry>,
    pub catalog_digest: String,
}

impl CapabilityCatalog {
    pub fn from_manifests(
        generation: u64,
        manifests: &[ExtensionManifest],
        available: &BTreeSet<String>,
        grant: &BTreeSet<String>,
        approval: &BTreeSet<String>,
    ) -> Result<Self, String> {
        if generation == 0 || manifests.len() > MAX_CAPABILITY_CATALOG_ENTRIES {
            return Err("capability_catalog_header_invalid".to_owned());
        }
        bounded_capabilities(available, "capability_available")?;
        bounded_capabilities(grant, "capability_grant")?;
        bounded_capabilities(approval, "capability_approval")?;
        let mut entries = manifests
            .iter()
            .map(|manifest| CapabilityCatalogEntry::new(manifest, available, grant, approval))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by(|left, right| {
            left.extension_id
                .cmp(&right.extension_id)
                .then_with(|| left.version.cmp(&right.version))
                .then_with(|| left.manifest_digest.cmp(&right.manifest_digest))
        });
        if entries
            .windows(2)
            .any(|pair| pair[0].extension_id == pair[1].extension_id)
        {
            return Err("capability_catalog_extension_duplicate".to_owned());
        }
        let mut catalog = Self {
            schema: CAPABILITY_CATALOG_SCHEMA.to_owned(),
            generation,
            entries,
            catalog_digest: String::new(),
        };
        catalog.catalog_digest = catalog.digest();
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CAPABILITY_CATALOG_SCHEMA
            || self.generation == 0
            || self.entries.len() > MAX_CAPABILITY_CATALOG_ENTRIES
        {
            return Err("capability_catalog_header_invalid".to_owned());
        }
        let mut extension_ids = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !extension_ids.insert(entry.extension_id.clone()) {
                return Err("capability_catalog_extension_duplicate".to_owned());
            }
        }
        digest(&self.catalog_digest, "capability_catalog_digest")?;
        if self.catalog_digest != self.digest() {
            return Err("capability_catalog_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert("catalog_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }

    /// Resolve the sealed entry for a manifest, refusing unknown or drifted manifests.
    ///
    /// The catalog and the manifest both self-validate first, so a tampered snapshot or a
    /// malformed manifest cannot select an entry; an unknown extension and every drift class are
    /// distinct stable reasons.
    pub fn entry_for_manifest(
        &self,
        manifest: &ExtensionManifest,
    ) -> Result<&CapabilityCatalogEntry, String> {
        self.validate()?;
        manifest
            .validate()
            .map_err(|error| format!("extension_manifest:{error}"))?;
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.extension_id == manifest.extension_id)
            .ok_or_else(|| {
                format!(
                    "capability_catalog_extension_unknown:{}",
                    manifest.extension_id
                )
            })?;
        entry.check_manifest_drift(manifest)?;
        Ok(entry)
    }

    /// Deny-first consumer entry point: resolve the manifest, then hand out one capability.
    pub fn authorize_manifest(
        &self,
        manifest: &ExtensionManifest,
        capability: &str,
    ) -> Result<(), String> {
        self.entry_for_manifest(manifest)?.authorize(capability)
    }
}

pub fn extension_manifest_digest(manifest: &ExtensionManifest) -> Result<String, String> {
    manifest
        .validate()
        .map_err(|error| format!("extension_manifest:{error}"))?;
    let value = serde_json::to_value(manifest)
        .map_err(|_| "extension_manifest_digest_encode_failed".to_owned())?;
    Ok(json_digest(&value))
}
