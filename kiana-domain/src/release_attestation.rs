//! Release artifact, provenance and signature binding contracts.
//!
//! This module is deliberately an evidence boundary. It validates names, digests and bindings
//! supplied by a release adapter, but it never signs, publishes, contacts a transparency log or
//! treats a filename/tag as proof of the bytes it names. A verifier must receive an external
//! signature check result and bind it to the exact manifest digest.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RELEASE_MANIFEST_SCHEMA: &str = "kiana.release-manifest.v1";
pub const RELEASE_PROVENANCE_SCHEMA: &str = "kiana.release-provenance.v1";
pub const RELEASE_SIGNATURE_SCHEMA: &str = "kiana.release-signature-attestation.v1";
pub const RELEASE_VERIFICATION_SCHEMA: &str = "kiana.release-verification.v1";
pub const RELEASE_ATTESTATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RELEASE_ARTIFACTS: usize = 128;
pub const MAX_RELEASE_MATERIALS: usize = 128;
pub const MAX_RELEASE_TEXT_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseArtifactKind {
    Binary,
    Archive,
    DesktopPackage,
    Checksum,
    Sbom,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseArtifactSubject {
    pub name: String,
    pub kind: ReleaseArtifactKind,
    pub target: String,
    pub digest: String,
    pub size_bytes: u64,
    pub media_type: String,
}

impl ReleaseArtifactSubject {
    pub fn validate(&self) -> Result<(), String> {
        safe_name(&self.name, "release_artifact_name")?;
        bounded(&self.target, "release_artifact_target")?;
        bounded(&self.media_type, "release_artifact_media_type")?;
        digest(&self.digest, "release_artifact_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema: String,
    pub version: SchemaVersion,
    pub release_id: String,
    pub tag: String,
    pub source_revision: String,
    pub source_tree_digest: String,
    pub cargo_lock_digest: String,
    pub toolchain_digest: String,
    pub builder_id: String,
    pub artifacts: Vec<ReleaseArtifactSubject>,
    pub sbom_digest: String,
    pub manifest_digest: String,
}

impl ReleaseManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        release_id: impl Into<String>,
        tag: impl Into<String>,
        source_revision: impl Into<String>,
        source_tree_digest: impl Into<String>,
        cargo_lock_digest: impl Into<String>,
        toolchain_digest: impl Into<String>,
        builder_id: impl Into<String>,
        artifacts: Vec<ReleaseArtifactSubject>,
        sbom_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut manifest = Self {
            schema: RELEASE_MANIFEST_SCHEMA.to_owned(),
            version: RELEASE_ATTESTATION_VERSION,
            release_id: release_id.into(),
            tag: tag.into(),
            source_revision: source_revision.into(),
            source_tree_digest: source_tree_digest.into(),
            cargo_lock_digest: cargo_lock_digest.into(),
            toolchain_digest: toolchain_digest.into(),
            builder_id: builder_id.into(),
            artifacts,
            sbom_digest: sbom_digest.into(),
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RELEASE_MANIFEST_SCHEMA
            || self.version != RELEASE_ATTESTATION_VERSION
        {
            return Err("release_manifest_header_invalid".to_owned());
        }
        bounded(&self.release_id, "release_manifest_release_id")?;
        bounded(&self.tag, "release_manifest_tag")?;
        if !self.tag.starts_with('v') || self.tag.contains(['/', '\\', ' ', '\n', '\r']) {
            return Err("release_manifest_tag_invalid".to_owned());
        }
        bounded(&self.source_revision, "release_manifest_source_revision")?;
        for (value, field) in [
            (&self.source_tree_digest, "release_manifest_source_tree_digest"),
            (&self.cargo_lock_digest, "release_manifest_cargo_lock_digest"),
            (&self.toolchain_digest, "release_manifest_toolchain_digest"),
            (&self.sbom_digest, "release_manifest_sbom_digest"),
            (&self.manifest_digest, "release_manifest_digest"),
        ] {
            digest(value, field)?;
        }
        bounded(&self.builder_id, "release_manifest_builder_id")?;
        if self.artifacts.is_empty() || self.artifacts.len() > MAX_RELEASE_ARTIFACTS {
            return Err("release_manifest_artifact_count_invalid".to_owned());
        }
        let mut names = BTreeSet::new();
        for artifact in &self.artifacts {
            artifact.validate()?;
            if !names.insert(&artifact.name) {
                return Err("release_manifest_artifact_duplicate".to_owned());
            }
        }
        if self.manifest_digest != self.digest() {
            return Err("release_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "release_id": self.release_id,
            "tag": self.tag,
            "source_revision": self.source_revision,
            "source_tree_digest": self.source_tree_digest,
            "cargo_lock_digest": self.cargo_lock_digest,
            "toolchain_digest": self.toolchain_digest,
            "builder_id": self.builder_id,
            "artifacts": self.artifacts,
            "sbom_digest": self.sbom_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseMaterial {
    pub uri: String,
    pub digest: String,
}

impl ReleaseMaterial {
    pub fn validate(&self) -> Result<(), String> {
        bounded(&self.uri, "release_material_uri")?;
        digest(&self.digest, "release_material_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseProvenance {
    pub schema: String,
    pub version: SchemaVersion,
    pub build_type: String,
    pub builder_id: String,
    pub invocation_digest: String,
    pub source_revision: String,
    pub source_tree_digest: String,
    pub cargo_lock_digest: String,
    pub toolchain_digest: String,
    pub materials: Vec<ReleaseMaterial>,
    pub subject_manifest_digest: String,
    pub provenance_digest: String,
}

impl ReleaseProvenance {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        build_type: impl Into<String>,
        builder_id: impl Into<String>,
        invocation_digest: impl Into<String>,
        source_revision: impl Into<String>,
        source_tree_digest: impl Into<String>,
        cargo_lock_digest: impl Into<String>,
        toolchain_digest: impl Into<String>,
        materials: Vec<ReleaseMaterial>,
        subject_manifest_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut provenance = Self {
            schema: RELEASE_PROVENANCE_SCHEMA.to_owned(),
            version: RELEASE_ATTESTATION_VERSION,
            build_type: build_type.into(),
            builder_id: builder_id.into(),
            invocation_digest: invocation_digest.into(),
            source_revision: source_revision.into(),
            source_tree_digest: source_tree_digest.into(),
            cargo_lock_digest: cargo_lock_digest.into(),
            toolchain_digest: toolchain_digest.into(),
            materials,
            subject_manifest_digest: subject_manifest_digest.into(),
            provenance_digest: String::new(),
        };
        provenance.provenance_digest = provenance.digest();
        provenance.validate()?;
        Ok(provenance)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RELEASE_PROVENANCE_SCHEMA
            || self.version != RELEASE_ATTESTATION_VERSION
        {
            return Err("release_provenance_header_invalid".to_owned());
        }
        bounded(&self.build_type, "release_provenance_build_type")?;
        bounded(&self.builder_id, "release_provenance_builder_id")?;
        bounded(&self.source_revision, "release_provenance_source_revision")?;
        for (value, field) in [
            (&self.invocation_digest, "release_provenance_invocation_digest"),
            (&self.source_tree_digest, "release_provenance_source_tree_digest"),
            (&self.cargo_lock_digest, "release_provenance_cargo_lock_digest"),
            (&self.toolchain_digest, "release_provenance_toolchain_digest"),
            (&self.subject_manifest_digest, "release_provenance_subject_digest"),
            (&self.provenance_digest, "release_provenance_digest"),
        ] {
            digest(value, field)?;
        }
        if self.materials.is_empty() || self.materials.len() > MAX_RELEASE_MATERIALS {
            return Err("release_provenance_material_count_invalid".to_owned());
        }
        let mut uris = BTreeSet::new();
        for material in &self.materials {
            material.validate()?;
            if !uris.insert(&material.uri) {
                return Err("release_provenance_material_duplicate".to_owned());
            }
        }
        if self.provenance_digest != self.digest() {
            return Err("release_provenance_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "build_type": self.build_type,
            "builder_id": self.builder_id,
            "invocation_digest": self.invocation_digest,
            "source_revision": self.source_revision,
            "source_tree_digest": self.source_tree_digest,
            "cargo_lock_digest": self.cargo_lock_digest,
            "toolchain_digest": self.toolchain_digest,
            "materials": self.materials,
            "subject_manifest_digest": self.subject_manifest_digest,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseSignatureAlgorithm {
    Ed25519,
    Sigstore,
    External,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSignatureAttestation {
    pub schema: String,
    pub version: SchemaVersion,
    pub algorithm: ReleaseSignatureAlgorithm,
    pub signer_id: String,
    pub key_id: String,
    pub subject_manifest_digest: String,
    pub signature_digest: String,
    pub transparency_log_entry_digest: String,
    pub verifier_id: String,
    pub verified: bool,
    pub attestation_digest: String,
}

impl ReleaseSignatureAttestation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        algorithm: ReleaseSignatureAlgorithm,
        signer_id: impl Into<String>,
        key_id: impl Into<String>,
        subject_manifest_digest: impl Into<String>,
        signature_digest: impl Into<String>,
        transparency_log_entry_digest: impl Into<String>,
        verifier_id: impl Into<String>,
        verified: bool,
    ) -> Result<Self, String> {
        let mut attestation = Self {
            schema: RELEASE_SIGNATURE_SCHEMA.to_owned(),
            version: RELEASE_ATTESTATION_VERSION,
            algorithm,
            signer_id: signer_id.into(),
            key_id: key_id.into(),
            subject_manifest_digest: subject_manifest_digest.into(),
            signature_digest: signature_digest.into(),
            transparency_log_entry_digest: transparency_log_entry_digest.into(),
            verifier_id: verifier_id.into(),
            verified,
            attestation_digest: String::new(),
        };
        attestation.attestation_digest = attestation.digest();
        attestation.validate()?;
        Ok(attestation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RELEASE_SIGNATURE_SCHEMA || self.version != RELEASE_ATTESTATION_VERSION
        {
            return Err("release_signature_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.signer_id, "release_signature_signer"),
            (&self.key_id, "release_signature_key"),
            (&self.verifier_id, "release_signature_verifier"),
        ] {
            bounded(value, field)?;
        }
        for (value, field) in [
            (&self.subject_manifest_digest, "release_signature_subject_digest"),
            (&self.signature_digest, "release_signature_digest"),
            (&self.attestation_digest, "release_signature_attestation_digest"),
        ] {
            digest(value, field)?;
        }
        if !self.transparency_log_entry_digest.is_empty() {
            digest(
                &self.transparency_log_entry_digest,
                "release_signature_transparency_digest",
            )?;
        }
        if self.attestation_digest != self.digest() {
            return Err("release_signature_attestation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "algorithm": self.algorithm,
            "signer_id": self.signer_id,
            "key_id": self.key_id,
            "subject_manifest_digest": self.subject_manifest_digest,
            "signature_digest": self.signature_digest,
            "transparency_log_entry_digest": self.transparency_log_entry_digest,
            "verifier_id": self.verifier_id,
            "verified": self.verified,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseVerificationStatus {
    Verified,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseVerificationReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: ReleaseVerificationStatus,
    pub manifest_digest: String,
    pub provenance_digest: String,
    pub signature_attestation_digest: String,
    pub reason: String,
    pub report_digest: String,
}

impl ReleaseVerificationReport {
    pub fn verify(
        manifest: &ReleaseManifest,
        provenance: &ReleaseProvenance,
        signature: &ReleaseSignatureAttestation,
        expected_tag: &str,
        expected_source_revision: &str,
        expected_builder_id: &str,
        expected_toolchain_digest: &str,
    ) -> Result<Self, String> {
        manifest.validate()?;
        provenance.validate()?;
        signature.validate()?;
        let reason = if manifest.tag != expected_tag {
            "release_verification_tag_mismatch"
        } else if manifest.source_revision != expected_source_revision
            || provenance.source_revision != expected_source_revision
        {
            "release_verification_source_revision_mismatch"
        } else if manifest.builder_id != expected_builder_id
            || provenance.builder_id != expected_builder_id
        {
            "release_verification_builder_mismatch"
        } else if manifest.toolchain_digest != expected_toolchain_digest
            || provenance.toolchain_digest != expected_toolchain_digest
        {
            "release_verification_toolchain_mismatch"
        } else if provenance.subject_manifest_digest != manifest.manifest_digest {
            "release_verification_provenance_subject_mismatch"
        } else if signature.subject_manifest_digest != manifest.manifest_digest {
            "release_verification_signature_subject_mismatch"
        } else if !signature.verified {
            "release_verification_signature_unverified"
        } else if signature.transparency_log_entry_digest.is_empty() {
            "release_verification_transparency_missing"
        } else {
            "ok"
        };
        let status = match reason {
            "ok" => ReleaseVerificationStatus::Verified,
            "release_verification_signature_unverified"
            | "release_verification_transparency_missing" => ReleaseVerificationStatus::Unknown,
            _ => ReleaseVerificationStatus::Blocked,
        };
        let mut report = Self {
            schema: RELEASE_VERIFICATION_SCHEMA.to_owned(),
            version: RELEASE_ATTESTATION_VERSION,
            status,
            manifest_digest: manifest.manifest_digest.clone(),
            provenance_digest: provenance.provenance_digest.clone(),
            signature_attestation_digest: signature.attestation_digest.clone(),
            reason: reason.to_owned(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RELEASE_VERIFICATION_SCHEMA
            || self.version != RELEASE_ATTESTATION_VERSION
        {
            return Err("release_verification_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.manifest_digest, "release_verification_manifest_digest"),
            (&self.provenance_digest, "release_verification_provenance_digest"),
            (
                &self.signature_attestation_digest,
                "release_verification_signature_digest",
            ),
            (&self.report_digest, "release_verification_report_digest"),
        ] {
            digest(value, field)?;
        }
        bounded(&self.reason, "release_verification_reason")?;
        if self.report_digest != self.digest() {
            return Err("release_verification_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "manifest_digest": self.manifest_digest,
            "provenance_digest": self.provenance_digest,
            "signature_attestation_digest": self.signature_attestation_digest,
            "reason": self.reason,
        }))
    }
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_RELEASE_TEXT_BYTES
        || value.contains(['\0', '\n', '\r'])
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn safe_name(value: &str, field: &str) -> Result<(), String> {
    bounded(value, field)?;
    if value == "." || value == ".." || value.contains(['/', '\\']) {
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
