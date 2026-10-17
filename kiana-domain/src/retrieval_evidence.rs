//! Retrieval result provenance, health and retry boundaries.
//!
//! A result is a derived read projection. Every hit carries source/generation evidence, while an
//! empty or degraded response must explain its status. Retryability is explicit and cannot be
//! inferred from a missing embedding or a transient-looking string.
//!
//! 2026-10-17（CM-14）：本模块的 `RetrievalItemProvenance` / `RetrievalHealth` 被统一检索合同
//! （`unified_retrieval.rs`）直接消费：候选在排名前必须带来源引用/revision/generation/freshness/evidence，
//! 每个 `RetrievalHit` 携带 `RetrievalEvidence`，`RetrievalResult` 带 Ready/Degraded/Denied/Unavailable
//! health 与空结果原因。重试性只能由显式策略声明（`RetrievalRetryPolicy`），确定性降级一律标为不可重试。

use crate::{json_digest, EvidenceStatus, Freshness, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const RETRIEVAL_EVIDENCE_SCHEMA: &str = "kiana.retrieval-evidence.v1";
pub const RETRIEVAL_HEALTH_SCHEMA: &str = "kiana.retrieval-health.v1";
pub const RETRIEVAL_RESPONSE_SCHEMA: &str = "kiana.retrieval-response.v1";
pub const RETRIEVAL_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 没有服务端 generation 的本地来源（例如 CLI 的 schema-v1 JSONL 记录）使用的快照 generation 标签。
///
/// 【作用】让 `RetrievalItemProvenance`/`RetrievalEvidence` 这类要求非零 generation 的合同仍能记录
/// “本次快照来自一个不维护代际计数器的本地存储”。
/// 【边界】它只是标签，不是索引 generation，也不代表任何服务端版本、epoch 或授权状态；具备真实
/// revision/data epoch 的来源必须使用自己的值。
pub const RETRIEVAL_UNVERSIONED_GENERATION: u64 = 1;

/// 降级/不可用通道的可重试策略。
///
/// 【作用】把“可以重试”与“重试是安全的”绑定成同一个显式声明：确定性失败（配置缺失、模型缺失、
/// 维度不匹配、词表未命中）重试同一快照不会得到不同结果，只能标为不可重试；只有调用方明确知道失败
/// 是瞬时的、重试不产生副作用的通道才允许 `Safe`。
/// 【边界】策略由调用方声明，Domain 只把标志写入 health 并拒绝 `retryable && !retry_safe` 的组合；
/// 它不探测真实通道、不重试、也不证明重试一定成功。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalRetryPolicy {
    /// 确定性失败：同一快照/配置下重试不会改变结果。
    Deterministic,
    /// 调用方确认的瞬时失败：重试无副作用且允许重试。
    Safe,
}

impl RetrievalRetryPolicy {
    /// 返回 `(retryable, retry_safe)`；两者只能同时为真。
    pub fn flags(self) -> (bool, bool) {
        match self {
            Self::Deterministic => (false, false),
            Self::Safe => (true, true),
        }
    }
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalHealthStatus {
    Ready,
    Degraded,
    Denied,
    Unavailable,
}

/// 一次检索的健康状态与重试边界。
///
/// 【作用】Ready/Degraded/Denied/Unavailable 只描述本次读取的可信程度，不改变授权结论。
/// 【规则】非 Ready 状态必须带非空 reason；`retryable` 只能与 `retry_safe` 同时为真；Denied 一律不可重试；
/// `source_generation` 为 0 只允许在没有命中的空结果（没有观察到候选代际）时出现。
/// 【边界】本结构是派生读标签：它不代表索引真实健康、不探测模型/网络，也不认证来源。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalHealth {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: RetrievalHealthStatus,
    #[serde(default)]
    pub reason: Option<String>,
    pub retryable: bool,
    pub retry_safe: bool,
    pub source_generation: u64,
    pub result_count: u32,
    pub health_digest: String,
}

impl RetrievalHealth {
    pub fn new(
        status: RetrievalHealthStatus,
        reason: Option<String>,
        retryable: bool,
        retry_safe: bool,
        source_generation: u64,
        result_count: u32,
    ) -> Result<Self, String> {
        let mut health = Self {
            schema: RETRIEVAL_HEALTH_SCHEMA.to_owned(),
            version: RETRIEVAL_EVIDENCE_VERSION,
            status,
            reason,
            retryable,
            retry_safe,
            source_generation,
            result_count,
            health_digest: String::new(),
        };
        health.health_digest = health.digest();
        health.validate()?;
        Ok(health)
    }

    /// Ready：本次检索在声明的通道上完成，没有降级原因，也不可重试。
    pub fn ready(source_generation: u64, result_count: u32) -> Result<Self, String> {
        Self::new(
            RetrievalHealthStatus::Ready,
            None,
            false,
            false,
            source_generation,
            result_count,
        )
    }

    /// Degraded：结果可用但某个通道降级；原因必须可见，重试性由显式策略决定。
    pub fn degraded_channel(
        reason: impl Into<String>,
        retry: RetrievalRetryPolicy,
        source_generation: u64,
        result_count: u32,
    ) -> Result<Self, String> {
        let (retryable, retry_safe) = retry.flags();
        Self::new(
            RetrievalHealthStatus::Degraded,
            Some(reason.into()),
            retryable,
            retry_safe,
            source_generation,
            result_count,
        )
    }

    /// Denied：上游权限/范围拒绝了全部候选；拒绝是确定性结果，不可重试。
    pub fn denied(
        reason: impl Into<String>,
        source_generation: u64,
        result_count: u32,
    ) -> Result<Self, String> {
        Self::new(
            RetrievalHealthStatus::Denied,
            Some(reason.into()),
            false,
            false,
            source_generation,
            result_count,
        )
    }

    /// Unavailable：检索源/通道不可用；原因必须可见，重试性由显式策略决定。
    pub fn unavailable(
        reason: impl Into<String>,
        retry: RetrievalRetryPolicy,
        source_generation: u64,
        result_count: u32,
    ) -> Result<Self, String> {
        let (retryable, retry_safe) = retry.flags();
        Self::new(
            RetrievalHealthStatus::Unavailable,
            Some(reason.into()),
            retryable,
            retry_safe,
            source_generation,
            result_count,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        // generation 0 只允许用于“没有任何候选来源”的空结果；有命中时必须绑定非零 generation，
        // 否则 hit 证据会指向一个不存在的快照。
        if self.schema != RETRIEVAL_HEALTH_SCHEMA
            || self.version != RETRIEVAL_EVIDENCE_VERSION
            || (self.source_generation == 0 && self.result_count != 0)
            || (self.retryable && !self.retry_safe)
        {
            return Err("retrieval_health_header_invalid".to_owned());
        }
        if matches!(self.status, RetrievalHealthStatus::Ready)
            && (self.reason.is_some() || self.retryable)
        {
            return Err("retrieval_health_ready_has_failure".to_owned());
        }
        if matches!(self.status, RetrievalHealthStatus::Denied) && self.retryable {
            return Err("retrieval_health_denied_retryable".to_owned());
        }
        if matches!(
            self.status,
            RetrievalHealthStatus::Degraded
                | RetrievalHealthStatus::Denied
                | RetrievalHealthStatus::Unavailable
        ) && self.reason.as_deref().is_none_or(str::is_empty)
        {
            return Err("retrieval_health_reason_required".to_owned());
        }
        if let Some(reason) = &self.reason {
            required(reason, "retrieval_health_reason", 256)?;
        }
        digest(&self.health_digest, "retrieval_health_digest")?;
        if self.health_digest != self.digest() {
            return Err("retrieval_health_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "reason": self.reason,
            "retryable": self.retryable,
            "retry_safe": self.retry_safe,
            "source_generation": self.source_generation,
            "result_count": self.result_count,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub candidate_id: String,
    pub source_ref: String,
    pub source_revision: String,
    pub source_snapshot_digest: String,
    pub generation: u64,
    pub freshness: Freshness,
    pub evidence: EvidenceStatus,
    pub rank: u32,
    pub rank_components: BTreeMap<String, f64>,
    pub evidence_digest: String,
}

impl RetrievalEvidence {
    pub fn new(
        candidate_id: impl Into<String>,
        source_ref: impl Into<String>,
        source_revision: impl Into<String>,
        source_snapshot_digest: impl Into<String>,
        generation: u64,
        freshness: Freshness,
        evidence: EvidenceStatus,
        rank: u32,
        rank_components: BTreeMap<String, f64>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: RETRIEVAL_EVIDENCE_SCHEMA.to_owned(),
            version: RETRIEVAL_EVIDENCE_VERSION,
            candidate_id: candidate_id.into(),
            source_ref: source_ref.into(),
            source_revision: source_revision.into(),
            source_snapshot_digest: source_snapshot_digest.into(),
            generation,
            freshness,
            evidence,
            rank,
            rank_components,
            evidence_digest: String::new(),
        };
        value.evidence_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_EVIDENCE_SCHEMA
            || self.version != RETRIEVAL_EVIDENCE_VERSION
            || self.generation == 0
            || self.rank == 0
        {
            return Err("retrieval_evidence_header_invalid".to_owned());
        }
        required(&self.candidate_id, "retrieval_evidence_candidate_id", 256)?;
        required(&self.source_ref, "retrieval_evidence_source_ref", 4_096)?;
        required(
            &self.source_revision,
            "retrieval_evidence_source_revision",
            256,
        )?;
        digest(
            &self.source_snapshot_digest,
            "retrieval_evidence_source_snapshot_digest",
        )?;
        if self.rank_components.len() > 32
            || self
                .rank_components
                .values()
                .any(|value| !value.is_finite())
        {
            return Err("retrieval_evidence_rank_components_invalid".to_owned());
        }
        digest(&self.evidence_digest, "retrieval_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("retrieval_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "candidate_id": self.candidate_id,
            "source_ref": self.source_ref,
            "source_revision": self.source_revision,
            "source_snapshot_digest": self.source_snapshot_digest,
            "generation": self.generation,
            "freshness": self.freshness,
            "evidence": self.evidence,
            "rank": self.rank,
            "rank_components": self.rank_components,
        }))
    }
}

/// 统一检索候选（item 侧）的来源证据。
///
/// 【作用】把“这条候选来自哪里、哪一代”固定在进入排名的输入上，让每个命中的
/// `RetrievalEvidence` 能逐字段回溯，而不是只在结果里留下一个内容摘要。
/// 【调用者】构造 `RetrievalItem` 的 adapter（daemon memory tool、CLI memory search、fixtures）。
/// 【输入/输出】来源引用、revision 标签、该候选所属快照的非零 generation、freshness 与 evidence。
/// 【失败】空/NUL/超长的来源引用或 revision、generation 为 0 时返回稳定错误，候选在打分前 fail-closed。
/// 【边界】这些字段由调用方声明：Domain 不读取来源、不重新授权，也不把 `Attributed` 提升为 `Verified`；
/// freshness 是调用方派生的只读标签，不是新鲜度探测。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalItemProvenance {
    pub source_ref: String,
    pub source_revision: String,
    pub generation: u64,
    pub freshness: Freshness,
    pub evidence: EvidenceStatus,
}

impl RetrievalItemProvenance {
    pub fn new(
        source_ref: impl Into<String>,
        source_revision: impl Into<String>,
        generation: u64,
        freshness: Freshness,
        evidence: EvidenceStatus,
    ) -> Result<Self, String> {
        let value = Self {
            source_ref: source_ref.into(),
            source_revision: source_revision.into(),
            generation,
            freshness,
            evidence,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.generation == 0 {
            return Err("retrieval_item_provenance_generation_invalid".to_owned());
        }
        required(&self.source_ref, "retrieval_evidence_source_ref", 4_096)?;
        required(
            &self.source_revision,
            "retrieval_evidence_source_revision",
            256,
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalResponse {
    pub schema: String,
    pub version: SchemaVersion,
    pub query_digest: String,
    pub generation: u64,
    pub hits: Vec<RetrievalEvidence>,
    pub health: RetrievalHealth,
    #[serde(default)]
    pub empty_reason: Option<String>,
    pub response_digest: String,
}

impl RetrievalResponse {
    pub fn new(
        query_digest: impl Into<String>,
        generation: u64,
        hits: Vec<RetrievalEvidence>,
        health: RetrievalHealth,
        empty_reason: Option<String>,
    ) -> Result<Self, String> {
        let mut response = Self {
            schema: RETRIEVAL_RESPONSE_SCHEMA.to_owned(),
            version: RETRIEVAL_EVIDENCE_VERSION,
            query_digest: query_digest.into(),
            generation,
            hits,
            health,
            empty_reason,
            response_digest: String::new(),
        };
        response.response_digest = response.digest();
        response.validate()?;
        Ok(response)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_RESPONSE_SCHEMA
            || self.version != RETRIEVAL_EVIDENCE_VERSION
            || self.generation == 0
            || self.hits.len() > 256
            || (self.hits.is_empty() && self.empty_reason.as_deref().is_none_or(str::is_empty))
            || (!self.hits.is_empty() && self.empty_reason.is_some())
        {
            return Err("retrieval_response_header_invalid".to_owned());
        }
        digest(&self.query_digest, "retrieval_response_query_digest")?;
        self.health.validate()?;
        if self.health.source_generation != self.generation
            || self.health.result_count != self.hits.len() as u32
        {
            return Err("retrieval_response_health_mismatch".to_owned());
        }
        if let Some(reason) = &self.empty_reason {
            required(reason, "retrieval_response_empty_reason", 256)?;
        }
        for (index, hit) in self.hits.iter().enumerate() {
            hit.validate()?;
            if hit.generation != self.generation || hit.rank != index as u32 + 1 {
                return Err("retrieval_response_hit_generation_or_rank_invalid".to_owned());
            }
        }
        digest(&self.response_digest, "retrieval_response_digest")?;
        if self.response_digest != self.digest() {
            return Err("retrieval_response_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "query_digest": self.query_digest,
            "generation": self.generation,
            "hits": self.hits,
            "health": self.health,
            "empty_reason": self.empty_reason,
        }))
    }
}
