//! EQ-51 read-only ControlPlane facade for the evidence archive contract.

use kiana_domain::QualityEvidenceArchive;

pub fn validate_quality_evidence_archive(archive: &QualityEvidenceArchive) -> Result<(), String> {
    archive.validate()
}
