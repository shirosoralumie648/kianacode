//! Core result-ingestion adapter for immutable Company evidence.

use kiana_domain::{CompanyEvidenceBundle, CompanyEvidenceReady};

pub(crate) fn ingest_company_evidence(
    bundle: &CompanyEvidenceBundle,
    source_cursor: u64,
    ready_at: u64,
) -> Result<CompanyEvidenceReady, &'static str> {
    CompanyEvidenceReady::from_bundle(bundle, source_cursor, ready_at)
}
