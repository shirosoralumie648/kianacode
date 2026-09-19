//! Cross-platform execution backend disposition contract.
//!
//! This contract separates an implemented backend from a target-only or unavailable platform.
//! It is metadata/evidence, not an OS sandbox implementation and never falls back to the host.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const PLATFORM_BACKEND_SCHEMA: &str = "kiana.platform-backend-disposition.v1";
pub const PLATFORM_BACKEND_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformTarget {
    Linux,
    Macos,
    Windows,
    Container,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformBackendDisposition {
    Implemented,
    TargetOnly,
    NotSupported,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformBackendReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub target: PlatformTarget,
    pub backend: String,
    pub disposition: PlatformBackendDisposition,
    pub behavior_verified: bool,
    pub no_host_fallback: bool,
    pub limitations: Vec<String>,
    pub report_digest: String,
}

impl PlatformBackendReport {
    pub fn new(
        target: PlatformTarget,
        backend: impl Into<String>,
        disposition: PlatformBackendDisposition,
        behavior_verified: bool,
        no_host_fallback: bool,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut report = Self {
            schema: PLATFORM_BACKEND_SCHEMA.to_owned(),
            version: PLATFORM_BACKEND_VERSION,
            target,
            backend: backend.into(),
            disposition,
            behavior_verified,
            no_host_fallback,
            limitations,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PLATFORM_BACKEND_SCHEMA
            || self.version != PLATFORM_BACKEND_VERSION
            || self.backend.trim().is_empty()
            || self.backend.len() > 128
            || self.limitations.len() > 32
        {
            return Err("platform_backend_report_invalid".to_owned());
        }
        if self.disposition == PlatformBackendDisposition::Implemented && !self.behavior_verified {
            return Err("platform_backend_implemented_requires_behavior".to_owned());
        }
        if self.disposition != PlatformBackendDisposition::Implemented && self.behavior_verified {
            return Err("platform_backend_unverified_disposition_mismatch".to_owned());
        }
        if self.disposition != PlatformBackendDisposition::Implemented
            && self.limitations.is_empty()
        {
            return Err("platform_backend_limitation_required".to_owned());
        }
        if !self.no_host_fallback {
            return Err("platform_backend_host_fallback_forbidden".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("platform_backend_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "target": self.target,
            "backend": self.backend,
            "disposition": self.disposition,
            "behavior_verified": self.behavior_verified,
            "no_host_fallback": self.no_host_fallback,
            "limitations": self.limitations,
        }))
    }
}
