//! Approval lifecycle facts and prepared mutations for the shared authority transaction.
use crate::{
    AggregateVersion, ApprovalChallenge, ApprovalDecision, ApprovalId, ApprovalState,
    PendingApproval, RequestId, RuntimeEvent,
};
use serde::{Deserialize, Serialize};

/// A prepared event is inert until core commits it together with the dispatch permit or pause.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedApprovalConsumption {
    pub expected_version: AggregateVersion,
    pub authority_versions: Vec<AggregateVersion>,
    pub event: RuntimeEvent,
    pub pending: PendingApproval,
}

/// Read projection for decision retries. Approved and Consumed never mean "execute again".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovalDecisionRecord {
    pub approval_id: ApprovalId,
    pub state: ApprovalState,
    pub challenge: ApprovalChallenge,
    pub decision: Option<ApprovalDecision>,
    pub decision_command_id: Option<RequestId>,
    pub decided_by: Option<String>,
    pub dispatch_command_id: Option<RequestId>,
    pub payload_available: bool,
    pub version: u64,
}
