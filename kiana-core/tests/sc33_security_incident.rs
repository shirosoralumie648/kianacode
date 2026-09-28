//! SC-33 failure-first fixtures for the security Incident / Vulnerability / Reconcile workflow.
//!
//! Every test before the last one names one item from the card's rejected-first column: an
//! unknown, a secret leak or a supply-chain drift that has no accountable incident; and an incident
//! that cannot be traced after it is closed. Nothing here opens a real incident, fences a real
//! lease or talks to a reconciler: the module under test is a read-only decision over supplied
//! facts, so these are contract fixtures and not runtime evidence.
//!
//! The order of the file is the order of the argument. The four card fields -- `severity`,
//! `owner`, `deadline`, `evidence` -- are each shown to be mandatory, the four actions
//! (`contain` -> `fence` -> `reconcile` -> `close`) are each shown to be unskippable and
//! irreversible, and only then does a full lifecycle run to `Closed`.

use kiana_core::{
    SecurityIncident, SecurityIncidentAction, SecurityIncidentActionRequest, SecurityIncidentClass,
    SecurityIncidentClosure, SecurityIncidentEvidence, SecurityIncidentPhase,
    SecurityIncidentReport, SecurityIncidentReportStatus, SecurityIncidentSeverity,
    SecurityIncidentState, SecurityVulnerability,
};
use kiana_domain::json_digest;
use serde_json::json;

const OWNER: &str = "security-owner";
const REVIEWER: &str = "security-reviewer";
/// The clock the fixtures are written against. Nothing in this file reads a wall clock.
const NOW: u64 = 1_700_000_000_000;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn evidence(seed: char) -> SecurityIncidentEvidence {
    SecurityIncidentEvidence::new(format!("event:{seed}"), digest(seed), 42)
        .expect("SC-33 evidence")
}

/// A `SecretLeak` incident with one committed fact behind it.
fn incident() -> SecurityIncident {
    SecurityIncident::open(
        "inc-1",
        SecurityIncidentClass::SecretLeak,
        SecurityIncidentSeverity::High,
        OWNER,
        NOW + 86_400_000,
        NOW,
        None,
        vec![evidence('a')],
    )
    .expect("SC-33 incident")
}

fn opened_state() -> SecurityIncidentState {
    SecurityIncidentState::opened("inc-1").expect("SC-33 opened state")
}

fn request(
    action: SecurityIncidentAction,
    cited: Vec<SecurityIncidentEvidence>,
) -> SecurityIncidentActionRequest {
    SecurityIncidentActionRequest::new(
        "inc-1",
        action,
        OWNER,
        NOW + 1_000,
        cited,
        match action {
            SecurityIncidentAction::Close => Some(closure(REVIEWER)),
            _ => None,
        },
    )
    .expect("SC-33 action request")
}

fn closure(reviewer: &str) -> SecurityIncidentClosure {
    SecurityIncidentClosure::new(
        "provider retried a timed-out call without an idempotency ref",
        "one external call, one duplicate charge",
        vec!["revoked the provider token".to_owned()],
        "duplicate external effect may still be visible to the provider",
        reviewer,
    )
    .expect("SC-33 closure")
}

// ---------------------------------------------------------------------------
// 1. The four card fields are mandatory. An incident missing any one of them is
//    unaccountable, not degraded, so each is refused on its own.
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_outcome_cannot_be_opened_without_an_owner() {
    let error = SecurityIncident::open(
        "inc-no-owner",
        SecurityIncidentClass::UnknownOutcome,
        SecurityIncidentSeverity::High,
        "   ",
        NOW + 86_400_000,
        NOW,
        None,
        vec![evidence('a')],
    )
    .expect_err("SC-33 a nameless owner must not open an incident");
    assert_eq!(error, "security_incident_owner_required");
}

#[test]
fn a_malformed_owner_is_rejected_as_malformed_not_as_missing() {
    // A blank owner and an unusable owner are different problems and must not share a code, or a
    // reader cannot tell "nobody was assigned" from "the assignment field is broken".
    let error = SecurityIncident::open(
        "inc-bad-owner",
        SecurityIncidentClass::UnknownOutcome,
        SecurityIncidentSeverity::High,
        "o".repeat(300),
        NOW + 86_400_000,
        NOW,
        None,
        vec![evidence('a')],
    )
    .expect_err("SC-33 an over-long owner must not open an incident");
    assert_eq!(error, "security_incident_owner_invalid");
}

#[test]
fn an_incident_without_a_deadline_cannot_be_admitted() {
    let error = SecurityIncident::open(
        "inc-no-deadline",
        SecurityIncidentClass::UnknownOutcome,
        SecurityIncidentSeverity::High,
        OWNER,
        0,
        NOW,
        None,
        vec![evidence('a')],
    )
    .expect_err("SC-33 a zero deadline must not open an incident");
    assert_eq!(error, "security_incident_deadline_required");
}

#[test]
fn an_already_expired_deadline_cannot_be_admitted() {
    // The deadline is behind the moment the incident was opened. An incident nobody is racing is
    // not a triaged incident, it is an unowned one wearing a severity label.
    let error = SecurityIncident::open(
        "inc-expired",
        SecurityIncidentClass::SecretLeak,
        SecurityIncidentSeverity::High,
        OWNER,
        NOW - 1,
        NOW,
        None,
        vec![evidence('a')],
    )
    .expect_err("SC-33 an expired deadline must not open an incident");
    assert_eq!(error, "security_incident_deadline_expired");
}

#[test]
fn an_incident_without_evidence_cannot_be_admitted() {
    let error = SecurityIncident::open(
        "inc-no-evidence",
        SecurityIncidentClass::SecretLeak,
        SecurityIncidentSeverity::High,
        OWNER,
        NOW + 86_400_000,
        NOW,
        None,
        Vec::new(),
    )
    .expect_err("SC-33 an incident with no committed fact must not open");
    assert_eq!(error, "security_incident_evidence_required");
}

#[test]
fn an_incident_cannot_cite_evidence_at_a_position_that_does_not_exist() {
    let error = SecurityIncidentEvidence::new("event:ghost", digest('a'), 0)
        .expect_err("SC-33 source cursor 0 is not a committed position");
    assert_eq!(error, "security_incident_evidence_header_invalid");
}

#[test]
fn a_supply_chain_drift_without_a_vulnerability_cannot_be_admitted() {
    // The card pairs "supply chain drift" with a vulnerability. Without one there is nothing to
    // compare provenance against, so the class is refused rather than quietly downgraded.
    let error = SecurityIncident::open(
        "inc-drift",
        SecurityIncidentClass::SupplyChainDrift,
        SecurityIncidentSeverity::High,
        OWNER,
        NOW + 86_400_000,
        NOW,
        None,
        vec![evidence('a')],
    )
    .expect_err("SC-33 a drift incident needs a vulnerability");
    assert_eq!(error, "security_incident_vulnerability_required");
}

#[test]
fn a_withdrawn_vulnerability_cannot_back_a_low_severity_drift() {
    // A withdrawn advisory still raises an incident -- that is normal -- but it cannot be filed as
    // `low`, because the severity floor is the whole reason the withdrawal is recorded.
    let withdrawn = SecurityVulnerability::new(
        "CVE-2026-0001",
        SecurityIncidentSeverity::Medium,
        true,
        "upstream fix landed in 1.4.2",
    )
    .expect("SC-33 withdrawn vulnerability");
    let error = SecurityIncident::open(
        "inc-drift-low",
        SecurityIncidentClass::SupplyChainDrift,
        SecurityIncidentSeverity::Low,
        OWNER,
        NOW + 86_400_000,
        NOW,
        Some(withdrawn),
        vec![evidence('a')],
    )
    .expect_err("SC-33 a withdrawn advisory cannot back a low severity");
    assert_eq!(
        error,
        "security_incident_severity_below_vulnerability_floor"
    );
}

// ---------------------------------------------------------------------------
// 2. Tampered records cannot publish themselves. A digest seal is only evidence
//    if something refuses a record whose seal does not match its contents.
// ---------------------------------------------------------------------------

#[test]
fn a_tampered_incident_record_cannot_publish_itself() {
    let mut tampered = incident();
    // Downgrade a High incident to Low and re-seal nothing.
    tampered.severity = SecurityIncidentSeverity::Low;
    let error = tampered
        .validate()
        .expect_err("SC-33 an edited severity must not pass its own seal");
    assert_eq!(error, "security_incident_digest_mismatch");
}

#[test]
fn a_tampered_incident_report_cannot_publish_itself() {
    let incident = incident();
    let state = opened_state();
    let action = request(SecurityIncidentAction::Contain, vec![evidence('a')]);
    let report = incident
        .evaluate(&state, &action)
        .expect("SC-33 contain report");

    // A report that claims it closed the incident when it only contained it.
    let mut forged = report.clone();
    forged.status = SecurityIncidentReportStatus::Admitted;
    forged.state_after = SecurityIncidentPhase::Closed;
    forged.reason = String::new();
    let error = forged
        .validate_against(&incident, &state, &action)
        .expect_err("SC-33 a forged close must not validate");
    assert_eq!(error, "security_incident_report_binding_invalid");
}

#[test]
fn a_report_digest_that_does_not_cover_its_own_contents_is_refused() {
    let incident = incident();
    let state = opened_state();
    let action = request(SecurityIncidentAction::Contain, vec![evidence('a')]);
    let mut report = incident
        .evaluate(&state, &action)
        .expect("SC-33 contain report");
    // Keep every derived field intact but break the seal: this is the "hand-edited enums" shape.
    report.status = SecurityIncidentReportStatus::Rejected;
    let error = report
        .validate_against(&incident, &state, &action)
        .expect_err("SC-33 an unresigned report must be refused");
    assert_eq!(error, "security_incident_report_binding_invalid");
}

#[test]
fn a_rejection_always_carries_a_reason_and_cannot_be_silenced() {
    let incident = incident();
    let state = opened_state();
    // Try to close straight from `Open`: the ordering rule refuses it with a reason.
    let action = request(SecurityIncidentAction::Close, vec![evidence('a')]);
    let report = incident
        .evaluate(&state, &action)
        .expect("SC-33 out-of-order close still produces a decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Rejected);
    // The invariant under test: a rejection names the rule that fired and offers a way forward.
    assert_eq!(report.reason, "security_incident_reconcile_required");
    assert!(report.remediation.contains("reconcile"));

    // Strip the reason and the report no longer re-derives: a reasonless rejection is refused
    // rather than accepted as an unexplained "no".
    let mut hidden = report.clone();
    hidden.reason = String::new();
    let error = hidden
        .validate_against(&incident, &state, &action)
        .expect_err("SC-33 a reasonless rejection must be refused");
    assert_eq!(error, "security_incident_report_binding_invalid");
}

// ---------------------------------------------------------------------------
// 3. Evidence is single-use. Re-citing a spent fact is how one timeout ends up
//    justifying a contain, a reconcile and a close.
// ---------------------------------------------------------------------------

#[test]
fn evidence_already_consumed_by_an_earlier_action_cannot_be_cited_again() {
    let incident = incident();
    let contained = SecurityIncidentState::new(
        "inc-1",
        SecurityIncidentPhase::Contained,
        vec![SecurityIncidentAction::Contain],
        std::collections::BTreeSet::from([evidence('a').evidence_digest.clone()]),
        OWNER,
        NOW + 1_000,
    )
    .expect("SC-33 contained state");
    // Same fact, second action.
    let action = request(SecurityIncidentAction::Fence, vec![evidence('a')]);
    let report = incident
        .evaluate(&contained, &action)
        .expect("SC-33 replay decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Rejected);
    assert_eq!(report.reason, "security_incident_evidence_replay");
    assert_eq!(report.state_after, SecurityIncidentPhase::Contained);
}

#[test]
fn evidence_from_another_incident_cannot_be_spliced_in() {
    let incident = incident();
    let state = opened_state();
    // `evidence('b')` is a perfectly valid fact -- it just is not this incident's fact.
    let action = request(SecurityIncidentAction::Contain, vec![evidence('b')]);
    let report = incident
        .evaluate(&state, &action)
        .expect("SC-33 foreign evidence decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Rejected);
    assert_eq!(report.reason, "security_incident_evidence_not_in_incident");
}

#[test]
fn an_action_that_cites_nothing_proves_nothing() {
    let incident = incident();
    let state = opened_state();
    let action = request(SecurityIncidentAction::Contain, Vec::new());
    let report = incident
        .evaluate(&state, &action)
        .expect("SC-33 evidence-free decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Rejected);
    assert_eq!(report.reason, "security_incident_evidence_required");
}

// ---------------------------------------------------------------------------
// 4. The ordering is load-bearing. contain -> fence -> reconcile -> close, with
//    no way to skip a step or walk one back.
// ---------------------------------------------------------------------------

#[test]
fn close_without_a_reconcile_is_refused_so_an_unknown_cannot_become_a_success() {
    let incident = incident();
    let contained = SecurityIncidentState::new(
        "inc-1",
        SecurityIncidentPhase::Contained,
        vec![SecurityIncidentAction::Contain],
        Default::default(),
        OWNER,
        NOW + 1_000,
    )
    .expect("SC-33 contained state");
    // Contained, never reconciled. Closing here would declare an unknown resolved.
    let action = request(SecurityIncidentAction::Close, vec![evidence('a')]);
    let report = incident
        .evaluate(&contained, &action)
        .expect("SC-33 unreconciled close decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Rejected);
    assert_eq!(report.reason, "security_incident_reconcile_required");
    assert_eq!(report.state_after, SecurityIncidentPhase::Contained);
}

#[test]
fn a_step_cannot_be_skipped_in_either_direction() {
    let incident = incident();
    // `fence` from `Open` skips `contain` entirely.
    let open = opened_state();
    let skip = request(SecurityIncidentAction::Fence, vec![evidence('a')]);
    let report = incident
        .evaluate(&open, &skip)
        .expect("SC-33 skip decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Rejected);
    assert_eq!(report.reason, "security_incident_action_out_of_order");

    // `reconcile` from `Contained` skips `fence`: the old capability may still act, so what the
    // outside world did is not yet knowable.
    let contained = SecurityIncidentState::new(
        "inc-1",
        SecurityIncidentPhase::Contained,
        vec![SecurityIncidentAction::Contain],
        Default::default(),
        OWNER,
        NOW + 1_000,
    )
    .expect("SC-33 contained state");
    let skip = request(SecurityIncidentAction::Reconcile, vec![evidence('a')]);
    let report = incident
        .evaluate(&contained, &skip)
        .expect("SC-33 skip decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Rejected);
    assert_eq!(report.reason, "security_incident_action_out_of_order");
}

#[test]
fn an_action_performed_after_the_deadline_is_refused() {
    let incident = incident();
    let state = opened_state();
    // Same action, same evidence -- only the clock moved past the deadline.
    let late = SecurityIncidentActionRequest::new(
        "inc-1",
        SecurityIncidentAction::Contain,
        OWNER,
        NOW + 86_400_001,
        vec![evidence('a')],
        None,
    )
    .expect("SC-33 late action");
    let report = incident
        .evaluate(&state, &late)
        .expect("SC-33 late decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Rejected);
    assert_eq!(report.reason, "security_incident_deadline_exceeded");
}

#[test]
fn an_anonymous_action_is_not_a_transition() {
    let error = SecurityIncidentActionRequest::new(
        "inc-1",
        SecurityIncidentAction::Contain,
        "",
        NOW + 1_000,
        vec![evidence('a')],
        None,
    )
    .expect_err("SC-33 an anonymous action must be refused");
    assert_eq!(error, "security_incident_action_actor_required");
}

#[test]
fn a_closed_incident_cannot_be_reopened_or_advanced_further() {
    let incident = incident();
    // A fully closed incident. `applied` is the complete canonical prefix and the consumed set is
    // exactly the evidence this incident actually carries, so the state is one this workflow could
    // really have produced rather than a forgery.
    let closed = SecurityIncidentState::new(
        "inc-1",
        SecurityIncidentPhase::Closed,
        SecurityIncidentAction::ALL.to_vec(),
        [evidence('a').evidence_digest.clone()]
            .into_iter()
            .collect(),
        REVIEWER,
        NOW + 4_000,
    )
    .expect("SC-33 closed state");
    closed
        .validate_against(&incident)
        .expect("SC-33 a genuinely closed state must validate");
    // Any further action is refused, and the refusal names irreversibility rather than ordering.
    // The citation is deliberately evidence this incident does not carry: the terminal check runs
    // first, so the reason must still be irreversibility and never a complaint about the evidence.
    for action_kind in SecurityIncidentAction::ALL {
        let action = request(action_kind, vec![evidence('b')]);
        let report = incident
            .evaluate(&closed, &action)
            .expect("SC-33 post-close decision");
        assert_eq!(
            report.status,
            SecurityIncidentReportStatus::Rejected,
            "{action_kind:?} must not be admissible after close"
        );
        assert_eq!(report.reason, "security_incident_state_irreversible");
        assert_eq!(report.state_after, SecurityIncidentPhase::Closed);
    }
}

#[test]
fn a_state_that_skips_a_step_cannot_be_presented_as_one_this_workflow_produced() {
    // `applied` claims `contain` then jumps to `close`.
    let state = SecurityIncidentState::new(
        "inc-1",
        SecurityIncidentPhase::Closed,
        vec![
            SecurityIncidentAction::Contain,
            SecurityIncidentAction::Close,
        ],
        Default::default(),
        REVIEWER,
        NOW + 4_000,
    )
    .expect("SC-33 forged state");
    let error = state
        .validate_against(&incident())
        .expect_err("SC-33 a skipped state must not validate");
    assert_eq!(error, "security_incident_state_order_invalid");
}

#[test]
fn a_state_claiming_more_progress_than_its_actions_support_is_refused() {
    // Actions say `contained`; the state claims `reconciled`.
    let state = SecurityIncidentState::new(
        "inc-1",
        SecurityIncidentPhase::Reconciled,
        vec![SecurityIncidentAction::Contain],
        Default::default(),
        OWNER,
        NOW + 1_000,
    )
    .expect("SC-33 over-claiming state");
    let error = state
        .validate_against(&incident())
        .expect_err("SC-33 an over-claiming state must not validate");
    assert_eq!(error, "security_incident_state_not_derived");
}

// ---------------------------------------------------------------------------
// 5. Closure is a signed claim. "Handled" is not an answer, and the person who
//    ran the response cannot be the person who signs it off.
// ---------------------------------------------------------------------------

#[test]
fn a_closure_without_containment_actions_is_not_a_closure() {
    let error = SecurityIncidentClosure::new(
        "provider retried without an idempotency ref",
        "one external call",
        Vec::new(),
        "duplicate effect may persist",
        REVIEWER,
    )
    .expect_err("SC-33 an empty containment list must be refused");
    assert_eq!(error, "security_incident_closure_containment_required");
}

#[test]
fn a_closure_without_a_reviewer_is_not_a_closure() {
    let error = SecurityIncidentClosure::new(
        "provider retried without an idempotency ref",
        "one external call",
        vec!["revoked the provider token".to_owned()],
        "duplicate effect may persist",
        "  ",
    )
    .expect_err("SC-33 an unreviewed closure must be refused");
    assert_eq!(error, "security_incident_closure_reviewer_required");
}

#[test]
fn a_malformed_reviewer_is_rejected_as_malformed_not_as_missing() {
    let error = SecurityIncidentClosure::new(
        "provider retried without an idempotency ref",
        "one external call",
        vec!["revoked the provider token".to_owned()],
        "duplicate effect may persist",
        "r".repeat(300),
    )
    .expect_err("SC-33 an over-long reviewer must be refused");
    assert_eq!(error, "security_incident_closure_reviewer_invalid");
}

#[test]
fn the_incident_owner_cannot_be_the_closure_reviewer() {
    // An incident with one distinct fact per step, so the close below is refused for the
    // reviewer reason and not for borrowing somebody else's evidence.
    let incident = SecurityIncident::open(
        "inc-self-review",
        SecurityIncidentClass::SecretLeak,
        SecurityIncidentSeverity::High,
        OWNER,
        NOW + 86_400_000,
        NOW,
        None,
        vec![evidence('a'), evidence('b'), evidence('c'), evidence('d')],
    )
    .expect("SC-33 self-review incident");
    let reconciled = SecurityIncidentState::new(
        "inc-self-review",
        SecurityIncidentPhase::Reconciled,
        vec![
            SecurityIncidentAction::Contain,
            SecurityIncidentAction::Fence,
            SecurityIncidentAction::Reconcile,
        ],
        [
            evidence('a').evidence_digest.clone(),
            evidence('b').evidence_digest.clone(),
            evidence('c').evidence_digest.clone(),
        ]
        .into_iter()
        .collect(),
        OWNER,
        NOW + 3_000,
    )
    .expect("SC-33 reconciled state");

    // The owner signs off on their own response.
    let conflicted = SecurityIncidentActionRequest::new(
        "inc-self-review",
        SecurityIncidentAction::Close,
        OWNER,
        NOW + 4_000,
        vec![evidence('d')],
        Some(closure(OWNER)),
    )
    .expect("SC-33 self-review action");
    let report = incident
        .evaluate(&reconciled, &conflicted)
        .expect("SC-33 self-review decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Rejected);
    assert_eq!(report.reason, "security_incident_closure_reviewer_conflict");

    // A different reviewer on the same fact is admitted, which shows the refusal above was
    // about the reviewer and not about the close itself.
    let independent = SecurityIncidentActionRequest::new(
        "inc-self-review",
        SecurityIncidentAction::Close,
        REVIEWER,
        NOW + 4_000,
        vec![evidence('d')],
        Some(closure(REVIEWER)),
    )
    .expect("SC-33 independent-review action");
    let report = incident
        .evaluate(&reconciled, &independent)
        .expect("SC-33 independent-review decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Admitted);
    assert_eq!(report.state_after, SecurityIncidentPhase::Closed);
}

#[test]
fn a_containment_action_cannot_smuggle_in_a_closure() {
    let error = SecurityIncidentActionRequest::new(
        "inc-1",
        SecurityIncidentAction::Contain,
        OWNER,
        NOW + 1_000,
        vec![evidence('a')],
        Some(closure(REVIEWER)),
    )
    .expect_err("SC-33 closure travels with close only");
    assert_eq!(error, "security_incident_closure_not_allowed");
}

#[test]
fn a_close_without_a_closure_is_refused() {
    let error = SecurityIncidentActionRequest::new(
        "inc-1",
        SecurityIncidentAction::Close,
        OWNER,
        NOW + 1_000,
        vec![evidence('a')],
        None,
    )
    .expect_err("SC-33 close must carry a closure");
    assert_eq!(error, "security_incident_closure_required");
}

// ---------------------------------------------------------------------------
// 6. The happy path, last: a full lifecycle that reaches `Closed`, proves each
//    transition, and leaves the underlying facts intact.
// ---------------------------------------------------------------------------

#[test]
fn a_fully_evidenced_incident_runs_contain_fence_reconcile_close_to_closed() {
    let incident = SecurityIncident::open(
        "inc-lifecycle",
        SecurityIncidentClass::UnknownOutcome,
        SecurityIncidentSeverity::Critical,
        OWNER,
        NOW + 86_400_000,
        NOW,
        None,
        vec![evidence('a'), evidence('b'), evidence('c'), evidence('d')],
    )
    .expect("SC-33 lifecycle incident");

    // Step 1: contain.
    let mut state = SecurityIncidentState::opened("inc-lifecycle").expect("SC-33 open");
    let mut consumed = std::collections::BTreeSet::new();
    for (index, (action_kind, evidence_seed)) in [
        (SecurityIncidentAction::Contain, 'a'),
        (SecurityIncidentAction::Fence, 'b'),
        (SecurityIncidentAction::Reconcile, 'c'),
        (SecurityIncidentAction::Close, 'd'),
    ]
    .into_iter()
    .enumerate()
    {
        let closure = if action_kind == SecurityIncidentAction::Close {
            Some(closure(REVIEWER))
        } else {
            None
        };
        let action = SecurityIncidentActionRequest::new(
            "inc-lifecycle",
            action_kind,
            if action_kind == SecurityIncidentAction::Close {
                REVIEWER
            } else {
                OWNER
            },
            NOW + 1_000 + index as u64,
            vec![evidence(evidence_seed)],
            closure,
        )
        .expect("SC-33 lifecycle action");

        let report = incident
            .evaluate(&state, &action)
            .expect("SC-33 lifecycle decision");
        assert_eq!(
            report.status,
            SecurityIncidentReportStatus::Admitted,
            "{action_kind:?} must be admissible: {}",
            report.reason
        );
        assert!(report.admitted());
        assert!(report.reason.is_empty());
        assert!(report.state_after > report.state_before);

        consumed.insert(evidence(evidence_seed).evidence_digest.clone());
        state = SecurityIncidentState::new(
            "inc-lifecycle",
            report.state_after,
            {
                let mut applied = state.applied.clone();
                applied.push(action_kind);
                applied
            },
            consumed.clone(),
            if action_kind == SecurityIncidentAction::Close {
                REVIEWER
            } else {
                OWNER
            },
            NOW + 1_000 + index as u64,
        )
        .expect("SC-33 lifecycle state");
    }

    // Terminal, with every cited fact still accounted for and nothing deleted.
    assert_eq!(state.state, SecurityIncidentPhase::Closed);
    assert_eq!(state.applied, SecurityIncidentAction::ALL.to_vec());
    assert_eq!(state.consumed_evidence.len(), 4);
    state
        .validate_against(&incident)
        .expect("SC-33 closed state must validate");
    // The incident record is untouched by the lifecycle: closing retired the response, not the facts.
    incident.validate().expect("SC-33 incident survives close");

    // A re-delivered `close` does not close twice, and does not get to cite fresh evidence to
    // manufacture a reason to reopen: the terminal state is checked before anything else.
    let redelivered = SecurityIncidentActionRequest::new(
        "inc-lifecycle",
        SecurityIncidentAction::Close,
        REVIEWER,
        NOW + 9_000,
        vec![SecurityIncidentEvidence::new("event:late", digest('z'), 99)
            .expect("SC-33 new committed fact")],
        Some(closure(REVIEWER)),
    )
    .expect("SC-33 re-delivered action");
    let report = incident
        .evaluate(&state, &redelivered)
        .expect("SC-33 re-delivery decision");
    assert_eq!(report.status, SecurityIncidentReportStatus::Rejected);
    assert_eq!(report.reason, "security_incident_state_irreversible");
}

#[test]
fn a_report_and_its_derivation_agree_after_a_successful_decision() {
    // Proves `validate_against` is a real re-derivation rather than a rubber stamp: the untouched
    // report validates, and any single mutated field is caught.
    let incident = incident();
    let state = opened_state();
    let action = request(SecurityIncidentAction::Contain, vec![evidence('a')]);
    let report: SecurityIncidentReport = incident
        .evaluate(&state, &action)
        .expect("SC-33 contain report");
    report
        .validate_against(&incident, &state, &action)
        .expect("SC-33 untouched report must validate");
    assert_eq!(
        report.digest(),
        json_digest(&json!({
            "schema": report.schema,
            "version": report.version,
            "incident_id": report.incident_id,
            "action": report.action,
            "status": report.status,
            "state_before": report.state_before,
            "state_after": report.state_after,
            "reason": report.reason,
            "remediation": report.remediation,
            "incident_digest": report.incident_digest,
            "action_digest": report.action_digest,
        })),
        "the report digest must cover exactly the published fields"
    );
}
