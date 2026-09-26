//! Core adapter for Company decision/lesson candidates.
//!
//! Promotion remains a validated fact for the existing memory.review path; this adapter never
//! writes a private collection or changes role policy.

use kiana_domain::{
    CompanyDecisionRecord, CompanyKnowledgeLedger, CompanyKnowledgePromotion,
    CompanyLessonCandidate,
};

pub(crate) fn publish_company_decision(
    ledger: &mut CompanyKnowledgeLedger,
    decision: CompanyDecisionRecord,
) -> Result<(), &'static str> {
    ledger.publish_decision(decision)
}

pub(crate) fn propose_company_candidate(
    ledger: &mut CompanyKnowledgeLedger,
    candidate: CompanyLessonCandidate,
) -> Result<(), &'static str> {
    ledger.propose_candidate(candidate)
}

pub(crate) fn review_company_candidate(
    ledger: &mut CompanyKnowledgeLedger,
    candidate_id: &str,
    approve: bool,
    reviewer: &str,
    reason: &str,
) -> Result<(), &'static str> {
    ledger.review_candidate(candidate_id, approve, reviewer, reason)
}

pub(crate) fn promote_company_candidate(
    ledger: &mut CompanyKnowledgeLedger,
    promotion: CompanyKnowledgePromotion,
) -> Result<(), &'static str> {
    ledger.promote(promotion)
}
