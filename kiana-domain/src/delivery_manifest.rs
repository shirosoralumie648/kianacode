//! Versioned local delivery manifest and package-boundary contract.
//!
//! The manifest is an immutable description of accepted artifacts.  A local package is a
//! successor descriptor whose entries must match that manifest exactly; neither type performs
//! filesystem I/O or claims that a recipient received anything.

use crate::{json_digest, AcceptanceStatus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const DELIVERY_MANIFEST_SCHEMA: &str = "kiana.delivery-manifest.v1";
pub const LOCAL_DELIVERY_PACKAGE_SCHEMA: &str = "kiana.local-delivery-package.v1";
const MAX_ARTIFACTS: usize = 256;
const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field)
    }
}

fn safe_relative_path(value: &str, field: &'static str) -> Result<(), &'static str> {
    required(value, field)?;
    if value.starts_with('/')
        || value.contains('\\')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(field);
    }
    Ok(())
}

fn unique(values: &[String], field: &'static str) -> Result<(), &'static str> {
    if values.iter().collect::<BTreeSet<_>>().len() != values.len() {
        return Err(field);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryManifestArtifact {
    pub artifact_ref: String,
    pub project_id: String,
    #[serde(default)]
    pub packet_id: Option<String>,
    pub artifact_version: u64,
    pub relative_path: String,
    pub content_hash: String,
    pub content_size: u64,
    pub producer_run_refs: Vec<String>,
}

impl DeliveryManifestArtifact {
    fn validate(&self, project_id: &str) -> Result<(), &'static str> {
        if self.project_id != project_id || self.artifact_version == 0 {
            return Err("delivery_manifest_artifact_binding_invalid");
        }
        required(
            &self.artifact_ref,
            "delivery_manifest_artifact_ref_required",
        )?;
        if !self.artifact_ref.starts_with("artifact:")
            || self.artifact_ref.trim_start_matches("artifact:").is_empty()
        {
            return Err("delivery_manifest_artifact_ref_invalid");
        }
        if let Some(packet_id) = &self.packet_id {
            required(packet_id, "delivery_manifest_packet_ref_invalid")?;
        }
        safe_relative_path(
            &self.relative_path,
            "delivery_manifest_artifact_path_invalid",
        )?;
        digest(
            &self.content_hash,
            "delivery_manifest_artifact_hash_invalid",
        )?;
        if self.producer_run_refs.is_empty() || self.producer_run_refs.len() > 64 {
            return Err("delivery_manifest_producer_runs_required");
        }
        for reference in &self.producer_run_refs {
            required(reference, "delivery_manifest_producer_run_invalid")?;
        }
        unique(
            &self.producer_run_refs,
            "delivery_manifest_duplicate_producer_run",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryManifest {
    pub schema: String,
    pub manifest_id: String,
    pub delivery_id: String,
    pub project_id: String,
    pub baseline_version: u64,
    pub acceptance_id: String,
    pub acceptance_status: AcceptanceStatus,
    pub acceptance_digest: String,
    pub channel: String,
    pub destination: String,
    pub recipient_ref: String,
    pub residual_obligations: Vec<String>,
    pub artifacts: Vec<DeliveryManifestArtifact>,
    pub digest: String,
}

impl DeliveryManifest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DELIVERY_MANIFEST_SCHEMA
            || self.baseline_version == 0
            || self.acceptance_status != AcceptanceStatus::Accepted
            || self.channel != "local_package"
            || self.artifacts.is_empty()
            || self.artifacts.len() > MAX_ARTIFACTS
            || self.residual_obligations.len() > 128
        {
            return Err("delivery_manifest_header_invalid");
        }
        for (value, field) in [
            (&self.manifest_id, "delivery_manifest_id_required"),
            (&self.delivery_id, "delivery_manifest_delivery_required"),
            (&self.project_id, "delivery_manifest_project_required"),
            (&self.acceptance_id, "delivery_manifest_acceptance_required"),
            (&self.channel, "delivery_manifest_channel_required"),
            (&self.recipient_ref, "delivery_manifest_recipient_required"),
        ] {
            required(value, field)?;
        }
        digest(
            &self.acceptance_digest,
            "delivery_manifest_acceptance_digest_invalid",
        )?;
        safe_relative_path(&self.destination, "delivery_manifest_destination_invalid")?;
        if self.destination != format!("lessons/deliveries/{}.json", self.delivery_id) {
            return Err("delivery_manifest_destination_invalid");
        }
        for obligation in &self.residual_obligations {
            required(obligation, "delivery_manifest_obligation_invalid")?;
        }
        let mut refs = Vec::with_capacity(self.artifacts.len());
        let mut paths = Vec::with_capacity(self.artifacts.len());
        let mut total = 0u64;
        for artifact in &self.artifacts {
            artifact.validate(&self.project_id)?;
            refs.push(artifact.artifact_ref.clone());
            paths.push(artifact.relative_path.clone());
            total = total
                .checked_add(artifact.content_size)
                .ok_or("delivery_manifest_size_overflow")?;
        }
        unique(&refs, "delivery_manifest_duplicate_artifact")?;
        unique(&paths, "delivery_manifest_duplicate_path")?;
        if total > MAX_TOTAL_BYTES {
            return Err("delivery_manifest_package_too_large");
        }
        digest(&self.digest, "delivery_manifest_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("delivery_manifest_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "manifest_id": self.manifest_id,
            "delivery_id": self.delivery_id,
            "project_id": self.project_id,
            "baseline_version": self.baseline_version,
            "acceptance_id": self.acceptance_id,
            "acceptance_status": self.acceptance_status,
            "acceptance_digest": self.acceptance_digest,
            "channel": self.channel,
            "destination": self.destination,
            "recipient_ref": self.recipient_ref,
            "residual_obligations": self.residual_obligations,
            "artifacts": self.artifacts,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalPackageEntry {
    pub artifact_ref: String,
    pub relative_path: String,
    pub content_hash: String,
    pub content_size: u64,
    pub symlink: bool,
}

impl LocalPackageEntry {
    fn validate(&self) -> Result<(), &'static str> {
        required(&self.artifact_ref, "local_package_artifact_ref_required")?;
        if !self.artifact_ref.starts_with("artifact:") {
            return Err("local_package_artifact_ref_invalid");
        }
        safe_relative_path(&self.relative_path, "local_package_path_invalid")?;
        digest(&self.content_hash, "local_package_hash_invalid")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalDeliveryPackage {
    pub schema: String,
    pub package_id: String,
    pub manifest_id: String,
    pub manifest_digest: String,
    pub package_root: String,
    pub entries: Vec<LocalPackageEntry>,
    pub package_digest: String,
}

impl LocalDeliveryPackage {
    pub fn validate_against(&self, manifest: &DeliveryManifest) -> Result<(), &'static str> {
        manifest.validate()?;
        if self.schema != LOCAL_DELIVERY_PACKAGE_SCHEMA
            || self.entries.len() != manifest.artifacts.len()
        {
            return Err("local_package_header_invalid");
        }
        required(&self.package_id, "local_package_id_required")?;
        if self.manifest_id != manifest.manifest_id || self.manifest_digest != manifest.digest {
            return Err("local_package_manifest_binding_invalid");
        }
        safe_relative_path(&self.package_root, "local_package_root_invalid")?;
        let expected = manifest
            .artifacts
            .iter()
            .map(|artifact| {
                (
                    artifact.artifact_ref.as_str(),
                    (
                        artifact.relative_path.as_str(),
                        artifact.content_hash.as_str(),
                        artifact.content_size,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut actual = BTreeMap::new();
        for entry in &self.entries {
            entry.validate()?;
            if entry.symlink {
                return Err("local_package_symlink_forbidden");
            }
            if actual
                .insert(
                    entry.artifact_ref.as_str(),
                    (
                        entry.relative_path.as_str(),
                        entry.content_hash.as_str(),
                        entry.content_size,
                    ),
                )
                .is_some()
            {
                return Err("local_package_duplicate_artifact");
            }
        }
        if actual != expected {
            return Err("local_package_manifest_entries_mismatch");
        }
        if self.package_digest != self.canonical_digest() {
            return Err("local_package_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "package_id": self.package_id,
            "manifest_id": self.manifest_id,
            "manifest_digest": self.manifest_digest,
            "package_root": self.package_root,
            "entries": self.entries,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeliveryManifestLedger {
    pub manifests: BTreeMap<String, DeliveryManifest>,
    pub packages: BTreeMap<String, LocalDeliveryPackage>,
}

impl DeliveryManifestLedger {
    pub fn publish_manifest(&mut self, manifest: DeliveryManifest) -> Result<(), &'static str> {
        manifest.validate()?;
        if let Some(existing) = self.manifests.get(&manifest.delivery_id) {
            if existing.digest == manifest.digest {
                return Ok(());
            }
            return Err("delivery_manifest_duplicate_digest_mismatch");
        }
        self.manifests
            .insert(manifest.delivery_id.clone(), manifest);
        Ok(())
    }

    pub fn record_package(&mut self, package: LocalDeliveryPackage) -> Result<(), &'static str> {
        let manifest = self
            .manifests
            .values()
            .find(|manifest| manifest.manifest_id == package.manifest_id)
            .ok_or("local_package_manifest_not_found")?;
        package.validate_against(manifest)?;
        if let Some(existing) = self.packages.get(&package.package_id) {
            if existing.package_digest == package.package_digest {
                return Ok(());
            }
            return Err("local_package_duplicate_digest_mismatch");
        }
        self.packages.insert(package.package_id.clone(), package);
        Ok(())
    }

    pub fn manifest_for_delivery(&self, delivery_id: &str) -> Option<&DeliveryManifest> {
        self.manifests.get(delivery_id)
    }
}
