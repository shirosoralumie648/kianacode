//! Separate retrieval quality, determinism and safety evaluation metrics.
//!
//! Fixture rank reproducibility is not semantic quality, and neither is allowed to mask an ACL or
//! revocation failure.  This module keeps those dimensions in one report with independent gates.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const RETRIEVAL_EVALUATION_SCHEMA: &str = "kiana.retrieval-evaluation.v1";
pub const RETRIEVAL_EVALUATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn score(value: f64, field: &str) -> Result<(), String> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

pub fn recall_at_k(expected: &[String], observed: &[String], k: usize) -> f64 {
    if expected.is_empty() || k == 0 {
        return 0.0;
    }
    let expected = expected.iter().cloned().collect::<BTreeSet<_>>();
    let hits = observed
        .iter()
        .take(k)
        .filter(|id| expected.contains(*id))
        .count();
    hits as f64 / expected.len() as f64
}

pub fn reciprocal_rank(expected: &[String], observed: &[String]) -> f64 {
    let expected = expected.iter().cloned().collect::<BTreeSet<_>>();
    observed
        .iter()
        .position(|id| expected.contains(id))
        .map(|index| 1.0 / (index as f64 + 1.0))
        .unwrap_or(0.0)
}

pub fn ndcg_at_k(expected: &[String], observed: &[String], k: usize) -> f64 {
    if expected.is_empty() || k == 0 {
        return 0.0;
    }
    let expected = expected.iter().cloned().collect::<BTreeSet<_>>();
    let dcg = observed
        .iter()
        .take(k)
        .enumerate()
        .filter(|(_, id)| expected.contains(*id))
        .map(|(index, _)| 1.0 / ((index as f64 + 2.0).log2()))
        .sum::<f64>();
    let ideal = (0..expected.len().min(k))
        .map(|index| 1.0 / ((index as f64 + 2.0).log2()))
        .sum::<f64>();
    if ideal == 0.0 {
        0.0
    } else {
        dcg / ideal
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalQualityMetrics {
    pub recall_at_k: f64,
    pub mrr: f64,
    pub ndcg: f64,
    pub citation_precision: f64,
    pub freshness_rate: f64,
    pub duplicate_rate: f64,
    pub p95_latency_ms: u64,
    pub budget_overflow_rate: f64,
}

impl RetrievalQualityMetrics {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        recall_at_k: f64,
        mrr: f64,
        ndcg: f64,
        citation_precision: f64,
        freshness_rate: f64,
        duplicate_rate: f64,
        p95_latency_ms: u64,
        budget_overflow_rate: f64,
    ) -> Result<Self, String> {
        let metrics = Self {
            recall_at_k,
            mrr,
            ndcg,
            citation_precision,
            freshness_rate,
            duplicate_rate,
            p95_latency_ms,
            budget_overflow_rate,
        };
        metrics.validate()?;
        Ok(metrics)
    }

    pub fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (self.recall_at_k, "retrieval_eval_recall"),
            (self.mrr, "retrieval_eval_mrr"),
            (self.ndcg, "retrieval_eval_ndcg"),
            (self.citation_precision, "retrieval_eval_citation_precision"),
            (self.freshness_rate, "retrieval_eval_freshness"),
            (self.duplicate_rate, "retrieval_eval_duplicate_rate"),
            (self.budget_overflow_rate, "retrieval_eval_budget_overflow"),
        ] {
            score(value, field)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalSafetyMetrics {
    pub unauthorized_hits: u64,
    pub revoked_hits: u64,
    pub stale_reinjected: u64,
    pub unverifiable_citations: u64,
    pub passed: bool,
}

impl RetrievalSafetyMetrics {
    pub fn new(
        unauthorized_hits: u64,
        revoked_hits: u64,
        stale_reinjected: u64,
        unverifiable_citations: u64,
    ) -> Self {
        Self {
            unauthorized_hits,
            revoked_hits,
            stale_reinjected,
            unverifiable_citations,
            passed: unauthorized_hits == 0
                && revoked_hits == 0
                && stale_reinjected == 0
                && unverifiable_citations == 0,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let expected = self.unauthorized_hits == 0
            && self.revoked_hits == 0
            && self.stale_reinjected == 0
            && self.unverifiable_citations == 0;
        if self.passed != expected {
            return Err("retrieval_eval_safety_status_mismatch".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalQualityEvidence {
    FixtureDeterminism,
    SemanticModel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalEvaluationReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub fixture_digest: String,
    pub algorithm_digest: String,
    pub embedding_digest: String,
    pub evidence: RetrievalQualityEvidence,
    pub determinism_pass: bool,
    pub semantic_quality_measured: bool,
    pub quality: RetrievalQualityMetrics,
    pub safety: RetrievalSafetyMetrics,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub report_digest: String,
}

impl RetrievalEvaluationReport {
    pub fn new(
        fixture_digest: impl Into<String>,
        algorithm_digest: impl Into<String>,
        embedding_digest: impl Into<String>,
        evidence: RetrievalQualityEvidence,
        determinism_pass: bool,
        semantic_quality_measured: bool,
        quality: RetrievalQualityMetrics,
        safety: RetrievalSafetyMetrics,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut report = Self {
            schema: RETRIEVAL_EVALUATION_SCHEMA.to_owned(),
            version: RETRIEVAL_EVALUATION_VERSION,
            fixture_digest: fixture_digest.into(),
            algorithm_digest: algorithm_digest.into(),
            embedding_digest: embedding_digest.into(),
            evidence,
            determinism_pass,
            semantic_quality_measured,
            quality,
            safety,
            limitations,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_EVALUATION_SCHEMA
            || self.version != RETRIEVAL_EVALUATION_VERSION
            || self.limitations.len() > 16
            || self
                .limitations
                .iter()
                .any(|limitation| limitation.trim().is_empty() || limitation.len() > 256)
            || (self.evidence == RetrievalQualityEvidence::FixtureDeterminism
                && self.semantic_quality_measured)
        {
            return Err("retrieval_eval_report_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.fixture_digest, "retrieval_eval_fixture_digest"),
            (&self.algorithm_digest, "retrieval_eval_algorithm_digest"),
            (&self.embedding_digest, "retrieval_eval_embedding_digest"),
            (&self.report_digest, "retrieval_eval_report_digest"),
        ] {
            digest(value, field)?;
        }
        self.quality.validate()?;
        self.safety.validate()?;
        if self.report_digest != self.digest() {
            return Err("retrieval_eval_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn safety_passed(&self) -> bool {
        self.safety.passed
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "fixture_digest": self.fixture_digest,
            "algorithm_digest": self.algorithm_digest,
            "embedding_digest": self.embedding_digest,
            "evidence": self.evidence,
            "determinism_pass": self.determinism_pass,
            "semantic_quality_measured": self.semantic_quality_measured,
            "quality": self.quality,
            "safety": self.safety,
            "limitations": self.limitations,
        }))
    }
}
