//! Stage-aware retrieval receipts and provenance-bound reviewer citations.
//!
//! Retrieval is not delivery: a hit may be retrieved and still be omitted from the context plan,
//! never sent to a provider, or never cited by a reviewer.  This contract keeps those stages
//! explicit and requires a citable source snapshot before a citation can be issued.

use crate::{
    json_digest, EvidenceStatus, Freshness, SchemaVersion, SourceKind, SourceRef, SourceSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const RETRIEVAL_RECEIPT_SCHEMA: &str = "kiana.retrieval-receipt.v1";
pub const REVIEWER_CITATION_SCHEMA: &str = "kiana.reviewer-citation.v1";
pub const RETRIEVAL_RECEIPT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_RECEIPT_ENTRIES: usize = 512;
const MAX_RECEIPT_OMISSIONS: usize = 512;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalReceiptStage {
    Retrieved,
    Selected,
    Sent,
    Cited,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalReceiptEntry {
    pub candidate_id: String,
    pub stage: RetrievalReceiptStage,
    pub source_snapshot: SourceSnapshot,
    pub source_revision: String,
    pub retrieval_evidence_digest: String,
    pub degraded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omission: Option<String>,
}

impl RetrievalReceiptEntry {
    pub fn new(
        candidate_id: impl Into<String>,
        stage: RetrievalReceiptStage,
        source_snapshot: SourceSnapshot,
        retrieval_evidence_digest: impl Into<String>,
        degraded: bool,
        omission: Option<String>,
    ) -> Result<Self, String> {
        let source_revision = source_snapshot.source.revision.clone();
        let entry = Self {
            candidate_id: candidate_id.into(),
            stage,
            source_snapshot,
            source_revision,
            retrieval_evidence_digest: retrieval_evidence_digest.into(),
            degraded,
            omission,
        };
        entry.validate()?;
        Ok(entry)
    }

    pub fn validate(&self) -> Result<(), String> {
        required(&self.candidate_id, "retrieval_receipt_candidate_id", 512)?;
        self.source_snapshot.validate()?;
        required(
            &self.source_revision,
            "retrieval_receipt_source_revision",
            256,
        )?;
        if self.source_revision != self.source_snapshot.source.revision {
            return Err("retrieval_receipt_source_revision_mismatch".to_owned());
        }
        digest(
            &self.retrieval_evidence_digest,
            "retrieval_receipt_evidence_digest",
        )?;
        if let Some(omission) = &self.omission {
            if !matches!(
                omission.as_str(),
                "token_budget" | "task_lens" | "unavailable" | "denied" | "stale"
            ) {
                return Err("retrieval_receipt_omission_invalid".to_owned());
            }
            if self.stage != RetrievalReceiptStage::Retrieved {
                return Err("retrieval_receipt_omission_stage_invalid".to_owned());
            }
        }
        if self.stage == RetrievalReceiptStage::Cited
            && !matches!(
                self.source_snapshot.evidence,
                EvidenceStatus::Attributed | EvidenceStatus::Verified
            )
        {
            return Err("retrieval_citation_provenance_unverifiable".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalReceiptOmission {
    pub candidate_id: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
}

impl RetrievalReceiptOmission {
    pub fn validate(&self) -> Result<(), String> {
        required(
            &self.candidate_id,
            "retrieval_receipt_omission_candidate",
            512,
        )?;
        if !matches!(
            self.reason.as_str(),
            "token_budget" | "task_lens" | "unavailable" | "denied" | "stale"
        ) {
            return Err("retrieval_receipt_omission_invalid".to_owned());
        }
        if self
            .source_revision
            .as_deref()
            .is_some_and(|revision| revision.trim().is_empty() || revision.len() > 256)
        {
            return Err("retrieval_receipt_omission_revision_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub receipt_id: String,
    pub query: String,
    pub query_digest: String,
    pub permission_scope_digest: String,
    pub algorithm_version: String,
    pub source_generation: u64,
    pub degraded: bool,
    #[serde(default)]
    pub degraded_reasons: Vec<String>,
    pub entries: Vec<RetrievalReceiptEntry>,
    #[serde(default)]
    pub omissions: Vec<RetrievalReceiptOmission>,
    pub receipt_digest: String,
}

impl RetrievalReceipt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        receipt_id: impl Into<String>,
        query: impl Into<String>,
        query_digest: impl Into<String>,
        permission_scope_digest: impl Into<String>,
        algorithm_version: impl Into<String>,
        source_generation: u64,
        degraded: bool,
        degraded_reasons: Vec<String>,
        entries: Vec<RetrievalReceiptEntry>,
        omissions: Vec<RetrievalReceiptOmission>,
    ) -> Result<Self, String> {
        let mut receipt = Self {
            schema: RETRIEVAL_RECEIPT_SCHEMA.to_owned(),
            version: RETRIEVAL_RECEIPT_VERSION,
            receipt_id: receipt_id.into(),
            query: query.into(),
            query_digest: query_digest.into(),
            permission_scope_digest: permission_scope_digest.into(),
            algorithm_version: algorithm_version.into(),
            source_generation,
            degraded,
            degraded_reasons,
            entries,
            omissions,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_RECEIPT_SCHEMA
            || self.version != RETRIEVAL_RECEIPT_VERSION
            || self.source_generation == 0
            || self.entries.len() > MAX_RECEIPT_ENTRIES
            || self.omissions.len() > MAX_RECEIPT_OMISSIONS
            || (!self.degraded && !self.degraded_reasons.is_empty())
            || (self.degraded && self.degraded_reasons.is_empty())
        {
            return Err("retrieval_receipt_header_invalid".to_owned());
        }
        required(&self.receipt_id, "retrieval_receipt_id", 256)?;
        required(&self.query, "retrieval_receipt_query", 32 * 1024)?;
        digest(&self.query_digest, "retrieval_receipt_query_digest")?;
        if self.query_digest != json_digest(&json!({"query": self.query})) {
            return Err("retrieval_receipt_query_digest_mismatch".to_owned());
        }
        digest(
            &self.permission_scope_digest,
            "retrieval_receipt_scope_digest",
        )?;
        required(&self.algorithm_version, "retrieval_receipt_algorithm", 256)?;
        if self.degraded_reasons.len() > 32
            || self
                .degraded_reasons
                .iter()
                .any(|reason| reason.trim().is_empty() || reason.len() > 256)
        {
            return Err("retrieval_receipt_degraded_reasons_invalid".to_owned());
        }
        let mut keys = BTreeSet::new();
        let mut stages: BTreeMap<String, BTreeSet<RetrievalReceiptStage>> = BTreeMap::new();
        for entry in &self.entries {
            entry.validate()?;
            if !keys.insert((entry.candidate_id.clone(), entry.stage)) {
                return Err("retrieval_receipt_duplicate_stage".to_owned());
            }
            stages
                .entry(entry.candidate_id.clone())
                .or_default()
                .insert(entry.stage);
        }
        for (candidate_id, candidate_stages) in stages {
            if candidate_stages.contains(&RetrievalReceiptStage::Selected)
                && !candidate_stages.contains(&RetrievalReceiptStage::Retrieved)
            {
                return Err(format!(
                    "retrieval_receipt_selected_without_retrieved:{candidate_id}"
                ));
            }
            if candidate_stages.contains(&RetrievalReceiptStage::Sent)
                && !candidate_stages.contains(&RetrievalReceiptStage::Selected)
            {
                return Err(format!(
                    "retrieval_receipt_sent_without_selected:{candidate_id}"
                ));
            }
            if candidate_stages.contains(&RetrievalReceiptStage::Cited)
                && !candidate_stages.contains(&RetrievalReceiptStage::Sent)
            {
                return Err(format!(
                    "retrieval_receipt_cited_without_sent:{candidate_id}"
                ));
            }
        }
        for omission in &self.omissions {
            omission.validate()?;
        }
        digest(&self.receipt_digest, "retrieval_receipt_digest")?;
        if self.receipt_digest != self.digest() {
            return Err("retrieval_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn cite(
        &self,
        candidate_id: &str,
        quote_digest: impl Into<String>,
    ) -> Result<ReviewerCitation, String> {
        self.validate()?;
        let entry = self
            .entries
            .iter()
            .find(|entry| {
                entry.candidate_id == candidate_id && entry.stage == RetrievalReceiptStage::Cited
            })
            .ok_or_else(|| "retrieval_citation_stage_missing".to_owned())?;
        if !matches!(
            entry.source_snapshot.evidence,
            EvidenceStatus::Attributed | EvidenceStatus::Verified
        ) {
            return Err("retrieval_citation_provenance_unverifiable".to_owned());
        }
        let citation = ReviewerCitation {
            schema: REVIEWER_CITATION_SCHEMA.to_owned(),
            candidate_id: candidate_id.to_owned(),
            receipt_digest: self.receipt_digest.clone(),
            source_snapshot_digest: entry.source_snapshot.snapshot_digest.clone(),
            source_revision: entry.source_revision.clone(),
            retrieval_evidence_digest: entry.retrieval_evidence_digest.clone(),
            quote_digest: quote_digest.into(),
            citation_digest: String::new(),
        };
        let mut citation = citation;
        citation.citation_digest = citation.digest();
        citation.validate_against(self)?;
        Ok(citation)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "receipt_id": self.receipt_id,
            "query": self.query,
            "query_digest": self.query_digest,
            "permission_scope_digest": self.permission_scope_digest,
            "algorithm_version": self.algorithm_version,
            "source_generation": self.source_generation,
            "degraded": self.degraded,
            "degraded_reasons": self.degraded_reasons,
            "entries": self.entries,
            "omissions": self.omissions,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerCitation {
    pub schema: String,
    pub candidate_id: String,
    pub receipt_digest: String,
    pub source_snapshot_digest: String,
    pub source_revision: String,
    pub retrieval_evidence_digest: String,
    pub quote_digest: String,
    pub citation_digest: String,
}

impl ReviewerCitation {
    pub fn validate_against(&self, receipt: &RetrievalReceipt) -> Result<(), String> {
        receipt.validate()?;
        if self.schema != REVIEWER_CITATION_SCHEMA
            || self.receipt_digest != receipt.receipt_digest
            || self.candidate_id.trim().is_empty()
        {
            return Err("reviewer_citation_receipt_binding_invalid".to_owned());
        }
        digest(
            &self.source_snapshot_digest,
            "reviewer_citation_snapshot_digest",
        )?;
        digest(
            &self.retrieval_evidence_digest,
            "reviewer_citation_evidence_digest",
        )?;
        digest(&self.quote_digest, "reviewer_citation_quote_digest")?;
        required(
            &self.source_revision,
            "reviewer_citation_source_revision",
            256,
        )?;
        digest(&self.citation_digest, "reviewer_citation_digest")?;
        if self.citation_digest != self.digest() {
            return Err("reviewer_citation_digest_mismatch".to_owned());
        }
        let entry = receipt
            .entries
            .iter()
            .find(|entry| {
                entry.candidate_id == self.candidate_id
                    && entry.stage == RetrievalReceiptStage::Cited
            })
            .ok_or_else(|| "retrieval_citation_stage_missing".to_owned())?;
        if entry.source_snapshot.snapshot_digest != self.source_snapshot_digest
            || entry.source_revision != self.source_revision
            || entry.retrieval_evidence_digest != self.retrieval_evidence_digest
            || !matches!(
                entry.source_snapshot.evidence,
                EvidenceStatus::Attributed | EvidenceStatus::Verified
            )
        {
            return Err("reviewer_citation_provenance_unverifiable".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "candidate_id": self.candidate_id,
            "receipt_digest": self.receipt_digest,
            "source_snapshot_digest": self.source_snapshot_digest,
            "source_revision": self.source_revision,
            "retrieval_evidence_digest": self.retrieval_evidence_digest,
            "quote_digest": self.quote_digest,
        }))
    }
}

/// Build a typed memory source snapshot for a receipt adapter without exposing record text.
pub fn memory_source_snapshot(
    record_id: &str,
    collection: &str,
    revision: &str,
    content_digest: &str,
    evidence: EvidenceStatus,
) -> Result<SourceSnapshot, String> {
    let source = SourceRef::new(
        format!("memory:{record_id}"),
        SourceKind::Memory,
        format!("{collection}:{record_id}"),
        revision,
        content_digest,
        None,
        evidence,
    )?;
    SourceSnapshot::new(source, Freshness::Current, evidence, None)
}
