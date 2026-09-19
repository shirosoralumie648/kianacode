//! Adapter helpers that normalize query/index results into the shared domain retrieval contract.
//!
//! These helpers do not read memory or files themselves and do not authorize a read.  They only
//! attach source/ACL/freshness metadata before a caller sends candidates to ContextPlan.

use crate::repo_map::RepoMapFile;
use kiana_domain::{
    json_digest, ContextMaterialClass, EvidenceStatus, Freshness, RetrievalCandidate,
    RetrievalSourceKind, SourceKind, SourceRef, SourceSnapshot,
};
use serde_json::json;

pub fn repo_map_candidate(
    root: &str,
    file: &RepoMapFile,
    permission_scope: &str,
    data_epoch: u64,
) -> Result<RetrievalCandidate, String> {
    let content_digest = if file.content_hash.starts_with("sha256:") {
        file.content_hash.clone()
    } else {
        json_digest(&json!({"path":file.path,"symbols":file.symbols}))
    };
    let source = SourceRef::new(
        format!("repo-map:{}", file.path),
        SourceKind::WorkspaceFile,
        format!("{root}/{}", file.path),
        content_digest.clone(),
        content_digest,
        None,
        EvidenceStatus::Attributed,
    )?;
    let snapshot =
        SourceSnapshot::new(source, Freshness::Current, EvidenceStatus::Attributed, None)?;
    RetrievalCandidate::new(
        format!("repo-map:{}", file.path),
        RetrievalSourceKind::RepoMap,
        ContextMaterialClass::ProjectKnowledge,
        format!(
            "{} ({:?}, {} bytes): {}",
            file.path,
            file.language,
            file.bytes,
            file.symbols.join(", ")
        ),
        snapshot,
        permission_scope,
        json_digest(&json!({"scope": permission_scope, "data_epoch": data_epoch})),
        500,
        vec![format!("source:repo-map:{}", file.path)],
    )
}
