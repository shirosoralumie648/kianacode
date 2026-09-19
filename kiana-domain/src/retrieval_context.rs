//! Shared retrieval candidates for Memory, repo map, code search and artifacts.
//!
//! Adapters may discover candidates, but this contract is the only shape admitted to ContextPlan.
//! Every candidate remains lower-trust Context material with source snapshot, ACL scope, freshness
//! and evidence; no candidate can promote itself to Product instructions or Company fact.

use crate::{
    json_digest, ContextCandidate, ContextMaterialType, ContextPlan, EvidenceStatus, Freshness,
    PromptAuthority, PromptBundle, SourceKind, SourceSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const RETRIEVAL_CANDIDATE_SCHEMA: &str = "kiana.retrieval-candidate.v1";
pub const RETRIEVAL_PACK_SCHEMA: &str = "kiana.retrieval-pack.v1";
pub const RETRIEVAL_VERSION: u32 = 1;
pub const MAX_RETRIEVAL_CANDIDATES: usize = 256;
pub const MAX_RETRIEVAL_TEXT_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalSourceKind {
    Memory,
    RepoMap,
    CodeSearch,
    Artifact,
}

impl RetrievalSourceKind {
    fn source_kind(self) -> SourceKind {
        match self {
            Self::Memory => SourceKind::Memory,
            Self::RepoMap | Self::CodeSearch => SourceKind::WorkspaceFile,
            Self::Artifact => SourceKind::Artifact,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMaterialClass {
    StableRole,
    ProjectKnowledge,
    Scratch,
    UserPrivate,
    LessonCandidate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalCandidate {
    pub schema: String,
    pub version: u32,
    pub candidate_id: String,
    pub source_kind: RetrievalSourceKind,
    pub material_class: ContextMaterialClass,
    pub text: String,
    pub source_snapshot: SourceSnapshot,
    pub permission_scope: String,
    pub acl_digest: String,
    pub relevance_milli: u16,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub candidate_digest: String,
}

impl RetrievalCandidate {
    pub fn new(
        candidate_id: impl Into<String>,
        source_kind: RetrievalSourceKind,
        material_class: ContextMaterialClass,
        text: impl Into<String>,
        source_snapshot: SourceSnapshot,
        permission_scope: impl Into<String>,
        acl_digest: impl Into<String>,
        relevance_milli: u16,
        evidence_refs: Vec<String>,
    ) -> Result<Self, String> {
        let mut candidate = Self {
            schema: RETRIEVAL_CANDIDATE_SCHEMA.to_owned(),
            version: RETRIEVAL_VERSION,
            candidate_id: candidate_id.into(),
            source_kind,
            material_class,
            text: text.into(),
            source_snapshot,
            permission_scope: permission_scope.into(),
            acl_digest: acl_digest.into(),
            relevance_milli,
            evidence_refs,
            candidate_digest: String::new(),
        };
        candidate.candidate_digest = candidate.digest();
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_CANDIDATE_SCHEMA
            || self.version != RETRIEVAL_VERSION
            || !bounded(&self.candidate_id, 256)
            || self.text.len() > MAX_RETRIEVAL_TEXT_BYTES
            || !bounded(&self.permission_scope, 256)
            || self.relevance_milli > 1_000
            || !digest(&self.acl_digest)
            || !digest(&self.candidate_digest)
            || self.candidate_digest != self.digest()
            || self.source_snapshot.source.kind != self.source_kind.source_kind()
            || self.evidence_refs.len() > 128
            || self
                .evidence_refs
                .iter()
                .any(|reference| !bounded(reference, 512))
        {
            return Err("retrieval_candidate_invalid".to_owned());
        }
        self.source_snapshot.validate()?;
        if self.material_class == ContextMaterialClass::LessonCandidate
            && self.evidence_refs.is_empty()
        {
            return Err("retrieval_lesson_evidence_required".to_owned());
        }
        Ok(())
    }

    /// Convert to the only plan input shape. Retrieval can never create Product authority.
    pub fn as_context_candidate(&self) -> Result<ContextCandidate, String> {
        self.validate()?;
        let freshness = match self.source_snapshot.freshness {
            Freshness::Current => "current",
            Freshness::Stale => "stale",
            Freshness::Unknown => "unknown",
        };
        let evidence = match self.source_snapshot.evidence {
            EvidenceStatus::Verified => "verified",
            EvidenceStatus::Attributed => "attributed",
            EvidenceStatus::Unverifiable => "unverifiable",
            EvidenceStatus::Missing => "missing",
        };
        let material_type = match self.source_kind {
            RetrievalSourceKind::Memory => ContextMaterialType::Memory,
            RetrievalSourceKind::RepoMap => ContextMaterialType::RepoMap,
            RetrievalSourceKind::CodeSearch => ContextMaterialType::LiveResult,
            RetrievalSourceKind::Artifact => ContextMaterialType::Packet,
        };
        Ok(ContextCandidate {
            name: format!("retrieval:{}", self.candidate_id),
            text: format!(
                "[freshness={freshness}; evidence={evidence}]\n{}",
                self.text
            ),
            source: self.source_snapshot.source.clone(),
            authority: PromptAuthority::Context,
            material_type,
            permission_scope: self.permission_scope.clone(),
            revision: self.source_snapshot.source.revision.clone(),
            priority: 1_000u32.saturating_sub(u32::from(self.relevance_milli)),
        })
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "candidate_id": self.candidate_id,
            "source_kind": self.source_kind,
            "material_class": self.material_class,
            "text": self.text,
            "source_snapshot": self.source_snapshot,
            "permission_scope": self.permission_scope,
            "acl_digest": self.acl_digest,
            "relevance_milli": self.relevance_milli,
            "evidence_refs": self.evidence_refs,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalPack {
    pub schema: String,
    pub version: u32,
    pub query_digest: String,
    pub data_epoch: u64,
    pub candidates: Vec<RetrievalCandidate>,
    pub pack_digest: String,
}

impl RetrievalPack {
    pub fn new(
        query_digest: impl Into<String>,
        data_epoch: u64,
        candidates: Vec<RetrievalCandidate>,
    ) -> Result<Self, String> {
        let mut pack = Self {
            schema: RETRIEVAL_PACK_SCHEMA.to_owned(),
            version: RETRIEVAL_VERSION,
            query_digest: query_digest.into(),
            data_epoch,
            candidates,
            pack_digest: String::new(),
        };
        pack.pack_digest = pack.digest();
        pack.validate()?;
        Ok(pack)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_PACK_SCHEMA
            || self.version != RETRIEVAL_VERSION
            || !digest(&self.query_digest)
            || self.data_epoch == 0
            || self.candidates.len() > MAX_RETRIEVAL_CANDIDATES
            || !digest(&self.pack_digest)
            || self.pack_digest != self.digest()
        {
            return Err("retrieval_pack_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for candidate in &self.candidates {
            candidate.validate()?;
            if !ids.insert(candidate.candidate_id.clone()) {
                return Err("retrieval_candidate_duplicate".to_owned());
            }
        }
        Ok(())
    }

    /// Compile Memory/repo/code/artifact candidates through the existing single ContextPlan.
    pub fn compile_context(
        &self,
        bundle: &PromptBundle,
        token_limit: u64,
    ) -> Result<ContextPlan, String> {
        self.validate()?;
        let mut candidates = self
            .candidates
            .iter()
            .map(RetrievalCandidate::as_context_candidate)
            .collect::<Result<Vec<_>, _>>()?;
        candidates.sort_by(|left, right| {
            (&left.name, &left.revision).cmp(&(&right.name, &right.revision))
        });
        ContextPlan::compile(bundle, candidates, token_limit)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "query_digest": self.query_digest,
            "data_epoch": self.data_epoch,
            "candidates": self.candidates,
        }))
    }
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
