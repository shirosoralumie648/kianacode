//! Release evidence bound to a deployment profile.
//!
//! `release_attestation` validates individual manifest, provenance and signature objects. This
//! module binds those existing facts to the deployment path and keeps the proof ceiling explicit:
//! it never signs, publishes, installs, starts a process or contacts a transparency service.

use crate::{
    json_digest, DeploymentProfile, ReleaseManifest, ReleaseProvenance,
    ReleaseSignatureAttestation, ReleaseVerificationReport, ReleaseVerificationStatus,
    SchemaVersion,
};
use serde::{Deserialize, Serialize};

pub const DEPLOYMENT_RELEASE_SCHEMA: &str = "kiana.deployment-release.v1";
pub const DEPLOYMENT_RELEASE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentReleaseBundle {
    pub schema: String,
    pub version: SchemaVersion,
    pub profile: DeploymentProfile,
    pub manifest: ReleaseManifest,
    pub provenance: ReleaseProvenance,
    pub signature: ReleaseSignatureAttestation,
    pub verification: ReleaseVerificationReport,
    pub secret_free: bool,
    pub bundle_digest: String,
}

impl DeploymentReleaseBundle {
    pub fn new(
        profile: DeploymentProfile,
        manifest: ReleaseManifest,
        provenance: ReleaseProvenance,
        signature: ReleaseSignatureAttestation,
        verification: ReleaseVerificationReport,
    ) -> Result<Self, String> {
        let mut bundle = Self {
            schema: DEPLOYMENT_RELEASE_SCHEMA.to_owned(),
            version: DEPLOYMENT_RELEASE_VERSION,
            profile,
            manifest,
            provenance,
            signature,
            verification,
            secret_free: true,
            bundle_digest: String::new(),
        };
        bundle.bundle_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_RELEASE_SCHEMA
            || self.version != DEPLOYMENT_RELEASE_VERSION
            || !self.secret_free
        {
            return Err("deployment_release_header_invalid".to_owned());
        }
        self.manifest.validate()?;
        self.provenance.validate()?;
        self.signature.validate()?;
        self.verification.validate()?;
        if self.verification.status != ReleaseVerificationStatus::Verified
            || self.verification.reason != "ok"
            || !self.signature.verified
        {
            return Err("deployment_release_not_verified".to_owned());
        }
        if self.verification.manifest_digest != self.manifest.manifest_digest
            || self.verification.provenance_digest != self.provenance.provenance_digest
            || self.verification.signature_attestation_digest != self.signature.attestation_digest
            || self.provenance.subject_manifest_digest != self.manifest.manifest_digest
            || self.signature.subject_manifest_digest != self.manifest.manifest_digest
        {
            return Err("deployment_release_subject_mismatch".to_owned());
        }
        if contains_secret_marker(self) {
            return Err("deployment_release_secret_marker".to_owned());
        }
        valid_digest(&self.bundle_digest, "deployment_release_digest")?;
        if self.bundle_digest != self.digest() {
            return Err("deployment_release_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "profile": self.profile,
            "manifest": self.manifest,
            "provenance": self.provenance,
            "signature": self.signature,
            "verification": self.verification,
            "secret_free": self.secret_free,
        }))
    }
}

fn contains_secret_marker(bundle: &DeploymentReleaseBundle) -> bool {
    let encoded = serde_json::to_string(&(
        &bundle.manifest,
        &bundle.provenance,
        &bundle.signature,
        &bundle.verification,
    ))
    .unwrap_or_default()
    .to_ascii_lowercase();
    [
        "secret=",
        "secret:",
        "api_key",
        "access_token",
        "refresh_token",
        "private_key",
        "password=",
        "authorization: bearer",
    ]
    .iter()
    .any(|marker| encoded.contains(marker))
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
