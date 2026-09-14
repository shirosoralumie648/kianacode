//! Immutable local execution permits. The journal, rather than this DTO, grants authority.
use crate::{
    AggregateVersion, ApprovalId, ExecutionId, InvocationId, RequestContext, RequestId, RunId,
    TurnId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DISPATCH_PERMIT_SCHEMA: &str = "kiana.dispatch-permit.v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchPermit {
    pub schema: String,
    pub execution_id: ExecutionId,
    pub invocation_id: InvocationId,
    pub request_id: RequestId,
    pub run_id: Option<RunId>,
    pub turn_id: Option<TurnId>,
    pub decision_id: String,
    pub approval_id: Option<ApprovalId>,
    pub context: RequestContext,
    pub action_digest: String,
    pub project_identity: serde_json::Value,
    pub authority_versions: Vec<AggregateVersion>,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

/// Purpose-separated stable command IDs make network retries and recovery unambiguous.
pub fn derived_request_id(purpose: &str, subject: &str) -> RequestId {
    let mut hash = Sha256::new();
    hash.update(purpose.as_bytes());
    hash.update([0]);
    hash.update(subject.as_bytes());
    let digest = hash.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    RequestId::from_uuid(uuid::Uuid::from_bytes(bytes))
}
