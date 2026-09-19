//! One deterministic sparse/dense/RRF/MMR retrieval contract.
//!
//! The caller must supply a server-derived permission scope and assert that ACL filtering already
//! happened. This module never reads a store, resolves permissions, loads a model or performs an
//! effect; it only ranks the already admissible candidate snapshot.

use crate::{json_digest, memory_tokens, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const UNIFIED_RETRIEVAL_PROFILE_SCHEMA: &str = "kiana.unified-retrieval-profile.v1";
pub const RETRIEVAL_REQUEST_SCHEMA: &str = "kiana.retrieval-request.v1";
pub const RETRIEVAL_RESULT_SCHEMA: &str = "kiana.retrieval-result.v1";
pub const UNIFIED_RETRIEVAL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalProfile {
    pub schema: String,
    pub version: SchemaVersion,
    pub algorithm_version: String,
    pub rrf_k: u32,
    pub bm25_k1: f64,
    pub bm25_b: f64,
    pub mmr_lambda: f64,
    pub profile_digest: String,
}

impl RetrievalProfile {
    pub fn unified() -> Self {
        let mut profile = Self {
            schema: UNIFIED_RETRIEVAL_PROFILE_SCHEMA.to_owned(),
            version: UNIFIED_RETRIEVAL_VERSION,
            algorithm_version: "exact+bm25-cjk+dense+rrf60+mmr.v1".to_owned(),
            rrf_k: 60,
            bm25_k1: 1.2,
            bm25_b: 0.75,
            mmr_lambda: 0.7,
            profile_digest: String::new(),
        };
        profile.profile_digest = profile.digest();
        profile
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UNIFIED_RETRIEVAL_PROFILE_SCHEMA
            || self.version != UNIFIED_RETRIEVAL_VERSION
            || self.rrf_k == 0
            || self.rrf_k > 4_096
            || !self.bm25_k1.is_finite()
            || self.bm25_k1 <= 0.0
            || !self.bm25_b.is_finite()
            || !(0.0..=1.0).contains(&self.bm25_b)
            || !self.mmr_lambda.is_finite()
            || !(0.0..=1.0).contains(&self.mmr_lambda)
        {
            return Err("retrieval_profile_invalid".to_owned());
        }
        required(&self.algorithm_version, "retrieval_algorithm_version", 128)?;
        digest(&self.profile_digest, "retrieval_profile_digest")?;
        if self.profile_digest != self.digest() {
            return Err("retrieval_profile_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "algorithm_version": self.algorithm_version,
            "rrf_k": self.rrf_k,
            "bm25_k1": self.bm25_k1,
            "bm25_b": self.bm25_b,
            "mmr_lambda": self.mmr_lambda,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub query: String,
    pub tokens: Vec<String>,
    #[serde(default)]
    pub dense_vector: Option<Vec<f64>>,
    pub permission_scope_digest: String,
    pub acl_filtered: bool,
    pub request_digest: String,
}

impl RetrievalRequest {
    pub fn new(
        query: impl Into<String>,
        permission_scope_digest: impl Into<String>,
        dense_vector: Option<Vec<f64>>,
    ) -> Result<Self, String> {
        let query = query.into();
        let tokens = memory_tokens(&query);
        let mut request = Self {
            schema: RETRIEVAL_REQUEST_SCHEMA.to_owned(),
            version: UNIFIED_RETRIEVAL_VERSION,
            query,
            tokens,
            dense_vector,
            permission_scope_digest: permission_scope_digest.into(),
            acl_filtered: true,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_REQUEST_SCHEMA
            || self.version != UNIFIED_RETRIEVAL_VERSION
            || !self.acl_filtered
            || self.query.len() > 32 * 1024
            || self.tokens.len() > 16_384
        {
            return Err("retrieval_request_invalid".to_owned());
        }
        digest(
            &self.permission_scope_digest,
            "retrieval_permission_scope_digest",
        )?;
        if self.dense_vector.as_ref().is_some_and(|vector| {
            vector.is_empty() || vector.iter().any(|value| !value.is_finite())
        }) {
            return Err("retrieval_dense_vector_invalid".to_owned());
        }
        digest(&self.request_digest, "retrieval_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("retrieval_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "query": self.query,
            "tokens": self.tokens,
            "dense_vector": self.dense_vector,
            "permission_scope_digest": self.permission_scope_digest,
            "acl_filtered": self.acl_filtered,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalItem {
    pub id: String,
    pub text: String,
    pub source_digest: String,
    pub permission_scope_digest: String,
    pub allowed: bool,
    pub created_at_ms: u64,
    #[serde(default)]
    pub dense_vector: Option<Vec<f64>>,
}

impl RetrievalItem {
    fn validate(&self) -> Result<(), String> {
        required(&self.id, "retrieval_item_id", 512)?;
        digest(&self.source_digest, "retrieval_item_source_digest")?;
        digest(
            &self.permission_scope_digest,
            "retrieval_item_permission_scope_digest",
        )?;
        if self.dense_vector.as_ref().is_some_and(|vector| {
            vector.is_empty() || vector.iter().any(|value| !value.is_finite())
        }) {
            return Err("retrieval_item_dense_vector_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalHit {
    pub id: String,
    pub source_digest: String,
    pub permission_scope_digest: String,
    pub rank: u32,
    pub exact_score: u32,
    pub bm25_score: f64,
    pub dense_score: Option<f64>,
    pub sparse_rank: Option<u32>,
    pub dense_rank: Option<u32>,
    pub rrf_score: f64,
    pub mmr_score: f64,
    pub matched_terms: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalResult {
    pub schema: String,
    pub version: SchemaVersion,
    pub algorithm_version: String,
    pub profile_digest: String,
    pub request_digest: String,
    pub permission_scope_digest: String,
    pub acl_filtered_count: u32,
    pub hits: Vec<RetrievalHit>,
    #[serde(default)]
    pub degraded_reasons: Vec<String>,
    pub result_digest: String,
}

impl RetrievalResult {
    fn new(
        profile: &RetrievalProfile,
        request: &RetrievalRequest,
        acl_filtered_count: u32,
        hits: Vec<RetrievalHit>,
        degraded_reasons: Vec<String>,
    ) -> Self {
        let mut result = Self {
            schema: RETRIEVAL_RESULT_SCHEMA.to_owned(),
            version: UNIFIED_RETRIEVAL_VERSION,
            algorithm_version: profile.algorithm_version.clone(),
            profile_digest: profile.profile_digest.clone(),
            request_digest: request.request_digest.clone(),
            permission_scope_digest: request.permission_scope_digest.clone(),
            acl_filtered_count,
            hits,
            degraded_reasons,
            result_digest: String::new(),
        };
        result.result_digest = result.digest();
        result
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_RESULT_SCHEMA || self.version != UNIFIED_RETRIEVAL_VERSION {
            return Err("retrieval_result_header_invalid".to_owned());
        }
        required(
            &self.algorithm_version,
            "retrieval_result_algorithm_version",
            128,
        )?;
        digest(&self.profile_digest, "retrieval_result_profile_digest")?;
        digest(&self.request_digest, "retrieval_result_request_digest")?;
        digest(
            &self.permission_scope_digest,
            "retrieval_result_permission_scope_digest",
        )?;
        for (rank, hit) in self.hits.iter().enumerate() {
            if hit.rank != rank as u32 + 1
                || hit.permission_scope_digest != self.permission_scope_digest
                || !hit.bm25_score.is_finite()
                || hit.dense_score.is_some_and(|value| !value.is_finite())
                || !hit.rrf_score.is_finite()
                || !hit.mmr_score.is_finite()
            {
                return Err("retrieval_result_hit_invalid".to_owned());
            }
        }
        digest(&self.result_digest, "retrieval_result_digest")?;
        if self.result_digest != self.digest() {
            return Err("retrieval_result_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "algorithm_version": self.algorithm_version,
            "profile_digest": self.profile_digest,
            "request_digest": self.request_digest,
            "permission_scope_digest": self.permission_scope_digest,
            "acl_filtered_count": self.acl_filtered_count,
            "hits": self.hits,
            "degraded_reasons": self.degraded_reasons,
        }))
    }
}

pub fn rank_retrieval(
    profile: &RetrievalProfile,
    request: &RetrievalRequest,
    items: &[RetrievalItem],
    limit: usize,
) -> Result<RetrievalResult, String> {
    profile.validate()?;
    request.validate()?;
    if items.len() > 16_384 || limit > 1_024 {
        return Err("retrieval_item_limit".to_owned());
    }
    for item in items {
        item.validate()?;
        if item.permission_scope_digest != request.permission_scope_digest {
            return Err("retrieval_item_scope_mismatch".to_owned());
        }
    }
    let allowed = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.allowed)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let acl_filtered_count = (items.len() - allowed.len()) as u32;
    let mut degraded_reasons = Vec::new();
    let query_terms = request.tokens.iter().cloned().collect::<BTreeSet<_>>();
    let token_sets = items
        .iter()
        .map(|item| memory_tokens(&item.text).into_iter().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let term_sets = token_sets
        .iter()
        .map(|tokens| tokens.iter().cloned().collect::<BTreeSet<_>>())
        .collect::<Vec<_>>();
    let average_length = allowed
        .iter()
        .map(|index| token_sets[*index].len())
        .sum::<usize>() as f64
        / (allowed.len().max(1) as f64);
    let document_count = allowed.len() as f64;
    let document_frequency = query_terms
        .iter()
        .map(|term| {
            (
                term.clone(),
                allowed
                    .iter()
                    .filter(|index| term_sets[**index].contains(term))
                    .count() as f64,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut exact = vec![0_u32; items.len()];
    let mut bm25 = vec![0.0; items.len()];
    let mut dense = vec![None; items.len()];
    for index in &allowed {
        let tokens = &token_sets[*index];
        for term in &query_terms {
            let tf = tokens.iter().filter(|token| *token == term).count() as f64;
            if tf > 0.0 {
                exact[*index] = exact[*index].saturating_add(1);
                let frequency = document_frequency[term];
                let idf = (1.0 + (document_count - frequency + 0.5) / (frequency + 0.5)).ln();
                bm25[*index] += idf * tf * (profile.bm25_k1 + 1.0)
                    / (tf
                        + profile.bm25_k1
                            * (1.0 - profile.bm25_b
                                + profile.bm25_b * tokens.len() as f64 / average_length.max(1.0)));
            }
        }
        dense[*index] = request
            .dense_vector
            .as_ref()
            .zip(items[*index].dense_vector.as_ref())
            .and_then(|(query, item)| {
                if query.len() != item.len() {
                    None
                } else {
                    Some(cosine(query, item))
                }
            })
            .filter(|score| *score > 0.0);
    }
    if request.dense_vector.is_some() && dense.iter().all(Option::is_none) {
        degraded_reasons.push("dense_unavailable_or_dimension_mismatch".to_owned());
    }
    let mut sparse_order = allowed
        .iter()
        .copied()
        .filter(|index| bm25[*index] > 0.0)
        .collect::<Vec<_>>();
    sparse_order.sort_by(|left, right| {
        bm25[*right]
            .total_cmp(&bm25[*left])
            .then(exact[*right].cmp(&exact[*left]))
            .then(items[*left].id.cmp(&items[*right].id))
    });
    let mut dense_order = allowed
        .iter()
        .copied()
        .filter(|index| dense[*index].is_some())
        .collect::<Vec<_>>();
    dense_order.sort_by(|left, right| {
        dense[*right]
            .unwrap_or_default()
            .total_cmp(&dense[*left].unwrap_or_default())
            .then(items[*left].id.cmp(&items[*right].id))
    });
    let mut sparse_rank = vec![None; items.len()];
    let mut dense_rank = vec![None; items.len()];
    let mut rrf = vec![0.0; items.len()];
    for (rank, index) in sparse_order.into_iter().enumerate() {
        sparse_rank[index] = Some(rank as u32 + 1);
        rrf[index] += 1.0 / (profile.rrf_k as f64 + rank as f64 + 1.0);
    }
    for (rank, index) in dense_order.into_iter().enumerate() {
        dense_rank[index] = Some(rank as u32 + 1);
        rrf[index] += 1.0 / (profile.rrf_k as f64 + rank as f64 + 1.0);
    }
    let max_rrf = rrf.iter().copied().fold(0.0, f64::max).max(f64::EPSILON);
    let mut remaining = allowed
        .iter()
        .copied()
        .filter(|index| rrf[*index] > 0.0)
        .collect::<BTreeSet<_>>();
    let mut selected = Vec::new();
    let mut hits = Vec::new();
    while selected.len() < limit && !remaining.is_empty() {
        let mut ranked = remaining
            .iter()
            .map(|index| {
                let redundancy = selected
                    .iter()
                    .map(|other: &usize| {
                        match (&items[*index].dense_vector, &items[*other].dense_vector) {
                            (Some(left), Some(right)) if left.len() == right.len() => {
                                cosine(left, right)
                            }
                            _ => overlap(&term_sets[*index], &term_sets[*other]),
                        }
                    })
                    .fold(0.0, f64::max);
                let base = rrf[*index] / max_rrf;
                let mmr = profile.mmr_lambda * base - (1.0 - profile.mmr_lambda) * redundancy;
                (*index, mmr)
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(left, left_score), (right, right_score)| {
            right_score
                .total_cmp(left_score)
                .then(rrf[*right].total_cmp(&rrf[*left]))
                .then(exact[*right].cmp(&exact[*left]))
                .then(items[*right].created_at_ms.cmp(&items[*left].created_at_ms))
                .then(items[*left].id.cmp(&items[*right].id))
        });
        let (index, mmr_score) = ranked[0];
        remaining.remove(&index);
        selected.push(index);
        let matched_terms = query_terms
            .iter()
            .filter(|term| term_sets[index].contains(*term))
            .cloned()
            .collect::<Vec<_>>();
        hits.push(RetrievalHit {
            id: items[index].id.clone(),
            source_digest: items[index].source_digest.clone(),
            permission_scope_digest: request.permission_scope_digest.clone(),
            rank: hits.len() as u32 + 1,
            exact_score: exact[index],
            bm25_score: bm25[index],
            dense_score: dense[index],
            sparse_rank: sparse_rank[index],
            dense_rank: dense_rank[index],
            rrf_score: rrf[index],
            mmr_score,
            matched_terms,
        });
    }
    let result = RetrievalResult::new(profile, request, acl_filtered_count, hits, degraded_reasons);
    result.validate()?;
    Ok(result)
}

fn cosine(left: &[f64], right: &[f64]) -> f64 {
    let left_norm = left.iter().map(|value| value * value).sum::<f64>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f64>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    (left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum::<f64>()
        / (left_norm * right_norm))
        .clamp(-1.0, 1.0)
}

fn overlap(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f64 {
    let union = left.union(right).count();
    if union == 0 {
        0.0
    } else {
        left.intersection(right).count() as f64 / union as f64
    }
}
